'use strict';

/**
 * Multi-contributor Ambient Weather → WRFCoin ingestion (closes core4#1732).
 *
 * The single-account collector (`fetch-ambient-weather.cjs`) submits every
 * reading under one hardcoded contributor/station identity. This variant
 * fetches the full contributor roster from the backend, then — per contributor —
 * fetches THEIR Ambient devices with THEIR keys, normalizes, and submits under
 * THAT contributor's ownerAddress + stationId.
 *
 * Backend contract (wrfcoin/backend — see PR body):
 *   GET http://${BACKEND_HOST:-wrfcoin-backend}:${BACKEND_PORT:-4001}/api/weather/stations/credentials
 *   Header: Authorization: Bearer ${WRFCOIN_OPERATOR_TOKEN}
 *   Response (200): [
 *     { stationId, ownerAddress, provider:'ambient', apiKey, appKey, latitude, longitude,
 *       mac, model, consentStatus, coarseMetadataStatus, bindingRef, elevation_m?, name? }
 *   ]
 *   API keys are a runtime-only provider boundary. The strict roster projection
 *   below carries only an opaque provider binding reference and never logs or validates key values.
 *
 * Fallback: if the credentials endpoint is unreachable, returns empty/non-array,
 * or MULTI_TENANT_DISABLED=1 is set, this script falls back to the original
 * single-account env behavior so nothing breaks before the backend ships.
 *
 * Per-contributor error isolation: one bad key / missing device / failed submit
 * does not abort the others.
 *
 * Environment variables:
 *   WRFCOIN_OPERATOR_TOKEN    — testnet operator token (node submit + backend auth)
 *   BACKEND_HOST              — backend host (default wrfcoin-backend)
 *   BACKEND_PORT              — backend port (default 4001)
 *   WRFCOIN_API_HOST          — node host (default 127.0.0.1)
 *   WRFCOIN_API_PORT          — node port (default 8556)
 *   WRFCOIN_MINER_ADDRESS     — this host's authorized producer payout address
 *   MULTI_TENANT_DISABLED     — set to "1" to force single-account fallback
 *   AMBIENT_READINGS_LIMIT    — readings per station (default 12)
 *   NWS_STATION_ID            — NWS station for fallback single-account comparison
 *
 *   Single-account fallback also reads (same as fetch-ambient-weather.cjs):
 *     AMBIENT_WEATHER_APP_KEY, AMBIENT_WEATHER_API_KEY, JONS_CONTRIBUTOR, ...
 *
 * Usage:
 *   node tests/security/fetch-ambient-weather-multi.cjs
 *   node tests/security/fetch-ambient-weather-multi.cjs --dry-run
 *   node tests/security/fetch-ambient-weather-multi.cjs --dry-run \
 *       --credentials-file tests/security/fixtures/multi-tenant-credentials.json
 *
 * Flags:
 *   --dry-run               Fetch + normalize only; never submit to the node
 *   --max-age-minutes <N>   Staleness threshold in minutes (default 30)
 *   --credentials-file <p>  Read the contributor roster from a local JSON fixture
 *                           instead of the backend (implies offline credential source;
 *                           still hits live Ambient unless combined with --dry-run on
 *                           a fixture that has no real keys).
 */

const http = require('http');
const path = require('path');
const fs = require('fs');
const crypto = require('crypto');

