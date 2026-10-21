const http = require('http');
const { performance } = require('perf_hooks');
const {
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');

const config = loadTestConfig({
    scriptName: 'tests/stress/test-burst-transfers.cjs',
    description: 'TDD-5.1 high-concurrency transfer burst runner.',
    requireToken: true,
    extraOptions: [
        { name: 'count', type: 'int', default: 100, description: 'Number of permissionless transfers to submit' },
    ],
});

// Configuration
const TEST_COUNT = config.values.count;
const CONCURRENCY = 20;
const TARGET_URL = `${config.apiBaseUrl}/transactions/permissionless/submit`;
const TOKEN = config.operatorToken;
const nacl = require('tweetnacl');

console.log(`🚀 Starting High-Concurrency Transfer Burst: ${TEST_COUNT} transactions`);
console.log(`👥 Simulating ${TEST_COUNT} distinct signers`);
console.log(formatRuntimeReport(config, { count: TEST_COUNT, concurrency: CONCURRENCY }));

async function postJson(url, data) {
    const urlObj = new URL(url);
    const body = JSON.stringify(data);
    
    return new Promise((resolve) => {
        const options = {
            hostname: urlObj.hostname,
            port: urlObj.port,
            path: urlObj.pathname,
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'Content-Length': Buffer.byteLength(body),
                'Authorization': `Bearer ${TOKEN}`
            }
        };

        const req = http.request(options, (res) => {
            let resData = '';
            res.on('data', chunk => resData += chunk);
            res.on('end', () => {
                resolve({
                    statusCode: res.statusCode,
                    data: resData
                });
            });
        });

        req.on('error', (err) => {
            resolve({
                statusCode: 0,
                error: err.message
            });
        });

        req.write(body);
        req.end();
    });
}

async function runBurst() {
    const start = performance.now();
    let successCount = 0;
    let failCount = 0;
    let currentIdx = 0;

    const runWorker = async () => {
        while (currentIdx < TEST_COUNT) {
            const id = currentIdx++;
            
            // UNIQUE SIGNER PER TRANSACTION
            const keyPair = nacl.sign.keyPair();
            const publicKeyHex = Buffer.from(keyPair.publicKey).toString('hex');
            
            const nonce = `burst-nonce-${Date.now()}-${id}`;
            const timestamp = Math.floor(Date.now() / 1000);
            const txType = "data_analysis";
            
            // VALID HEX ADDRESSES
            const from = "0x1111111111111111111111111111111111111111";
            const to = "0x" + Buffer.from(nacl.randomBytes(20)).toString('hex');
            
            // PAYLOAD KEYS MUST BE ALPHABETICAL
            const payload = { 
                amount: 1, 
                from: from, 
                to: to
            };
            
            // Build canonical signing message
            const canonicalPayload = JSON.stringify(payload);
            const message = `permissionless-v1|${publicKeyHex}|${nonce}|${timestamp}|${txType}|${canonicalPayload}`;
            const signature = nacl.sign.detached(Buffer.from(message), keyPair.secretKey);
            const signatureHex = Buffer.from(signature).toString('hex');

            const txData = {
                signer_public_key: publicKeyHex,
                signature: signatureHex,
                nonce: nonce,
                timestamp: timestamp,
                tx_type: txType,
                payload: payload
            };

            const res = await postJson(TARGET_URL, txData);
            
            if (res.statusCode === 201) {
                successCount++;
            } else {
                failCount++;
                console.error(`  [${id}] FAILED: ${res.statusCode} ${res.data}`);
            }
        }
    };

    const workers = Array.from({ length: CONCURRENCY }, () => runWorker());
    await Promise.all(workers);

    const end = performance.now();
    const duration = (end - start) / 1000;
    const tps = TEST_COUNT / duration;

    console.log(`\n=== Burst Complete ===`);
    console.log(`Success: ${successCount}`);
    console.log(`Fail: ${failCount}`);
    console.log(`Duration: ${duration.toFixed(2)}s`);
    console.log(`Throughput: ${tps.toFixed(2)} req/s`);

    if (failCount === 0 && successCount === TEST_COUNT) {
        console.log(`\n✅ TDD-5.1 PASSED: Burst handled with zero failures across ${TEST_COUNT} distinct signers.`);
        process.exit(0);
    } else {
        console.log(`\n❌ TDD-5.1 FAILED: Encounted failures during burst.`);
        process.exit(1);
    }
}

runBurst().catch(console.error);
