#![deny(missing_docs)]

//! Shared meteorological data types for WRFCoin and external weather tooling.

pub mod domain;
pub mod geo;
pub mod iot_accuracy;
pub mod observation;
pub mod quality;
pub mod validation;

pub use domain::{
    AirQualityPayload, EnvironmentalDomain, EnvironmentalReport, HydrologyPayload, OceanPayload,
    ProvenanceChain, ProvenanceEntry, ReportPayload, SoilPayload, WaterQualityPayload,
    WeatherPayload, WildfirePayload,
};
pub use geo::GeoLocation;
pub use iot_accuracy::{
    IoTAccuracyError, SensorAccuracyBand, SensorAccuracyMetrics, SensorReferencePair,
};
pub use observation::EnvironmentalWeatherMeasurements;
pub use quality::DataQualityError;
pub use validation::{
    DEFAULT_MAX_TEMPERATURE_RATE_C_PER_MIN, EARTH_MAX_AIR_TEMPERATURE_C, EARTH_MAX_PRESSURE_HPA,
    EARTH_MAX_RELATIVE_HUMIDITY_PERCENT, EARTH_MAX_WIND_SPEED_MS, EARTH_MIN_AIR_TEMPERATURE_C,
    EARTH_MIN_PRESSURE_HPA, EARTH_MIN_RELATIVE_HUMIDITY_PERCENT, EARTH_MIN_WIND_SPEED_MS,
    PhysicsRangePolicy, PhysicsViolation, ValidationReading,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Canonical schema identifier for weather observation payload version 1.
pub const WEATHER_SCHEMA_V1: &str = "wrf.weather.observation.v1";

/// Error returned when constructing a [`DataQualityScore`] from a scalar value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Error)]
pub enum DataQualityScoreError {
    /// The supplied value was not finite.
    #[error("data quality score must be finite")]
    NotFinite,
    /// The supplied value was outside the inclusive range `[0.0, 1.0]`.
    #[error("data quality score must be in [0.0, 1.0], got {0}")]
    OutOfRange(f64),
}

// `DataQualityScore` and `DataQualityBand` are defined (`DataQualityError` was
// already re-exported above)
// once, in [`quality`], and re-exported here so every existing
// `wrfcoin_meteo_types::DataQualityScore` import keeps working (#3207).
// Before #3207 this file carried a second, field-identical `DataQualityScore`
// and `DataQualityBand` next to the `quality.rs` pair; `quality.rs` was the
// dead copy. The scalar `TryFrom<f64>` constructor below is kept as the
// documented "only a blended score is known" path.
pub use quality::{DataQualityBand, DataQualityScore};

/// Build a [`DataQualityScore`] from a single blended value.
///
/// Every component is set to the same value: this is the honest
/// representation when only `overall` survived (e.g. a persisted
/// `data_quality_overall` metric), and it is exactly what the three
/// scalar call sites (`blockchain::state`, `consensus::proof_of_publishing`,
/// launch_api `transition_extractor`) reconstruct by hand. Producers that
/// actually measure range and consistency separately —
/// `weather::validation::validate_weather_data` — use
/// [`DataQualityScore::new`] / a struct literal instead.
impl TryFrom<f64> for DataQualityScore {
    type Error = DataQualityScoreError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if !value.is_finite() {
            return Err(DataQualityScoreError::NotFinite);
        }
        if !(0.0..=1.0).contains(&value) {
            return Err(DataQualityScoreError::OutOfRange(value));
        }
        Ok(Self {
            overall: value,
            range_score: value,
            consistency_score: value,
        })
    }
}

/// Derived policy action for a data-quality score.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DataQualityEvaluation {
    /// Assigned quality band.
    pub band: DataQualityBand,
    /// Scoring multiplier applied during consensus scoring.
    pub scoring_multiplier: f64,
    /// Reward multiplier applied during emissions.
    pub reward_multiplier: f64,
    /// Whether the observation is warning-only.
    pub warning_only: bool,
    /// Whether the observation should trigger a challenge path.
    pub trigger_challenge: bool,
    /// Whether the observation increments the low-quality slash streak.
    pub counts_toward_low_quality_streak: bool,
}

