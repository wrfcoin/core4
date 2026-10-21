'use strict';

/**
 * Fetches real weather data from Ambient Weather Network personal weather
 * stations and submits it to the local WRFCoin testnet via commit/reveal.
 *
 * Configure PWS_STATION_ID, PWS_STATION_NAME, PWS_LOCATION, PWS_LAT and
 * PWS_LON for an operator-owned station. Location defaults are synthetic.
 *
 * Environment variables (set in .env.local or shell):
 *   AMBIENT_WEATHER_APP_KEY    — Ambient Weather application key
 *   AMBIENT_WEATHER_API_KEY   — Ambient Weather API key (preferred)
 *   AMBIENT_WEATHER_API_JON   — Legacy alias for the station API key
 *   AMBIENT_WEATHER_API_KEY2  — Fallback API key
 *   WRFCOIN_OPERATOR_TOKEN    — Testnet operator token
 *
 * Optional:
 *   AMBIENT_DEVICE_MAC        — operator device; default is a synthetic example
 *   WRFCOIN_API_HOST          — default 127.0.0.1
 *   WRFCOIN_API_PORT          — default 8556
 *   WRFCOIN_MINER_ADDRESS     — this host's authorized producer payout address
 *   AMBIENT_READINGS_LIMIT    — max readings to fetch (default 12, max 288)
 *
 * Usage:
 *   node tests/security/fetch-ambient-weather.cjs
 *   node tests/security/fetch-ambient-weather.cjs --dry-run   # fetch only, don't submit
 *   node tests/security/fetch-ambient-weather.cjs --dry-run --skip-nws   # skip NWS comparison
 *   node tests/security/fetch-ambient-weather.cjs --dry-run --replay-file tests/security/fixtures/duluth-ws5000-kdlh-replay.json
 *   node tests/security/fetch-ambient-weather.cjs --max-age-minutes 60   # custom staleness threshold
 *
 * Flags:
 *   --dry-run              Fetch and compare only, don't submit to testnet
 *   --skip-nws             Skip NWS reference comparison
 *   --max-age-minutes <N>  Staleness threshold in minutes (default 30)
 *   --replay-file <path>   Use a deterministic local replay bundle instead of live APIs
 *
 * Optional environment variables:
 *   NWS_STATION_ID         NWS station for reference comparison (default KDLH)
 */

const http = require('http');
const fs = require('fs');
const path = require('path');
const {
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');
const {
    buildAccuracyReport,
    computeReadingQualityTier,
    loadReplayBundle,
    normalizeReading,
    prepareReplayReadings,
} = require('./ambient-weather-comparison.cjs');
// Shared submission pipeline (HTTP helpers, commit/reveal hashing,
// pending-reveal state, redaction, metrics, mining). Single source of truth
// shared with fetch-ambient-weather-multi.cjs — do not duplicate this logic.
const {
    redactSecrets,
    emitFailureMetric,
    logStructured,
    httpsGet,
    httpGet,
    httpPost,
    sleep,
    isValidMinerAddress,
    postWithRetry,
    fetchDevices,
    fetchDeviceHistory,
    selectAmbientReadings,
    fetchNwsObservations,
    sortKeys,
    computeCommitId,
    computePendingRevealReadingId,
    computeNextLastSubmittedTs,
    getPendingRevealsPath,
    loadPendingRevealEntries,
    savePendingRevealEntries,
    getBlockchainHeight,
    submitContributionCommit,
    submitContributionReveal,
    processPendingReveals,
    submitCommitReveal,
    submitStationReadings,
    mineBlock,
    fetchRegisteredElevation,
} = require('./lib/ambient-pipeline.cjs');

function getDefaultCliArgv() {
    if (require.main === module) return process.argv.slice(2);
    // When required by the multi-tenant collector's single-account fallback, it
    // sets WRFCOIN_FALLBACK_ARGV (a JSON array) so flags like --dry-run survive
    // the import-time config parse. Without this, getDefaultCliArgv() returns []
    // and a requested dry-run would be lost — the fallback could submit anyway.
    const override = process.env.WRFCOIN_FALLBACK_ARGV;
    if (override) {
        try {
            const parsed = JSON.parse(override);
            if (Array.isArray(parsed)) return parsed.map(String);
        } catch (_) { /* malformed override → no flags, fail safe */ }
    }
    return [];
}

// ---------------------------------------------------------------------------
// Load .env.local if present (simple key=value parser, no dependencies)
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
            if (val && !process.env[key]) {
                process.env[key] = val;
            }
        }
    } catch (_) {
        // .env.local is optional
    }
}

