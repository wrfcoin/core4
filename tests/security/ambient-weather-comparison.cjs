'use strict';

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');

const INHG_TO_HPA = 33.8639;
const MPH_TO_MS = 0.44704;
const IN_TO_MM = 25.4;
const PRESSURE_RELATIVE_DELTA_HPA = 0.5;
const PRESSURE_ELEVATION_COEFFICIENT_HPA_PER_M = 0.12;
const COLLECTOR_NODE_ID = process.env.AMBIENT_COLLECTION_NODE_ID || 'ambient-weather-collector';

const VARIABLE_POLICIES = {
    temperature: {
        lowVarianceStddev: 2.0,
        thresholds: { excellent: 1.0, good: 2.0, marginal: 3.5, poor: 5.0 },
    },
    pressure: {
        lowVarianceStddev: 2.0,
        thresholds: { excellent: 1.5, good: 3.0, marginal: 5.0, poor: 8.0 },
    },
    humidity: {
        lowVarianceStddev: 5.0,
        thresholds: { excellent: 4.0, good: 8.0, marginal: 12.0, poor: 20.0 },
    },
    wind: {
        lowVarianceStddev: 2.0,
        thresholds: { excellent: 1.5, good: 3.0, marginal: 5.0, poor: 8.0 },
    },
};

function roundTo(value, digits) {
    if (value == null || !Number.isFinite(value)) return null;
    return Number.parseFloat(value.toFixed(digits));
}

function toHpa(inchesHg) {
    if (inchesHg == null) return null;
    return roundTo(inchesHg * INHG_TO_HPA, 1);
}

function approximateSeaLevelPressure(pressureHpa, elevationM) {
    if (pressureHpa == null) return null;
    if (elevationM == null || !Number.isFinite(elevationM)) return pressureHpa;
    return roundTo(
        pressureHpa + (elevationM * PRESSURE_ELEVATION_COEFFICIENT_HPA_PER_M),
        1,
    );
}

// #3285: NWS altimeter-setting reduction, a line-for-line port of
// shared-protocol weather::station_reference::altimeter_setting_hpa. The
// attester recomputes the submitted pressure_sea_level_hpa with the Rust
// function from the on-chain pressure_hpa and the station's REGISTERED
// elevation, and trusts it only within 1 hPa, so both sides must reduce
// identically. Pinned to each other by
// shared-protocol/fixtures/altimeter_setting_vectors.json.
const ALTIMETER_EXPONENT = 0.190284;

function altimeterSettingHpa(stationHpa, elevationM) {
    if (!Number.isFinite(stationHpa) || !Number.isFinite(elevationM) || stationHpa <= 0.3) {
        return null;
    }
    const p = stationHpa - 0.3;
    const k = (Math.pow(1013.25, ALTIMETER_EXPONENT) * 0.0065) / 288.0;
    const base = 1.0 + k * (elevationM / Math.pow(p, ALTIMETER_EXPONENT));
    if (base <= 0) return null;
    const altimeter = p * Math.pow(base, 1.0 / ALTIMETER_EXPONENT);
    return Number.isFinite(altimeter) ? altimeter : null;
}

// Where a station's elevation may come from for the sea-level pressure key.
// Owner ruling (#3285): only the on-chain registration, or an explicit
// operator override. A default, a guess, or an unregistered roster value
// produces no key at all.
const SEA_LEVEL_ELEVATION_SOURCES = new Set(['registry', 'operator_override']);

function seaLevelElevationM(stationMeta = {}) {
    if (!SEA_LEVEL_ELEVATION_SOURCES.has(stationMeta.elevation_source)) return null;
    return Number.isFinite(stationMeta.elevation_m) ? stationMeta.elevation_m : null;
}

