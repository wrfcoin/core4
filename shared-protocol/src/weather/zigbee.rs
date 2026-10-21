//! Zigbee sensor payload types and validation for coordinator-aggregated
//! environmental data.
//!
//! A `ZigbeeSensorPayload` captures the context of a Zigbee device report as
//! received via Zigbee2MQTT: device identity, Zigbee Home Automation (HA)
//! cluster info, link quality, and the decoded environmental readings.
//!
//! After validation, the payload is mapped to a `WeatherObservation` with
//! `RadioSourceType::Zigbee` provenance and submitted through the standard
//! commit-reveal pipeline.

use serde::{Deserialize, Serialize};

use super::validation::{self, WeatherObservation};
use wrfcoin_meteo_types::{RadioSourceMetadata, RadioSourceType};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Zigbee 2.4 GHz ISM band (MHz).  IEEE 802.15.4 channels 11–26.
const ZIGBEE_BAND_MHZ: (f64, f64) = (2400.0, 2483.5);

/// Zigbee link quality indicator range (0–255, per ZCL spec).
const MAX_LINK_QUALITY: u8 = 255;

// ---------------------------------------------------------------------------
// Known Zigbee HA cluster IDs
// ---------------------------------------------------------------------------

/// Well-known Zigbee Home Automation / ZCL cluster IDs relevant to
/// environmental monitoring.  Used by `validate_zigbee_payload` to gate
/// which clusters are accepted into the weather pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum ZigbeeHaCluster {
    /// Temperature Measurement (0x0402).
    TemperatureMeasurement = 0x0402,
    /// Relative Humidity Measurement (0x0405).
    RelativeHumidity = 0x0405,
    /// Pressure Measurement (0x0403).
    PressureMeasurement = 0x0403,
    /// Illuminance Measurement (0x0400).
    IlluminanceMeasurement = 0x0400,
    /// Soil Moisture (vendor-specific, mapped to 0xFC01 by convention).
    SoilMoisture = 0xFC01,
}

impl ZigbeeHaCluster {
    /// Return the cluster for a raw u16, if it's a known environmental cluster.
    pub fn from_id(id: u16) -> Option<Self> {
        match id {
            0x0402 => Some(Self::TemperatureMeasurement),
            0x0405 => Some(Self::RelativeHumidity),
            0x0403 => Some(Self::PressureMeasurement),
            0x0400 => Some(Self::IlluminanceMeasurement),
            0xFC01 => Some(Self::SoilMoisture),
            _ => None,
        }
    }

    /// All known environmental cluster IDs.
    pub fn all_known_ids() -> &'static [u16] {
        &[0x0402, 0x0405, 0x0403, 0x0400, 0xFC01]
    }
}

// ---------------------------------------------------------------------------
// Payload type
// ---------------------------------------------------------------------------

/// Zigbee sensor payload as received from Zigbee2MQTT.
///
/// Captures device identity (IEEE address), the reporting ZCL cluster and
/// endpoint, radio quality indicators, and the decoded environmental fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZigbeeSensorPayload {
    // ---- Device identity ----
    /// IEEE 802.15.4 extended address: 16-character lowercase hex (8 bytes).
    pub ieee_address: String,
    /// Zigbee2MQTT friendly name (human-readable device label).
    pub friendly_name: Option<String>,

    // ---- Zigbee cluster info ----
    /// ZCL cluster ID of the reporting attribute (e.g. 0x0402 for temperature).
    pub cluster_id: u16,
    /// Zigbee endpoint number (1–240).
    pub endpoint: u8,

    // ---- Radio quality ----
    /// RSSI at the coordinator in dBm.
    pub rssi_dbm: Option<i16>,
    /// Link Quality Indicator (0–255, higher is better).
    pub link_quality: u8,

    // ---- Decoded environmental readings ----
    /// Air temperature in degrees Celsius.
    pub temperature_c: Option<f64>,
    /// Relative humidity percentage (0–100).
    pub humidity_percent: Option<f64>,
    /// Barometric pressure in hPa.
    pub pressure_hpa: Option<f64>,
    /// Illuminance in lux.
    pub illuminance_lux: Option<f64>,
    /// Soil moisture as volumetric water content percentage (0–100).
    pub soil_moisture_percent: Option<f64>,
    /// Battery level percentage (0–100), if reported by the device.
    pub battery_percent: Option<u8>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// RSSI plausibility range for 2.4 GHz Zigbee (dBm).
const MIN_RSSI_DBM: i16 = -110;
const MAX_RSSI_DBM: i16 = 0;

