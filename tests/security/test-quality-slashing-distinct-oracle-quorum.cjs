'use strict';
// ---------------------------------------------------------------------------
// test-quality-slashing-distinct-oracle-quorum.cjs   (TDD-6.3)
//
// Negative-path quorum test: quorum=2 is satisfied ONLY when two DISTINCT
// oracle IDs approve the same commit.  A single oracle cannot satisfy quorum
// by voting multiple times.
//
// Scenario  (two-phase)
// ─────────────────────
// Phase 1 — Negative path (quorum must NOT be reached with one oracle):
//   1a. Configure oracle set = [Oracle-A, Oracle-B],  quorum = 2.
//   1b. Oracle-A submits attestation (1st) for commit X.
//   1c. Oracle-A submits the same commit again (duplicate, different timestamp).
//   1d. Mine a block.
//   1e. Assert commit X is in commits_pending_quorum (not with_quorum).
//       → One oracle, even voting twice, cannot satisfy quorum of 2.
//
// Phase 2 — Positive path (distinct oracle satisfies quorum):
//   2a. Oracle-B submits attestation for commit X.
//   2b. Mine another block.
//   2c. Assert commit X has moved to commits_with_quorum.
//       → A second DISTINCT oracle pushed the commit over the quorum threshold.
//
// The phase-2 positive assertion makes the negative phase meaningful: we know
// the commit itself is valid and would reach quorum given distinct IDs.
//
// Exit codes: 0 = PASS, 1 = FAIL.
//
// Prerequisites: WRFCOIN_OPERATOR_TOKEN env var set; API server at :8556.
//
// Run:
//   WRFCOIN_OPERATOR_TOKEN=<token> node tests/security/test-quality-slashing-distinct-oracle-quorum.cjs
// ---------------------------------------------------------------------------