function stableSourceId(stationMeta = {}) {
    if (typeof stationMeta.source_id === 'string' && stationMeta.source_id.trim()) {
        return stationMeta.source_id.trim();
    }

    if (typeof stationMeta.name === 'string' && stationMeta.name.trim()) {
        return stationMeta.name
            .trim()
            .toLowerCase()
            .replace(/[^a-z0-9]+/g, '-')
            .replace(/^-+|-+$/g, '');
    }

    return 'ambient-weather-station';
}

function buildSourceVerificationEnvelope(stationMeta = {}) {
    const sourceId = stableSourceId(stationMeta);
    const rawId = stationMeta.mac ?? stationMeta.device_id ?? null;
    const macDerived = typeof rawId === 'string' && rawId.trim()
        ? crypto.createHash('sha256').update(rawId.trim().toLowerCase()).digest('hex')
        : null;
    const providedHash = typeof stationMeta.device_id_hash === 'string'
        ? stationMeta.device_id_hash.trim().toLowerCase()
        : null;
    const isVerified = Boolean(macDerived && providedHash && macDerived === providedHash);

    // #2736: provenance of the device-model binding. `provider_verified`
    // means the provider's device listing corroborated the roster's model;
    // `operator_asserted` means the provider returned no model and the value
    // rests on the roster entry alone — a visibly weaker binding downstream
    // policy may treat differently. Callers that predate the distinction
    // (the single-station script) render as `unspecified`, never as verified.
    const modelBinding = stationMeta.model_binding === 'provider_verified'
        || stationMeta.model_binding === 'operator_asserted'
        ? stationMeta.model_binding
        : 'unspecified';

    return {
        source_id: sourceId,
        source_verification_status: isVerified ? 'verified' : 'unverified',
        source_verification_method: macDerived
            ? (providedHash
                ? (isVerified ? 'stable_identifier_hash_match' : 'stable_identifier_hash_mismatch')
                : 'derived_identifier_hash')
            : 'no_stable_identifier',
        device_model_binding: modelBinding,
        collection_node_id: COLLECTOR_NODE_ID,
    };
}

function upstreamDeviceIdHash(stationMeta = {}) {
    if (typeof stationMeta.device_id_hash === 'string' && stationMeta.device_id_hash.trim()) {
        return stationMeta.device_id_hash.trim().toLowerCase();
    }

    const rawIdentifier = stationMeta.mac ?? stationMeta.device_id ?? null;
    if (typeof rawIdentifier !== 'string' || !rawIdentifier.trim()) {
        return null;
    }

    return crypto
        .createHash('sha256')
        .update(rawIdentifier.trim().toLowerCase())
        .digest('hex');
}

