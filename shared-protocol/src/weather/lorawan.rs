//! LoRaWAN sensor payload types and validation for gateway-aggregated
//! environmental data.
//!
//! A `LoRaWanSensorPayload` captures the full context of a LoRaWAN uplink
//! event as received by ChirpStack: device identity, radio parameters
//! (frequency, spreading factor, RSSI, SNR), and the decoded weather fields.
//!
//! After validation, the payload is mapped to a `WeatherObservation` with
//! `RadioSourceType::LoRa` provenance and submitted through the standard
//! commit-reveal pipeline.
//!
//! See `docs/architecture/lorawan-gateway.md` for the full design.

use serde::{Deserialize, Serialize};

use super::validation::{self, WeatherObservation};
use wrfcoin_meteo_types::{RadioSourceMetadata, RadioSourceType};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Minimum valid LoRa spreading factor.
pub const MIN_SPREADING_FACTOR: u8 = 7;
/// Maximum valid LoRa spreading factor.
pub const MAX_SPREADING_FACTOR: u8 = 12;

/// Plausible RSSI range for LoRa reception (dBm).
/// Typical range: -120 dBm (weak) to -30 dBm (very strong / close range).
const MIN_RSSI_DBM: i16 = -140;
const MAX_RSSI_DBM: i16 = 0;

/// Plausible SNR range for LoRa (dB).
/// SF12 can decode at SNR as low as -20 dB; clean signals reach +10 dB.
const MIN_SNR_DB: f32 = -25.0;
const MAX_SNR_DB: f32 = 15.0;

/// LoRa ISM frequency bands (MHz) — matches `LORA_BANDS_MHZ` in validation.rs.
const LORA_BANDS_MHZ: &[(f64, f64)] = &[
    (433.05, 434.79), // EU433 / AS433
    (863.0, 870.0),   // EU868
    (902.0, 928.0),   // US915
];

// ---------------------------------------------------------------------------
// Payload type
// ---------------------------------------------------------------------------

/// LoRaWAN uplink sensor payload as received from ChirpStack.
///
/// This struct bridges the LoRaWAN transport layer and the WRFCoin weather
/// observation pipeline.  It captures gateway-level metadata for data quality
/// assessment and debugging, alongside the decoded weather fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoRaWanSensorPayload {
    // ---- Device identity ----
    /// Device EUI: 16-character lowercase hex string (8 bytes, IEEE EUI-64).
    pub device_eui: String,

    // ---- Radio parameters ----
    /// Uplink frequency in MHz (e.g. 868.1, 915.0).
    pub frequency_mhz: f64,
    /// LoRa spreading factor (7–12).
    pub spreading_factor: u8,
    /// Received signal strength at the gateway in dBm.
    pub rssi_dbm: i16,
    /// Signal-to-noise ratio at the gateway in dB.
    pub snr_db: f32,

    // ---- Decoded weather fields ----
    /// Air temperature in degrees Celsius.
    pub temperature_c: Option<f64>,
    /// Relative humidity percentage (0–100).
    pub humidity_percent: Option<f64>,
    /// Barometric pressure in hPa.
    pub pressure_hpa: Option<f64>,
    /// Soil temperature in degrees Celsius.
    pub soil_temperature_c: Option<f64>,
    /// Soil moisture as volumetric water content percentage (0–100).
    pub soil_moisture_percent: Option<f64>,
    /// Distance measurement in meters (e.g. water level sensor).
    pub distance_m: Option<f64>,

    // ---- Gateway metadata ----
    /// Gateway identifier (ChirpStack gateway ID or EUI).
    pub gateway_id: Option<String>,
    /// Frame counter from the end-node (monotonically increasing).
    pub frame_count: Option<u32>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate a LoRaWAN sensor payload.
