//! Environmental domain payloads covering 7 observational domains.
//!
//! Each domain has a typed payload struct with SI / standard units.
//! The [`ReportPayload`] enum wraps all domains for tagged serialization.

use crate::{geo::GeoLocation, observation::EnvironmentalWeatherMeasurements};
use serde::{Deserialize, Serialize};

/// Environmental observation domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
#[non_exhaustive]
pub enum EnvironmentalDomain {
    /// Weather domain.
    Weather,
    /// Air quality domain.
    AirQuality,
    /// Water quality domain.
    WaterQuality,
    /// Wildfire domain.
    Wildfire,
    /// Soil domain.
    Soil,
    /// Ocean domain.
    Ocean,
    /// Hydrology domain.
    Hydrology,
}

impl core::fmt::Display for EnvironmentalDomain {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Weather => write!(f, "Weather"),
            Self::AirQuality => write!(f, "AirQuality"),
            Self::WaterQuality => write!(f, "WaterQuality"),
            Self::Wildfire => write!(f, "Wildfire"),
            Self::Soil => write!(f, "Soil"),
            Self::Ocean => write!(f, "Ocean"),
            Self::Hydrology => write!(f, "Hydrology"),
        }
    }
}

/// Standard weather payload (surface observations).
///
/// The wider [`EnvironmentalWeatherMeasurements`] layout includes wind direction and
/// visibility. This alias retains the existing [`ReportPayload::Weather`] wire contract;
/// it is distinct from the canonical seven-field [`crate::WeatherObservation`].
pub type WeatherPayload = EnvironmentalWeatherMeasurements;

/// Air quality payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct AirQualityPayload {
    /// Air quality index.
    pub aqi: Option<u16>,
    /// PM2.5 concentration in micrograms per cubic meter.
    pub pm25_ugm3: Option<f64>,
    /// PM10 concentration in micrograms per cubic meter.
    pub pm10_ugm3: Option<f64>,
    /// Ozone concentration in parts per billion.
    pub o3_ppb: Option<f64>,
    /// Nitrogen dioxide concentration in parts per billion.
    pub no2_ppb: Option<f64>,
    /// Sulfur dioxide concentration in parts per billion.
    pub so2_ppb: Option<f64>,
    /// Carbon monoxide concentration in parts per million.
    pub co_ppm: Option<f64>,
}

/// Water quality payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct WaterQualityPayload {
    /// pH level.
    pub ph: Option<f64>,
    /// Dissolved oxygen in milligrams per liter.
    pub dissolved_oxygen_mgl: Option<f64>,
    /// Turbidity in Nephelometric Turbidity Units.
    pub turbidity_ntu: Option<f64>,
    /// Conductivity in microsiemens per meter.
    pub conductivity_usm: Option<f64>,
    /// Water temperature in degrees Celsius.
    pub temperature_c: Option<f64>,
    /// Total dissolved solids in milligrams per liter.
    pub total_dissolved_solids_mgl: Option<f64>,
}

/// Wildfire observation payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct WildfirePayload {
    /// Fire radiative power in megawatts.
    pub fire_radiative_power_mw: Option<f64>,
    /// Burn area in square kilometers.
    pub burn_area_km2: Option<f64>,
    /// Detection confidence as a percentage.
    pub confidence_pct: Option<f64>,
    /// Smoke aerosol optical depth.
    pub smoke_aod: Option<f64>,
    /// Containment percentage.
    pub containment_pct: Option<f64>,
    /// Whether the fire is currently active.
    pub active: Option<bool>,
}

/// Soil observation payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct SoilPayload {
    /// Soil moisture as a percentage.
    pub moisture_pct: Option<f64>,
    /// Soil temperature in degrees Celsius.
    pub temperature_c: Option<f64>,
    /// Soil pH level.
    pub ph: Option<f64>,
    /// Organic carbon as a percentage.
    pub organic_carbon_pct: Option<f64>,
    /// Nitrogen in milligrams per liter.
    pub nitrogen_mgl: Option<f64>,
    /// Phosphorus in milligrams per liter.
    pub phosphorus_mgl: Option<f64>,
    /// Potassium in milligrams per liter.
    pub potassium_mgl: Option<f64>,
    /// Soil conductivity in dS/m.
    pub conductivity_dsm: Option<f64>,
    /// Measurement depth in centimeters.
    pub depth_cm: Option<f64>,
}

