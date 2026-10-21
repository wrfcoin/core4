'use strict';

/**
 * #2736 — the model binding under Option 2 (operator-asserted provenance).
 *
 * The provider-model binding was unsatisfiable: Ambient's /v1/devices returns
 * no model under any candidate path for real hardware (measured on
 * PWS-EXAMPLE-AMBIENT-01, 2026-08-05), so `selectExactDevice` refused every
 * contributor at `model_unverified` and #1732 multi-tenant onboarding never
 * worked for anyone.
 *
 * The Option 2 contract these tests bind:
 *  - provider supplies a model and it MATCHES  -> ok, `provider_verified`
 *  - provider supplies a model and it DIFFERS  -> still REFUSES with
 *    `model_mismatch` (+ detail saying expected vs returned). The check keeps
 *    its teeth wherever there is something to check.
 *  - provider supplies NO model               -> proceeds on the roster's
 *    operator-asserted model, marked `operator_asserted` — visibly weaker,
 *    never silently equivalent.
 *  - the provenance is CARRIED, not dropped: it reaches the per-reading
 *    `source_verification` attribution envelope and the pipeline result the
 *    collector logs.
 *
 * These tests bind the REAL verifier, the REAL pipeline seam
 * (fetchStationReadings with injected fetchers), and the REAL normalization
 * envelope — no mirrors.
 */

const assert = require('assert');
const { selectExactDevice } = require('./lib/ambient-roster-verifier.cjs');
const { fetchStationReadings } = require('./lib/ambient-pipeline.cjs');
const { normalizeReading } = require('./ambient-weather-comparison.cjs');

function test(name, fn) {
    Promise.resolve()
        .then(fn)
        .then(() => console.log(`ok - ${name}`))
        .catch((error) => {
            console.error(`FAIL - ${name}: ${error.message}`);
            process.exitCode = 1;
        });
}

const ENTRY = { mac: 'D4:8C:49:30:33:60', model: 'WS-2902' };

// The measured shape from #2736: MAC matches, but the device carries no model
// under any candidate path — info has only coords/name, lastData has no
// stationtype.
const MODELLESS_DEVICE = {
    macAddress: 'D4:8C:49:30:33:60',
    info: { coords: { lat: 41.5, lon: -77.4 }, name: 'Tioga Backyard' },
    lastData: { tempf: 80.1, humidity: 55, dateutc: Date.now() },
};

// ---------------------------------------------------------------------------
// Verifier contract
// ---------------------------------------------------------------------------

test('provider model present and matching -> ok, provider_verified (#2736)', () => {
    const result = selectExactDevice(ENTRY, [{ ...MODELLESS_DEVICE, model: 'WS-2902' }]);

    assert.strictEqual(result.ok, true);
    assert.strictEqual(result.modelBinding, 'provider_verified');
    assert.strictEqual(result.model, 'WS-2902');
    assert.strictEqual(result.providerModel, 'WS-2902');
});

test('provider model present and CONTRADICTING -> still refuses model_mismatch (#2736 control)', () => {
    // THE test that proves Option 2 did not simply weaken the binding: where
    // the provider gives us something to check, a contradiction refuses.
    const result = selectExactDevice(ENTRY, [{ ...MODELLESS_DEVICE, model: 'WS-5000' }]);

    assert.strictEqual(result.ok, false);
    assert.strictEqual(result.code, 'model_mismatch');
    assert.strictEqual(result.detail.expectedModel, 'WS-2902');
    assert.strictEqual(result.detail.returnedModel, 'WS-5000');
});

test('provider model ABSENT -> proceeds on the roster assertion, marked operator_asserted (#2736)', () => {
    const result = selectExactDevice(ENTRY, [MODELLESS_DEVICE]);

    assert.strictEqual(
        result.ok,
        true,
        `a modelless provider response must no longer refuse onboarding, got: ${JSON.stringify(result)}`
    );
    assert.strictEqual(result.modelBinding, 'operator_asserted');
    assert.strictEqual(result.model, 'WS-2902', 'the operator-asserted roster model carries');
    assert.strictEqual(
        result.providerModel,
        null,
        'the absence of provider corroboration must be explicit'
    );
});

