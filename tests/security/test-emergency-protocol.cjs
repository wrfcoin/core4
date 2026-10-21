'use strict';

const http = require('http');
const {
    formatRuntimeReport,
    loadTestConfig,
} = require('../utils/test-config.cjs');

const config = loadTestConfig({
    scriptName: 'tests/security/test-emergency-protocol.cjs',
    description: 'TDD-5.3 emergency halt and resume flow.',
    requireToken: true,
});

const API_HOST = config.apiHost;
const API_PORT = config.apiPort;
const TOKEN = config.operatorToken;

function request(method, path, body) {
    return new Promise((resolve, reject) => {
        const options = {
            hostname: API_HOST,
            port: API_PORT,
            path,
            method,
            headers: {
                Authorization: `Bearer ${TOKEN}`,
                'Content-Type': 'application/json',
            },
        };
        const req = http.request(options, (res) => {
            let raw = '';
            res.on('data', (chunk) => { raw += chunk; });
            res.on('end', () => {
                try {
                    resolve({ status: res.statusCode, data: JSON.parse(raw) });
                } catch (_) {
                    resolve({ status: res.statusCode, data: raw });
                }
            });
        });
        req.on('error', reject);
        if (body) req.write(JSON.stringify(body));
        req.end();
    });
}

async function runEmergencyTest() {
    console.log(`🚀 TDD-5.3: Emergency Protocol Pause & Governance (Bridge Context)`);
    console.log(formatRuntimeReport(config));

    // 1. Check initial status
    const status1 = await request('GET', '/bridge/safety/status');
    console.log(`🔹 Initial Halted Status: ${status1.data.halted}`);

    // 2. Halt
    console.log('🔴 Triggering Emergency Halt...');
    const haltRes = await request('POST', '/bridge/safety/halt', { reason: 'TDD Phase 5 Stress Test' });
    if (haltRes.status !== 200) {
        console.error('❌ Halt failed:', haltRes.status, haltRes.data);
        process.exit(1);
    }
    console.log(`✅ Halted: ${haltRes.data.halted}, Reason: ${haltRes.data.halt_reason}`);

    // 3. Verify Halt via Audit Log
    const auditRes = await request('GET', '/bridge/safety/audit-log');
    const latestEvent = auditRes.data[auditRes.data.length - 1];
    const eventType = latestEvent.event || latestEvent.event_type || latestEvent.type;
    console.log(`📝 Latest Audit Event: ${eventType} (${latestEvent.reason})`);

    if (haltRes.data.halted !== true || (eventType !== 'ManualHalt' && eventType !== 'AutoHalt' && eventType !== 'halt')) {
        console.error('❌ TDD-5.3 FAILED: Bridge did not transition to halted state correctly.');
        process.exit(1);
    }

    // 4. Resume
    console.log('🟢 Triggering Governance Resume...');
    const resumeRes = await request('POST', '/bridge/safety/resume', { 
        reason: 'Recovery verified by TDD',
        operator: '0x0000000000000000000000000000000000000001'
    });
    if (resumeRes.status !== 200) {
        console.error('❌ Resume failed:', resumeRes.status, resumeRes.data);
        process.exit(1);
    }
    console.log(`✅ Halted: ${resumeRes.data.halted}`);

    if (resumeRes.data.halted === false) {
        console.log(`\n✅ TDD-5.3 PASSED: Emergency Halt/Resume lifecycle verified.`);
        process.exit(0);
    } else {
        console.log(`\n❌ TDD-5.3 FAILED: Bridge failed to resume.`);
        process.exit(1);
    }
}

runEmergencyTest().catch(err => {
    console.error('Test error:', err);
    process.exit(1);
});
