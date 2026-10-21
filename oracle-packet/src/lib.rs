use serde::{Deserialize, Serialize};
use thiserror::Error;
use wrfcoin_shared_protocol::blockchain::transaction::{
    ForecastArtifactManifest, ForecastVerificationReceipt,
};
use wrfcoin_shared_protocol::crypto::domain_signing::{
    DomainSeparated, DomainSigningAction, DomainSigningContext, canonical_signing_message,
    payload_digest_hex, signing_chain_id, signing_network_id,
};
use wrfcoin_shared_protocol::crypto::signature::{PublicKey, Signature};
use wrfcoin_shared_protocol::weather::validation::WeatherObservation;

/// Current canonical packet version.
pub const ORACLE_PACKET_VERSION: u8 = 1;

/// Canonical envelope for weather-oracle packet transmission.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "packetType")]
pub enum OraclePacket {
    WeatherObservation {
        version: u8,
        timestamp: u64,
        station_id: String,
        observation: WeatherObservation,
        nonce: u64,
        signature: Vec<u8>,
    },
    ForecastManifest {
        version: u8,
        timestamp: u64,
        oracle_id: String,
        manifest: ForecastArtifactManifest,
        signature: Vec<u8>,
    },
    ForecastReceipt {
        version: u8,
        timestamp: u64,
        oracle_id: String,
        receipt: ForecastVerificationReceipt,
        signature: Vec<u8>,
    },
}

// #2249: `DomainSeparated` for the cross-crate signed type (tag reserved in
// `wrfcoin_shared_protocol::crypto::domain_registry`). The variant discriminant
// is bound first so the three variants cannot alias; each variant's `signature`
// is excluded.
impl DomainSeparated for OraclePacket {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.oracle_packet.v1";

    fn canonical_body(&self) -> Vec<u8> {
        use wrfcoin_shared_protocol::crypto::domain_impls::BodyBuilder;
        let b = BodyBuilder::new().str_field(self.packet_type());
        match self {
            OraclePacket::WeatherObservation {
                version,
                timestamp,
                station_id,
                observation,
                nonce,
                signature: _,
            } => b
                .u64_field(*version as u64)
                .u64_field(*timestamp)
                .str_field(station_id)
                .str_field(&serde_json::to_string(observation).unwrap_or_default())
                .u64_field(*nonce)
                .finish(),
            OraclePacket::ForecastManifest {
                version,
                timestamp,
                oracle_id,
                manifest,
                signature: _,
            } => b
                .u64_field(*version as u64)
                .u64_field(*timestamp)
                .str_field(oracle_id)
                .str_field(&serde_json::to_string(manifest).unwrap_or_default())
                .finish(),
            OraclePacket::ForecastReceipt {
                version,
                timestamp,
                oracle_id,
                receipt,
                signature: _,
            } => b
                .u64_field(*version as u64)
                .u64_field(*timestamp)
                .str_field(oracle_id)
                .str_field(&serde_json::to_string(receipt).unwrap_or_default())
                .finish(),
        }
    }
}

#[derive(Debug, Error)]
pub enum OraclePacketError {
    #[error("failed to serialize oracle packet: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("invalid signature bytes")]
    InvalidSignature,
    #[error("packet timestamp is out of range for signing context")]
    InvalidTimestamp,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "packetType")]
enum SignableOraclePacket<'a> {
    WeatherObservation {
        version: u8,
        timestamp: u64,
        station_id: &'a str,
        observation: &'a WeatherObservation,
        nonce: u64,
    },
    ForecastManifest {
        version: u8,
        timestamp: u64,
        oracle_id: &'a str,
        manifest: &'a ForecastArtifactManifest,
    },
    ForecastReceipt {
        version: u8,
        timestamp: u64,
        oracle_id: &'a str,
        receipt: &'a ForecastVerificationReceipt,
    },
}

impl OraclePacket {
    pub fn packet_type(&self) -> &str {
        match self {
            Self::WeatherObservation { .. } => "weatherObservation",
            Self::ForecastManifest { .. } => "forecastManifest",
            Self::ForecastReceipt { .. } => "forecastReceipt",
        }
    }

    pub fn oracle_id(&self) -> Option<&str> {
        match self {
            Self::ForecastManifest { oracle_id, .. } | Self::ForecastReceipt { oracle_id, .. } => {
                Some(oracle_id.as_str())
            },
            Self::WeatherObservation { .. } => None,
        }
    }

