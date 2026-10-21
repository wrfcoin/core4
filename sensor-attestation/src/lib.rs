//! Device trust, calibration proof, tamper evidence, and freshness attestation types.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use wrf_primitives::{Hash, Timestamp};
use wrfcoin_shared_protocol::crypto::domain_signing::{
    DomainSeparated, DomainSigningAction, DomainSigningContext, canonical_signing_message,
    payload_digest_hex, signing_chain_id, signing_network_id,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey, Signature};
use wrfcoin_shared_protocol::station_registry::WeatherStation;

pub mod device_identity;
pub mod plausibility;

/// Backward-compatible alias for the simplified sensor attestation shape.
pub type SensorAttestation = DeviceAttestation;
/// Backward-compatible alias for weather-oriented device classification.
pub type SensorType = DeviceType;

/// Errors returned by sensor-attestation helpers.
#[derive(Debug, Error)]
pub enum SensorAttestationError {
    /// Failed to serialize a signable payload.
    #[error("failed to serialize sensor attestation payload: {0}")]
    Serialize(#[from] serde_json::Error),
    /// Attestation timestamps cannot be represented in the signing context.
    #[error("attestation timestamp is out of range for signing context")]
    InvalidTimestamp,
    /// Signature bytes are malformed.
    #[error("invalid hardware signature bytes")]
    InvalidSignature,
    /// Input validation failed.
    #[error("{0}")]
    Invalid(String),
}

/// Canonical device classification for station and sensor hardware.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeviceType {
    /// Full weather station assembly.
    WeatherStation,
    /// Air quality sensing hardware.
    AirQualitySensor,
    /// Soil moisture sensing hardware.
    SoilMoistureSensor,
    /// Water quality sensing hardware.
    WaterQualitySensor,
    /// Seismic sensing hardware.
    SeismicSensor,
    /// Pressure sensor.
    Barometer,
    /// Temperature sensor.
    Thermometer,
    /// Humidity sensor.
    Hygrometer,
    /// Wind sensing hardware.
    Anemometer,
    /// GPS hardware.
    Gps,
    /// Photo-voltaic power subsystem sensor.
    PhotoVoltaic,
}

/// Hardware trust classification.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AttestationType {
    /// Trusted execution environment.
    Tee,
    /// Trusted platform module.
    Tpm,
    /// Dedicated secure element.
    SecureElement,
    /// Software-only attestation path.
    SoftwareOnly,
    /// Remote attestation.
    RemoteAttestation,
}

/// Proof that a calibration event occurred.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationProof {
    /// Time of the most recent calibration event.
    pub last_calibrated_at: Timestamp,
    /// Calibration method or procedure identifier.
    pub method: String,
    /// Hash of the calibration certificate or report.
    pub certificate_hash: Hash,
    /// Identifier for the calibrator or issuing authority.
    pub calibrator: String,
}

/// Tamper-evidence metadata carried alongside a device attestation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TamperEvidence {
    /// Whether the physical seal is still intact.
    pub seal_intact: bool,
    /// Most recent enclosure-open event, if any.
    pub enclosure_opened_at: Option<Timestamp>,
    /// Monotonic tamper counter emitted by the device.
    pub tamper_counter: u32,
}

/// Liveness proof showing the device was recently active.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FreshnessProof {
    /// Time at which freshness was asserted.
    pub attested_at: Timestamp,
    /// Boot/session nonce emitted by the device.
    pub boot_nonce: String,
}

/// Signed hardware attestation proving a device produced a reading hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAttestation {
    /// Station identifier tied to the device.
    pub station_id: String,
    /// Device classification.
    pub device_type: DeviceType,
    /// Trust level of the attestation mechanism.
    pub attestation_type: AttestationType,
    /// Hash of the reading or evidence bundle.
    pub reading_hash: Hash,
    /// Raw hardware signature bytes over the canonical payload.
    pub hardware_signature: Vec<u8>,
    /// Device model identifier.
    pub device_model: String,
    /// Firmware version producing the reading.
    pub firmware_version: String,
    /// Device public key bytes used for verification.
    pub device_public_key: Vec<u8>,
    /// Attestation time.
    pub attested_at: Timestamp,
    /// Optional calibration proof.
    pub calibration: Option<CalibrationProof>,
    /// Optional tamper evidence metadata.
    pub tamper: Option<TamperEvidence>,
    /// Optional freshness proof.
    pub freshness: Option<FreshnessProof>,
}