// Try loading from repo root and cwd
loadEnvFile(path.join(__dirname, '../../.env.local'));
loadEnvFile(path.join(process.cwd(), '.env.local'));

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------
const APP_KEYS = [
    process.env.AMBIENT_WEATHER_APP_KEY,
    process.env.AMBIENT_APP_KEY2,
].filter(Boolean);
const API_KEYS = [
    process.env.AMBIENT_WEATHER_API_KEY,
    process.env.AMBIENT_WEATHER_API_JON,
    process.env.AMBIENT_WEATHER_API_KEY2,
].filter(Boolean);

const config = loadTestConfig({
    argv: getDefaultCliArgv(),
    scriptName: 'tests/security/fetch-ambient-weather.cjs',
    description: 'Fetch Ambient Weather data and optionally submit it to the local WRFCoin testnet.',
    requireToken: false,
    extraOptions: [
        { name: 'dry-run', type: 'boolean', description: 'Fetch and compare only, do not submit to testnet' },
        { name: 'skip-nws', type: 'boolean', description: 'Skip NWS comparison' },
        { name: 'replay-file', type: 'string', description: 'Replay bundle path for deterministic offline runs' },
        { name: 'max-age-minutes', type: 'int', default: 30, description: 'Maximum accepted data age before submission is skipped' },
    ],
});

const WRFCOIN_HOST = config.apiHost;
const WRFCOIN_PORT = config.apiPort;
const WRFCOIN_TOKEN = config.operatorToken;
const READINGS_LIMIT = parseInt(process.env.AMBIENT_READINGS_LIMIT || '12', 10);
const DRY_RUN = Boolean(config.values['dry-run']);
const SKIP_NWS = Boolean(config.values['skip-nws']);
const REPLAY_FILE = config.values['replay-file'] || null;
const MAX_AGE_MINUTES = config.values['max-age-minutes'];

// NWS reference station used by the example comparison.
const NWS_STATION_ID = process.env.NWS_STATION_ID || 'KDLH';
const NWS_MAX_PAIR_DELTA_SECONDS = 300; // 5 minutes

