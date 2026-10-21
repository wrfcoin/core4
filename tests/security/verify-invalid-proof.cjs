const http = require('http');
const {
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');

const config = loadTestConfig({
    scriptName: 'tests/security/verify-invalid-proof.cjs',
    description: 'Reject tampered weather-model proof public inputs.',
    requireToken: true,
});

const token = config.operatorToken;
const API_URL = config.apiBaseUrl;

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
                    console.error(`Failed to parse JSON response: ${resData}`);
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
    console.log("🧪 Verifying TDD-5.2: Invalid Proof Rejection");
    console.log(formatRuntimeReport(config));

    // 1. Generate a valid proof
    const genData = {
        input_hash: "0000000000000000000000000000000000000000000000000000000000000001",
        output_hash: "0000000000000000000000000000000000000000000000000000000000000002",
        model_id: 1,
        time_secs: 3600,
        samples: [100, 200]
    };
    
    console.log("Generating valid weather proof...");
    const genRes = await postJson('/zk/weather-model/generate', genData);
    if (genRes.statusCode !== 200) {
        console.error("Failed to generate proof:", genRes.data);
        process.exit(1);
    }
    const proof = genRes.data.proof;
    console.log("✅ Proof generated.");

    // 2. Verify valid proof
    console.log("Verifying valid proof...");
    const verifyData = {
        proof: proof,
        input_hash: genData.input_hash,
        output_hash: genData.output_hash,
        model_id: genData.model_id,
        time_secs: genData.time_secs
    };
    const verRes = await postJson('/zk/weather-model/verify', verifyData);
    if (verRes.statusCode === 200 && verRes.data.valid === true) {
        console.log("✅ Valid proof correctly verified.");
    } else {
        console.error("❌ FAILED: Valid proof rejected!", verRes.data);
        process.exit(1);
    }

    // 3. Attempt to verify with tampered public inputs (modified output_hash)
    console.log("Attempting to verify with tampered output_hash...");
    const tamperedData = { ...verifyData, output_hash: "0000000000000000000000000000000000000000000000000000000000000003" };
    const tamRes = await postJson('/zk/weather-model/verify', tamperedData);
    
    if (tamRes.statusCode === 200 && tamRes.data.valid === false) {
        console.log("✅ TDD-5.2 PASSED: Tampered ZK-proof correctly rejected by API.");
        process.exit(0);
    } else {
        console.error("❌ TDD-5.2 FAILED: Tampered proof was NOT rejected!", tamRes.data);
        process.exit(1);
    }
}

runTest().catch(console.error);