// #2249: `DomainSeparated` for the cross-crate signed type (tag reserved in
// `wrfcoin_shared_protocol::crypto::domain_registry`). Excludes
// `hardware_signature`; field ordering mirrors `SignableDeviceAttestation`.
impl DomainSeparated for DeviceAttestation {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.device_attestation.v1";

    fn canonical_body(&self) -> Vec<u8> {
        use wrfcoin_shared_protocol::crypto::domain_impls::BodyBuilder;
        let reading_hash = self.reading_hash.into_inner();
        let calibration = self
            .calibration
            .as_ref()
            .map(|c| serde_json::to_string(c).unwrap_or_default());
        let tamper = self
            .tamper
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());
        let freshness = self
            .freshness
            .as_ref()
            .map(|f| serde_json::to_string(f).unwrap_or_default());
        BodyBuilder::new()
            .str_field(&self.station_id)
            .str_field(&format!("{:?}", self.device_type))
            .str_field(&format!("{:?}", self.attestation_type))
            .bytes_field(&reading_hash)
            .str_field(&self.device_model)
            .str_field(&self.firmware_version)
            .bytes_field(&self.device_public_key)
            .u64_field(self.attested_at.into_inner())
            .opt_str_field(calibration.as_deref())
            .opt_str_field(tamper.as_deref())
            .opt_str_field(freshness.as_deref())
            .finish()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignableDeviceAttestation<'a> {
    station_id: &'a str,
    device_type: DeviceType,
    attestation_type: AttestationType,
    reading_hash: Hash,
    device_model: &'a str,
    firmware_version: &'a str,
    device_public_key: &'a [u8],
    attested_at: Timestamp,
    calibration: Option<&'a CalibrationProof>,
    tamper: Option<&'a TamperEvidence>,
    freshness: Option<&'a FreshnessProof>,
}

impl CalibrationProof {
    /// Returns `true` when the calibration record is too old.
    pub fn is_expired(&self, current_time: Timestamp, max_age_secs: u64) -> bool {
        match self
            .last_calibrated_at
            .into_inner()
            .checked_add(max_age_secs)
            .map(Timestamp::new)
        {
            Some(expires_at) => current_time >= expires_at,
            None => true,
        }
    }
}

impl FreshnessProof {
    /// Returns `true` when the proof is within the given freshness window.
    pub fn is_fresh(&self, current_time: Timestamp, max_age_secs: u64) -> bool {
        timestamp_within_age(self.attested_at, current_time, max_age_secs)
    }
}

impl DeviceAttestation {
    /// Create and sign a device attestation.
    #[allow(clippy::too_many_arguments)]
    pub fn sign(
        key: &PrivateKey,
        station_id: impl Into<String>,
        device_type: DeviceType,
        attestation_type: AttestationType,
        reading_hash: Hash,
        device_model: impl Into<String>,
        firmware_version: impl Into<String>,
        attested_at: Timestamp,
        calibration: Option<CalibrationProof>,
        tamper: Option<TamperEvidence>,
        freshness: Option<FreshnessProof>,
    ) -> Result<Self, SensorAttestationError> {
        let device_public_key = PublicKey::from_private_key(key).as_bytes().to_vec();
        let unsigned = Self {
            station_id: station_id.into(),
            device_type,
            attestation_type,
            reading_hash,
            hardware_signature: Vec::new(),
            device_model: device_model.into(),
            firmware_version: firmware_version.into(),
            device_public_key,
            attested_at,
            calibration,
            tamper,
            freshness,
        };
        unsigned.validate()?;
        let hardware_signature = key.sign(&unsigned.signing_bytes()?).as_bytes().to_vec();

        Ok(Self {
            hardware_signature,
            ..unsigned
        })
    }

