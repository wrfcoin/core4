'use strict';

const assert = require('node:assert/strict');
const path = require('path');

const fs = require('fs');

const {
    altimeterSettingHpa,
    buildAccuracyReport,
    derivePressureNormalization,
    loadReplayBundle,
    normalizeReading,
    prepareReplayReadings,
} = require('./ambient-weather-comparison.cjs');

test('corrects station pressure to sea-level pressure when Ambient relative and absolute match', () => {
    const pressure = derivePressureNormalization(
        { baromrelin: 29.312, baromabsin: 29.312 },
        { elevation_m: 220 },
    );

    assert.ok(Math.abs(pressure.submissionPressureHpa - 992.6) < 0.1);
    assert.ok(Math.abs(pressure.comparisonPressureHpa - 1019.0) < 0.1);
    assert.equal(pressure.pressureComparisonMethod, 'elevation_corrected_station_pressure');
});

test('uses residential MAE bands for the Duluth replay and removes the artificial pressure bias', () => {
    const fixturePath = path.join(
        __dirname,
        'fixtures',
        'duluth-ws5000-kdlh-replay.json',
    );
    const { bundle } = loadReplayBundle(fixturePath);
    const normalized = prepareReplayReadings(bundle);
    const { report } = buildAccuracyReport(normalized, bundle.nws_observations, 300);

    assert.ok(report.pressure.mae < 0.6);
    assert.ok(report.pressure.bias < 0);
    assert.ok(report.pressure.bias > -0.6);
    assert.equal(report.pressure.scoring_method, 'residential_mae_bands');
    assert.equal(report.pressure.legacy_band, 'Poor');
    assert.equal(report.pressure.band, 'Excellent');

    assert.equal(report.temperature.scoring_method, 'residential_mae_bands');
    assert.equal(report.temperature.legacy_accuracy_score, 0);
    assert.equal(report.temperature.band, 'Good');

    assert.equal(report.humidity.band, 'Excellent');
    assert.equal(report.wind.band, 'Good');
});

test('normalizes an Example Station raw Ambient payload into canonical fields without leaking raw device identifiers', () => {
    const reading = normalizeReading(
        {
            tempf: 24.1,
            humidity: 60,
            baromrelin: 29.312,
            baromabsin: 29.312,
            windspeedmph: 1.5,
            windgustmph: 3.4,
            winddir: 270,
            hourlyrainin: 0.02,
            dewPoint: 12.2,
            uv: 0,
            solarradiation: 5,
            tempinf: 70.0,
            humidityin: 35,
            dateutc: 1773299100000,
        },
        {
            name: 'Example Station',
            source_id: 'example-station-ws5000',
            mac: '02:00:00:00:00:01',
            device_id_hash: 'c27c02f297b7a634b7b2c5c214e807c983bf64855e84e49620441b9f37d4df3d',
            device_model: 'WS-5000',
            location: 'Duluth, MN',
            lat: 10.123456,
            lon: 20.654321,
            elevation_m: 220,
        },
    );

    assert.equal(reading.temperature_c, -4.4);
    assert.equal(reading.humidity_pct, 60);
    assert.equal(reading.pressure_hpa, 992.6);
    assert.equal(reading.comparison_pressure_hpa, 1019.0);
    assert.equal(reading.wind_speed_ms, 0.7);
    assert.equal(reading.wind_gust_ms, 1.5);
    assert.equal(reading.precipitation_mm, 0.5);
    assert.equal(reading.dew_point_c, -11.0);
    assert.equal(reading.recorded_at, 1773299100000);
    assert.equal(reading.source, 'ambient_weather_network');
    assert.equal(reading.source_id, 'example-station-ws5000');
    assert.equal(reading.source_device, 'WS-5000');
    assert.equal(
        reading.upstream_device_id_hash,
        'c27c02f297b7a634b7b2c5c214e807c983bf64855e84e49620441b9f37d4df3d', // pragma: allowlist secret — SHA-256 of synthetic device 02:00:00:00:00:01, not a credential
    );
    assert.equal(reading.source_verification.source_id, 'example-station-ws5000');
    assert.equal(reading.source_verification.source_verification_status, 'verified');
    assert.equal(reading.source_verification.source_verification_method, 'stable_identifier_hash_match');
    assert.equal(reading.source_verification.collection_node_id, 'ambient-weather-collector');
    assert.ok(!Object.hasOwn(reading, 'source_mac'));
});

