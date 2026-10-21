//! Canonical observations and wider environmental-report measurements.
//!
//! Wider callers must select their report contract explicitly. The historical
//! module observation name is unavailable, preventing silent loss of direction
//! or visibility when a downstream JSON decoder migrates.
//!
//! ```
//! use wrfcoin_meteo_types::{EnvironmentalWeatherMeasurements, WeatherObservation};
//! let _node = WeatherObservation::default();
//! let report = EnvironmentalWeatherMeasurements {
//!     wind_direction_deg: Some(40.0),
//!     visibility_m: Some(80.0),
//!     ..Default::default()
//! };
//! assert_eq!(report.wind_direction_deg, Some(40.0));
//! assert_eq!(report.visibility_m, Some(80.0));
//! ```
//!
//! ```compile_fail
//! use wrfcoin_meteo_types::observation::WeatherObservation;
//! let _ = WeatherObservation::default();
//! ```

use serde::{Deserialize, Serialize};

use crate::{
    RadioSourceMetadata,
    validation::{
        EARTH_MAX_AIR_TEMPERATURE_C, EARTH_MAX_PRESSURE_HPA, EARTH_MAX_RELATIVE_HUMIDITY_PERCENT,
        EARTH_MAX_WIND_SPEED_MS, EARTH_MIN_AIR_TEMPERATURE_C, EARTH_MIN_PRESSURE_HPA,
        EARTH_MIN_RELATIVE_HUMIDITY_PERCENT, EARTH_MIN_WIND_SPEED_MS,
    },
};

/// Wider surface measurements used by environmental reports.
///
/// This preserves the existing report layout, including direction and visibility.
/// Use [`crate::WeatherObservation`] for node/parser observations; these are distinct wire contracts.
///
/// All fields are optional — sensors may not report every variable.
/// Units follow SI / WMO conventions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct EnvironmentalWeatherMeasurements {
    /// Air temperature in degrees Celsius.
    pub temperature_c: Option<f64>,
    /// Relative humidity as a percentage [0, 100].
    pub humidity_percent: Option<f64>,
    /// Atmospheric pressure in hectopascals (hPa / mbar).
    pub pressure_hpa: Option<f64>,
    /// Wind speed in meters per second.
    pub wind_speed_ms: Option<f64>,
    /// Wind direction in degrees [0, 360). 0 = North, 90 = East.
    pub wind_direction_deg: Option<f64>,
    /// Precipitation accumulation in millimeters.
    pub precipitation_mm: Option<f64>,
    /// Dewpoint temperature in degrees Celsius.
    pub dewpoint_c: Option<f64>,
    /// Horizontal visibility in meters.
    pub visibility_m: Option<f64>,
    /// Optional radio-layer provenance for observations sourced from SDR, Zigbee, or LoRa.
    // TypeScript export only: serde(default) already makes the key optional on
    // the wire, so ts-rs must emit `radio_metadata?: RadioSourceMetadata | null`.
    // The attribute is compiled only with the `ts` feature; serde is unchanged.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub radio_metadata: Option<RadioSourceMetadata>,
}

impl EnvironmentalWeatherMeasurements {
    /// Returns `true` if all present values fall within physically plausible ranges.
    ///
    /// Ranges based on observed Earth records:
    /// - Temperature: -89.2 degrees C (Vostok) to 56.7 degrees C (Death Valley)
    /// - Humidity: 0-100%
    /// - Pressure: 870-1084 hPa (observed extremes)
    /// - Wind speed: 0-113 m/s (strongest recorded gust)
    /// - Wind direction: 0-360 degrees
    /// - Precipitation: 0-500 mm (per observation period)
    /// - Dewpoint: -80 degrees C to 35 degrees C
    /// - Visibility: 0-100,000 m
    #[must_use]
    pub fn is_physically_plausible(&self) -> bool {
        let checks = [
            self.temperature_c
                .map(|v| (EARTH_MIN_AIR_TEMPERATURE_C..=EARTH_MAX_AIR_TEMPERATURE_C).contains(&v)),
            self.humidity_percent.map(|v| {
                (EARTH_MIN_RELATIVE_HUMIDITY_PERCENT..=EARTH_MAX_RELATIVE_HUMIDITY_PERCENT)
                    .contains(&v)
            }),
            self.pressure_hpa
                .map(|v| (EARTH_MIN_PRESSURE_HPA..=EARTH_MAX_PRESSURE_HPA).contains(&v)),
            self.wind_speed_ms
                .map(|v| (EARTH_MIN_WIND_SPEED_MS..=EARTH_MAX_WIND_SPEED_MS).contains(&v)),
            self.wind_direction_deg.map(|v| (0.0..360.0).contains(&v)),
            self.precipitation_mm.map(|v| (0.0..=500.0).contains(&v)),
            self.dewpoint_c.map(|v| (-80.0..=35.0).contains(&v)),
            self.visibility_m.map(|v| (0.0..=100_000.0).contains(&v)),
        ];
        checks.iter().all(|c| c.unwrap_or(true))
    }

