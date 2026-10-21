//! #2205 — hardware-backed device keys with tiered attestation trust.
//!
//! What the crate already had (reported first, as the story asks): a
//! domain-separated [`crate::DeviceAttestation`] signed by an ordinary
//! ed25519 key, with caller-supplied [`crate::AttestationType`], calibration /
//! tamper / freshness metadata and signature-tamper tests. That proves a key
//! signed a reading hash; it does **not** prove the key lives in hardware. A
//! software key can pass the signature check while labelling itself
//! `SecureElement`, and `provenance_class` — which decides payment — is an
//! authorised assertion at registration, not a device proof.
//!
//! This module adds the identity model and the tier mapping around that gap,
//! without changing what consensus pays:
//!
//! - [`crate::device_identity::KeyBacking`] says *what the key claims to be backed by*, carrying the
//!   evidence (an attestation chain, a TPM quote) rather than a label;
//! - [`crate::device_identity::DeviceIdentity`] binds a device key to an operator account and a
//!   station, with the backing and the enrollment time;
//! - [`crate::device_identity::TrustTier`] is **earned by proof, never self-asserted**: `Attested`
//!   needs a secure-element chain that a [`crate::device_identity::HardwareAttestationVerifier`]
//!   accepted, `Verified` a TPM quote it accepted, `Basic` a software key
//!   with a proven-continuous observation history, and everything else —
//!   including a claimed hardware backing the verifier did not accept — is
//!   `Unverified`. The default verifier, [`crate::device_identity::NoVendorRoots`], accepts nothing,
//!   so a deployment with no configured roots cannot mint an `Attested` tier
//!   by accident;
//! - [`crate::device_identity::DeviceKeyScope`] scopes a device key to observation submission: it
//!   cannot authorise a transfer, a stake, a vote or a registration;
//! - [`crate::device_identity::DeviceRegistry`] bounds devices per operator, the same
//!   stations-per-identity number the ingress Sybil policy uses (#2148).
//!
//! Unattested stations keep participating: a tier lowers the proposed reward
//! weight and the quality-tier ceiling, it never gates acceptance. The
//! weights here are a **proposal recorded for ADR-005**; nothing in consensus
//! reads them, and wiring them into a reward path is a feature-gated change
//! (rolling-upgrade discipline), not part of this module.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use wrf_primitives::Timestamp;
use wrfcoin_shared_protocol::crypto::signature::PublicKey;
use wrfcoin_shared_protocol::station_registry::{DataQualityTier, WeatherStation};

/// How a device key claims to be protected, with the evidence for the claim.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum KeyBacking {
    /// A dedicated secure element; `attestation` is the vendor attestation
    /// chain over the device public key, `model` the part the chain must name.
    SecureElement { attestation: Vec<u8>, model: String },
    /// A TPM; `quote` is the signed quote over the device public key.
    Tpm { quote: Vec<u8> },
    /// An OS keystore (Android Keystore, iOS Keychain, …) with no attestation.
    SoftwareKeystore,
    /// A key file on disk.
    SoftwareFile,
}

impl KeyBacking {
    /// Whether this backing *claims* hardware protection. A claim earns
    /// nothing by itself — see [`trust_tier`].
    pub fn claims_hardware(&self) -> bool {
        matches!(self, Self::SecureElement { .. } | Self::Tpm { .. })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SecureElement { .. } => "secure_element",
            Self::Tpm { .. } => "tpm",
            Self::SoftwareKeystore => "software_keystore",
            Self::SoftwareFile => "software_file",
        }
    }
}

/// A device key registered to an operator for one station.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIdentity {
    /// Stable device identifier chosen at enrollment (serial, IMEI hash, …).
    pub device_id: String,
    /// The key that signs this device's readings.
    pub public_key: PublicKey,
    pub backing: KeyBacking,
    pub registered_at: Timestamp,
    /// Operator account address the device is registered to.
    pub operator: String,
    /// Station the device submits for.
    pub station_id: String,
}