    pub fn station_id(&self) -> Option<&str> {
        match self {
            Self::WeatherObservation { station_id, .. } => Some(station_id.as_str()),
            Self::ForecastManifest { .. } | Self::ForecastReceipt { .. } => None,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, OraclePacketError> {
        Ok(serde_json::to_vec(self)?)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, OraclePacketError> {
        Ok(serde_json::from_slice(bytes)?)
    }

    pub fn verify_signature(&self, public_key: &PublicKey) -> bool {
        self.verify_signature_checked(public_key).unwrap_or(false)
    }

    pub fn verify_signature_checked(
        &self,
        public_key: &PublicKey,
    ) -> Result<bool, OraclePacketError> {
        let message = self.signing_bytes()?;
        let signature = Signature::from_bytes(self.signature())
            .map_err(|_| OraclePacketError::InvalidSignature)?;
        Ok(public_key.verify(&message, &signature))
    }

    fn signature(&self) -> &[u8] {
        match self {
            Self::WeatherObservation { signature, .. }
            | Self::ForecastManifest { signature, .. }
            | Self::ForecastReceipt { signature, .. } => signature.as_slice(),
        }
    }

    fn signing_payload(&self) -> SignableOraclePacket<'_> {
        match self {
            Self::WeatherObservation {
                version,
                timestamp,
                station_id,
                observation,
                nonce,
                ..
            } => SignableOraclePacket::WeatherObservation {
                version: *version,
                timestamp: *timestamp,
                station_id,
                observation,
                nonce: *nonce,
            },
            Self::ForecastManifest {
                version,
                timestamp,
                oracle_id,
                manifest,
                ..
            } => SignableOraclePacket::ForecastManifest {
                version: *version,
                timestamp: *timestamp,
                oracle_id,
                manifest,
            },
            Self::ForecastReceipt {
                version,
                timestamp,
                oracle_id,
                receipt,
                ..
            } => SignableOraclePacket::ForecastReceipt {
                version: *version,
                timestamp: *timestamp,
                oracle_id,
                receipt,
            },
        }
    }

    fn signing_bytes(&self) -> Result<Vec<u8>, OraclePacketError> {
        let payload_digest = payload_digest_hex(&self.signing_payload());
        let (action, signer_id, timestamp, nonce_string) = match self {
            Self::WeatherObservation {
                station_id,
                timestamp,
                nonce,
                ..
            } => (
                DomainSigningAction::OracleAttestation,
                station_id.as_str(),
                *timestamp,
                Some(nonce.to_string()),
            ),
            Self::ForecastManifest {
                oracle_id,
                timestamp,
                ..
            }
            | Self::ForecastReceipt {
                oracle_id,
                timestamp,
                ..
            } => (
                DomainSigningAction::ForecastVerification,
                oracle_id.as_str(),
                *timestamp,
                None,
            ),
        };

        let signing_timestamp =
            i64::try_from(timestamp).map_err(|_| OraclePacketError::InvalidTimestamp)?;
        let cid = signing_chain_id();
        let nid = signing_network_id();
        let context = DomainSigningContext {
            action,
            signer_id,
            timestamp: signing_timestamp,
            nonce: nonce_string.as_deref(),
            chain_id: &cid,
            network_id: &nid,
        };
        Ok(canonical_signing_message(&context, &payload_digest).into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;
    use wrfcoin_shared_protocol::blockchain::transaction::ForecastAccuracyMetrics;
    use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

    fn sample_weather_packet() -> OraclePacket {
        OraclePacket::WeatherObservation {
            version: ORACLE_PACKET_VERSION,
            timestamp: 1_730_000_000,
            station_id: "station-001".to_string(),
            observation: WeatherObservation {
                temperature_c: Some(21.5),
                humidity_percent: Some(64.0),
                pressure_hpa: Some(1012.1),
                wind_speed_ms: Some(3.4),
                precipitation_mm: Some(0.2),
                dewpoint_c: Some(14.2),
                radio_metadata: None,
            },
            nonce: 7,
            signature: vec![0; 64],
        }
    }

    fn sample_manifest_packet() -> OraclePacket {
        OraclePacket::ForecastManifest {
            version: ORACLE_PACKET_VERSION,
            timestamp: 1_730_000_100,
            oracle_id: "oracle-abc".to_string(),
            manifest: ForecastArtifactManifest {
                schema_version: 1,
                model_id: "wrf-v1".to_string(),
                input_hash: "a".repeat(64),
                output_hash: "b".repeat(64),
                produced_at: 1_730_000_100,
                region: "us-west".to_string(),
                horizon_hours: 24,
                input_variables: vec!["humidity".to_string(), "temperature".to_string()],
                output_variables: vec!["precipitation".to_string()],
            },
            signature: vec![1; 64],
        }
    }

    fn sample_receipt_packet() -> OraclePacket {
        OraclePacket::ForecastReceipt {
            version: ORACLE_PACKET_VERSION,
            timestamp: 1_730_000_200,
            oracle_id: "oracle-xyz".to_string(),
            receipt: ForecastVerificationReceipt {
                manifest_digest: "c".repeat(64),
                commit_id: "commit-123".to_string(),
                oracle_id: "d".repeat(64),
                metrics: ForecastAccuracyMetrics {
                    mae: 1.2,
                    rmse: 1.8,
                    bias: 0.1,
                    skill_score: 0.82,
                },
                verified_at: 1_730_000_200,
                nonce: "nonce-1".to_string(),
                zk_proof: None,
                signature: None,
            },
            signature: vec![2; 64],
        }
    }

    /// #2249 sensitivity: the per-variant signature is excluded from the
    /// `DomainSeparated` body, a signed field is bound, and the variant
    /// discriminant is bound (no cross-variant collision). Each assertion goes
    /// RED under the corresponding `canonical_body` regression.
    #[test]
    fn oracle_packet_domain_body_sensitivity_and_signature_exclusion_2249() {
        use wrfcoin_shared_protocol::crypto::domain_signing::DomainSeparated;
        let base = sample_weather_packet().signing_preimage();

        // Signature is excluded: same packet, different signature → same preimage.
        let sig_mutated = match sample_weather_packet() {
            OraclePacket::WeatherObservation {
                version,
                timestamp,
                station_id,
                observation,
                nonce,
                ..
            } => OraclePacket::WeatherObservation {
                version,
                timestamp,
                station_id,
                observation,
                nonce,
                signature: vec![0xAB; 64],
            },
            _ => unreachable!(),
        };
        assert_eq!(
            base,
            sig_mutated.signing_preimage(),
            "the variant signature must be excluded from the domain body"
        );

        // A signed field (station_id) is bound.
        let field_mutated = match sample_weather_packet() {
            OraclePacket::WeatherObservation {
                version,
                timestamp,
                observation,
                nonce,
                signature,
                ..
            } => OraclePacket::WeatherObservation {
                version,
                timestamp,
                station_id: "station-999".to_string(),
                observation,
                nonce,
                signature,
            },
            _ => unreachable!(),
        };
        assert_ne!(
            base,
            field_mutated.signing_preimage(),
            "a signed field (station_id) must be bound into the domain body"
        );

        // The variant discriminant is bound — distinct variants cannot alias.
        assert_ne!(
            sample_weather_packet().signing_preimage(),
            sample_manifest_packet().signing_preimage(),
            "the variant discriminant must be bound into the domain body"
        );
    }

    #[test]
    fn roundtrip_weather_observation_packet() {
        let packet = sample_weather_packet();
        let bytes = packet.to_bytes().expect("serialize weather packet");
        let decoded = OraclePacket::from_bytes(&bytes).expect("deserialize weather packet");
        assert_eq!(packet, decoded);
        assert_eq!(decoded.packet_type(), "weatherObservation");
        assert_eq!(decoded.oracle_id(), None);
        assert_eq!(decoded.station_id(), Some("station-001"));
    }

    #[test]
    fn roundtrip_forecast_manifest_packet() {
        let packet = sample_manifest_packet();
        let bytes = packet.to_bytes().expect("serialize forecast manifest");
        let decoded = OraclePacket::from_bytes(&bytes).expect("deserialize forecast manifest");
        assert_eq!(packet, decoded);
        assert_eq!(decoded.packet_type(), "forecastManifest");
        assert_eq!(decoded.oracle_id(), Some("oracle-abc"));
    }

    #[test]
    fn roundtrip_forecast_receipt_packet() {
        let packet = sample_receipt_packet();
        let bytes = packet.to_bytes().expect("serialize forecast receipt");
        let decoded = OraclePacket::from_bytes(&bytes).expect("deserialize forecast receipt");
        assert_eq!(packet, decoded);
        assert_eq!(decoded.packet_type(), "forecastReceipt");
        assert_eq!(decoded.oracle_id(), Some("oracle-xyz"));
        assert_eq!(decoded.station_id(), None);
    }

    #[test]
    fn verifies_signature_against_canonical_signing_payload() {
        let mut packet = sample_weather_packet();
        let mut rng = OsRng;
        let private_key = PrivateKey::generate(&mut rng);
        let public_key = PublicKey::from_private_key(&private_key);

        let signing_bytes = packet.signing_bytes().expect("build signing payload");
        let signature = private_key.sign(&signing_bytes).as_bytes().to_vec();
        if let OraclePacket::WeatherObservation { signature: sig, .. } = &mut packet {
            *sig = signature;
        }

        assert!(packet.verify_signature(&public_key));

        if let OraclePacket::WeatherObservation { nonce, .. } = &mut packet {
            *nonce += 1;
        }
        assert!(!packet.verify_signature(&public_key));
    }

    #[test]
    fn from_bytes_rejects_malformed_json() {
        let err = OraclePacket::from_bytes(b"{not-json").expect_err("malformed JSON must fail");
        assert!(matches!(err, OraclePacketError::Serialize(_)));
    }

    #[test]
    fn from_bytes_rejects_unknown_packet_type() {
        let payload = br#"{"packetType":"unknownType","version":1}"#;
        let err = OraclePacket::from_bytes(payload).expect_err("unknown packetType must fail");
        assert!(matches!(err, OraclePacketError::Serialize(_)));
    }
}
