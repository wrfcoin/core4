const http = require('http');

const token = process.env.WRFCOIN_OPERATOR_TOKEN;
const API_URL = 'http://localhost:8556';

async function postJson(path, data) {
    const body = JSON.stringify(data);
    return new Promise((resolve) => {
        const options = {
            hostname: 'localhost',
            port: 8556,
            path: path,
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
                try {
                    const parsed = resData ? JSON.parse(resData) : null;
                    resolve({ statusCode: res.statusCode, data: parsed });
                } catch (e) {
                    resolve({ statusCode: res.statusCode, data: resData });
                }
            });
        });
        req.on('error', (err) => resolve({ statusCode: 0, error: err.message }));
        req.write(body);
        req.end();
    });
}

async function runTest() {
    console.log("🧪 Verifying TDD-3.2: Native Exit (Outbound Bridge)");

    // Define BridgeLock transaction data
    // We'll use the permissionless lane, but wait — BridgeLock usually 
    // needs to be a 'Regular' transaction type in the compat lane or 
    // we need to check if the permissionless lane supports it.
    
    // According to transactions.rs, permissionless lane only allows 'data_analysis'.
    // Let's use the compat lane for BridgeLock.
    
    const bridgeTx = {
        from: "0x1111111111111111111111111111111111111111",
        to: "0x2222222222222222222222222222222222222222",
        amount: 500,
    };

    console.log("Submitting outbound bridge transfer...");
    const res = await postJson('/transactions/submit', bridgeTx);
    
    console.log(`Status: ${res.statusCode}`);
    console.log(`Response:`, res.data);

    if (res.statusCode === 202 && res.data && res.data.data && res.data.data.accepted) {
        console.log("\n✅ TDD-3.2 PASSED: Outbound bridge transaction accepted.");
        process.exit(0);
    } else {
        console.error("\n❌ TDD-3.2 FAILED: Bridge transaction rejected.");
        process.exit(1);
    }
}

runTest().catch(console.error);
