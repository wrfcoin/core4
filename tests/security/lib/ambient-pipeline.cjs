'use strict';

/**
 * Shared Ambient Weather → WRFCoin submission pipeline.
 *
 * This module holds the reusable machinery that both the single-account
 * collector (`fetch-ambient-weather.cjs`) and the multi-contributor collector
 * (`fetch-ambient-weather-multi.cjs`) build on:
 *
 *   - HTTP/HTTPS helpers (node, Ambient Weather, NWS)
 *   - Secret redaction for safe error logging
 *   - Structured logging + failure metrics
 *   - Commit/reveal hashing (SHA3-256, sorted-key contract — see
 *     reference_commit_reveal_hash_contract.md)
 *   - Pending-reveal durable state (crash recovery)
 *   - submitCommitReveal / processPendingReveals / mineBlock
 *   - A per-station fetch+normalize+submit loop (collectStation) used to
 *     attribute readings under a specific contributor/station identity.
 *
 * The meteorological normalization + NWS accuracy math intentionally lives in
 * `ambient-weather-comparison.cjs` and is NOT duplicated here — both scripts
 * import it directly. This module only owns the submission/orchestration layer.
 */

const https = require('https');
const http = require('http');
const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');

const {
    buildAccuracyReport,
    computeReadingQualityTier,
    normalizeReading,
} = require('../ambient-weather-comparison.cjs');
const {
    selectExactDevice,
    verifyObservationFreshness,
} = require('./ambient-roster-verifier.cjs');

const AMBIENT_BASE = 'https://rt.ambientweather.net';
const NWS_MAX_PAIR_DELTA_SECONDS = 300; // 5 minutes
const PENDING_REVEAL_MAX_AGE_BLOCKS = 100;
const PENDING_ATTESTATIONS_PAGE_LIMIT = 500;
const ATTESTATION_POLL_INTERVAL_MS = 1_000;
const ATTESTATION_MAX_POLLS = 30;
// #3266: a pending reveal that can no longer reach attestation quorum must
// reach a terminal outcome instead of wedging every later cycle. The
// freshness window is the attester's own knob (oracle_attester.rs reads
// WRFCOIN_ATTESTER_MAX_AGE_SECS, default shared_protocol::oracle::mechanical::
// DEFAULT_MAX_AGE_SECS = 3600); the collector reads the same variable so the
// two can only diverge if an operator sets it on one side alone.
const ATTESTATION_FRESHNESS_WINDOW_ENV = 'WRFCOIN_ATTESTER_MAX_AGE_SECS';
const DEFAULT_ATTESTATION_FRESHNESS_WINDOW_SECS = 3_600;
const PENDING_REVEAL_MAX_TIMEOUT_CYCLES_ENV = 'WRFCOIN_PENDING_REVEAL_MAX_TIMEOUT_CYCLES';
const DEFAULT_PENDING_REVEAL_MAX_TIMEOUT_CYCLES = 3;
const TERMINAL_OUTCOME_EXPIRED_UNQUALIFIED = 'expired_unqualified';
// #3270: /blocks/mine returned 201 but the block did not carry the reveal.
const REVEAL_NOT_INCLUDED = 'reveal_not_included';
const PENDING_PHASE_COMMITTED = 'committed';
const PENDING_PHASE_AWAITING_QUORUM = 'awaiting_quorum';
// 'pending_reconciliation' (#2828): the cycle SUCCEEDED — reveals mined,
// quorum reached — but canonical per-station reconciliation has not produced
// an outcome yet. It is a benign in-flight state, not a failure; alerting on
// it as a timeout is what caused the false blocker #2821.
const COLLECTOR_CYCLE_OUTCOMES = new Set(['paid', 'rejected', 'upstream_stale', 'timeout', 'pending_reconciliation']);
const REJECTED_SETTLEMENT_REASONS = new Set([
    'rejected',
    'unrecoverable',
    'expired_unrecoverable',
    'canonical_rejected',
    'mine_failed',
    REVEAL_NOT_INCLUDED,
]);

const COLLECTOR_PENDING_REVEALS_PATH = path.join(
    os.homedir(),
    '.wrfcoin',
    'collector-state',
    'pending-reveals.json',
);
const MINER_ADDRESS_PATTERN = /^0x[0-9a-fA-F]{40}$/;

// ---------------------------------------------------------------------------
// Secret redaction for error logging
// ---------------------------------------------------------------------------
/**
 * Redact sensitive data (tokens, keys, bearer auth) from error bodies
 * before logging to prevent credential leaks in docker logs.
 */