const {
    exportToEnvironment,
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');

const config = loadTestConfig({
    scriptName: 'tests/security/test-quality-slashing-distinct-oracle-quorum.cjs',
    description: 'TDD-6.3 distinct-oracle quorum regression.',
    requireToken: true,
});
exportToEnvironment(config);
const helpers = require('./oracle-test-helpers.cjs');

const TOKEN = config.operatorToken;

async function run() {
    console.log('🧪 TDD-6.3: Quorum requires DISTINCT oracle IDs (not repeated votes from one oracle)\n');
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
    console.log('   ✅ Oracle config updated: quorum=2, 2 oracles registered\n');

    // ── 3. Capture baseline oracle status ──────────────────────────────────
    const baseline = await helpers.getJson('/transactions/oracle/status', TOKEN);
    if (baseline.statusCode !== 200) {
        console.error(`❌ GET /transactions/oracle/status failed (${baseline.statusCode})`);
        process.exit(1);
    }
    const pendingBefore    = baseline.data.commits_pending_quorum || 0;
    const withQuorumBefore = baseline.data.commits_with_quorum    || 0;
    console.log(`   Baseline: commits_pending_quorum=${pendingBefore}, commits_with_quorum=${withQuorumBefore}`);

    // ── PHASE 1: Negative path ─────────────────────────────────────────────
    console.log('\n   ── Phase 1: single oracle voting twice must NOT reach quorum ──\n');

    // 4a. Oracle-A attests commit X (first vote)
    const commitId = helpers.uniqueCommitId();
    const ts1      = Math.floor(Date.now() / 1000);

    const attestA1 = helpers.buildSignedAttestation(
        oracleA.privKeyDer, oracleA.pubKeyHex, commitId, true, { timestamp: ts1 },
    );
    const resA1 = await helpers.postJson('/transactions/oracle/attestation', attestA1, TOKEN);
    if (resA1.statusCode !== 200 && resA1.statusCode !== 201) {
        console.error(`❌ Oracle-A first attestation failed (${resA1.statusCode}):`, resA1.data);
        process.exit(1);
    }
    console.log(`   ✅ Oracle-A attested commit ${commitId.slice(0, 8)}… (1st vote, ts=${ts1})`);

    // 4b. Oracle-A attests the same commit again (duplicate, ts2 = ts1 + 1)
    const ts2 = ts1 + 1;
    const attestA2 = helpers.buildSignedAttestation(
        oracleA.privKeyDer, oracleA.pubKeyHex, commitId, true, { timestamp: ts2 },
    );
    const resA2 = await helpers.postJson('/transactions/oracle/attestation', attestA2, TOKEN);
    const replayRejected = resA2.statusCode === 400
        && JSON.stringify(resA2.data).includes('Launch protocol replay detected');
    if (resA2.statusCode !== 200 && resA2.statusCode !== 201 && !replayRejected) {
        console.error(`❌ Oracle-A duplicate attestation HTTP request failed (${resA2.statusCode}):`, resA2.data);
        process.exit(1);
    }
    console.log(
        replayRejected
            ? '   ✅ Duplicate rejected at mempool admission by the launch replay guard'
            : `   ✅ Duplicate enqueued (${resA2.statusCode}) for state-level deduplication`,
    );

    // 4c. Mine block — processes both mempool transactions; duplicate is dropped
    const mine1 = await helpers.mineBlock(TOKEN);
    if (mine1.statusCode !== 200 && mine1.statusCode !== 201) {
        console.error(`❌ POST /blocks/mine (phase-1) failed (${mine1.statusCode}):`, mine1.data);
        process.exit(1);
    }
    console.log(`   ✅ Block mined (height=${mine1.data.height || '?'})`);

    // 4d. Verify commit is still pending quorum after phase-1
    const status1 = await helpers.getJson('/transactions/oracle/status', TOKEN);
    if (status1.statusCode !== 200) {
        console.error(`❌ GET /transactions/oracle/status failed (${status1.statusCode})`);
        process.exit(1);
    }
    const pendingPhase1    = status1.data.commits_pending_quorum || 0;
    const withQuorumPhase1 = status1.data.commits_with_quorum    || 0;

    if (pendingPhase1 <= pendingBefore) {
        console.error(
            `❌ FAIL (Phase 1): commits_pending_quorum did not increase after Oracle-A's vote.\n` +
            `       Before=${pendingBefore}  After=${pendingPhase1}`,
        );
        process.exit(1);
    }
    console.log(`   ✅ Commit in pending_quorum (pending=${pendingPhase1})`);

    if (withQuorumPhase1 > withQuorumBefore) {
        console.error(
            `❌ FAIL (Phase 1 negative assertion): commits_with_quorum increased!\n` +
            `       Before=${withQuorumBefore}  After=${withQuorumPhase1}\n` +
            `       Oracle-A voted twice; the duplicate must be ignored.\n` +
            `       A single oracle identity CANNOT satisfy quorum=2.`,
        );
        process.exit(1);
    }
    console.log(`   ✅ commits_with_quorum unchanged (${withQuorumPhase1}) — one oracle is not enough`);

    // No quality slashing should have fired yet
    const events1 = await helpers.getJson(
        `/transactions/quality/events?commit_id=${commitId}&event_type=slashing`,
        TOKEN,
    );
    if ((events1.data || {}).total > 0) {
        console.error(
            `❌ FAIL (Phase 1): Slashing fired for quorum-less commit. total=${events1.data.total}`,
        );
        process.exit(1);
    }
    console.log('   ✅ Zero slashing events after phase-1 (quorum not met → no false-positive slashing)');

    // ── PHASE 2: Positive path ─────────────────────────────────────────────
    console.log('\n   ── Phase 2: second DISTINCT oracle satisfies quorum ──\n');

    // 5a. Oracle-B attests the same commit X
    const ts3      = ts1 + 2;
    const attestB  = helpers.buildSignedAttestation(
        oracleB.privKeyDer, oracleB.pubKeyHex, commitId, true, { timestamp: ts3 },
    );
    const resB = await helpers.postJson('/transactions/oracle/attestation', attestB, TOKEN);
    if (resB.statusCode !== 200 && resB.statusCode !== 201) {
        console.error(`❌ Oracle-B attestation failed (${resB.statusCode}):`, resB.data);
        process.exit(1);
    }
    console.log(`   ✅ Oracle-B attested commit ${commitId.slice(0, 8)}… (distinct oracle, ts=${ts3})`);

    // 5b. Mine another block
    const mine2 = await helpers.mineBlock(TOKEN);
    if (mine2.statusCode !== 200 && mine2.statusCode !== 201) {
        console.error(`❌ POST /blocks/mine (phase-2) failed (${mine2.statusCode}):`, mine2.data);
        process.exit(1);
    }
    console.log(`   ✅ Block mined (height=${mine2.data.height || '?'})`);

    // 5c. Verify commit has moved to with_quorum
    const status2 = await helpers.getJson('/transactions/oracle/status', TOKEN);
    if (status2.statusCode !== 200) {
        console.error(`❌ GET /transactions/oracle/status failed (${status2.statusCode})`);
        process.exit(1);
    }
    const withQuorumPhase2 = status2.data.commits_with_quorum    || 0;
    const pendingPhase2    = status2.data.commits_pending_quorum  || 0;

    if (withQuorumPhase2 <= withQuorumBefore) {
        console.error(
            `❌ FAIL (Phase 2): commits_with_quorum did not increase after Oracle-B's distinct vote.\n` +
            `       Before=${withQuorumBefore}  After=${withQuorumPhase2}\n` +
            `       Two distinct oracles should have satisfied quorum=2.`,
        );
        process.exit(1);
    }
    console.log(`   ✅ commits_with_quorum increased to ${withQuorumPhase2} — quorum reached with 2 distinct oracles`);

    // The commit should no longer appear in pending_quorum (it graduated)
    if (pendingPhase2 >= pendingPhase1) {
        // This is a soft warning, not a hard failure — other commits may have
        // been added concurrently.  The critical assertion is that with_quorum grew.
        console.log(`   ℹ️  commits_pending_quorum=${pendingPhase2} (may include concurrent commits)`);
    } else {
        console.log(`   ✅ commits_pending_quorum decreased (${pendingPhase1} → ${pendingPhase2}) — commit graduated`);
    }

    // ── PASS ──────────────────────────────────────────────────────────────
    console.log('\n✅ TDD-6.3 PASSED: Quorum requires distinct oracle IDs — repeated votes from one oracle are insufficient.\n');
    process.exit(0);
}

run().catch(err => {
    console.error('❌ Unexpected error:', err.message || err);
    process.exit(1);
});
