const http = require('http');
const { performance } = require('perf_hooks');
const {
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');

const config = loadTestConfig({
    scriptName: 'tests/security/verify-double-spend.cjs',
    description: 'Replay rejection check for permissionless transaction submission.',
    requireToken: true,
});

const token = config.operatorToken;

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
                'Authorization': `Bearer ${token}`
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

async function runDoubleSpendTest() {
    const url = `${config.apiBaseUrl}/transactions/permissionless/submit`;
    const nonce = "test-nonce-" + Date.now();
    
    const txData = {
        signer_public_key: "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        signature: "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
        nonce: nonce,
        timestamp: Math.floor(Date.now() / 1000),
        tx_type: "data_analysis",
        payload: { amount: 100, recipient: "0x123" }
    };

    console.log(`🛡️ Testing Double Spend Protection (Nonce: ${nonce})...`);
    console.log(formatRuntimeReport(config));

    // 1. Submit first time
    console.log("Submitting first transaction...");
    const res1 = await postJson(url, txData);
    console.log(`First submission status: ${res1.statusCode}`);
    // Note: It might fail sig verification but it should populate the "seen" cache

    // 2. Submit second time with SAME nonce
    console.log("Submitting second transaction with same nonce...");
    const res2 = await postJson(url, txData);
    console.log(`Second submission status: ${res2.statusCode}`);
    console.log(`Response: ${res2.data}`);

    if (res2.statusCode === 409 && res2.data.includes("REPLAY_REJECTED")) {
        console.log("\n✅ TDD-5.1 PASSED: Replay attack/Double spend rejected by nonce check.");
        process.exit(0);
    } else {
        console.log("\n❌ TDD-5.1 FAILED: Server did not reject the duplicate nonce correctly.");
        process.exit(1);
    }
}

runDoubleSpendTest().catch(console.error);