const {
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');
const {
    redactSecrets,
    emitFailureMetric,
    logStructured,
    fetchNwsObservations,
    getPendingRevealsPath,
    fetchStationReadings,
    fetchRegisteredElevation,
    submitStationReadings,
    isValidMinerAddress,
    COLLECTOR_CYCLE_OUTCOMES,
} = require('./lib/ambient-pipeline.cjs');
const {
    validateStationRoster,
} = require('./lib/ambient-roster-verifier.cjs');
// Dashboard-visible per-station output files (infra#897). Same module the
// single-account collector uses for its own ambient-weather-jon-accuracy.json
// — reused here, not reimplemented, so the two collectors can never drift on
// what "accuracy" means.
const {
    buildAccuracyReport,
} = require('./ambient-weather-comparison.cjs');

// ---------------------------------------------------------------------------
// .env.local loader (no deps)
// ---------------------------------------------------------------------------
function loadEnvFile(filePath) {
    try {
        const content = fs.readFileSync(filePath, 'utf8');
        for (const line of content.split('\n')) {
            const trimmed = line.trim();
            if (!trimmed || trimmed.startsWith('#')) continue;
            const eqIdx = trimmed.indexOf('=');
            if (eqIdx === -1) continue;
            const key = trimmed.slice(0, eqIdx).trim();
            const val = trimmed.slice(eqIdx + 1).trim();
            if (val && !process.env[key]) process.env[key] = val;
        }
    } catch (_) { /* optional */ }
}
loadEnvFile(path.join(__dirname, '../../.env.local'));
loadEnvFile(path.join(process.cwd(), '.env.local'));

function getDefaultCliArgv() {
    return require.main === module ? process.argv.slice(2) : [];
}

const config = loadTestConfig({
    argv: getDefaultCliArgv(),
    scriptName: 'tests/security/fetch-ambient-weather-multi.cjs',
    description: 'Multi-contributor Ambient Weather ingestion: fetch roster from backend, submit per owner.',
    requireToken: false,
    extraOptions: [
        { name: 'dry-run', type: 'boolean', description: 'Fetch and normalize only, do not submit' },
        { name: 'skip-nws', type: 'boolean', description: 'Skip NWS comparison (propagated to single-account fallback)' },
        { name: 'max-age-minutes', type: 'int', default: 30, description: 'Max accepted data age (minutes) before skipping submission' },
        { name: 'credentials-file', type: 'string', description: 'Local JSON roster fixture instead of backend' },
    ],
});

const WRFCOIN_HOST = config.apiHost;
const WRFCOIN_PORT = config.apiPort;
const WRFCOIN_TOKEN = config.operatorToken;
const READINGS_LIMIT = parseInt(process.env.AMBIENT_READINGS_LIMIT || '12', 10);
const DRY_RUN = Boolean(config.values['dry-run']);
const MAX_AGE_MINUTES = config.values['max-age-minutes'];
const CREDENTIALS_FILE = config.values['credentials-file'] || null;

const BACKEND_HOST = process.env.BACKEND_HOST || 'wrfcoin-backend';
const BACKEND_PORT = parseInt(process.env.BACKEND_PORT || '4001', 10);
const CREDENTIALS_PATH = '/api/weather/stations/credentials';
const MULTI_TENANT_DISABLED = process.env.MULTI_TENANT_DISABLED === '1';

const NWS_STATION_ID = process.env.NWS_STATION_ID || 'KDLH';
// Same value fetch-ambient-weather.cjs uses for its own accuracy report.
const NWS_MAX_PAIR_DELTA_SECONDS = 300; // 5 minutes

// Dashboard-visible output directory (infra#897). Same path
// fetch-ambient-weather.cjs resolves to for ambient-weather-jon-*.json:
// __dirname is the read-only-mounted tests/security, so ../../data is
// /app/data inside the collector container. Ephemeral (not a docker
// volume) on both collectors today — a pre-existing limitation, not a
// regression introduced here.
const OUTPUT_DIR = path.join(__dirname, '../../data');

/**
 * Derive a stable, filesystem-safe per-station key from a roster stationId,
 * for ambient-weather-<key>-latest.json / -accuracy.json. The dashboard's
 * discovery glob (infra#895) only cares about the substring between
 * `ambient-weather-` and `-latest.json` — any stable string works — so this
 * is just a readable slug, not a protocol.
 */
function stationOutputKey(stationId) {
    return String(stationId)
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, '-')
        .replace(/^-+|-+$/g, '') || 'unknown';
}