/// Earned trust, highest first.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum TrustTier {
    /// Anything without proof, including a claimed hardware backing the
    /// verifier did not accept.
    Unverified,
    /// A software key with a proven-continuous observation history.
    Basic,
    /// A TPM quote the verifier accepted.
    Verified,
    /// A secure-element attestation chain the verifier accepted.
    Attested,
}

impl TrustTier {
    /// Proposed reward weight, basis points of the unweighted reward. Not
    /// consumed by consensus; recorded for ADR-005 and a feature-gated wiring.
    pub fn proposed_reward_weight_bps(self) -> u32 {
        match self {
            Self::Attested => 10_000,
            Self::Verified => 9_000,
            Self::Basic => 7_500,
            Self::Unverified => 5_000,
        }
    }

    /// The highest data-quality tier a station may hold at this trust tier.
    pub fn quality_tier_ceiling(self) -> DataQualityTier {
        match self {
            Self::Attested => DataQualityTier::Gold,
            Self::Verified => DataQualityTier::Silver,
            Self::Basic => DataQualityTier::Bronze,
            Self::Unverified => DataQualityTier::Unverified,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Attested => "attested",
            Self::Verified => "verified",
            Self::Basic => "basic",
            Self::Unverified => "unverified",
        }
    }
}

/// Verifies hardware evidence against vendor roots. Implementations own the
/// roots; this crate ships only the verifier that accepts nothing.
pub trait HardwareAttestationVerifier {
    /// Does `attestation` prove that `identity.public_key` is held by a
    /// secure element of `model`, under a configured vendor root?
    fn verify_secure_element(&self, identity: &DeviceIdentity, attestation: &[u8], model: &str) -> bool;
    /// Does `quote` prove that `identity.public_key` is TPM-resident?
    fn verify_tpm_quote(&self, identity: &DeviceIdentity, quote: &[u8]) -> bool;
}

/// The honest default: no vendor roots are configured, so no hardware claim
/// verifies and every claimed backing maps to `Unverified`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoVendorRoots;

impl HardwareAttestationVerifier for NoVendorRoots {
    fn verify_secure_element(&self, _: &DeviceIdentity, _: &[u8], _: &str) -> bool {
        false
    }
    fn verify_tpm_quote(&self, _: &DeviceIdentity, _: &[u8]) -> bool {
        false
    }
}

/// Continuity of a software key's observation record, the evidence for
/// [`TrustTier::Basic`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContinuousHistory {
    pub first_observation_at: Timestamp,
    pub last_observation_at: Timestamp,
    pub observation_count: u64,
    /// Longest interval between consecutive accepted observations.
    pub longest_gap_secs: u64,
}

/// A software key earns `Basic` after a week of at least hourly observations
/// with no gap longer than six hours.
pub const BASIC_TIER_MIN_HISTORY_SECS: u64 = 7 * 24 * 3_600;
pub const BASIC_TIER_MIN_OBSERVATIONS: u64 = 7 * 24;
pub const BASIC_TIER_MAX_GAP_SECS: u64 = 6 * 3_600;

impl ContinuousHistory {
    pub fn is_continuous(&self) -> bool {
        let span = self
            .last_observation_at
            .into_inner()
            .saturating_sub(self.first_observation_at.into_inner());
        span >= BASIC_TIER_MIN_HISTORY_SECS
            && self.observation_count >= BASIC_TIER_MIN_OBSERVATIONS
            && self.longest_gap_secs <= BASIC_TIER_MAX_GAP_SECS
    }
}