/// Ocean observation payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct OceanPayload {
    /// Sea surface temperature in degrees Celsius.
    pub sea_surface_temperature_c: Option<f64>,
    /// Salinity in practical salinity units.
    pub salinity_psu: Option<f64>,
    /// Significant wave height in meters.
    pub wave_height_m: Option<f64>,
    /// Wave period in seconds.
    pub wave_period_s: Option<f64>,
    /// Ocean current speed in meters per second.
    pub current_speed_ms: Option<f64>,
    /// Ocean current direction in degrees.
    pub current_direction_deg: Option<f64>,
    /// Chlorophyll-a concentration in milligrams per liter.
    pub chlorophyll_a_mgl: Option<f64>,
    /// Dissolved oxygen in milligrams per liter.
    pub dissolved_oxygen_mgl: Option<f64>,
    /// Measurement depth in meters.
    pub depth_m: Option<f64>,
}

/// Hydrology observation payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct HydrologyPayload {
    /// River discharge in cubic meters per second.
    pub river_discharge_m3s: Option<f64>,
    /// Water level in meters.
    pub water_level_m: Option<f64>,
    /// Flow velocity in meters per second.
    pub flow_velocity_ms: Option<f64>,
    /// Sediment load in milligrams per liter.
    pub sediment_load_mgl: Option<f64>,
    /// Groundwater level in meters.
    pub groundwater_level_m: Option<f64>,
    /// Reservoir storage as a percentage.
    pub reservoir_storage_pct: Option<f64>,
    /// Flood risk level (0-255).
    pub flood_risk_level: Option<u8>,
    /// Drought index value.
    pub drought_index: Option<f64>,
}

/// Tagged union of all environmental domain payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
#[serde(tag = "domain", content = "data")]
#[non_exhaustive]
pub enum ReportPayload {
    /// Weather observation payload.
    Weather(WeatherPayload),
    /// Air quality observation payload.
    AirQuality(AirQualityPayload),
    /// Water quality observation payload.
    WaterQuality(WaterQualityPayload),
    /// Wildfire observation payload.
    Wildfire(WildfirePayload),
    /// Soil observation payload.
    Soil(SoilPayload),
    /// Ocean observation payload.
    Ocean(OceanPayload),
    /// Hydrology observation payload.
    Hydrology(HydrologyPayload),
}

impl ReportPayload {
    /// Return the environmental domain of this payload.
    pub fn domain(&self) -> EnvironmentalDomain {
        match self {
            Self::Weather(_) => EnvironmentalDomain::Weather,
            Self::AirQuality(_) => EnvironmentalDomain::AirQuality,
            Self::WaterQuality(_) => EnvironmentalDomain::WaterQuality,
            Self::Wildfire(_) => EnvironmentalDomain::Wildfire,
            Self::Soil(_) => EnvironmentalDomain::Soil,
            Self::Ocean(_) => EnvironmentalDomain::Ocean,
            Self::Hydrology(_) => EnvironmentalDomain::Hydrology,
        }
    }
}

/// A single provenance attestation step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct ProvenanceEntry {
    /// Attester identifier.
    pub attester_id: String,
    /// Unix timestamp of the attestation.
    pub timestamp: u64,
}

/// Lightweight provenance chain: ordered list of attestation entries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct ProvenanceChain(pub Vec<ProvenanceEntry>);