// Operator-configured station; public defaults are synthetic.
const JONS_MAC = process.env.AMBIENT_DEVICE_MAC || '02:00:00:00:00:01';
const JONS_STATION_ID = process.env.PWS_STATION_ID || 'PWS-EXAMPLE-WS5000';
const JONS_CONTRIBUTOR = process.env.JONS_CONTRIBUTOR || '0x5Eaf842a8341CD38812A63De6B5dcc5103f071c0';
// #3285: no default elevation. The elevation used for pressure_sea_level_hpa
// comes from the station's on-chain registration (read below), unless an
// operator sets JONS_ELEVATION_M / PWS_ELEVATION_M as an explicit override.
// With neither, the reading carries no sea-level key: there is no guess.
function parseElevationOverride(env = process.env) {
    for (const name of ['JONS_ELEVATION_M', 'PWS_ELEVATION_M']) {
        const raw = env[name];
        if (raw == null || String(raw).trim() === '') continue;
        const value = Number(raw);
        if (Number.isFinite(value)) return value;
    }
    return null;
}
const JONS_ELEVATION_OVERRIDE_M = parseElevationOverride();

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
    console.log('=== Ambient Weather → WRFCoin Testnet Pipeline ===\n');
    console.log(formatRuntimeReport(config, {
        dry_run: DRY_RUN,
        skip_nws: SKIP_NWS,
        replay_file: REPLAY_FILE || 'none',
        max_age_minutes: MAX_AGE_MINUTES,
        ambient_readings_limit: READINGS_LIMIT,
    }));

    // Refuse a producer configuration error before fetching Ambient/NWS data
    // or touching the chain. Otherwise commit/reveal writes can accumulate
    // every cycle even though mining can never be authorized.
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

    if (!REPLAY_FILE) {
        if (APP_KEYS.length === 0) {
            emitFailureMetric('invalid_config', {
                missing_var: 'AMBIENT_WEATHER_APP_KEY or AMBIENT_APP_KEY2',
            });
            logStructured('error', 'No Ambient Weather application key set', {
                required_vars: ['AMBIENT_WEATHER_APP_KEY', 'AMBIENT_APP_KEY2'],
            });
            return;
        }
        if (API_KEYS.length === 0) {
            emitFailureMetric('invalid_config', {
                missing_var: 'AMBIENT_WEATHER_API_KEY / API_JON / API_KEY2',
            });
            logStructured('error', 'No Ambient Weather API key set', {
                required_vars: ['AMBIENT_WEATHER_API_KEY', 'AMBIENT_WEATHER_API_JON', 'AMBIENT_WEATHER_API_KEY2'],
            });
            return;
        }
    }
    if (!WRFCOIN_TOKEN && !DRY_RUN && !REPLAY_FILE) {
        emitFailureMetric('invalid_config', {
            missing_var: 'WRFCOIN_OPERATOR_TOKEN',
        });
        logStructured('error', 'WRFCOIN_OPERATOR_TOKEN not set', {
            context: 'testnet submission requires token',
            workaround: 'Use --dry-run to fetch without submitting',
        });
        return;
    }

    let normalized = [];
    let readings = [];
    let nwsObsForTier = null; // set after NWS fetch; used for per-reading quality tier
    let stationMeta = {
        mac: JONS_MAC,
        device_model: 'WS-5000',
        location: process.env.PWS_LOCATION || 'Example location',
        lat: Number(process.env.PWS_LAT || '10.0'),
        lon: Number(process.env.PWS_LON || '20.0'),
        elevation_m: JONS_ELEVATION_OVERRIDE_M,
        elevation_source: JONS_ELEVATION_OVERRIDE_M != null ? 'operator_override' : null,
    };
    let stationName = process.env.PWS_STATION_NAME || 'Example Station';
    let stale = false;
    let replayBundle = null;

    const outputDir = path.join(__dirname, '../../data');
    try { fs.mkdirSync(outputDir, { recursive: true }); } catch (_) {}

    if (REPLAY_FILE) {
        const { bundle, absolutePath } = loadReplayBundle(REPLAY_FILE);
        replayBundle = bundle;
        stationMeta = {
            ...stationMeta,
            ...(bundle.station || {}),
        };
        stationName = bundle.station?.name || stationName;
        normalized = prepareReplayReadings(bundle);
        readings = bundle.raw_history || [];

        console.log(`Loaded replay bundle: ${path.relative(process.cwd(), absolutePath)}`);
        console.log(`  Station: ${stationName}`);
        console.log(`  Elevation: ${stationMeta.elevation_m ?? 'unknown'} m`);
        console.log(`  Readings: ${normalized.length}\n`);
    } else {
        let devices = null;
        let workingAppKey = null;
        let workingApiKey = null;

        outer:
        for (const appKey of APP_KEYS) {
            for (const apiKey of API_KEYS) {
                const appPrefix = appKey.slice(0, 4) + '...';
                const apiPrefix = apiKey.slice(0, 4) + '...';
                console.log(`Trying app=${appPrefix} api=${apiPrefix}`);
                await sleep(1100); // respect 1 req/sec rate limit
                const res = await fetchDevices(appKey, apiKey);

                if (res.status === 200 && Array.isArray(res.data)) {
                    devices = res.data;
                    workingAppKey = appKey;
                    workingApiKey = apiKey;
                    console.log(`  OK — found ${devices.length} device(s)\n`);
                    break outer;
                } else if (res.status === 401) {
                    console.log('  401 Unauthorized');
                } else {
                    const bodyStr = typeof res.data === 'string' ? res.data : JSON.stringify(res.data);
                    console.log(`  HTTP ${res.status} — ${redactSecrets(bodyStr).slice(0, 100)}`);
                }
            }
        }

        if (!devices || !workingApiKey) {
            emitFailureMetric('auth', {
                app_keys_tried: APP_KEYS.length,
                api_keys_tried: API_KEYS.length,
                reason: 'All key combinations returned 401 or network error',
            });
            logStructured('error', 'Failed to authenticate with Ambient Weather API', {
                app_keys_tried: APP_KEYS.length,
                api_keys_tried: API_KEYS.length,
                action: 'Check API keys in .env.local or environment variables',
                retryable: true,
            });
            return;
        }

        const jonsDevice = devices.find((device) =>
            device.macAddress && device.macAddress.toUpperCase() === JONS_MAC.toUpperCase()
        );

        if (!jonsDevice) {
            const availableMacs = devices.map(d => d.macAddress).join(', ');
            emitFailureMetric('device_missing', {
                expected_mac: JONS_MAC,
                available_macs: availableMacs,
                device_count: devices.length,
            });
            logStructured('error', `Device ${JONS_MAC} not found in device list`, {
                expected: JONS_MAC,
                available_devices: devices.length,
                available_macs: availableMacs,
                action: 'Check if station MAC changed or was removed from Ambient Weather account',
                retryable: true,
            });
            return;
        }

        const stationInfo = jonsDevice.info || {};
        stationMeta = {
            ...stationMeta,
            location: stationInfo.location || stationMeta.location,
            lat: stationInfo.coords?.lat ?? stationMeta.lat,
            lon: stationInfo.coords?.lon ?? stationMeta.lon,
        };
        stationName = stationInfo.name || stationName;

        console.log(`Found configured WS-5000:`);
        console.log(`  Name: ${stationName || 'N/A'}`);
        console.log(`  Location: ${stationMeta.location || 'N/A'}`);
        if (stationInfo.coords) {
            console.log(`  Coords: ${stationMeta.lat}, ${stationMeta.lon}`);
        }
        if (stationMeta.elevation_source !== 'operator_override') {
            const registered = await fetchRegisteredElevation(JONS_STATION_ID, {
                host: WRFCOIN_HOST,
                port: WRFCOIN_PORT,
                token: WRFCOIN_TOKEN,
            });
            if (Number.isFinite(registered.elevation_m)) {
                stationMeta.elevation_m = registered.elevation_m;
                stationMeta.elevation_source = 'registry';
            } else {
                console.log(`  No registered elevation (${registered.reason}): no pressure_sea_level_hpa will be submitted`);
            }
        }
        console.log(`  Elevation: ${stationMeta.elevation_m ?? 'none'} m (${stationMeta.elevation_source ?? 'no source'})\n`);

        if (jonsDevice.lastData) {
            const latest = jonsDevice.lastData;
            console.log('Latest reading:');
            console.log(`  Temp: ${latest.tempf}°F  Humidity: ${latest.humidity}%  Pressure: ${latest.baromrelin} inHg`);
            console.log(`  Wind: ${latest.windspeedmph} mph @ ${latest.winddir}°  Gusts: ${latest.windgustmph} mph`);
            console.log(`  Rain (hourly): ${latest.hourlyrainin} in  UV: ${latest.uv}  Solar: ${latest.solarradiation} W/m²`);
            console.log(`  Date: ${latest.date}`);
            console.log('');
        }

        console.log(`Fetching last ${READINGS_LIMIT} readings...`);
        await sleep(1100);
        const histRes = await fetchDeviceHistory(JONS_MAC, workingAppKey, workingApiKey, READINGS_LIMIT);

        if (histRes.status !== 200 || !Array.isArray(histRes.data)) {
            emitFailureMetric('history', {
                http_status: histRes.status,
                mac_address: JONS_MAC,
                limit: READINGS_LIMIT,
            });
            logStructured('error', `Failed to fetch device history (HTTP ${histRes.status})`, {
                mac_address: JONS_MAC,
                limit: READINGS_LIMIT,
                response_type: typeof histRes.data,
                retryable: histRes.status !== 401,
            });
            return;
        }

        readings = selectAmbientReadings(histRes.data, jonsDevice.lastData);
        console.log(`  Received ${histRes.data.length} historical readings`);
        if (histRes.data.length === 0 && readings.length === 1) {
            console.log('  Using fresh device lastData as a singleton fallback');
        }
        console.log('');
        normalized = readings.map((reading) => normalizeReading(reading, stationMeta));

        console.log('Normalized readings (first 3):');
        for (const reading of normalized.slice(0, 3)) {
            console.log(
                `  ${reading.recorded_at}: ${reading.temperature_c}°C, ${reading.humidity_pct}% RH, ` +
                `${reading.pressure_hpa} hPa submitted, ${reading.comparison_pressure_hpa} hPa compared (${reading.pressure_comparison_method}), ` +
                `wind ${reading.wind_speed_ms} m/s @ ${reading.wind_direction_deg}°`,
            );
        }
        console.log('');

        if (normalized.length > 0) {
            const newestTs = Math.max(...normalized.map((reading) =>
                reading.recorded_at ? new Date(reading.recorded_at).getTime() : 0
            ));
            if (newestTs > 0) {
                const ageMinutes = (Date.now() - newestTs) / 60_000;
                console.log(`Most recent reading age: ${ageMinutes.toFixed(1)} minutes`);
                if (ageMinutes > MAX_AGE_MINUTES) {
                    console.log(`WARNING: Data is stale (${ageMinutes.toFixed(0)}m > ${MAX_AGE_MINUTES}m threshold). Skipping testnet submission.\n`);
                    stale = true;
                }
            }
        }

        const outputFile = path.join(outputDir, 'ambient-weather-jon-latest.json');
        fs.writeFileSync(outputFile, JSON.stringify({
            fetched_at: new Date().toISOString(),
            station: { ...stationMeta, name: stationName },
            raw_count: readings.length,
            normalized,
        }, null, 2));
        console.log(`Saved ${readings.length} readings to ${path.relative(process.cwd(), outputFile)}\n`);
    }

    // NWS reference comparison (runs even when stale or dry-run)
    if (!SKIP_NWS) {
        try {
            console.log(`=== NWS Reference Comparison (station ${NWS_STATION_ID}) ===\n`);
            const nwsObs = REPLAY_FILE
                ? replayBundle.nws_observations
                : await fetchNwsObservations(NWS_STATION_ID);
            if (!Array.isArray(nwsObs)) {
                throw new Error('Replay bundle is missing nws_observations.');
            }
            nwsObsForTier = nwsObs; // captured for per-reading quality tier during submission
            console.log(`  Fetched ${nwsObs.length} NWS observations\n`);

            const { report: accuracyReport, variableDefs } = buildAccuracyReport(
                normalized,
                nwsObs,
                NWS_MAX_PAIR_DELTA_SECONDS,
            );

            console.log('  Variable              Pairs   MAE     RMSE    Bias    Score   Band       Method');
            console.log('  ' + '-'.repeat(96));

            for (const variable of variableDefs) {
                const metrics = accuracyReport[variable.key];
                if (metrics) {
                    console.log(
                        `  ${variable.label.padEnd(22)} ${String(metrics.sample_count).padStart(5)}   ` +
                        `${metrics.mae.toFixed(2).padStart(6)}  ${metrics.rmse.toFixed(2).padStart(6)}  ` +
                        `${metrics.bias.toFixed(2).padStart(6)}  ${metrics.accuracy_score.toFixed(3).padStart(6)}  ` +
                        `${metrics.band.padEnd(10)} ${metrics.scoring_method}`,
                    );
                } else {
                    console.log(`  ${variable.label.padEnd(22)}     0   (no paired readings)`);
                }
            }
            console.log('');

            const accuracyFile = path.join(outputDir, 'ambient-weather-jon-accuracy.json');
            fs.writeFileSync(accuracyFile, JSON.stringify({
                computed_at: new Date().toISOString(),
                mode: REPLAY_FILE ? 'replay' : 'live',
                nws_station: NWS_STATION_ID,
                max_pair_delta_seconds: NWS_MAX_PAIR_DELTA_SECONDS,
                pws_station: JONS_STATION_ID,
                station: { ...stationMeta, name: stationName },
                metrics: accuracyReport,
            }, null, 2));
            console.log(`Saved accuracy report to ${path.relative(process.cwd(), accuracyFile)}\n`);
        } catch (nwsErr) {
            console.warn(`WARNING: NWS comparison failed: ${nwsErr.message}\n`);
        }
    }

    if (DRY_RUN || REPLAY_FILE) {
        if (REPLAY_FILE) {
            console.log('Replay mode, skipping testnet submission.');
            return;
        }
        console.log('--dry-run specified, skipping testnet submission.');
        return;
    }

    if (stale) {
        console.log('Data is stale, skipping new testnet submissions.');
        normalized = [];
    }

    // Submit to testnet
    console.log('=== Submitting to WRFCoin testnet ===\n');

    // Verify testnet is up
    try {
        const health = await new Promise((resolve, reject) => {
            http.get({ hostname: WRFCOIN_HOST, port: WRFCOIN_PORT, path: '/health' }, (res) => {
                let raw = '';
                res.on('data', c => raw += c);
                res.on('end', () => resolve({ status: res.statusCode }));
            }).on('error', reject);
        });
        if (health.status !== 200) {
            emitFailureMetric('node_unreachable', {
                host: WRFCOIN_HOST,
                port: WRFCOIN_PORT,
                path: '/health',
                http_status: health.status,
            });
            logStructured('error', `Testnet health check failed (HTTP ${health.status})`, {
                host: WRFCOIN_HOST,
                port: WRFCOIN_PORT,
                endpoint: '/health',
                retryable: true,
            });
            return;
        }
        console.log('Testnet health: OK\n');
    } catch (e) {
        emitFailureMetric('node_unreachable', {
            host: WRFCOIN_HOST,
            port: WRFCOIN_PORT,
            error: e.message,
        });
        logStructured('error', `Testnet not reachable at ${WRFCOIN_HOST}:${WRFCOIN_PORT}`, {
            host: WRFCOIN_HOST,
            port: WRFCOIN_PORT,
            error_message: e.message,
            retryable: true,
        });
        return;
    }

    const collectorStatePath = getPendingRevealsPath();
    const lastTsFile = path.join(outputDir, 'last-submitted-ts.txt');
    const result = await submitStationReadings(
        normalized,
        JONS_STATION_ID,
        JONS_CONTRIBUTOR,
        {
            host: WRFCOIN_HOST,
            port: WRFCOIN_PORT,
            token: WRFCOIN_TOKEN,
            stateFilePath: collectorStatePath,
            watermarkPath: lastTsFile,
            nwsObsForTier,
        },
    );
    // #3270: a 201 mine whose block did not carry the reveals is a
    // rejection, not a withhold and never a success.
    const excludedNote = result.excluded > 0 ? ` (${result.excluded} reveal(s) NOT included)` : '';
    const status = result.mined
        ? ` — MINED QUORUM-QUALIFIED REVEALS${excludedNote}`
        : result.reason === 'reveal_not_included'
            ? ` — REJECTED (reveal_not_included${excludedNote})`
            : result.withheld
            ? ` — WITHHELD (${result.reason})`
            : '';
    console.log(
        `\nSubmission complete: ${result.committed} committed, ${result.failed} failed out of ${normalized.length} current readings${status}`,
    );
    if (result.mineFailed || result.withheld) {
        process.exitCode = 1;
    }

    // Check timeline
    const timelineUrl = `/transactions/rewards/trust/timeline?offset=0&limit=10`;
    const res = await new Promise((resolve, reject) => {
        http.get({
            hostname: WRFCOIN_HOST,
            port: WRFCOIN_PORT,
            path: timelineUrl,
            headers: { Authorization: `Bearer ${WRFCOIN_TOKEN}` },
        }, (r) => {
            let raw = '';
            r.on('data', c => raw += c);
            r.on('end', () => resolve(JSON.parse(raw)));
        }).on('error', reject);
    });

    console.log(`Timeline total: ${res.total} entries`);
}