function derivePressureNormalization(raw, stationMeta = {}) {
    const relativePressureHpa = toHpa(raw.baromrelin);
    const absolutePressureHpa = toHpa(raw.baromabsin);
    const submissionPressureHpa = relativePressureHpa ?? absolutePressureHpa;
    const hasDistinctRelativePressure =
        relativePressureHpa != null &&
        absolutePressureHpa != null &&
        Math.abs(relativePressureHpa - absolutePressureHpa) >= PRESSURE_RELATIVE_DELTA_HPA;

    if (submissionPressureHpa == null) {
        return {
            submissionPressureHpa: null,
            comparisonPressureHpa: null,
            pressureComparisonMethod: 'missing_pressure',
            seaLevelPressureHpa: null,
            seaLevelPressureMethod: 'missing_pressure',
            pressureKind: null,
            absolutePressureHpa,
            relativePressureHpa,
        };
    }

    if (hasDistinctRelativePressure) {
        // The owner set a sea-level offset in the Ambient app, so the relative
        // value already is sea-level pressure (Steven). Submitted as-is.
        return {
            submissionPressureHpa,
            comparisonPressureHpa: relativePressureHpa,
            pressureComparisonMethod: 'ambient_relative_pressure',
            seaLevelPressureHpa: relativePressureHpa,
            seaLevelPressureMethod: 'ambient_relative_pressure',
            // pressure_hpa is this relative value: already sea-level, and the
            // attester must never reduce it again (#3285).
            pressureKind: 'relative',
            absolutePressureHpa,
            relativePressureHpa,
        };
    }

    // Relative = absolute: pressure_hpa is station pressure. Reduce the exact
    // value that goes on chain, as the attester will, with the registered (or
    // operator-override) elevation; otherwise submit no sea-level key.
    const seaLevelElevation = seaLevelElevationM(stationMeta);
    const reduced = seaLevelElevation == null
        ? null
        : roundTo(altimeterSettingHpa(submissionPressureHpa, seaLevelElevation), 2);
    const seaLevel = {
        seaLevelPressureHpa: reduced,
        seaLevelPressureMethod: reduced == null
            ? 'no_registered_elevation'
            : 'altimeter_setting_from_registered_elevation',
        // Relative = absolute (no Ambient offset): pressure_hpa is station
        // pressure, the only kind the attester recomputes from elevation.
        pressureKind: 'station',
    };

    if (stationMeta.elevation_m != null && Number.isFinite(stationMeta.elevation_m)) {
        return {
            submissionPressureHpa,
            comparisonPressureHpa: approximateSeaLevelPressure(
                submissionPressureHpa,
                stationMeta.elevation_m,
            ),
            pressureComparisonMethod: 'elevation_corrected_station_pressure',
            ...seaLevel,
            absolutePressureHpa,
            relativePressureHpa,
        };
    }

    return {
        submissionPressureHpa,
        comparisonPressureHpa: submissionPressureHpa,
        pressureComparisonMethod: 'uncorrected_station_pressure',
        ...seaLevel,
        absolutePressureHpa,
        relativePressureHpa,
    };
}

function normalizeReading(raw, stationMeta) {
    const tempF = raw.tempf ?? raw.temp;
    const tempC = tempF != null ? roundTo((tempF - 32) * 5 / 9, 1) : null;
    const pressure = derivePressureNormalization(raw, stationMeta);
    const windMph = raw.windspeedmph ?? raw.windSpeed;
    const windMs = windMph != null ? roundTo(windMph * MPH_TO_MS, 1) : null;
    const rainIn = raw.hourlyrainin ?? 0;
    const dewF = raw.dewPoint ?? raw.dewpoint;
    const dewC = dewF != null ? roundTo((dewF - 32) * 5 / 9, 1) : null;
    const sourceVerification = buildSourceVerificationEnvelope(stationMeta);

    return {
        temperature_c: tempC,
        humidity_pct: raw.humidity ?? null,
        pressure_hpa: pressure.submissionPressureHpa,
        // #3285: present only when a sea-level value exists; an absent key
        // (not null) is what the attester reads as "no claim".
        // pressure_kind travels with it so the attester knows whether the raw
        // value may be reduced from elevation ("station") or not ("relative").
        ...(pressure.seaLevelPressureHpa != null
            ? {
                pressure_sea_level_hpa: pressure.seaLevelPressureHpa,
                pressure_kind: pressure.pressureKind,
            }
            : {}),
        comparison_pressure_hpa: pressure.comparisonPressureHpa,
        pressure_comparison_method: pressure.pressureComparisonMethod,
        absolute_pressure_hpa: pressure.absolutePressureHpa,
        relative_pressure_hpa: pressure.relativePressureHpa,
        wind_speed_ms: windMs,
        wind_direction_deg: raw.winddir ?? null,
        wind_gust_ms: raw.windgustmph != null ? roundTo(raw.windgustmph * MPH_TO_MS, 1) : null,
        precipitation_mm: roundTo(rainIn * IN_TO_MM, 1),
        dew_point_c: dewC,
        uv_index: raw.uv ?? null,
        solar_radiation_wm2: raw.solarradiation ?? null,
        indoor_temperature_c: raw.tempinf != null ? roundTo((raw.tempinf - 32) * 5 / 9, 1) : null,
        indoor_humidity_pct: raw.humidityin ?? null,
        source: 'ambient_weather_network',
        source_id: stableSourceId(stationMeta),
        source_device: stationMeta.device_model || 'WS-5000',
        upstream_device_id_hash: upstreamDeviceIdHash(stationMeta),
        source_verification: sourceVerification,
        location: stationMeta.location || 'Duluth, MN',
        latitude: stationMeta.lat,
        longitude: stationMeta.lon,
        elevation_m: stationMeta.elevation_m ?? null,
        recorded_at: raw.dateutc || raw.date || null,
    };
}

