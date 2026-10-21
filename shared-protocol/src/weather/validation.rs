use serde::{Deserialize, Serialize};
use wrfcoin_meteo_types::{
    DEFAULT_MAX_TEMPERATURE_RATE_C_PER_MIN, EARTH_MIN_WIND_SPEED_MS,
    PhysicsRangePolicy as MeteoPhysicsRangePolicy,
};

pub use crate::weather::spatial::{CollusionPolicy, SpatialConsistencyPolicy};
pub use wrfcoin_meteo_types::{
    DataQualityBand, DataQualityEvaluation, DataQualityPolicy, DataQualityScore,
    ParsedWeatherPayload, RadioSourceMetadata, RadioSourceType, WEATHER_SCHEMA_V1,
    WeatherFieldKeyMode, WeatherObservation, WeatherObservationPoint, WeatherPayloadClassification,
    WeatherPayloadParseOptions, WeatherPayloadSchemaVersion, WeatherValidationPolicy,
    WeatherValidationResult,
};

fn canonical_physics_policy() -> MeteoPhysicsRangePolicy {
    MeteoPhysicsRangePolicy::default()
}

/// Policy controlling immediate slashing for physical-range violations in weather data.
///
/// Separate from `DataQualityPolicy` (score-based banding). These are hard outer
/// bounds; any value outside them indicates impossible/fabricated sensor data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataQualitySlashingPolicy {
    pub min_temperature_c: f64,
    pub max_temperature_c: f64,
    pub min_humidity_percent: f64,
    pub max_humidity_percent: f64,
    pub min_pressure_hpa: f64,
    pub max_pressure_hpa: f64,
    pub max_wind_speed_ms: f64,
    /// Slash amount in base token units per violation event.
    pub slash_amount: u128,
    /// Block height at or above which enforcement activates.
    pub activation_height: u64,
    /// Physics-range policy for structured violation detection (issue #372).
    #[serde(default)]
    pub physics_policy: PhysicsRangePolicy,
    /// Spatial consistency ("buddy check") policy (issue #808).
    #[serde(default)]
    pub spatial_policy: SpatialConsistencyPolicy,
    /// Collusion detection policy (issue #808).
    #[serde(default)]
    pub collusion_policy: CollusionPolicy,
}

impl Default for DataQualitySlashingPolicy {
    fn default() -> Self {
        let canonical = canonical_physics_policy();
        Self {
            min_temperature_c: canonical.temp_min_c,
            max_temperature_c: canonical.temp_max_c,
            min_humidity_percent: canonical.humidity_min_pct,
            max_humidity_percent: canonical.humidity_max_pct,
            min_pressure_hpa: canonical.pressure_min_hpa,
            max_pressure_hpa: canonical.pressure_max_hpa,
            max_wind_speed_ms: canonical.wind_speed_max_ms,
            slash_amount: 30,
            activation_height: 0,
            physics_policy: PhysicsRangePolicy::default(),
            spatial_policy: SpatialConsistencyPolicy::default(),
            collusion_policy: CollusionPolicy::default(),
        }
    }
}

/// Pure physics-range policy for weather readings.
///
/// Encapsulates hard meteorological bounds (derived from WMO world records) and
/// rate-of-change limits. Used by `validate_physics()` to produce structured
/// `PhysicsViolation` values rather than string-based errors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhysicsRangePolicy {
    /// Minimum plausible temperature in degrees Celsius (World record low: -89.2°C).
    pub temp_min_c: f64,
    /// Maximum plausible temperature in degrees Celsius (Death Valley air temperature record: +56.7°C).
    pub temp_max_c: f64,
    /// Minimum plausible barometric pressure in hPa (Typhoon Tip: 870 hPa).
    pub pressure_min_hpa: f64,
    /// Maximum plausible barometric pressure in hPa (Siberian High rounded to 1084 hPa).
    pub pressure_max_hpa: f64,
    /// Minimum relative humidity in percent.
    pub humidity_min_pct: f64,
    /// Maximum relative humidity in percent.
    pub humidity_max_pct: f64,
    /// Minimum plausible wind speed in m/s (calm = 0.0 m/s).
    pub wind_speed_min_ms: f64,
    /// Maximum plausible wind speed in m/s (Mount Washington gust record: 113.2 m/s).
    pub wind_speed_max_ms: f64,
    /// Maximum physically plausible temperature rate of change in degrees Celsius per minute.
    pub temp_rate_max_c_per_min: f64,
    /// Block height at or above which physics validation activates.
    pub activation_height: u64,
}

impl Default for PhysicsRangePolicy {
    fn default() -> Self {
        let canonical = canonical_physics_policy();
        Self {
            temp_min_c: canonical.temp_min_c,
            temp_max_c: canonical.temp_max_c,
            pressure_min_hpa: canonical.pressure_min_hpa,
            pressure_max_hpa: canonical.pressure_max_hpa,
            humidity_min_pct: canonical.humidity_min_pct,
            humidity_max_pct: canonical.humidity_max_pct,
            wind_speed_min_ms: EARTH_MIN_WIND_SPEED_MS,
            wind_speed_max_ms: canonical.wind_speed_max_ms,
            temp_rate_max_c_per_min: DEFAULT_MAX_TEMPERATURE_RATE_C_PER_MIN,
            activation_height: 0,
        }
    }
}

/// Canonical input to physics validation, parsed from a weather payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeatherReading {
    pub temperature_c: Option<f64>,
    pub pressure_hpa: Option<f64>,
    pub humidity_pct: Option<f64>,
    pub wind_speed_ms: Option<f64>,
    pub timestamp_unix: i64,
}

const MIN_SIGNAL_STRENGTH_DBM: f64 = -150.0;
const MAX_SIGNAL_STRENGTH_DBM: f64 = 20.0;
const SUB_GHZ_WEATHER_BANDS_MHZ: &[(f64, f64)] = &[(433.0, 434.79), (863.0, 870.0)];
const NOAA_APT_SATELLITE_BANDS_MHZ: &[(f64, f64)] = &[(137.0, 138.0)];
const LORA_BANDS_MHZ: &[(f64, f64)] = &[(433.0, 434.79), (863.0, 870.0), (902.0, 928.0)];
const ZIGBEE_BANDS_MHZ: &[(f64, f64)] = &[(2400.0, 2483.5)];

fn frequency_bands_for_source(source_type: RadioSourceType) -> &'static [(f64, f64)] {
    match source_type {
        RadioSourceType::Sdr => SUB_GHZ_WEATHER_BANDS_MHZ,
        RadioSourceType::Satellite => NOAA_APT_SATELLITE_BANDS_MHZ,
        RadioSourceType::Zigbee => ZIGBEE_BANDS_MHZ,
        RadioSourceType::LoRa => LORA_BANDS_MHZ,
    }
}

fn radio_source_label(source_type: RadioSourceType) -> &'static str {
    match source_type {
        RadioSourceType::Sdr => "SDR",
        RadioSourceType::Satellite => "Satellite NOAA APT",
        RadioSourceType::Zigbee => "Zigbee",
        RadioSourceType::LoRa => "LoRa",
    }
}

fn frequency_is_allowed_for_source(source_type: RadioSourceType, frequency_mhz: f64) -> bool {
    frequency_bands_for_source(source_type)
        .iter()
        .any(|(min, max)| (*min..=*max).contains(&frequency_mhz))
}

fn protocol_is_allowed_for_source(source_type: RadioSourceType, protocol: &str) -> bool {
    if !matches!(source_type, RadioSourceType::Satellite) {
        return true;
    }

    let normalized = protocol
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_'], " ");
    matches!(
        normalized.as_str(),
        "apt" | "noaa apt" | "noaa apt downlink"
    )
}

/// Validate radio-layer provenance attached to a weather observation.
///
/// Returns a list of hard validation errors. An empty list indicates success.
pub fn validate_radio_source_metadata(metadata: &RadioSourceMetadata) -> Vec<String> {
    let mut errors = Vec::new();

    if let Some(signal_strength_dbm) = metadata.signal_strength_dbm {
        if !signal_strength_dbm.is_finite()
            || !(MIN_SIGNAL_STRENGTH_DBM..=MAX_SIGNAL_STRENGTH_DBM).contains(&signal_strength_dbm)
        {
            errors.push(format!(
                "radio signal strength out of range: {signal_strength_dbm:.2} dBm not in [{MIN_SIGNAL_STRENGTH_DBM}, {MAX_SIGNAL_STRENGTH_DBM}]"
            ));
        }
    }

    if let Some(frequency_mhz) = metadata.frequency_mhz {
        if !frequency_mhz.is_finite() || frequency_mhz <= 0.0 {
            errors.push("radio frequency must be a finite positive MHz value".to_string());
        } else if !frequency_is_allowed_for_source(metadata.source_type, frequency_mhz) {
            errors.push(format!(
                "radio frequency {frequency_mhz:.3} MHz is not allowed for {} sources",
                radio_source_label(metadata.source_type)
            ));
        }
    }

    if let Some(protocol) = metadata.protocol.as_deref() {
        if protocol.trim().is_empty() {
            errors.push("radio protocol must not be blank".to_string());
        } else if !protocol_is_allowed_for_source(metadata.source_type, protocol) {
            errors.push(format!(
                "radio protocol '{protocol}' is not allowed for {} sources",
                radio_source_label(metadata.source_type)
            ));
        }
    }

    if let Some(raw_packet_hash) = metadata.raw_packet_hash.as_deref() {
        if raw_packet_hash.is_empty() {
            errors.push("radio packet hash must not be empty".to_string());
        } else if raw_packet_hash.len() % 2 != 0 || hex::decode(raw_packet_hash).is_err() {
            errors.push("radio packet hash must be valid even-length hex".to_string());
        }
    }

    errors
}

/// A structured physics violation produced by `validate_physics()`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PhysicsViolation {
    TemperatureOutOfRange {
        value_c: f64,
        min: f64,
        max: f64,
    },
    PressureOutOfRange {
        value_hpa: f64,
        min: f64,
        max: f64,
    },
    HumidityOutOfRange {
        value_pct: f64,
        min: f64,
        max: f64,
    },
    WindSpeedExceedsMaximum {
        value_ms: f64,
        max: f64,
    },
    WindSpeedBelowMinimum {
        value_ms: f64,
        min: f64,
    },
    TemperatureRateExceeded {
        delta_c: f64,
        limit: f64,
    },
    CrossParameterInconsistency {
        detail: String,
        dew_point_c: f64,
        temp_c: f64,
    },
}

/// Standard atmosphere expected pressure at a given elevation (meters).
/// ISA formula: P(h) = 1013.25 * (1 - 0.0000225577 * h)^5.25588
pub fn expected_pressure_at_altitude(elevation_m: f64) -> f64 {
    1013.25 * (1.0 - 0.0000225577 * elevation_m).powf(5.25588)
}