/// Governance-controlled thresholds for weather data quality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataQualityPolicy {
    /// Score at or above this threshold is high quality.
    pub high_quality_threshold: f64,
    /// Minimum accepted score for non-warning contributions.
    pub min_acceptance_threshold: f64,
    /// Minimum score for warning-only treatment.
    pub warning_only_threshold: f64,
    /// Scoring multiplier for borderline observations.
    pub borderline_scoring_multiplier: f64,
    /// Reward multiplier for borderline observations.
    pub borderline_reward_multiplier: f64,
    /// Scoring multiplier for warning-only observations.
    pub warning_only_scoring_multiplier: f64,
    /// Reward multiplier for warning-only observations.
    pub warning_only_reward_multiplier: f64,
    /// Scoring multiplier for low-quality observations.
    pub low_quality_scoring_multiplier: f64,
    /// Reward multiplier for low-quality observations.
    pub low_quality_reward_multiplier: f64,
    /// Consecutive low-quality submissions required before slashing.
    pub low_quality_slash_streak: u32,
    /// Slash amount applied once the low-quality streak threshold is met.
    pub low_quality_slash_amount: u128,
    /// Block height at which low-quality enforcement becomes active.
    #[serde(default)]
    pub quality_enforcement_activation_height: u64,
}

impl Default for DataQualityPolicy {
    fn default() -> Self {
        Self {
            high_quality_threshold: 0.90,
            min_acceptance_threshold: 0.70,
            warning_only_threshold: 0.50,
            borderline_scoring_multiplier: 0.70,
            borderline_reward_multiplier: 0.50,
            warning_only_scoring_multiplier: 0.0,
            warning_only_reward_multiplier: 0.0,
            low_quality_scoring_multiplier: 0.0,
            low_quality_reward_multiplier: 0.0,
            low_quality_slash_streak: 3,
            low_quality_slash_amount: 15,
            quality_enforcement_activation_height: 0,
        }
    }
}

impl DataQualityPolicy {
    /// Validates threshold ordering and multiplier ranges.
    pub fn validate(&self) -> Result<(), String> {
        fn validate_unit_interval(name: &str, value: f64) -> Result<(), String> {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(format!("{name} must be a finite value in [0, 1]"));
            }
            Ok(())
        }

        validate_unit_interval(
            "quality.high_quality_threshold",
            self.high_quality_threshold,
        )?;
        validate_unit_interval(
            "quality.min_acceptance_threshold",
            self.min_acceptance_threshold,
        )?;
        validate_unit_interval(
            "quality.warning_only_threshold",
            self.warning_only_threshold,
        )?;
        validate_unit_interval(
            "quality.borderline_scoring_multiplier",
            self.borderline_scoring_multiplier,
        )?;
        validate_unit_interval(
            "quality.borderline_reward_multiplier",
            self.borderline_reward_multiplier,
        )?;
        validate_unit_interval(
            "quality.warning_only_scoring_multiplier",
            self.warning_only_scoring_multiplier,
        )?;
        validate_unit_interval(
            "quality.warning_only_reward_multiplier",
            self.warning_only_reward_multiplier,
        )?;
        validate_unit_interval(
            "quality.low_quality_scoring_multiplier",
            self.low_quality_scoring_multiplier,
        )?;
        validate_unit_interval(
            "quality.low_quality_reward_multiplier",
            self.low_quality_reward_multiplier,
        )?;

        if self.high_quality_threshold < self.min_acceptance_threshold {
            return Err(
                "quality.high_quality_threshold must be >= quality.min_acceptance_threshold"
                    .to_string(),
            );
        }
        if self.min_acceptance_threshold < self.warning_only_threshold {
            return Err(
                "quality.min_acceptance_threshold must be >= quality.warning_only_threshold"
                    .to_string(),
            );
        }
        if self.low_quality_slash_streak == 0 {
            return Err("quality.low_quality_slash_streak must be >= 1".to_string());
        }
        if self.low_quality_slash_amount == 0 {
            return Err("quality.low_quality_slash_amount must be >= 1".to_string());
        }