function prepareReplayReadings(bundle) {
    if (Array.isArray(bundle.raw_history)) {
        return bundle.raw_history.map((reading) => normalizeReading(reading, bundle.station || {}));
    }

    if (!Array.isArray(bundle.normalized)) {
        throw new Error('Replay bundle must include either raw_history or normalized readings.');
    }

    return bundle.normalized.map((reading) => {
        if (reading.comparison_pressure_hpa != null || reading.pressure_hpa == null) {
            return reading;
        }

        if (bundle.station?.elevation_m != null && Number.isFinite(bundle.station.elevation_m)) {
            return {
                ...reading,
                comparison_pressure_hpa: approximateSeaLevelPressure(
                    reading.pressure_hpa,
                    bundle.station.elevation_m,
                ),
                pressure_comparison_method: 'replay_elevation_corrected_submission_pressure',
            };
        }

        return {
            ...reading,
            comparison_pressure_hpa: reading.pressure_hpa,
            pressure_comparison_method: 'replay_uncorrected_submission_pressure',
        };
    });
}

function pairReadings(pwsReadings, nwsObservations, maxDeltaSeconds) {
    const pairs = { temperature: [], pressure: [], humidity: [], wind: [] };

    for (const pws of pwsReadings) {
        const pwsTs = pws.recorded_at ? new Date(pws.recorded_at).getTime() : null;
        if (!pwsTs) continue;

        let bestNws = null;
        let bestDelta = Infinity;
        for (const nws of nwsObservations) {
            const delta = Math.abs(pwsTs - nws.timestamp_ms) / 1000;
            if (delta < bestDelta) {
                bestDelta = delta;
                bestNws = nws;
            }
        }

        if (!bestNws || bestDelta > maxDeltaSeconds) continue;

        if (pws.temperature_c != null && bestNws.temperature_c != null) {
            pairs.temperature.push({ sensor: pws.temperature_c, reference: bestNws.temperature_c });
        }
        const pressureHpa = pws.comparison_pressure_hpa ?? pws.pressure_hpa;
        if (pressureHpa != null && bestNws.pressure_hpa != null) {
            pairs.pressure.push({ sensor: pressureHpa, reference: bestNws.pressure_hpa });
        }
        if (pws.humidity_pct != null && bestNws.humidity_pct != null) {
            pairs.humidity.push({ sensor: pws.humidity_pct, reference: bestNws.humidity_pct });
        }
        if (pws.wind_speed_ms != null && bestNws.wind_speed_ms != null) {
            pairs.wind.push({ sensor: pws.wind_speed_ms, reference: bestNws.wind_speed_ms });
        }
    }

    return pairs;
}

function computeLegacyAccuracyMetrics(pairs) {
    const n = pairs.length;
    if (n === 0) return null;

    const errors = pairs.map((pair) => pair.sensor - pair.reference);
    const absErrors = errors.map((value) => Math.abs(value));
    const mae = absErrors.reduce((sum, value) => sum + value, 0) / n;
    const rmse = Math.sqrt(errors.reduce((sum, value) => sum + (value * value), 0) / n);
    const bias = errors.reduce((sum, value) => sum + value, 0) / n;

    const meanRef = pairs.reduce((sum, pair) => sum + pair.reference, 0) / n;
    const varianceRef = pairs.reduce(
        (sum, pair) => sum + ((pair.reference - meanRef) ** 2),
        0,
    ) / n;
    const referenceStddev = Math.sqrt(varianceRef);

    let legacyScore;
    if (referenceStddev > 0) {
        legacyScore = Math.max(0, Math.min(1, 1 - (rmse / referenceStddev)));
    } else if (rmse === 0) {
        legacyScore = 1;
    } else {
        legacyScore = 0;
    }

    return {
        mae,
        rmse,
        bias,
        legacy_accuracy_score: legacyScore,
        reference_stddev: referenceStddev,
        sample_count: n,
    };
}

