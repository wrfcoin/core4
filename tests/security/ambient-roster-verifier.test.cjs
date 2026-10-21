'use strict';

const assert = require('assert');
const {
    validateStationRoster,
    selectExactDevice,
    verifyObservationFreshness,
} = require('./lib/ambient-roster-verifier.cjs');

const BASE = {
    provider: 'ambient',
    stationId: 'PWS-TEST-001',
    ownerAddress: '0xTEST',
    mac: '00:00:00:00:00:01',
    model: 'WS-5000',
    consentStatus: 'approved',
    coarseMetadataStatus: 'approved',
    bindingRef: 'binding-a',
};

function expectRosterError(entry, code) {
    const result = validateStationRoster([entry]);
    assert.strictEqual(result.ok, false);
    assert.ok(result.errors.some((error) => error.code === code), `missing ${code}`);
}

function test(name, fn) {
    try {
        fn();
        console.log(`ok - ${name}`);
    } catch (error) {
        console.error(`FAIL - ${name}: ${error.message}`);
        process.exitCode = 1;
    }
}

test('rejects a roster entry missing its private MAC selector', () => {
    const entry = { ...BASE, mac: undefined };
    expectRosterError(entry, 'missing_mac');
});

test('rejects a roster entry missing its explicit model', () => {
    const entry = { ...BASE, model: undefined };
    expectRosterError(entry, 'missing_model');
});

test('rejects duplicate private MAC selectors', () => {
    const result = validateStationRoster([
        BASE,
        { ...BASE, stationId: 'PWS-TEST-002' },
    ]);
    assert.strictEqual(result.ok, false);
    assert.ok(result.errors.some((error) => error.code === 'duplicate_mac'));
});

test('rejects a device response with the wrong MAC', () => {
    const result = selectExactDevice(BASE, [{ macAddress: '00:00:00:00:00:02', model: 'WS-5000' }]);
    assert.strictEqual(result.ok, false);
    assert.strictEqual(result.code, 'device_mismatch');
});

test('rejects a stale observation without contacting a provider', () => {
    const result = verifyObservationFreshness({ recorded_at: '2020-01-01T00:00:00.000Z' }, {
        nowMs: Date.parse('2020-01-01T01:00:00.000Z'),
        maxAgeMinutes: 30,
    });
    assert.strictEqual(result.ok, false);
    assert.strictEqual(result.code, 'stale_observation');
});

test('accepts a valid exact MAC/model match and fresh observation', () => {
    const roster = validateStationRoster([BASE]);
    assert.strictEqual(roster.ok, true);
    const selected = selectExactDevice(roster.entries[0], [{
        macAddress: '00:00:00:00:00:01',
        model: 'WS-5000',
    }]);
    assert.strictEqual(selected.ok, true);
    const fresh = verifyObservationFreshness({ recorded_at: '2020-01-01T00:20:00.000Z' }, {
        nowMs: Date.parse('2020-01-01T00:30:00.000Z'),
        maxAgeMinutes: 30,
    });
    assert.strictEqual(fresh.ok, true);
});
