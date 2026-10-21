'use strict';
// ---------------------------------------------------------------------------
// test-quality-slashing.cjs   (TDD-12.1 / Phase 3B positive path)
//
// Positive-path quality slashing gate: verifies that three consecutive
// low-band contribution reveals (quality < 0.50) from the same station,
// each attested by a two-oracle quorum, trigger a SlashingApplied quality
// policy event after the default streak threshold (3) is reached.
//
// Scenario
// ────────
// 1. Configure oracle set = [Oracle-A, Oracle-B], quorum = 2.
// 2. For each of 3 rounds:
//    a. Generate a contribution commit with quality = 0.20 (Low band).
//    b. POST /transactions/contribution/commit
//    c. POST /transactions/contribution/reveal
//    d. Both oracles attest the commit (approved = true, quorum met).
//    e. POST /blocks/mine
// 3. Assert: slashing event appears in /transactions/quality/events
//    with quality_band = "low" and event_type = "slashing".
// 4. Assert: quality summary slashing_events count increased.
//
// Exit codes: 0 = PASS, 1 = FAIL.
//
// Prerequisites:
//   WRFCOIN_OPERATOR_TOKEN env var set (64-char hex)
//   API server at :8556 (or WRFCOIN_API_HOST / WRFCOIN_API_PORT)
//
// Run:
//   WRFCOIN_OPERATOR_TOKEN=<token> node tests/security/test-quality-slashing.cjs
// ---------------------------------------------------------------------------

const crypto = require('crypto');
const http   = require('http');
const {
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');
const { signCommitAuthorization, sortKeys } = require('./lib/ambient-pipeline.cjs');
const oracleHelpers = require('./oracle-test-helpers.cjs');

const config = loadTestConfig({
    scriptName: 'tests/security/test-quality-slashing.cjs',
    description: 'TDD-12.1 positive-path low-band slashing gate.',
    requireToken: true,
});

const API_HOST = config.apiHost;
const API_PORT = config.apiPort;
const TOKEN    = config.operatorToken;

// Default slash streak from DataQualityPolicy::default().low_quality_slash_streak = 3.
const STREAK_ROUNDS = 3;
// quality = 0.20 is firmly in the Low band (threshold is 0.50).
const LOW_QUALITY   = 0.20;

// ---------------------------------------------------------------------------
// Crypto helpers
// ---------------------------------------------------------------------------

/**
 * Compute WRFCoin commit_hash for a contribution reveal.
 *
 * Mirrors Transaction::compute_commit_id in shared-protocol:
 *   preimage = json(data) + "||" + nonce + "||" + station_id + "||" + timestamp + "||" + contribution_type
 *   hash     = sha3-256(preimage)
 *
 * IMPORTANT: data must be serialised with integer values for whole numbers
 * (e.g. volume=1, not 1.0) to match serde_json's re-serialisation of the
 * parsed Value when the server computes the hash on its side.
 */
function computeCommitHash(data, nonce, stationId, timestamp, contributionType) {
    const dataStr  = JSON.stringify(sortKeys(data));
    const preimage = `${dataStr}||${nonce}||${stationId}||${timestamp}||${contributionType}`;
    return crypto.createHash('sha3-256').update(preimage, 'utf8').digest('hex');
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
                ...(token ? { Authorization: `Bearer ${token}` } : {}),
            },
        };
        const req = http.request(options, (res) => {
            let data = '';
            res.on('data', chunk => { data += chunk; });
            res.on('end', () => {
                try { resolve({ statusCode: res.statusCode, data: JSON.parse(data) }); }
                catch (_) { resolve({ statusCode: res.statusCode, data }); }
            });
        });
        req.on('error', err => resolve({ statusCode: 0, error: err.message }));
        if (bodyStr) req.write(bodyStr);
        req.end();
    });
}

const getJson  = (path)        => httpRequest('GET',  path, null, TOKEN);
const postJson = (path, body)  => httpRequest('POST', path, body, TOKEN);
const putJson  = (path, body)  => httpRequest('PUT',  path, body, TOKEN);

// ---------------------------------------------------------------------------
// Test runner
// ---------------------------------------------------------------------------