test('source verification: MAC-only path returns derived_identifier_hash', () => {
    const reading = normalizeReading(
        { tempf: 50, humidity: 50, dateutc: 1773299100000 },
        { mac: '02:00:00:00:00:01' },
    );
    assert.equal(reading.source_verification.source_verification_status, 'unverified');
    assert.equal(reading.source_verification.source_verification_method, 'derived_identifier_hash');
});

test('source verification: no identifier returns no_stable_identifier', () => {
    const reading = normalizeReading(
        { tempf: 50, humidity: 50, dateutc: 1773299100000 },
        {},
    );
    assert.equal(reading.source_verification.source_verification_status, 'unverified');
    assert.equal(reading.source_verification.source_verification_method, 'no_stable_identifier');
});

test('source verification: hash mismatch returns stable_identifier_hash_mismatch', () => {
    const reading = normalizeReading(
        { tempf: 50, humidity: 50, dateutc: 1773299100000 },
        {
            mac: '02:00:00:00:00:01',
            device_id_hash: 'deadbeef00000000000000000000000000000000000000000000000000000000',
        },
    );
    assert.equal(reading.source_verification.source_verification_status, 'unverified');
    assert.equal(reading.source_verification.source_verification_method, 'stable_identifier_hash_mismatch');
});

test('Duluth replay fixture keeps provenance-friendly identifiers redacted', () => {
    const fixturePath = path.join(
        __dirname,
        'fixtures',
        'duluth-ws5000-kdlh-replay.json',
    );
    const { bundle } = loadReplayBundle(fixturePath);

    assert.equal(bundle.station.source_id, 'example-station-ws5000');
    assert.equal(
        bundle.station.device_id_hash,
        'c27c02f297b7a634b7b2c5c214e807c983bf64855e84e49620441b9f37d4df3d', // pragma: allowlist secret — SHA-256 of synthetic device 02:00:00:00:00:01, not a credential
    );
    assert.ok(!Object.hasOwn(bundle.station, 'mac'));
});

// ---- #3285: pressure_sea_level_hpa ----------------------------------------

const ALTIMETER_VECTORS = path.join(
    __dirname, '..', '..', 'shared-protocol', 'fixtures', 'altimeter_setting_vectors.json',
);
const JON_RAW = { baromrelin: 29.566, baromabsin: 29.566, dateutc: 1790224800000 };

test('#3285 the JS altimeter helper matches the Rust golden vectors', () => {
    const doc = JSON.parse(fs.readFileSync(ALTIMETER_VECTORS, 'utf8'));
    expect(doc.vectors.length).toBeGreaterThanOrEqual(5);
    for (const v of doc.vectors) {
        const got = altimeterSettingHpa(v.station_pressure_hpa, v.elevation_m);
        expect(Math.abs(got - v.altimeter_setting_hpa)).toBeLessThanOrEqual(doc.tolerance_hpa);
    }
    expect(altimeterSettingHpa(Number.NaN, 100)).toBeNull();
    expect(altimeterSettingHpa(0.3, 100)).toBeNull();
});