/// Validate a Zigbee sensor payload.
///
/// Returns a list of hard errors.  An empty list means the payload is valid.
/// Checks IEEE address format, cluster ID, endpoint, RSSI, and link quality.
/// Weather field ranges are checked separately via `validate_weather_observation()`.
pub fn validate_zigbee_payload(payload: &ZigbeeSensorPayload) -> Vec<String> {
    let mut errors = Vec::new();

    // IEEE address: must be exactly 16 hex characters.
    if payload.ieee_address.len() != 16 {
        errors.push(format!(
            "ieee_address must be 16 hex characters, got {} characters",
            payload.ieee_address.len()
        ));
    } else if !payload.ieee_address.chars().all(|c| c.is_ascii_hexdigit()) {
        errors.push(
            "ieee_address must contain only hex characters (0-9, a-f, A-F)".to_string(),
        );
    }

    // Cluster ID: must be a known environmental HA cluster.
    if ZigbeeHaCluster::from_id(payload.cluster_id).is_none() {
        errors.push(format!(
            "cluster_id 0x{:04X} is not a known environmental Zigbee HA cluster",
            payload.cluster_id
        ));
    }

    // Endpoint: 1–240 per Zigbee spec (0 is ZDO, 241–254 reserved, 255 broadcast).
    if payload.endpoint == 0 || payload.endpoint > 240 {
        errors.push(format!(
            "endpoint must be 1–240, got {}",
            payload.endpoint
        ));
    }

    // RSSI plausibility (if provided).
    if let Some(rssi) = payload.rssi_dbm {
        if !(MIN_RSSI_DBM..=MAX_RSSI_DBM).contains(&rssi) {
            errors.push(format!(
                "rssi_dbm {} is outside plausible range [{MIN_RSSI_DBM}, {MAX_RSSI_DBM}]",
                rssi
            ));
        }
    }

    // Link quality: 0–255 is the full range; we accept all values but the
    // type already constrains this via u8.  No additional check needed.
    let _ = payload.link_quality; // satisfy clippy if nothing else uses it

    errors
}