    /// Build the canonical domain-separated signing message.
    pub fn canonical_signing_message(&self) -> Result<String, SensorAttestationError> {
        self.validate()?;
        let payload = SignableDeviceAttestation {
            station_id: &self.station_id,
            device_type: self.device_type,
            attestation_type: self.attestation_type,
            reading_hash: self.reading_hash,
            device_model: &self.device_model,
            firmware_version: &self.firmware_version,
            device_public_key: &self.device_public_key,
            attested_at: self.attested_at,
            calibration: self.calibration.as_ref(),
            tamper: self.tamper.as_ref(),
            freshness: self.freshness.as_ref(),
        };
        let digest = payload_digest_hex(&payload);
        let cid = signing_chain_id();
        let nid = signing_network_id();
        let context = DomainSigningContext {
            action: DomainSigningAction::OracleAttestation,
            signer_id: &self.station_id,
            timestamp: i64::try_from(self.attested_at.into_inner())
                .map_err(|_| SensorAttestationError::InvalidTimestamp)?,
            nonce: self
                .freshness
                .as_ref()
                .map(|proof| proof.boot_nonce.as_str()),
            chain_id: &cid,
            network_id: &nid,
        };
        Ok(canonical_signing_message(&context, &digest))
    }

    /// Verify the hardware signature against the canonical payload.
    pub fn verify_hardware_signature(&self, public_key: &PublicKey) -> bool {
        self.verify_hardware_signature_checked(public_key)
            .unwrap_or(false)
    }

    /// Verify the hardware signature and surface structural errors.
    pub fn verify_hardware_signature_checked(
        &self,
        public_key: &PublicKey,
    ) -> Result<bool, SensorAttestationError> {
        if public_key.as_bytes() != self.device_public_key.as_slice() {
            return Ok(false);
        }
        let signature = Signature::from_bytes(&self.hardware_signature)
            .map_err(|_| SensorAttestationError::InvalidSignature)?;
        Ok(public_key.verify(&self.signing_bytes()?, &signature))
    }

    /// Returns `true` when the attestation is within the freshness window.
    pub fn is_fresh(&self, current_time: Timestamp, max_age_secs: u64) -> bool {
        let attestation_fresh = timestamp_within_age(self.attested_at, current_time, max_age_secs);
        let proof_fresh = self
            .freshness
            .as_ref()
            .map(|proof| proof.is_fresh(current_time, max_age_secs))
            .unwrap_or(true);
        attestation_fresh && proof_fresh
    }

    /// Validate station linkage against the current registry entry.
    pub fn validate_against_station(&self, station: &WeatherStation) -> bool {
        self.station_id == station.station_id && station.is_active
    }

    /// Validate attestation fields without checking a signature.
    pub fn validate(&self) -> Result<(), SensorAttestationError> {
        validate_trimmed("station_id", &self.station_id)?;
        validate_trimmed("device_model", &self.device_model)?;
        validate_trimmed("firmware_version", &self.firmware_version)?;
        if self.device_public_key.is_empty() {
            return Err(SensorAttestationError::Invalid(
                "device_public_key is required".to_string(),
            ));
        }
        if let Some(calibration) = &self.calibration {
            validate_trimmed("calibration.method", &calibration.method)?;
            validate_trimmed("calibration.calibrator", &calibration.calibrator)?;
        }
        if let Some(freshness) = &self.freshness {
            validate_trimmed("freshness.boot_nonce", &freshness.boot_nonce)?;
        }
        Ok(())
    }

    fn signing_bytes(&self) -> Result<Vec<u8>, SensorAttestationError> {
        Ok(self.canonical_signing_message()?.into_bytes())
    }
}

fn timestamp_within_age(
    attested_at: Timestamp,
    current_time: Timestamp,
    max_age_secs: u64,
) -> bool {
    current_time
        .into_inner()
        .checked_sub(attested_at.into_inner())
        .map(|age| age <= max_age_secs)
        .unwrap_or(false)
}