async function run() {
    console.log('🧪 TDD-12.1 (quality-slashing): low-band slashing gate — positive path\n');
    console.log(formatRuntimeReport(config, { streak_rounds: STREAK_ROUNDS }));

    // ── 0. Health check ────────────────────────────────────────────────────
    const health = await getJson('/health');
    if (health.statusCode !== 200) {
        console.error(`❌ API not reachable (GET /health → ${health.statusCode})`);
        process.exit(1);
    }
    console.log('   ✅ API server healthy');

    // ── 1. Generate oracle key pairs ───────────────────────────────────────
    const oracleA = oracleHelpers.phase3cOracleKeyPair('a');
    const oracleB = oracleHelpers.phase3cOracleKeyPair('b');
    console.log(`   Oracle-A: ${oracleA.pubKeyHex.slice(0, 16)}…`);
    console.log(`   Oracle-B: ${oracleB.pubKeyHex.slice(0, 16)}…`);

    // ── 2. Configure oracle set: quorum = 2 ────────────────────────────────
    const cfgRes = await oracleHelpers.ensureOracleConfig(
        [oracleA.pubKeyHex, oracleB.pubKeyHex],
        2,
        TOKEN,
    );
    if (cfgRes.statusCode !== 200) {
        console.error(`❌ PUT /transactions/oracle/config (${cfgRes.statusCode}):`, cfgRes.data);
        process.exit(1);
    }
    console.log('   ✅ Oracle config: quorum=2, 2 oracles registered');

    // ── 3. Baseline quality summary ────────────────────────────────────────
    const baseSum = await getJson('/transactions/quality/summary');
    if (baseSum.statusCode !== 200) {
        console.error(`❌ GET /transactions/quality/summary (${baseSum.statusCode})`);
        process.exit(1);
    }
    const baseSlashingEvents = Number(baseSum.data.slashing_events || 0);
    const baseLowCount       = Number((baseSum.data.band_counters || {}).low || 0);
    console.log(`   Baseline: slashing_events=${baseSlashingEvents}, band_counters.low=${baseLowCount}\n`);

    // ── 4. Three rounds of low-quality contribution + quorum + mine ────────
    const stationId    = `test-slashing-${crypto.randomBytes(8).toString('hex')}`;
    const contributorId = '0000000000000000000000000000000000000001';

    for (let round = 1; round <= STREAK_ROUNDS; round++) {
        console.log(`   ── Round ${round}/${STREAK_ROUNDS} (quality=${LOW_QUALITY}) ──`);

        const payload    = { quality: LOW_QUALITY, volume: 1, uniqueness: 1 };
        const dataHash   = crypto
            .createHash('sha256')
            .update(JSON.stringify(sortKeys(payload)), 'utf8')
            .digest('hex');
        const nonce      = crypto.randomBytes(16).toString('hex');
        const timestamp  = Math.floor(Date.now() / 1000) + round;  // ensure unique per round
        const commitHash = computeCommitHash(payload, nonce, stationId, timestamp, 'contribution');
        const commitAuth = signCommitAuthorization(stationId, commitHash, 'contribution', {
            signingSeed: 'phase3c-quality-slashing-fixture',
        });

        // 4a. Contribution commit
        const commitRes = await postJson('/transactions/contribution/commit', {
            contributor: contributorId,
            station_id:  stationId,
            commit_hash: commitHash,
            ...commitAuth,
        });
        if (commitRes.statusCode !== 200 && commitRes.statusCode !== 201) {
            console.error(`   ❌ POST /transactions/contribution/commit (${commitRes.statusCode}):`, commitRes.data);
            process.exit(1);
        }
        console.log(`   ✅ Commit accepted (${commitRes.statusCode}), commit_hash=${commitHash.slice(0, 8)}…`);

        // 4b. Contribution reveal
        const revealRes = await postJson('/transactions/contribution/reveal', {
            contributor:  contributorId,
            station_id:   stationId,
            payload,
            nonce,
            commit_hash:  commitHash,
            timestamp,
            data_cid: 'QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG', // pragma: allowlist secret
            data_hash: dataHash,
            storage_provider: 'ipfs',
        });
        if (revealRes.statusCode !== 200 && revealRes.statusCode !== 201) {
            console.error(`   ❌ POST /transactions/contribution/reveal (${revealRes.statusCode}):`, revealRes.data);
            process.exit(1);
        }
        console.log(`   ✅ Reveal accepted (${revealRes.statusCode})`);

        // 4c. Oracle-A attestation
        const tsA     = timestamp + 1;
        const nonceA  = `attest-a-r${round}`;
        const attARes = await postJson(
            '/transactions/oracle/attestation',
            oracleHelpers.buildSignedAttestation(
                oracleA.privKeyDer,
                oracleA.pubKeyHex,
                commitHash,
                true,
                { timestamp: tsA, nonce: nonceA },
            ),
        );
        if (attARes.statusCode !== 200 && attARes.statusCode !== 201) {
            console.error(`   ❌ Oracle-A attestation (${attARes.statusCode}):`, attARes.data);
            process.exit(1);
        }

        // 4d. Oracle-B attestation
        const tsB     = timestamp + 2;
        const nonceB  = `attest-b-r${round}`;
        const attBRes = await postJson(
            '/transactions/oracle/attestation',
            oracleHelpers.buildSignedAttestation(
                oracleB.privKeyDer,
                oracleB.pubKeyHex,
                commitHash,
                true,
                { timestamp: tsB, nonce: nonceB },
            ),
        );
        if (attBRes.statusCode !== 200 && attBRes.statusCode !== 201) {
            console.error(`   ❌ Oracle-B attestation (${attBRes.statusCode}):`, attBRes.data);
            process.exit(1);
        }
        console.log(`   ✅ Both oracles attested (quorum=2 met)`);

        // 4e. Mine a block
        const mineRes = await postJson('/blocks/mine', {
            miner_address: '0000000000000000000000000000000000000001',
        });
        if (mineRes.statusCode !== 200 && mineRes.statusCode !== 201) {
            console.error(`   ❌ POST /blocks/mine (${mineRes.statusCode}):`, mineRes.data);
            process.exit(1);
        }
        console.log(`   ✅ Block mined (height=${mineRes.data.height || '?'})\n`);
    }

    // ── 5. Verify slashing event fired ─────────────────────────────────────
    const eventsRes = await getJson(
        `/transactions/quality/events?event_type=slashing&offset=0&limit=50`
    );
    if (eventsRes.statusCode !== 200) {
        console.error(`❌ GET /transactions/quality/events (${eventsRes.statusCode}):`, eventsRes.data);
        process.exit(1);
    }

    const slashingTotal = eventsRes.data.total || 0;
    const newSlashing   = slashingTotal - baseSlashingEvents;

    if (newSlashing < 1) {
        console.error(
            `❌ FAIL: No slashing event recorded after ${STREAK_ROUNDS} consecutive low-band reveals.\n` +
            `       baseline_slashing_events=${baseSlashingEvents}\n` +
            `       current_slashing_events=${slashingTotal}\n` +
            `       Expected at least 1 new SlashingApplied event for station ${stationId}.`
        );
        process.exit(1);
    }
    console.log(`   ✅ Slashing event(s) recorded: new_count=${newSlashing} (total=${slashingTotal})`);

    // Verify at least one slashing event has quality_band = "low"
    const slashingItems = eventsRes.data.items || [];
    const hasLowBandSlash = slashingItems.some(e => e.quality_band === 'low');
    if (!hasLowBandSlash && slashingItems.length > 0) {
        console.error(
            `❌ FAIL: Slashing event(s) found but none have quality_band="low".\n` +
            `       Bands seen: [${[...new Set(slashingItems.map(e => e.quality_band))].join(', ')}]`
        );
        process.exit(1);
    }
    if (hasLowBandSlash) {
        console.log(`   ✅ At least one slashing event has quality_band="low"`);
    }

    // ── 6. Verify quality summary updated ──────────────────────────────────
    const afterSum = await getJson('/transactions/quality/summary');
    if (afterSum.statusCode === 200) {
        const afterSlashing = Number(afterSum.data.slashing_events || 0);
        const afterLow      = Number((afterSum.data.band_counters || {}).low || 0);
        if (afterSlashing <= baseSlashingEvents) {
            console.error(
                `❌ FAIL: Quality summary slashing_events did not increase.\n` +
                `       before=${baseSlashingEvents}  after=${afterSlashing}`
            );
            process.exit(1);
        }
        if (afterLow <= baseLowCount) {
            console.error(
                `❌ FAIL: Quality summary band_counters.low did not increase.\n` +
                `       before=${baseLowCount}  after=${afterLow}`
            );
            process.exit(1);
        }
        console.log(`   ✅ Quality summary updated: slashing_events=${afterSlashing}, band_counters.low=${afterLow}`);
    }

    console.log('\n✅ TDD-12.1 (quality-slashing) PASSED: low-band slashing gate validated.\n');
    process.exit(0);
}

run().catch(err => {
    console.error('❌ Unexpected error:', err.message || err);
    process.exit(1);
});
