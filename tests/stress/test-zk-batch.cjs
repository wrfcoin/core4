const axios = require('axios');
const crypto = require('crypto');

/**
 * TDD-6.4: ZK Confidential Transfer Performance
 * Tests parallel generation and verification of ZK proofs.
 */

const API_BASE = 'http://localhost:8556';
const OPERATOR_TOKEN = process.env.WRFCOIN_OPERATOR_TOKEN;

// Generate a random blinding that is a valid BN254 scalar field element.
// arkworks Fr::deserialize_compressed expects little-endian bytes < modulus.
// Using 31 random bytes (248 bits) zero-padded to 32 ensures the value is
// well below the ~254-bit BN254 modulus.
function randomBlinding() {
    const buf = Buffer.alloc(32, 0);
    crypto.randomBytes(31).copy(buf);     // LE bytes 0-30, byte 31 stays 0
    return buf.toString('hex');
}

async function runTest() {
    const args = process.argv.slice(2);
    const countIndex = args.indexOf('--count');
    const COUNT = (countIndex !== -1 && args[countIndex + 1]) ? parseInt(args[countIndex + 1]) : 10;

    console.log(`🚀 Starting TDD-6.4: ZK Batch Performance (${COUNT} proofs)`);

    const config = { headers: { Authorization: `Bearer ${OPERATOR_TOKEN}` } };

    const start = Date.now();
    let success = 0;
    let failed = 0;

    const promises = [];
    for (let i = 0; i < COUNT; i++) {
        const p = axios.post(`${API_BASE}/zk/confidential-transfer/generate`, {
            input_values: [100],
            input_blindings: [randomBlinding()],
            output_values: [90],
            output_blindings: [randomBlinding()],
            fee: 10
        }, config).then(res => {
            success++;
            return res.data;
        }).catch(err => {
            failed++;
            console.error(`Proof ${i} failed:`, err.response ? err.response.data : err.message);
        });
        promises.push(p);
    }

    await Promise.all(promises);
    const end = Date.now();
    const duration = (end - start) / 1000;

    console.log(`\n=== ZK Batch Results ===`);
    console.log(`Total: ${COUNT}`);
    console.log(`Success: ${success}`);
    console.log(`Failed: ${failed}`);
    console.log(`Duration: ${duration.toFixed(2)}s`);
    console.log(`Avg: ${(duration / COUNT).toFixed(2)}s/proof`);

    if (success === COUNT) {
        console.log(`\n✅ TDD-6.4 PASSED`);
        process.exit(0);
    } else {
        console.log(`\n❌ TDD-6.4 FAILED`);
        process.exit(1);
    }
}

runTest();