/**
 * Writes ambient-weather-<key>-latest.json and, when NWS observations are
 * available this cycle, ambient-weather-<key>-accuracy.json for one
 * contributor — the same two files fetch-ambient-weather.cjs writes for
 * Jon, keyed per-station instead of hardcoded to one name. This is what
 * makes a station visible to node-dashboard.py's WEATHER STATIONS panel
 * (discovery-based since infra#895); the multi-tenant collector wrote
 * neither file before this, so a station monitored only through this
 * collector never appeared there even though it was being collected and
 * paid (infra#897).
 *
 * Best-effort and isolated: a write failure for one station must not abort
 * the others or the submission that already happened, matching this file's
 * existing per-contributor error isolation. NWS unavailability this cycle
 * (nwsObs is null) skips only the accuracy file, matching
 * fetch-ambient-weather.cjs's own try/catch-and-warn behavior for a failed
 * NWS fetch — it does not skip the latest.json write, and does not affect
 * submission, which has already happened by the time this runs.
 */
function writeStationDashboardFiles(station, fetchResult, nwsObs) {
    const key = stationOutputKey(station.stationId);
    const stationMeta = {
        mac: fetchResult.stationMeta?.mac ?? null,
        device_model: fetchResult.stationMeta?.device_model ?? 'unknown',
        location: fetchResult.stationMeta?.location ?? null,
        lat: fetchResult.stationMeta?.lat ?? null,
        lon: fetchResult.stationMeta?.lon ?? null,
        elevation_m: fetchResult.stationMeta?.elevation_m ?? null,
        name: fetchResult.stationName || station.stationId,
    };

    try {
        fs.mkdirSync(OUTPUT_DIR, { recursive: true });
        const latestFile = path.join(OUTPUT_DIR, `ambient-weather-${key}-latest.json`);
        fs.writeFileSync(latestFile, JSON.stringify({
            fetched_at: new Date().toISOString(),
            station: stationMeta,
            raw_count: fetchResult.rawCount ?? fetchResult.normalized.length,
            normalized: fetchResult.normalized,
        }, null, 2));
    } catch (err) {
        logStructured('warn', 'Failed to write dashboard latest.json for station', {
            station_id: station.stationId,
            error_message: err.message,
        });
    }

    if (!Array.isArray(nwsObs)) return; // NWS unavailable this cycle; latest.json above still landed.

    try {
        const { report: accuracyReport } = buildAccuracyReport(
            fetchResult.normalized,
            nwsObs,
            NWS_MAX_PAIR_DELTA_SECONDS,
        );
        const accuracyFile = path.join(OUTPUT_DIR, `ambient-weather-${key}-accuracy.json`);
        fs.writeFileSync(accuracyFile, JSON.stringify({
            computed_at: new Date().toISOString(),
            mode: 'live',
            nws_station: NWS_STATION_ID,
            max_pair_delta_seconds: NWS_MAX_PAIR_DELTA_SECONDS,
            pws_station: station.stationId,
            station: stationMeta,
            metrics: accuracyReport,
        }, null, 2));
    } catch (err) {
        logStructured('warn', 'Failed to write dashboard accuracy.json for station', {
            station_id: station.stationId,
            error_message: err.message,
        });
    }
}

// ---------------------------------------------------------------------------
// Backend roster fetch
// ---------------------------------------------------------------------------
function backendGetJson(host, port, urlPath, token, timeoutMs = 8000) {
    return new Promise((resolve) => {
        const req = http.request({
            hostname: host,
            port,
            path: urlPath,
            method: 'GET',
            headers: { ...(token ? { Authorization: `Bearer ${token}` } : {}) },
            timeout: timeoutMs,
        }, (res) => {
            let raw = '';
            res.on('data', (c) => { raw += c; });
            res.on('end', () => {
                try {
                    resolve({ status: res.statusCode, data: JSON.parse(raw) });
                } catch (_) {
                    resolve({ status: res.statusCode, data: raw });
                }
            });
        });
        req.on('timeout', () => { req.destroy(new Error('timeout')); });
        req.on('error', (err) => resolve({ status: null, error: err.message }));
        req.end();
    });
}

function strictRosterProjection(entry) {
    return {
        provider: entry?.provider || 'ambient',
        stationId: entry?.stationId,
        ownerAddress: entry?.ownerAddress,
        mac: entry?.mac,
        model: entry?.model ?? entry?.device_model,
        consent: entry?.consent,
        consentStatus: entry?.consentStatus,
        coarseMetadata: entry?.coarseMetadata,
        coarseMetadataStatus: entry?.coarseMetadataStatus,
        bindingRef: entry?.bindingRef,
    };
}