impl ProvenanceChain {
    /// Create a new provenance chain from a list of entries.
    pub fn new(entries: Vec<ProvenanceEntry>) -> Self {
        Self(entries)
    }
    /// Returns `true` if the chain contains no attestation entries.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Returns the number of attestation entries in the chain.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// A complete environmental observation report.
///
/// # Direct construction warning
///
/// When constructing via struct literal, the `domain` field must match the domain of
/// `payload` -- use `payload.domain()` to derive it. Mismatches will cause
/// [`validate()`][EnvironmentalReport::validate] to return an error.
/// Prefer [`EnvironmentalReport::new`] which auto-derives `domain` from `payload`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export, export_to = "bindings/"))]
pub struct EnvironmentalReport {
    /// Unique report identifier.
    pub report_id: String,
    /// Environmental domain -- must match `payload.domain()`.
    pub domain: EnvironmentalDomain,
    /// Station or sensor identifier.
    pub station_id: String,
    /// Geographic location of the observation.
    pub location: GeoLocation,
    /// Unix timestamp when the observation was taken.
    pub observed_at: u64,
    /// Unix timestamp when the report was submitted.
    pub submitted_at: u64,
    /// Domain-specific observation data.
    pub payload: ReportPayload,
    /// Optional quality score [0.0, 1.0].
    pub quality_score: Option<f64>,
    /// Optional lightweight provenance chain.
    #[serde(default)]
    pub provenance: Option<ProvenanceChain>,
}

impl EnvironmentalReport {
    /// Construct a validated report.
    ///
    /// `domain` is derived automatically from `payload.domain()`, preventing
    /// domain/payload mismatches at construction time.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        report_id: String,
        station_id: String,
        location: GeoLocation,
        observed_at: u64,
        submitted_at: u64,
        payload: ReportPayload,
        quality_score: Option<f64>,
        provenance: Option<ProvenanceChain>,
    ) -> Result<Self, String> {
        let domain = payload.domain();
        let report = Self {
            report_id,
            domain,
            station_id,
            location,
            observed_at,
            submitted_at,
            payload,
            quality_score,
            provenance,
        };
        report.validate()?;
        Ok(report)
    }

    /// Validate report consistency and field ranges.
    #[must_use = "validation errors must be handled"]
    pub fn validate(&self) -> Result<(), String> {
        if self.report_id.is_empty() {
            return Err("report_id must not be empty".into());
        }
        if self.station_id.is_empty() {
            return Err("station_id must not be empty".into());
        }
        if self.domain != self.payload.domain() {
            return Err("environmental report domain does not match payload domain".into());
        }
        if let Some(score) = self.quality_score {
            if !score.is_finite() || !(0.0..=1.0).contains(&score) {
                return Err("quality_score must be finite and in [0, 1]".into());
            }
        }
        Ok(())
    }

