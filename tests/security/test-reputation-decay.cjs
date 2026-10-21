'use strict';
// ---------------------------------------------------------------------------
// test-reputation-decay.cjs   (TDD-12.1 / Phase 3B positive path)
//
// Positive-path warning-band reputation decay gate: verifies that a
// warning-band contribution reveal (0.50 ≤ quality < 0.70) attested by a
// two-oracle quorum is accepted into a mined block and produces a
// quality event with quality_band = "warning_only" and warning_only = true,
// WITHOUT triggering a slashing event.
//
// Scenario
// ────────
// 1. Configure oracle set = [Oracle-A, Oracle-B], quorum = 2.
// 2. Submit one warning-band contribution (quality = 0.55):
//    a. POST /transactions/contribution/commit
//    b. POST /transactions/contribution/reveal
//    c. Both oracles attest (approved = true, quorum met).
//    d. POST /blocks/mine
// 3. Assert: quality event appears with quality_band = "warning_only"
//    and warning_only = true.
// 4. Assert: event_type is NOT "slashing" (reputation decay, not slash).
// 5. Assert: quality summary band_counters.warning_only increased.
//
// Exit codes: 0 = PASS, 1 = FAIL.
//
// Prerequisites:
//   WRFCOIN_OPERATOR_TOKEN env var set (64-char hex)
//   API server at :8556 (or WRFCOIN_API_HOST / WRFCOIN_API_PORT)
//
// Run:
//   WRFCOIN_OPERATOR_TOKEN=<token> node tests/security/test-reputation-decay.cjs
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
    scriptName: 'tests/security/test-reputation-decay.cjs',
    description: 'TDD-12.1 positive-path warning-band reputation decay gate.',
    requireToken: true,
});

const API_HOST = config.apiHost;
const API_PORT = config.apiPort;
const TOKEN    = config.operatorToken;

// quality = 0.55 is in the WarningOnly band: [0.50, 0.70).
const WARNING_QUALITY = 0.55;

// ---------------------------------------------------------------------------
// Crypto helpers
// ---------------------------------------------------------------------------