/// Standard tropospheric environmental lapse rate in degrees Celsius per meter.
///
/// Meteorological reference: the ISA/WMO standard atmosphere assumes air temperature
/// decreases by about 6.5°C per 1000 m in the lower troposphere.
const STANDARD_LAPSE_RATE_C_PER_M: f64 = 0.0065;

/// Shift sea-level temperature bounds down by the standard lapse-rate offset.
fn altitude_adjusted_temperature_bounds(
    elevation_m: f64,
    policy: &PhysicsRangePolicy,
) -> (f64, f64) {
    let offset = elevation_m * STANDARD_LAPSE_RATE_C_PER_M;
    (policy.temp_min_c - offset, policy.temp_max_c - offset)
}

/// Shift sea-level pressure bounds down by the altitude offset.
fn altitude_adjusted_pressure_bounds(elevation_m: f64, policy: &PhysicsRangePolicy) -> (f64, f64) {
    let offset = 1013.25 - expected_pressure_at_altitude(elevation_m);
    (
        policy.pressure_min_hpa - offset,
        policy.pressure_max_hpa - offset,
    )
}

/// Validate a `WeatherReading` against hard physical bounds and consistency rules.
///
/// Returns an empty `Vec` when `current_height < policy.activation_height`.
/// All present fields are checked independently; all violations are collected and
/// returned (not short-circuited), so callers can log or act on the full set.
///
/// # Altitude-aware pressure and temperature
/// When `elevation_m` is `Some`, pressure bounds are shifted down by the ISA
/// barometric offset and temperature bounds are shifted down by the standard
/// 6.5°C/km lapse rate so that high-altitude stations are validated against an
/// elevation-appropriate envelope.
///
/// # Rate-of-change
/// Requires `prev_reading` to check temperature rate of change. When `prev_reading`
/// is `None` the rate check is silently skipped.
///
/// # Cross-parameter
/// Approximates dew point as `temp - (100 - humidity) / 5.0` (Magnus approximation)
/// and asserts it does not exceed `temp + 2.0°C`.
pub fn validate_physics(
    reading: &WeatherReading,
    prev_reading: Option<&WeatherReading>,
    policy: &PhysicsRangePolicy,
    current_height: u64,
    elevation_m: Option<f64>,
) -> Vec<PhysicsViolation> {
    if current_height < policy.activation_height {
        return vec![];
    }

    let mut violations = Vec::new();
    let (temp_min_c, temp_max_c) = match elevation_m {
        Some(h) => altitude_adjusted_temperature_bounds(h, policy),
        None => (policy.temp_min_c, policy.temp_max_c),
    };

    if let Some(t) = reading.temperature_c {
        if !t.is_finite() || t < temp_min_c || t > temp_max_c {
            violations.push(PhysicsViolation::TemperatureOutOfRange {
                value_c: t,
                min: temp_min_c,
                max: temp_max_c,
            });
        }
    }

    if let Some(p) = reading.pressure_hpa {
        let (p_min, p_max) = match elevation_m {
            Some(h) => altitude_adjusted_pressure_bounds(h, policy),
            None => (policy.pressure_min_hpa, policy.pressure_max_hpa),
        };
        if !p.is_finite() || p < p_min || p > p_max {
            violations.push(PhysicsViolation::PressureOutOfRange {
                value_hpa: p,
                min: p_min,
                max: p_max,
            });
        }
    }

    if let Some(h) = reading.humidity_pct {
        if !h.is_finite() || h < policy.humidity_min_pct || h > policy.humidity_max_pct {
            violations.push(PhysicsViolation::HumidityOutOfRange {
                value_pct: h,
                min: policy.humidity_min_pct,
                max: policy.humidity_max_pct,
            });
        }
    }

    if let Some(w) = reading.wind_speed_ms {
        if !w.is_finite() {
            violations.push(PhysicsViolation::WindSpeedExceedsMaximum {
                value_ms: w,
                max: policy.wind_speed_max_ms,
            });
        } else if w < policy.wind_speed_min_ms {
            violations.push(PhysicsViolation::WindSpeedBelowMinimum {
                value_ms: w,
                min: policy.wind_speed_min_ms,
            });
        } else if w > policy.wind_speed_max_ms {
            violations.push(PhysicsViolation::WindSpeedExceedsMaximum {
                value_ms: w,
                max: policy.wind_speed_max_ms,
            });
        }
    }

    // Rate-of-change: temperature per minute
    if let Some(prev) = prev_reading {
        if let (Some(curr_t), Some(prev_t)) = (reading.temperature_c, prev.temperature_c) {
            let elapsed_secs = reading.timestamp_unix.saturating_sub(prev.timestamp_unix);
            if elapsed_secs > 0 {
                let elapsed_min = elapsed_secs as f64 / 60.0;
                let delta_c = (curr_t - prev_t).abs();
                let limit = policy.temp_rate_max_c_per_min * elapsed_min;
                if delta_c > limit {
                    violations.push(PhysicsViolation::TemperatureRateExceeded { delta_c, limit });
                }
            }
        }
    }

    // Cross-parameter: dew point consistency
    // Magnus approximation: dew_point ≈ temp - (100 - humidity) / 5
    // Physical law: dew point must not exceed temperature (± small tolerance)
    if let (Some(temp), Some(hum)) = (reading.temperature_c, reading.humidity_pct) {
        let approx_dp = temp - (100.0 - hum) / 5.0;
        if approx_dp > temp + 2.0 {
            violations.push(PhysicsViolation::CrossParameterInconsistency {
                detail: format!(
                    "approx dew point ({:.1}°C) exceeds temperature ({:.1}°C) + 2°C tolerance",
                    approx_dp, temp
                ),
                dew_point_c: approx_dp,
                temp_c: temp,
            });
        }
    }

    violations
}

/// Check whether a weather observation carries valid RF provenance suitable for
/// a reward bonus.
///
/// This is a **soft gate**: observations without RF metadata still return `false`
/// (no bonus), but they are NOT invalid — the caller decides whether to apply the
/// bonus. An observation with RF metadata that fails validation also returns `false`.
///
/// Returns `true` only when:
/// - `radio_metadata` is `Some`
/// - The frequency is present and falls within an allowed band for the source type
/// - No hard validation errors exist on the metadata
pub fn has_valid_rf_provenance(obs: &WeatherObservation) -> bool {
    let Some(metadata) = obs.radio_metadata.as_ref() else {
        return false;
    };
    // Must have a frequency to prove over-the-air reception.
    if metadata.frequency_mhz.is_none() {
        return false;
    }
    validate_radio_source_metadata(metadata).is_empty()
}

pub fn weather_observation_from_json(data: &serde_json::Value) -> Option<WeatherObservation> {
    WeatherObservation::from_json(data)
}

pub fn weather_observation_from_json_with_key_mode(
    data: &serde_json::Value,
    key_mode: WeatherFieldKeyMode,
) -> Option<WeatherObservation> {
    WeatherObservation::from_json_with_key_mode(data, key_mode)
}

pub fn validate_weather_observation(obs: &WeatherObservation) -> WeatherValidationResult {
    validate_weather_observation_with_elevation(obs, None)
}

pub fn validate_weather_observation_with_elevation(
    obs: &WeatherObservation,
    elevation_m: Option<f64>,
) -> WeatherValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut range_score = 1.0f64;
    let mut consistency_score = 1.0f64;
    let pressure_policy = PhysicsRangePolicy::default();
    let (temp_min_c, temp_max_c) = match elevation_m {
        Some(elevation_m) => altitude_adjusted_temperature_bounds(elevation_m, &pressure_policy),
        None => (pressure_policy.temp_min_c, pressure_policy.temp_max_c),
    };
    let (pressure_min_hpa, pressure_max_hpa) = match elevation_m {
        Some(elevation_m) => altitude_adjusted_pressure_bounds(elevation_m, &pressure_policy),
        None => (
            pressure_policy.pressure_min_hpa,
            pressure_policy.pressure_max_hpa,
        ),
    };

    check_range(
        "temperature",
        obs.temperature_c,
        temp_min_c,
        temp_max_c,
        &mut errors,
        &mut range_score,
    );
    check_range(
        "humidity",
        obs.humidity_percent,
        pressure_policy.humidity_min_pct,
        pressure_policy.humidity_max_pct,
        &mut errors,
        &mut range_score,
    );
    check_range(
        "pressure",
        obs.pressure_hpa,
        pressure_min_hpa,
        pressure_max_hpa,
        &mut errors,
        &mut range_score,
    );
    check_range(
        "wind_speed",
        obs.wind_speed_ms,
        pressure_policy.wind_speed_min_ms,
        pressure_policy.wind_speed_max_ms,
        &mut errors,
        &mut range_score,
    );
    check_range(
        "precipitation",
        obs.precipitation_mm,
        0.0,
        305.0,
        &mut errors,
        &mut range_score,
    );
    check_range(
        "dewpoint",
        obs.dewpoint_c,
        pressure_policy.temp_min_c,
        35.0,
        &mut errors,
        &mut range_score,
    );

    if let (Some(dew), Some(temp)) = (obs.dewpoint_c, obs.temperature_c) {
        if dew > temp {
            errors.push(format!(
                "dewpoint {dew:.2}C cannot exceed temperature {temp:.2}C"
            ));
            consistency_score *= 0.5;
        }
    }

    if let (Some(temp), Some(dew), Some(humidity)) =
        (obs.temperature_c, obs.dewpoint_c, obs.humidity_percent)
    {
        let expected = expected_relative_humidity(temp, dew);
        let delta = (humidity - expected).abs();
        if delta > 20.0 {
            warnings.push(format!(
                "humidity/dewpoint inconsistency: observed {humidity:.2}% vs expected {expected:.2}%"
            ));
            consistency_score *= 0.8;
        }
    }

    if let Some(radio_metadata) = obs.radio_metadata.as_ref() {
        errors.extend(validate_radio_source_metadata(radio_metadata));
    }

    let overall = (range_score * 0.7 + consistency_score * 0.3).clamp(0.0, 1.0);
    WeatherValidationResult {
        valid: errors.is_empty(),
        score: DataQualityScore {
            overall,
            range_score,
            consistency_score,
        },
        errors,
        warnings,
    }
}

pub fn validate_weather_observation_with_temporal_context(
    obs: &WeatherObservation,
    observed_at_unix: i64,
    observed_at_block: u64,
    previous: Option<&WeatherObservationPoint>,
    policy: &WeatherValidationPolicy,
) -> WeatherValidationResult {
    validate_weather_observation_with_temporal_context_and_elevation(
        obs,
        observed_at_unix,
        observed_at_block,
        previous,
        policy,
        None,
    )
}

pub fn validate_weather_observation_with_temporal_context_and_elevation(
    obs: &WeatherObservation,
    observed_at_unix: i64,
    observed_at_block: u64,
    previous: Option<&WeatherObservationPoint>,
    policy: &WeatherValidationPolicy,
    elevation_m: Option<f64>,
) -> WeatherValidationResult {
    validate_weather_observation_with_temporal_context_elevation_and_block_time(
        obs,
        observed_at_unix,
        observed_at_block,
        previous,
        policy,
        elevation_m,
        None,
    )
}