        Ok(())
    }

    /// Classifies a score according to the configured thresholds.
    pub fn evaluate(&self, score: Option<&DataQualityScore>) -> DataQualityEvaluation {
        let overall = score.map(|s| s.overall.clamp(0.0, 1.0));
        let band = match overall {
            Some(value) if value >= self.high_quality_threshold => DataQualityBand::High,
            Some(value) if value >= self.min_acceptance_threshold => DataQualityBand::Borderline,
            Some(value) if value >= self.warning_only_threshold => DataQualityBand::WarningOnly,
            Some(_) => DataQualityBand::Low,
            None => DataQualityBand::Borderline,
        };

        match band {
            DataQualityBand::High => DataQualityEvaluation {
                band,
                scoring_multiplier: 1.0,
                reward_multiplier: 1.0,
                warning_only: false,
                trigger_challenge: false,
                counts_toward_low_quality_streak: false,
            },
            DataQualityBand::Borderline => DataQualityEvaluation {
                band,
                scoring_multiplier: self.borderline_scoring_multiplier,
                reward_multiplier: self.borderline_reward_multiplier,
                warning_only: false,
                trigger_challenge: false,
                counts_toward_low_quality_streak: false,
            },
            DataQualityBand::WarningOnly => DataQualityEvaluation {
                band,
                scoring_multiplier: self.warning_only_scoring_multiplier,
                reward_multiplier: self.warning_only_reward_multiplier,
                warning_only: true,
                trigger_challenge: true,
                counts_toward_low_quality_streak: false,
            },
            DataQualityBand::Low => DataQualityEvaluation {
                band,
                scoring_multiplier: self.low_quality_scoring_multiplier,
                reward_multiplier: self.low_quality_reward_multiplier,
                warning_only: false,
                trigger_challenge: true,
                counts_toward_low_quality_streak: true,
            },
        }
    }
}

/// Result of weather observation validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherValidationResult {
    /// Whether validation completed without hard errors.
    pub valid: bool,
    /// Composite data-quality score derived from the observation.
    pub score: DataQualityScore,
    /// Hard validation errors.
    pub errors: Vec<String>,
    /// Soft validation warnings.
    pub warnings: Vec<String>,
}

/// Radio transport that delivered a weather observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub enum RadioSourceType {
    /// Software-defined radio reception of sub-GHz weather broadcasts.
    #[default]
    Sdr,
    /// Satellite weather reception such as NOAA APT downlinks.
    Satellite,
    /// Zigbee 802.15.4 mesh sensor transport.
    Zigbee,
    /// LoRa long-range low-power radio transport.
    LoRa,
}

/// Radio-layer provenance for a weather observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct RadioSourceMetadata {
    /// Radio transport that delivered this observation.
    pub source_type: RadioSourceType,
    /// Center frequency in megahertz used by the received packet or channel.
    pub frequency_mhz: Option<f64>,
    /// Received signal strength in dBm at the collector.
    pub signal_strength_dbm: Option<f64>,
    /// Decoded over-the-air protocol name such as Davis ISS or Oregon Scientific v3.
    pub protocol: Option<String>,
    /// Hex-encoded hash of the raw packet payload used for deduplication.
    pub raw_packet_hash: Option<String>,
    /// Receiver device identifier (e.g. RTL-SDR serial number) for provenance tracking.
    // TypeScript export only: serde(default) already makes the key optional on
    // the wire, so ts-rs must emit `device_id?: string | null`. The attribute
    // is compiled only with the `ts` feature; serde is unchanged.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub device_id: Option<String>,
}