// ---------------------------------------------------------------------------
// Unit tests for 429 retry logic
// ---------------------------------------------------------------------------
async function testPostWithRetry() {
    // Test 1: Success on first try
    const mockRequest1 = async () => ({ status: 200, data: { ok: true } });
    const result1 = await postWithRetry(mockRequest1, 'localhost', 8080, '/test', {}, 'token', 'test-phase');
    if (result1.status !== 200) throw new Error('Test 1 failed: expected 200');

    // Test 2: 429 then success on retry
    let attempt2 = 0;
    const mockRequest2 = async () => {
        attempt2++;
        if (attempt2 === 1) return { status: 429, data: { retry_after: 1 } };
        return { status: 200, data: { ok: true } };
    };
    const result2 = await postWithRetry(mockRequest2, 'localhost', 8080, '/test', {}, 'token', 'test-phase');
    if (result2.status !== 200) throw new Error('Test 2 failed: expected 200 after retry');
    if (attempt2 !== 2) throw new Error('Test 2 failed: expected 2 attempts');

    // Test 3: 429 then 429 on retry (both fail)
    let attempt3 = 0;
    const mockRequest3 = async () => {
        attempt3++;
        return { status: 429, data: { retry_after: 1 } };
    };
    const result3 = await postWithRetry(mockRequest3, 'localhost', 8080, '/test', {}, 'token', 'test-phase');
    if (result3.status !== 429) throw new Error('Test 3 failed: expected final 429');
    if (attempt3 !== 2) throw new Error('Test 3 failed: expected 2 attempts');
}

