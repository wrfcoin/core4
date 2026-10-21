'use strict';

const crypto = require('crypto');
const nacl = require('tweetnacl');

const helpers = require('./oracle-test-helpers.cjs');

const TOKEN = process.env.WRFCOIN_OPERATOR_TOKEN;
const SEEDED_ORACLE_BALANCE = 100000;

if (!TOKEN) {
    console.error('❌ ERROR: WRFCOIN_OPERATOR_TOKEN is not set.');
    process.exit(1);
}

function assertStatus(label, response, expected = [200, 201]) {
    if (expected.includes(response.statusCode)) {
        return;
    }

    console.error(`❌ ${label} failed (${response.statusCode}):`, response.data);
    process.exit(1);
}

function deriveOracleAddress(pubKeyHex) {
    const hash = crypto
        .createHash('sha3-256')
        .update(Buffer.from(pubKeyHex, 'hex'))
        .digest('hex');
    return `0x${hash.slice(0, 40)}`;
}

function buildSigningMessage(payload, oracleId) {
    const signingPayload = {
        commit_id: payload.commit_id,
        oracle_id: payload.oracle_id,
        approved: payload.approved,
        confidence: payload.confidence || null,
        timestamp: payload.timestamp,
        nonce: payload.nonce || null,
    };
    const digest = crypto
        .createHash('sha3-256')
        .update(JSON.stringify(signingPayload))
        .digest('hex');
    return `wrfcoin-sign-v1|wrfcoin.oracle_attestation.v1|wrfcoin-main|mainnet|${oracleId}|${payload.timestamp}|${payload.nonce || ''}|${digest}`;
}

function buildSignedAttestation(secretKey, oracleId, commitId, approved, timestamp, nonce) {
    const signingPayload = {
        commit_id: commitId,
        oracle_id: oracleId,
        approved,
        timestamp,
        nonce,
    };
    const message = buildSigningMessage(signingPayload, oracleId);
    const signature = Buffer.from(
        nacl.sign.detached(Buffer.from(message, 'utf8'), secretKey),
    ).toString('hex');
    return {
        ...signingPayload,
        commit_hash: commitId,
        signature,
    };
}

