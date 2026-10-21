'use strict';

const assert = require('assert');
const pipeline = require('./lib/ambient-pipeline.cjs');

async function run() {
    let deviceFetchCalled = false;
    const result = await pipeline.fetchStationReadings({
        stationId: 'synthetic-station-no-model',
        ownerAddress: 'synthetic-owner-no-model',
        mac: '00:00:00:00:00:05',
    }, {
        sleepMs: 0,
        fetchDevices: async () => {
            deviceFetchCalled = true;
            return { status: 200, data: [] };
        },
    });

    assert.strictEqual(result.ok, false);
    assert.strictEqual(result.reason, 'invalid_config');
    assert.strictEqual(result.detail, 'Missing model binding for station');
    assert.strictEqual(deviceFetchCalled, false, 'model rejection must precede provider enumeration');
    console.log('ok - fetchStationReadings rejects missing model binding before provider enumeration');
}

run().catch((error) => {
    console.error(`FAIL - ${error.message}`);
    process.exitCode = 1;
});