// ---------------------------------------------------------------------------
// Unit tests for redactSecrets (inline assertions)
// ---------------------------------------------------------------------------
function testRedactSecrets() {
    // Test Bearer token redaction
    const bearerTest = 'Error: Bearer mySecretToken123456abcdef';
    const bearerResult = redactSecrets(bearerTest);
    if (!bearerResult.includes('Bearer <REDACTED>') || bearerResult.includes('mySecretToken')) {
        throw new Error(`Bearer redaction failed: ${bearerResult}`);
    }

    // Test long hex/base64 redaction
    const longTokenTest = 'Error: a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0';
    const tokenResult = redactSecrets(longTokenTest);
    if (!tokenResult.includes('<TOKEN_REDACTED>') || tokenResult.includes('a1b2c3d4')) {
        throw new Error(`Long token redaction failed: ${tokenResult}`);
    }

    // Test truncation (500 char max)
    const longString = 'x'.repeat(600);
    const truncResult = redactSecrets(longString);
    if (truncResult.length > 520) { // 500 + "... (truncated)"
        throw new Error(`Truncation failed: length ${truncResult.length}`);
    }

    // Test that normal strings pass through
    const normalTest = 'Error: Connection timeout';
    const normalResult = redactSecrets(normalTest);
    if (!normalResult.includes('Connection timeout')) {
        throw new Error(`Normal string failed: ${normalResult}`);
    }
}