test('#3285 a registered elevation adds pressure_sea_level_hpa and keeps pressure_hpa raw', () => {
    const reading = normalizeReading(JON_RAW, { elevation_m: 227.7, elevation_source: 'registry' });
    expect(reading.pressure_hpa).toBe(1001.2);
    expect(reading.pressure_sea_level_hpa).toBe(1028.29);
    expect(reading.pressure_kind).toBe('station');

    const override = normalizeReading(JON_RAW, { elevation_m: 227.7, elevation_source: 'operator_override' });
    expect(override.pressure_sea_level_hpa).toBe(1028.29);
});

test('#3285 without a registered elevation the key is absent, not null', () => {
    for (const meta of [
        {},
        { elevation_m: null, elevation_source: null },
        { elevation_m: 220 },
        { elevation_m: 240, elevation_source: 'roster' },
        { elevation_m: null, elevation_source: 'registry' },
    ]) {
        const reading = normalizeReading(JON_RAW, meta);
        expect(Object.prototype.hasOwnProperty.call(reading, 'pressure_sea_level_hpa')).toBe(false);
        expect(Object.prototype.hasOwnProperty.call(reading, 'pressure_kind')).toBe(false);
        expect(reading.pressure_hpa).toBe(1001.2);
    }
});

test('#3285 an owner-offset relative pressure is submitted as the sea-level value', () => {
    // Steven: relative 1020.7 vs absolute 992.4 (the Ambient app carries the offset).
    const raw = { baromrelin: 30.141, baromabsin: 29.306, dateutc: 1790224800000 };
    const pressure = derivePressureNormalization(raw, {});
    expect(pressure.seaLevelPressureMethod).toBe('ambient_relative_pressure');
    const reading = normalizeReading(raw, {});
    expect(reading.pressure_hpa).toBe(reading.relative_pressure_hpa);
    expect(reading.pressure_sea_level_hpa).toBe(reading.relative_pressure_hpa);
    expect(reading.pressure_kind).toBe('relative');
    // A relative value is never reduced again, even with a registered elevation.
    const registered = normalizeReading(raw, { elevation_m: 242.5, elevation_source: 'registry' });
    expect(registered.pressure_sea_level_hpa).toBe(registered.relative_pressure_hpa);
    expect(registered.pressure_kind).toBe('relative');
});

test('#3285 no collector falls back to a 220 m elevation', () => {
    const files = [
        'fetch-ambient-weather.cjs',
        path.join('lib', 'ambient-pipeline.cjs'),
        'ambient-weather-comparison.cjs',
        'fetch-ambient-weather-multi.cjs',
    ];
    const defaultPattern = /(['"`]220(\.0)?['"`])|((\?\?|\|\||:)\s*220(\.0)?\b)/;
    for (const file of files) {
        const source = fs.readFileSync(path.join(__dirname, file), 'utf8');
        expect({ file, match: source.match(defaultPattern)?.[0] ?? null })
            .toEqual({ file, match: null });
    }
});

test('#3285 the commit id covers pressure_sea_level_hpa (twin of commit_reveal_hash_parity.rs)', () => {
    const { computeCommitId } = require('./lib/ambient-pipeline.cjs');
    const withKey = {
        temperature_c: 10.2, humidity_pct: 97, pressure_hpa: 1001.2,
        pressure_sea_level_hpa: 1028.29, pressure_kind: 'station',
        elevation_m: 227.7, wind_speed_ms: 0.4, precipitation_mm: 0,
    };
    const { pressure_sea_level_hpa: _dropped, pressure_kind: _kind, ...withoutKey } = withKey;
    const id = (data) => computeCommitId(data, 'n-3285', 'PWS-EXAMPLE-WS5000', 1790224800, 'contribution');
    expect(id(withKey)).toBe('7ce5f51074a3ee1959bfa5960ae72fae11d5129fd052a992a21280c4da7d10e3'); // pragma: allowlist secret — SHA3-256 commit id test vector
    expect(id(withoutKey)).toBe('617a12bdd305de8777bab29b3458041ffeee598876cbca2f5d6fe9591a01e5c9'); // pragma: allowlist secret — SHA3-256 commit id test vector
});