/// The tier mapping, given the verifier's verdict on the claimed backing.
///
/// `attestation_valid` is meaningful only for hardware claims; a software
/// backing ignores it. A hardware claim that did not verify is `Unverified`,
/// not `Basic`: a key that lied about its backing does not get the software
/// tier its history might otherwise earn.
///
/// Private on purpose: a public function taking `attestation_valid: bool`
/// would let any caller self-assert `Attested`. The only way in is
/// [`verified_trust_tier`], which runs the verifier itself. This does not
/// compile from outside the crate:
///
/// ```compile_fail
/// use wrfcoin_sensor_attestation::device_identity::trust_tier;
/// ```
fn trust_tier(
    identity: &DeviceIdentity,
    attestation_valid: bool,
    history: Option<&ContinuousHistory>,
) -> TrustTier {
    match &identity.backing {
        KeyBacking::SecureElement { .. } if attestation_valid => TrustTier::Attested,
        KeyBacking::Tpm { .. } if attestation_valid => TrustTier::Verified,
        KeyBacking::SecureElement { .. } | KeyBacking::Tpm { .. } => TrustTier::Unverified,
        KeyBacking::SoftwareKeystore | KeyBacking::SoftwareFile => {
            if history.is_some_and(ContinuousHistory::is_continuous) {
                TrustTier::Basic
            } else {
                TrustTier::Unverified
            }
        },
    }
}

/// Run the verifier on the claimed backing, then map the tier. This is the
/// entry point a registrar uses; it cannot be told the attestation is valid.
pub fn verified_trust_tier(
    identity: &DeviceIdentity,
    verifier: &dyn HardwareAttestationVerifier,
    history: Option<&ContinuousHistory>,
) -> TrustTier {
    let attestation_valid = match &identity.backing {
        KeyBacking::SecureElement { attestation, model } => {
            !attestation.is_empty()
                && !model.trim().is_empty()
                && verifier.verify_secure_element(identity, attestation, model)
        },
        KeyBacking::Tpm { quote } => !quote.is_empty() && verifier.verify_tpm_quote(identity, quote),
        KeyBacking::SoftwareKeystore | KeyBacking::SoftwareFile => false,
    };
    trust_tier(identity, attestation_valid, history)
}

/// Things a key might be asked to authorise.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKeyAction {
    SubmitObservation,
    /// Sign a [`crate::DeviceAttestation`] over a reading hash.
    AttestReading,
    Transfer,
    Stake,
    Vote,
    RegisterStation,
    RotateDeviceKey,
}

/// What a device key may do. There is one scope: observation submission.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKeyScope {
    ObservationOnly,
}

