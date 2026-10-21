'use strict';
// ---------------------------------------------------------------------------
// test-quality-slashing-no-quorum.cjs   (TDD-6.1)
//
// Negative-path quorum test: a commit attested by only ONE oracle (below the
// configured quorum of 2) must NOT trigger quality-band slashing events.
//
// Scenario
// ────────
// 1. Configure oracle set  = [Oracle-A, Oracle-B],  quorum = 2.
// 2. Oracle-A submits ONE valid attestation for commit X.
// 3. Mine a block so the attestation is applied to chain state.
// 4. Assert: commit X is in commits_pending_quorum (not yet satisfied).
// 5. Assert: zero quality-policy slashing events exist for commit X.
//
// Exit codes: 0 = PASS, 1 = FAIL.
//
// Prerequisites: WRFCOIN_OPERATOR_TOKEN env var set; API server at :8556.
//
// Run:
//   WRFCOIN_OPERATOR_TOKEN=<token> node tests/security/test-quality-slashing-no-quorum.cjs
// ---------------------------------------------------------------------------

const {
    exportToEnvironment,
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');

const config = loadTestConfig({
    scriptName: 'tests/security/test-quality-slashing-no-quorum.cjs',
    description: 'TDD-6.1 negative-quorum slashing regression.',
    requireToken: true,
});
exportToEnvironment(config);
const helpers = require('./oracle-test-helpers.cjs');

const TOKEN = config.operatorToken;

async function run() {
    console.log('🧪 TDD-6.1: No false-positive slashing when oracle quorum is not met\n');
    console.log(formatRuntimeReport(config));

    // ── 0. Health check ────────────────────────────────────────────────────
    const health = await helpers.getJson('/health', TOKEN);
    if (health.statusCode !== 200) {
        console.error(`❌ API server not reachable (GET /health → ${health.statusCode})`);
        process.exit(1);
    }
    console.log('   ✅ API server healthy');

    // ── 1. Generate two oracle key pairs ───────────────────────────────────
    const oracleA = helpers.phase3cOracleKeyPair('a');
    const oracleB = helpers.phase3cOracleKeyPair('b');
    console.log(`   Oracle-A: ${oracleA.pubKeyHex.slice(0, 16)}…`);
    console.log(`   Oracle-B: ${oracleB.pubKeyHex.slice(0, 16)}…`);

    // ── 2. Set oracle config: both oracles registered, quorum = 2 ──────────
    const cfgRes = await helpers.ensureOracleConfig(
        [oracleA.pubKeyHex, oracleB.pubKeyHex],
        2,
        TOKEN,
    );
    if (cfgRes.statusCode !== 200) {
        console.error(`❌ PUT /transactions/oracle/config failed (${cfgRes.statusCode}):`, cfgRes.data);
        process.exit(1);
    }
    console.log('   ✅ Oracle config updated: quorum=2, 2 oracles registered');

    // ── 3. Capture baseline oracle status ──────────────────────────────────
    const baseline = await helpers.getJson('/transactions/oracle/status', TOKEN);
    if (baseline.statusCode !== 200) {
        console.error(`❌ GET /transactions/oracle/status failed (${baseline.statusCode})`);
        process.exit(1);
    }
    const pendingBefore   = baseline.data.commits_pending_quorum  || 0;
    const withQuorumBefore = baseline.data.commits_with_quorum    || 0;
    console.log(`   Baseline: commits_pending_quorum=${pendingBefore}, commits_with_quorum=${withQuorumBefore}`);

    // ── 4. Oracle-A attests commit X (Oracle-B does NOT attest) ───────────
    const commitId = helpers.uniqueCommitId();
    const attest   = helpers.buildSignedAttestation(
        oracleA.privKeyDer,
        oracleA.pubKeyHex,
        commitId,
        true,   // approved
    );
    const attestRes = await helpers.postJson('/transactions/oracle/attestation', attest, TOKEN);
    if (attestRes.statusCode !== 200 && attestRes.statusCode !== 201) {
        console.error(`❌ Oracle-A attestation rejected (${attestRes.statusCode}):`, attestRes.data);
        process.exit(1);
    }
    console.log(`   ✅ Oracle-A attested commit ${commitId.slice(0, 8)}… (1/2 approvals — below quorum)`);

    // ── 5. Mine a block so the attestation enters chain state ─────────────
    const mineRes = await helpers.mineBlock(TOKEN);
    if (mineRes.statusCode !== 200 && mineRes.statusCode !== 201) {
        console.error(`❌ POST /blocks/mine failed (${mineRes.statusCode}):`, mineRes.data);
        process.exit(1);
    }
    console.log(`   ✅ Block mined (height=${mineRes.data.height || '?'})`);

    // ── 6. Verify commit is still pending quorum ───────────────────────────
    const statusRes = await helpers.getJson('/transactions/oracle/status', TOKEN);
    if (statusRes.statusCode !== 200) {
        console.error(`❌ GET /transactions/oracle/status failed (${statusRes.statusCode})`);
        process.exit(1);
    }
    const pendingAfter    = statusRes.data.commits_pending_quorum || 0;
    const withQuorumAfter = statusRes.data.commits_with_quorum    || 0;

    if (pendingAfter <= pendingBefore) {
        console.error(
            `❌ FAIL: commits_pending_quorum did not increase after single-oracle attestation.\n` +
            `       Before=${pendingBefore}  After=${pendingAfter}\n` +
            `       Expected After > Before (commit must wait for the second oracle).`,
        );
        process.exit(1);
    }
    console.log(`   ✅ Commit correctly in pending_quorum (pending=${pendingAfter})`);

    if (withQuorumAfter > withQuorumBefore) {
        console.error(
            `❌ FAIL: commits_with_quorum increased even though only ONE oracle voted.\n` +
            `       Before=${withQuorumBefore}  After=${withQuorumAfter}\n` +
            `       A single oracle attestation must NOT satisfy quorum=2.`,
        );
        process.exit(1);
    }
    console.log(`   ✅ commits_with_quorum unchanged (${withQuorumAfter}) — quorum correctly NOT met`);

    // ── 7. Verify zero quality-policy slashing events for this commit ──────
    const eventsRes = await helpers.getJson(
        `/transactions/quality/events?commit_id=${commitId}&event_type=slashing`,
        TOKEN,
    );
    if (eventsRes.statusCode !== 200) {
        console.error(`❌ GET /transactions/quality/events failed (${eventsRes.statusCode}):`, eventsRes.data);
        process.exit(1);
    }
    const slashingCount = eventsRes.data.total || 0;
    if (slashingCount > 0) {
        console.error(
            `❌ FAIL: Quality-band slashing event(s) fired for a commit without quorum.\n` +
            `       commit_id=${commitId}\n` +
            `       slashing_events=${slashingCount}\n` +
            `       No slashing must occur until quorum is reached.`,
        );
        process.exit(1);
    }
    console.log('   ✅ Zero quality-band slashing events for quorum-less commit');

    // ── 8. PASS ─────────────────────────────────────────────────────────────
    console.log('\n✅ TDD-6.1 PASSED: No false-positive slashing when oracle quorum is not met.\n');
    process.exit(0);
}

run().catch(err => {
    console.error('❌ Unexpected error:', err.message || err);
    process.exit(1);
});
