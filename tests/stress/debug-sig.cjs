const http = require('http');
const { performance } = require('perf_hooks');
const {
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');

const config = loadTestConfig({
    scriptName: 'tests/stress/debug-sig.cjs',
    description: 'Debug permissionless signature submission against launch_api.',
    requireToken: true,
});

// Configuration
const TEST_COUNT = 1;
const TARGET_URL = `${config.apiBaseUrl}/transactions/permissionless/submit`;
const TOKEN = config.operatorToken;
const nacl = require('tweetnacl');

// Generate a test keypair for signing
const seed = new Uint8Array(32); // Fixed seed for debugging
seed.fill(7); 
const keyPair = nacl.sign.keyPair.fromSeed(seed);
const publicKeyHex = Buffer.from(keyPair.publicKey).toString('hex');

console.log(`🚀 Debugging Permissionless Signature`);
console.log(`🔑 Public Key: ${publicKeyHex}`);
console.log(formatRuntimeReport(config, { count: TEST_COUNT }));

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

async function debugSignature() {
    const nonce = "debug-nonce-123";
    const timestamp = 1710216000; // Fixed timestamp
    const txType = "data_analysis";
    const payload = { 
        from: "0x1111111111111111111111111111111111111111", 
        to: "0x2222222222222222222222222222222222222222", 
        amount: 5 
    };
    
    // Build canonical signing message: permissionless-v1|signer_public_key|nonce|timestamp|tx_type|canonical_payload
    const canonicalPayload = JSON.stringify(payload);
    const message = `permissionless-v1|${publicKeyHex}|${nonce}|${timestamp}|${txType}|${canonicalPayload}`;
    console.log(`📝 Signing Message: ${message}`);
    
    const signature = nacl.sign.detached(Buffer.from(message), keyPair.secretKey);
    const signatureHex = Buffer.from(signature).toString('hex');
    console.log(`✍️  Signature: ${signatureHex}`);

    const txData = {
        signer_public_key: publicKeyHex,
        signature: signatureHex,
        nonce: nonce,
        timestamp: timestamp,
        tx_type: txType,
        payload: payload
    };

    const res = await postJson(TARGET_URL, txData);
    console.log(`📥 Status: ${res.statusCode}`);
    console.log(`📥 Body: ${res.data}`);
}

debugSignature().catch(console.error);