/**
 * Validate and normalize one roster entry. Returns null (and logs) if invalid.
 * Only the 'ambient' provider is handled here. Provider key material remains
 * outside the typed/configured roster projection.
 */
function normalizeRosterEntry(entry, index) {
    if (!entry || typeof entry !== 'object') {
        logStructured('warn', 'Skipping malformed roster entry', { index });
        return null;
    }
    const provider = (entry.provider || 'ambient').toLowerCase();
    if (provider !== 'ambient') {
        logStructured('info', 'Skipping non-ambient roster entry', { index, provider });
        return null;
    }
    const strict = validateStationRoster([strictRosterProjection(entry)]);
    if (!strict.ok) {
        logStructured('warn', 'Skipping roster entry missing required fields', {
            index,
            reason_codes: strict.errors.map((error) => error.code),
        });
        return null;
    }
    const configEntry = strict.entries[0];
    const runtimeCredentials = {};
    if (typeof entry.apiKey === 'string') runtimeCredentials.apiKey = String(entry.apiKey);
    if (typeof entry.appKey === 'string') runtimeCredentials.appKey = String(entry.appKey);
    return {
        ...configEntry,
        ...runtimeCredentials,
        latitude: Number.isFinite(entry.latitude) ? entry.latitude : null,
        longitude: Number.isFinite(entry.longitude) ? entry.longitude : null,
        elevation_m: Number.isFinite(entry.elevation_m) ? entry.elevation_m : undefined,
        name: entry.name ? String(entry.name) : undefined,
    };
}

function finalizeRoster(list, source) {
    const entries = list.map(normalizeRosterEntry).filter(Boolean);
    if (list.length > 0 && entries.length === 0) {
        logStructured('error', 'Roster rejected fail-closed', { roster_source: source, entry_count: list.length });
        return { entries: [], source: 'invalid' };
    }
    const strict = validateStationRoster(entries.map(strictRosterProjection));
    if (!strict.ok) {
        logStructured('error', 'Roster rejected fail-closed', {
            roster_source: source,
            entry_count: list.length,
            reason_codes: strict.errors.map((error) => error.code),
        });
        return { entries: [], source: 'invalid' };
    }
    return { entries, source };
}

/**
 * Source the contributor roster. Returns { entries, source } where source is
 * one of 'file' | 'backend' | 'none'. Never throws.
 */
async function loadRoster() {
    if (CREDENTIALS_FILE) {
        try {
            const abs = path.isAbsolute(CREDENTIALS_FILE)
                ? CREDENTIALS_FILE
                : path.join(process.cwd(), CREDENTIALS_FILE);
            const parsed = JSON.parse(fs.readFileSync(abs, 'utf8'));
            const list = Array.isArray(parsed) ? parsed : Array.isArray(parsed?.stations) ? parsed.stations : [];
            logStructured('info', 'Loaded contributor roster from fixture', {
                file: CREDENTIALS_FILE,
                station_count: list.length,
            });
            return finalizeRoster(list, 'file');
        } catch (err) {
            logStructured('error', 'Failed to read credentials fixture', {
                file: CREDENTIALS_FILE,
                error_message: err.message,
            });
            return { entries: [], source: 'none' };
        }
    }

    const res = await backendGetJson(BACKEND_HOST, BACKEND_PORT, CREDENTIALS_PATH, WRFCOIN_TOKEN);
    if (res.status !== 200 || !Array.isArray(res.data)) {
        logStructured('warn', 'Backend credentials endpoint unavailable or empty', {
            backend: `${BACKEND_HOST}:${BACKEND_PORT}${CREDENTIALS_PATH}`,
            http_status: res.status,
            error: res.error || null,
            // Redact any incidental token/body content.
            body: res.data ? redactSecrets(res.data).slice(0, 200) : null,
        });
        return { entries: [], source: 'none' };
    }
    logStructured('info', 'Loaded contributor roster from backend', {
        backend: `${BACKEND_HOST}:${BACKEND_PORT}`,
        station_count: res.data.length,
    });
    return finalizeRoster(res.data, 'backend');
}