/// Canonical weather observation fields used by WRFCoin weather flows.
///
/// Use this crate-root type for node/parser observations.
/// Wider reports use [`EnvironmentalWeatherMeasurements`] without changing this layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct WeatherObservation {
    /// Air temperature in degrees Celsius.
    pub temperature_c: Option<f64>,
    /// Relative humidity percentage.
    pub humidity_percent: Option<f64>,
    /// Atmospheric pressure in hectopascals.
    pub pressure_hpa: Option<f64>,
    /// Wind speed in meters per second.
    pub wind_speed_ms: Option<f64>,
    /// Precipitation amount in millimeters.
    pub precipitation_mm: Option<f64>,
    /// Dewpoint in degrees Celsius.
    pub dewpoint_c: Option<f64>,
    /// Optional radio-layer provenance for observations sourced from SDR, Zigbee, or LoRa.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub radio_metadata: Option<RadioSourceMetadata>,
}

/// Key-compatibility mode used when parsing weather observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeatherFieldKeyMode {
    /// Accept legacy aliases and canonical keys.
    LegacyCompatible,
    /// Accept only canonical version 1 keys.
    CanonicalV1Only,
}

/// Observation plus temporal context used for continuity checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherObservationPoint {
    /// Observation timestamp as a Unix epoch in seconds.
    pub observed_at_unix: i64,
    /// Block height at which the observation was accepted.
    pub observed_at_block: u64,
    /// Parsed weather observation.
    pub observation: WeatherObservation,
}

/// Temporal continuity policy for comparing consecutive weather observations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherValidationPolicy {
    /// Maximum expected wall-clock seconds represented by one block.
    pub max_seconds_per_block: i64,
    /// Additional permitted future skew in seconds.
    pub max_future_skew_seconds: i64,
    /// Maximum permitted temperature delta per second.
    pub max_temp_delta_per_second: f64,
    /// Maximum permitted humidity delta per second.
    pub max_humidity_delta_per_second: f64,
    /// Maximum permitted pressure delta per second.
    pub max_pressure_delta_per_second: f64,
    /// Maximum permitted wind-speed delta per second.
    pub max_wind_delta_per_second: f64,
    /// Maximum permitted precipitation delta per second.
    pub max_precip_delta_per_second: f64,
    /// Maximum permitted dewpoint delta per second.
    pub max_dewpoint_delta_per_second: f64,
}

impl Default for WeatherValidationPolicy {
    fn default() -> Self {
        Self {
            max_seconds_per_block: 14_400,
            max_future_skew_seconds: 900,
            max_temp_delta_per_second: 0.10,
            max_humidity_delta_per_second: 1.50,
            max_pressure_delta_per_second: 0.05,
            max_wind_delta_per_second: 1.50,
            max_precip_delta_per_second: 1.00,
            max_dewpoint_delta_per_second: 0.08,
        }
    }
}

/// Static key alias groups used by weather JSON parsing.
type WeatherFieldAliases = (
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
);

impl WeatherObservation {
    /// Parses a weather observation from a JSON object using legacy-compatible keys.
    pub fn from_json(data: &serde_json::Value) -> Option<Self> {
        Self::from_json_with_key_mode(data, WeatherFieldKeyMode::LegacyCompatible)
    }