test('a stationtype the provider DID return is surfaced for diagnosis, not compared (#2736)', () => {
    const result = selectExactDevice(ENTRY, [{
        ...MODELLESS_DEVICE,
        lastData: { ...MODELLESS_DEVICE.lastData, stationtype: 'AMBWeatherPro_V5.1.9' },
    }]);

    // A firmware string is not a model: it must neither refuse (that would
    // convert the lockout into a differently-worded lockout) nor upgrade the
    // binding to provider_verified.
    assert.strictEqual(result.ok, true);
    assert.strictEqual(result.modelBinding, 'operator_asserted');
    assert.strictEqual(result.stationtype, 'AMBWeatherPro_V5.1.9');
});

// ---------------------------------------------------------------------------
// Provenance carriage — metadata and pipeline result, not just return values
// ---------------------------------------------------------------------------

const STATION = {
    stationId: 'PWS-EXAMPLE-AMBIENT-01',
    ownerAddress: '0xSTEVEN',
    mac: 'D4:8C:49:30:33:60',
    model: 'WS-2902',
    apiKey: 'test-api-key', // pragma: allowlist secret -- placeholder in a unit-test fixture
    appKey: 'test-app-key', // pragma: allowlist secret -- placeholder in a unit-test fixture
};

const ONE_READING = [{ dateutc: Date.now(), tempf: 80.1, humidity: 55, baromrelin: 29.92 }];

function pipelineOpts(device) {
    return {
        sleepMs: 0,
        fetchDevices: async () => ({ status: 200, data: [device] }),
        fetchDeviceHistory: async () => ({ status: 200, data: ONE_READING }),
    };
}

test('operator-asserted provenance reaches the source_verification envelope AND the pipeline result (#2736)', async () => {
    const result = await fetchStationReadings(STATION, pipelineOpts(MODELLESS_DEVICE));

    assert.strictEqual(result.ok, true, `pipeline must proceed, got: ${JSON.stringify(result)}`);
    assert.strictEqual(
        result.modelBinding,
        'operator_asserted',
        'the collector logs the pipeline result — the binding must be on it'
    );
    assert.ok(result.normalized.length > 0, 'test premise: at least one normalized reading');
    assert.strictEqual(
        result.normalized[0].source_verification.device_model_binding,
        'operator_asserted',
        'every submitted reading must carry the binding provenance in its attribution envelope'
    );
});

test('provider-verified provenance reaches the envelope the same way (#2736)', async () => {
    const result = await fetchStationReadings(
        STATION,
        pipelineOpts({ ...MODELLESS_DEVICE, model: 'WS-2902' })
    );

    assert.strictEqual(result.ok, true);
    assert.strictEqual(result.modelBinding, 'provider_verified');
    assert.strictEqual(
        result.normalized[0].source_verification.device_model_binding,
        'provider_verified'
    );
});

test('a contradicting provider model still refuses through the pipeline, with detail (#2736 control)', async () => {
    const result = await fetchStationReadings(
        STATION,
        pipelineOpts({ ...MODELLESS_DEVICE, model: 'WS-5000' })
    );

    assert.strictEqual(result.ok, false);
    assert.strictEqual(result.reason, 'model_mismatch');
    assert.strictEqual(result.detail.returnedModel, 'WS-5000');
});

test('legacy callers that never set a binding render as unspecified, never as verified (#2736)', () => {
    // The single-station script and any older caller do not set model_binding
    // on stationMeta. They must not silently claim provider verification.
    const reading = normalizeReading(ONE_READING[0], { mac: 'D4:8C:49:30:33:60' });

    assert.strictEqual(reading.source_verification.device_model_binding, 'unspecified');
});
