const http = require('http');
const { performance } = require('perf_hooks');
const fs = require('fs');
const path = require('path');

// Try to load token from .env if not in process.env
let token = process.env.WRFCOIN_OPERATOR_TOKEN;
if (!token) {
    try {
        const envPath = path.join(__dirname, '../../.env');
        const envContent = fs.readFileSync(envPath, 'utf8');
        const match = envContent.match(/WRFCOIN_OPERATOR_TOKEN=(.*)/);
        if (match) token = match[1].trim();
    } catch (e) {}
}

async function runTPSVerification() {
    const url = 'http://localhost:8556/blockchain';
    const totalRequests = 100;
    const concurrency = 10;
    
    console.log(`🚀 Starting SECURE TPS Verification against ${url}...`);
    console.log(`   Requests: ${totalRequests}, Concurrency: ${concurrency}`);
    console.log(`   Auth: ${token ? 'Bearer Token configured' : 'NO TOKEN FOUND'}`);

    const startTime = performance.now();
    let successful = 0;
    let latencies = [];

    const makeRequest = () => {
        return new Promise((resolve) => {
            const startReq = performance.now();
            const options = {
                headers: {}
            };
            if (token) {
                options.headers['Authorization'] = `Bearer ${token}`;
            }

            http.get(url, options, (res) => {
                let data = '';
                res.on('data', chunk => data += chunk);
                res.on('end', () => {
                    if (res.statusCode === 200) {
                        successful++;
                        latencies.push(performance.now() - startReq);
                    } else {
                        console.error(`Status: ${res.statusCode}`);
                    }
                    resolve();
                });
            }).on('error', (err) => {
                console.error(`Error: ${err.message}`);
                resolve();
            });
        });
    };

    for (let i = 0; i < totalRequests; i += concurrency) {
        const batch = [];
        for (let j = 0; j < concurrency && (i + j) < totalRequests; j++) {
            batch.push(makeRequest());
        }
        await Promise.all(batch);
    }

    const endTime = performance.now();
    const durationSecs = (endTime - startTime) / 1000;
    const tps = successful / durationSecs;
    const avgLatency = latencies.length > 0 ? latencies.reduce((a, b) => a + b, 0) / latencies.length : 0;

    console.log('\n📊 Results:');
    console.log(`   Successful: ${successful}/${totalRequests}`);
    console.log(`   Duration: ${durationSecs.toFixed(2)}s`);
    console.log(`   TPS: ${tps.toFixed(2)}`);
    console.log(`   Avg Latency: ${avgLatency.toFixed(2)}ms`);

    if (successful > 90 && tps > 10) {
        console.log('\n✅ TDD-4.1 PASSED');
        process.exit(0);
    } else {
        console.log('\n❌ TDD-4.1 FAILED (Check performance or connectivity)');
        process.exit(1);
    }
}

runTPSVerification().catch(err => {
    console.error(err);
    process.exit(1);
});