// ---------------------------------------------------------------------------
// Node health
// ---------------------------------------------------------------------------
function nodeHealthy(host, port) {
    return new Promise((resolve) => {
        http.get({ hostname: host, port, path: '/health' }, (res) => {
            res.resume();
            resolve(res.statusCode === 200);
        }).on('error', () => resolve(false));
    });
}

// ---------------------------------------------------------------------------
// Single-account fallback (delegates to the original collector's main())
// ---------------------------------------------------------------------------
/**
 * Build the argv that the single-account collector should see, preserving the
 * flags the operator passed to THIS multi script. The single module parses its
 * CLI at import via getDefaultCliArgv(), which returns [] when it's required
 * (not the main module). Without propagation, a `--dry-run` (and other flags)
 * passed to the multi script would be silently lost and the fallback could
 * SUBMIT despite a requested dry run. We rebuild argv from the already-parsed
 * multi config so the fallback honors the same intent.
 */
function buildFallbackArgv() {
    const argv = [];
    if (DRY_RUN) argv.push('--dry-run');
    if (config.values['skip-nws']) argv.push('--skip-nws');
    if (Number.isFinite(MAX_AGE_MINUTES)) {
        argv.push('--max-age-minutes', String(MAX_AGE_MINUTES));
    }
    return argv;
}

async function runSingleAccountFallback() {
    const fallbackArgv = buildFallbackArgv();
    logStructured('info', 'Falling back to single-account collector', {
        reason: MULTI_TENANT_DISABLED ? 'MULTI_TENANT_DISABLED=1' : 'no contributors available',
        script: 'fetch-ambient-weather.cjs',
        dry_run: DRY_RUN,
        propagated_flags: fallbackArgv,
    });
    // The single module parses CLI flags at import time via getDefaultCliArgv(),
    // which returns [] when required (require.main !== module). Pass the
    // propagated flags through WRFCOIN_FALLBACK_ARGV (a JSON array) BEFORE the
    // require so --dry-run et al. are honored — otherwise the fallback could
    // SUBMIT despite an explicitly requested dry run.
    const savedOverride = process.env.WRFCOIN_FALLBACK_ARGV;
    process.env.WRFCOIN_FALLBACK_ARGV = JSON.stringify(fallbackArgv);
    let original;
    try {
        original = require('./fetch-ambient-weather.cjs');
    } finally {
        if (savedOverride === undefined) {
            delete process.env.WRFCOIN_FALLBACK_ARGV;
        } else {
            process.env.WRFCOIN_FALLBACK_ARGV = savedOverride;
        }
    }
    await original.main();
}

// ---------------------------------------------------------------------------
// Per-contributor processing with error isolation
// ---------------------------------------------------------------------------
function boundedCommitFingerprints(result) {
    const supplied = Array.isArray(result.commitIds)
        ? result.commitIds
        : Array.isArray(result.canonicalOutcome?.commit_ids)
            ? result.canonicalOutcome.commit_ids
            : result.quorum?.commitId
                ? [result.quorum.commitId]
                : result.commit_id
                    ? [result.commit_id]
                    : [];
    return [...new Set(supplied.map(String).filter(Boolean))]
        .slice(0, 20)
        .map((commitId) => crypto.createHash('sha256').update(commitId).digest('hex').slice(0, 16));
}

function safeCount(value) {
    const parsed = Number(value);
    return Number.isSafeInteger(parsed) && parsed >= 0 ? parsed : 0;
}