/// Consensus block-header timestamps for the #3270 halt-aware forward-skew
/// allowance (`ConsensusFeature::WeatherSkewBlockTimeAllowance`).
///
/// Both values are the unix-second timestamps of block HEADERS, which every
/// node validates identically; neither is ever read from the wall clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockTimeSkewContext {
    /// Timestamp of the block that accepted the station's previous reading.
    pub previous_accepting_block_unix: i64,
    /// Timestamp of the block now applying the reading.
    pub current_block_unix: i64,
}

impl BlockTimeSkewContext {
    /// Chain time elapsed between the two blocks, never negative.
    pub fn gap_seconds(&self) -> i64 {
        self.current_block_unix
            .saturating_sub(self.previous_accepting_block_unix)
            .max(0)
    }
}

/// [`validate_weather_observation_with_temporal_context_and_elevation`] with
/// the optional #3270 block-time term.
///
/// `block_time == None` is the pre-activation rule, byte for byte: the forward
/// allowance is `max(1, elapsed_blocks) × effective_max + grace`. With
/// `Some(ctx)` the allowance is the larger of that and
/// `ctx.gap_seconds() + grace`, so a reading is still bounded by consensus
/// chain time, but a halt (hours of block time, few blocks) no longer makes
/// every honest reading "too far in the future". Every other check is
/// unchanged.
pub fn validate_weather_observation_with_temporal_context_elevation_and_block_time(
    obs: &WeatherObservation,
    observed_at_unix: i64,
    observed_at_block: u64,
    previous: Option<&WeatherObservationPoint>,
    policy: &WeatherValidationPolicy,
    // determinism-ok: elevation passthrough, unchanged; #3270 adds only the block-time arg
    elevation_m: Option<f64>,
    block_time: Option<BlockTimeSkewContext>,
) -> WeatherValidationResult {
    let mut result = validate_weather_observation_with_elevation(obs, elevation_m);
    if observed_at_unix <= 0 {
        result
            .errors
            .push("timestamp must be a positive unix epoch value".to_string());
        result.valid = false;
        return result;
    }

    let Some(previous) = previous else {
        result.valid = result.errors.is_empty();
        return result;
    };

    if observed_at_unix < previous.observed_at_unix {
        result.errors.push(format!(
            "stale weather timestamp: {} < previous {}",
            observed_at_unix, previous.observed_at_unix
        ));
    }

    let elapsed_blocks = observed_at_block.saturating_sub(previous.observed_at_block);
    let elapsed_seconds = observed_at_unix.saturating_sub(previous.observed_at_unix);
    let dynamic_max_per_block =
        crate::consensus::proof_of_publishing::compute_target_block_time(observed_at_block) as i64;
    let effective_max = dynamic_max_per_block.min(policy.max_seconds_per_block);
    // Use max(1, elapsed_blocks) so that same-block batch submissions (multiple
    // observations per station per block) are allowed at least one block time's
    // worth of temporal spread.  At genesis block time (14400s), a single block
    // can legitimately contain an hour of 5-minute readings from one station.
    let block_count_allowance = (elapsed_blocks as i64)
        .max(1)
        .saturating_mul(effective_max)
        + policy.max_future_skew_seconds;
    match block_time {
        None => {
            if elapsed_seconds > block_count_allowance {
                result.errors.push(format!(
                    "future timestamp skew too large: delta={}s exceeds limit={}s for {} elapsed blocks",
                    elapsed_seconds, block_count_allowance, elapsed_blocks
                ));
            }
        },
        Some(ctx) => {
            // #3270 option B: max(block-count allowance, block-time gap + grace).
            let gap = ctx.gap_seconds();
            let block_time_allowance = gap.saturating_add(policy.max_future_skew_seconds);
            let max_forward_seconds = block_count_allowance.max(block_time_allowance);
            if elapsed_seconds > max_forward_seconds {
                result.errors.push(format!(
                    "future timestamp skew too large: delta={}s exceeds limit={}s for {} elapsed blocks and {}s block-time gap",
                    elapsed_seconds, max_forward_seconds, elapsed_blocks, gap
                ));
            }
        },
    }

    if elapsed_seconds > 0 {
        check_rate_limit(
            "temperature",
            obs.temperature_c,
            previous.observation.temperature_c,
            elapsed_seconds as f64,
            policy.max_temp_delta_per_second,
            &mut result.errors,
        );
        check_rate_limit(
            "humidity",
            obs.humidity_percent,
            previous.observation.humidity_percent,
            elapsed_seconds as f64,
            policy.max_humidity_delta_per_second,
            &mut result.errors,
        );
        check_rate_limit(
            "pressure",
            obs.pressure_hpa,
            previous.observation.pressure_hpa,
            elapsed_seconds as f64,
            policy.max_pressure_delta_per_second,
            &mut result.errors,
        );
        check_rate_limit(
            "wind_speed",
            obs.wind_speed_ms,
            previous.observation.wind_speed_ms,
            elapsed_seconds as f64,
            policy.max_wind_delta_per_second,
            &mut result.errors,
        );
        // Precipitation uses gauge-reset-aware validation instead of the
        // generic absolute-delta rate limiter (issue #950).
        let precip_check = check_precipitation_rate(
            obs.precipitation_mm,
            previous.observation.precipitation_mm,
            elapsed_seconds as f64,
        );
        result.errors.extend(precip_check.errors);
        result.warnings.extend(precip_check.warnings);

        check_rate_limit(
            "dewpoint",
            obs.dewpoint_c,
            previous.observation.dewpoint_c,
            elapsed_seconds as f64,
            policy.max_dewpoint_delta_per_second,
            &mut result.errors,
        );
    }

    result.valid = result.errors.is_empty();
    result
}

fn check_rate_limit(
    field: &str,
    current: Option<f64>,
    previous: Option<f64>,
    elapsed_seconds: f64,
    max_delta_per_second: f64,
    errors: &mut Vec<String>,
) {
    let (Some(current), Some(previous)) = (current, previous) else {
        return;
    };

    if elapsed_seconds <= 0.0 {
        return;
    }

    let delta_per_second = (current - previous).abs() / elapsed_seconds;
    if delta_per_second > max_delta_per_second {
        errors.push(format!(
            "{field} changed too quickly: {delta_per_second:.4}/s exceeds {max_delta_per_second:.4}/s"
        ));
    }
}

/// Maximum physically plausible rain accumulation rate in mm per minute.
///
/// The WMO world record is approximately 300 mm/hr (~5 mm/min).  We use
/// 4 mm/min as the hard ceiling for per-reading validation, providing a
/// small margin below the absolute physical maximum.
const MAX_PRECIP_RATE_MM_PER_MIN: f64 = 4.0;

/// Known power-of-two gauge counter widths that can overflow.
///
/// Consumer weather stations commonly use 8-bit, 10-bit, or 16-bit counters
/// for the precipitation accumulator.  A jump that lands exactly on one of
/// these boundaries is a strong signal of counter overflow.
const GAUGE_OVERFLOW_THRESHOLDS: &[f64] = &[256.0, 512.0, 1024.0];

/// Tolerance in mm for detecting exact power-of-two overflow jumps.
const GAUGE_OVERFLOW_TOLERANCE: f64 = 0.5;

/// Result of precipitation-specific validation.
#[derive(Debug, Clone, PartialEq)]
pub struct PrecipitationCheckResult {
    /// Hard errors that should cause rejection.
    pub errors: Vec<String>,
    /// Soft warnings for audit logging (gauge resets, overflow suspicion).
    pub warnings: Vec<String>,
}

/// Validate a precipitation reading against its predecessor, accounting for
/// rain gauge accumulator semantics.
///
/// Unlike temperature or pressure, consumer rain gauges report a **running
/// total** that periodically resets to zero (daily, or when the counter rolls
/// over).  A decrease in the reading is therefore a normal gauge reset, not
/// an anomaly.
///
/// # Rules
///
/// 1. **Gauge reset**: `current < previous` is flagged for audit but NOT
///    rejected.
/// 2. **Positive-increment rate**: The positive delta (`current - previous`)
///    divided by elapsed time must not exceed `MAX_PRECIP_RATE_MM_PER_MIN`.
/// 3. **Counter overflow**: A positive jump that matches a known power-of-two
///    boundary (256, 512, 1024 mm) is flagged as a probable hardware overflow.
pub fn check_precipitation_rate(
    current: Option<f64>,
    previous: Option<f64>,
    elapsed_seconds: f64,
) -> PrecipitationCheckResult {
    let mut result = PrecipitationCheckResult {
        errors: Vec::new(),
        warnings: Vec::new(),
    };

    let (Some(curr), Some(prev)) = (current, previous) else {
        return result;
    };

    if elapsed_seconds <= 0.0 {
        return result;
    }

    // Gauge reset: current < previous means the accumulator was zeroed.
    // This is expected behaviour — flag for audit, do not reject.
    if curr < prev {
        result.warnings.push(format!(
            "precipitation gauge reset detected: {prev:.1} mm -> {curr:.1} mm"
        ));
        return result;
    }

    let delta_mm = curr - prev;

    // Check for counter overflow (exact power-of-two jumps).
    for &threshold in GAUGE_OVERFLOW_THRESHOLDS {
        if (delta_mm - threshold).abs() < GAUGE_OVERFLOW_TOLERANCE {
            result.warnings.push(format!(
                "precipitation jump of {delta_mm:.1} mm is suspiciously close to \
                 {threshold:.0} mm — possible gauge counter overflow"
            ));
        }
    }

    // Rate-limit: only the positive increment needs bounding.
    let elapsed_min = elapsed_seconds / 60.0;
    if elapsed_min > 0.0 {
        let rate_mm_per_min = delta_mm / elapsed_min;
        if rate_mm_per_min > MAX_PRECIP_RATE_MM_PER_MIN {
            result.errors.push(format!(
                "precipitation rate {rate_mm_per_min:.2} mm/min exceeds \
                 physical maximum {MAX_PRECIP_RATE_MM_PER_MIN:.1} mm/min"
            ));
        }
    }

    result
}

fn check_range(
    field: &str,
    value: Option<f64>,
    min: f64,
    max: f64,
    errors: &mut Vec<String>,
    range_score: &mut f64,
) {
    if let Some(value) = value {
        if !value.is_finite() {
            errors.push(format!("{field} is not finite: {value}"));
            *range_score = 0.0;
            return;
        }
        if value < min || value > max {
            errors.push(format!(
                "{field} out of range: {value:.2} not in [{min}, {max}]"
            ));
            *range_score *= 0.6;
        }
    }
}

