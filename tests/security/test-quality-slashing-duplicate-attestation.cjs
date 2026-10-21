'use strict';
// ---------------------------------------------------------------------------
// test-quality-slashing-duplicate-attestation.cjs   (TDD-6.2)
//
// Negative-path quorum test: a duplicate oracle vote (same oracle_id, same
// commit_hash submitted twice) must NOT count as two distinct approvals and
// therefore must NOT satisfy quorum=2.
//
// Scenario
// ────────
// 1. Configure oracle set  = [Oracle-A, Oracle-B],  quorum = 2.
// 2. Oracle-A submits a valid attestation for commit X  → accepted (201).
// 3. Oracle-A submits THE SAME attestation again (different timestamp, so the
//    Ed25519 signature differs, but oracle_id and commit_hash are identical).
// 4. Mine a block.  The block processor applies both transactions; the second
//    is silently dropped by the duplicate-detection guard in apply_attestation.
// 5. Assert: commit X is still in commits_pending_quorum (not moved to
//    commits_with_quorum) — the duplicate did not count as a second vote.
// 6. Assert: zero quality-policy slashing events for commit X.
//
// Exit codes: 0 = PASS, 1 = FAIL.
//
// Prerequisites: WRFCOIN_OPERATOR_TOKEN env var set; API server at :8556.
//
// Run:
//   WRFCOIN_OPERATOR_TOKEN=<token> node tests/security/test-quality-slashing-duplicate-attestation.cjs
// ---------------------------------------------------------------------------

const {
    exportToEnvironment,
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');

const config = loadTestConfig({
    scriptName: 'tests/security/test-quality-slashing-duplicate-attestation.cjs',
    description: 'TDD-6.2 duplicate-attestation quorum regression.',
    requireToken: true,
});
exportToEnvironment(config);
const helpers = require('./oracle-test-helpers.cjs');

const TOKEN = config.operatorToken;

async function run() {
    console.log('🧪 TDD-6.2: Duplicate oracle votes must NOT satisfy quorum\n');
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
    const pendingBefore    = baseline.data.commits_pending_quorum || 0;
    const withQuorumBefore = baseline.data.commits_with_quorum    || 0;
    console.log(`   Baseline: commits_pending_quorum=${pendingBefore}, commits_with_quorum=${withQuorumBefore}`);

    // ── 4. Oracle-A submits first attestation for commit X ─────────────────
    const commitId = helpers.uniqueCommitId();
    const ts1      = Math.floor(Date.now() / 1000);

    const attest1 = helpers.buildSignedAttestation(
        oracleA.privKeyDer,
        oracleA.pubKeyHex,
        commitId,
        true,   // approved
        { timestamp: ts1 },
    );
    const res1 = await helpers.postJson('/transactions/oracle/attestation', attest1, TOKEN);
    if (res1.statusCode !== 200 && res1.statusCode !== 201) {
        console.error(`❌ Oracle-A first attestation rejected (${res1.statusCode}):`, res1.data);
        process.exit(1);
    }
    console.log(`   ✅ Oracle-A attested commit ${commitId.slice(0, 8)}… (1st submission, ts=${ts1})`);

    // ── 5. Oracle-A submits duplicate attestation for the SAME commit ───────
    // Use a distinct timestamp (ts2 > ts1) so the Ed25519 signature differs —
    // this rules out the server's nonce/replay filter as the rejection reason.
    // The duplicate guard in apply_attestation triggers on (oracle_id, commit_id)
    // identity, regardless of timestamp.
    const ts2 = ts1 + 1;  // one second later — distinct signature, same oracle
    const attest2 = helpers.buildSignedAttestation(
        oracleA.privKeyDer,
        oracleA.pubKeyHex,
        commitId,
        true,
        { timestamp: ts2 },
    );
    const res2 = await helpers.postJson('/transactions/oracle/attestation', attest2, TOKEN);
    const replayRejected = res2.statusCode === 400
        && JSON.stringify(res2.data).includes('Launch protocol replay detected');
    if (res2.statusCode !== 200 && res2.statusCode !== 201 && !replayRejected) {
        console.error(`❌ Oracle-A duplicate attestation HTTP request failed (${res2.statusCode}):`, res2.data);
        process.exit(1);
    }
    console.log(
        replayRejected
            ? '   ✅ Duplicate rejected at mempool admission by the launch replay guard'
            : `   ✅ Duplicate enqueued (${res2.statusCode}) for state-level deduplication`,
    );

    // ── 6. Mine a block (processes both mempool transactions) ─────────────
    const mineRes = await helpers.mineBlock(TOKEN);
    if (mineRes.statusCode !== 200 && mineRes.statusCode !== 201) {
        console.error(`❌ POST /blocks/mine failed (${mineRes.statusCode}):`, mineRes.data);
        process.exit(1);
    }
    console.log(`   ✅ Block mined (height=${mineRes.data.height || '?'})`);

    // ── 7. Verify commit is still pending quorum ───────────────────────────
    const statusRes = await helpers.getJson('/transactions/oracle/status', TOKEN);
    if (statusRes.statusCode !== 200) {
        console.error(`❌ GET /transactions/oracle/status failed (${statusRes.statusCode})`);
        process.exit(1);
    }
    const pendingAfter    = statusRes.data.commits_pending_quorum || 0;
    const withQuorumAfter = statusRes.data.commits_with_quorum    || 0;

    // The commit must appear in pending_quorum (exactly one distinct approval).
    if (pendingAfter <= pendingBefore) {
        console.error(
            `❌ FAIL: commits_pending_quorum did not increase after Oracle-A's attestation.\n` +
            `       Before=${pendingBefore}  After=${pendingAfter}\n` +
            `       At least the first attestation must have been recorded.`,
        );
        process.exit(1);
    }
    console.log(`   ✅ Commit is in pending_quorum (pending=${pendingAfter})`);

    // The commit must NOT be in with_quorum — the duplicate must not count.
    if (withQuorumAfter > withQuorumBefore) {
        console.error(
            `❌ FAIL: commits_with_quorum increased even though only ONE distinct oracle voted.\n` +
            `       Before=${withQuorumBefore}  After=${withQuorumAfter}\n` +
            `       Oracle-A voted twice for the same commit; the duplicate must be ignored.\n` +
            `       Quorum of 2 requires 2 DISTINCT oracles.`,
        );
        process.exit(1);
    }
    console.log(`   ✅ commits_with_quorum unchanged (${withQuorumAfter}) — duplicate vote not counted`);

    // ── 8. Verify zero quality-policy slashing events for this commit ──────
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
            `❌ FAIL: Slashing event(s) fired for a quorum-less commit.\n` +
            `       commit_id=${commitId}  slashing_events=${slashingCount}`,
        );
        process.exit(1);
    }
    console.log('   ✅ Zero quality-band slashing events (quorum not met → no false-positive slashing)');

    // ── 9. PASS ─────────────────────────────────────────────────────────────
    console.log('\n✅ TDD-6.2 PASSED: Duplicate oracle votes do not satisfy quorum.\n');
    process.exit(0);
}

run().catch(err => {
    console.error('❌ Unexpected error:', err.message || err);
    process.exit(1);
});