// ---------------------------------------------------------------------------
// Unit tests for computeReadingQualityTier (inline assertions)
// ---------------------------------------------------------------------------
function testComputeReadingQualityTier() {
    const now = new Date('2024-01-15T12:00:00Z').getTime();
    const nwsObs = [
        {
            timestamp_ms: now,
            temperature_c: 10.0,
            pressure_hpa: 1013.0,
            humidity_pct: 50.0,
            wind_speed_ms: 2.0,
        },
    ];
    const makeReading = (temp, ts = now) => ({
        recorded_at: new Date(ts).toISOString(),
        temperature_c: temp,
        pressure_hpa: 1013.0,
        humidity_pct: 50.0,
        wind_speed_ms: 2.0,
    });

    // Test (a): good reading (temp within Good band ≤ 2°C MAE) → tier=good
    const goodReading = makeReading(10.5); // 0.5°C residual — Excellent
    const goodTier = computeReadingQualityTier(goodReading, nwsObs, 300);
    if (goodTier !== 'good') throw new Error(`Test (a) failed: expected 'good', got '${goodTier}'`);

    // Test (b): outlier reading with NWS reachable (temp 3.0°C residual → Marginal band) → tier=degraded
    const marginalReading = makeReading(13.0); // 3.0°C residual — Marginal (good≤2, marginal≤3.5)
    const marginalTier = computeReadingQualityTier(marginalReading, nwsObs, 300);
    if (marginalTier !== 'degraded') throw new Error(`Test (b) failed: expected 'degraded', got '${marginalTier}'`);

    // Test (b2): severe outlier (temp >5°C → Poor) → tier=outlier
    const poorReading = makeReading(17.0); // 7°C residual — Poor
    const poorTier = computeReadingQualityTier(poorReading, nwsObs, 300);
    if (poorTier !== 'outlier') throw new Error(`Test (b2) failed: expected 'outlier', got '${poorTier}'`);

    // Test (c): outlier reading with NWS unreachable → tier=good
    const unreachableTier = computeReadingQualityTier(makeReading(17.0), null, 300);
    if (unreachableTier !== 'good') throw new Error(`Test (c) failed: expected 'good' (no NWS), got '${unreachableTier}'`);

    // Test (d): NWS obs outside max delta → tier=good
    const staleNws = [{ timestamp_ms: now - 400_000, temperature_c: 10.0, pressure_hpa: 1013.0, humidity_pct: 50.0, wind_speed_ms: 2.0 }];
    const staleTier = computeReadingQualityTier(makeReading(17.0), staleNws, 300);
    if (staleTier !== 'good') throw new Error(`Test (d) failed: expected 'good' (stale NWS), got '${staleTier}'`);
}

