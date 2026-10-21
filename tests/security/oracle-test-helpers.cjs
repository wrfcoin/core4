'use strict';
// ---------------------------------------------------------------------------
// oracle-test-helpers.cjs
//
// Shared helpers for Phase-3C negative-quorum API tests.
//
// Design notes
// ─────────────
// Oracle attestation payloads are signed with Ed25519.  The signing message
// is a domain-separated canonical envelope whose payload digest is SHA3-256
// of the compact-JSON-serialised signing fields — matching the Rust
// implementation in shared-protocol/src/crypto/domain_signing.rs and
// shared-protocol/src/blockchain/transaction.rs.
//
// Requirements: Node.js ≥ 18 (built-in Ed25519 + SHA3-256 via crypto module).
// ---------------------------------------------------------------------------

const crypto = require('crypto');
const http   = require('http');
const { loadTestConfig } = require('../utils/test-config.cjs');
const { keyPairFromSeed } = require('./lib/ambient-pipeline.cjs');

const config = loadTestConfig({
    argv: [],
    scriptName: 'tests/security/oracle-test-helpers.cjs',
    description: 'Shared oracle attestation helpers.',
    requireToken: false,
});

const API_HOST = config.apiHost;
const API_PORT = config.apiPort;
const TEST_MINER_ADDRESS = '0000000000000000000000000000000000000001';

// ---------------------------------------------------------------------------
// Cryptographic helpers
// ---------------------------------------------------------------------------

/**
 * Generate an Ed25519 key pair for use as a test oracle identity.
 *
 * Returns:
 *   pubKeyHex  – 64-char lowercase hex (32-byte raw Ed25519 public key).
 *                This is the oracle_id expected by the WRFCoin API.
 *   privKeyDer – PKCS#8 DER Buffer for signing (kept in-process only).
 */
function generateOracleKeyPair() {
    const { privateKey, publicKey } = crypto.generateKeyPairSync('ed25519');

    // SubjectPublicKeyInfo DER for Ed25519 is 44 bytes; the last 32 are the
    // raw public key.
    const spkiDer  = publicKey.export({ type: 'spki', format: 'der' });
    const pubKeyHex = spkiDer.slice(-32).toString('hex');

    const privKeyDer = privateKey.export({ type: 'pkcs8', format: 'der' });

    return { pubKeyHex, privKeyDer };
}

function phase3cOracleKeyPair(label) {
    const seed = crypto
        .createHash('sha256')
        .update(`wrfcoin-phase3c-oracle-${label}`, 'utf8')
        .digest();
    const key = keyPairFromSeed(seed);
    return {
        pubKeyHex: key.pubHex,
        privKeyDer: key.privateKey.export({ type: 'pkcs8', format: 'der' }),
    };
}

/**
 * Build the canonical oracle-attestation signing message that the Rust server
 * expects.
 *
 * Mirrors:
 *   shared-protocol/src/crypto/domain_signing.rs :: canonical_signing_message()
 *   shared-protocol/src/blockchain/transaction.rs :: OracleAttestationPayload::signing_message()
 *
 * Format:
 *   "wrfcoin-sign-v1|wrfcoin.oracle_attestation.v1|wrfcoin-main|mainnet
 *    |{oracle_id}|{timestamp}|{nonce}|{payload_digest}"
 *
 * payload_digest = sha3-256( compact_json( signing_payload ) ) as lowercase hex
 *
 * signing_payload fields (in declaration order, matching the Rust struct):
 *   commit_id, oracle_id, approved, confidence, timestamp, nonce
 */
function buildSigningMessage(commitId, oracleId, approved, confidence, timestamp, nonce) {
    // Step 1 – build the signing payload JSON with fields in the EXACT same
    // declaration order as Rust's OracleAttestationSigningPayload struct.
    // Use null for absent Option<T> fields so JSON.stringify produces "null"
    // matching serde_json's serialisation of Option::None.
    const signingPayload = {
        commit_id:  commitId,
        oracle_id:  oracleId,
        approved:   approved,
        confidence: (confidence !== undefined && confidence !== null) ? confidence : null,
        timestamp:  timestamp,
        nonce:      (nonce !== undefined && nonce !== null) ? nonce : null,
    };

    // Step 2 – SHA3-256 digest of the compact JSON bytes (no spaces / sorting).
    const jsonBytes     = Buffer.from(JSON.stringify(signingPayload), 'utf8');
    const payloadDigest = crypto.createHash('sha3-256').update(jsonBytes).digest('hex');

    // Step 3 – assemble the canonical envelope.
    const nonceStr = (nonce !== undefined && nonce !== null) ? String(nonce) : '';
    return [
        'wrfcoin-sign-v1',
        'wrfcoin.oracle_attestation.v1',
        'wrfcoin-main',
        'mainnet',
        oracleId,
        String(timestamp),
        nonceStr,
        payloadDigest,
    ].join('|');
}