impl DeviceKeyScope {
    pub fn permits(self, action: DeviceKeyAction) -> bool {
        match self {
            Self::ObservationOnly => {
                matches!(action, DeviceKeyAction::SubmitObservation | DeviceKeyAction::AttestReading)
            },
        }
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DeviceKeyError {
    #[error("device key {device_id} is scoped to observation submission and cannot {action:?}")]
    OutOfScope { device_id: String, action: DeviceKeyAction },
    #[error("device {device_id} is registered to {operator}, not to the station owner {owner}")]
    OperatorMismatch { device_id: String, operator: String, owner: String },
    #[error("device {device_id} is registered for station {registered}, not {requested}")]
    StationMismatch { device_id: String, registered: String, requested: String },
    #[error("station {station_id} is inactive")]
    StationInactive { station_id: String },
    #[error("operator {operator} already has {max} registered devices")]
    OperatorDeviceLimit { operator: String, max: u32 },
    #[error("device {device_id} is already registered")]
    DuplicateDevice { device_id: String },
    #[error("public key is already registered as device {existing_device_id}")]
    DuplicateKey { existing_device_id: String },
    #[error("{field} is required")]
    Missing { field: &'static str },
}

impl DeviceIdentity {
    /// Every device key has the observation-only scope.
    pub fn scope(&self) -> DeviceKeyScope {
        DeviceKeyScope::ObservationOnly
    }

    /// Refuse anything outside the device key's scope.
    pub fn authorize(&self, action: DeviceKeyAction) -> Result<(), DeviceKeyError> {
        if self.scope().permits(action) {
            Ok(())
        } else {
            Err(DeviceKeyError::OutOfScope { device_id: self.device_id.clone(), action })
        }
    }

    /// The device may submit for `station` only if the station is active,
    /// registered to the device's operator, and is the station the device was
    /// enrolled for.
    pub fn bind_to_station(&self, station: &WeatherStation) -> Result<(), DeviceKeyError> {
        if station.station_id != self.station_id {
            return Err(DeviceKeyError::StationMismatch {
                device_id: self.device_id.clone(),
                registered: self.station_id.clone(),
                requested: station.station_id.clone(),
            });
        }
        if !station.is_active {
            return Err(DeviceKeyError::StationInactive { station_id: station.station_id.clone() });
        }
        if !station.owner_address.eq_ignore_ascii_case(&self.operator) {
            return Err(DeviceKeyError::OperatorMismatch {
                device_id: self.device_id.clone(),
                operator: self.operator.clone(),
                owner: station.owner_address.clone(),
            });
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), DeviceKeyError> {
        for (field, value) in [
            ("device_id", &self.device_id),
            ("operator", &self.operator),
            ("station_id", &self.station_id),
        ] {
            if value.trim().is_empty() {
                return Err(DeviceKeyError::Missing { field });
            }
        }
        Ok(())
    }
}

/// Devices per operator, the same bound as stations per identity at the
/// ingress edge (#2148), so a device farm is capped where a station farm is.
pub const MAX_DEVICES_PER_OPERATOR: u32 = 8;

/// Bounded, in-memory device enrollment index.
#[derive(Debug, Clone, Default)]
pub struct DeviceRegistry {
    devices: BTreeMap<String, DeviceIdentity>,
    by_operator: BTreeMap<String, BTreeSet<String>>,
    /// Public key bytes → device id: one key is one device, across all
    /// operators, so a single key cannot pose as a device farm.
    by_key: BTreeMap<Vec<u8>, String>,
    max_per_operator: u32,
}

impl DeviceRegistry {
    pub fn new() -> Self {
        Self::with_limit(MAX_DEVICES_PER_OPERATOR)
    }

    pub fn with_limit(max_per_operator: u32) -> Self {
        Self { max_per_operator: max_per_operator.max(1), ..Self::default() }
    }

    pub fn register(&mut self, identity: DeviceIdentity) -> Result<(), DeviceKeyError> {
        identity.validate()?;
        if self.devices.contains_key(&identity.device_id) {
            return Err(DeviceKeyError::DuplicateDevice { device_id: identity.device_id });
        }
        let key_bytes = identity.public_key.as_bytes().to_vec();
        if let Some(existing) = self.by_key.get(&key_bytes) {
            return Err(DeviceKeyError::DuplicateKey { existing_device_id: existing.clone() });
        }
        let operator = identity.operator.to_ascii_lowercase();
        let owned = self.by_operator.entry(operator.clone()).or_default();
        if owned.len() as u32 >= self.max_per_operator {
            return Err(DeviceKeyError::OperatorDeviceLimit {
                operator: identity.operator,
                max: self.max_per_operator,
            });
        }
        owned.insert(identity.device_id.clone());
        self.by_key.insert(key_bytes, identity.device_id.clone());
        self.devices.insert(identity.device_id.clone(), identity);
        Ok(())
    }

    pub fn get(&self, device_id: &str) -> Option<&DeviceIdentity> {
        self.devices.get(device_id)
    }

    pub fn devices_for(&self, operator: &str) -> usize {
        self.by_operator
            .get(&operator.to_ascii_lowercase())
            .map(|s| s.len())
            .unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::crypto::signature::PrivateKey;
    use wrfcoin_shared_protocol::station_registry::{
        ContributionProvenanceClass, StationLocation,
    };

    /// Accepts exactly one attestation chain / quote: the one it was built
    /// with. Everything else — a forged chain, a chain for another key — fails.
    struct FixedRoot {
        chain: Vec<u8>,
        quote: Vec<u8>,
        model: &'static str,
        key: PublicKey,
    }

    impl HardwareAttestationVerifier for FixedRoot {
        fn verify_secure_element(&self, id: &DeviceIdentity, attestation: &[u8], model: &str) -> bool {
            id.public_key == self.key && attestation == self.chain.as_slice() && model == self.model
        }
        fn verify_tpm_quote(&self, id: &DeviceIdentity, quote: &[u8]) -> bool {
            id.public_key == self.key && quote == self.quote.as_slice()
        }
    }

    fn key(seed: u8) -> PublicKey {
        use rand::SeedableRng;
        let sk = PrivateKey::generate(&mut rand::rngs::StdRng::seed_from_u64(seed as u64));
        PublicKey::from_private_key(&sk)
    }

    fn identity(seed: u8, backing: KeyBacking) -> DeviceIdentity {
        DeviceIdentity {
            device_id: format!("dev-{seed}"), // secret-leak-ok: test fixture; `seed` is a u8 fixture index naming the device, not key material
            public_key: key(seed),
            backing,
            registered_at: Timestamp::new(1_700_000_000),
            operator: "0xOperatorA".to_string(),
            station_id: "station-a".to_string(),
        }
    }

    fn root_for(seed: u8) -> FixedRoot {
        FixedRoot { chain: vec![0xAA; 64], quote: vec![0xBB; 96], model: "ATECC608B", key: key(seed) }
    }

    fn history(days: u64, gap_secs: u64) -> ContinuousHistory {
        ContinuousHistory {
            first_observation_at: Timestamp::new(1_700_000_000),
            last_observation_at: Timestamp::new(1_700_000_000 + days * 86_400),
            observation_count: days * 24,
            longest_gap_secs: gap_secs,
        }
    }

    #[test]
    fn a_valid_secure_element_attestation_yields_attested() {
        let root = root_for(1);
        let id = identity(1, KeyBacking::SecureElement { attestation: vec![0xAA; 64], model: "ATECC608B".into() });
        assert_eq!(verified_trust_tier(&id, &root, None), TrustTier::Attested);
        let tpm = identity(1, KeyBacking::Tpm { quote: vec![0xBB; 96] });
        assert_eq!(verified_trust_tier(&tpm, &root, None), TrustTier::Verified);
    }

    #[test]
    fn a_forged_or_mismatched_attestation_yields_unverified() {
        let root = root_for(1);
        let forged = identity(1, KeyBacking::SecureElement { attestation: vec![0xAB; 64], model: "ATECC608B".into() });
        assert_eq!(verified_trust_tier(&forged, &root, None), TrustTier::Unverified);
        let wrong_model = identity(1, KeyBacking::SecureElement { attestation: vec![0xAA; 64], model: "SE050".into() });
        assert_eq!(verified_trust_tier(&wrong_model, &root, None), TrustTier::Unverified);
        // A genuine chain for a DIFFERENT key proves nothing about this one.
        let other_key = identity(2, KeyBacking::SecureElement { attestation: vec![0xAA; 64], model: "ATECC608B".into() });
        assert_eq!(verified_trust_tier(&other_key, &root, None), TrustTier::Unverified);
        // …and a week of history does not rescue a lying hardware claim.
        assert_eq!(verified_trust_tier(&forged, &root, Some(&history(10, 600))), TrustTier::Unverified);
    }

    #[test]
    fn a_self_asserted_backing_without_a_verifier_is_worth_nothing() {
        let id = identity(3, KeyBacking::SecureElement { attestation: vec![0xAA; 64], model: "ATECC608B".into() });
        assert_eq!(verified_trust_tier(&id, &NoVendorRoots, None), TrustTier::Unverified);
        let tpm = identity(3, KeyBacking::Tpm { quote: vec![1, 2, 3] });
        assert_eq!(verified_trust_tier(&tpm, &NoVendorRoots, None), TrustTier::Unverified);
        // Even the raw mapping cannot be talked into a tier by the label alone.
        assert_eq!(trust_tier(&id, false, Some(&history(30, 60))), TrustTier::Unverified);
        // An empty chain never reaches the verifier.
        let empty = identity(3, KeyBacking::SecureElement { attestation: vec![], model: "ATECC608B".into() });
        assert_eq!(verified_trust_tier(&empty, &root_for(3), None), TrustTier::Unverified);
    }

    #[test]
    fn a_software_key_earns_basic_only_with_continuous_history() {
        let id = identity(4, KeyBacking::SoftwareKeystore);
        assert_eq!(verified_trust_tier(&id, &NoVendorRoots, None), TrustTier::Unverified);
        assert_eq!(verified_trust_tier(&id, &NoVendorRoots, Some(&history(3, 600))), TrustTier::Unverified, "too short");
        assert_eq!(verified_trust_tier(&id, &NoVendorRoots, Some(&history(10, 7 * 3_600))), TrustTier::Unverified, "gap too long");
        assert_eq!(verified_trust_tier(&id, &NoVendorRoots, Some(&history(10, 3_600))), TrustTier::Basic);
        let file = identity(5, KeyBacking::SoftwareFile);
        assert_eq!(trust_tier(&file, true, Some(&history(10, 3_600))), TrustTier::Basic, "attestation_valid is ignored for software");
    }

    #[test]
    fn tiers_order_reward_weight_and_quality_ceiling_and_never_exclude() {
        let rank = |t: DataQualityTier| match t {
            DataQualityTier::Unverified => 0,
            DataQualityTier::Bronze => 1,
            DataQualityTier::Silver => 2,
            DataQualityTier::Gold => 3,
        };
        let tiers = [TrustTier::Unverified, TrustTier::Basic, TrustTier::Verified, TrustTier::Attested];
        for pair in tiers.windows(2) {
            assert!(pair[0] < pair[1]);
            assert!(pair[0].proposed_reward_weight_bps() < pair[1].proposed_reward_weight_bps());
            assert!(rank(pair[0].quality_tier_ceiling()) < rank(pair[1].quality_tier_ceiling()));
        }
        assert!(TrustTier::Unverified.proposed_reward_weight_bps() > 0, "unattested stations still earn");
        assert_eq!(TrustTier::Attested.proposed_reward_weight_bps(), 10_000);
        assert_eq!(TrustTier::Attested.quality_tier_ceiling(), DataQualityTier::Gold);
        assert_eq!(TrustTier::Unverified.quality_tier_ceiling(), DataQualityTier::Unverified);
    }

    #[test]
    fn a_device_key_cannot_sign_anything_but_observations() {
        let id = identity(6, KeyBacking::SoftwareKeystore);
        assert!(id.authorize(DeviceKeyAction::SubmitObservation).is_ok());
        assert!(id.authorize(DeviceKeyAction::AttestReading).is_ok());
        for action in [
            DeviceKeyAction::Transfer,
            DeviceKeyAction::Stake,
            DeviceKeyAction::Vote,
            DeviceKeyAction::RegisterStation,
            DeviceKeyAction::RotateDeviceKey,
        ] {
            assert_eq!(
                id.authorize(action),
                Err(DeviceKeyError::OutOfScope { device_id: "dev-6".into(), action })
            );
        }
    }

    #[test]
    fn a_device_binds_only_to_its_operators_active_station() {
        let id = identity(7, KeyBacking::SoftwareFile);
        let mut station = WeatherStation {
            station_id: "station-a".into(),
            owner_address: "0xOPERATORA".into(),
            location: StationLocation { latitude: 46.7, longitude: -92.1, elevation_meters: None },
            hardware_model: "test".into(),
            calibration_date: None,
            is_active: true,
            reputation_score: 0,
            data_quality_tier: DataQualityTier::Unverified,
            provenance_class: ContributionProvenanceClass::Real,
            ring_public_key: None,
        };
        assert!(id.bind_to_station(&station).is_ok(), "owner compared case-insensitively");
        station.owner_address = "0xSomeoneElse".into();
        assert!(matches!(id.bind_to_station(&station), Err(DeviceKeyError::OperatorMismatch { .. })));
        station.owner_address = "0xOperatorA".into();
        station.is_active = false;
        assert!(matches!(id.bind_to_station(&station), Err(DeviceKeyError::StationInactive { .. })));
        station.is_active = true;
        station.station_id = "station-b".into();
        assert!(matches!(id.bind_to_station(&station), Err(DeviceKeyError::StationMismatch { .. })));
    }

    #[test]
    fn a_forged_secure_element_claim_cannot_reach_attested_through_the_public_api() {
        // The public entry point runs the verifier; a caller cannot hand it a
        // verdict. A forged claim with no vendor roots, or with a root for a
        // different key, stays Unverified whatever history it brings.
        let forged = identity(
            20,
            KeyBacking::SecureElement { attestation: vec![0], model: "any".into() },
        );
        let long = history(400, 60);
        assert_eq!(verified_trust_tier(&forged, &NoVendorRoots, None), TrustTier::Unverified);
        assert_eq!(verified_trust_tier(&forged, &NoVendorRoots, Some(&long)), TrustTier::Unverified);
        assert_eq!(verified_trust_tier(&forged, &root_for(21), Some(&long)), TrustTier::Unverified);
    }

    #[test]
    fn one_public_key_registers_as_one_device_only() {
        let mut registry = DeviceRegistry::new();
        registry.register(identity(30, KeyBacking::SoftwareFile)).unwrap();
        // Same key, new device id, same operator.
        let mut again = identity(30, KeyBacking::SoftwareFile);
        again.device_id = "dev-30-bis".into();
        assert_eq!(
            registry.register(again),
            Err(DeviceKeyError::DuplicateKey { existing_device_id: "dev-30".into() })
        );
        // Same key under another operator.
        let mut elsewhere = identity(30, KeyBacking::SoftwareFile);
        elsewhere.device_id = "dev-30-other".into();
        elsewhere.operator = "0xOperatorB".into();
        assert_eq!(
            registry.register(elsewhere),
            Err(DeviceKeyError::DuplicateKey { existing_device_id: "dev-30".into() })
        );
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.devices_for("0xoperatorb"), 0);
        // A different key still registers.
        registry.register(identity(31, KeyBacking::SoftwareFile)).unwrap();
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn devices_per_operator_are_bounded_like_stations_per_identity() {
        let mut registry = DeviceRegistry::with_limit(2);
        registry.register(identity(10, KeyBacking::SoftwareFile)).unwrap();
        registry.register(identity(11, KeyBacking::SoftwareFile)).unwrap();
        assert_eq!(registry.devices_for("0xoperatora"), 2);
        assert_eq!(
            registry.register(identity(12, KeyBacking::SoftwareFile)),
            Err(DeviceKeyError::OperatorDeviceLimit { operator: "0xOperatorA".into(), max: 2 })
        );
        assert_eq!(
            registry.register(identity(10, KeyBacking::SoftwareFile)),
            Err(DeviceKeyError::DuplicateDevice { device_id: "dev-10".into() })
        );
        let mut other = identity(13, KeyBacking::SoftwareFile);
        other.operator = "0xOperatorB".into();
        registry.register(other).unwrap();
        assert_eq!(registry.len(), 3);
        assert_eq!(DeviceRegistry::new().max_per_operator, MAX_DEVICES_PER_OPERATOR);
        let mut blank = identity(14, KeyBacking::SoftwareFile);
        blank.operator = "  ".into();
        assert_eq!(registry.register(blank), Err(DeviceKeyError::Missing { field: "operator" }));
    }

    #[test]
    fn identities_round_trip_with_their_evidence() {
        let id = identity(8, KeyBacking::SecureElement { attestation: vec![1, 2, 3], model: "ATECC608B".into() });
        let json = serde_json::to_string(&id).unwrap();
        assert!(json.contains("\"kind\":\"secureElement\""), "{json}");
        let back: DeviceIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
        assert!(id.backing.claims_hardware());
        assert!(!KeyBacking::SoftwareFile.claims_hardware());
    }
}