    /// Return whether report consistency checks pass.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_payload_domain() {
        let payload = ReportPayload::Weather(WeatherPayload {
            temperature_c: Some(22.0),
            humidity_percent: Some(65.0),
            pressure_hpa: Some(1013.25),
            wind_speed_ms: None,
            wind_direction_deg: None,
            precipitation_mm: None,
            dewpoint_c: Some(14.0),
            visibility_m: None,
            radio_metadata: None,
        });
        assert_eq!(payload.domain(), EnvironmentalDomain::Weather);
    }

    #[test]
    fn weather_payload_is_environmental_measurements_alias() {
        let payload: WeatherPayload = EnvironmentalWeatherMeasurements {
            temperature_c: Some(18.5),
            humidity_percent: Some(55.0),
            pressure_hpa: None,
            wind_speed_ms: None,
            wind_direction_deg: None,
            precipitation_mm: None,
            dewpoint_c: Some(9.0),
            visibility_m: None,
            radio_metadata: None,
        };
        assert!(payload.is_physically_plausible());
        assert_eq!(payload.variable_count(), 3);
    }

    #[test]
    fn report_payload_serde_roundtrip() {
        let payload = ReportPayload::AirQuality(AirQualityPayload {
            aqi: Some(42),
            pm25_ugm3: Some(12.5),
            pm10_ugm3: None,
            o3_ppb: Some(30.0),
            no2_ppb: None,
            so2_ppb: None,
            co_ppm: None,
        });
        let json = serde_json::to_string(&payload).unwrap();
        let deserialized: ReportPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(payload, deserialized);
    }

    #[test]
    fn environmental_report_construction() {
        let report = EnvironmentalReport {
            report_id: "rpt-001".into(),
            domain: EnvironmentalDomain::Ocean,
            station_id: "buoy-42".into(),
            location: GeoLocation::with_altitude(25.0, -90.0, 0.0),
            observed_at: 1709856000,
            submitted_at: 1709856060,
            payload: ReportPayload::Ocean(OceanPayload {
                sea_surface_temperature_c: Some(26.5),
                salinity_psu: Some(35.0),
                wave_height_m: Some(1.2),
                wave_period_s: Some(8.0),
                current_speed_ms: None,
                current_direction_deg: None,
                chlorophyll_a_mgl: None,
                dissolved_oxygen_mgl: None,
                depth_m: Some(0.0),
            }),
            quality_score: Some(0.92),
            provenance: None,
        };
        assert_eq!(report.domain, EnvironmentalDomain::Ocean);
        assert!(report.location.is_valid());
        assert!(report.is_valid());
    }

    #[test]
    fn environmental_report_new_auto_derives_domain() {
        let report = EnvironmentalReport::new(
            "rpt-002".into(),
            "station-1".into(),
            GeoLocation::new(40.0, -105.0),
            1709856000,
            1709856060,
            ReportPayload::Ocean(OceanPayload {
                sea_surface_temperature_c: Some(21.0),
                salinity_psu: Some(33.0),
                wave_height_m: None,
                wave_period_s: None,
                current_speed_ms: None,
                current_direction_deg: None,
                chlorophyll_a_mgl: None,
                dissolved_oxygen_mgl: None,
                depth_m: None,
            }),
            None,
            None,
        )
        .unwrap();
        assert_eq!(report.domain, EnvironmentalDomain::Ocean);
    }

    #[test]
    fn environmental_report_new_rejects_invalid_quality_score() {
        let result = EnvironmentalReport::new(
            "rpt-003".into(),
            "station-1".into(),
            GeoLocation::new(40.0, -105.0),
            1709856000,
            1709856060,
            ReportPayload::Weather(WeatherPayload {
                temperature_c: Some(18.0),
                humidity_percent: Some(50.0),
                pressure_hpa: None,
                wind_speed_ms: None,
                wind_direction_deg: None,
                precipitation_mm: None,
                dewpoint_c: None,
                visibility_m: None,
                radio_metadata: None,
            }),
            Some(1.2),
            None,
        );
        assert_eq!(
            result,
            Err("quality_score must be finite and in [0, 1]".to_string())
        );
    }

    #[test]
    fn environmental_report_new_accepts_matching_domain() {
        let report = EnvironmentalReport::new(
            "rpt-004".into(),
            "station-1".into(),
            GeoLocation::new(40.0, -105.0),
            1709856000,
            1709856060,
            ReportPayload::Weather(WeatherPayload {
                temperature_c: Some(18.0),
                humidity_percent: Some(50.0),
                pressure_hpa: None,
                wind_speed_ms: None,
                wind_direction_deg: None,
                precipitation_mm: None,
                dewpoint_c: None,
                visibility_m: None,
                radio_metadata: None,
            }),
            Some(0.95),
            None,
        )
        .unwrap();
        assert!(report.is_valid());
    }

    #[test]
    fn environmental_domain_display() {
        assert_eq!(EnvironmentalDomain::Weather.to_string(), "Weather");
        assert_eq!(EnvironmentalDomain::AirQuality.to_string(), "AirQuality");
        assert_eq!(
            EnvironmentalDomain::WaterQuality.to_string(),
            "WaterQuality"
        );
        assert_eq!(EnvironmentalDomain::Wildfire.to_string(), "Wildfire");
        assert_eq!(EnvironmentalDomain::Soil.to_string(), "Soil");
        assert_eq!(EnvironmentalDomain::Ocean.to_string(), "Ocean");
        assert_eq!(EnvironmentalDomain::Hydrology.to_string(), "Hydrology");
    }

    #[test]
    fn environmental_report_serde_roundtrip() {
        let report = EnvironmentalReport::new(
            "rpt-serde".into(),
            "station-serde".into(),
            GeoLocation::new(51.5, -0.1),
            1709856000,
            1709856060,
            ReportPayload::Weather(WeatherPayload {
                temperature_c: Some(15.0),
                humidity_percent: Some(72.0),
                pressure_hpa: Some(1008.0),
                wind_speed_ms: Some(3.5),
                wind_direction_deg: Some(270.0),
                precipitation_mm: Some(0.2),
                dewpoint_c: Some(10.0),
                visibility_m: Some(8000.0),
                radio_metadata: None,
            }),
            Some(0.88),
            None,
        )
        .unwrap();
        let json = serde_json::to_string(&report).unwrap();
        let deserialized: EnvironmentalReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, deserialized);
    }
}