async function run() {
    console.log('🧪 TDD-6.2: Validator Slashing via conflicting oracle attestation\n');

    const health = await helpers.getJson('/health', TOKEN);
    assertStatus('GET /health', health, [200]);
    console.log('   ✅ API server healthy');

    const oracleKey = nacl.sign.keyPair();
    const oracleId = Buffer.from(oracleKey.publicKey).toString('hex');
    const oracleAddress = deriveOracleAddress(oracleId);
    const activatedAt = Math.floor(Date.now() / 1000) - 3600;

    console.log(`   Oracle ID: ${oracleId.slice(0, 16)}…`);
    console.log(`   Oracle address: ${oracleAddress}`);

    const configRes = await helpers.putJson(
        '/transactions/oracle/config',
        {
            oracle_set: [oracleId],
            oracle_quorum: 1,
        },
        TOKEN,
    );
    assertStatus('PUT /transactions/oracle/config', configRes);
    console.log('   ✅ Oracle set configured for the live signing-key precondition');

    const registerRes = await helpers.postJson(
        '/transactions/signing-keys/register',
        { identity: oracleId, role: 'oracle', epoch: 1, public_key: oracleId, activated_at: activatedAt },
        TOKEN,
    );
    assertStatus('POST /transactions/signing-keys/register', registerRes);
    console.log('   ✅ Oracle signing key registered');

    const signingKeysRes = await helpers.getJson('/transactions/signing-keys/oracle', TOKEN);
    assertStatus('GET /transactions/signing-keys/oracle', signingKeysRes, [200]);
    const identities = signingKeysRes.data.identities || [];
    if (!identities.some((item) => item.identity === oracleId)) {
        console.error('❌ Registered oracle signing key not returned by GET /transactions/signing-keys/oracle');
        process.exit(1);
    }
    console.log('   ✅ Oracle signing-key snapshot includes the registered identity');

    const seedOracleRes = await helpers.postJson(
        '/debug/seed-account',
        {
            address: oracleAddress,
            amount: SEEDED_ORACLE_BALANCE,
        },
        TOKEN,
    );
    assertStatus('POST /debug/seed-account (oracle)', seedOracleRes);

    const seedMinerRes = await helpers.postJson(
        '/debug/seed-account',
        {
            address: helpers.TEST_MINER_ADDRESS,
            amount: 1000,
        },
        TOKEN,
    );
    assertStatus('POST /debug/seed-account (miner)', seedMinerRes);

    const commitHash = helpers.uniqueCommitId();
    const commitRes = await helpers.postJson(
        '/transactions/contribution/commit',
        {
            contributor: `0x${'2'.repeat(40)}`,
            station_id: 'slashing-test-station',
            commit_hash: commitHash,
        },
        TOKEN,
    );
    assertStatus('POST /transactions/contribution/commit', commitRes);
    console.log(`   ✅ Commit submitted: ${commitHash}`);

    const timestamp = Math.floor(Date.now() / 1000);
    const approveAttestation = buildSignedAttestation(
        oracleKey.secretKey,
        oracleId,
        commitHash,
        true,
        timestamp,
        `approve-${crypto.randomBytes(4).toString('hex')}`,
    );
    const approveRes = await helpers.postJson(
        '/transactions/oracle/attestation',
        approveAttestation,
        TOKEN,
    );
    assertStatus('POST /transactions/oracle/attestation (approve)', approveRes);
    console.log('   ✅ First attestation accepted into the mempool');

    const firstMineRes = await helpers.mineBlock(TOKEN);
    assertStatus('POST /blocks/mine (approve attestation)', firstMineRes);
    console.log('   ✅ First attestation mined into chain state');

    const rejectAttestation = buildSignedAttestation(
        oracleKey.secretKey,
        oracleId,
        commitHash,
        false,
        timestamp,
        `reject-${crypto.randomBytes(4).toString('hex')}`,
    );
    const rejectRes = await helpers.postJson(
        '/transactions/oracle/attestation',
        rejectAttestation,
        TOKEN,
    );
    assertStatus('POST /transactions/oracle/attestation (conflict)', rejectRes);
    console.log('   ✅ Conflicting attestation accepted after the first attestation was mined');

    const mineRes = await helpers.mineBlock(TOKEN);
    if ([200, 201].includes(mineRes.statusCode)) {
        console.error('❌ Expected mining to fail with a slashing trigger, but the block was accepted:', mineRes.data);
        process.exit(1);
    }

    const mineText =
        typeof mineRes.data === 'string' ? mineRes.data : JSON.stringify(mineRes.data);
    if (!/Slashing triggered|DoubleAttestation|OracleAttestationFraud/.test(mineText)) {
        console.error('❌ Mining failed, but not with the expected slashing trigger:', mineRes.data);
        process.exit(1);
    }
    console.log('   ✅ Mining rejected the conflicting attestation with a slashing-trigger error');

    const oracleAccountRes = await helpers.getJson(`/addresses/${oracleAddress}`, TOKEN);
    assertStatus('GET /addresses/{oracle}', oracleAccountRes, [200]);
    const accountBalance =
        oracleAccountRes.data?.data?.balance ?? oracleAccountRes.data?.balance;
    console.log(`   Oracle account balance remains queryable after the rejection: ${accountBalance}`);

    console.log('\n✅ TDD-6.2 PASSED: live signing-key registration and conflicting-attestation slashing path match launch_api.\n');
}

run().catch((error) => {
    console.error('❌ Unexpected error:', error.message || error);
    if (error.stack) {
        console.error(error.stack);
    }
    process.exit(1);
});