function emitCollectorCycleOutcome(stationId, result, emit = null) {
    const classified = result.stationCycleOutcome;
    if (!classified || typeof classified !== 'object') {
        throw new Error('submitStationReadings must supply stationCycleOutcome');
    }
    if (!COLLECTOR_CYCLE_OUTCOMES.has(classified.outcome)) {
        throw new Error(`unsupported collector cycle outcome: ${classified.outcome}`);
    }
    const commitFingerprints = boundedCommitFingerprints(result);
    const metric = {
        timestamp_ms: Date.now(),
        metric_type: 'collector_station_cycle_outcome',
        outcome: classified.outcome,
        reason: classified.reason,
        station_id: stationId,
        attempts: safeCount(result.quorum?.attempts || result.attempts),
        pending_count: safeCount(result.pending),
        commit_count: safeCount(result.committed || commitFingerprints.length),
        commit_fingerprints: commitFingerprints,
        ...(classified.reward_status ? { reward_status: classified.reward_status } : {}),
        ...(classified.reward_amount ? { reward_amount: classified.reward_amount } : {}),
    };
    const BENIGN_OUTCOMES = ['paid', 'upstream_stale', 'pending_reconciliation'];
    const sink = emit || (BENIGN_OUTCOMES.includes(classified.outcome) ? console.log : console.error);
    sink(JSON.stringify(metric));
    return { ...result, cycleOutcome: classified.outcome, cycleReason: classified.reason };
}

async function processContributor(station, ctx) {
    const tag = `${station.stationId} / ${station.ownerAddress.slice(0, 10)}…`;
    const fetchStationReadingsFn = ctx.fetchStationReadings || fetchStationReadings;
    const submitStationReadingsFn = ctx.submitStationReadings || submitStationReadings;
    const emitCycleOutcome = ctx.emitCycleOutcome || null;
    const finish = (result) => ctx.dryRun
        ? result
        : emitCollectorCycleOutcome(station.stationId, result, emitCycleOutcome);
    try {
        const fetchResult = await fetchStationReadingsFn(station, {
            readingsLimit: READINGS_LIMIT,
            maxAgeMinutes: MAX_AGE_MINUTES,
            // #3285: the registered elevation feeds pressure_sea_level_hpa.
            lookupRegisteredElevation: ctx.lookupRegisteredElevation
                || (ctx.host
                    ? (stationId) => fetchRegisteredElevation(stationId, {
                        host: ctx.host,
                        port: ctx.port,
                        token: ctx.token,
                    })
                    : undefined),
        });

        if (!fetchResult.ok) {
            emitFailureMetric(fetchResult.reason || 'history', {
                station_id: station.stationId,
                contributor: station.ownerAddress,
                detail: fetchResult.detail,
            });
            logStructured('warn', `Skipping contributor (${fetchResult.reason})`, {
                station_id: station.stationId,
                contributor: station.ownerAddress,
                detail: fetchResult.detail,
            });
            return finish({
                stationId: station.stationId,
                committed: 0,
                failed: 0,
                skipped: true,
                reason: fetchResult.reason || 'fetch_failed',
                stationCycleOutcome: {
                    outcome: 'rejected',
                    reason: fetchResult.reason || 'fetch_failed',
                    station_id: station.stationId,
                },
            });
        }

        console.log(`\n[${tag}] ${fetchResult.normalized.length} readings, station="${fetchResult.stationName}", stale=${fetchResult.stale}, model_binding=${fetchResult.modelBinding}`);
        if (fetchResult.modelBinding === 'operator_asserted') {
            // #2736: the weaker binding must be loud at the point it is
            // accepted, not only discoverable in per-reading metadata.
            logStructured('info', 'Model binding is operator-asserted (provider returned no model)', {
                station_id: station.stationId,
                contributor: station.ownerAddress,
                model_binding: fetchResult.modelBinding,
            });
        }

        // infra#897: write the dashboard-visible files before the dry-run
        // early return, matching fetch-ambient-weather.cjs, which writes
        // the station latest.json (and, unless --skip-nws, accuracy.json) even in
        // --dry-run — the fetch already happened; recording it doesn't
        // depend on whether submission does too.
        writeStationDashboardFiles(station, fetchResult, ctx.nwsObs);

        if (ctx.dryRun) {
            for (const r of fetchResult.normalized.slice(0, 3)) {
                console.log(`  (dry) ${r.recorded_at}: ${r.temperature_c}°C, ${r.humidity_pct}% RH, ${r.pressure_hpa} hPa`);
            }
            return { stationId: station.stationId, committed: 0, failed: 0, dryRun: true, readings: fetchResult.normalized.length, modelBinding: fetchResult.modelBinding };
        }

        if (fetchResult.stale || fetchResult.normalized.length === 0) {
            logStructured('info', 'Skipping submission: data stale', {
                station_id: station.stationId,
                reason: fetchResult.stale ? 'provider_observation_stale' : 'no_provider_readings',
            });
            return finish({
                stationId: station.stationId,
                committed: 0,
                failed: 0,
                stale: true,
                empty: fetchResult.normalized.length === 0,
                reason: fetchResult.stale ? 'provider_observation_stale' : 'no_provider_readings',
                stationCycleOutcome: {
                    outcome: 'upstream_stale',
                    reason: fetchResult.stale
                        ? 'provider_observation_stale'
                        : 'no_provider_readings',
                    station_id: station.stationId,
                },
            });
        }

        const result = await submitStationReadingsFn(
            fetchResult.normalized,
            station.stationId,
            station.ownerAddress,
            {
                host: ctx.host,
                port: ctx.port,
                token: ctx.token,
                stateFilePath: ctx.stateFilePath,
                nwsObsForTier: ctx.nwsObs,
            },
        );
        return finish({ stationId: station.stationId, ...result });
    } catch (err) {
        // Hard isolation: never let one contributor abort the run.
        emitFailureMetric('contributor_error', {
            station_id: station.stationId,
            contributor: station.ownerAddress,
            error: err.message,
        });
        logStructured('error', 'Unhandled error processing contributor', {
            station_id: station.stationId,
            contributor: station.ownerAddress,
            error_message: err.message,
        });
        return finish({
            stationId: station.stationId,
            committed: 0,
            failed: 1,
            error: true,
            reason: 'contributor_error',
            stationCycleOutcome: {
                outcome: 'rejected',
                reason: 'contributor_error',
                station_id: station.stationId,
            },
        });
    }
}