/**
 * Sign a UTF-8 message string with an Ed25519 private key.
 *
 * Returns the 64-byte signature as lowercase hex.
 */
function signMessage(privKeyDer, message) {
    const privateKey = crypto.createPrivateKey({
        key:    privKeyDer,
        type:   'pkcs8',
        format: 'der',
    });
    return crypto.sign(null, Buffer.from(message, 'utf8'), privateKey).toString('hex');
}

/**
 * Assemble a fully-signed oracle attestation request body ready for
 * POST /transactions/oracle/attestation.
 *
 * @param {Buffer} privKeyDer   – PKCS#8 DER private key
 * @param {string} oracleId     – 64-char hex public key
 * @param {string} commitId     – 64-char hex commit hash
 * @param {boolean} approved
 * @param {object} [opts]
 * @param {number|null} [opts.confidence]  – float in [0,1] or null
 * @param {string|null} [opts.nonce]
 * @param {number}      [opts.timestamp]   – unix seconds (defaults to now)
 */
function buildSignedAttestation(privKeyDer, oracleId, commitId, approved, opts) {
    opts = opts || {};
    const confidence = (opts.confidence !== undefined) ? opts.confidence : null;
    const nonce      = (opts.nonce      !== undefined)
        ? opts.nonce
        : crypto.randomBytes(16).toString('hex');
    const timestamp  = opts.timestamp || Math.floor(Date.now() / 1000);

    const message   = buildSigningMessage(commitId, oracleId, approved, confidence, timestamp, nonce);
    const signature = signMessage(privKeyDer, message);

    // Build request body (omit undefined-optional fields so the server's
    // JSON deserialiser sees them as absent, keeping parity with Option::None).
    const body = {
        oracle_id:   oracleId,
        commit_hash: commitId,
        approved,
        signature,
        timestamp,
    };
    if (confidence !== null) body.confidence = confidence;
    if (nonce      !== null) body.nonce      = nonce;

    return body;
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

function httpRequest(method, path, body, token) {
    return new Promise((resolve) => {
        const bodyStr = body ? JSON.stringify(body) : '';
        const options = {
            hostname: API_HOST,
            port:     API_PORT,
            path,
            method,
            headers: {
                'Content-Type':   'application/json',
                'Content-Length': Buffer.byteLength(bodyStr),
                ...(token ? { 'Authorization': `Bearer ${token}` } : {}),
            },
        };

        const req = http.request(options, (res) => {
            let data = '';
            res.on('data', chunk => { data += chunk; });
            res.on('end', () => {
                try {
                    resolve({ statusCode: res.statusCode, data: JSON.parse(data) });
                } catch (_) {
                    resolve({ statusCode: res.statusCode, data });
                }
            });
        });
        req.on('error', err => resolve({ statusCode: 0, error: err.message }));
        if (bodyStr) req.write(bodyStr);
        req.end();
    });
}

const getJson  = (path, token) => httpRequest('GET',  path, null,  token);
const postJson = (path, body, token) => httpRequest('POST', path, body, token);
const putJson  = (path, body, token) => httpRequest('PUT',  path, body, token);

async function ensureOracleConfig(oracleIds, oracleQuorum, token) {
    const expected = [...oracleIds].sort();
    const current = await getJson('/transactions/oracle/config', token);
    if (current.statusCode !== 200) return current;

    const currentSet = [...(current.data.oracle_set || [])].sort();
    if (
        current.data.oracle_quorum === oracleQuorum
        && JSON.stringify(currentSet) === JSON.stringify(expected)
    ) {
        return { statusCode: 200, data: current.data, reused: true };
    }
    if (currentSet.length > 0) {
        return {
            statusCode: 409,
            data: {
                error: 'Phase 3C fixture oracle config differs from the active chain config',
                expected: { oracle_set: expected, oracle_quorum: oracleQuorum },
                current: current.data,
            },
        };
    }
    return putJson(
        '/transactions/oracle/config',
        { oracle_set: expected, oracle_quorum: oracleQuorum },
        token,
    );
}

/**
 * Mine one block via POST /blocks/mine.
 * Returns the HTTP response { statusCode, data }.
 */
async function mineBlock(token) {
    return postJson('/blocks/mine', { miner_address: TEST_MINER_ADDRESS }, token);
}

// ---------------------------------------------------------------------------
// Unique commit ID generator
// ---------------------------------------------------------------------------

/**
 * Generate a random 64-character hex string suitable for use as a
 * commit_hash in API calls (matches server's validate_commit_hash).
 */
function uniqueCommitId() {
    return crypto.randomBytes(32).toString('hex');
}

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

module.exports = {
    generateOracleKeyPair,
    phase3cOracleKeyPair,
    buildSignedAttestation,
    buildSigningMessage,
    signMessage,
    getJson,
    postJson,
    putJson,
    ensureOracleConfig,
    mineBlock,
    uniqueCommitId,
    TEST_MINER_ADDRESS,
};
