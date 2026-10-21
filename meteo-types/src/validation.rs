//! Physics validation types for meteorological data quality checking.
//!
//! These types mirror the validation logic in the WRFCoin blockchain's
//! `shared-protocol/src/weather/validation.rs` but omit blockchain-specific
//! fields (e.g., `activation_height`).

use serde::{Deserialize, Serialize};

/// Minimum plausible Earth air temperature in degrees Celsius (Vostok Station record).
pub const EARTH_MIN_AIR_TEMPERATURE_C: f64 = -89.2;
/// Maximum plausible Earth air temperature in degrees Celsius (Death Valley air temperature record).
pub const EARTH_MAX_AIR_TEMPERATURE_C: f64 = 56.7;
/// Minimum plausible sea-level barometric pressure in hPa (Typhoon Tip central pressure).
pub const EARTH_MIN_PRESSURE_HPA: f64 = 870.0;
/// Maximum plausible sea-level barometric pressure in hPa (Siberian High, rounded from 1083.8 hPa).
pub const EARTH_MAX_PRESSURE_HPA: f64 = 1084.0;
/// Minimum plausible relative humidity in percent.
pub const EARTH_MIN_RELATIVE_HUMIDITY_PERCENT: f64 = 0.0;
/// Maximum plausible relative humidity in percent.
pub const EARTH_MAX_RELATIVE_HUMIDITY_PERCENT: f64 = 100.0;
/// Minimum plausible wind speed in meters per second.
pub const EARTH_MIN_WIND_SPEED_MS: f64 = 0.0;
/// Maximum plausible wind speed in meters per second (Mount Washington gust record).
pub const EARTH_MAX_WIND_SPEED_MS: f64 = 113.2;
/// Default physically plausible temperature change rate in degrees Celsius per minute.
pub const DEFAULT_MAX_TEMPERATURE_RATE_C_PER_MIN: f64 = 5.0;

/// Configurable bounds for physics-plausibility checks on weather readings.
///
/// Default bounds are based on observed Earth records (WMO extremes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct PhysicsRangePolicy {
    /// Minimum plausible temperature in degrees Celsius (Vostok Station record).
    pub temp_min_c: f64,
    /// Maximum plausible temperature in degrees Celsius (Death Valley air temperature record).
    pub temp_max_c: f64,
    /// Minimum plausible barometric pressure in hPa (Typhoon Tip: 870 hPa).
    pub pressure_min_hpa: f64,
    /// Maximum plausible barometric pressure in hPa (Siberian High, rounded to 1084 hPa).
    pub pressure_max_hpa: f64,
    /// Minimum relative humidity in percent.
    pub humidity_min_pct: f64,
    /// Maximum relative humidity in percent.
    pub humidity_max_pct: f64,
    /// Maximum plausible wind speed in m/s (Mount Washington gust record).
    pub wind_speed_max_ms: f64,
    /// Maximum physically plausible temperature rate of change in degrees C per minute.
    pub temp_rate_max_c_per_min: f64,
}

impl Default for PhysicsRangePolicy {
    fn default() -> Self {
        Self {
            temp_min_c: EARTH_MIN_AIR_TEMPERATURE_C,
            temp_max_c: EARTH_MAX_AIR_TEMPERATURE_C,
            pressure_min_hpa: EARTH_MIN_PRESSURE_HPA,
            pressure_max_hpa: EARTH_MAX_PRESSURE_HPA,
            humidity_min_pct: EARTH_MIN_RELATIVE_HUMIDITY_PERCENT,
            humidity_max_pct: EARTH_MAX_RELATIVE_HUMIDITY_PERCENT,
            wind_speed_max_ms: EARTH_MAX_WIND_SPEED_MS,
            temp_rate_max_c_per_min: DEFAULT_MAX_TEMPERATURE_RATE_C_PER_MIN,
        }
    }
}

/// A physics violation detected during weather reading validation.
///
/// Serialized as an internally tagged union with `"type"` as the discriminator,
/// matching the TypeScript discriminated union pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub enum PhysicsViolation {
    /// Temperature value is outside the configured plausible range.
    #[serde(rename_all = "camelCase")]
    TemperatureOutOfRange {
        /// Observed temperature in degrees Celsius.
        value_c: f64,
        /// Configured minimum.
        min: f64,
        /// Configured maximum.
        max: f64,
    },
    /// Pressure value is outside the configured plausible range.
    #[serde(rename_all = "camelCase")]
    PressureOutOfRange {
        /// Observed pressure in hPa.
        value_hpa: f64,
        /// Configured minimum.
        min: f64,
        /// Configured maximum.
        max: f64,
    },
    /// Humidity value is outside the configured plausible range.
    #[serde(rename_all = "camelCase")]
    HumidityOutOfRange {
        /// Observed humidity in percent.
        value_pct: f64,
        /// Configured minimum.
        min: f64,
        /// Configured maximum.
        max: f64,
    },
    /// Wind speed exceeds the configured maximum.
    #[serde(rename_all = "camelCase")]
    WindSpeedExceedsMaximum {
        /// Observed wind speed in m/s.
        value_ms: f64,
        /// Configured maximum.
        max: f64,
    },
    /// Temperature rate of change exceeds the configured maximum.
    #[serde(rename_all = "camelCase")]
    TemperatureRateExceeded {
        /// Observed temperature delta in degrees Celsius.
        delta_c: f64,
        /// Configured limit.
        limit: f64,
    },
    /// Cross-parameter inconsistency detected.
    #[serde(rename_all = "camelCase")]
    CrossParameterInconsistency {
        /// Description of the inconsistency.
        detail: String,
    },
}

