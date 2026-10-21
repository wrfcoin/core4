'use strict';

const crypto = require('crypto');
const {
    exportToEnvironment,
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');

const config = loadTestConfig({
    scriptName: 'tests/security/test-batch-rewards.cjs',
    description: 'TDD-5.2 batch reward distribution accuracy runner.',
    requireToken: true,
    extraOptions: [
        { name: 'stations', type: 'int', default: 2, description: 'Number of stations to simulate' },
        { name: 'readings', type: 'int', default: 5, description: 'Number of readings per station' },
    ],
});
exportToEnvironment(config);
const helpers = require('./oracle-test-helpers.cjs');

const TOKEN = config.operatorToken;

// ---------------------------------------------------------------------------
// Helpers
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
// Main
// ---------------------------------------------------------------------------

async function run() {
    console.log(`🚀 TDD-5.2: Batch Reward Distribution Accuracy`);

    const STATION_COUNT = config.values.stations;
    const READINGS_PER_STATION = config.values.readings;
    const TOTAL_READINGS = STATION_COUNT * READINGS_PER_STATION;
    console.log(formatRuntimeReport(config, {
        stations: STATION_COUNT,
        readings_per_station: READINGS_PER_STATION,
        total_readings: TOTAL_READINGS,
    }));
    console.log(`📊 Target: ${TOTAL_READINGS} readings from ${STATION_COUNT} stations`);

    let originalConfig = null;
    try {
        // 1. Save original oracle config
        console.log('📥 Backing up current oracle config...');
        try {
            const configResp = await helpers.getJson('/transactions/oracle/config', TOKEN);
            originalConfig = configResp.data || configResp;
            console.log('✅ Config backup successful');
        } catch (e) {
            console.warn('⚠️  Could not backup oracle config, proceeding anyway...');
        }

        // 2. Setup oracles
        const oracleA = helpers.generateOracleKeyPair();
        const oracleB = helpers.generateOracleKeyPair();
        
        console.log('⚙️  Configuring oracles...');
        await helpers.putJson('/transactions/oracle/config', {
            oracle_set: [oracleA.pubKeyHex, oracleB.pubKeyHex],
            oracle_quorum: 2
        }, TOKEN);

        console.log('🔑 Registering signing keys for oracles...');
        for (const oracle of [oracleA, oracleB]) {
            await helpers.postJson('/transactions/signing-keys/register', {
                identity: oracle.pubKeyHex,
                role: 'oracle',
                epoch: 1,
                public_key: oracle.pubKeyHex,
                activated_at: Math.floor(Date.now() / 1000) - 3600
            }, TOKEN);
        }
        console.log('✅ Oracles ready');

        // 3. Get baseline summary
        const initialSummary = await helpers.getJson('/transactions/rewards/trust/summary', TOKEN);
        const initialCount = initialSummary.data.minted_count || 0;
        console.log(`🔹 Initial Minted Count: ${initialCount}`);

        // 4. Prepare and Submit Lifecycle per Reading
        let processed = 0;
        for (let r = 0; r < READINGS_PER_STATION; r++) {
            console.log(`\n⏳ Processing Epoch ${r+1}/${READINGS_PER_STATION}...`);
            for (let s = 0; s < STATION_COUNT; s++) {
                const station_id = `BATCH-STATION-${s}-${Date.now().toString().slice(-4)}`;
                const contributor = `0x${(s + 100 + r).toString(16).padStart(40, '0')}`;
                
                const payload = { 
                    humidity_pct: 45, 
                    pressure_hpa: 1012, 
                    quality: 0.95,
                    temperature_c: 22.5,
                    uniqueness: 1.0,
                    volume: 1.0
                };
                const nonce = crypto.randomBytes(16).toString('hex');
                const timestamp = Math.floor(Date.now() / 1000);
                const commitHash = computeCommitId(payload, nonce, station_id, timestamp, 'contribution');

                // A. Commit
                await helpers.postJson('/transactions/contribution/commit', {
                    contributor,
                    station_id,
                    commit_hash: commitHash,
                }, TOKEN);
                await helpers.mineBlock(TOKEN);

                // B. Attest
                const attestA = helpers.buildSignedAttestation(oracleA.privKeyDer, oracleA.pubKeyHex, commitHash, true, { timestamp });
                const attestB = helpers.buildSignedAttestation(oracleB.privKeyDer, oracleB.pubKeyHex, commitHash, true, { timestamp });
                await helpers.postJson('/transactions/oracle/attestation', attestA, TOKEN);
                await helpers.postJson('/transactions/oracle/attestation', attestB, TOKEN);

                // C. Reveal
                await helpers.postJson('/transactions/contribution/reveal', {
                    contributor,
                    station_id,
                    payload,
                    nonce,
                    commit_hash: commitHash,
                    timestamp,
                }, TOKEN);
                
                // Finalize this reading
                await helpers.mineBlock(TOKEN);
                processed++;
                process.stdout.write(`✅ Reading ${processed}/${TOTAL_READINGS} finalized.\n`);
            }
        }

        // 5. Verify results
        console.log('\n⏳ Waiting for state to settle...');
        await new Promise(r => setTimeout(r, 1000));

        const finalSummary = await helpers.getJson('/transactions/rewards/trust/summary', TOKEN);
        const finalCount = finalSummary.data.minted_count || 0;
        const deltaCount = finalCount - initialCount;

        console.log(`\n=== Final Results ===`);
        console.log(`Delta Minted Count: ${deltaCount}`);

        if (deltaCount >= TOTAL_READINGS) {
            console.log(`\n✅ TDD-5.2 PASSED: ${deltaCount} readings converted to rewards.`);
        } else {
            console.log(`\n❌ TDD-5.2 FAILED: Expected at least ${TOTAL_READINGS} readings, got ${deltaCount}.`);
            process.exitCode = 1;
        }
    } finally {
        // 6. Restore oracle config
        if (originalConfig) {
            console.log('\n📤 Restoring oracle config...');
            try {
                await helpers.putJson('/transactions/oracle/config', {
                    oracle_set: originalConfig.oracle_set,
                    oracle_quorum: originalConfig.oracle_quorum
                }, TOKEN);
                console.log('✅ Config restored');
            } catch (e) {
                console.error('❌ Failed to restore oracle config:', e.message);
            }
        }
    }
}

run().catch(err => {
    console.error('Test error:', err);
    process.exit(1);
});