///
/// Returns a list of hard errors.  An empty list indicates success.
/// Checks device EUI format, frequency band, spreading factor, RSSI, and SNR.
/// Weather field ranges are NOT checked here — those go through the standard
/// `validate_weather_observation()` pipeline after conversion.
pub fn validate_lorawan_payload(payload: &LoRaWanSensorPayload) -> Vec<String> {
    let mut errors = Vec::new();

    // Device EUI: must be exactly 16 hex characters.
    if payload.device_eui.len() != 16 {
        errors.push(format!(
            "device_eui must be 16 hex characters, got {} characters",
            payload.device_eui.len()
        ));
    } else if !payload.device_eui.chars().all(|c| c.is_ascii_hexdigit()) {
        errors.push("device_eui must contain only hex characters (0-9, a-f, A-F)".to_string());
    }

    // Frequency: must be in a known LoRa ISM band.
    if !payload.frequency_mhz.is_finite() || payload.frequency_mhz <= 0.0 {
        errors.push("frequency_mhz must be a positive finite value".to_string());
    } else if !LORA_BANDS_MHZ
        .iter()
        .any(|(min, max)| (*min..=*max).contains(&payload.frequency_mhz))
    {
        errors.push(format!(
            "frequency {:.3} MHz is not in a known LoRa ISM band",
            payload.frequency_mhz
        ));
    }

    // Spreading factor: SF7–SF12.
    if payload.spreading_factor < MIN_SPREADING_FACTOR
        || payload.spreading_factor > MAX_SPREADING_FACTOR
    {
        errors.push(format!(
            "spreading_factor must be {MIN_SPREADING_FACTOR}–{MAX_SPREADING_FACTOR}, got {}",
            payload.spreading_factor
        ));
    }

    // RSSI plausibility.
    if payload.rssi_dbm < MIN_RSSI_DBM || payload.rssi_dbm > MAX_RSSI_DBM {
        errors.push(format!(
            "rssi_dbm {} is outside plausible range [{MIN_RSSI_DBM}, {MAX_RSSI_DBM}]",
            payload.rssi_dbm
        ));
    }

    // SNR plausibility.
    if !payload.snr_db.is_finite() {
        errors.push("snr_db must be a finite value".to_string());
    } else if payload.snr_db < MIN_SNR_DB || payload.snr_db > MAX_SNR_DB {
        errors.push(format!(
            "snr_db {:.1} is outside plausible range [{MIN_SNR_DB}, {MAX_SNR_DB}]",
            payload.snr_db
        ));
    }

    errors
}