    /// Parses a weather observation from a JSON object using the requested key mode.
    pub fn from_json_with_key_mode(
        data: &serde_json::Value,
        key_mode: WeatherFieldKeyMode,
    ) -> Option<Self> {
        let object = data.as_object()?;
        let (
            temperature_keys,
            humidity_keys,
            pressure_keys,
            wind_keys,
            precipitation_keys,
            dewpoint_keys,
        ): WeatherFieldAliases = match key_mode {
            WeatherFieldKeyMode::LegacyCompatible => (
                &[
                    "temperature",
                    "temp_c",
                    "temperature_c",
                    "temperature_celsius",
                ],
                &["humidity", "relative_humidity"],
                &["pressure", "pressure_hpa"],
                &["windSpeed", "wind_speed", "wind_speed_ms"],
                &["precipitation", "rainfall", "precip_mm", "precipitation_mm"],
                &["dewpoint", "dew_point", "dewpoint_c"],
            ),
            WeatherFieldKeyMode::CanonicalV1Only => (
                &["temperature_c"],
                &["humidity"],
                &["pressure_hpa"],
                &["wind_speed_ms"],
                &["precipitation"],
                &["dewpoint_c"],
            ),
        };

        let temperature_c = first_number(object, temperature_keys);
        let humidity_percent = first_number(object, humidity_keys);
        let pressure_hpa = first_number(object, pressure_keys);
        let wind_speed_ms = first_number(object, wind_keys);
        let precipitation_mm = first_number(object, precipitation_keys);
        let dewpoint_c = first_number(object, dewpoint_keys);

        let has_any_weather_signal = temperature_c.is_some()
            || humidity_percent.is_some()
            || pressure_hpa.is_some()
            || wind_speed_ms.is_some()
            || precipitation_mm.is_some()
            || dewpoint_c.is_some();
        if !has_any_weather_signal {
            return None;
        }

        Some(Self {
            temperature_c,
            humidity_percent,
            pressure_hpa,
            wind_speed_ms,
            precipitation_mm,
            dewpoint_c,
            radio_metadata: None,
        })
    }
}

/// Schema marker carried by weather payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeatherPayloadSchemaVersion {
    /// Legacy payload without an explicit schema marker.
    LegacyUnversioned,
    /// Canonical weather schema version 1.
    V1,
}

/// Parsed weather payload metadata and observation content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedWeatherPayload {
    /// Schema variant used for the payload.
    pub version: WeatherPayloadSchemaVersion,
    /// Parsed observation body.
    pub observation: WeatherObservation,
}

/// Options controlling weather payload parsing behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WeatherPayloadParseOptions {
    /// When true, only canonical V1 observation keys are accepted.
    pub strict_v1_observation_keys: bool,
}

impl WeatherPayloadParseOptions {
    /// Returns parser options that require canonical V1 keys.
    pub const fn strict_v1_keys() -> Self {
        Self {
            strict_v1_observation_keys: true,
        }
    }
}

/// Classification produced by weather payload inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeatherPayloadClassification {
    /// Payload does not describe weather data.
    NonWeather,
    /// Payload matches canonical weather schema version 1.
    CanonicalV1,
    /// Payload uses the legacy unversioned weather shape.
    LegacyUnversioned,
    /// Payload advertises an unsupported schema marker.
    UnsupportedSchemaVersion,
    /// Payload contains a malformed schema marker type.
    InvalidSchemaVersionMarkerType,
    /// Payload claims to be canonical weather data but fails parsing.
    InvalidCanonicalPayload,
}