fn validate_trimmed(field: &str, value: &str) -> Result<(), SensorAttestationError> {
    if value.trim().is_empty() {
        return Err(SensorAttestationError::Invalid(format!(
            "{field} is required"
        )));
    }
    if value.trim() != value {
        return Err(SensorAttestationError::Invalid(format!(
            "{field} cannot contain leading or trailing whitespace"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;
    use wrfcoin_shared_protocol::station_registry::{
        DataQualityTier, StationLocation, WeatherStation,
    };

    fn sample_station(is_active: bool) -> WeatherStation {
        WeatherStation {
            station_id: "station-1".to_string(),
            owner_address: "wrf1owner".to_string(),
            location: StationLocation {
                latitude: 1.0,
                longitude: 2.0,
                elevation_meters: Some(3.0),
            },
            hardware_model: "WRF-200".to_string(),
            calibration_date: Some("2026-03-01".to_string()),
            is_active,
            reputation_score: 99,
            data_quality_tier: DataQualityTier::Gold,
            provenance_class: Default::default(),
            ring_public_key: None,
        }
    }

    fn sample_calibration() -> CalibrationProof {
        CalibrationProof {
            last_calibrated_at: Timestamp::new(1_730_000_000),
            method: "nvlap".to_string(),
            certificate_hash: Hash::from([7; 32]),
            calibrator: "cal-lab-1".to_string(),
        }
    }

    fn sample_freshness() -> FreshnessProof {
        FreshnessProof {
            attested_at: Timestamp::new(1_730_000_010),
            boot_nonce: "boot-123".to_string(),
        }
    }

    #[test]
    fn device_attestation_sign_verify_round_trip() {
        let mut rng = OsRng;
        let key = PrivateKey::generate(&mut rng);
        let public_key = PublicKey::from_private_key(&key);
        let attestation = DeviceAttestation::sign(
            &key,
            "station-1",
            DeviceType::Thermometer,
            AttestationType::SecureElement,
            Hash::from([1; 32]),
            "TS-100",
            "1.0.2",
            Timestamp::new(1_730_000_010),
            Some(sample_calibration()),
            Some(TamperEvidence {
                seal_intact: true,
                enclosure_opened_at: None,
                tamper_counter: 0,
            }),
            Some(sample_freshness()),
        )
        .expect("signing should succeed");

        assert!(attestation.verify_hardware_signature(&public_key));
    }

    /// #2249 sensitivity: a signed field is bound into the `DomainSeparated`
    /// body and the signature is excluded. Removing `.str_field(&self.station_id)`
    /// from `canonical_body` makes the first assertion RED; folding the signature
    /// in makes the second RED.
    #[test]
    fn device_attestation_domain_body_sensitivity_and_signature_exclusion_2249() {
        use wrfcoin_shared_protocol::crypto::domain_signing::DomainSeparated;
        let mut rng = OsRng;
        let key = PrivateKey::generate(&mut rng);
        let signed = DeviceAttestation::sign(
            &key,
            "station-1",
            DeviceType::Thermometer,
            AttestationType::SecureElement,
            Hash::from([1; 32]),
            "TS-100",
            "1.0.2",
            Timestamp::new(1_730_000_010),
            Some(sample_calibration()),
            Some(TamperEvidence {
                seal_intact: true,
                enclosure_opened_at: None,
                tamper_counter: 0,
            }),
            Some(sample_freshness()),
        )
        .expect("signing should succeed");
        let base = signed.signing_preimage();

        let mut sig_only = signed.clone();
        sig_only.hardware_signature = vec![0xAB; 64];
        assert_eq!(
            base,
            sig_only.signing_preimage(),
            "hardware_signature must be excluded from the domain body"
        );

        let mut changed = signed.clone();
        changed.station_id = "station-2".to_string();
        assert_ne!(
            base,
            changed.signing_preimage(),
            "a signed field (station_id) must be bound into the domain body"
        );
    }

    #[test]
    fn device_attestation_verify_rejects_tampered_attestation() {
        let mut rng = OsRng;
        let key = PrivateKey::generate(&mut rng);
        let public_key = PublicKey::from_private_key(&key);
        let mut attestation = DeviceAttestation::sign(
            &key,
            "station-1",
            DeviceType::Barometer,
            AttestationType::Tpm,
            Hash::from([2; 32]),
            "BP-200",
            "2.0.0",
            Timestamp::new(1_730_000_010),
            None,
            None,
            Some(sample_freshness()),
        )
        .expect("signing should succeed");

        attestation.reading_hash = Hash::from([3; 32]);
        assert!(!attestation.verify_hardware_signature(&public_key));
    }

    #[test]
    fn device_attestation_verify_rejects_tampered_signature() {
        let mut rng = OsRng;
        let key = PrivateKey::generate(&mut rng);
        let public_key = PublicKey::from_private_key(&key);
        let mut attestation = DeviceAttestation::sign(
            &key,
            "station-1",
            DeviceType::Gps,
            AttestationType::RemoteAttestation,
            Hash::from([4; 32]),
            "GPS-9",
            "3.1.4",
            Timestamp::new(1_730_000_010),
            None,
            None,
            None,
        )
        .expect("signing should succeed");

        attestation.hardware_signature[0] ^= 0xFF;
        assert!(!attestation.verify_hardware_signature(&public_key));
    }

    #[test]
    fn freshness_window_is_enforced() {
        let attestation = DeviceAttestation {
            station_id: "station-1".to_string(),
            device_type: DeviceType::Thermometer,
            attestation_type: AttestationType::SecureElement,
            reading_hash: Hash::from([9; 32]),
            hardware_signature: vec![1; 64],
            device_model: "TS-100".to_string(),
            firmware_version: "1.0.0".to_string(),
            device_public_key: vec![1; 32],
            attested_at: Timestamp::new(100),
            calibration: None,
            tamper: None,
            freshness: Some(FreshnessProof {
                attested_at: Timestamp::new(105),
                boot_nonce: "boot-1".to_string(),
            }),
        };

        assert!(attestation.is_fresh(Timestamp::new(110), 10));
        assert!(!attestation.is_fresh(Timestamp::new(121), 10));
    }

    #[test]
    fn calibration_proof_expiry_is_enforced() {
        let calibration = sample_calibration();
        assert!(!calibration.is_expired(Timestamp::new(1_730_000_100), 200));
        assert!(calibration.is_expired(Timestamp::new(1_730_000_300), 200));
    }

    #[test]
    fn station_validation_requires_match_and_active_station() {
        let attestation = DeviceAttestation {
            station_id: "station-1".to_string(),
            device_type: DeviceType::WeatherStation,
            attestation_type: AttestationType::SoftwareOnly,
            reading_hash: Hash::from([5; 32]),
            hardware_signature: vec![1; 64],
            device_model: "WS-1".to_string(),
            firmware_version: "1.0.0".to_string(),
            device_public_key: vec![1; 32],
            attested_at: Timestamp::new(100),
            calibration: None,
            tamper: None,
            freshness: None,
        };

        assert!(attestation.validate_against_station(&sample_station(true)));
        assert!(!attestation.validate_against_station(&sample_station(false)));
        let mut mismatched = sample_station(true);
        mismatched.station_id = "station-2".to_string();
        assert!(!attestation.validate_against_station(&mismatched));
    }

    #[test]
    fn serde_round_trip_preserves_enum_and_payload_shapes() {
        let attestation = DeviceAttestation {
            station_id: "station-1".to_string(),
            device_type: DeviceType::PhotoVoltaic,
            attestation_type: AttestationType::Tee,
            reading_hash: Hash::from([6; 32]),
            hardware_signature: vec![7; 64],
            device_model: "PV-8".to_string(),
            firmware_version: "4.0.0".to_string(),
            device_public_key: vec![8; 32],
            attested_at: Timestamp::new(200),
            calibration: Some(sample_calibration()),
            tamper: Some(TamperEvidence {
                seal_intact: false,
                enclosure_opened_at: Some(Timestamp::new(199)),
                tamper_counter: 2,
            }),
            freshness: Some(sample_freshness()),
        };

        let json = serde_json::to_string(&attestation).expect("serialize should succeed");
        let decoded: DeviceAttestation =
            serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(decoded, attestation);
    }
}