    /// Count how many variables are present (non-None).
    pub fn variable_count(&self) -> usize {
        [
            self.temperature_c.is_some(),
            self.humidity_percent.is_some(),
            self.pressure_hpa.is_some(),
            self.wind_speed_ms.is_some(),
            self.wind_direction_deg.is_some(),
            self.precipitation_mm.is_some(),
            self.dewpoint_c.is_some(),
            self.visibility_m.is_some(),
        ]
        .iter()
        .filter(|&&v| v)
        .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        let obs = EnvironmentalWeatherMeasurements::default();
        assert_eq!(obs.variable_count(), 0);
        assert!(obs.is_physically_plausible());
    }

    #[test]
    fn plausible_observation() {
        let obs = EnvironmentalWeatherMeasurements {
            temperature_c: Some(22.5),
            humidity_percent: Some(65.0),
            pressure_hpa: Some(1013.25),
            wind_speed_ms: Some(5.0),
            wind_direction_deg: Some(180.0),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(15.0),
            visibility_m: Some(10_000.0),
            radio_metadata: None,
        };
        assert!(obs.is_physically_plausible());
        assert_eq!(obs.variable_count(), 8);
    }

    #[test]
    fn implausible_temperature() {
        let obs = EnvironmentalWeatherMeasurements {
            temperature_c: Some(100.0), // impossible
            ..Default::default()
        };
        assert!(!obs.is_physically_plausible());
    }

    #[test]
    fn partial_observation() {
        let obs = EnvironmentalWeatherMeasurements {
            temperature_c: Some(20.0),
            pressure_hpa: Some(1013.0),
            ..Default::default()
        };
        assert_eq!(obs.variable_count(), 2);
        assert!(obs.is_physically_plausible());
    }

    #[test]
    fn serde_roundtrip() {
        let obs = EnvironmentalWeatherMeasurements {
            temperature_c: Some(22.5),
            humidity_percent: Some(65.0),
            pressure_hpa: Some(1013.25),
            wind_speed_ms: Some(5.0),
            wind_direction_deg: Some(180.0),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(15.0),
            visibility_m: Some(10_000.0),
            radio_metadata: Some(crate::RadioSourceMetadata {
                source_type: crate::RadioSourceType::Zigbee,
                frequency_mhz: Some(2405.0),
                signal_strength_dbm: Some(-61.0),
                protocol: Some("Zigbee Cluster Library".to_string()),
                raw_packet_hash: Some("abcdef0123456789".to_string()),
                device_id: None,
            }),
        };
        let json = serde_json::to_string(&obs).unwrap();
        let deserialized: EnvironmentalWeatherMeasurements = serde_json::from_str(&json).unwrap();
        assert_eq!(obs, deserialized);
    }

    #[test]
    fn serde_deserialize_without_radio_metadata_defaults_to_none() {
        let obs: EnvironmentalWeatherMeasurements = serde_json::from_value(serde_json::json!({
            "temperature_c": 22.5,
            "humidity_percent": 65.0,
            "pressure_hpa": 1013.25,
            "wind_speed_ms": 5.0,
            "wind_direction_deg": 180.0,
            "precipitation_mm": 0.0,
            "dewpoint_c": 15.0,
            "visibility_m": 10_000.0
        }))
        .unwrap();

        assert!(obs.radio_metadata.is_none());
    }

    /// The TypeScript export must mirror serde(default): the key may be absent.
    /// backend's committed binding (shared/generated-types) uses this form.
    #[cfg(feature = "ts")]
    #[test]
    fn ts_export_marks_radio_metadata_optional_and_nullable() {
        use ts_rs::TS;
        let decl = EnvironmentalWeatherMeasurements::decl();
        assert!(
            decl.contains("radio_metadata?: RadioSourceMetadata | null"),
            "unexpected ts-rs declaration: {decl}"
        );
    }
}