function formatContributorResultStatus(result) {
    if (result.dryRun) return `DRY(${result.readings})`;
    if (result.cycleOutcome) return `${result.cycleOutcome.toUpperCase()} (${result.cycleReason})`;
    if (result.error) return 'ERROR';
    if (result.skipped) return 'SKIPPED';
    if (result.stale) return 'UPSTREAM_STALE';
    if (result.mineFailed) {
        return `${result.committed} submitted / ${result.failed} submit failed / MINE FAILED`;
    }
    if (result.withheld) {
        return `${result.committed || 0} committed / ${result.failed || 0} failed / WITHHELD (${result.reason || 'quorum unavailable'})`;
    }
    if (result.mined) {
        return `${result.committed} submitted / ${result.failed} failed / MINED`;
    }
    return `${result.committed || 0} submitted / ${result.failed || 0} failed / NO MINE NEEDED`;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------
async function main() {
    console.log('=== Multi-Contributor Ambient Weather → WRFCoin Pipeline ===\n');
    console.log(formatRuntimeReport(config, {
        dry_run: DRY_RUN,
        backend: `${BACKEND_HOST}:${BACKEND_PORT}`,
        multi_tenant_disabled: MULTI_TENANT_DISABLED,
        credentials_file: CREDENTIALS_FILE || 'none',
        max_age_minutes: MAX_AGE_MINUTES,
        ambient_readings_limit: READINGS_LIMIT,
    }));

    // Validate producer custody before roster/backend, Ambient/NWS, or node
    // HTTP. A bad address must not create an endless commit/reveal write loop.
    if (!DRY_RUN && !isValidMinerAddress(process.env.WRFCOIN_MINER_ADDRESS)) {
        emitFailureMetric('invalid_config', {
            missing_var: 'WRFCOIN_MINER_ADDRESS',
            error_code: 'MINER_ADDRESS_REQUIRED',
        });
        logStructured('error', 'WRFCOIN_MINER_ADDRESS must be an explicit 0x-prefixed 20-byte address', {
            context: 'testnet block production requires an authorized payout address',
        });
        process.exitCode = 1;
        return;
    }

    if (MULTI_TENANT_DISABLED) {
        await runSingleAccountFallback();
        return;
    }

    const { entries, source } = await loadRoster();

    if (entries.length === 0 && source === 'invalid') {
        logStructured('error', 'Contributor roster invalid — refusing fallback', { roster_source: source });
        return;
    }

    if (entries.length === 0) {
        logStructured('warn', 'No contributors resolved — using single-account fallback', {
            roster_source: source,
        });
        await runSingleAccountFallback();
        return;
    }

    logStructured('info', 'MODE: multi-tenant', {
        roster_source: source,
        contributor_count: entries.length,
        dry_run: DRY_RUN,
    });

    if (!WRFCOIN_TOKEN && !DRY_RUN) {
        emitFailureMetric('invalid_config', { missing_var: 'WRFCOIN_OPERATOR_TOKEN' });
        logStructured('error', 'WRFCOIN_OPERATOR_TOKEN not set; use --dry-run to fetch without submitting', {});
        return;
    }

    // Optional NWS observations for quality tiering (best-effort, shared across stations
    // near the same region; per-station distance filtering happens in computeReadingQualityTier).
    let nwsObs = null;
    if (!DRY_RUN) {
        try {
            nwsObs = await fetchNwsObservations(NWS_STATION_ID);
            logStructured('info', 'Fetched NWS reference observations', {
                station: NWS_STATION_ID,
                count: nwsObs.length,
            });
        } catch (err) {
            logStructured('warn', 'NWS reference fetch failed; quality tiers default to good', {
                station: NWS_STATION_ID,
                error_message: err.message,
            });
        }
    }

    // Verify node reachable before submitting.
    if (!DRY_RUN) {
        const healthy = await nodeHealthy(WRFCOIN_HOST, WRFCOIN_PORT);
        if (!healthy) {
            emitFailureMetric('node_unreachable', { host: WRFCOIN_HOST, port: WRFCOIN_PORT });
            logStructured('error', `Node not reachable at ${WRFCOIN_HOST}:${WRFCOIN_PORT}`, {});
            return;
        }
    }

    const stateFilePath = getPendingRevealsPath();

    const ctx = {
        host: WRFCOIN_HOST,
        port: WRFCOIN_PORT,
        token: WRFCOIN_TOKEN,
        stateFilePath,
        nwsObs,
        dryRun: DRY_RUN,
    };

    const results = [];
    for (const station of entries) {
        // Sequential, not parallel: respects Ambient Weather's 1 req/sec limit
        // and keeps node block production ordered. Per-contributor try/catch
        // inside processContributor guarantees isolation.
        results.push(await processContributor(station, ctx));
    }

    // Summary
    const totals = results.reduce((acc, r) => {
        acc.committed += r.committed || 0;
        acc.failed += r.failed || 0;
        if (r.mineFailed) acc.mineFailed += 1;
        if (r.withheld) acc.withheld += 1;
        if (r.skipped || r.error) acc.skipped += 1;
        // #2828: 'pending_reconciliation' is a successful cycle whose
        // canonical reconciliation has not caught up yet — not a failure.
        if (
            r.cycleOutcome
            && r.cycleOutcome !== 'paid'
            && r.cycleOutcome !== 'pending_reconciliation'
        ) acc.nonPaidOutcomes += 1;
        return acc;
    }, { committed: 0, failed: 0, mineFailed: 0, withheld: 0, skipped: 0, nonPaidOutcomes: 0 });

    console.log('\n=== Summary ===');
    for (const r of results) {
        console.log(`  ${r.stationId}: ${formatContributorResultStatus(r)}`);
    }
    console.log(`\nContributors: ${results.length}  Submitted: ${totals.committed}  Failed: ${totals.failed}  Mining failures: ${totals.mineFailed}  Withheld: ${totals.withheld}  Skipped: ${totals.skipped}`);
    if (totals.mineFailed > 0 || totals.withheld > 0 || totals.nonPaidOutcomes > 0) process.exitCode = 1;
}

module.exports = {
    backendGetJson,
    normalizeRosterEntry,
    loadRoster,
    processContributor,
    emitCollectorCycleOutcome,
    formatContributorResultStatus,
    runSingleAccountFallback,
    main,
    stationOutputKey,
    writeStationDashboardFiles,
    OUTPUT_DIR,
};

if (require.main === module) {
    main().catch((err) => {
        console.error('Fatal error:', err);
        process.exit(1);
    });
}