function bandFromThresholds(mae, thresholds) {
    if (mae <= thresholds.excellent) return 'Excellent';
    if (mae <= thresholds.good) return 'Good';
    if (mae <= thresholds.marginal) return 'Marginal';
    return 'Poor';
}

function scoreFromThresholds(mae, thresholds) {
    if (mae <= thresholds.excellent) {
        const ratio = thresholds.excellent === 0 ? 1 : (mae / thresholds.excellent);
        return 1 - (0.1 * ratio);
    }
    if (mae <= thresholds.good) {
        const span = thresholds.good - thresholds.excellent || 1;
        const ratio = (mae - thresholds.excellent) / span;
        return 0.9 - (0.15 * ratio);
    }
    if (mae <= thresholds.marginal) {
        const span = thresholds.marginal - thresholds.good || 1;
        const ratio = (mae - thresholds.good) / span;
        return 0.75 - (0.25 * ratio);
    }
    if (mae <= thresholds.poor) {
        const span = thresholds.poor - thresholds.marginal || 1;
        const ratio = (mae - thresholds.marginal) / span;
        return Math.max(0, 0.5 - (0.5 * ratio));
    }
    return 0;
}

function residentialComparisonMetrics(mae, thresholds) {
    return {
        accuracy_score: scoreFromThresholds(mae, thresholds),
        band: bandFromThresholds(mae, thresholds),
    };
}

function legacyBand(score) {
    if (score >= 0.90) return 'Excellent';
    if (score >= 0.75) return 'Good';
    if (score >= 0.50) return 'Marginal';
    return 'Poor';
}

function computeComparisonMetrics(pairs, variableKey) {
    const legacy = computeLegacyAccuracyMetrics(pairs);
    if (!legacy) return null;

    const policy = VARIABLE_POLICIES[variableKey];
    if (!policy) {
        throw new Error(`No comparison policy configured for ${variableKey}`);
    }

    const legacyBandName = legacyBand(legacy.legacy_accuracy_score);
    const shouldUseResidentialScoring = legacy.reference_stddev <= policy.lowVarianceStddev;

    if (shouldUseResidentialScoring) {
        const residential = residentialComparisonMetrics(legacy.mae, policy.thresholds);
        return {
            mae: legacy.mae,
            rmse: legacy.rmse,
            bias: legacy.bias,
            accuracy_score: residential.accuracy_score,
            band: residential.band,
            sample_count: legacy.sample_count,
            scoring_method: 'residential_mae_bands',
            legacy_accuracy_score: legacy.legacy_accuracy_score,
            legacy_band: legacyBandName,
            reference_stddev: legacy.reference_stddev,
        };
    }

    return {
        mae: legacy.mae,
        rmse: legacy.rmse,
        bias: legacy.bias,
        accuracy_score: legacy.legacy_accuracy_score,
        band: legacyBandName,
        sample_count: legacy.sample_count,
        scoring_method: 'legacy_skill_score',
        legacy_accuracy_score: legacy.legacy_accuracy_score,
        legacy_band: legacyBandName,
        reference_stddev: legacy.reference_stddev,
    };
}