fn first_number(object: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(serde_json::Value::as_f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_quality_score_try_from_accepts_boundaries() {
        let zero = DataQualityScore::try_from(0.0).expect("0.0 should be valid");
        let one = DataQualityScore::try_from(1.0).expect("1.0 should be valid");

        assert_eq!(zero.overall, 0.0);
        assert_eq!(one.consistency_score, 1.0);
    }

    #[test]
    fn data_quality_score_try_from_rejects_out_of_range() {
        let err = DataQualityScore::try_from(1.2).expect_err("value above 1.0 must fail");
        assert_eq!(err, DataQualityScoreError::OutOfRange(1.2));
    }

    #[test]
    fn data_quality_score_try_from_rejects_nan() {
        let err = DataQualityScore::try_from(f64::NAN).expect_err("NaN must fail");
        assert_eq!(err, DataQualityScoreError::NotFinite);
    }

    #[test]
    fn data_quality_band_display_is_human_readable() {
        assert_eq!(DataQualityBand::High.to_string(), "High");
        assert_eq!(DataQualityBand::Low.to_string(), "Low");
    }

    #[test]
    fn weather_observation_roundtrips_through_serde() {
        let observation = WeatherObservation {
            temperature_c: Some(21.5),
            humidity_percent: Some(55.0),
            pressure_hpa: Some(1008.2),
            wind_speed_ms: Some(3.2),
            precipitation_mm: Some(0.4),
            dewpoint_c: Some(12.0),
            radio_metadata: Some(RadioSourceMetadata {
                source_type: RadioSourceType::Sdr,
                frequency_mhz: Some(433.92),
                signal_strength_dbm: Some(-72.5),
                protocol: Some("Acurite 5n1".to_string()),
                raw_packet_hash: Some("0123456789abcdef".to_string()),
                device_id: Some("rtlsdr-00000001".to_string()),
            }),
        };

        let json = serde_json::to_string(&observation).expect("serialize observation");
        let recovered: WeatherObservation =
            serde_json::from_str(&json).expect("deserialize observation");

        assert_eq!(recovered, observation);
    }

    #[test]
    fn radio_source_type_satellite_serde_is_explicit_and_strict() {
        let encoded = serde_json::to_value(RadioSourceType::Satellite).expect("serialize");
        assert_eq!(encoded, serde_json::json!("Satellite"));

        let decoded: RadioSourceType =
            serde_json::from_value(encoded).expect("deserialize Satellite variant");
        assert_eq!(decoded, RadioSourceType::Satellite);

        let unknown = serde_json::from_value::<RadioSourceType>(serde_json::json!("Orbital"));
        assert!(
            unknown.is_err(),
            "unknown radio source variants must fail fast"
        );
    }

    #[test]
    fn weather_observation_legacy_json_without_radio_metadata_still_deserializes() {
        let observation: WeatherObservation = serde_json::from_value(serde_json::json!({
            "temperature_c": 21.5,
            "humidity_percent": 55.0,
            "pressure_hpa": 1008.2,
            "wind_speed_ms": 3.2,
            "precipitation_mm": 0.4,
            "dewpoint_c": 12.0
        }))
        .expect("legacy observation JSON should deserialize");

        assert_eq!(observation.temperature_c, Some(21.5));
        assert!(observation.radio_metadata.is_none());
    }

    #[test]
    fn weather_observation_parses_legacy_json_keys() {
        let observation = WeatherObservation::from_json(&serde_json::json!({
            "temperature": 21.5,
            "humidity": 55.0,
            "pressure": 1008.2,
            "windSpeed": 3.2,
            "rainfall": 0.4,
            "dew_point": 12.0
        }))
        .expect("legacy payload should parse");

        assert_eq!(observation.temperature_c, Some(21.5));
        assert_eq!(observation.precipitation_mm, Some(0.4));
        assert_eq!(observation.dewpoint_c, Some(12.0));
    }

    #[test]
    fn weather_observation_strict_parser_rejects_legacy_keys() {
        let observation = WeatherObservation::from_json_with_key_mode(
            &serde_json::json!({
                "temperature": 21.5,
                "relative_humidity": 55.0
            }),
            WeatherFieldKeyMode::CanonicalV1Only,
        );

        assert_eq!(observation, None);
    }

    #[test]
    fn weather_validation_policy_default_accommodates_genesis_block_time() {
        let policy = WeatherValidationPolicy::default();
        assert!(
            policy.max_seconds_per_block >= 14_400,
            "default weather policy must allow the 4-hour genesis block interval"
        );
    }

    #[test]
    fn data_quality_policy_classifies_warning_band() {
        let policy = DataQualityPolicy::default();
        let score = DataQualityScore::try_from(0.55).expect("score should be valid");
        let evaluation = policy.evaluate(Some(&score));

        assert_eq!(evaluation.band, DataQualityBand::WarningOnly);
        assert!(evaluation.trigger_challenge);
        assert!(evaluation.warning_only);
    }

    /// The TypeScript export must mirror serde(default): the key may be absent.
    /// backend's generated binding relies on this optional form.
    #[cfg(feature = "ts")]
    #[test]
    fn ts_export_marks_device_id_optional_and_nullable() {
        use ts_rs::TS;
        let decl = RadioSourceMetadata::decl();
        assert!(
            decl.contains("device_id?: string | null"),
            "unexpected ts-rs declaration: {decl}"
        );
    }
}

#[cfg(test)]
mod canonical_observation_tests;