/**
 * Compute WRFCoin commit_hash for a contribution reveal.
 *
 * Mirrors Transaction::compute_commit_id in shared-protocol:
 *   preimage = json(data) + "||" + nonce + "||" + station_id + "||" + timestamp + "||" + contribution_type
 *   hash     = sha3-256(preimage)
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

const getJson  = (path)       => httpRequest('GET',  path, null, TOKEN);
const postJson = (path, body) => httpRequest('POST', path, body, TOKEN);
const putJson  = (path, body) => httpRequest('PUT',  path, body, TOKEN);

// ---------------------------------------------------------------------------
// Test runner
// ---------------------------------------------------------------------------

async function run() {
    console.log('🧪 TDD-12.1 (reputation-decay): warning-band decay gate — positive path\n');
    console.log(formatRuntimeReport(config));

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
    const baseWarningCount   = Number((baseSum.data.band_counters || {}).warning_only || 0);
    console.log(`   Baseline: slashing_events=${baseSlashingEvents}, band_counters.warning_only=${baseWarningCount}\n`);

    // ── 4. One round of warning-band contribution + quorum + mine ──────────
    const stationId     = `test-warning-${crypto.randomBytes(8).toString('hex')}`;
    const contributorId = '0000000000000000000000000000000000000055';

    const payload   = { quality: WARNING_QUALITY, volume: 1, uniqueness: 1 };
    const dataHash  = crypto
        .createHash('sha256')
        .update(JSON.stringify(sortKeys(payload)), 'utf8')
        .digest('hex');
    const nonce     = crypto.randomBytes(16).toString('hex');
    const timestamp = Math.floor(Date.now() / 1000);
    const commitHash = computeCommitHash(payload, nonce, stationId, timestamp, 'contribution');
    const commitAuth = signCommitAuthorization(stationId, commitHash, 'contribution', {
        signingSeed: 'phase3c-reputation-decay-fixture',
    });

    console.log(`   Station: ${stationId}`);
    console.log(`   Quality: ${WARNING_QUALITY} (WarningOnly band: [0.50, 0.70))`);

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
        contributor: contributorId,
        station_id:  stationId,
        payload,
        nonce,
        commit_hash: commitHash,
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
    const tsA    = timestamp + 1;
    const nonceA = `attest-warning-a`;
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
    const tsB    = timestamp + 2;
    const nonceB = `attest-warning-b`;
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

    // ── 5. Verify warning-band quality event fired ─────────────────────────
    const eventsRes = await getJson(
        `/transactions/quality/events?band=warning_only&offset=0&limit=50`
    );
    if (eventsRes.statusCode !== 200) {
        console.error(`❌ GET /transactions/quality/events (${eventsRes.statusCode}):`, eventsRes.data);
        process.exit(1);
    }

    const warningTotal = eventsRes.data.total || 0;
    const newWarning   = warningTotal - baseWarningCount;

    if (newWarning < 1) {
        console.error(
            `❌ FAIL: No warning_only quality event recorded after warning-band reveal.\n` +
            `       baseline_warning_events=${baseWarningCount}\n` +
            `       current_warning_events=${warningTotal}\n` +
            `       Expected at least 1 new WarningOnly event for station ${stationId}.`
        );
        process.exit(1);
    }
    console.log(`   ✅ Warning-band event(s) recorded: new_count=${newWarning} (total=${warningTotal})`);

    // Verify events have quality_band = "warning_only" and warning_only = true
    const items = eventsRes.data.items || [];
    const newItems = items.slice(0, newWarning);
    const badBand = newItems.filter(e => e.quality_band !== 'warning_only');
    if (badBand.length > 0) {
        console.error(
            `❌ FAIL: Some events don't have quality_band="warning_only": ` +
            badBand.map(e => `${e.event_id}→${e.quality_band}`).join(', ')
        );
        process.exit(1);
    }
    console.log(`   ✅ All new events have quality_band="warning_only"`);

    const withWarnFlag = newItems.filter(e => e.warning_only === true);
    if (withWarnFlag.length < newWarning) {
        console.error(
            `❌ FAIL: Some warning_only events have warning_only=false: ` +
            newItems.filter(e => !e.warning_only).map(e => `event_id=${e.event_id}`).join(', ')
        );
        process.exit(1);
    }
    console.log(`   ✅ All new events have warning_only=true (reputation decay, not slashing)`);

    // ── 6. Assert no new slashing events ───────────────────────────────────
    const afterSum = await getJson('/transactions/quality/summary');
    if (afterSum.statusCode === 200) {
        const afterSlashing = Number(afterSum.data.slashing_events || 0);
        if (afterSlashing > baseSlashingEvents) {
            console.error(
                `❌ FAIL: Slashing event fired for a warning-band (not low-band) contribution.\n` +
                `       before=${baseSlashingEvents}  after=${afterSlashing}\n` +
                `       WarningOnly events must NOT trigger slashing.`
            );
            process.exit(1);
        }
        console.log(`   ✅ No new slashing events (slashing_events=${afterSlashing}) — warning-only correctly separated from slash`);

        const afterWarning = Number((afterSum.data.band_counters || {}).warning_only || 0);
        if (afterWarning <= baseWarningCount) {
            console.error(
                `❌ FAIL: Quality summary band_counters.warning_only did not increase.\n` +
                `       before=${baseWarningCount}  after=${afterWarning}`
            );
            process.exit(1);
        }
        console.log(`   ✅ Quality summary band_counters.warning_only updated: ${baseWarningCount} → ${afterWarning}`);
    }

    console.log('\n✅ TDD-12.1 (reputation-decay) PASSED: warning-band decay gate validated.\n');
    process.exit(0);
}

run().catch(err => {
    console.error('❌ Unexpected error:', err.message || err);
    process.exit(1);
});