// Run tests only in test environments (not when imported)
if (process.env.NODE_ENV === 'test' || process.argv.includes('--test')) {
    try {
        if (typeof testPostWithRetry === 'function') {
            testPostWithRetry().catch(e => {
                console.error('testPostWithRetry failed:', e.message);
                process.exit(1);
            });
        }
        testRedactSecrets();
        testComputeReadingQualityTier();
    } catch (e) {
        console.error('inline unit tests failed:', e.message);
        process.exit(1);
    }
}

module.exports = {
    parseElevationOverride,
    computeCommitId,
    computeNextLastSubmittedTs,
    computePendingRevealReadingId,
    fetchDeviceHistory,
    fetchDevices,
    selectAmbientReadings,
    fetchNwsObservations,
    httpPost,
    httpsGet,
    main,
    mineBlock,
    processPendingReveals,
    postWithRetry,
    redactSecrets,
    getPendingRevealsPath,
    loadPendingRevealEntries,
    savePendingRevealEntries,
    sortKeys,
    submitCommitReveal,
    submitContributionCommit,
    submitContributionReveal,
    testPostWithRetry,
    testRedactSecrets,
    testComputeReadingQualityTier,
    // Test-only: exposes the import-time-parsed flags so the multi-tenant
    // fallback can be verified to honor --dry-run propagated via
    // WRFCOIN_FALLBACK_ARGV. Not part of the runtime contract.
    __dryRunForTest: DRY_RUN,
    __maxAgeMinutesForTest: MAX_AGE_MINUTES,
    __skipNwsForTest: SKIP_NWS,
};

if (require.main === module) {
    if (process.env.NODE_ENV === 'test' || process.argv.includes('--test')) {
        testPostWithRetry()
            .then(() => {
                console.log('✓ All postWithRetry tests passed');
                try {
                    testComputeReadingQualityTier();
                    console.log('✓ All computeReadingQualityTier tests passed');
                } catch (err) {
                    console.error('computeReadingQualityTier tests failed:', err.message);
                    process.exit(1);
                }
            })
            .catch((err) => {
                console.error('postWithRetry tests failed:', err.message);
                process.exit(1);
            });
    } else {
        main().catch((err) => {
            console.error('Fatal error:', err);
            process.exit(1);
        });
    }
}