function buildAccuracyReport(pwsReadings, nwsObservations, maxDeltaSeconds) {
    const paired = pairReadings(pwsReadings, nwsObservations, maxDeltaSeconds);
    const variableDefs = [
        { key: 'temperature', label: 'Temperature (°C)' },
        { key: 'pressure', label: 'Pressure (hPa)' },
        { key: 'humidity', label: 'Humidity (%)' },
        { key: 'wind', label: 'Wind Speed (m/s)' },
    ];

    const report = {};
    for (const variable of variableDefs) {
        report[variable.key] = computeComparisonMetrics(paired[variable.key], variable.key);
    }

    return { paired, report, variableDefs };
}

function loadReplayBundle(replayFile) {
    const absolutePath = path.isAbsolute(replayFile)
        ? replayFile
        : path.resolve(process.cwd(), replayFile);
    const bundle = JSON.parse(fs.readFileSync(absolutePath, 'utf8'));
    return { bundle, absolutePath };
}

/**
 * Option-C hybrid quality tier for a single reading.
 *
 * Finds the closest NWS observation within maxDeltaSeconds.  For each primary
 * variable present in both the reading and the matched NWS observation,
 * computes the absolute residual and classifies it against the residential MAE
 * band thresholds.  The worst band across all variables determines the tier:
 *
 *   Excellent/Good → 'good'     (full reward)
 *   Marginal       → 'degraded' (reduced reward)
 *   Poor           → 'outlier'  (minimal reward, but still accepted)
 *
 * Returns 'good' when NWS is unavailable (no gating without reference data).
 *
 * @param {object}   reading          Normalized PWS reading
 * @param {object[]|null} nwsObs      Array of NWS observation objects, or null
 * @param {number}   maxDeltaSeconds  Max time gap for a valid NWS pair
 * @returns {'good'|'degraded'|'outlier'}
 */
function computeReadingQualityTier(reading, nwsObs, maxDeltaSeconds) {
    if (!nwsObs || nwsObs.length === 0) return 'good';

    const pwsTs = reading.recorded_at ? new Date(reading.recorded_at).getTime() : null;
    if (!pwsTs) return 'good';

    let bestNws = null;
    let bestDelta = Infinity;
    for (const obs of nwsObs) {
        const delta = Math.abs(pwsTs - obs.timestamp_ms) / 1000;
        if (delta < bestDelta) {
            bestDelta = delta;
            bestNws = obs;
        }
    }

    if (!bestNws || bestDelta > maxDeltaSeconds) return 'good';

    const checks = [
        {
            key: 'temperature',
            sensorVal: reading.temperature_c,
            refVal: bestNws.temperature_c,
        },
        {
            key: 'pressure',
            sensorVal: reading.comparison_pressure_hpa ?? reading.pressure_hpa,
            refVal: bestNws.pressure_hpa,
        },
        {
            key: 'humidity',
            sensorVal: reading.humidity_pct,
            refVal: bestNws.humidity_pct,
        },
        {
            key: 'wind',
            sensorVal: reading.wind_speed_ms,
            refVal: bestNws.wind_speed_ms,
        },
    ];

    const tierRank = { good: 0, degraded: 1, outlier: 2 };
    let worstTier = 'good';

    for (const { key, sensorVal, refVal } of checks) {
        if (sensorVal == null || refVal == null) continue;
        const policy = VARIABLE_POLICIES[key];
        if (!policy) continue;
        const mae = Math.abs(sensorVal - refVal);
        const band = bandFromThresholds(mae, policy.thresholds);
        const tier = band === 'Poor' ? 'outlier' : band === 'Marginal' ? 'degraded' : 'good';
        if (tierRank[tier] > tierRank[worstTier]) {
            worstTier = tier;
        }
    }

    return worstTier;
}

module.exports = {
    altimeterSettingHpa,
    approximateSeaLevelPressure,
    buildAccuracyReport,
    computeReadingQualityTier,
    derivePressureNormalization,
    loadReplayBundle,
    normalizeReading,
    pairReadings,
    prepareReplayReadings,
    SEA_LEVEL_ELEVATION_SOURCES,
};