/// A minimal weather reading used as input for physics validation.
///
/// Separate from [`crate::WeatherObservation`] -- this carries only the subset of
/// fields needed for physics range-checking plus a timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct ValidationReading {
    /// Air temperature in degrees Celsius.
    pub temperature_c: Option<f64>,
    /// Atmospheric pressure in hectopascals (hPa).
    pub pressure_hpa: Option<f64>,
    /// Relative humidity as a percentage [0, 100].
    pub humidity_pct: Option<f64>,
    /// Wind speed in meters per second.
    pub wind_speed_ms: Option<f64>,
    /// Unix timestamp of the reading (seconds since epoch).
    pub timestamp_unix: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_matches_wmo_extremes() {
        let policy = PhysicsRangePolicy::default();
        assert_eq!(policy.temp_min_c, EARTH_MIN_AIR_TEMPERATURE_C);
        assert_eq!(policy.temp_max_c, EARTH_MAX_AIR_TEMPERATURE_C);
        assert_eq!(policy.pressure_min_hpa, EARTH_MIN_PRESSURE_HPA);
        assert_eq!(policy.pressure_max_hpa, EARTH_MAX_PRESSURE_HPA);
        assert_eq!(policy.humidity_min_pct, EARTH_MIN_RELATIVE_HUMIDITY_PERCENT);
        assert_eq!(policy.humidity_max_pct, EARTH_MAX_RELATIVE_HUMIDITY_PERCENT);
        assert_eq!(policy.wind_speed_max_ms, EARTH_MAX_WIND_SPEED_MS);
        assert_eq!(
            policy.temp_rate_max_c_per_min,
            DEFAULT_MAX_TEMPERATURE_RATE_C_PER_MIN
        );
    }

    #[test]
    fn policy_serde_roundtrip() {
        let policy = PhysicsRangePolicy::default();
        let json = serde_json::to_string(&policy).unwrap();
        let deserialized: PhysicsRangePolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, deserialized);
    }

    #[test]
    fn policy_serializes_camel_case() {
        let policy = PhysicsRangePolicy::default();
        let json = serde_json::to_string(&policy).unwrap();
        assert!(json.contains("tempMinC"));
        assert!(json.contains("pressureMinHpa"));
        assert!(json.contains("windSpeedMaxMs"));
        assert!(json.contains("tempRateMaxCPerMin"));
        // Should NOT contain snake_case
        assert!(!json.contains("temp_min_c"));
    }

    #[test]
    fn violation_serde_roundtrip() {
        let violations = vec![
            PhysicsViolation::TemperatureOutOfRange {
                value_c: 100.0,
                min: EARTH_MIN_AIR_TEMPERATURE_C,
                max: EARTH_MAX_AIR_TEMPERATURE_C,
            },
            PhysicsViolation::PressureOutOfRange {
                value_hpa: 500.0,
                min: EARTH_MIN_PRESSURE_HPA,
                max: EARTH_MAX_PRESSURE_HPA,
            },
            PhysicsViolation::HumidityOutOfRange {
                value_pct: 120.0,
                min: EARTH_MIN_RELATIVE_HUMIDITY_PERCENT,
                max: EARTH_MAX_RELATIVE_HUMIDITY_PERCENT,
            },
            PhysicsViolation::WindSpeedExceedsMaximum {
                value_ms: 200.0,
                max: EARTH_MAX_WIND_SPEED_MS,
            },
            PhysicsViolation::TemperatureRateExceeded {
                delta_c: 15.0,
                limit: DEFAULT_MAX_TEMPERATURE_RATE_C_PER_MIN,
            },
            PhysicsViolation::CrossParameterInconsistency {
                detail: "Dew point exceeds temperature".into(),
            },
        ];
        for v in &violations {
            let json = serde_json::to_string(v).unwrap();
            let deserialized: PhysicsViolation = serde_json::from_str(&json).unwrap();
            assert_eq!(*v, deserialized);
        }
    }

    #[test]
    fn violation_serializes_tagged_camel_case() {
        let v = PhysicsViolation::TemperatureOutOfRange {
            value_c: 100.0,
            min: EARTH_MIN_AIR_TEMPERATURE_C,
            max: EARTH_MAX_AIR_TEMPERATURE_C,
        };
        let json = serde_json::to_string(&v).unwrap();
        // Tagged with "type"
        assert!(json.contains(r#""type":"TemperatureOutOfRange""#));
        // Fields are camelCase
        assert!(json.contains("valueC"));
        assert!(!json.contains("value_c"));
    }

    #[test]
    fn reading_serde_roundtrip() {
        let reading = ValidationReading {
            temperature_c: Some(22.5),
            pressure_hpa: Some(1013.25),
            humidity_pct: Some(65.0),
            wind_speed_ms: Some(5.0),
            timestamp_unix: 1700000000.0,
        };
        let json = serde_json::to_string(&reading).unwrap();
        let deserialized: ValidationReading = serde_json::from_str(&json).unwrap();
        assert_eq!(reading, deserialized);
    }

    #[test]
    fn reading_serializes_camel_case() {
        let reading = ValidationReading {
            temperature_c: Some(22.5),
            pressure_hpa: None,
            humidity_pct: None,
            wind_speed_ms: None,
            timestamp_unix: 1000.0,
        };
        let json = serde_json::to_string(&reading).unwrap();
        assert!(json.contains("temperatureC"));
        assert!(json.contains("timestampUnix"));
        assert!(!json.contains("temperature_c"));
    }

    #[test]
    fn default_reading_is_empty() {
        let reading = ValidationReading::default();
        assert_eq!(reading.temperature_c, None);
        assert_eq!(reading.pressure_hpa, None);
        assert_eq!(reading.humidity_pct, None);
        assert_eq!(reading.wind_speed_ms, None);
        assert_eq!(reading.timestamp_unix, 0.0);
    }
}