/// Convert a validated `ZigbeeSensorPayload` into a `WeatherObservation`
/// with Zigbee radio provenance.
///
/// Only standard weather fields (temperature, humidity, pressure) are mapped.
/// Illuminance, soil moisture, and battery are Zigbee-specific and carried
/// separately in transaction metadata.
pub fn to_weather_observation(payload: &ZigbeeSensorPayload) -> WeatherObservation {
    WeatherObservation {
        temperature_c: payload.temperature_c,
        humidity_percent: payload.humidity_percent,
        pressure_hpa: payload.pressure_hpa,
        radio_metadata: Some(RadioSourceMetadata {
            source_type: RadioSourceType::Zigbee,
            frequency_mhz: Some(2405.0), // Zigbee channel 11 default
            signal_strength_dbm: payload.rssi_dbm.map(|r| r as f64),
            protocol: Some(format!(
                "ZCL cluster 0x{:04X} endpoint {}",
                payload.cluster_id, payload.endpoint
            )),
            raw_packet_hash: None,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Validate both Zigbee transport layer and the converted weather observation.
pub fn validate_full(payload: &ZigbeeSensorPayload) -> Vec<String> {
    let mut errors = validate_zigbee_payload(payload);
    let obs = to_weather_observation(payload);
    let weather_result = validation::validate_weather_observation(&obs);
    errors.extend(weather_result.errors);
    errors
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_payload() -> ZigbeeSensorPayload {
        ZigbeeSensorPayload {
            ieee_address: "00158d0001a2b3c4".to_string(),
            friendly_name: Some("living-room-sensor".to_string()),
            cluster_id: 0x0402, // Temperature
            endpoint: 1,
            rssi_dbm: Some(-65),
            link_quality: 180,
            temperature_c: Some(21.5),
            humidity_percent: Some(58.0),
            pressure_hpa: Some(1012.0),
            illuminance_lux: None,
            soil_moisture_percent: None,
            battery_percent: Some(87),
        }
    }

    // ---- Basic validation ----

    #[test]
    fn valid_payload_passes_validation() {
        let errors = validate_zigbee_payload(&valid_payload());
        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    #[test]
    fn valid_payload_passes_full_validation() {
        let errors = validate_full(&valid_payload());
        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    // ---- IEEE address ----

    #[test]
    fn invalid_ieee_address_too_short() {
        let mut p = valid_payload();
        p.ieee_address = "00158d".to_string();
        let errors = validate_zigbee_payload(&p);
        assert!(
            errors.iter().any(|e| e.contains("16 hex characters")),
            "expected length error, got: {errors:?}"
        );
    }

    #[test]
    fn invalid_ieee_address_non_hex() {
        let mut p = valid_payload();
        p.ieee_address = "gggggggggggggggg".to_string();
        let errors = validate_zigbee_payload(&p);
        assert!(
            errors.iter().any(|e| e.contains("hex characters")),
            "expected hex error, got: {errors:?}"
        );
    }

    // ---- Cluster ID ----

    #[test]
    fn all_known_clusters_pass() {
        for &id in ZigbeeHaCluster::all_known_ids() {
            let mut p = valid_payload();
            p.cluster_id = id;
            let errors = validate_zigbee_payload(&p);
            assert!(
                errors.is_empty(),
                "cluster 0x{id:04X} should be valid, got: {errors:?}"
            );
        }
    }

    #[test]
    fn unknown_cluster_rejected() {
        let mut p = valid_payload();
        p.cluster_id = 0x0006; // On/Off cluster, not environmental
        let errors = validate_zigbee_payload(&p);
        assert!(
            errors.iter().any(|e| e.contains("not a known environmental")),
            "expected cluster error, got: {errors:?}"
        );
    }

    // ---- Endpoint ----

    #[test]
    fn endpoint_zero_rejected() {
        let mut p = valid_payload();
        p.endpoint = 0;
        let errors = validate_zigbee_payload(&p);
        assert!(
            errors.iter().any(|e| e.contains("endpoint must be 1")),
            "expected endpoint error, got: {errors:?}"
        );
    }

    #[test]
    fn endpoint_241_rejected() {
        let mut p = valid_payload();
        p.endpoint = 241;
        let errors = validate_zigbee_payload(&p);
        assert!(
            errors.iter().any(|e| e.contains("endpoint must be 1")),
            "expected endpoint error, got: {errors:?}"
        );
    }

    #[test]
    fn endpoint_boundaries_pass() {
        for ep in [1u8, 240] {
            let mut p = valid_payload();
            p.endpoint = ep;
            let errors = validate_zigbee_payload(&p);
            assert!(
                errors.is_empty(),
                "endpoint {ep} should be valid, got: {errors:?}"
            );
        }
    }

    // ---- RSSI ----

    #[test]
    fn rssi_out_of_range_rejected() {
        let mut p = valid_payload();
        p.rssi_dbm = Some(-120);
        let errors = validate_zigbee_payload(&p);
        assert!(
            errors.iter().any(|e| e.contains("rssi_dbm")),
            "expected RSSI error, got: {errors:?}"
        );
    }

    #[test]
    fn rssi_none_accepted() {
        let mut p = valid_payload();
        p.rssi_dbm = None;
        let errors = validate_zigbee_payload(&p);
        assert!(errors.is_empty(), "None RSSI should be fine, got: {errors:?}");
    }

    // ---- Observation mapping ----

    #[test]
    fn to_weather_observation_maps_fields() {
        let p = valid_payload();
        let obs = to_weather_observation(&p);
        assert_eq!(obs.temperature_c, Some(21.5));
        assert_eq!(obs.humidity_percent, Some(58.0));
        assert_eq!(obs.pressure_hpa, Some(1012.0));

        let radio = obs.radio_metadata.as_ref().expect("should have radio metadata");
        assert_eq!(radio.source_type, RadioSourceType::Zigbee);
        assert_eq!(radio.frequency_mhz, Some(2405.0));
        assert_eq!(radio.signal_strength_dbm, Some(-65.0));
        assert!(radio.protocol.as_ref().unwrap().contains("0x0402"));
    }

    // ---- Serialization ----

    #[test]
    fn payload_roundtrips_through_json() {
        let p = valid_payload();
        let json = serde_json::to_string(&p).expect("serialize");
        let recovered: ZigbeeSensorPayload =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(recovered, p);
    }

    // ---- Combined validation ----

    #[test]
    fn validate_full_catches_both_zigbee_and_weather_errors() {
        let mut p = valid_payload();
        p.cluster_id = 0x0000;          // unknown cluster
        p.temperature_c = Some(200.0);  // impossible temperature
        let errors = validate_full(&p);
        assert!(
            errors.iter().any(|e| e.contains("not a known environmental")),
            "should contain Zigbee cluster error"
        );
        assert!(
            errors.iter().any(|e| e.contains("temperature")),
            "should contain weather error"
        );
    }

    // ---- Cluster enum ----

    #[test]
    fn cluster_from_id_returns_none_for_unknown() {
        assert!(ZigbeeHaCluster::from_id(0x0000).is_none());
        assert!(ZigbeeHaCluster::from_id(0xFFFF).is_none());
    }

    #[test]
    fn cluster_from_id_returns_correct_variant() {
        assert_eq!(
            ZigbeeHaCluster::from_id(0x0402),
            Some(ZigbeeHaCluster::TemperatureMeasurement)
        );
        assert_eq!(
            ZigbeeHaCluster::from_id(0x0405),
            Some(ZigbeeHaCluster::RelativeHumidity)
        );
        assert_eq!(
            ZigbeeHaCluster::from_id(0xFC01),
            Some(ZigbeeHaCluster::SoilMoisture)
        );
    }
}