/// Convert a validated `LoRaWanSensorPayload` into a `WeatherObservation`
/// with LoRa radio provenance.
///
/// Only the standard weather fields (temperature, humidity, pressure) are
/// mapped. Soil and distance fields are LoRaWAN-specific and carried
/// separately (e.g. in transaction metadata).
pub fn to_weather_observation(payload: &LoRaWanSensorPayload) -> WeatherObservation {
    WeatherObservation {
        temperature_c: payload.temperature_c,
        humidity_percent: payload.humidity_percent,
        pressure_hpa: payload.pressure_hpa,
        radio_metadata: Some(RadioSourceMetadata {
            source_type: RadioSourceType::LoRa,
            frequency_mhz: Some(payload.frequency_mhz),
            signal_strength_dbm: Some(payload.rssi_dbm as f64),
            protocol: Some(format!("LoRaWAN SF{}", payload.spreading_factor)),
            raw_packet_hash: None,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Check whether a converted observation passes standard weather validation
/// AND the LoRaWAN-layer validation.
pub fn validate_full(payload: &LoRaWanSensorPayload) -> Vec<String> {
    let mut errors = validate_lorawan_payload(payload);
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

    fn valid_payload() -> LoRaWanSensorPayload {
        LoRaWanSensorPayload {
            device_eui: "a1b2c3d4e5f60718".to_string(),
            frequency_mhz: 868.1,
            spreading_factor: 10,
            rssi_dbm: -95,
            snr_db: 3.5,
            temperature_c: Some(22.3),
            humidity_percent: Some(61.0),
            pressure_hpa: Some(1013.0),
            soil_temperature_c: None,
            soil_moisture_percent: None,
            distance_m: None,
            gateway_id: Some("gw-001".to_string()),
            frame_count: Some(42),
        }
    }

    #[test]
    fn valid_payload_passes_validation() {
        let errors = validate_lorawan_payload(&valid_payload());
        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    #[test]
    fn invalid_device_eui_too_short() {
        let mut p = valid_payload();
        p.device_eui = "abcdef".to_string();
        let errors = validate_lorawan_payload(&p);
        assert!(
            errors.iter().any(|e| e.contains("16 hex characters")),
            "expected EUI length error, got: {errors:?}"
        );
    }

    #[test]
    fn invalid_device_eui_non_hex() {
        let mut p = valid_payload();
        p.device_eui = "zzzzzzzzzzzzzzzz".to_string();
        let errors = validate_lorawan_payload(&p);
        assert!(
            errors.iter().any(|e| e.contains("hex characters")),
            "expected hex error, got: {errors:?}"
        );
    }

    #[test]
    fn invalid_frequency_outside_lora_band() {
        let mut p = valid_payload();
        p.frequency_mhz = 2400.0; // WiFi band, not LoRa
        let errors = validate_lorawan_payload(&p);
        assert!(
            errors.iter().any(|e| e.contains("not in a known LoRa ISM band")),
            "expected frequency error, got: {errors:?}"
        );
    }

    #[test]
    fn valid_us915_frequency_passes() {
        let mut p = valid_payload();
        p.frequency_mhz = 915.0;
        let errors = validate_lorawan_payload(&p);
        assert!(errors.is_empty(), "US915 should be valid, got: {errors:?}");
    }

    #[test]
    fn valid_eu433_frequency_passes() {
        let mut p = valid_payload();
        p.frequency_mhz = 433.5;
        let errors = validate_lorawan_payload(&p);
        assert!(errors.is_empty(), "EU433 should be valid, got: {errors:?}");
    }

    #[test]
    fn invalid_spreading_factor_too_low() {
        let mut p = valid_payload();
        p.spreading_factor = 5;
        let errors = validate_lorawan_payload(&p);
        assert!(
            errors.iter().any(|e| e.contains("spreading_factor")),
            "expected SF error, got: {errors:?}"
        );
    }

    #[test]
    fn invalid_spreading_factor_too_high() {
        let mut p = valid_payload();
        p.spreading_factor = 13;
        let errors = validate_lorawan_payload(&p);
        assert!(
            errors.iter().any(|e| e.contains("spreading_factor")),
            "expected SF error, got: {errors:?}"
        );
    }

    #[test]
    fn boundary_spreading_factors_pass() {
        for sf in [7, 12] {
            let mut p = valid_payload();
            p.spreading_factor = sf;
            let errors = validate_lorawan_payload(&p);
            assert!(errors.is_empty(), "SF{sf} should be valid, got: {errors:?}");
        }
    }

    #[test]
    fn invalid_rssi_too_low() {
        let mut p = valid_payload();
        p.rssi_dbm = -150;
        let errors = validate_lorawan_payload(&p);
        assert!(
            errors.iter().any(|e| e.contains("rssi_dbm")),
            "expected RSSI error, got: {errors:?}"
        );
    }

    #[test]
    fn invalid_snr_out_of_range() {
        let mut p = valid_payload();
        p.snr_db = 20.0;
        let errors = validate_lorawan_payload(&p);
        assert!(
            errors.iter().any(|e| e.contains("snr_db")),
            "expected SNR error, got: {errors:?}"
        );
    }

    #[test]
    fn to_weather_observation_maps_fields() {
        let p = valid_payload();
        let obs = to_weather_observation(&p);
        assert_eq!(obs.temperature_c, Some(22.3));
        assert_eq!(obs.humidity_percent, Some(61.0));
        assert_eq!(obs.pressure_hpa, Some(1013.0));

        let radio = obs.radio_metadata.as_ref().expect("should have radio metadata");
        assert_eq!(radio.source_type, RadioSourceType::LoRa);
        assert_eq!(radio.frequency_mhz, Some(868.1));
        assert_eq!(radio.signal_strength_dbm, Some(-95.0));
        assert!(radio.protocol.as_ref().unwrap().contains("SF10"));
    }

    #[test]
    fn to_weather_observation_passes_full_validation() {
        let p = valid_payload();
        let errors = validate_full(&p);
        assert!(
            errors.is_empty(),
            "full validation should pass for valid payload, got: {errors:?}"
        );
    }

    #[test]
    fn payload_roundtrips_through_json() {
        let p = valid_payload();
        let json = serde_json::to_string(&p).expect("serialize");
        let recovered: LoRaWanSensorPayload =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(recovered, p);
    }

    #[test]
    fn validate_full_catches_both_lora_and_weather_errors() {
        let mut p = valid_payload();
        p.spreading_factor = 1;        // LoRa validation error
        p.temperature_c = Some(200.0); // weather validation error
        let errors = validate_full(&p);
        assert!(
            errors.iter().any(|e| e.contains("spreading_factor")),
            "should contain LoRa error"
        );
        assert!(
            errors.iter().any(|e| e.contains("temperature")),
            "should contain weather error"
        );
    }
}