function redactSecrets(body) {
    if (!body) return body;

    let str = typeof body === 'string' ? body : JSON.stringify(body);

    // Redact Bearer tokens
    str = str.replace(/[Bb]earer\s+[A-Za-z0-9._-]+/g, 'Bearer <REDACTED>');

    // Redact apiKey / applicationKey query params (Ambient Weather URLs)
    str = str.replace(/(applicationKey|apiKey|appKey)=[^&\s"']+/gi, '$1=<REDACTED>');

    // Redact long hex/base64 sequences (32+ chars)
    str = str.replace(/([A-Fa-f0-9]|[A-Za-z0-9+/=]){32,}/g, '<TOKEN_REDACTED>');

    // Truncate to 500 chars max
    if (str.length > 500) {
        str = str.slice(0, 500) + '... (truncated)';
    }

    return str;
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------
function httpsGet(url) {
    return new Promise((resolve, reject) => {
        https.get(url, (res) => {
            let raw = '';
            res.on('data', (chunk) => { raw += chunk; });
            res.on('end', () => {
                if (res.statusCode === 429) {
                    resolve({ status: 429, data: null, retryAfter: 2 });
                    return;
                }
                try {
                    resolve({ status: res.statusCode, data: JSON.parse(raw) });
                } catch (_) {
                    resolve({ status: res.statusCode, data: raw });
                }
            });
        }).on('error', reject);
    });
}

function httpGet(host, port, urlPath, token, options = {}) {
    return new Promise((resolve, reject) => {
        const req = http.request({
            hostname: host,
            port,
            path: urlPath,
            method: 'GET',
            headers: {
                ...(token ? { Authorization: `Bearer ${token}` } : {}),
            },
        }, (res) => {
            let raw = '';
            if (Number.isFinite(options.timeoutMs) && options.timeoutMs > 0) res.on('error', reject);
            res.on('data', (chunk) => { raw += chunk; });
            res.on('end', () => {
                try {
                    resolve({ status: res.statusCode, data: JSON.parse(raw) });
                } catch (_) {
                    resolve({ status: res.statusCode, data: raw });
                }
            });
        });
        req.on('error', reject);
        if (Number.isFinite(options.timeoutMs) && options.timeoutMs > 0) {
            const timer = setTimeout(() => req.destroy(new Error('request timeout')), options.timeoutMs);
            req.on('close', () => clearTimeout(timer));
        }
        req.end();
    });
}

function httpPost(host, port, urlPath, body, token) {
    return new Promise((resolve, reject) => {
        const payload = JSON.stringify(body);
        const req = http.request({
            hostname: host,
            port,
            path: urlPath,
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'Content-Length': Buffer.byteLength(payload),
                ...(token ? { Authorization: `Bearer ${token}` } : {}),
            },
        }, (res) => {
            let raw = '';
            res.on('data', (chunk) => { raw += chunk; });
            res.on('end', () => {
                try {
                    resolve({ status: res.statusCode, data: JSON.parse(raw) });
                } catch (_) {
                    resolve({ status: res.statusCode, data: raw });
                }
            });
        });
        req.on('error', reject);
        req.write(payload);
        req.end();
    });
}

function sleep(ms) { return new Promise((r) => setTimeout(r, ms)); }

function isValidMinerAddress(value) {
    return typeof value === 'string' && MINER_ADDRESS_PATTERN.test(value);
}

// ---------------------------------------------------------------------------
// Metrics and structured logging
// ---------------------------------------------------------------------------
function emitFailureMetric(reason, details = {}) {
    const metric = {
        timestamp_ms: Date.now(),
        metric_type: 'collector_fetch_failure',
        reason,
        severity: ['auth', 'invalid_config', 'mine_failed'].includes(reason) ? 'persistent' : 'transient',
        ...details,
    };
    console.error(JSON.stringify(metric));
}

function emitCollectorOutcome(outcome, details = {}) {
    if (!COLLECTOR_CYCLE_OUTCOMES.has(outcome)) {
        throw new Error(`unsupported collector cycle outcome: ${outcome}`);
    }
    const metric = {
        timestamp_ms: Date.now(),
        metric_type: 'collector_cycle_outcome',
        outcome,
        ...details,
    };
    const sink = ['paid', 'upstream_stale', 'pending_reconciliation'].includes(outcome) ? console.log : console.error;
    sink(JSON.stringify(metric));
}

function logStructured(level, message, context = {}) {
    const entry = {
        timestamp: new Date().toISOString(),
        level,
        message,
        component: 'ambient-weather-collector',
        ...context,
    };
    if (level === 'error') {
        console.error(JSON.stringify(entry));
    } else {
        console.log(JSON.stringify(entry));
    }
}

// ---------------------------------------------------------------------------
// HTTP request retry on 429 (rate limit)
// ---------------------------------------------------------------------------
async function postWithRetry(request, host, port, urlPath, body, token, phase) {
    const res = await request(host, port, urlPath, body, token);

    if (res.status === 429) {
        const retryAfter = res.data?.retry_after || 60;
        console.log(`  [${phase}] Rate limited (429), retrying after ${retryAfter}s...`);
        await sleep(retryAfter * 1000);

        const retryRes = await request(host, port, urlPath, body, token);
        if (retryRes.status === 429) {
            console.warn(JSON.stringify({
                phase,
                status: 429,
                retried: true,
                message: 'Rate limited after retry, giving up',
            }));
            return retryRes;
        }
        return retryRes;
    }

    return res;
}

// ---------------------------------------------------------------------------
// Ambient Weather API
// ---------------------------------------------------------------------------
async function fetchDevices(appKey, apiKey) {
    const url = `${AMBIENT_BASE}/v1/devices?applicationKey=${appKey}&apiKey=${apiKey}`;
    for (let attempt = 0; attempt < 3; attempt++) {
        const res = await httpsGet(url);
        if (res.status === 429) {
            console.log(`  Rate limited, waiting ${2 ** (attempt + 1)}s...`);
            await sleep(2000 * (attempt + 1));
            continue;
        }
        return res;
    }
    return { status: 429, data: null };
}

async function fetchDeviceHistory(macAddress, appKey, apiKey, limit) {
    const url = `${AMBIENT_BASE}/v1/devices/${encodeURIComponent(macAddress)}?applicationKey=${appKey}&apiKey=${apiKey}&limit=${limit}`;
    for (let attempt = 0; attempt < 3; attempt++) {
        const res = await httpsGet(url);
        if (res.status === 429) {
            console.log(`  Rate limited, waiting ${2 ** (attempt + 1)}s...`);
            await sleep(2000 * (attempt + 1));
            continue;
        }
        return res;
    }
    return { status: 429, data: null };
}

function selectAmbientReadings(history, lastData) {
    if (Array.isArray(history) && history.length > 0) return history;
    if (lastData && typeof lastData === 'object' && !Array.isArray(lastData)) {
        return [lastData];
    }
    return [];
}

// ---------------------------------------------------------------------------
// NWS API (api.weather.gov — free, no key required)
// ---------------------------------------------------------------------------
function nwsFetch(urlPath) {
    return new Promise((resolve, reject) => {
        const options = {
            hostname: 'api.weather.gov',
            path: urlPath,
            headers: {
                'User-Agent': 'WRFCoin-PWS-Accuracy/1.0 (weather@wrfcoin.org)',
                Accept: 'application/geo+json',
            },
        };
        https.get(options, (res) => {
            let raw = '';
            res.on('data', (chunk) => { raw += chunk; });
            res.on('end', () => {
                try {
                    resolve({ status: res.statusCode, data: JSON.parse(raw) });
                } catch (_) {
                    resolve({ status: res.statusCode, data: raw });
                }
            });
        }).on('error', reject);
    });
}

async function fetchNwsObservations(stationId) {
    const res = await nwsFetch(`/stations/${encodeURIComponent(stationId)}/observations?limit=24`);
    if (res.status !== 200 || !res.data?.features) {
        throw new Error(`NWS API returned HTTP ${res.status} for station ${stationId}`);
    }

    return res.data.features
        .map((f) => {
            const p = f.properties;
            if (!p || !p.timestamp) return null;
            return {
                timestamp_ms: new Date(p.timestamp).getTime(),
                temperature_c: p.temperature?.value ?? null,
                pressure_hpa: p.barometricPressure?.value != null
                    ? p.barometricPressure.value / 100
                    : null,
                humidity_pct: p.relativeHumidity?.value ?? null,
                wind_speed_ms: p.windSpeed?.value != null
                    ? p.windSpeed.value / 3.6
                    : null,
            };
        })
        .filter(Boolean);
}

// ---------------------------------------------------------------------------
// WRFCoin commit/reveal helpers (serde_json BTreeMap key ordering)
// ---------------------------------------------------------------------------
function sortKeys(obj) {
    if (Array.isArray(obj)) return obj.map(sortKeys);
    if (obj !== null && typeof obj === 'object') {
        return Object.keys(obj).sort().reduce((acc, key) => {
            acc[key] = sortKeys(obj[key]);
            return acc;
        }, {});
    }
    return obj;
}

function computeCommitId(data, nonce, stationId, timestamp, contributionType) {
    const dataStr = JSON.stringify(sortKeys(data));
    const preimage = `${dataStr}||${nonce}||${stationId}||${timestamp}||${contributionType}`;
    return crypto.createHash('sha3-256').update(preimage).digest('hex');
}

// ---------------------------------------------------------------------------
// Commit-authorization signing (#1848 / #1967)
// ---------------------------------------------------------------------------
// As of #1848 the mempool rejects ContributionCommit/ForecastCommit txs at
// admission unless the CommitPayload carries a valid `auth_pubkey` +
// `auth_signature` over the canonical authorization message. The collector
// therefore must sign every commit it POSTs. The node-side verify
// (`CommitPayload::verify_authorization`) is NOT sender-bound — any valid
// ed25519 keypair's signature over the correct message passes — so we manage a
// stable collector signing key purely for auditability, not as an identity.
//
// Message contract (must match shared-protocol/src/blockchain/transaction.rs
// `commit_authorization_message_with_domain`, bumped v1->v2 in #2034/#2076 to
// fold the runtime chain identity into the preimage for cross-network replay
// protection — see docs/specifications/SIGNING_PREIMAGE_V2.md):
//   b"wrfcoin_commit_auth_v2"
//   ++ u32_LE(chain_id.len)          ++ chain_id_utf8      (chain_domain)
//   ++ u32_LE(network_id.len)        ++ network_id_utf8    (chain_domain)
//   ++ u32_LE(station_id.len)        ++ station_id_utf8
//   ++ u32_LE(commit_id.len)         ++ commit_id_utf8
//   ++ u32_LE(contribution_type.len) ++ contribution_type_utf8
//
// chain_id/network_id are resolved the same way the Rust signer/verifier
// resolve them (`crypto::domain_signing::signing_chain_id`/
// `signing_network_id`): read `WRFCOIN_CHAIN_ID`/`WRFCOIN_NETWORK_ID` from the
// environment; empty/unset falls back to the mainnet constants. The collector
// process MUST run with the deployment's env vars set (e.g.
// `WRFCOIN_CHAIN_ID=777` / `WRFCOIN_NETWORK_ID=testnet` for this testnet) so it
// resolves the same identity the node verifies against.
//
// Crypto interop: ed25519-dalek raw-hex. `auth_pubkey` = raw 32-byte public
// key hex; `auth_signature` = raw 64-byte signature hex (Ed25519 over the raw
// message bytes via Node's `crypto.sign(null, ...)`).

const COMMIT_AUTH_DOMAIN = Buffer.from('wrfcoin_commit_auth_v2', 'utf8');

// Mirrors shared-protocol/src/crypto/domain_signing.rs DEFAULT_CHAIN_ID /
// DEFAULT_NETWORK_ID / CHAIN_ID_ENV / NETWORK_ID_ENV exactly (#2034/#2076).
const DEFAULT_CHAIN_ID = 'wrfcoin-main';
const DEFAULT_NETWORK_ID = 'mainnet';
const CHAIN_ID_ENV = 'WRFCOIN_CHAIN_ID';
const NETWORK_ID_ENV = 'WRFCOIN_NETWORK_ID';

const COLLECTOR_SIGNING_KEY_PATH = path.join(
    os.homedir(),
    '.wrfcoin',
    'collector-state',
    'collector-signing-key.json',
);

function u32le(n) {
    const b = Buffer.allocUnsafe(4);
    b.writeUInt32LE(n >>> 0, 0);
    return b;
}

/**
 * Resolve the runtime chain id folded into signing preimages (#2034/#2076).
 * Mirrors `crypto::domain_signing::signing_chain_id` exactly: a non-empty
 * `WRFCOIN_CHAIN_ID` wins, otherwise the mainnet fallback.
 */
function signingChainId() {
    const v = process.env[CHAIN_ID_ENV];
    return (typeof v === 'string' && v.trim() !== '') ? v : DEFAULT_CHAIN_ID;
}

/**
 * Resolve the runtime network id folded into signing preimages (#2034/#2076).
 * Mirrors `crypto::domain_signing::signing_network_id` exactly.
 */
function signingNetworkId() {
    const v = process.env[NETWORK_ID_ENV];
    return (typeof v === 'string' && v.trim() !== '') ? v : DEFAULT_NETWORK_ID;
}

/**
 * Fold the runtime chain identity `(chain_id, network_id)` into the preimage,
 * length-prefixed. Mirrors `push_chain_domain` in
 * shared-protocol/src/blockchain/transaction.rs exactly (#2034/#2076).
 */
function pushChainDomain(chainId, networkId) {
    const chainBuf = Buffer.from(String(chainId), 'utf8');
    const netBuf = Buffer.from(String(networkId), 'utf8');
    return Buffer.concat([
        u32le(chainBuf.length), chainBuf,
        u32le(netBuf.length), netBuf,
    ]);
}

/**
 * Build the canonical commit-authorization message (the bytes to sign) with an
 * explicit chain identity. Mirrors
 * `commit_authorization_message_with_domain` in shared-protocol exactly
 * (#2034/#2076) — the deterministic core used by golden-vector tests.
 */
function commitAuthorizationMessageWithDomain(chainId, networkId, stationId, commitId, contributionType) {
    const stationBuf = Buffer.from(String(stationId), 'utf8');
    const commitBuf = Buffer.from(String(commitId), 'utf8');
    const typeBuf = Buffer.from(String(contributionType), 'utf8');
    return Buffer.concat([
        COMMIT_AUTH_DOMAIN,
        pushChainDomain(chainId, networkId),
        u32le(stationBuf.length), stationBuf,
        u32le(commitBuf.length), commitBuf,
        u32le(typeBuf.length), typeBuf,
    ]);
}

/**
 * Build the canonical commit-authorization message (the bytes to sign),
 * resolving the chain identity from the environment. Mirrors
 * `commit_authorization_message` in shared-protocol exactly (#2034/#2076).
 */
function commitAuthorizationMessage(stationId, commitId, contributionType) {
    return commitAuthorizationMessageWithDomain(
        signingChainId(),
        signingNetworkId(),
        stationId,
        commitId,
        contributionType,
    );
}

/**
 * Extract the raw 32-byte ed25519 public key (hex) from a Node KeyObject.
 * Node exposes the public key via JWK `x` (base64url of the raw point).
 */
function rawEd25519PublicKeyHex(publicKeyObject) {
    const jwk = publicKeyObject.export({ format: 'jwk' });
    return Buffer.from(jwk.x, 'base64url').toString('hex');
}

/**
 * Load (or lazily create + persist) a stable ed25519 signing key for the
 * collector. Key resolution order:
 *   1. COLLECTOR_SIGNING_SEED env var — deterministic 32-byte seed (hex or any
 *      string; hashed to 32 bytes with SHA-256 if not 64 hex chars). Stable
 *      across machines without a key file.
 *   2. A persisted key file under the collector data dir (reused across runs).
 *   3. A freshly generated key, persisted to that file.
 *
 * Returns { privateKey: KeyObject, publicKey: KeyObject, pubHex }.
 */
function loadCollectorSigningKey(options = {}) {
    // Test injection hook.
    if (options.signingKey) return options.signingKey;

    const seed = options.signingSeed
        || (typeof process !== 'undefined' && process.env
            ? process.env.COLLECTOR_SIGNING_SEED
            : undefined);

    if (seed) {
        const seedBytes = /^[0-9a-fA-F]{64}$/.test(seed)
            ? Buffer.from(seed, 'hex')
            : crypto.createHash('sha256').update(String(seed), 'utf8').digest();
        const key = keyPairFromSeed(seedBytes);
        return key;
    }

    const keyPath = options.signingKeyPath
        || (typeof process !== 'undefined' && process.env
            ? process.env.COLLECTOR_SIGNING_KEY_FILE
            : undefined)
        || COLLECTOR_SIGNING_KEY_PATH;

    // Reuse a persisted key if present.
    try {
        if (fs.existsSync(keyPath)) {
            const raw = JSON.parse(fs.readFileSync(keyPath, 'utf8'));
            if (raw && typeof raw.seed_hex === 'string' && /^[0-9a-fA-F]{64}$/.test(raw.seed_hex)) {
                return keyPairFromSeed(Buffer.from(raw.seed_hex, 'hex'));
            }
        }
    } catch (_) {
        // Fall through to generation on any read/parse error.
    }

    // Generate a fresh stable key and persist it.
    const seedBytes = crypto.randomBytes(32);
    const key = keyPairFromSeed(seedBytes);
    try {
        fs.mkdirSync(path.dirname(keyPath), { recursive: true });
        fs.writeFileSync(
            keyPath,
            JSON.stringify({ seed_hex: seedBytes.toString('hex'), pub_hex: key.pubHex }, null, 2),
            { mode: 0o600 },
        );
    } catch (_) {
        // Persistence is best-effort; a non-persisted key still authorizes.
    }
    return key;
}

/**
 * Build a Node ed25519 KeyObject pair from a raw 32-byte seed. Wraps the seed
 * in a PKCS#8 DER envelope (the fixed Ed25519 prefix + the 32-byte seed).
 */
function keyPairFromSeed(seedBytes) {
    if (!Buffer.isBuffer(seedBytes) || seedBytes.length !== 32) {
        throw new Error('ed25519 seed must be exactly 32 bytes');
    }
    // PKCS#8 prefix for an Ed25519 private key (RFC 8410). The trailing 32
    // bytes are the seed.
    const pkcs8Prefix = Buffer.from('302e020100300506032b657004220420', 'hex');
    const der = Buffer.concat([pkcs8Prefix, seedBytes]);
    const privateKey = crypto.createPrivateKey({ key: der, format: 'der', type: 'pkcs8' });
    const publicKey = crypto.createPublicKey(privateKey);
    return { privateKey, publicKey, pubHex: rawEd25519PublicKeyHex(publicKey) };
}

/**
 * Produce the {auth_pubkey, auth_signature} pair authorizing a commit.
 * `auth_signature` is the raw 64-byte ed25519 signature (hex) over the
 * canonical message; `auth_pubkey` is the raw 32-byte public key (hex).
 */
function signCommitAuthorization(stationId, commitId, contributionType, options = {}) {
    const key = loadCollectorSigningKey(options);
    const message = commitAuthorizationMessage(stationId, commitId, contributionType);
    const signature = crypto.sign(null, message, key.privateKey);
    return {
        auth_pubkey: key.pubHex,
        auth_signature: Buffer.from(signature).toString('hex'),
    };
}

function computePendingRevealReadingId(reading, stationId, timestamp) {
    if (!reading) return null;
    const payloadHash = crypto
        .createHash('sha3-256')
        .update(JSON.stringify(sortKeys(reading)))
        .digest('hex');
    const preimage = `${stationId}||${timestamp}||${payloadHash}`;
    return crypto.createHash('sha3-256').update(preimage).digest('hex');
}

function computeNextLastSubmittedTs(previousLastTs, successfulReadings) {
    const prior = Number.isFinite(previousLastTs) ? previousLastTs : 0;
    const newestThisCycle = successfulReadings.reduce((max, reading) => {
        const ts = reading?.recorded_at ? new Date(reading.recorded_at).getTime() : 0;
        return Math.max(max, ts);
    }, 0);
    return Math.max(prior, newestThisCycle);
}

function getReadingTimestamp(reading, now = Date.now) {
    return reading.recorded_at
        ? Math.floor(new Date(reading.recorded_at).getTime() / 1000)
        : Math.floor(now() / 1000);
}

// ---------------------------------------------------------------------------
// Pending reveal durable state
// ---------------------------------------------------------------------------
function getPendingRevealsPath(options = {}) {
    return options.stateFilePath
        || process.env.WRFCOIN_COLLECTOR_STATE_FILE
        || COLLECTOR_PENDING_REVEALS_PATH;
}

function normalizePendingRevealEntry(entry) {
    if (!entry || typeof entry !== 'object' || Array.isArray(entry)) return null;
    if (!entry.commit_hash || !entry.nonce || !entry.reading) return null;

    return {
        commit_hash: String(entry.commit_hash),
        nonce: String(entry.nonce),
        reading: entry.reading,
        reading_id: entry.reading_id
            ? String(entry.reading_id)
            : computePendingRevealReadingId(entry.reading, entry.station_id, entry.timestamp),
        commit_block_height: Number.isFinite(entry.commit_block_height)
            ? entry.commit_block_height
            : null,
        station_id: entry.station_id ? String(entry.station_id) : null,
        contributor: entry.contributor ? String(entry.contributor) : null,
        timestamp: Number.isFinite(entry.timestamp) ? entry.timestamp : null,
        quality_tier: entry.quality_tier || null,
        phase: entry.phase === PENDING_PHASE_AWAITING_QUORUM
            ? PENDING_PHASE_AWAITING_QUORUM
            : PENDING_PHASE_COMMITTED,
        reveal_submitted_at_height: Number.isFinite(entry.reveal_submitted_at_height)
            ? entry.reveal_submitted_at_height
            : null,
        // #3266: consecutive collector cycles this reveal was polled without
        // reaching quorum. Durable so a restart cannot reset the bound.
        timeout_cycles: Number.isSafeInteger(entry.timeout_cycles) && entry.timeout_cycles > 0
            ? entry.timeout_cycles
            : 0,
    };
}

function loadPendingRevealEntries(stateFilePath) {
    try {
        const raw = fs.readFileSync(stateFilePath, 'utf8');
        const parsed = JSON.parse(raw);
        const entries = Array.isArray(parsed)
            ? parsed
            : Array.isArray(parsed?.pending)
                ? parsed.pending
                : Array.isArray(parsed?.pending_reveals)
                    ? parsed.pending_reveals
                    : [];
        return entries.map(normalizePendingRevealEntry).filter(Boolean);
    } catch (_) {
        return [];
    }
}

function savePendingRevealEntries(stateFilePath, entries) {
    const dir = path.dirname(stateFilePath);
    try {
        fs.mkdirSync(dir, { recursive: true });
    } catch (_) {}

    const normalized = [...entries]
        .map(normalizePendingRevealEntry)
        .filter(Boolean)
        .reduce((acc, entry) => {
            acc.set(entry.reading_id, entry);
            return acc;
        }, new Map())
        .values();

    const sorted = [...normalized]
        .sort((a, b) => {
            const aHeight = Number.isFinite(a.commit_block_height) ? a.commit_block_height : Number.MAX_SAFE_INTEGER;
            const bHeight = Number.isFinite(b.commit_block_height) ? b.commit_block_height : Number.MAX_SAFE_INTEGER;
            if (aHeight !== bHeight) return aHeight - bHeight;
            return a.commit_hash.localeCompare(b.commit_hash);
        });

    const tmpPath = `${stateFilePath}.tmp`;
    fs.writeFileSync(tmpPath, JSON.stringify(sorted, null, 2));
    fs.renameSync(tmpPath, stateFilePath);
}

function getTerminalOutcomesPath(options = {}) {
    if (options.terminalOutcomesPath) return options.terminalOutcomesPath;
    return path.join(path.dirname(getPendingRevealsPath(options)), 'terminal-outcomes.json');
}

function loadTerminalOutcomes(options = {}) {
    try {
        const parsed = JSON.parse(fs.readFileSync(getTerminalOutcomesPath(options), 'utf8'));
        return Array.isArray(parsed) ? parsed.filter((item) => item && item.reading_id) : [];
    } catch (_) {
        return [];
    }
}

function recordTerminalOutcomes(entries, outcome, options = {}, reasonFor = null) {
    if (entries.length === 0) return;
    const outputPath = getTerminalOutcomesPath(options);
    fs.mkdirSync(path.dirname(outputPath), { recursive: true });
    const byReading = new Map(loadTerminalOutcomes(options).map((item) => [String(item.reading_id), item]));
    const recordedAt = new Date().toISOString();
    for (const entry of entries) {
        byReading.set(String(entry.reading_id), {
            reading_id: String(entry.reading_id),
            commit_hash: String(entry.commit_hash),
            station_id: entry.station_id ? String(entry.station_id) : null,
            outcome,
            ...(typeof reasonFor === 'function' && reasonFor(entry)
                ? { reason: String(reasonFor(entry)) }
                : {}),
            recorded_at: recordedAt,
        });
    }
    const tmpPath = `${outputPath}.tmp`;
    fs.writeFileSync(tmpPath, JSON.stringify([...byReading.values()], null, 2));
    fs.renameSync(tmpPath, outputPath);
}

function upsertPendingRevealEntry(stateFilePath, entry) {
    const normalizedEntry = normalizePendingRevealEntry(entry);
    if (!normalizedEntry) return;
    const existing = loadPendingRevealEntries(stateFilePath);
    const next = existing.filter((item) => item.reading_id !== normalizedEntry.reading_id);
    next.push(normalizedEntry);
    savePendingRevealEntries(stateFilePath, next);
}

function removePendingRevealEntry(stateFilePath, commitHash, readingId = null) {
    const existing = loadPendingRevealEntries(stateFilePath);
    const next = existing.filter((item) => item.commit_hash !== commitHash && item.reading_id !== readingId);
    savePendingRevealEntries(stateFilePath, next);
}

// ---------------------------------------------------------------------------
// Per-station last-submitted-timestamp watermark
// ---------------------------------------------------------------------------
// Mirrors the single-account collector's `last-submitted-ts.txt` dedup, but
// keyed per stationId so the multi-tenant loop does not re-commit each station's
// overlapping Ambient history every run (which would fail reveal as stale and
// clog pending-reveal state). Stored next to the pending-reveals state file as
// `last-submitted-ts-<stationId>.txt` (stationId sanitized for filesystem safety).

function sanitizeStationIdForFile(stationId) {
    return String(stationId == null ? 'unknown' : stationId).replace(/[^A-Za-z0-9._-]/g, '_');
}

function getStationWatermarkPath(stationId, options = {}) {
    if (options.watermarkPath) return options.watermarkPath;
    const stateFilePath = getPendingRevealsPath(options);
    const dir = path.dirname(stateFilePath);
    return path.join(dir, `last-submitted-ts-${sanitizeStationIdForFile(stationId)}.txt`);
}

function readStationWatermark(stationId, options = {}) {
    const watermarkPath = getStationWatermarkPath(stationId, options);
    try {
        const parsed = parseInt(fs.readFileSync(watermarkPath, 'utf8'), 10);
        return Number.isFinite(parsed) ? parsed : 0;
    } catch (_) {
        return 0;
    }
}

function writeStationWatermark(stationId, tsMs, options = {}) {
    if (!Number.isFinite(tsMs) || tsMs <= 0) return;
    const watermarkPath = getStationWatermarkPath(stationId, options);
    try {
        fs.mkdirSync(path.dirname(watermarkPath), { recursive: true });
    } catch (_) {}
    try {
        const existing = readStationWatermark(stationId, options);
        if (tsMs > existing) {
            const tmpPath = `${watermarkPath}.tmp`;
            fs.writeFileSync(tmpPath, String(tsMs));
            fs.renameSync(tmpPath, watermarkPath);
        }
    } catch (_) { /* best-effort; dedup degrades gracefully */ }
}

function createPendingRevealEntry(reading, stationId, contributor, commitHash, nonce, commitBlockHeight, timestamp, qualityTier = null) {
    return {
        commit_hash: commitHash,
        nonce,
        reading: sortKeys(reading),
        commit_block_height: Number.isFinite(commitBlockHeight) ? commitBlockHeight : null,
        station_id: stationId,
        contributor,
        timestamp,
        quality_tier: qualityTier,
        phase: PENDING_PHASE_COMMITTED,
        reveal_submitted_at_height: null,
    };
}

// ---------------------------------------------------------------------------
// Node interaction
// ---------------------------------------------------------------------------
async function getBlockchainHeight(options = {}) {
    const request = options.httpGet || httpGet;
    const host = options.host;
    const port = options.port;
    const token = options.token;
    try {
        const res = await request(host, port, '/blockchain', token);
        if (res.status !== 200 || !res.data) return null;
        const chain = res.data.data || res.data;
        return Number.isFinite(chain?.height) ? chain.height : null;
    } catch (_) {
        return null;
    }
}

async function submitContributionCommit(reading, stationId, contributor, options = {}) {
    const request = options.httpPost || httpPost;
    const randomBytes = options.randomBytes || crypto.randomBytes;
    const now = options.now || Date.now;
    const host = options.host;
    const port = options.port;
    const token = options.token;
    const sorted = options.sortedReading || sortKeys(reading);
    const nonce = Object.prototype.hasOwnProperty.call(options, 'nonce')
        ? options.nonce
        : randomBytes(16).toString('hex');
    const timestamp = Object.prototype.hasOwnProperty.call(options, 'timestamp')
        ? options.timestamp
        : getReadingTimestamp(reading, now);
    const commitHash = options.commitHash
        || computeCommitId(sorted, nonce, stationId, timestamp, 'contribution');
    const commitBlockHeight = Object.prototype.hasOwnProperty.call(options, 'commitBlockHeight')
        ? options.commitBlockHeight
        : null;

    // #1848/#1967: the mempool rejects unsigned commits at admission. Sign the
    // canonical commit-authorization message and attach the auth fields. The
    // node parses commit_id=commit_hash, contribution_type="contribution", and
    // station_id=station_id for the ContributionCommit tx, so we sign over the
    // same triple (see launch_api submit_contribution_commit).
    const auth = signCommitAuthorization(stationId, commitHash, 'contribution', options);

    try {
        const commitRes = await postWithRetry(
            request,
            host,
            port,
            '/transactions/contribution/commit',
            {
                contributor,
                station_id: stationId,
                commit_hash: commitHash,
                auth_pubkey: auth.auth_pubkey,
                auth_signature: auth.auth_signature,
            },
            token,
            'commit',
        );
        if (commitRes.status !== 201 && commitRes.status !== 200) {
            return { ok: false, phase: 'commit', status: commitRes.status, detail: commitRes.data };
        }
    } catch (error) {
        return { ok: false, phase: 'commit', status: null, detail: { error: error.message } };
    }

    return { ok: true, commitHash, nonce, timestamp, payload: sorted, commitBlockHeight };
}

async function submitContributionReveal(reading, stationId, contributor, nonce, commitHash, timestamp, options = {}) {
    const request = options.httpPost || httpPost;
    const host = options.host;
    const port = options.port;
    const token = options.token;
    const sorted = options.sortedReading || sortKeys(reading);
    const qualityTier = options.qualityTier || null;

    try {
        const revealRes = await postWithRetry(
            request,
            host,
            port,
            '/transactions/contribution/reveal',
            {
                contributor,
                station_id: stationId,
                payload: sorted,
                nonce,
                commit_hash: commitHash,
                timestamp,
                ...(qualityTier != null ? { quality_tier: qualityTier } : {}),
            },
            token,
            'reveal',
        );
        if (revealRes.status !== 201 && revealRes.status !== 200) {
            return { ok: false, phase: 'reveal', status: revealRes.status, detail: revealRes.data };
        }
    } catch (error) {
        return { ok: false, phase: 'reveal', status: null, detail: { error: error.message } };
    }

    return { ok: true };
}

async function processPendingReveals(options = {}) {
    const stateFilePath = getPendingRevealsPath(options);
    const currentBlockHeight = Object.prototype.hasOwnProperty.call(options, 'currentBlockHeight')
        ? options.currentBlockHeight
        : await getBlockchainHeight(options);
    const pending = loadPendingRevealEntries(stateFilePath);
    const revealedEntries = [];
    const failedEntries = [];
    const droppedEntries = [];

    if (pending.length === 0) {
        return { attempted: 0, revealed: 0, failed: 0, dropped: 0, revealedEntries, failedEntries, droppedEntries };
    }

    const requestOptions = { ...options, stateFilePath, currentBlockHeight };

    for (const entry of pending) {
        if (
            Number.isFinite(currentBlockHeight)
            && Number.isFinite(entry.commit_block_height)
            && (currentBlockHeight - entry.commit_block_height) > PENDING_REVEAL_MAX_AGE_BLOCKS
        ) {
            const ageBlocks = currentBlockHeight - entry.commit_block_height;
            emitFailureMetric('audit_loss', {
                reason: 'pending_reveal_too_old',
                commit_hash: entry.commit_hash,
                commit_block_height: entry.commit_block_height,
                current_block_height: currentBlockHeight,
                age_blocks: ageBlocks,
                max_age_blocks: PENDING_REVEAL_MAX_AGE_BLOCKS,
                station_id: entry.station_id,
                contributor: entry.contributor,
            });
            logStructured('warn', 'Dropping expired pending reveal', {
                commit_hash: entry.commit_hash,
                age_blocks: ageBlocks,
                station_id: entry.station_id,
                contributor: entry.contributor,
            });
            droppedEntries.push(entry);
            continue;
        }

        const result = await submitContributionReveal(
            entry.reading,
            entry.station_id,
            entry.contributor,
            entry.nonce,
            entry.commit_hash,
            entry.timestamp,
            { ...requestOptions, qualityTier: entry.quality_tier || null },
        );

        if (result.ok) {
            revealedEntries.push(entry);
            const dateStr = entry.reading?.recorded_at
                ? new Date(entry.reading.recorded_at).toLocaleTimeString()
                : 'pending';
            console.log(`  PENDING REVEAL OK: ${entry.reading?.temperature_c ?? '?'}°C @ ${dateStr}`);
        } else {
            failedEntries.push(entry);
            console.error(`  PENDING REVEAL FAIL (${result.phase || 'reveal'} ${result.status ?? 'n/a'}): ${redactSecrets(result.detail).slice(0, 300)}`);
        }
    }

    try {
        savePendingRevealEntries(stateFilePath, pending.filter((entry) =>
            !revealedEntries.some((done) => done.reading_id === entry.reading_id)
            && !droppedEntries.some((dropped) => dropped.reading_id === entry.reading_id)
        ));
    } catch (error) {
        logStructured('warn', 'Failed to update pending reveal state file', {
            error_message: error.message,
            state_file: stateFilePath,
        });
    }

    return {
        attempted: pending.length,
        revealed: revealedEntries.length,
        failed: failedEntries.length,
        dropped: droppedEntries.length,
        revealedEntries,
        failedEntries,
        droppedEntries,
    };
}

async function submitCommitReveal(reading, stationId, contributor, options = {}) {
    const stateFilePath = getPendingRevealsPath(options);
    const commitBlockHeight = Object.prototype.hasOwnProperty.call(options, 'commitBlockHeight')
        ? options.commitBlockHeight
        : await getBlockchainHeight(options);
    const commitResult = await submitContributionCommit(reading, stationId, contributor, {
        ...options,
        commitBlockHeight,
    });

    if (!commitResult.ok) return commitResult;

    const pendingEntry = createPendingRevealEntry(
        reading,
        stationId,
        contributor,
        commitResult.commitHash,
        commitResult.nonce,
        commitBlockHeight,
        commitResult.timestamp,
        options.qualityTier || null,
    );

    try {
        upsertPendingRevealEntry(stateFilePath, pendingEntry);
    } catch (error) {
        logStructured('warn', 'Failed to persist pending reveal entry before reveal', {
            commit_hash: commitResult.commitHash,
            state_file: stateFilePath,
            error_message: error.message,
        });
    }

    const revealResult = await submitContributionReveal(
        reading,
        stationId,
        contributor,
        commitResult.nonce,
        commitResult.commitHash,
        commitResult.timestamp,
        options,
    );

    if (!revealResult.ok) return revealResult;

    try {
        removePendingRevealEntry(stateFilePath, commitResult.commitHash, pendingEntry.reading_id);
    } catch (error) {
        logStructured('warn', 'Failed to clear pending reveal after success', {
            commit_hash: commitResult.commitHash,
            state_file: stateFilePath,
            error_message: error.message,
        });
    }

    return { ok: true, commitHash: commitResult.commitHash, timestamp: commitResult.timestamp };
}

/**
 * Persist a commit for the attestation-gated two-block path.  Unlike the
 * legacy submitCommitReveal helper above, this deliberately does not submit a
 * reveal: the commit must first be applied by a mined block.
 */
async function submitCommitForAttestation(reading, stationId, contributor, options = {}) {
    const stateFilePath = getPendingRevealsPath(options);
    const commitBlockHeight = Object.prototype.hasOwnProperty.call(options, 'commitBlockHeight')
        ? options.commitBlockHeight
        : await getBlockchainHeight(options);
    const commitResult = await submitContributionCommit(reading, stationId, contributor, {
        ...options,
        commitBlockHeight,
    });
    if (!commitResult.ok) return commitResult;

    const pendingEntry = createPendingRevealEntry(
        reading,
        stationId,
        contributor,
        commitResult.commitHash,
        commitResult.nonce,
        commitBlockHeight,
        commitResult.timestamp,
        options.qualityTier || null,
    );
    try {
        upsertPendingRevealEntry(stateFilePath, pendingEntry);
    } catch (error) {
        // Losing the durable handoff here would make restart duplicate the
        // commit and could later advance a watermark for work that was never
        // quorum-qualified. Treat persistence as a hard submission failure.
        return {
            ok: false,
            phase: 'persist_commit',
            status: null,
            detail: { error: error.message },
        };
    }
    return {
        ok: true,
        commitHash: commitResult.commitHash,
        timestamp: commitResult.timestamp,
        entry: normalizePendingRevealEntry(pendingEntry),
    };
}

async function fetchPendingAttestations(options = {}) {
    const request = options.httpGet || httpGet;
    let response;
    try {
        response = await request(
            options.host,
            options.port,
            '/transactions/oracle/pending?limit=500',
            options.token,
        );
    } catch (error) {
        return { ok: false, reason: 'request_failed', status: null, detail: error.message };
    }
    if (response.status === 401 || response.status === 403) {
        return { ok: false, reason: 'auth', status: response.status, detail: response.data };
    }
    if (response.status !== 200) {
        return { ok: false, reason: 'http', status: response.status, detail: response.data };
    }
    const body = response.data?.data || response.data;
    if (
        !body
        || !Array.isArray(body.observations)
        || !Number.isInteger(body.effective_quorum)
        || body.effective_quorum < 1
    ) {
        return { ok: false, reason: 'invalid_response', status: response.status, detail: response.data };
    }
    return {
        ok: true,
        observations: body.observations,
        readyToMine: Array.isArray(body.ready_to_mine) ? body.ready_to_mine : [],
        effectiveQuorum: body.effective_quorum,
        height: Number.isFinite(body.height) ? body.height : null,
        unrecoverable: Array.isArray(body.unrecoverable) ? body.unrecoverable : [],
    };
}

/**
 * Read the canonical public contribution status for a locally persisted
 * awaiting-quorum entry.  The pending-attestation API intentionally omits
 * entries once their reveal is in canonical history, so absence there cannot
 * distinguish a visibility delay from an already-settled historical reveal.
 * Only a commit-bound, station-bound, finalized status is terminal enough to
 * reconcile local state.
 */
async function fetchCanonicalContributionStatus(entry, options = {}) {
    const request = options.httpGet || httpGet;
    const commitId = String(entry?.commit_hash || '');
    if (!commitId) return { settled: false, reason: 'invalid_entry' };
    let response;
    try {
        response = await request(
            options.host,
            options.port,
            `/transactions/commit/${encodeURIComponent(commitId)}/status`,
            null,
        );
    } catch (error) {
        return { settled: false, reason: 'request_failed', detail: error.message };
    }
    if (response.status !== 200) {
        return { settled: false, reason: response.status === 404 ? 'not_found' : 'http', status: response.status };
    }
    const body = response.data?.data || response.data;
    if (
        !body
        || String(body.commit_id || '') !== commitId
        || String(body.station_id || '') !== String(entry.station_id || '')
    ) {
        // Exact commit-ID and station-ID binding (#2800). Neither accepted
        // form below is consulted for a response about some other commit or
        // station. The body travels with the result: expiry recovery (#2810)
        // reads it to preserve entries whose reveal is visibly applied.
        return { settled: false, reason: 'not_finalized', status: body };
    }

    // Form 1: the reveal is still in canonical applied_reveals — finalized,
    // with an integer reveal height and a revealed_* status.
    const revealedForm = body.reward_finalized === true
        && Number.isInteger(body.revealed_at)
        && body.revealed_at >= 1
        && typeof body.status === 'string'
        && body.status.startsWith('revealed_');

    // Form 2 (#2801): canonical applied_reveals are pruned 10,000 blocks
    // after application, after which the route serves the DURABLE terminal
    // form: status "reward_finalized", revealed_at null, and the station
    // identity sourced from the canonical reward record itself. Accept it
    // only when that reward state proves finalization: reward_finalized true
    // AND a terminal reward_status (the three values the route can only emit
    // for a finalized Some(record)). The station binding above is required
    // to be NON-EMPTY here — with the reveal gone, the reward record's
    // station identity is the only corroboration left, so an entry or
    // response without one stays fail-closed.
    const TERMINAL_REWARD_STATUSES = ['paid', 'finalized_zero', 'excluded_non_real'];
    const prunedTerminalForm = body.reward_finalized === true
        && body.status === 'reward_finalized'
        && (body.revealed_at === null || body.revealed_at === undefined)
        && TERMINAL_REWARD_STATUSES.includes(String(body.reward_status || ''))
        && String(entry.station_id || '') !== ''
        && String(body.station_id || '') !== '';

    if (revealedForm) {
        return { settled: true, reason: 'canonical_finalized', status: body };
    }
    if (prunedTerminalForm) {
        return { settled: true, reason: 'canonical_finalized_pruned', status: body };
    }
    return { settled: false, reason: 'not_finalized', status: body };
}

async function reconcileCanonicallySettledEntries(entries, options = {}) {
    const settledEntries = [];
    const paidEntries = [];
    const terminalEntries = [];
    const classifications = [];
    for (const entry of entries) {
        const canonical = await fetchCanonicalContributionStatus(entry, options);
        if (!canonical.settled) continue;
        settledEntries.push(entry);
        const rewardAmount = Number(canonical.status.reward_amount || 0);
        if (canonical.status.reward_status === 'paid' && rewardAmount > 0) {
            paidEntries.push(entry);
            classifications.push({
                outcome: 'paid',
                station_id: String(entry.station_id || ''),
                commit_id: String(entry.commit_hash),
                reward_status: 'paid',
                reward_amount: rewardAmount,
            });
            emitCollectorOutcome('paid', {
                commit_hash: entry.commit_hash,
                station_id: entry.station_id,
                reward_amount: rewardAmount,
            });
        } else {
            terminalEntries.push(entry);
            classifications.push({
                outcome: 'rejected',
                reason: 'canonical_non_paid',
                station_id: String(entry.station_id || ''),
                commit_id: String(entry.commit_hash),
                reward_status: canonical.status.reward_status || null,
                reward_amount: Number.isFinite(rewardAmount) ? rewardAmount : 0,
            });
            emitCollectorOutcome('rejected', {
                commit_hash: entry.commit_hash,
                station_id: entry.station_id,
                reason: canonical.status.reward_status || 'finalized_non_paid',
            });
        }
        logStructured('info', 'Reconciled canonically finalized collector entry', {
            commit_hash: entry.commit_hash,
            station_id: entry.station_id,
            // null when the applied reveal has been pruned (#2801) — the
            // reason field distinguishes the two accepted forms.
            revealed_at: canonical.status.revealed_at ?? null,
            reward_status: canonical.status.reward_status || null,
            reason: canonical.reason,
        });
    }
    if (settledEntries.length > 0) {
        const stateFilePath = getPendingRevealsPath(options);
        removePendingRevealEntries(stateFilePath, settledEntries);
        advanceWatermarksForEntries(paidEntries, options);
        recordTerminalOutcomes(terminalEntries, 'finalized_non_paid', options);
    }
    const stationOutcomes = [];
    const stationIds = [...new Set(classifications.map((item) => item.station_id))];
    for (const stationId of stationIds) {
        const stationItems = classifications.filter((item) => item.station_id === stationId);
        const rejected = stationItems.filter((item) => item.outcome === 'rejected');
        if (rejected.length > 0) {
            stationOutcomes.push({
                outcome: 'rejected',
                reason: 'canonical_non_paid',
                station_id: stationId,
                commit_ids: rejected.map((item) => item.commit_id),
                reward_statuses: [...new Set(rejected.map((item) => item.reward_status))],
            });
            continue;
        }
        stationOutcomes.push({
            outcome: 'paid',
            reason: 'canonical_positive_reward',
            station_id: stationId,
            commit_ids: stationItems.map((item) => item.commit_id),
            reward_status: 'paid',
            reward_amount: stationItems.reduce((sum, item) => sum + item.reward_amount, 0),
        });
    }
    return { settledEntries, stationOutcomes };
}

async function reconcileExpiredAwaitingEntries(entries, currentBlockHeight, options = {}) {
    if (!Number.isFinite(currentBlockHeight)) return [];
    const expired = entries.filter((entry) =>
        Number.isFinite(entry.commit_block_height)
        && (currentBlockHeight - entry.commit_block_height) > PENDING_REVEAL_MAX_AGE_BLOCKS
    );
    if (expired.length === 0) return [];

    const pending = await fetchPendingAttestations(options);
    if (!pending.ok) return [];
    // Absence is only meaningful on a complete page. At the API cap an older
    // live reveal may simply be outside this response, so terminal recovery
    // must remain fail-closed until the page is no longer saturated.
    if (
        pending.observations.length >= PENDING_ATTESTATIONS_PAGE_LIMIT
        || pending.readyToMine.length >= PENDING_ATTESTATIONS_PAGE_LIMIT
    ) return [];
    const visible = new Set(
        [...pending.observations, ...pending.readyToMine].map((item) => String(item.commit_id)),
    );
    const unrecoverable = new Set(pending.unrecoverable.map(String));
    const terminal = [];
    for (const entry of expired) {
        if (visible.has(entry.commit_hash)) continue;
        const canonical = await fetchCanonicalContributionStatus(entry, options);
        if (canonical.settled || ['request_failed', 'http'].includes(canonical.reason)) continue;
        if (
            canonical.status
            && String(canonical.status.station_id || '') === String(entry.station_id || '')
            && Number.isInteger(canonical.status.revealed_at)
            && canonical.status.revealed_at > 0
        ) continue;
        // An entry older than the protocol window, absent from both explicit
        // pending lanes, and not canonically finalized cannot become payable.
        // Preserve it as a terminal outcome so overlapping upstream history
        // cannot silently recommit it on every collector cycle.
        terminal.push(entry);
        emitCollectorOutcome('rejected', {
                reason: unrecoverable.has(entry.commit_hash)
                    ? 'reveal_preimage_unrecoverable'
                    : 'expired_unrecoverable',
                commit_hash: entry.commit_hash,
                station_id: entry.station_id,
                age_blocks: currentBlockHeight - entry.commit_block_height,
            },
        );
    }
    if (terminal.length > 0) {
        recordTerminalOutcomes(terminal, 'expired_unrecoverable', options);
        removePendingRevealEntries(getPendingRevealsPath(options), terminal);
    }
    return terminal;
}

function resolvePositiveIntegerSetting(optionValue, envName, fallback) {
    if (Number.isSafeInteger(optionValue) && optionValue > 0) return optionValue;
    const configured = parseInt(process.env[envName] || '', 10);
    return Number.isSafeInteger(configured) && configured > 0 ? configured : fallback;
}

function resolveAttestationFreshnessWindowSecs(options = {}) {
    return resolvePositiveIntegerSetting(
        options.freshnessWindowSecs,
        ATTESTATION_FRESHNESS_WINDOW_ENV,
        DEFAULT_ATTESTATION_FRESHNESS_WINDOW_SECS,
    );
}

function resolvePendingRevealMaxTimeoutCycles(options = {}) {
    return resolvePositiveIntegerSetting(
        options.maxTimeoutCycles,
        PENDING_REVEAL_MAX_TIMEOUT_CYCLES_ENV,
        DEFAULT_PENDING_REVEAL_MAX_TIMEOUT_CYCLES,
    );
}

function pendingRevealReadingAgeSecs(entry, nowMs) {
    const readingSecs = Number.isFinite(entry.timestamp)
        ? entry.timestamp
        : entry.reading?.recorded_at
            ? Math.floor(new Date(entry.reading.recorded_at).getTime() / 1000)
            : null;
    if (!Number.isFinite(readingSecs)) return null;
    return Math.floor(nowMs / 1000) - readingSecs;
}

/**
 * Terminalize entries that are no longer eligible for quorum (#3266): record
 * the durable `expired_unqualified` outcome with its reason, emit one
 * collector_cycle_outcome line per entry, and drop them from pending state so
 * the cycle can commit fresh readings. The watermark is NOT advanced: the
 * reading was never paid, and its reading_id in terminal-outcomes.json is what
 * stops it being recommitted.
 */
function terminalizeUnqualifiedEntries(terminal, options = {}) {
    if (terminal.length === 0) return [];
    const reasons = new Map(terminal.map(({ entry, reason }) => [entry.reading_id, reason]));
    const entries = terminal.map(({ entry }) => entry);
    for (const { entry, reason, detail } of terminal) {
        emitCollectorOutcome('rejected', {
            reason: TERMINAL_OUTCOME_EXPIRED_UNQUALIFIED,
            terminal_reason: reason,
            commit_hash: entry.commit_hash,
            station_id: entry.station_id,
            timeout_cycles: entry.timeout_cycles || 0,
            ...detail,
        });
    }
    recordTerminalOutcomes(
        entries,
        TERMINAL_OUTCOME_EXPIRED_UNQUALIFIED,
        options,
        (entry) => reasons.get(entry.reading_id),
    );
    removePendingRevealEntries(getPendingRevealsPath(options), entries);
    return entries;
}

/**
 * #3266: before spending the attestation poll budget on awaiting reveals
 * recovered from an earlier cycle, prove which of them can never qualify:
 *
 *   - rejected_by_quorum_member: a commit-bound attestation set (pending
 *     observation or canonical status) already carries a rejection. This is
 *     the same predicate evaluateAttestationQuorum treats as not-ready.
 *   - absent_from_node_pending: on a complete (unsaturated) page, the commit
 *     is in neither pending lane and the commit-bound canonical status proves
 *     the node holds no commit, reveal or reward record for it. There is no
 *     reveal left for an attester to attest (the 2026-09-24 shape: another
 *     producer's block swept it and the node dropped the stale reveal).
 *   - beyond_attestation_freshness_window: the reading is older than the
 *     attester freshness window while fewer than quorum oracles have
 *     attested, so every missing attester is guaranteed to refuse it.
 *
 * Fail-closed: a pending/canonical read error, a saturated page, a reveal
 * that is quorum-ready, or one whose attester count is unknown stays pending
 * and keeps the pre-#3266 behaviour (poll, then the K-cycle bound).
 */
async function reconcileUnqualifiableAwaitingEntries(entries, options = {}) {
    if (entries.length === 0) return [];
    const pending = await fetchPendingAttestations(options);
    if (!pending.ok) return [];
    const saturated = pending.observations.length >= PENDING_ATTESTATIONS_PAGE_LIMIT
        || pending.readyToMine.length >= PENDING_ATTESTATIONS_PAGE_LIMIT;
    const readyIds = new Set(pending.readyToMine.map((item) => String(item.commit_id)));
    const byCommit = new Map(
        [...pending.observations, ...pending.readyToMine]
            .map((item) => [String(item.commit_id), item]),
    );
    const nowMs = Number.isFinite(options.nowMs) ? options.nowMs : Date.now();
    const windowSecs = resolveAttestationFreshnessWindowSecs(options);
    const terminal = [];
    for (const entry of entries) {
        const commitId = String(entry.commit_hash);
        if (readyIds.has(commitId)) continue;
        const observation = byCommit.get(commitId) || null;
        const ageSecs = pendingRevealReadingAgeSecs(entry, nowMs);
        const detail = {
            ...(ageSecs !== null ? { reading_age_secs: ageSecs } : {}),
            freshness_window_secs: windowSecs,
        };
        let reason = null;
        if (observation && Number(observation.rejections || 0) > 0) {
            reason = 'rejected_by_quorum_member';
            detail.rejections = Number(observation.rejections);
        }
        if (!reason) {
            const canonical = await fetchCanonicalContributionStatus(entry, options);
            // Settled entries belong to canonical reconciliation; a transport
            // error is no evidence at all.
            if (canonical.settled || ['request_failed', 'http', 'invalid_entry'].includes(canonical.reason)) {
                continue;
            }
            const body = canonical.status;
            const bound = Boolean(body) && String(body.commit_id || '') === commitId;
            // The node's contribution_status reports these two statuses only
            // when canonical state holds no commit, no reveal and no reward
            // record for the commit (routes/transactions.rs). Anything else —
            // including an unbound body — is not proof of absence (#2801).
            const noCanonicalRecords = canonical.reason === 'not_found'
                || (bound && ['attested_without_pending_records', 'not_found'].includes(String(body.status || '')));
            if (bound && Number(body.rejections || 0) > 0) {
                reason = 'rejected_by_quorum_member';
                detail.rejections = Number(body.rejections);
            } else if (!observation && !saturated && noCanonicalRecords) {
                reason = 'absent_from_node_pending';
                detail.canonical_status = bound ? String(body.status) : 'not_found';
            }
        }
        if (!reason && observation && ageSecs !== null && ageSecs > windowSecs) {
            const attesters = new Set(
                Array.isArray(observation.attesting_oracles)
                    ? observation.attesting_oracles.map(String)
                    : [],
            );
            if (attesters.size < pending.effectiveQuorum) {
                reason = 'beyond_attestation_freshness_window';
                detail.attesting = attesters.size;
                detail.effective_quorum = pending.effectiveQuorum;
            }
        }
        if (reason) terminal.push({ entry, reason, detail });
    }
    return terminalizeUnqualifiedEntries(terminal, options);
}

// Reasons that mean the node answered and the reveal still did not qualify.
// Transport/auth failures say nothing about the reveal and never count.
const UNQUALIFIED_POLL_REASONS = new Set([
    'timeout',
    'rejected',
    'unrecoverable',
    'already_mined',
    'station_binding_mismatch',
]);

/**
 * #3266 K-cycle bound: count one more unqualified cycle for every entry of a
 * batch that did not reach quorum, then terminalize those that have now
 * missed quorum in `maxTimeoutCycles` consecutive cycles (default 3,
 * WRFCOIN_PENDING_REVEAL_MAX_TIMEOUT_CYCLES). A batch member that is itself
 * quorum-ready on a fresh read is kept (its counter reset) so a stuck peer
 * cannot drag a qualified reveal down with it.
 */
async function recordUnqualifiedCycle(entries, quorumReason, options = {}) {
    if (entries.length === 0 || !UNQUALIFIED_POLL_REASONS.has(quorumReason)) return [];
    const stateFilePath = getPendingRevealsPath(options);
    const maxCycles = resolvePendingRevealMaxTimeoutCycles(options);
    const counted = new Map(entries.map((entry) => [entry.reading_id, (entry.timeout_cycles || 0) + 1]));
    const exhaustedIds = new Set(
        [...counted].filter(([, cycles]) => cycles >= maxCycles).map(([readingId]) => readingId),
    );
    let readyIds = new Set();
    if (exhaustedIds.size > 0) {
        const pending = await fetchPendingAttestations(options);
        if (pending.ok) {
            readyIds = new Set(
                entries
                    .filter((entry) => exhaustedIds.has(entry.reading_id))
                    .filter((entry) => evaluateAttestationQuorum([entry], pending).ready)
                    .map((entry) => entry.reading_id),
            );
        }
    }
    const current = loadPendingRevealEntries(stateFilePath);
    const next = current.map((entry) => {
        if (!counted.has(entry.reading_id)) return entry;
        if (readyIds.has(entry.reading_id)) return { ...entry, timeout_cycles: 0 };
        return { ...entry, timeout_cycles: counted.get(entry.reading_id) };
    });
    savePendingRevealEntries(stateFilePath, next);
    const exhausted = next.filter((entry) =>
        exhaustedIds.has(entry.reading_id) && !readyIds.has(entry.reading_id));
    return terminalizeUnqualifiedEntries(
        exhausted.map((entry) => ({
            entry,
            reason: 'unqualified_timeout_cycles',
            detail: { last_quorum_reason: quorumReason, max_timeout_cycles: maxCycles },
        })),
        options,
    );
}

function evaluateAttestationQuorum(commits, pendingResponse) {
    const wanted = [...new Set(commits.map((item) => String(item?.commit_hash || item)))];
    const expectedByCommit = new Map(
        commits
            .filter((item) => item && typeof item === 'object')
            .map((item) => [String(item.commit_hash), item]),
    );
    if (!pendingResponse?.ok || wanted.length === 0) {
        return { ready: false, reason: pendingResponse?.reason || 'empty_batch' };
    }
    const unrecoverable = new Set(pendingResponse.unrecoverable || []);
    if (wanted.some((commitId) => unrecoverable.has(commitId))) {
        return { ready: false, reason: 'unrecoverable' };
    }
    const readyToMine = Array.isArray(pendingResponse.readyToMine) ? pendingResponse.readyToMine : [];
    if (pendingResponse.observations.length === 0 && readyToMine.length === 0) {
        return { ready: false, reason: 'empty_response' };
    }

    const byCommit = new Map(
        [...pendingResponse.observations, ...readyToMine]
            .map((observation) => [String(observation.commit_id), observation]),
    );
    for (const commitId of wanted) {
        const observation = byCommit.get(commitId);
        // Absence is never promoted to success: only a full, commit-bound
        // reveal returned by the node can authorize a collector mine.
        if (!observation) return { ready: false, reason: 'missing_commit', commitId };
        const expected = expectedByCommit.get(commitId);
        const isReadyToMine = readyToMine.some((item) => String(item.commit_id) === commitId);
        if (isReadyToMine && expected && String(observation.station_id || '') !== String(expected.station_id || '')) {
            return { ready: false, reason: 'station_binding_mismatch', commitId };
        }
        if (Number(observation.rejections || 0) > 0) {
            return { ready: false, reason: 'rejected', commitId };
        }
        if (observation.source && observation.source !== 'mempool') {
            return { ready: false, reason: 'already_mined', commitId };
        }
        const distinctAttesters = new Set(
            Array.isArray(observation.attesting_oracles) ? observation.attesting_oracles.map(String) : [],
        );
        if (isReadyToMine) {
            continue;
        }
        // For newly submitted work the two attester transactions may still be
        // in the mempool, so distinct identities are also sufficient.
        if (distinctAttesters.size < pendingResponse.effectiveQuorum) {
            return {
                ready: false,
                reason: 'awaiting_quorum',
                commitId,
                attesting: distinctAttesters.size,
                effectiveQuorum: pendingResponse.effectiveQuorum,
            };
        }
    }
    return { ready: true, reason: 'quorum', effectiveQuorum: pendingResponse.effectiveQuorum };
}

async function awaitAttestationQuorum(commits, options = {}) {
    const configuredMaxPolls = parseInt(
        process.env.WRFCOIN_ATTESTATION_MAX_POLLS || ATTESTATION_MAX_POLLS,
        10,
    );
    const configuredPollIntervalMs = parseInt(
        process.env.WRFCOIN_ATTESTATION_POLL_MS || ATTESTATION_POLL_INTERVAL_MS,
        10,
    );
    const maxPolls = Number.isInteger(options.maxPolls)
        ? Math.max(1, options.maxPolls)
        : Number.isInteger(configuredMaxPolls) && configuredMaxPolls > 0
            ? configuredMaxPolls
            : ATTESTATION_MAX_POLLS;
    const pollIntervalMs = Number.isFinite(options.pollIntervalMs)
        ? Math.max(0, options.pollIntervalMs)
        : Number.isInteger(configuredPollIntervalMs) && configuredPollIntervalMs >= 0
            ? configuredPollIntervalMs
            : ATTESTATION_POLL_INTERVAL_MS;
    const sleepFn = options.sleepFn || sleep;
    let last = { ready: false, reason: 'not_polled' };

    for (let attempt = 1; attempt <= maxPolls; attempt++) {
        const pending = await fetchPendingAttestations(options);
        if (!pending.ok) {
            return { ready: false, reason: pending.reason, status: pending.status, attempts: attempt };
        }
        last = evaluateAttestationQuorum(commits, pending);
        if (last.ready) return { ...last, attempts: attempt, height: pending.height };
        // A valid empty response can race reveal admission and attester polling,
        // so it consumes the bounded retry budget instead of becoming terminal.
        if (['rejected', 'unrecoverable', 'already_mined', 'station_binding_mismatch'].includes(last.reason)) {
            return { ...last, attempts: attempt };
        }
        if (attempt < maxPolls) await sleepFn(pollIntervalMs);
    }
    return { ...last, ready: false, reason: 'timeout', attempts: maxPolls };
}

function removePendingRevealEntries(stateFilePath, entries) {
    const completed = new Set(entries.map((entry) => entry.reading_id));
    savePendingRevealEntries(
        stateFilePath,
        loadPendingRevealEntries(stateFilePath).filter((entry) => !completed.has(entry.reading_id)),
    );
}

function advanceWatermarksForEntries(entries, options = {}) {
    const readingsByStation = new Map();
    for (const entry of entries) {
        if (!entry.station_id) continue;
        const readings = readingsByStation.get(entry.station_id) || [];
        readings.push(entry.reading);
        readingsByStation.set(entry.station_id, readings);
    }
    for (const [stationId, readings] of readingsByStation) {
        const previous = readStationWatermark(stationId, options);
        writeStationWatermark(stationId, computeNextLastSubmittedTs(previous, readings), options);
    }
}

async function submitEligiblePendingReveals(options = {}) {
    const stateFilePath = getPendingRevealsPath(options);
    const currentBlockHeight = Number.isFinite(options.currentBlockHeight)
        ? options.currentBlockHeight
        : await getBlockchainHeight(options);
    const entries = loadPendingRevealEntries(stateFilePath);
    const next = [];
    const awaitingEntries = [];
    const failedEntries = [];
    const droppedEntries = [];

    for (const entry of entries) {
        if (entry.phase === PENDING_PHASE_AWAITING_QUORUM) {
            // Once a reveal is admitted, dropping local state would allow a
            // later collector mine to include it without re-checking quorum.
            // Keep it withheld until the API proves the safe transition (or
            // an operator reconciles an already-mined/expired entry).
            next.push(entry);
            awaitingEntries.push(entry);
            continue;
        }
        if (options.heartbeatCommitIds && !options.heartbeatCommitIds.has(entry.commit_hash)) {
            next.push(entry);
            continue;
        }
        if (
            Number.isFinite(currentBlockHeight)
            && Number.isFinite(entry.commit_block_height)
            && (currentBlockHeight - entry.commit_block_height) > PENDING_REVEAL_MAX_AGE_BLOCKS
        ) {
            droppedEntries.push(entry);
            emitFailureMetric('audit_loss', {
                reason: 'pending_reveal_too_old',
                commit_hash: entry.commit_hash,
                station_id: entry.station_id,
            });
            continue;
        }
        if (
            !Number.isFinite(currentBlockHeight)
            || !Number.isFinite(entry.commit_block_height)
            || currentBlockHeight <= entry.commit_block_height
        ) {
            next.push(entry);
            continue;
        }
        const reveal = await submitContributionReveal(
            entry.reading,
            entry.station_id,
            entry.contributor,
            entry.nonce,
            entry.commit_hash,
            entry.timestamp,
            { ...options, qualityTier: entry.quality_tier || null },
        );
        if (!reveal.ok) {
            next.push(entry);
            failedEntries.push(entry);
            continue;
        }
        const awaiting = {
            ...entry,
            phase: PENDING_PHASE_AWAITING_QUORUM,
            reveal_submitted_at_height: currentBlockHeight,
        };
        next.push(awaiting);
        awaitingEntries.push(awaiting);
    }
    savePendingRevealEntries(stateFilePath, next);
    return { awaitingEntries, failedEntries, droppedEntries, currentBlockHeight };
}

async function mineAwaitingQuorumEntries(entries, options = {}) {
    if (entries.length === 0) return { mined: false, withheld: false, reason: 'empty' };
    const externalMining = !collectorMineEnabled(options);
    // The heartbeat owns production in this mode. Canonical inclusion, rather
    // than a disappearing mempool quorum, proves successful settlement.
    const quorum = externalMining
        ? { ready: true, reason: 'heartbeat' }
        : await awaitAttestationQuorum(entries, options);
    if (!quorum.ready) {
        emitFailureMetric('attestation_quorum_withheld', {
            reason: quorum.reason,
            commit_count: entries.length,
            attempts: quorum.attempts || 0,
        });
        emitCollectorOutcome(quorum.reason === 'rejected' ? 'rejected' : 'timeout', {
            reason: quorum.reason,
            commit_count: entries.length,
        });
        return { mined: false, withheld: true, reason: quorum.reason, quorum };
    }
    const mineResult = externalMining
        ? await waitForHeartbeatInclusion(entries, 'reveal', options)
        : await mineBlock(options);
    if (mineResult.status !== 201) {
        emitFailureMetric('mine_failed', {
            reason: 'quorum_qualified_reveal_mine_failed',
            status: mineResult.status,
            error_code: mineResult.data?.code || null,
        });
        emitCollectorOutcome('timeout', {
            reason: 'mine_failed',
            commit_count: entries.length,
        });
        return { mined: false, withheld: true, reason: 'mine_failed', mineResult, quorum };
    }
    const stateFilePath = getPendingRevealsPath(options);
    const height = mineResult.data?.height ?? null;
    // A 201 proves a block was mined, not that it carried these reveals: block
    // assembly skips and evicts any reveal that fails state replay (the
    // 2026-09-24 post-halt backlog: 34 attested reveals skipped with
    // SP_CONS_INV_POP_STATE_TRANSITION_FAILED while the collector logged
    // MINED). Only claim, and only advance the watermark for, reveals the
    // node's commit-bound status shows as applied (#3270).
    const included = [];
    const unverified = [];
    const lost = [];
    const notYetIncluded = [];
    for (const entry of entries) {
        const inclusion = await classifyMinedRevealInclusion(entry, options);
        if (inclusion.state === 'included') included.push(entry);
        else if (inclusion.state === 'lost') lost.push({ entry, inclusion });
        else if (inclusion.state === 'not_yet_included') notYetIncluded.push({ entry, inclusion });
        else if (externalMining) notYetIncluded.push({ entry, inclusion });
        else unverified.push(entry);
    }
    const claimed = [...included, ...unverified];
    removePendingRevealEntries(stateFilePath, claimed);
    advanceWatermarksForEntries(claimed, options);
    if (claimed.length > 0) {
        emitCollectorOutcome('pending_reconciliation', {
            reason: 'mined_pending_finalization',
            commit_count: claimed.length,
            height,
            // A status read that cannot be bound to the commit keeps the
            // earlier claim, but says so instead of passing as verified.
            ...(unverified.length > 0 ? { inclusion_unverified: unverified.length } : {}),
        });
    }
    const excludedItems = [...lost, ...notYetIncluded];
    for (const { entry, inclusion } of excludedItems) {
        emitFailureMetric('audit_loss', {
            reason: REVEAL_NOT_INCLUDED,
            commit_hash: entry.commit_hash,
            station_id: entry.station_id,
            height,
            canonical_status: inclusion.canonicalStatus,
            terminal: inclusion.state === 'lost',
        });
    }
    if (lost.length > 0) {
        // The commit record is gone and the reveal was not applied, so nothing
        // can reveal this commit again. Terminal and never paid; the watermark
        // is untouched and the reading_id in terminal-outcomes.json stops a
        // recommit.
        const statusByReading = new Map(
            lost.map(({ entry, inclusion }) => [entry.reading_id, inclusion.canonicalStatus]),
        );
        recordTerminalOutcomes(
            lost.map(({ entry }) => entry),
            REVEAL_NOT_INCLUDED,
            options,
            (entry) => statusByReading.get(entry.reading_id),
        );
        removePendingRevealEntries(stateFilePath, lost.map(({ entry }) => entry));
    }
    // notYetIncluded stays AWAITING_QUORUM: the commit is still live, so the
    // reveal may only have been deferred to a later block. The next cycle's
    // canonical reconciliation settles it if it lands; if the node evicted it,
    // the commit expires and #3266's absent_from_node_pending terminalizes it.
    // Either way nothing is claimed as mined now.
    if (excludedItems.length > 0) {
        emitCollectorOutcome('rejected', {
            reason: REVEAL_NOT_INCLUDED,
            commit_count: excludedItems.length,
            terminal: lost.length,
            height,
        });
    }
    return {
        mined: claimed.length > 0,
        withheld: claimed.length === 0,
        ...(claimed.length === 0 ? { reason: REVEAL_NOT_INCLUDED } : {}),
        height,
        revealed: claimed.length,
        excluded: excludedItems.length,
        mineResult,
        quorum,
    };
}

/**
 * #3270: after a 201 from /blocks/mine, read the commit-bound public status
 * to learn whether the node actually applied this reveal.
 *
 *   included          revealed_* / reward_* status, or an integer revealed_at
 *   lost              attested_without_pending_records or not_found: no
 *                     commit, no applied reveal and no reward record, so the
 *                     reveal can never be applied
 *   not_yet_included  committed_pending_reveal: the commit is live but this
 *                     block did not apply the reveal (skipped, or deferred)
 *   unverified        transport error, non-200, or a body not bound to this
 *                     commit and station: no evidence either way
 */
async function classifyMinedRevealInclusion(entry, options = {}) {
    const request = options.httpGet || httpGet;
    const commitId = String(entry?.commit_hash || '');
    if (!commitId) return { state: 'unverified', reason: 'invalid_entry' };
    let response;
    try {
        response = await request(
            options.host,
            options.port,
            `/transactions/commit/${encodeURIComponent(commitId)}/status`,
            null,
        );
    } catch (_) {
        return { state: 'unverified', reason: 'request_failed' };
    }
    if (!response || response.status !== 200) {
        return { state: 'unverified', reason: 'http', status: response?.status ?? null };
    }
    const body = response.data?.data || response.data;
    if (!body || String(body.commit_id || '') !== commitId) {
        return { state: 'unverified', reason: 'unbound' };
    }
    // The route reports station_id null once no commit/reveal/reward record
    // remains; a non-null station must match.
    if (
        body.station_id !== null && body.station_id !== undefined
        && String(body.station_id) !== String(entry.station_id || '')
    ) {
        return { state: 'unverified', reason: 'station_binding_mismatch' };
    }
    const status = String(body.status || '');
    if (
        Number.isInteger(body.revealed_at)
        || status.startsWith('revealed_')
        || status.startsWith('reward_')
    ) {
        return { state: 'included', canonicalStatus: status };
    }
    if (status === 'attested_without_pending_records' || status === 'not_found') {
        return { state: 'lost', canonicalStatus: status };
    }
    if (status === 'committed_pending_reveal') {
        return { state: 'not_yet_included', canonicalStatus: status };
    }
    return { state: 'unverified', reason: 'unknown_status', canonicalStatus: status };
}

/**
 * Resume the durable COMMITTED -> REVEAL_PENDING -> AWAITING_QUORUM -> MINE
 * state machine. Every call to /blocks/mine while reveals are present is
 * guarded by distinct mempool attesters >= the API's effective_quorum.
 */
async function settlePendingAttestationBatch(options = {}) {
    const stateFilePath = getPendingRevealsPath(options);
    let canonicalStationOutcomes = [];
    const finish = (result) => ({ ...result, canonicalStationOutcomes });
    let entries = loadPendingRevealEntries(stateFilePath);
    if (entries.length === 0) {
        return finish({ mined: false, withheld: false, reason: 'empty', pending: 0 });
    }
    let currentBlockHeight = Number.isFinite(options.currentBlockHeight)
        ? options.currentBlockHeight
        : await getBlockchainHeight(options);

    // Never mine commits while a reveal is already waiting: the node selects
    // its own mempool contents, so that would also mine the unqualified reveal.
    let awaiting = entries.filter((entry) => entry.phase === PENDING_PHASE_AWAITING_QUORUM);
    if (awaiting.length > 0) {
        const reconciled = await reconcileCanonicallySettledEntries(awaiting, options);
        canonicalStationOutcomes = reconciled.stationOutcomes;
        if (reconciled.settledEntries.length > 0) {
            entries = loadPendingRevealEntries(stateFilePath);
            awaiting = entries.filter((entry) => entry.phase === PENDING_PHASE_AWAITING_QUORUM);
            if (entries.length === 0) {
                return finish({
                    mined: false,
                    withheld: false,
                    reason: 'canonical_finalized',
                    reconciled: reconciled.settledEntries.length,
                    pending: 0,
                });
            }
        }
    }
    if (awaiting.length > 0) {
        const terminal = await reconcileExpiredAwaitingEntries(awaiting, currentBlockHeight, options);
        if (terminal.length > 0) {
            entries = loadPendingRevealEntries(stateFilePath);
            awaiting = entries.filter((entry) => entry.phase === PENDING_PHASE_AWAITING_QUORUM);
            if (entries.length === 0) {
                return finish({
                    mined: false,
                    withheld: false,
                    reason: 'expired_unrecoverable',
                    terminal: terminal.length,
                    pending: 0,
                });
            }
        }
    }
    let unqualifiedTerminal = 0;
    if (awaiting.length > 0) {
        // #3266: prove, before polling, which recovered reveals can never
        // qualify, so a stale one cannot withhold this cycle's fresh readings.
        const unqualified = await reconcileUnqualifiableAwaitingEntries(awaiting, options);
        if (unqualified.length > 0) {
            unqualifiedTerminal += unqualified.length;
            entries = loadPendingRevealEntries(stateFilePath);
            awaiting = entries.filter((entry) => entry.phase === PENDING_PHASE_AWAITING_QUORUM);
        }
    }
    if (awaiting.length > 0) {
        const prior = await mineAwaitingQuorumEntries(awaiting, options);
        if (!prior.mined) {
            // #3266 K-cycle bound: a reveal that keeps missing quorum is
            // terminalized after K consecutive cycles. Until then, and for
            // any batch member still in flight, keep withholding: mining now
            // would sweep the unqualified reveal into the block.
            const exhausted = prior.reason === 'mine_failed'
                ? []
                : await recordUnqualifiedCycle(awaiting, prior.quorum?.reason || prior.reason, options);
            unqualifiedTerminal += exhausted.length;
            entries = loadPendingRevealEntries(stateFilePath);
            awaiting = entries.filter((entry) => entry.phase === PENDING_PHASE_AWAITING_QUORUM);
            if (awaiting.length > 0 || exhausted.length === 0) {
                return finish({ ...prior, pending: entries.length, unqualifiedTerminal });
            }
        } else {
            currentBlockHeight = Number.isFinite(prior.height) ? prior.height : currentBlockHeight;
            entries = loadPendingRevealEntries(stateFilePath);
            if (entries.length === 0) return finish({ ...prior, pending: 0 });
        }
    }

    const committed = entries.filter((entry) => entry.phase === PENDING_PHASE_COMMITTED);
    if (committed.length === 0) {
        return finish({
            mined: false,
            withheld: false,
            reason: unqualifiedTerminal > 0 ? TERMINAL_OUTCOME_EXPIRED_UNQUALIFIED : 'empty',
            ...(unqualifiedTerminal > 0 ? { terminal: unqualifiedTerminal } : {}),
            pending: 0,
        });
    }

    // If the commit block has not advanced yet, mine commits only. The reveal
    // is not admitted until this returns 201 and the chain height moves.
    const needsCommitBlock = committed.some((entry) =>
        !Number.isFinite(currentBlockHeight)
        || !Number.isFinite(entry.commit_block_height)
        || currentBlockHeight <= entry.commit_block_height
    );
    let commitBlockMined = false;
    let heartbeatCommitIds;
    if (!collectorMineEnabled(options)) {
        const commitWait = await waitForHeartbeatCommits(committed, { ...options, currentBlockHeight });
        heartbeatCommitIds = new Set(commitWait.included.map((entry) => entry.commit_hash));
        currentBlockHeight = commitWait.height ?? currentBlockHeight;
    } else if (needsCommitBlock) {
        const commitMine = await mineBlock(options);
        if (commitMine.status !== 201) {
            return finish({
                mined: false,
                withheld: true,
                reason: 'commit_mine_failed',
                mineFailed: true,
                mineStatus: commitMine.status,
                pending: entries.length,
            });
        }
        commitBlockMined = true;
        currentBlockHeight = commitMine.data?.height ?? currentBlockHeight;
    }

    const revealState = await submitEligiblePendingReveals({
        ...options,
        currentBlockHeight,
        heartbeatCommitIds,
    });
    awaiting = revealState.awaitingEntries;
    if (awaiting.length === 0) {
        return finish({
            mined: false,
            withheld: revealState.failedEntries.length > 0,
            reason: revealState.failedEntries.length > 0 ? 'reveal_failed' : 'no_eligible_reveals',
            commitBlockMined,
            pending: loadPendingRevealEntries(stateFilePath).length,
        });
    }
    const revealMine = await mineAwaitingQuorumEntries(awaiting, options);
    if (!revealMine.mined && revealMine.reason !== 'mine_failed' && collectorMineEnabled(options)) {
        // Freshly revealed work starts its #3266 unqualified-cycle count here.
        await recordUnqualifiedCycle(awaiting, revealMine.quorum?.reason || revealMine.reason, options);
    }
    return finish({
        ...revealMine,
        commitBlockMined,
        pending: loadPendingRevealEntries(stateFilePath).length,
    });
}

async function mineBlock(options = {}) {
    if (!collectorMineEnabled(options)) {
        return { status: null, data: { code: 'COLLECTOR_MINING_DISABLED' } };
    }
    const request = options.httpPost || httpPost;
    const host = options.host;
    const port = options.port;
    const token = options.token;
    const minerAddress = options.minerAddress || process.env.WRFCOIN_MINER_ADDRESS;
    if (!isValidMinerAddress(minerAddress)) {
        return {
            status: null,
            data: {
                code: 'MINER_ADDRESS_REQUIRED',
                message: 'WRFCOIN_MINER_ADDRESS must be an explicit 0x-prefixed 20-byte address',
            },
        };
    }
    const res = await request(host, port,
        '/blocks/mine',
        { miner_address: minerAddress },
        token);
    return res;
}

function collectorMineEnabled(options = {}) {
    const value = options.collectorMine ?? process.env.COLLECTOR_MINE ?? 'true';
    if (value === true || value === 'true') return true;
    if (value === false || value === 'false') return false;
    throw new Error('COLLECTOR_MINE must be true or false');
}

// Each COMMITTED record has its own evidence and expiry. One lost record must
// not hold back a confirmed neighbour or admission of fresh work next cycle.
// A transient 404 stays pending; local abandonment uses the existing age
// policy and durable terminal reading IDs, never a new commit packet.
async function waitForHeartbeatCommits(entries, options = {}) {
    const interval = String(options.heartbeatIntervalSeconds
        ?? process.env.BLOCK_HEARTBEAT_INTERVAL_SECONDS ?? '77');
    if (!/^\d+$/.test(interval) || Number(interval) < 30 || Number(interval) > 600) {
        throw new Error('invalid heartbeat interval');
    }
    const now = options.monotonicNow || (() => require('node:perf_hooks').performance.now());
    const start = now();
    const deadline = start + Number(interval) * 3000;
    const wallStart = Number.isFinite(options.nowMs) ? options.nowMs : Date.now();
    const request = options.httpGet || httpGet;
    const sleepFn = options.sleepFn || sleep;
    const pending = new Map(entries.map((entry) => [entry.commit_hash, entry]));
    const previous = new Map();
    // Tip/status endpoints do not provide an atomic snapshot. Bracket a fresh
    // status with matching canonical tips before any durable local retirement.
    // Read failures, a changed tip/hash, or a surviving live record fail closed.
    async function confirmTerminal(entry) {
        async function read(route, token) {
            if (now() >= deadline) return null;
            try {
                return await request(options.host, options.port, route, token,
                    { timeoutMs: Math.min(10000, Math.max(1, deadline - now())) });
            } catch (_) { return null; }
        }
        const firstResponse = await read('/blockchain', options.token);
        const first = firstResponse?.data?.data || firstResponse?.data;
        if (firstResponse?.status !== 200 || !Number.isSafeInteger(first?.height) || first.height < 0) return null;
        const statusResponse = await read(`/transactions/commit/${encodeURIComponent(entry.commit_hash)}/status`, null);
        const body = statusResponse?.data?.data || statusResponse?.data;
        const lastResponse = await read('/blockchain', options.token);
        const last = lastResponse?.data?.data || lastResponse?.data;
        if (now() >= deadline || lastResponse?.status !== 200 || first.height !== last?.height
            || first.tip !== last.tip) return null;
        const bound = statusResponse?.status === 200 && body
            && body.commit_id === entry.commit_hash && body.station_id === entry.station_id;
        if (bound && (Number.isSafeInteger(body.revealed_at) && body.revealed_at >= 1
            || body.reward_finalized === true)) return null;
        const expires = bound
            ? [body.commit_expires_at, body.reveal_expires_at].filter(Number.isSafeInteger) : [];
        if (expires.length > 0 && last.height >= Math.min(...expires)) {
            return { reason: 'commit_window_expired', height: last.height };
        }
        // Max-age abandonment is a separate local policy, not proof that a
        // live canonical commit expired. Reconfirm absence before using it.
        const absent = statusResponse?.status === 404 || (statusResponse?.status === 200 && body
            && body.commit_id === entry.commit_hash && body.station_id == null
            && ['not_found', 'attested_without_pending_records'].includes(body.status));
        const ageSecs = pendingRevealReadingAgeSecs(entry, wallStart + now() - start);
        const aged = (Number.isFinite(entry.commit_block_height)
            && last.height - entry.commit_block_height > PENDING_REVEAL_MAX_AGE_BLOCKS)
            || (ageSecs !== null && ageSecs > resolveAttestationFreshnessWindowSecs(options));
        return absent && aged ? { reason: 'commit_abandoned_max_age', height: last.height } : null;
    }
    let height = options.currentBlockHeight;
    while (pending.size > 0 && now() < deadline) {
        try {
            const chainResponse = await request(options.host, options.port, '/blockchain', options.token,
                { timeoutMs: Math.min(10000, Math.max(1, deadline - now())) });
            const chain = chainResponse?.data?.data || chainResponse?.data;
            if (chainResponse?.status === 200 && Number.isSafeInteger(chain?.height)) height = chain.height;
        } catch (_) { /* no height evidence; keep per-record state */ }
        const included = [];
        const promoted = new Set();
        for (const entry of pending.values()) {
            if (now() >= deadline) break;
            let response;
            try {
                response = await request(options.host, options.port,
                    `/transactions/commit/${encodeURIComponent(entry.commit_hash)}/status`, null,
                    { timeoutMs: Math.min(10000, Math.max(1, deadline - now())) });
            } catch (_) { response = null; }
            const body = response?.data?.data || response?.data;
            const bound = response?.status === 200 && body
                && body.commit_id === entry.commit_hash && body.station_id === entry.station_id;
            // A reveal already applied during a restart/reorg race belongs to
            // reconciliation. Do not reveal it again or abandon it as old.
            const applied = bound && Number.isSafeInteger(body.revealed_at) && body.revealed_at >= 1;
            const finalized = bound && body.reward_finalized === true && body.status === 'reward_finalized'
                && ['paid', 'finalized_zero', 'excluded_non_real'].includes(body.reward_status);
            if (applied || finalized) {
                promoted.add(entry.commit_hash);
                pending.delete(entry.commit_hash);
                if (applied) height = Math.max(height || 0, body.revealed_at);
                continue;
            }
            const expires = bound
                ? [body.commit_expires_at, body.reveal_expires_at].filter(Number.isSafeInteger)
                : [];
            const pastWindow = expires.length > 0 && Number.isFinite(height)
                && height >= Math.min(...expires); // next block would be too late
            const ageSecs = pendingRevealReadingAgeSecs(entry, wallStart + now() - start);
            const pastAge = (Number.isFinite(height) && Number.isFinite(entry.commit_block_height)
                && height - entry.commit_block_height > PENDING_REVEAL_MAX_AGE_BLOCKS)
                || (ageSecs !== null && ageSecs > resolveAttestationFreshnessWindowSecs(options));
            if (pastWindow || pastAge) {
                const confirmed = await confirmTerminal(entry);
                if (confirmed) {
                    // Persist synchronously after the confirming reads, before
                    // processing another record can introduce an async gap.
                    terminalizeUnqualifiedEntries([{ entry, reason: confirmed.reason, detail: {} }], options);
                    pending.delete(entry.commit_hash);
                    height = confirmed.height;
                }
                previous.delete(entry.commit_hash);
                continue;
            }
            const signature = bound && Number.isSafeInteger(body.committed_at) && body.committed_at >= 1
                ? String(body.committed_at) : null;
            if (signature && previous.get(entry.commit_hash) === signature) included.push(entry);
            previous.set(entry.commit_hash, signature);
            if (signature) height = Math.max(height || 0, body.committed_at);
        }
        if (promoted.size > 0) {
            const stateFilePath = getPendingRevealsPath(options);
            savePendingRevealEntries(stateFilePath, loadPendingRevealEntries(stateFilePath).map((entry) =>
                promoted.has(entry.commit_hash) ? { ...entry, phase: PENDING_PHASE_AWAITING_QUORUM } : entry));
        }
        if (included.length > 0 || pending.size === 0) return { included, height };
        const remaining = deadline - now();
        if (remaining > 0) await sleepFn(Math.min(1000, remaining));
    }
    return { included: [], height };
}

// A height advance alone does not prove inclusion. Require two consecutive
// commit- and station-bound canonical observations with the same inclusion
// heights. A disappearance/change (including a reorg) resets the observation.
// Timeout preserves the existing durable pending state for the next cycle.
async function waitForHeartbeatInclusion(entries, phase, options = {}) {
    const interval = String(options.heartbeatIntervalSeconds
        ?? process.env.BLOCK_HEARTBEAT_INTERVAL_SECONDS ?? '77');
    if (!/^\d+$/.test(interval) || Number(interval) < 30 || Number(interval) > 600) {
        throw new Error('invalid heartbeat interval');
    }
    const now = options.monotonicNow || (() => require('node:perf_hooks').performance.now());
    const sleepFn = options.sleepFn || sleep;
    const request = options.httpGet || httpGet;
    const deadline = now() + Number(interval) * 3000;
    let previous = null;
    while (now() < deadline) {
        const observations = [];
        for (const entry of entries) {
            if (now() >= deadline) { observations.length = 0; break; }
            let response;
            try {
                response = await request(options.host, options.port,
                    `/transactions/commit/${encodeURIComponent(entry.commit_hash)}/status`, null,
                    { timeoutMs: Math.min(10000, Math.max(1, deadline - now())) });
            } catch (_) { response = null; }
            const body = response?.data?.data || response?.data;
            const bound = response?.status === 200 && body
                && body.commit_id === entry.commit_hash && body.station_id === entry.station_id;
            const included = bound && (phase === 'commit'
                ? Number.isSafeInteger(body.committed_at) && body.committed_at >= 1
                : Number.isSafeInteger(body.revealed_at) && body.revealed_at >= 1);
            if (!included) { observations.length = 0; break; }
            observations.push({ committed: body.committed_at, revealed: body.revealed_at,
                height: phase === 'commit' ? body.committed_at : body.revealed_at });
        }
        const signature = observations.length === entries.length && entries.length > 0
            ? JSON.stringify(observations) : null;
        if (signature && signature === previous && now() <= deadline) {
            return { status: 201, data: { height: Math.max(...observations.map((item) => item.height)) },
                externalMining: true };
        }
        previous = signature;
        const remaining = deadline - now();
        if (remaining > 0) await sleepFn(Math.min(1000, remaining));
    }
    return { status: null, data: { code: 'HEARTBEAT_INCLUSION_TIMEOUT' }, externalMining: true };
}

// ---------------------------------------------------------------------------
// Registered station elevation (#3285)
// ---------------------------------------------------------------------------

/**
 * Read a station's REGISTERED elevation from the node's canonical registry:
 * GET /transactions/station/registrations/{stationId} -> elevation_meters.
 * This is the value the oracle attester recomputes pressure_sea_level_hpa
 * against, so it is the only automatic source the collector may use.
 *
 * Never throws and never guesses: an unreachable node, an unregistered
 * station, or a registration without an elevation all return
 * { elevation_m: null } with a reason, and the caller then submits no
 * sea-level key.
 *
 * @returns {Promise<{ elevation_m: number|null, reason: string }>}
 */
async function fetchRegisteredElevation(stationId, options = {}) {
    const request = options.httpGet || httpGet;
    const urlPath = `/transactions/station/registrations/${encodeURIComponent(String(stationId))}`;
    let res;
    try {
        res = await request(options.host, options.port, urlPath, options.token);
    } catch (error) {
        return { elevation_m: null, reason: 'registry_unreachable', detail: error.message };
    }
    if (!res || res.status === 404) return { elevation_m: null, reason: 'station_unregistered' };
    if (res.status !== 200 || !res.data || typeof res.data !== 'object') {
        return { elevation_m: null, reason: 'registry_unavailable', status: res.status };
    }
    const body = res.data.data && typeof res.data.data === 'object' ? res.data.data : res.data;
    const elevation = body.elevation_meters;
    if (typeof elevation !== 'number' || !Number.isFinite(elevation)) {
        return { elevation_m: null, reason: 'no_registered_elevation' };
    }
    return { elevation_m: elevation, reason: 'registry' };
}

// ---------------------------------------------------------------------------
// Per-station fetch + normalize + (optional) submit loop
// ---------------------------------------------------------------------------

/**
 * Fetch and normalize Ambient Weather readings for a single station.
 *
 * @param {object} station
 *   { stationId, ownerAddress, appKey, apiKey, mac?, latitude, longitude,
 *     elevation_m?, name?, location? }
 * @param {object} opts { readingsLimit, maxAgeMinutes, sleepMs, lookupRegisteredElevation? }
 *   lookupRegisteredElevation(stationId) -> { elevation_m, reason } (#3285):
 *   the registered elevation wins over a roster elevation_m, and only a
 *   registered one feeds pressure_sea_level_hpa.
 * @returns {object} { ok, normalized, stationMeta, stationName, stale, reason?, detail? }
 */
async function fetchStationReadings(station, opts = {}) {
    const readingsLimit = opts.readingsLimit || 12;
    const maxAgeMinutes = Number.isFinite(opts.maxAgeMinutes) ? opts.maxAgeMinutes : 30;
    const sleepMs = Number.isFinite(opts.sleepMs) ? opts.sleepMs : 1100;
    const fetchDevicesFn = opts.fetchDevices || fetchDevices;
    const fetchDeviceHistoryFn = opts.fetchDeviceHistory || fetchDeviceHistory;

    if (!station.mac) {
        return { ok: false, reason: 'invalid_config', detail: 'Missing private MAC selector for station' };
    }

    if (!station.model && !station.device_model) {
        return { ok: false, reason: 'invalid_config', detail: 'Missing model binding for station' };
    }

    if (!station.appKey || !station.apiKey) {
        return { ok: false, reason: 'invalid_config', detail: 'Missing appKey/apiKey for station' };
    }

    let stationMeta = {
        mac: station.mac || null,
        device_model: station.model || station.device_model || 'unknown',
        location: station.location || null,
        lat: station.latitude ?? null,
        lon: station.longitude ?? null,
        // #3285: no default elevation. A roster value is kept for the accuracy
        // report only; the registry lookup below replaces it when available.
        elevation_m: Number.isFinite(station.elevation_m) ? station.elevation_m : null,
        elevation_source: Number.isFinite(station.elevation_m) ? 'roster' : null,
    };
    let stationName = station.name || station.stationId;

    if (typeof opts.lookupRegisteredElevation === 'function') {
        let registered = null;
        try {
            registered = await opts.lookupRegisteredElevation(station.stationId);
        } catch (_) {
            registered = null;
        }
        if (registered && Number.isFinite(registered.elevation_m)) {
            stationMeta.elevation_m = registered.elevation_m;
            stationMeta.elevation_source = 'registry';
        }
    }

    await sleep(sleepMs); // respect 1 req/sec rate limit
    const devicesRes = await fetchDevicesFn(station.appKey, station.apiKey);
    if (devicesRes.status !== 200 || !Array.isArray(devicesRes.data)) {
        return { ok: false, reason: 'auth', detail: `devices HTTP ${devicesRes.status}` };
    }
    const devices = devicesRes.data;

    // The strict roster always carries private MAC and model selectors. Do
    // not auto-select even a sole returned device: that would turn a missing
    // selector or model binding into an attribution decision.
    const selection = selectExactDevice({
        mac: station.mac,
        model: station.model || station.device_model || null,
    }, devices);
    if (!selection.ok) {
        // #2736: pass the verifier's detail through — the collector's
        // structured skip log reads fetchResult.detail, so this is what makes
        // "provider returned no model" visible without live instrumentation.
        return {
            ok: false,
            reason: selection.code === 'device_mismatch' ? 'device_missing' : selection.code,
            detail: selection.detail,
        };
    }
    const device = selection.device;
    if (!device) return { ok: false, reason: 'device_missing', detail: 'private MAC selector did not match a device' };
    stationMeta.mac = device.macAddress || station.mac;
    // #2736: carry the model-binding provenance into the per-reading
    // attribution envelope (via normalizeReading -> source_verification) and
    // into this function's result (which the collector logs). The weakening
    // to an operator assertion, when it happens, must be auditable — not
    // computed here and dropped.
    stationMeta.model_binding = selection.modelBinding;
    stationMeta.provider_model = selection.providerModel ?? null;

    const stationInfo = device.info || {};
    stationMeta = {
        ...stationMeta,
        location: stationInfo.location || stationMeta.location,
        lat: stationMeta.lat ?? stationInfo.coords?.lat ?? null,
        lon: stationMeta.lon ?? stationInfo.coords?.lon ?? null,
    };
    stationName = stationInfo.name || stationName;

    await sleep(sleepMs);
    const histRes = await fetchDeviceHistoryFn(stationMeta.mac, station.appKey, station.apiKey, readingsLimit);
    if (histRes.status !== 200 || !Array.isArray(histRes.data)) {
        return { ok: false, reason: 'history', detail: `history HTTP ${histRes.status}` };
    }

    // Ambient may expose a fresh observation in the device-list response while
    // its history endpoint temporarily returns an empty array. Treat lastData as
    // a singleton fallback only in that case. The existing station watermark
    // still prevents the same provider timestamp from being rewarded twice.
    const providerReadings = selectAmbientReadings(histRes.data, device.lastData);
    const normalized = providerReadings.map((reading) => normalizeReading(reading, stationMeta));

    let stale = false;
    if (normalized.length > 0) {
        const newest = normalized.reduce((latest, reading) => {
            const timestamp = reading.recorded_at ? new Date(reading.recorded_at).getTime() : 0;
            return timestamp > latest.timestamp ? { timestamp, reading } : latest;
        }, { timestamp: 0, reading: null });
        if (newest.reading) {
            const freshness = verifyObservationFreshness(newest.reading, { maxAgeMinutes });
            stale = !freshness.ok && freshness.code === 'stale_observation';
        }
    }

    return {
        ok: true,
        normalized,
        stationMeta,
        stationName,
        stale,
        rawCount: providerReadings.length,
        usedLatestFallback: histRes.data.length === 0 && providerReadings.length === 1,
        // #2736: the collector's structured logs read this off the result.
        modelBinding: selection.modelBinding,
    };
}

/**
 * Submit a list of (already normalized, deduped) readings for one station
 * under one contributor through the attestation-gated two-block state machine.
 *
 * @returns {object} { committed, failed, mined, height }
 */
function stationCycleOutcomeFromSubmission(result, stationId) {
    const canonical = (result.canonicalStationOutcomes || []).find(
        (item) => String(item.station_id || '') === String(stationId || ''),
    );
    if (canonical) return canonical;
    if (result.mineFailed || Number(result.failed || 0) > 0) {
        return {
            outcome: 'rejected',
            reason: result.reason || (result.mineFailed ? 'mine_failed' : 'collector_failure'),
            station_id: stationId,
        };
    }
    if (result.withheld && REJECTED_SETTLEMENT_REASONS.has(result.reason)) {
        return { outcome: 'rejected', reason: result.reason, station_id: stationId };
    }
    if (result.withheld) {
        return {
            outcome: 'timeout',
            reason: result.reason || 'attestation_quorum_unavailable',
            station_id: stationId,
        };
    }
    if (result.mined) {
        // #2828: reveals mined with quorum, canonical reconciliation just has
        // not caught up for this station yet. The reward decision is durable
        // on-chain — this is NOT a timeout and must not alarm.
        return {
            outcome: 'pending_reconciliation',
            reason: 'canonical_reward_unverified',
            station_id: stationId,
        };
    }
    return {
        outcome: 'timeout',
        reason: result.reason || 'no_terminal_disposition',
        station_id: stationId,
    };
}

async function submitStationReadings(normalized, stationId, contributor, options = {}) {
    const host = options.host;
    const port = options.port;
    const token = options.token;
    const stateFilePath = getPendingRevealsPath(options);
    const nwsObsForTier = options.nwsObsForTier || null;
    // Allow injecting node transports for tests / alternate clients.
    const transport = {};
    if (options.httpPost) transport.httpPost = options.httpPost;
    if (options.httpGet) transport.httpGet = options.httpGet;
    if (options.minerAddress) transport.minerAddress = options.minerAddress;
    if (options.maxPolls) transport.maxPolls = options.maxPolls;
    if (Object.prototype.hasOwnProperty.call(options, 'pollIntervalMs')) {
        transport.pollIntervalMs = options.pollIntervalMs;
    }
    if (options.sleepFn) transport.sleepFn = options.sleepFn;
    if (Object.prototype.hasOwnProperty.call(options, 'collectorMine')) transport.collectorMine = options.collectorMine;
    if (options.heartbeatIntervalSeconds) transport.heartbeatIntervalSeconds = options.heartbeatIntervalSeconds;
    if (options.monotonicNow) transport.monotonicNow = options.monotonicNow;
    // #3266 unqualified-reveal bounds (env-configurable; injectable for tests).
    if (Number.isFinite(options.nowMs)) transport.nowMs = options.nowMs;
    if (options.freshnessWindowSecs) transport.freshnessWindowSecs = options.freshnessWindowSecs;
    if (options.maxTimeoutCycles) transport.maxTimeoutCycles = options.maxTimeoutCycles;
    if (options.terminalOutcomesPath) transport.terminalOutcomesPath = options.terminalOutcomesPath;

    const minerAddress = options.minerAddress || process.env.WRFCOIN_MINER_ADDRESS;
    if (!isValidMinerAddress(minerAddress)) {
        const result = {
            committed: 0,
            failed: 0,
            mined: false,
            mineFailed: true,
            mineStatus: null,
            withheld: true,
            reason: 'MINER_ADDRESS_REQUIRED',
            skippedByWatermark: 0,
        };
        return {
            ...result,
            stationCycleOutcome: stationCycleOutcomeFromSubmission(result, stationId),
        };
    }

    const stateOptions = {
        host,
        port,
        token,
        stateFilePath,
        watermarkPath: options.watermarkPath,
        ...transport,
    };

    // Finish any reveal recovered from a prior cycle before admitting more
    // work. Calling mine while one of those reveals lacks quorum would let the
    // node include it even if this station's new transactions were only commits.
    const recovered = await settlePendingAttestationBatch(stateOptions);
    const recoveredCanonicalOutcome = recovered.canonicalStationOutcomes.find(
        (item) => String(item.station_id || '') === String(stationId || ''),
    ) || null;
    if (recovered.withheld) {
        const result = {
            committed: 0,
            failed: 0,
            mined: false,
            mineFailed: Boolean(recovered.mineFailed),
            mineStatus: recovered.mineStatus ?? null,
            withheld: true,
            reason: recovered.reason,
            skippedByWatermark: 0,
            canonicalOutcome: recoveredCanonicalOutcome,
            canonicalStationOutcomes: recovered.canonicalStationOutcomes,
        };
        return {
            ...result,
            stationCycleOutcome: stationCycleOutcomeFromSubmission(result, stationId),
        };
    }

    const cycleBlockHeight = Object.prototype.hasOwnProperty.call(options, 'commitBlockHeight')
        ? options.commitBlockHeight
        : await getBlockchainHeight({ host, port, token, ...transport });

    // Per-station dedup: skip any reading at/older than this station's watermark
    // so we don't re-commit overlapping Ambient history every run (which would
    // fail reveal as stale and clog pending-reveal state). Watermark is advanced
    // only after a successful mine below.
    const watermarkOptions = stateOptions;
    const previousWatermarkMs = readStationWatermark(stationId, watermarkOptions);
    const pendingReadingIds = new Set(
        loadPendingRevealEntries(stateFilePath).map((entry) => entry.reading_id),
    );
    const terminalReadingIds = new Set(
        loadTerminalOutcomes(stateOptions).map((entry) => String(entry.reading_id)),
    );

    // Sort oldest-first so chain stale-timestamp check passes.
    const sorted = [...normalized]
        .filter((r) => {
            const ts = r.recorded_at ? new Date(r.recorded_at).getTime() : 0;
            const readingId = computePendingRevealReadingId(
                r,
                stationId,
                getReadingTimestamp(r),
            );
            return ts > previousWatermarkMs
                && !pendingReadingIds.has(readingId)
                && !terminalReadingIds.has(readingId);
        })
        .sort((a, b) => {
            const ta = a.recorded_at ? new Date(a.recorded_at).getTime() : 0;
            const tb = b.recorded_at ? new Date(b.recorded_at).getTime() : 0;
            return ta - tb;
        });

    const skippedByWatermark = normalized.filter((reading) => {
        const ts = reading.recorded_at ? new Date(reading.recorded_at).getTime() : 0;
        return ts <= previousWatermarkMs;
    }).length;
    if (skippedByWatermark > 0) {
        console.log(`  Dedup [${stationId}]: ${skippedByWatermark} reading(s) at/older than watermark already submitted, ${sorted.length} new`);
    }

    let committed = 0;
    let failed = 0;

    for (const reading of sorted) {
        const qualityTier = computeReadingQualityTier(reading, nwsObsForTier, NWS_MAX_PAIR_DELTA_SECONDS);
        const result = await submitCommitForAttestation(reading, stationId, contributor, {
            host,
            port,
            token,
            stateFilePath,
            commitBlockHeight: cycleBlockHeight,
            qualityTier,
            ...transport,
        });
        if (result.ok) {
            committed++;
            const dateStr = reading.recorded_at ? new Date(reading.recorded_at).toLocaleTimeString() : 'now';
            console.log(`  COMMIT OK [${stationId} / ${contributor.slice(0, 10)}…]: ${reading.temperature_c}°C @ ${dateStr} [quality_tier=${qualityTier}]`);
        } else {
            failed++;
            console.error(`  FAIL [${stationId}] (${result.phase} ${result.status}): ${redactSecrets(result.detail).slice(0, 300)}`);
        }
    }

    let settlement = { mined: recovered.mined || false, withheld: false };
    if (committed > 0) {
        settlement = await settlePendingAttestationBatch({
            ...stateOptions,
            currentBlockHeight: cycleBlockHeight,
        });
        if (settlement.excluded > 0) {
            // #3270: the mined block did not carry these reveals. Say so,
            // instead of counting them as mined.
            console.error(`  REJECTED [${stationId}]: ${settlement.excluded} reveal(s) not included in mined block ${settlement.height} (${REVEAL_NOT_INCLUDED})`);
        }
        if (settlement.mined) {
            const revealed = Number.isInteger(settlement.revealed) ? settlement.revealed : committed;
            console.log(`  MINED quorum-qualified reveal block ${settlement.height} (${revealed} readings for ${stationId})\n`);
        } else if (settlement.reason === REVEAL_NOT_INCLUDED) {
            emitFailureMetric('reveal_not_included', {
                station_id: stationId,
                contributor,
                height: settlement.height ?? null,
                excluded: settlement.excluded || 0,
            });
        } else {
            console.error(`  WITHHELD [${stationId}]: ${settlement.reason || 'attestation quorum not reached'}`);
            emitFailureMetric('attestation_quorum_withheld', {
                station_id: stationId,
                contributor,
                reason: settlement.reason || 'unknown',
            });
        }
    }

    const result = {
        committed,
        failed,
        mined: Boolean(settlement.mined),
        mineFailed: Boolean(settlement.mineFailed),
        mineStatus: settlement.mineStatus ?? settlement.mineResult?.status ?? null,
        height: settlement.height ?? null,
        withheld: Boolean(settlement.withheld),
        reason: settlement.reason || null,
        revealed: Number.isInteger(settlement.revealed) ? settlement.revealed : null,
        excluded: settlement.excluded || 0,
        skippedByWatermark,
        canonicalOutcome: committed === 0 ? recoveredCanonicalOutcome : null,
        commitIds: committed === 0 ? recoveredCanonicalOutcome?.commit_ids || [] : [],
        canonicalStationOutcomes: committed === 0
            ? recovered.canonicalStationOutcomes
            : settlement.canonicalStationOutcomes,
    };
    return {
        ...result,
        stationCycleOutcome: stationCycleOutcomeFromSubmission(result, stationId),
    };
}

module.exports = {
    // constants
    AMBIENT_BASE,
    NWS_MAX_PAIR_DELTA_SECONDS,
    PENDING_REVEAL_MAX_AGE_BLOCKS,
    PENDING_ATTESTATIONS_PAGE_LIMIT,
    ATTESTATION_POLL_INTERVAL_MS,
    ATTESTATION_MAX_POLLS,
    PENDING_PHASE_COMMITTED,
    PENDING_PHASE_AWAITING_QUORUM,
    DEFAULT_ATTESTATION_FRESHNESS_WINDOW_SECS,
    DEFAULT_PENDING_REVEAL_MAX_TIMEOUT_CYCLES,
    TERMINAL_OUTCOME_EXPIRED_UNQUALIFIED,
    REVEAL_NOT_INCLUDED,
    COLLECTOR_CYCLE_OUTCOMES,
    COLLECTOR_PENDING_REVEALS_PATH,
    MINER_ADDRESS_PATTERN,
    // re-export comparison helpers for convenience
    buildAccuracyReport,
    computeReadingQualityTier,
    normalizeReading,
    // logging / redaction
    redactSecrets,
    emitFailureMetric,
    emitCollectorOutcome,
    logStructured,
    // http
    httpsGet,
    httpGet,
    httpPost,
    sleep,
    isValidMinerAddress,
    postWithRetry,
    // ambient
    fetchDevices,
    fetchDeviceHistory,
    selectAmbientReadings,
    // nws
    nwsFetch,
    fetchNwsObservations,
    // hashing
    sortKeys,
    computeCommitId,
    // commit-authorization signing (#1848/#1967, v2 chain-domain fold #2034/#2076)
    commitAuthorizationMessage,
    commitAuthorizationMessageWithDomain,
    signingChainId,
    signingNetworkId,
    loadCollectorSigningKey,
    keyPairFromSeed,
    signCommitAuthorization,
    rawEd25519PublicKeyHex,
    computePendingRevealReadingId,
    computeNextLastSubmittedTs,
    getReadingTimestamp,
    // pending-reveal state
    getPendingRevealsPath,
    normalizePendingRevealEntry,
    loadPendingRevealEntries,
    savePendingRevealEntries,
    getTerminalOutcomesPath,
    loadTerminalOutcomes,
    recordTerminalOutcomes,
    upsertPendingRevealEntry,
    removePendingRevealEntry,
    createPendingRevealEntry,
    // per-station watermark
    sanitizeStationIdForFile,
    getStationWatermarkPath,
    readStationWatermark,
    writeStationWatermark,
    // node interaction
    getBlockchainHeight,
    submitContributionCommit,
    submitContributionReveal,
    processPendingReveals,
    submitCommitReveal,
    submitCommitForAttestation,
    fetchPendingAttestations,
    evaluateAttestationQuorum,
    awaitAttestationQuorum,
    reconcileExpiredAwaitingEntries,
    reconcileUnqualifiableAwaitingEntries,
    recordUnqualifiedCycle,
    resolveAttestationFreshnessWindowSecs,
    resolvePendingRevealMaxTimeoutCycles,
    submitEligiblePendingReveals,
    mineAwaitingQuorumEntries,
    classifyMinedRevealInclusion,
    settlePendingAttestationBatch,
    mineBlock,
    collectorMineEnabled,
    waitForHeartbeatInclusion,
    waitForHeartbeatCommits,
    // station loop
    fetchStationReadings,
    fetchRegisteredElevation,
    stationCycleOutcomeFromSubmission,
    submitStationReadings,
};