fn expected_relative_humidity(temp_c: f64, dewpoint_c: f64) -> f64 {
    // Tetens approximation.
    let sat_vapor_pressure = (17.625 * temp_c / (243.04 + temp_c)).exp();
    let actual_vapor_pressure = (17.625 * dewpoint_c / (243.04 + dewpoint_c)).exp();
    (100.0 * actual_vapor_pressure / sat_vapor_pressure).clamp(0.0, 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_meteo_types::{
        EARTH_MAX_AIR_TEMPERATURE_C, EARTH_MAX_PRESSURE_HPA, EARTH_MAX_RELATIVE_HUMIDITY_PERCENT,
        EARTH_MAX_WIND_SPEED_MS, EARTH_MIN_AIR_TEMPERATURE_C, EARTH_MIN_PRESSURE_HPA,
        EARTH_MIN_RELATIVE_HUMIDITY_PERCENT,
    };

    fn radio_metadata(source_type: RadioSourceType) -> RadioSourceMetadata {
        RadioSourceMetadata {
            source_type,
            frequency_mhz: Some(match source_type {
                RadioSourceType::Sdr => 433.92,
                RadioSourceType::Satellite => 137.62,
                RadioSourceType::Zigbee => 2405.0,
                RadioSourceType::LoRa => 915.0,
            }),
            signal_strength_dbm: Some(-67.0),
            protocol: Some(match source_type {
                RadioSourceType::Sdr => "Davis ISS".to_string(),
                RadioSourceType::Satellite => "NOAA APT".to_string(),
                RadioSourceType::Zigbee => "Zigbee Cluster Library".to_string(),
                RadioSourceType::LoRa => "LoRaWAN".to_string(),
            }),
            raw_packet_hash: Some("0123456789abcdef".to_string()),
            device_id: None,
        }
    }

    fn observed_weather() -> WeatherObservation {
        WeatherObservation {
            temperature_c: Some(19.0),
            humidity_percent: Some(65.0),
            pressure_hpa: Some(1005.0),
            wind_speed_ms: Some(7.0),
            precipitation_mm: Some(1.2),
            dewpoint_c: Some(12.0),
            radio_metadata: None,
        }
    }

    #[test]
    fn data_quality_slashing_policy_default_is_valid() {
        let policy = DataQualitySlashingPolicy::default();
        assert_eq!(policy.slash_amount, 30);
        assert_eq!(policy.activation_height, 0);
        assert_eq!(policy.min_temperature_c, EARTH_MIN_AIR_TEMPERATURE_C);
        assert_eq!(policy.max_temperature_c, EARTH_MAX_AIR_TEMPERATURE_C);
        assert_eq!(
            policy.min_humidity_percent,
            EARTH_MIN_RELATIVE_HUMIDITY_PERCENT
        );
        assert_eq!(
            policy.max_humidity_percent,
            EARTH_MAX_RELATIVE_HUMIDITY_PERCENT
        );
        assert_eq!(policy.min_pressure_hpa, EARTH_MIN_PRESSURE_HPA);
        assert_eq!(policy.max_pressure_hpa, EARTH_MAX_PRESSURE_HPA);
        assert_eq!(policy.max_wind_speed_ms, EARTH_MAX_WIND_SPEED_MS);
        // Physics policy is embedded with sensible defaults
        assert_eq!(
            policy.physics_policy.temp_min_c,
            EARTH_MIN_AIR_TEMPERATURE_C
        );
        assert_eq!(
            policy.physics_policy.temp_max_c,
            EARTH_MAX_AIR_TEMPERATURE_C
        );
        assert_eq!(policy.physics_policy.activation_height, 0);
    }

    // --- PhysicsRangePolicy / validate_physics tests ---

    #[test]
    fn temperature_out_of_range_triggers_violation() {
        let policy = PhysicsRangePolicy::default();
        let reading = WeatherReading {
            temperature_c: Some(200.0),
            pressure_hpa: Some(1013.0),
            humidity_pct: Some(50.0),
            wind_speed_ms: Some(5.0),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, None);
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, PhysicsViolation::TemperatureOutOfRange { .. })),
            "expected TemperatureOutOfRange, got: {violations:?}"
        );
    }

    #[test]
    fn pressure_in_range_passes() {
        let policy = PhysicsRangePolicy::default();
        let reading = WeatherReading {
            temperature_c: Some(20.0),
            pressure_hpa: Some(1013.25),
            humidity_pct: Some(60.0),
            wind_speed_ms: Some(5.0),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, None);
        assert!(
            violations.is_empty(),
            "expected no violations for normal reading, got: {violations:?}"
        );
    }

    #[test]
    fn humidity_boundary_edge_cases() {
        let policy = PhysicsRangePolicy::default();

        for valid_hum in [0.0_f64, 100.0_f64] {
            let reading = WeatherReading {
                temperature_c: Some(20.0),
                pressure_hpa: None,
                humidity_pct: Some(valid_hum),
                wind_speed_ms: None,
                timestamp_unix: 0,
            };
            let violations = validate_physics(&reading, None, &policy, 0, None);
            assert!(
                violations
                    .iter()
                    .all(|v| !matches!(v, PhysicsViolation::HumidityOutOfRange { .. })),
                "humidity {valid_hum} should be in range, got: {violations:?}"
            );
        }

        for invalid_hum in [-1.0_f64, 101.0_f64] {
            let reading = WeatherReading {
                temperature_c: Some(20.0),
                pressure_hpa: None,
                humidity_pct: Some(invalid_hum),
                wind_speed_ms: None,
                timestamp_unix: 0,
            };
            let violations = validate_physics(&reading, None, &policy, 0, None);
            assert!(
                violations
                    .iter()
                    .any(|v| matches!(v, PhysicsViolation::HumidityOutOfRange { .. })),
                "humidity {invalid_hum} should be out of range, got: {violations:?}"
            );
        }
    }

    #[test]
    fn rate_of_change_exceeded() {
        let policy = PhysicsRangePolicy::default(); // temp_rate_max_c_per_min = 5.0
        let prev = WeatherReading {
            temperature_c: Some(20.0),
            pressure_hpa: Some(1013.0),
            humidity_pct: Some(50.0),
            wind_speed_ms: Some(5.0),
            timestamp_unix: 1_700_000_000,
        };
        // 10°C change in 60 seconds (1 minute) → 10°C/min > 5.0°C/min limit
        let curr = WeatherReading {
            temperature_c: Some(30.0),
            pressure_hpa: Some(1013.0),
            humidity_pct: Some(50.0),
            wind_speed_ms: Some(5.0),
            timestamp_unix: 1_700_000_060,
        };
        let violations = validate_physics(&curr, Some(&prev), &policy, 0, None);
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, PhysicsViolation::TemperatureRateExceeded { .. })),
            "expected TemperatureRateExceeded, got: {violations:?}"
        );
    }

    #[test]
    fn dew_point_inconsistency_triggers_violation() {
        // Use a relaxed humidity max so the range check doesn't fire first;
        // the cross-parameter check fires when humidity > 110% (approx_dp > temp + 2°C).
        let policy = PhysicsRangePolicy {
            humidity_max_pct: 200.0,
            ..PhysicsRangePolicy::default()
        };
        // approx_dp = 20 - (100 - 115) / 5 = 20 + 3 = 23 > 20 + 2 = 22
        let reading = WeatherReading {
            temperature_c: Some(20.0),
            pressure_hpa: Some(1013.0),
            humidity_pct: Some(115.0),
            wind_speed_ms: Some(5.0),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, None);
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, PhysicsViolation::CrossParameterInconsistency { .. })),
            "expected CrossParameterInconsistency, got: {violations:?}"
        );
    }

    #[test]
    fn physics_inactive_before_activation_height() {
        let policy = PhysicsRangePolicy {
            activation_height: 1_000,
            ..PhysicsRangePolicy::default()
        };
        let reading = WeatherReading {
            temperature_c: Some(200.0), // would fail range check if active
            pressure_hpa: Some(50.0),   // would fail range check if active
            humidity_pct: Some(-10.0),  // would fail range check if active
            wind_speed_ms: Some(500.0), // would fail range check if active
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, None);
        assert!(
            violations.is_empty(),
            "physics validation must be inactive below activation_height, got: {violations:?}"
        );
    }

    // --- Altitude-aware pressure validation tests (issue #875) ---

    #[test]
    fn pressure_valid_at_sea_level_with_elevation() {
        let policy = PhysicsRangePolicy::default();
        let reading = WeatherReading {
            temperature_c: Some(20.0),
            pressure_hpa: Some(1013.25),
            humidity_pct: Some(50.0),
            wind_speed_ms: Some(5.0),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, Some(0.0));
        assert!(
            violations.is_empty(),
            "sea-level elevation should not change bounds, got: {violations:?}"
        );
    }

    #[test]
    fn pressure_valid_at_denver_altitude() {
        let policy = PhysicsRangePolicy::default();
        // Denver ~1600m, expected pressure ~838 hPa
        let reading = WeatherReading {
            temperature_c: Some(20.0),
            pressure_hpa: Some(835.0),
            humidity_pct: Some(50.0),
            wind_speed_ms: Some(5.0),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, Some(1600.0));
        assert!(
            !violations
                .iter()
                .any(|v| matches!(v, PhysicsViolation::PressureOutOfRange { .. })),
            "835 hPa is valid at Denver altitude (1600m), got: {violations:?}"
        );
    }

    #[test]
    fn pressure_valid_at_high_altitude() {
        let policy = PhysicsRangePolicy::default();
        // ~3000m, expected pressure ~701 hPa
        let reading = WeatherReading {
            temperature_c: Some(10.0),
            pressure_hpa: Some(700.0),
            humidity_pct: Some(40.0),
            wind_speed_ms: Some(3.0),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, Some(3000.0));
        assert!(
            !violations
                .iter()
                .any(|v| matches!(v, PhysicsViolation::PressureOutOfRange { .. })),
            "700 hPa is valid at 3000m elevation, got: {violations:?}"
        );
    }

    #[test]
    fn pressure_falls_back_without_elevation() {
        let policy = PhysicsRangePolicy::default();
        // 835 hPa is below sea-level minimum of 870 hPa — should fail without elevation
        let reading = WeatherReading {
            temperature_c: Some(20.0),
            pressure_hpa: Some(835.0),
            humidity_pct: Some(50.0),
            wind_speed_ms: Some(5.0),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, None);
        assert!(
            violations
                .iter()
                .any(|v| matches!(v, PhysicsViolation::PressureOutOfRange { .. })),
            "835 hPa should fail sea-level bounds without elevation, got: {violations:?}"
        );
    }

    fn assert_temperature_envelope_at_elevation(
        elevation_m: f64,
        valid_temp_c: f64,
        invalid_temp_c: f64,
    ) {
        let policy = PhysicsRangePolicy::default();
        let valid_reading = WeatherReading {
            temperature_c: Some(valid_temp_c),
            pressure_hpa: None,
            humidity_pct: None,
            wind_speed_ms: None,
            timestamp_unix: 0,
        };
        let invalid_reading = WeatherReading {
            temperature_c: Some(invalid_temp_c),
            pressure_hpa: None,
            humidity_pct: None,
            wind_speed_ms: None,
            timestamp_unix: 0,
        };

        let valid_violations =
            validate_physics(&valid_reading, None, &policy, 0, Some(elevation_m));
        assert!(
            !valid_violations
                .iter()
                .any(|v| matches!(v, PhysicsViolation::TemperatureOutOfRange { .. })),
            "{valid_temp_c}C should be valid at {elevation_m}m, got: {valid_violations:?}"
        );

        let invalid_violations =
            validate_physics(&invalid_reading, None, &policy, 0, Some(elevation_m));
        assert!(
            invalid_violations
                .iter()
                .any(|v| matches!(v, PhysicsViolation::TemperatureOutOfRange { .. })),
            "{invalid_temp_c}C should be out of range at {elevation_m}m, got: {invalid_violations:?}"
        );

        let valid_observation = WeatherObservation {
            temperature_c: Some(valid_temp_c),
            humidity_percent: None,
            pressure_hpa: None,
            wind_speed_ms: None,
            precipitation_mm: None,
            dewpoint_c: None,
            ..Default::default()
        };
        let invalid_observation = WeatherObservation {
            temperature_c: Some(invalid_temp_c),
            humidity_percent: None,
            pressure_hpa: None,
            wind_speed_ms: None,
            precipitation_mm: None,
            dewpoint_c: None,
            ..Default::default()
        };

        let valid_result =
            validate_weather_observation_with_elevation(&valid_observation, Some(elevation_m));
        assert!(
            valid_result.valid,
            "{valid_temp_c}C should pass observation validation at {elevation_m}m, got: {valid_result:?}"
        );

        let invalid_result =
            validate_weather_observation_with_elevation(&invalid_observation, Some(elevation_m));
        assert!(
            invalid_result
                .errors
                .iter()
                .any(|error| error.contains("temperature out of range")),
            "{invalid_temp_c}C should fail observation validation at {elevation_m}m, got: {invalid_result:?}"
        );
    }

    #[test]
    fn temperature_envelope_matches_sea_level_bounds() {
        assert_temperature_envelope_at_elevation(0.0, 50.0, 60.0);
    }

    #[test]
    fn temperature_envelope_shifts_at_1500m() {
        assert_temperature_envelope_at_elevation(1500.0, 45.0, 50.0);
    }

    #[test]
    fn temperature_envelope_shifts_at_3000m() {
        assert_temperature_envelope_at_elevation(3000.0, 35.0, 40.0);
    }

    #[test]
    fn temperature_envelope_shifts_at_5000m() {
        assert_temperature_envelope_at_elevation(5000.0, 20.0, 30.0);
    }

    #[test]
    fn weather_observation_detected_from_payload() {
        let payload = serde_json::json!({
            "temperature": 21.5,
            "humidity": 55.0,
            "pressure": 1008.2
        });
        let obs = weather_observation_from_json(&payload).unwrap();
        assert_eq!(obs.temperature_c, Some(21.5));
        assert_eq!(obs.humidity_percent, Some(55.0));
        assert_eq!(obs.pressure_hpa, Some(1008.2));
    }

    #[test]
    fn canonical_v1_key_mode_accepts_canonical_keys() {
        let payload = serde_json::json!({
            "temperature_c": 21.5,
            "humidity": 55.0,
            "pressure_hpa": 1008.2,
            "wind_speed_ms": 3.0,
            "precipitation": 0.2,
            "dewpoint_c": 13.0
        });
        let obs = weather_observation_from_json_with_key_mode(
            &payload,
            WeatherFieldKeyMode::CanonicalV1Only,
        )
        .expect("canonical payload should parse in strict mode");
        assert_eq!(obs.temperature_c, Some(21.5));
        assert_eq!(obs.humidity_percent, Some(55.0));
        assert_eq!(obs.precipitation_mm, Some(0.2));
    }

    #[test]
    fn canonical_v1_key_mode_rejects_alias_only_payload() {
        let payload = serde_json::json!({
            "temp_c": 21.5,
            "relative_humidity": 55.0,
            "pressure": 1008.2,
            "windSpeed": 3.0,
            "rainfall": 0.2,
            "dew_point": 13.0
        });
        let obs = weather_observation_from_json_with_key_mode(
            &payload,
            WeatherFieldKeyMode::CanonicalV1Only,
        );
        assert!(obs.is_none(), "alias-only payload should fail strict mode");
    }

    #[test]
    fn reexported_weather_observation_preserves_inherent_json_helpers() {
        let payload = serde_json::json!({
            "temperature": 21.5,
            "humidity": 55.0,
            "pressure": 1008.2
        });

        let via_inherent = WeatherObservation::from_json(&payload)
            .expect("legacy payload should parse through inherent helper");
        let via_free_fn = weather_observation_from_json(&payload)
            .expect("legacy payload should parse through shared-protocol helper");

        assert_eq!(via_inherent, via_free_fn);
    }

    #[test]
    fn reexported_weather_observation_preserves_strict_json_helper() {
        let payload = serde_json::json!({
            "temperature_c": 21.5,
            "humidity": 55.0,
            "pressure_hpa": 1008.2
        });

        let via_inherent = WeatherObservation::from_json_with_key_mode(
            &payload,
            WeatherFieldKeyMode::CanonicalV1Only,
        )
        .expect("canonical payload should parse through inherent helper");
        let via_free_fn = weather_observation_from_json_with_key_mode(
            &payload,
            WeatherFieldKeyMode::CanonicalV1Only,
        )
        .expect("canonical payload should parse through shared-protocol helper");

        assert_eq!(via_inherent, via_free_fn);
    }

    #[test]
    fn valid_sdr_radio_metadata_passes_validation() {
        let errors = validate_radio_source_metadata(&radio_metadata(RadioSourceType::Sdr));
        assert!(
            errors.is_empty(),
            "expected valid metadata, got: {errors:?}"
        );
    }

    #[test]
    fn validate_radio_source_metadata_accepts_satellite_noaa_apt_band() {
        let errors = validate_radio_source_metadata(&radio_metadata(RadioSourceType::Satellite));
        assert!(
            errors.is_empty(),
            "expected valid satellite metadata, got: {errors:?}"
        );
    }

    #[test]
    fn validate_radio_source_metadata_rejects_satellite_out_of_band_frequency() {
        let mut metadata = radio_metadata(RadioSourceType::Satellite);
        metadata.frequency_mhz = Some(433.92);
        let errors = validate_radio_source_metadata(&metadata);
        assert!(
            errors.iter().any(|error| error.contains(
                "radio frequency 433.920 MHz is not allowed for Satellite NOAA APT sources"
            )),
            "expected satellite frequency error, got: {errors:?}"
        );
    }

    #[test]
    fn validate_radio_source_metadata_rejects_satellite_non_apt_protocol() {
        let mut metadata = radio_metadata(RadioSourceType::Satellite);
        metadata.protocol = Some("LoRaWAN".to_string());
        let errors = validate_radio_source_metadata(&metadata);
        assert!(
            errors.iter().any(|error| error.contains(
                "radio protocol 'LoRaWAN' is not allowed for Satellite NOAA APT sources"
            )),
            "expected satellite protocol error, got: {errors:?}"
        );
    }

    #[test]
    fn validate_radio_source_metadata_satellite_serde_is_strict() {
        let encoded = serde_json::to_value(RadioSourceType::Satellite).expect("serialize");
        assert_eq!(encoded, serde_json::json!("Satellite"));

        let decoded: RadioSourceType =
            serde_json::from_value(encoded).expect("deserialize Satellite variant");
        assert_eq!(decoded, RadioSourceType::Satellite);

        assert!(
            serde_json::from_value::<RadioSourceType>(serde_json::json!("Orbital")).is_err(),
            "unknown radio source variants must fail fast"
        );
    }

    #[test]
    fn invalid_signal_strength_is_rejected() {
        let mut metadata = radio_metadata(RadioSourceType::Sdr);
        metadata.signal_strength_dbm = Some(-200.0);
        let errors = validate_radio_source_metadata(&metadata);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("radio signal strength out of range")),
            "expected signal strength error, got: {errors:?}"
        );
    }

    #[test]
    fn invalid_frequency_band_is_rejected() {
        let mut metadata = radio_metadata(RadioSourceType::Zigbee);
        metadata.frequency_mhz = Some(915.0);
        let errors = validate_radio_source_metadata(&metadata);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("radio frequency 915.000 MHz is not allowed")),
            "expected frequency error, got: {errors:?}"
        );
    }

    #[test]
    fn malformed_packet_hash_is_rejected() {
        let mut metadata = radio_metadata(RadioSourceType::LoRa);
        metadata.raw_packet_hash = Some("xyz".to_string());
        let errors = validate_radio_source_metadata(&metadata);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("radio packet hash must be valid even-length hex")),
            "expected packet hash error, got: {errors:?}"
        );
    }

    #[test]
    fn weather_observation_with_invalid_radio_metadata_fails_validation() {
        let mut obs = observed_weather();
        let mut metadata = radio_metadata(RadioSourceType::Sdr);
        metadata.frequency_mhz = Some(2450.0);
        obs.radio_metadata = Some(metadata);

        let result = validate_weather_observation(&obs);
        assert!(!result.valid, "invalid radio metadata must fail validation");
        assert!(
            result
                .errors
                .iter()
                .any(|error| error.contains("radio frequency 2450.000 MHz is not allowed")),
            "expected radio frequency error, got: {result:?}"
        );
    }

    #[test]
    fn weather_observation_with_valid_radio_metadata_passes_validation() {
        let mut obs = observed_weather();
        obs.radio_metadata = Some(radio_metadata(RadioSourceType::Zigbee));

        let result = validate_weather_observation(&obs);
        assert!(result.valid, "expected valid result, got: {result:?}");
    }

    #[test]
    fn valid_weather_observation_passes() {
        let obs = observed_weather();
        let result = validate_weather_observation(&obs);
        assert!(result.valid);
        assert!(result.score.overall > 0.8);
    }

    #[test]
    fn impossible_readings_are_rejected() {
        let obs = WeatherObservation {
            temperature_c: Some(120.0),
            humidity_percent: Some(180.0),
            pressure_hpa: Some(50.0),
            wind_speed_ms: Some(220.0),
            precipitation_mm: Some(-1.0),
            dewpoint_c: Some(130.0),
            ..Default::default()
        };
        let result = validate_weather_observation(&obs);
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn temporal_rejects_stale_timestamp() {
        let previous = WeatherObservationPoint {
            observed_at_unix: 1_700_000_100,
            observed_at_block: 10,
            observation: WeatherObservation {
                temperature_c: Some(20.0),
                humidity_percent: Some(60.0),
                pressure_hpa: Some(1008.0),
                wind_speed_ms: Some(6.0),
                precipitation_mm: Some(0.0),
                dewpoint_c: Some(12.0),
                ..Default::default()
            },
        };
        let current = WeatherObservation {
            temperature_c: Some(20.1),
            humidity_percent: Some(60.4),
            pressure_hpa: Some(1008.2),
            wind_speed_ms: Some(6.5),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(12.1),
            ..Default::default()
        };
        let result = validate_weather_observation_with_temporal_context(
            &current,
            1_700_000_099,
            11,
            Some(&previous),
            &WeatherValidationPolicy::default(),
        );
        assert!(!result.valid);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.contains("stale weather timestamp"))
        );
    }

    #[test]
    fn temporal_rejects_impossible_rate_change() {
        let previous = WeatherObservationPoint {
            observed_at_unix: 1_700_000_000,
            observed_at_block: 10,
            observation: WeatherObservation {
                temperature_c: Some(20.0),
                humidity_percent: Some(50.0),
                pressure_hpa: Some(1005.0),
                wind_speed_ms: Some(5.0),
                precipitation_mm: Some(0.0),
                dewpoint_c: Some(10.0),
                ..Default::default()
            },
        };
        let current = WeatherObservation {
            temperature_c: Some(30.0),
            humidity_percent: Some(85.0),
            pressure_hpa: Some(1000.0),
            wind_speed_ms: Some(30.0),
            precipitation_mm: Some(20.0),
            dewpoint_c: Some(20.0),
            ..Default::default()
        };
        let result = validate_weather_observation_with_temporal_context(
            &current,
            1_700_000_020,
            11,
            Some(&previous),
            &WeatherValidationPolicy::default(),
        );
        assert!(!result.valid);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.contains("changed too quickly"))
        );
    }

    #[test]
    fn synthetic_collector_pressure_trace_reaches_production_rate_guard_2737() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/security/fixtures/pressure-rate-trace-2737.json"
        ))
        .expect("pressure trace fixture must be valid JSON");
        let expected = &fixture["expected"];
        let previous_pressure = expected["previous_pressure_hpa"]
            .as_f64()
            .expect("previous pressure");
        let current_pressure = expected["current_pressure_hpa"]
            .as_f64()
            .expect("current pressure");
        let elapsed_seconds = expected["elapsed_seconds"]
            .as_i64()
            .expect("elapsed seconds");
        let expected_error = expected["validator_error"]
            .as_str()
            .expect("validator error");
        let previous_unix = fixture["previous"]["dateutc"]
            .as_i64()
            .expect("previous dateutc")
            / 1_000;
        let current_unix = fixture["current"]["dateutc"]
            .as_i64()
            .expect("current dateutc")
            / 1_000;
        assert_eq!(current_unix - previous_unix, elapsed_seconds);

        let previous = WeatherObservationPoint {
            observed_at_unix: previous_unix,
            observed_at_block: 10,
            observation: WeatherObservation {
                temperature_c: Some(20.0),
                humidity_percent: Some(50.0),
                pressure_hpa: Some(previous_pressure),
                wind_speed_ms: Some(0.9),
                precipitation_mm: Some(0.0),
                dewpoint_c: Some(10.0),
                ..Default::default()
            },
        };
        let current = WeatherObservation {
            temperature_c: Some(20.0),
            humidity_percent: Some(50.0),
            pressure_hpa: Some(current_pressure),
            wind_speed_ms: Some(0.9),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(10.0),
            ..Default::default()
        };
        let policy = WeatherValidationPolicy::default();
        assert_eq!(
            policy.max_pressure_delta_per_second,
            expected["validator_limit_hpa_per_second"]
                .as_f64()
                .expect("validator limit")
        );

        let result = validate_weather_observation_with_temporal_context(
            &current,
            current_unix,
            11,
            Some(&previous),
            &policy,
        );
        assert!(!result.valid);
        assert!(
            result.errors.iter().any(|error| error == expected_error),
            "expected exact pressure-rate rejection, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn temporal_rejects_excessive_future_skew() {
        // Use high block heights where effective_max = min(15, 14400) = 15.
        // For 1 elapsed block: max_forward = 15 + 900 = 915. Delta 1500 > 915.
        let previous = WeatherObservationPoint {
            observed_at_unix: 1_700_000_000,
            observed_at_block: 1_000_000,
            observation: WeatherObservation {
                temperature_c: Some(20.0),
                humidity_percent: Some(50.0),
                pressure_hpa: Some(1005.0),
                wind_speed_ms: Some(5.0),
                precipitation_mm: Some(0.0),
                dewpoint_c: Some(10.0),
                ..Default::default()
            },
        };
        let current = WeatherObservation {
            temperature_c: Some(20.2),
            humidity_percent: Some(50.5),
            pressure_hpa: Some(1005.3),
            wind_speed_ms: Some(5.2),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(10.1),
            ..Default::default()
        };
        let result = validate_weather_observation_with_temporal_context(
            &current,
            1_700_001_500,
            1_000_001,
            Some(&previous),
            &WeatherValidationPolicy::default(),
        );
        assert!(!result.valid);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.contains("future timestamp skew too large"))
        );
    }

    #[test]
    fn temporal_accepts_rapid_but_plausible_transition() {
        let previous = WeatherObservationPoint {
            observed_at_unix: 1_700_000_000,
            observed_at_block: 10,
            observation: WeatherObservation {
                temperature_c: Some(20.0),
                humidity_percent: Some(60.0),
                pressure_hpa: Some(1007.0),
                wind_speed_ms: Some(10.0),
                precipitation_mm: Some(2.0),
                dewpoint_c: Some(14.0),
                ..Default::default()
            },
        };
        let current = WeatherObservation {
            temperature_c: Some(24.0),
            humidity_percent: Some(72.0),
            pressure_hpa: Some(1005.5),
            wind_speed_ms: Some(15.0),
            precipitation_mm: Some(4.0),
            dewpoint_c: Some(16.0),
            ..Default::default()
        };
        let result = validate_weather_observation_with_temporal_context(
            &current,
            1_700_000_120,
            12,
            Some(&previous),
            &WeatherValidationPolicy::default(),
        );
        assert!(
            result.valid,
            "expected valid result, got {:?}",
            result.errors
        );
    }

    #[test]
    fn temporal_policy_can_be_tightened_via_config() {
        let previous = WeatherObservationPoint {
            observed_at_unix: 1_700_010_000,
            observed_at_block: 200,
            observation: WeatherObservation {
                temperature_c: Some(20.0),
                humidity_percent: Some(55.0),
                pressure_hpa: Some(1006.0),
                wind_speed_ms: Some(5.0),
                precipitation_mm: Some(0.0),
                dewpoint_c: Some(11.0),
                ..Default::default()
            },
        };
        let current = WeatherObservation {
            temperature_c: Some(20.3),
            humidity_percent: Some(55.5),
            pressure_hpa: Some(1006.0),
            wind_speed_ms: Some(5.1),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(11.1),
            ..Default::default()
        };
        let strict_policy = WeatherValidationPolicy {
            max_temp_delta_per_second: 0.001,
            ..WeatherValidationPolicy::default()
        };
        let result = validate_weather_observation_with_temporal_context(
            &current,
            1_700_010_030,
            201,
            Some(&previous),
            &strict_policy,
        );
        assert!(!result.valid);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.contains("temperature changed too quickly"))
        );
    }

    #[test]
    fn data_quality_policy_defaults_are_valid() {
        let policy = DataQualityPolicy::default();
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn data_quality_policy_classifies_high_warning_and_low_bands() {
        let policy = DataQualityPolicy::default();
        let high = DataQualityScore {
            overall: 0.95,
            range_score: 0.95,
            consistency_score: 0.95,
        };
        let warning = DataQualityScore {
            overall: 0.55,
            range_score: 0.55,
            consistency_score: 0.55,
        };
        let low = DataQualityScore {
            overall: 0.20,
            range_score: 0.20,
            consistency_score: 0.20,
        };

        let high_eval = policy.evaluate(Some(&high));
        assert_eq!(high_eval.band, DataQualityBand::High);
        assert!(!high_eval.trigger_challenge);

        let warning_eval = policy.evaluate(Some(&warning));
        assert_eq!(warning_eval.band, DataQualityBand::WarningOnly);
        assert!(warning_eval.warning_only);
        assert!(warning_eval.trigger_challenge);
        assert!(!warning_eval.counts_toward_low_quality_streak);

        let low_eval = policy.evaluate(Some(&low));
        assert_eq!(low_eval.band, DataQualityBand::Low);
        assert!(low_eval.trigger_challenge);
        assert!(low_eval.counts_toward_low_quality_streak);
    }

    #[test]
    fn data_quality_policy_rejects_invalid_threshold_ordering() {
        let mut policy = DataQualityPolicy::default();
        policy.warning_only_threshold = 0.75;
        policy.min_acceptance_threshold = 0.70;
        let err = policy
            .validate()
            .expect_err("warning threshold above acceptance must be rejected");
        assert!(err.contains("min_acceptance_threshold"));
    }

    #[test]
    fn temporal_accepts_genesis_era_observations() {
        // At genesis (block ~10), compute_target_block_time = 14400s.
        // Observations 7200s (2 hours) apart across 1 block should be accepted:
        // max_forward = 1 * 14400 + 900 = 15300 > 7200.
        let previous = WeatherObservationPoint {
            observed_at_unix: 1_700_000_000,
            observed_at_block: 10,
            observation: WeatherObservation {
                temperature_c: Some(20.0),
                humidity_percent: Some(55.0),
                pressure_hpa: Some(1010.0),
                wind_speed_ms: Some(3.0),
                precipitation_mm: Some(0.0),
                dewpoint_c: Some(10.0),
                ..Default::default()
            },
        };
        let current = WeatherObservation {
            temperature_c: Some(21.0),
            humidity_percent: Some(56.0),
            pressure_hpa: Some(1009.5),
            wind_speed_ms: Some(3.5),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(10.5),
            ..Default::default()
        };
        let result = validate_weather_observation_with_temporal_context(
            &current,
            1_700_007_200, // 7200s later
            11,
            Some(&previous),
            &WeatherValidationPolicy::default(),
        );
        assert!(
            result.valid,
            "genesis-era observations 2 hours apart should be accepted, got: {:?}",
            result.errors
        );
    }

    // --- Wind speed lower bound tests (issue #876) ---

    #[test]
    fn negative_wind_speed_triggers_violation() {
        let policy = PhysicsRangePolicy::default();
        let reading = WeatherReading {
            temperature_c: Some(20.0),
            pressure_hpa: Some(1013.0),
            humidity_pct: Some(50.0),
            wind_speed_ms: Some(-50.0),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, None);
        assert_eq!(violations.len(), 1);
        assert!(
            matches!(
                &violations[0],
                PhysicsViolation::WindSpeedBelowMinimum { value_ms, min }
                if *value_ms == -50.0 && *min == 0.0
            ),
            "expected WindSpeedBelowMinimum for -50.0, got: {violations:?}"
        );
    }

    #[test]
    fn zero_wind_speed_is_valid() {
        let policy = PhysicsRangePolicy::default();
        let reading = WeatherReading {
            temperature_c: Some(20.0),
            pressure_hpa: Some(1013.0),
            humidity_pct: Some(50.0),
            wind_speed_ms: Some(0.0),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, None);
        assert!(
            violations.is_empty(),
            "calm wind (0.0 m/s) must be valid, got: {violations:?}"
        );
    }

    #[test]
    fn upper_bound_wind_speed_still_valid() {
        let policy = PhysicsRangePolicy::default();
        let reading = WeatherReading {
            temperature_c: Some(20.0),
            pressure_hpa: Some(1013.0),
            humidity_pct: Some(50.0),
            wind_speed_ms: Some(EARTH_MAX_WIND_SPEED_MS),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, None);
        assert!(
            violations.is_empty(),
            "{EARTH_MAX_WIND_SPEED_MS} m/s is within canonical wind bounds, got: {violations:?}"
        );
    }

    #[test]
    fn temporal_rejects_excessive_skew_at_high_height() {
        // At height 1_000_001, compute_target_block_time = 15s.
        // effective_max = min(15, 14400) = 15. For 1 elapsed block:
        // max_forward = 1 * 15 + 900 = 915s. Delta of 2000s should be rejected.
        let previous = WeatherObservationPoint {
            observed_at_unix: 1_700_000_000,
            observed_at_block: 1_000_000,
            observation: WeatherObservation {
                temperature_c: Some(20.0),
                humidity_percent: Some(55.0),
                pressure_hpa: Some(1010.0),
                wind_speed_ms: Some(3.0),
                precipitation_mm: Some(0.0),
                dewpoint_c: Some(10.0),
                ..Default::default()
            },
        };
        let current = WeatherObservation {
            temperature_c: Some(20.1),
            humidity_percent: Some(55.1),
            pressure_hpa: Some(1010.0),
            wind_speed_ms: Some(3.0),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(10.0),
            ..Default::default()
        };
        let result = validate_weather_observation_with_temporal_context(
            &current,
            1_700_002_000, // 2000s later
            1_000_001,
            Some(&previous),
            &WeatherValidationPolicy::default(),
        );
        assert!(!result.valid);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.contains("future timestamp skew too large")),
            "expected future skew rejection at high block height, got: {:?}",
            result.errors
        );
    }

    // --- NaN / Infinity guard tests (issue #990) ---

    #[test]
    fn nan_rejected_by_check_range() {
        let mut errors = Vec::new();
        let mut score = 1.0;
        check_range("temperature", Some(f64::NAN), -89.2, 70.7, &mut errors, &mut score);
        assert!(!errors.is_empty(), "NaN must produce an error");
        assert_eq!(score, 0.0, "NaN must zero out the score");
        assert!(errors[0].contains("not finite"), "error should mention 'not finite'");
    }

    #[test]
    fn positive_infinity_rejected_by_check_range() {
        let mut errors = Vec::new();
        let mut score = 1.0;
        check_range("humidity", Some(f64::INFINITY), 0.0, 100.0, &mut errors, &mut score);
        assert!(!errors.is_empty(), "Infinity must produce an error");
        assert_eq!(score, 0.0, "Infinity must zero out the score");
    }

    #[test]
    fn negative_infinity_rejected_by_check_range() {
        let mut errors = Vec::new();
        let mut score = 1.0;
        check_range("pressure", Some(f64::NEG_INFINITY), 870.0, 1085.0, &mut errors, &mut score);
        assert!(!errors.is_empty(), "-Infinity must produce an error");
        assert_eq!(score, 0.0, "-Infinity must zero out the score");
    }

    #[test]
    fn nan_temperature_rejected_by_validate_physics() {
        let policy = PhysicsRangePolicy::default();
        let reading = WeatherReading {
            temperature_c: Some(f64::NAN),
            pressure_hpa: Some(1013.0),
            humidity_pct: Some(50.0),
            wind_speed_ms: Some(5.0),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, None);
        assert!(
            violations.iter().any(|v| matches!(v, PhysicsViolation::TemperatureOutOfRange { .. })),
            "NaN temperature must be rejected, got: {violations:?}"
        );
    }

    #[test]
    fn nan_pressure_rejected_by_validate_physics() {
        let policy = PhysicsRangePolicy::default();
        let reading = WeatherReading {
            temperature_c: Some(20.0),
            pressure_hpa: Some(f64::NAN),
            humidity_pct: Some(50.0),
            wind_speed_ms: Some(5.0),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, None);
        assert!(
            violations.iter().any(|v| matches!(v, PhysicsViolation::PressureOutOfRange { .. })),
            "NaN pressure must be rejected, got: {violations:?}"
        );
    }

    #[test]
    fn infinity_wind_rejected_by_validate_physics() {
        let policy = PhysicsRangePolicy::default();
        let reading = WeatherReading {
            temperature_c: Some(20.0),
            pressure_hpa: Some(1013.0),
            humidity_pct: Some(50.0),
            wind_speed_ms: Some(f64::INFINITY),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, None);
        assert!(
            violations.iter().any(|v| matches!(v, PhysicsViolation::WindSpeedExceedsMaximum { .. })),
            "Infinity wind speed must be rejected, got: {violations:?}"
        );
    }

    #[test]
    fn nan_humidity_rejected_by_validate_physics() {
        let policy = PhysicsRangePolicy::default();
        let reading = WeatherReading {
            temperature_c: Some(20.0),
            pressure_hpa: Some(1013.0),
            humidity_pct: Some(f64::NAN),
            wind_speed_ms: Some(5.0),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, None);
        assert!(
            violations.iter().any(|v| matches!(v, PhysicsViolation::HumidityOutOfRange { .. })),
            "NaN humidity must be rejected, got: {violations:?}"
        );
    }

    // --- Precipitation gauge-reset validation tests (issue #950) ---

    #[test]
    fn precip_gauge_reset_is_warning_not_error() {
        // Gauge resets from 23.4 -> 0.2 (typical daily reset).
        let result = check_precipitation_rate(Some(0.2), Some(23.4), 300.0);
        assert!(
            result.errors.is_empty(),
            "gauge reset must not produce errors, got: {:?}",
            result.errors
        );
        assert!(
            result.warnings.iter().any(|w| w.contains("gauge reset")),
            "gauge reset should produce a warning, got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn precip_normal_accumulation_passes() {
        // Normal light rain: 5.0 -> 12.0 over 5 minutes (300s).
        // Rate = 7.0 / 5.0 = 1.4 mm/min (well under 4.0 mm/min limit).
        let result = check_precipitation_rate(Some(12.0), Some(5.0), 300.0);
        assert!(
            result.errors.is_empty(),
            "normal accumulation must pass, got: {:?}",
            result.errors
        );
        assert!(
            result.warnings.is_empty(),
            "normal accumulation should not warn, got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn precip_heavy_rain_within_limit() {
        // Heavy rain: 0.0 -> 15.0 over 5 minutes (300s).
        // Rate = 15.0 / 5.0 = 3.0 mm/min (under 4.0 mm/min limit).
        let result = check_precipitation_rate(Some(15.0), Some(0.0), 300.0);
        assert!(
            result.errors.is_empty(),
            "heavy but plausible rain must pass, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn precip_exceeds_physical_maximum_rate() {
        // Impossible rate: 0.0 -> 30.0 over 1 minute (60s).
        // Rate = 30.0 / 1.0 = 30.0 mm/min >> 4.0 mm/min limit.
        let result = check_precipitation_rate(Some(30.0), Some(0.0), 60.0);
        assert!(
            result.errors.iter().any(|e| e.contains("precipitation rate")),
            "impossible rate must be rejected, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn precip_sensor_overflow_256_flagged() {
        // Jump of exactly 256 mm signals 8-bit counter overflow.
        // 5.0 -> 261.0 = 256 mm delta.
        let result = check_precipitation_rate(Some(261.0), Some(5.0), 3600.0);
        assert!(
            result.warnings.iter().any(|w| w.contains("256")),
            "256 mm jump should flag counter overflow, got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn precip_sensor_overflow_1024_flagged() {
        // Jump of ~1024 mm signals 16-bit counter overflow.
        let result = check_precipitation_rate(Some(1029.0), Some(5.0), 7200.0);
        assert!(
            result.warnings.iter().any(|w| w.contains("1024")),
            "1024 mm jump should flag counter overflow, got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn precip_none_values_pass_silently() {
        let result = check_precipitation_rate(None, Some(5.0), 300.0);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());

        let result2 = check_precipitation_rate(Some(5.0), None, 300.0);
        assert!(result2.errors.is_empty());
        assert!(result2.warnings.is_empty());
    }

    #[test]
    fn precip_zero_elapsed_passes_silently() {
        let result = check_precipitation_rate(Some(10.0), Some(5.0), 0.0);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn temporal_accepts_gauge_reset_in_full_pipeline() {
        // End-to-end: gauge resets should not cause temporal validation to fail.
        let previous = WeatherObservationPoint {
            observed_at_unix: 1_700_000_000,
            observed_at_block: 10,
            observation: WeatherObservation {
                temperature_c: Some(20.0),
                humidity_percent: Some(55.0),
                pressure_hpa: Some(1010.0),
                wind_speed_ms: Some(3.0),
                precipitation_mm: Some(23.4),
                dewpoint_c: Some(10.0),
                ..Default::default()
            },
        };
        let current = WeatherObservation {
            temperature_c: Some(20.1),
            humidity_percent: Some(55.1),
            pressure_hpa: Some(1010.0),
            wind_speed_ms: Some(3.0),
            precipitation_mm: Some(0.2), // gauge reset
            dewpoint_c: Some(10.0),
            ..Default::default()
        };
        let result = validate_weather_observation_with_temporal_context(
            &current,
            1_700_000_300, // 300s later
            11,
            Some(&previous),
            &WeatherValidationPolicy::default(),
        );
        assert!(
            result.valid,
            "gauge reset must not cause validation failure, got: {:?}",
            result.errors
        );
        assert!(
            result.warnings.iter().any(|w| w.contains("gauge reset")),
            "gauge reset should produce a warning, got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn temporal_rejects_impossible_precip_rate() {
        // Impossible accumulation: 0 -> 300 mm in 60 seconds.
        // Rate = 300 / 1.0 = 300 mm/min >> 4.0 mm/min limit.
        // 300 mm is within the static range check (0-305), so only the
        // rate check fires.
        let previous = WeatherObservationPoint {
            observed_at_unix: 1_700_000_000,
            observed_at_block: 10,
            observation: WeatherObservation {
                temperature_c: Some(20.0),
                humidity_percent: Some(55.0),
                pressure_hpa: Some(1010.0),
                wind_speed_ms: Some(3.0),
                precipitation_mm: Some(0.0),
                dewpoint_c: Some(10.0),
                ..Default::default()
            },
        };
        let current = WeatherObservation {
            temperature_c: Some(20.0),
            humidity_percent: Some(55.0),
            pressure_hpa: Some(1010.0),
            wind_speed_ms: Some(3.0),
            precipitation_mm: Some(300.0),
            dewpoint_c: Some(10.0),
            ..Default::default()
        };
        let result = validate_weather_observation_with_temporal_context(
            &current,
            1_700_000_060,
            11,
            Some(&previous),
            &WeatherValidationPolicy::default(),
        );
        assert!(
            !result.valid,
            "impossible precipitation rate must be rejected"
        );
        assert!(
            result.errors.iter().any(|e| e.contains("precipitation rate")),
            "error should mention precipitation rate, got: {:?}",
            result.errors
        );
    }

    // --- RF provenance soft-gate tests (#729) ---

    #[test]
    fn rf_provenance_valid_sdr_metadata_returns_true() {
        let mut obs = observed_weather();
        obs.radio_metadata = Some(radio_metadata(RadioSourceType::Sdr));
        assert!(
            has_valid_rf_provenance(&obs),
            "valid SDR metadata should grant RF provenance"
        );
    }

    #[test]
    fn rf_provenance_missing_metadata_returns_false() {
        let obs = observed_weather();
        assert!(
            !has_valid_rf_provenance(&obs),
            "missing radio_metadata is a soft no — no provenance bonus, not an error"
        );
        // The observation itself is still valid:
        let result = validate_weather_observation(&obs);
        assert!(result.valid, "observation without RF metadata is still valid");
    }

    #[test]
    fn rf_provenance_invalid_frequency_returns_false() {
        let mut obs = observed_weather();
        let mut metadata = radio_metadata(RadioSourceType::Sdr);
        // 2450 MHz is Zigbee band, not valid for SDR source
        metadata.frequency_mhz = Some(2450.0);
        obs.radio_metadata = Some(metadata);
        assert!(
            !has_valid_rf_provenance(&obs),
            "SDR source with Zigbee-band frequency should fail provenance"
        );
    }

    #[test]
    fn rf_provenance_missing_frequency_returns_false() {
        let mut obs = observed_weather();
        let mut metadata = radio_metadata(RadioSourceType::Sdr);
        metadata.frequency_mhz = None;
        obs.radio_metadata = Some(metadata);
        assert!(
            !has_valid_rf_provenance(&obs),
            "metadata without frequency cannot prove over-the-air reception"
        );
    }

    #[test]
    fn rf_provenance_with_device_id_returns_true() {
        let mut obs = observed_weather();
        let mut metadata = radio_metadata(RadioSourceType::Sdr);
        metadata.device_id = Some("rtlsdr-00000001".to_string());
        obs.radio_metadata = Some(metadata);
        assert!(
            has_valid_rf_provenance(&obs),
            "valid metadata with device_id should grant RF provenance"
        );
    }

    // ── #3270: halt-aware forward-skew allowance (option B) ────────────────
    //
    // The 2026-09-22 → 2026-09-24 halt, reconstructed from node container block
    // headers: the reference station’s last accepted reading (observed 21:20:00Z) was accepted
    // in h21101 (21:23:32Z); the chain halted after h21105 (21:29:01Z) and
    // resumed at h21106 (2026-09-24 03:35:29Z); the backlog reveal (observed
    // 02:40:00Z, 105600 s later) was tried in h21108 (03:35:34Z), 7 blocks on.
    const H21101: u64 = 21_101;
    const H21108: u64 = 21_108;
    const H21101_BLOCK_UNIX: i64 = 1_790_112_212; // 2026-09-22T21:23:32Z
    const H21108_BLOCK_UNIX: i64 = 1_790_220_934; // 2026-09-24T03:35:34Z
    const PREV_OBS_UNIX: i64 = 1_790_112_000; // 2026-09-22T21:20:00Z
    const BACKLOG_DELTA: i64 = 105_600;

    fn steady_obs() -> WeatherObservation {
        WeatherObservation {
            temperature_c: Some(12.0),
            humidity_percent: Some(70.0),
            pressure_hpa: Some(990.0),
            wind_speed_ms: Some(2.0),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(6.8),
            ..Default::default()
        }
    }

    fn prev_point(observed_at_unix: i64, observed_at_block: u64) -> WeatherObservationPoint {
        WeatherObservationPoint {
            observed_at_unix,
            observed_at_block,
            observation: steady_obs(),
        }
    }

    fn validate_3270(
        delta: i64,
        prev_block: u64,
        current_block: u64,
        block_time: Option<BlockTimeSkewContext>,
    ) -> WeatherValidationResult {
        validate_weather_observation_with_temporal_context_elevation_and_block_time(
            &steady_obs(),
            PREV_OBS_UNIX + delta,
            current_block,
            Some(&prev_point(PREV_OBS_UNIX, prev_block)),
            &WeatherValidationPolicy::default(),
            None,
            block_time,
        )
    }

    fn halt_ctx() -> BlockTimeSkewContext {
        BlockTimeSkewContext {
            previous_accepting_block_unix: H21101_BLOCK_UNIX,
            current_block_unix: H21108_BLOCK_UNIX,
        }
    }

    #[test]
    fn regression_2026_09_24_halt_backlog_rejected_pre_activation_with_the_live_error_3270() {
        // Pre-activation (`None`): the exact string the N5 node logged.
        let result = validate_3270(BACKLOG_DELTA, H21101, H21108, None);
        assert!(!result.valid);
        assert_eq!(
            result.errors,
            vec![
                "future timestamp skew too large: delta=105600s exceeds limit=26100s for 7 \
                 elapsed blocks"
                    .to_string()
            ]
        );
        // The pre-activation entry point is the `None` path, byte for byte.
        let legacy = validate_weather_observation_with_temporal_context_and_elevation(
            &steady_obs(),
            PREV_OBS_UNIX + BACKLOG_DELTA,
            H21108,
            Some(&prev_point(PREV_OBS_UNIX, H21101)),
            &WeatherValidationPolicy::default(),
            None,
        );
        assert_eq!(legacy.errors, result.errors);
        assert_eq!(legacy.valid, result.valid);
    }

    #[test]
    fn regression_2026_09_24_halt_backlog_accepted_post_activation_3270() {
        assert_eq!(halt_ctx().gap_seconds(), 108_722, "30h12m02s of block time");
        // Every JON delta the node skipped at h21108 (105600..=108900) passes.
        for delta in [BACKLOG_DELTA, 107_000, 108_900] {
            let result = validate_3270(delta, H21101, H21108, Some(halt_ctx()));
            assert!(result.valid, "delta {delta}: {:?}", result.errors);
        }
    }

    #[test]
    fn block_time_term_is_bounded_by_the_gap_plus_grace_3270() {
        // Boundary: gap + 900 accepted, gap + 901 rejected (with the gap named).
        let gap = halt_ctx().gap_seconds();
        assert!(validate_3270(gap + 900, H21101, H21108, Some(halt_ctx())).valid);
        let over = validate_3270(gap + 901, H21101, H21108, Some(halt_ctx()));
        assert!(!over.valid);
        assert_eq!(
            over.errors,
            vec![format!(
                "future timestamp skew too large: delta={}s exceeds limit={}s for 7 elapsed \
                 blocks and {}s block-time gap",
                gap + 901,
                gap + 900,
                gap
            )]
        );
    }

    #[test]
    fn far_future_reading_is_still_rejected_post_activation_3270() {
        // Attack: a station claims a reading ten days ahead after the same halt.
        let result = validate_3270(10 * 86_400, H21101, H21108, Some(halt_ctx()));
        assert!(!result.valid);
        assert!(
            result.errors.iter().any(|e| e.starts_with("future timestamp skew too large")),
            "{:?}",
            result.errors
        );
        // Without a halt (block time on cadence) the far-future reading gets
        // no extra room from the block-time term either.
        let on_cadence = BlockTimeSkewContext {
            previous_accepting_block_unix: H21101_BLOCK_UNIX,
            current_block_unix: H21101_BLOCK_UNIX + 7 * 300,
        };
        assert!(!validate_3270(10 * 86_400, H21101, H21108, Some(on_cadence)).valid);
        // A backwards block-time pair (impossible under the monotonic header
        // rule) clamps to a zero gap and never widens the allowance.
        let backwards = BlockTimeSkewContext {
            previous_accepting_block_unix: H21108_BLOCK_UNIX,
            current_block_unix: H21101_BLOCK_UNIX,
        };
        assert_eq!(backwards.gap_seconds(), 0);
        let r = validate_3270(26_101, H21101, H21108, Some(backwards));
        assert!(!r.valid, "limit stays the block-count 26100");
    }

    #[test]
    fn normal_cadence_verdicts_are_unchanged_by_the_block_time_term_3270() {
        // On cadence the block gap never exceeds elapsed_blocks × target, so
        // max(block-count, gap + grace) == block-count and every verdict — and
        // the limit value — matches the pre-activation rule.
        for (prev_block, elapsed_blocks) in [(21_101_u64, 1_u64), (21_101, 3), (24_990, 5), (30_000, 2)] {
            let current_block = prev_block + elapsed_blocks;
            let target = crate::consensus::proof_of_publishing::compute_target_block_time(
                current_block,
            ) as i64;
            let block_count_limit = (elapsed_blocks as i64).max(1) * target + 900;
            for block_interval in [60_i64, 300, target] {
                let ctx = BlockTimeSkewContext {
                    previous_accepting_block_unix: H21101_BLOCK_UNIX,
                    current_block_unix: H21101_BLOCK_UNIX + elapsed_blocks as i64 * block_interval,
                };
                for delta in [0, 300, block_count_limit - 1, block_count_limit, block_count_limit + 1] {
                    let pre = validate_3270(delta, prev_block, current_block, None);
                    let post = validate_3270(delta, prev_block, current_block, Some(ctx));
                    assert_eq!(
                        pre.valid, post.valid,
                        "blocks {prev_block}->{current_block}, interval {block_interval}, delta {delta}"
                    );
                    assert_eq!(pre.valid, delta <= block_count_limit);
                    if !post.valid {
                        assert!(post.errors[0].contains(&format!("limit={block_count_limit}s")));
                    }
                }
            }
        }
    }

    #[test]
    fn target_block_time_drop_at_h25000_needs_the_block_time_term_3270() {
        // At h>=25000 the target is 600 s, so the block-count allowance for the
        // same 7-block recovery shrinks to 7 × 600 + 900 = 5100 s.
        assert_eq!(crate::consensus::proof_of_publishing::compute_target_block_time(24_999), 3_600);
        assert_eq!(crate::consensus::proof_of_publishing::compute_target_block_time(25_000), 600);
        let (prev, current) = (24_998_u64, 25_005_u64);
        let pre = validate_3270(BACKLOG_DELTA, prev, current, None);
        assert_eq!(
            pre.errors,
            vec![
                "future timestamp skew too large: delta=105600s exceeds limit=5100s for 7 \
                 elapsed blocks"
                    .to_string()
            ]
        );
        assert!(validate_3270(BACKLOG_DELTA, prev, current, Some(halt_ctx())).valid);
        // On cadence at 600 s, the post-activation limit is the block-count one.
        let on_cadence = BlockTimeSkewContext {
            previous_accepting_block_unix: H21101_BLOCK_UNIX,
            current_block_unix: H21101_BLOCK_UNIX + 7 * 600,
        };
        assert!(validate_3270(5_100, prev, current, Some(on_cadence)).valid);
        assert!(!validate_3270(5_101, prev, current, Some(on_cadence)).valid);
    }
}
