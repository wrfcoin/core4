//! Genesis oracle-registration ceremony construction (#2530).
//!
//! `Transaction::new_oracle_registration` has existed since #2055, but every
//! call site in the tree was inside `#[cfg(test)]`: no CLI command and no HTTP
//! route could produce one. So the block-1 agreement rule (#2488/#2496) had
//! nothing to validate, because the input it validates had no producer outside
//! a test binary. This module is that producer.
//!
//! # Why the whole ceremony is one value
//!
//! The set and quorum are taken **once**, for the whole ceremony, and shared by
//! construction across every emitted transaction. That is deliberate and it is
//! the central design point: the incremental shape — register A at quorum 1,
//! then `[A, B]` at quorum 2 — is the one a drafter reaches for by intuition,
//! and it is correct only if the transactions are applied in the order they
//! were written. Nothing guarantees that. Block-1 transaction order is not
//! stable across processes (#2488), so an incremental ceremony seats one oracle
//! behind an unreachable quorum, on some nodes and not others.
//!
//! There is no API here that can express the incremental shape. It is not
//! rejected by a check; it is unrepresentable.
//!
//! # Determinism
//!
//! Every input that lands in a signature or in `state_root` is an explicit
//! field with no default:
//!
//! - **Chain identity.**
//!   [`crate::blockchain::transaction::oracle_registration_authorization_message`]
//!   folds in `signing_chain_id()`/`signing_network_id()`, which resolve from
//!   process environment with a silent fallback. A ceremony host whose
//!   environment differs from the nodes' signs under a different domain, and
//!   every registration then fails authorization on every peer — with no error
//!   at construction time. Held explicitly here, and echoed in the artifact so
//!   the mismatch is auditable rather than invisible.
//! - **`activated_at`.** The #1775 admin route defaults this to
//!   `chrono::Utc::now().timestamp()` and it lands in `state_root`. Two nodes
//!   running the identical ceremony minutes apart therefore produce different
//!   genesis state roots. Required here.
//! - **`epoch`.** Required; `0` is reserved for legacy identity keys.
//! - **`submitted_at`.** `Transaction::new` stamps `timestamp` from the wall
//!   clock and `calculate_hash` folds it into the txid, so identical ceremony
//!   inputs otherwise yield different transaction ids — different block-1
//!   content, different `state_root`. Required here and pinned onto every
//!   emitted transaction.
//!
//! Given identical inputs, [`GenesisOracleCeremony::build`] is a pure function
//! of those inputs: identical bytes on any machine, in any environment.

use std::collections::{
    BTreeMap,
    BTreeSet,
};
use std::fmt;

use chrono::{
    DateTime,
    Utc,
};
use serde::{
    Deserialize,
    Serialize,
};
use thiserror::Error;

use crate::blockchain::transaction::{
    OracleAttestationPayload,
    Transaction,
};
use crate::crypto::canonical::CanonicalReader;
use crate::crypto::hash::hash_bytes;
use crate::crypto::signature::{
    PrivateKey,
    PublicKey,
    Signature,
};
use crate::crypto::signer::BlockSigner;
use crate::security::key_registry::{
    SigningKeyRegistry,
    SigningRole,
};

/// Discriminant written into every emitted [`GenesisOracleCeremonyArtifact`].
pub const CEREMONY_ARTIFACT_VERSION: &str = "genesis_oracle_ceremony_v1";

/// Why a proposed ceremony cannot be built.
///
/// Every variant is a condition that would otherwise surface as a silent
/// divergence or an abort at block 1, so each one is named rather than folded
/// into a generic string: on cut day the operator needs to know which input to
/// fix, not that "validation failed".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CeremonyError {
    /// The chain identity a signature binds cannot be empty — an empty value
    /// would silently fall back to the process default at verification time.
    EmptyChainIdentity { field: &'static str },
    /// A ceremony with no oracles cannot establish any quorum.
    EmptyOracleSet,
    /// `oracle_set` entries must be ed25519 public-key hex. Operator names such
    /// as `"oracle-n5-pro"` are rejected by `validate_oracle_config` on the
    /// node; catching it here turns a cut-day abort into a typo.
    NotAPublicKey { field: &'static str, value: String },
    /// The same operator listed twice — the node would dedup it and reach a
    /// different set size, and therefore a different quorum relationship, than
    /// the ceremony intended.
    DuplicateOracle { identity: String },
    /// A quorum of zero can be satisfied by nobody attesting anything.
    ZeroQuorum,
    /// `submitted_at` must be a positive absolute timestamp, explicitly chosen.
    NonPositiveSubmittedAt { submitted_at: i64 },
    /// A quorum larger than the set can never be reached.
    QuorumExceedsSet { quorum: usize, set_size: usize },
    /// An operator in the set has no signing key, so it would be seated with
    /// nothing able to sign for it.
    MissingSigningKey { identity: String },
    /// A signing key was supplied for an operator outside the set — almost
    /// always a sign that the set and the key material disagree about who is
    /// participating.
    SigningKeyNotInSet { identity: String },
    /// Epoch `0` is reserved for legacy oracle identity keys.
    ReservedEpoch,
    /// `activated_at` must be a positive absolute timestamp, explicitly chosen.
    NonPositiveActivatedAt { activated_at: i64 },
}

impl fmt::Display for CeremonyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyChainIdentity { field } => {
                write!(f, "{field} must be set explicitly and cannot be empty")
            },
            Self::EmptyOracleSet => write!(f, "oracle_set cannot be empty"),
            Self::NotAPublicKey { field, value } => write!(
                f,
                "{field} entries must be ed25519 public key hex strings, got {value:?}"
            ),
            Self::DuplicateOracle { identity } => {
                write!(f, "oracle_set contains {identity:?} more than once")
            },
            Self::ZeroQuorum => write!(f, "oracle_quorum must be greater than zero"),
            Self::NonPositiveSubmittedAt { submitted_at } => write!(
                f,
                "submitted_at must be a positive timestamp, got {submitted_at}"
            ),
            Self::QuorumExceedsSet { quorum, set_size } => write!(
                f,
                "oracle_quorum {quorum} cannot exceed oracle_set size {set_size}"
            ),
            Self::MissingSigningKey { identity } => {
                write!(f, "no signing key supplied for oracle {identity:?}")
            },
            Self::SigningKeyNotInSet { identity } => write!(
                f,
                "signing key supplied for {identity:?}, which is not in oracle_set"
            ),
            Self::ReservedEpoch => {
                write!(f, "epoch 0 is reserved for legacy oracle identity keys")
            },
            Self::NonPositiveActivatedAt { activated_at } => write!(
                f,
                "activated_at must be a positive timestamp, got {activated_at}"
            ),
        }
    }
}

impl std::error::Error for CeremonyError {}

/// A validated genesis oracle ceremony.
///
/// Construction goes through [`GenesisOracleCeremony::new`], which is the only
/// way to obtain one, so an existing value is already known to describe a
/// reachable quorum over a fully-keyed set.
// Every field is public ceremony content: identities and signing PUBLIC keys,
// the chain identity, and two timestamps. No private key is held here — `build`
// takes the governance private key as a borrowed argument and never stores it.
#[derive(Debug, Clone, PartialEq, Eq)] // secret-leak-ok: `signing_pubkeys` holds ed25519 PUBLIC keys, not secrets
pub struct GenesisOracleCeremony {
    chain_id: String,
    network_id: String,
    oracle_set: Vec<String>,
    oracle_quorum: usize,
    signing_pubkeys: BTreeMap<String, String>,
    epoch: u64,
    activated_at: i64,
    submitted_at: i64,
}

impl GenesisOracleCeremony {
    /// Validate a proposed ceremony.
    ///
    /// `signing_pubkeys` maps each operator identity to the ed25519 public key
    /// it signs attestations with at `epoch`. It must cover the set
    /// exactly: a missing key seats an operator that cannot act, and an
    /// extra key means the set and the key material disagree about who is
    /// participating.
    ///
    /// `activated_at` and `submitted_at` are deliberately separate, because
    /// they are constrained by different things and conflating them would
    /// couple two unrelated decisions:
    ///
    /// - `activated_at` is **consensus content**: it lands in `state_root`, so
    ///   every node must see the same value, and nothing bounds how far it sits
    ///   from block-1 production.
    /// - `submitted_at` becomes each transaction's `timestamp`, which is an
    ///   **admission-window** value: `classify_block_transaction` refuses a
    ///   transaction whose timestamp differs from block time by more than
    ///   `submission_time_window` (default 3600s). It must therefore be chosen
    ///   near when block 1 is actually mined.
    ///
    /// Folding them into one field would force the ceremony to pick its key
    /// activation based on when it expects to finish mining.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chain_id: impl Into<String>,
        network_id: impl Into<String>,
        oracle_set: Vec<String>,
        oracle_quorum: usize,
        signing_pubkeys: BTreeMap<String, String>,
        epoch: u64,
        activated_at: i64,
        submitted_at: i64,
    ) -> Result<Self, CeremonyError> {
        let chain_id = chain_id.into();
        let network_id = network_id.into();
        if chain_id.trim().is_empty() {
            return Err(CeremonyError::EmptyChainIdentity { field: "chain_id" });
        }
        if network_id.trim().is_empty() {
            return Err(CeremonyError::EmptyChainIdentity {
                field: "network_id",
            });
        }
        if oracle_set.is_empty() {
            return Err(CeremonyError::EmptyOracleSet);
        }

        // Normalize every identity through the same parse the node uses, so the
        // ceremony and the chain agree on entry form (case, padding) before any
        // signature binds it.
        let mut normalized = Vec::with_capacity(oracle_set.len());
        for entry in &oracle_set {
            normalized.push(normalize_pubkey(entry, "oracle_set")?);
        }
        normalized.sort();
        for pair in normalized.windows(2) {
            if pair[0] == pair[1] {
                return Err(CeremonyError::DuplicateOracle {
                    identity: pair[0].clone(),
                });
            }
        }

        if oracle_quorum == 0 {
            return Err(CeremonyError::ZeroQuorum);
        }
        if oracle_quorum > normalized.len() {
            return Err(CeremonyError::QuorumExceedsSet {
                quorum: oracle_quorum,
                set_size: normalized.len(),
            });
        }

        let mut normalized_keys = BTreeMap::new();
        for (identity, key) in &signing_pubkeys {
            let identity = normalize_pubkey(identity, "signing key identity")?;
            if !normalized.contains(&identity) {
                return Err(CeremonyError::SigningKeyNotInSet { identity });
            }
            normalized_keys.insert(identity, normalize_pubkey(key, "signing key")?);
        }
        for identity in &normalized {
            if !normalized_keys.contains_key(identity) {
                return Err(CeremonyError::MissingSigningKey {
                    identity: identity.clone(),
                });
            }
        }

        if epoch == 0 {
            return Err(CeremonyError::ReservedEpoch);
        }
        if activated_at <= 0 {
            return Err(CeremonyError::NonPositiveActivatedAt { activated_at });
        }
        if submitted_at <= 0 {
            return Err(CeremonyError::NonPositiveSubmittedAt { submitted_at });
        }

        Ok(Self {
            chain_id,
            network_id,
            oracle_set: normalized,
            oracle_quorum,
            signing_pubkeys: normalized_keys,
            epoch,
            activated_at,
            submitted_at,
        })
    }

    /// The final absolute oracle set, normalized and sorted.
    pub fn oracle_set(&self) -> &[String] {
        &self.oracle_set
    }

    /// The final absolute quorum.
    pub fn oracle_quorum(&self) -> usize {
        self.oracle_quorum
    }

    /// Build one signed registration per operator, in sorted set order.
    ///
    /// Every transaction carries the **identical** absolute `oracle_set` and
    /// `oracle_quorum` — `self.oracle_set` is cloned into each, so there is no
    /// per-transaction set to get wrong — and registers only its own operator's
    /// signing key. This is the shape #2496 requires and the only shape this
    /// type can produce.
    pub fn build(&self, governance_key: &PrivateKey) -> Vec<Transaction> {
        self.oracle_set
            .iter()
            .map(|identity| {
                let signing_key = self
                    .signing_pubkeys
                    .get(identity)
                    .map(String::as_str)
                    // `new` proved the map covers the set, and both are private,
                    // so this branch is unreachable for any constructed value.
                    .unwrap_or(identity.as_str());
                let mut tx = Transaction::new_oracle_registration_with_domain(
                    &self.chain_id,
                    &self.network_id,
                    self.oracle_set.clone(),
                    self.oracle_quorum,
                    identity,
                    signing_key,
                    self.epoch,
                    self.activated_at,
                    governance_key,
                );
                // `Transaction::new` stamps `timestamp` from the wall clock, and
                // `calculate_hash` folds it into the txid. Left alone, two hosts
                // running the identical ceremony emit transactions with
                // different ids — so block 1 differs, and the two nodes reach
                // different state roots. This is the same wall-clock-forks-
                // genesis hazard as the #1775 route's `activated_at` default,
                // one layer down and invisible in the payload bytes.
                tx.timestamp = DateTime::from_timestamp(self.submitted_at, 0)
                    .unwrap_or_else(|| DateTime::<Utc>::from_timestamp_nanos(0));
                tx.set_id();
                tx
            })
            .collect()
    }

    /// Build the transactions and wrap them with the inputs they were signed
    /// under, so a reviewer can check the domain and timing without re-deriving
    /// them from the transaction bytes.
    pub fn build_artifact(&self, governance_key: &PrivateKey) -> GenesisOracleCeremonyArtifact {
        GenesisOracleCeremonyArtifact {
            version: CEREMONY_ARTIFACT_VERSION.to_string(),
            chain_id: self.chain_id.clone(),
            network_id: self.network_id.clone(),
            oracle_set: self.oracle_set.clone(),
            oracle_quorum: self.oracle_quorum,
            epoch: self.epoch,
            activated_at: self.activated_at,
            submitted_at: self.submitted_at,
            governance_pubkey: PublicKey::from_private_key(governance_key).to_hex(),
            transactions: self.build(governance_key),
        }
    }
}

/// The on-disk ceremony output: the signed transactions plus every input they
/// were signed under.
///
/// The echoed inputs are what make the artifact reviewable before any node sees
/// it — in particular `chain_id`/`network_id`, which are otherwise invisible
/// (they are folded into signature preimages, not stored in the payload) and
/// whose mismatch is the failure that produces no diagnostic at all.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisOracleCeremonyArtifact {
    pub version: String,
    pub chain_id: String,
    pub network_id: String,
    pub oracle_set: Vec<String>,
    pub oracle_quorum: usize,
    pub epoch: u64,
    pub activated_at: i64,
    pub submitted_at: i64,
    pub governance_pubkey: String,
    pub transactions: Vec<Transaction>,
}

const POSSESSION_COMMITMENT_DOMAIN: &[u8] = b"wrfcoin_genesis_oracle_possession_v1";
// IEEE-754 binary64 encoding of positive one. Ceremony possession proofs must
// carry this exact bit pattern, not merely any finite confidence value.
const FULL_CONFIDENCE_IEEE754_BITS: u64 = 0x3ff0_0000_0000_0000;

/// Bob's signed block-1 artifact plus live key-possession proofs from its
/// operator nodes. N, K, chain identity, timing, and transaction IDs have one
/// source of truth: `artifact`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisOraclePossessionBundle {
    pub ceremony_id: String,
    pub artifact: GenesisOracleCeremonyArtifact,
    pub possession_proofs: Vec<OracleAttestationPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifiedOraclePossession {
    pub ceremony_id: String,
    pub registration_commitment: String,
    pub transaction_ids: Vec<String>,
    pub oracle_set: Vec<String>,
    pub oracle_quorum: usize,
    pub verified_operator_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OraclePossessionChallenge {
    pub oracle_id: String,
    pub registration_commitment: String,
    pub commit_id: String,
    pub nonce: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OraclePossessionError {
    #[error("ceremony_id is required")]
    MissingCeremonyId,
    #[error("the expected chain_id and network_id are required")]
    MissingExpectedDomain,
    #[error("artifact version is not genesis_oracle_ceremony_v1")]
    UnsupportedArtifact,
    #[error("artifact chain identity does not match the node's expected chain identity")]
    DomainMismatch,
    #[error("artifact oracle_set must be sorted, unique, non-empty canonical ed25519 hex")]
    NonCanonicalOracleSet,
    #[error("artifact oracle_quorum must be between 1 and oracle_set size")]
    InvalidQuorum,
    #[error("artifact must carry exactly one registration transaction per oracle identity")]
    RegistrationCoverage,
    #[error("a registration disagrees with the artifact header")]
    RegistrationDisagreement,
    #[error("a registration transaction ID is absent or does not match its contents")]
    InvalidTransactionId,
    #[error("oracle signing keys must be canonical and distinct across operator nodes")]
    InvalidOrDuplicateSigningKey,
    #[error("registration governance signer is absent from the node's supplied allowlist")]
    UnauthorizedGovernanceSigner,
    #[error("registration governance signature is invalid under the artifact chain domain")]
    InvalidGovernanceSignature,
    #[error("the local oracle signer does not match exactly one artifact registration")]
    SignerNotRegistered,
    #[error("the local oracle signer failed or produced a signature that did not verify")]
    OracleSignerFailure,
    #[error("every final oracle identity must provide exactly one possession proof")]
    ProofCoverage,
    #[error("possession proof does not carry the final registration challenge")]
    WrongChallenge,
    #[error("possession proof must be an approval with exact positive full confidence")]
    InvalidProofShape,
    #[error("possession proof signature is invalid for the registered operator signing key")]
    InvalidPossessionSignature,
}

struct PossessionContext {
    ceremony_id: String,
    registration_commitment: String,
    transaction_ids: Vec<String>,
    oracle_set: Vec<String>,
    oracle_quorum: usize,
    identities: BTreeSet<String>,
    signing_keys: SigningKeyRegistry,
}

fn push_possession_field(material: &mut Vec<u8>, bytes: &[u8]) {
    material.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    material.extend_from_slice(bytes);
}

fn registration_commitment(
    ceremony_id: &str,
    artifact: &GenesisOracleCeremonyArtifact,
    registrations: &[(String, String, String)],
) -> String {
    let mut material = Vec::new();
    material.extend_from_slice(POSSESSION_COMMITMENT_DOMAIN);
    push_possession_field(&mut material, ceremony_id.as_bytes());
    push_possession_field(&mut material, artifact.version.as_bytes());
    push_possession_field(&mut material, artifact.chain_id.as_bytes());
    push_possession_field(&mut material, artifact.network_id.as_bytes());
    material.extend_from_slice(&(artifact.oracle_set.len() as u64).to_le_bytes());
    for identity in &artifact.oracle_set {
        push_possession_field(&mut material, identity.as_bytes());
    }
    material.extend_from_slice(&(artifact.oracle_quorum as u64).to_le_bytes());
    material.extend_from_slice(&artifact.epoch.to_le_bytes());
    material.extend_from_slice(&artifact.activated_at.to_le_bytes());
    push_possession_field(&mut material, artifact.governance_pubkey.as_bytes());
    material.extend_from_slice(&(registrations.len() as u64).to_le_bytes());
    for (identity, signing_key, governance_signature) in registrations {
        push_possession_field(&mut material, identity.as_bytes());
        push_possession_field(&mut material, signing_key.as_bytes());
        push_possession_field(&mut material, governance_signature.as_bytes());
    }
    hash_bytes(&material).to_hex()
}

fn possession_context(
    bundle: &GenesisOraclePossessionBundle,
    expected_chain_id: &str,
    expected_network_id: &str,
    governance_pubkeys: &[String],
) -> Result<PossessionContext, OraclePossessionError> {
    let ceremony_id = bundle.ceremony_id.trim();
    if ceremony_id.is_empty() {
        return Err(OraclePossessionError::MissingCeremonyId);
    }
    if expected_chain_id.trim().is_empty() || expected_network_id.trim().is_empty() {
        return Err(OraclePossessionError::MissingExpectedDomain);
    }
    let artifact = &bundle.artifact;
    if artifact.version != CEREMONY_ARTIFACT_VERSION {
        return Err(OraclePossessionError::UnsupportedArtifact);
    }
    if artifact.chain_id != expected_chain_id || artifact.network_id != expected_network_id {
        return Err(OraclePossessionError::DomainMismatch);
    }

    let mut canonical_set = artifact.oracle_set.clone();
    canonical_set.sort();
    canonical_set.dedup();
    if canonical_set != artifact.oracle_set
        || canonical_set.is_empty()
        || canonical_set.iter().any(|identity| {
            identity.trim() != identity
                || identity.to_ascii_lowercase() != *identity
                || PublicKey::from_hex(identity).is_err()
        })
    {
        return Err(OraclePossessionError::NonCanonicalOracleSet);
    }
    if artifact.oracle_quorum == 0 || artifact.oracle_quorum > artifact.oracle_set.len() {
        return Err(OraclePossessionError::InvalidQuorum);
    }
    if artifact.transactions.len() != artifact.oracle_set.len() {
        return Err(OraclePossessionError::RegistrationCoverage);
    }

    let allowed_governance = governance_pubkeys
        .iter()
        .map(|key| key.trim().to_ascii_lowercase())
        .filter(|key| !key.is_empty())
        .collect::<BTreeSet<_>>();
    if !allowed_governance.contains(&artifact.governance_pubkey.to_ascii_lowercase()) {
        return Err(OraclePossessionError::UnauthorizedGovernanceSigner);
    }

    let identities = artifact.oracle_set.iter().cloned().collect::<BTreeSet<_>>();
    let mut registered_identities = BTreeSet::new();
    let mut registered_signing_keys = BTreeSet::new();
    let mut signing_keys = SigningKeyRegistry::default();
    let mut registrations = Vec::with_capacity(artifact.transactions.len());
    let mut transaction_ids_by_identity = Vec::with_capacity(artifact.transactions.len());

    for tx in &artifact.transactions {
        let Some(payload) = tx.parse_oracle_registration() else {
            return Err(OraclePossessionError::RegistrationDisagreement);
        };
        if payload.oracle_set != artifact.oracle_set
            || payload.oracle_quorum != artifact.oracle_quorum
            || payload.epoch != artifact.epoch
            || payload.activated_at != artifact.activated_at
            || !identities.contains(&payload.identity)
            || !registered_identities.insert(payload.identity.clone())
            || tx.timestamp.timestamp() != artifact.submitted_at
        {
            return Err(OraclePossessionError::RegistrationDisagreement);
        }
        let Some(txid) = tx.id else {
            return Err(OraclePossessionError::InvalidTransactionId);
        };
        if txid != tx.calculate_hash() {
            return Err(OraclePossessionError::InvalidTransactionId);
        }
        if payload.public_key.trim() != payload.public_key
            || payload.public_key.to_ascii_lowercase() != payload.public_key
            || PublicKey::from_hex(&payload.public_key).is_err()
            || !registered_signing_keys.insert(payload.public_key.clone())
        {
            return Err(OraclePossessionError::InvalidOrDuplicateSigningKey);
        }
        if payload.auth_pubkey.as_deref() != Some(artifact.governance_pubkey.as_str()) {
            return Err(OraclePossessionError::UnauthorizedGovernanceSigner);
        }
        if !payload
            .has_valid_authorization_signature_with_domain(&artifact.chain_id, &artifact.network_id)
        {
            return Err(OraclePossessionError::InvalidGovernanceSignature);
        }
        signing_keys
            .register_key_epoch(
                &payload.identity,
                SigningRole::Oracle,
                payload.epoch,
                payload.public_key.clone(),
                payload.activated_at,
            )
            .map_err(|_| OraclePossessionError::InvalidOrDuplicateSigningKey)?;
        registrations.push((
            payload.identity.clone(),
            payload.public_key,
            payload.auth_signature.unwrap_or_default(),
        ));
        transaction_ids_by_identity.push((payload.identity, txid.to_hex()));
    }
    if registered_identities != identities {
        return Err(OraclePossessionError::RegistrationCoverage);
    }
    registrations.sort();
    transaction_ids_by_identity.sort();
    let transaction_ids = transaction_ids_by_identity
        .iter()
        .map(|(_, txid)| txid.clone())
        .collect::<Vec<_>>();
    let registration_commitment = registration_commitment(ceremony_id, artifact, &registrations);

    Ok(PossessionContext {
        ceremony_id: ceremony_id.to_string(),
        registration_commitment,
        transaction_ids,
        oracle_set: artifact.oracle_set.clone(),
        oracle_quorum: artifact.oracle_quorum,
        identities,
        signing_keys,
    })
}

fn possession_challenge(context: &PossessionContext, oracle_id: &str) -> OraclePossessionChallenge {
    OraclePossessionChallenge {
        oracle_id: oracle_id.to_string(),
        registration_commitment: context.registration_commitment.clone(),
        commit_id: format!(
            "genesis-oracle-ceremony:{}:{}",
            context.ceremony_id, context.registration_commitment
        ),
        nonce: format!(
            "genesis-oracle-possession:{}:{}:{}",
            context.ceremony_id, context.registration_commitment, oracle_id
        ),
    }
}

/// Validate the final artifact, then derive the challenge for the registration
/// matching this node's production oracle signer. Identity is never supplied
/// independently on argv.
pub fn oracle_possession_challenge_for_public_key(
    bundle: &GenesisOraclePossessionBundle,
    expected_chain_id: &str,
    expected_network_id: &str,
    governance_pubkeys: &[String],
    signing_public_key: &str,
) -> Result<OraclePossessionChallenge, OraclePossessionError> {
    let context = possession_context(
        bundle,
        expected_chain_id,
        expected_network_id,
        governance_pubkeys,
    )?;
    let mut matches = bundle.artifact.transactions.iter().filter_map(|tx| {
        tx.parse_oracle_registration()
            .filter(|payload| payload.public_key == signing_public_key)
    });
    let Some(payload) = matches.next() else {
        return Err(OraclePossessionError::SignerNotRegistered);
    };
    if matches.next().is_some() {
        return Err(OraclePossessionError::SignerNotRegistered);
    }
    Ok(possession_challenge(&context, &payload.identity))
}

pub fn sign_oracle_possession_proof(
    bundle: &GenesisOraclePossessionBundle,
    expected_chain_id: &str,
    expected_network_id: &str,
    governance_pubkeys: &[String],
    signer: &dyn BlockSigner,
    timestamp: i64,
) -> Result<OracleAttestationPayload, OraclePossessionError> {
    let signing_public = signer.public_key();
    let challenge = oracle_possession_challenge_for_public_key(
        bundle,
        expected_chain_id,
        expected_network_id,
        governance_pubkeys,
        &signing_public.to_hex(),
    )?;
    let mut proof = OracleAttestationPayload {
        commit_id: challenge.commit_id,
        oracle_id: challenge.oracle_id,
        approved: true,
        signature: None,
        confidence: Some(
            CanonicalReader::new(&FULL_CONFIDENCE_IEEE754_BITS.to_le_bytes())
                .read_f64()
                .expect("the full-confidence IEEE-754 bit pattern is finite"),
        ),
        timestamp,
        nonce: Some(challenge.nonce),
    };
    let message =
        proof.canonical_signing_message_with_domain(expected_chain_id, expected_network_id);
    let signature = signer
        .sign(message.as_bytes())
        .map_err(|_| OraclePossessionError::OracleSignerFailure)?;
    if !signing_public.verify(message.as_bytes(), &signature) {
        return Err(OraclePossessionError::OracleSignerFailure);
    }
    proof.signature = Some(signature.to_hex());
    Ok(proof)
}

/// Require live possession from every registered node even when runtime
/// attestation quorum K is smaller than the final operator count N.
pub fn verify_oracle_possession(
    bundle: &GenesisOraclePossessionBundle,
    expected_chain_id: &str,
    expected_network_id: &str,
    governance_pubkeys: &[String],
) -> Result<VerifiedOraclePossession, OraclePossessionError> {
    let context = possession_context(
        bundle,
        expected_chain_id,
        expected_network_id,
        governance_pubkeys,
    )?;
    if bundle.possession_proofs.len() != context.oracle_set.len() {
        return Err(OraclePossessionError::ProofCoverage);
    }
    let mut proven_identities = BTreeSet::new();
    for proof in &bundle.possession_proofs {
        if !context.identities.contains(&proof.oracle_id)
            || !proven_identities.insert(proof.oracle_id.clone())
        {
            return Err(OraclePossessionError::ProofCoverage);
        }
        let expected = possession_challenge(&context, &proof.oracle_id);
        if proof.commit_id != expected.commit_id
            || proof.nonce.as_deref() != Some(expected.nonce.as_str())
        {
            return Err(OraclePossessionError::WrongChallenge);
        }
        if !proof.approved
            || !matches!(
                proof.confidence,
                Some(value) if value.to_bits() == FULL_CONFIDENCE_IEEE754_BITS
            )
            || !proof.is_signable()
            || proof.signature.is_none()
        {
            return Err(OraclePossessionError::InvalidProofShape);
        }
        let message =
            proof.canonical_signing_message_with_domain(expected_chain_id, expected_network_id);
        if !context.signing_keys.verify_signature_for_identity_role(
            &proof.oracle_id,
            SigningRole::Oracle,
            proof.timestamp,
            message.as_bytes(),
            proof.signature.as_deref().unwrap_or_default(),
        ) {
            return Err(OraclePossessionError::InvalidPossessionSignature);
        }
    }
    if proven_identities != context.identities {
        return Err(OraclePossessionError::ProofCoverage);
    }

    Ok(VerifiedOraclePossession {
        ceremony_id: context.ceremony_id,
        registration_commitment: context.registration_commitment,
        transaction_ids: context.transaction_ids,
        oracle_set: context.oracle_set,
        oracle_quorum: context.oracle_quorum,
        verified_operator_count: proven_identities.len(),
    })
}

fn normalize_pubkey(value: &str, field: &'static str) -> Result<String, CeremonyError> {
    let trimmed = value.trim();
    PublicKey::from_hex(trimmed)
        .map(|key| key.to_hex())
        .map_err(|_| CeremonyError::NotAPublicKey {
            field,
            value: trimmed.to_string(),
        })
}


// ---------------------------------------------------------------------------
// #3096 — remote ceremony kit: coordinator-side assembly and producer proofs
// ---------------------------------------------------------------------------

/// Why a proof was refused by [`assemble_possession_bundle`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AssembleError {
    /// The bundle's artifact does not validate (chain, quorum, coverage, …);
    /// carries the underlying possession error.
    #[error("bundle artifact is not assemblable: {0}")]
    InvalidBundle(OraclePossessionError),
    /// Two proofs name the same oracle identity.
    #[error("duplicate proof for oracle identity {oracle_id}")]
    DuplicateIdentity { oracle_id: String },
    /// A proof names an identity the artifact does not register.
    #[error("proof for oracle identity {oracle_id} is not in the artifact's oracle set")]
    UnknownIdentity { oracle_id: String },
    /// A proof was signed over a different artifact (its challenge embeds a
    /// different `registration_commitment`) — the coordinator refreshed the
    /// bundle, or an operator signed the wrong draft.
    #[error(
        "proof for oracle identity {oracle_id} was signed over registration commitment \
         {found}, this bundle's is {expected}"
    )]
    WrongArtifact {
        oracle_id: String,
        expected: String,
        found: String,
    },
}

/// The registration commitment this bundle's challenges embed, derived from
/// the artifact through the public challenge API (any registered key yields
/// the same commitment).
pub fn bundle_registration_commitment(
    bundle: &GenesisOraclePossessionBundle,
) -> Result<String, OraclePossessionError> {
    let governance = vec![bundle.artifact.governance_pubkey.clone()];
    let first_key = bundle
        .artifact
        .transactions
        .iter()
        .find_map(|tx| tx.parse_oracle_registration().map(|p| p.public_key))
        .ok_or(OraclePossessionError::RegistrationCoverage)?;
    oracle_possession_challenge_for_public_key(
        bundle,
        &bundle.artifact.chain_id,
        &bundle.artifact.network_id,
        &governance,
        &first_key,
    )
    .map(|challenge| challenge.registration_commitment)
}

/// Coordinator step (#3096): fold operator-returned possession proofs into
/// the bundle, refusing a duplicate identity, an identity the artifact does
/// not register, and a proof signed over a different artifact. Signature
/// validity is NOT checked here — that is [`verify_oracle_possession`], which
/// the coordinator runs on the result (`assemble --verify`).
///
/// Proofs already in `bundle.possession_proofs` count as present. The result
/// keeps proofs in oracle-identity order so two coordinators assembling the
/// same inputs write byte-identical bundles.
pub fn assemble_possession_bundle(
    bundle: &GenesisOraclePossessionBundle,
    proofs: &[OracleAttestationPayload],
) -> Result<GenesisOraclePossessionBundle, AssembleError> {
    let expected_commitment =
        bundle_registration_commitment(bundle).map_err(AssembleError::InvalidBundle)?;
    let identities: BTreeSet<String> = bundle
        .artifact
        .transactions
        .iter()
        .filter_map(|tx| tx.parse_oracle_registration().map(|p| p.identity))
        .collect();
    let expected_commit_id = format!(
        "genesis-oracle-ceremony:{}:{}",
        bundle.ceremony_id, expected_commitment
    );

    let mut assembled: std::collections::BTreeMap<String, OracleAttestationPayload> =
        std::collections::BTreeMap::new();
    for proof in bundle.possession_proofs.iter().chain(proofs) {
        if !identities.contains(&proof.oracle_id) {
            return Err(AssembleError::UnknownIdentity {
                oracle_id: proof.oracle_id.clone(),
            });
        }
        if proof.commit_id != expected_commit_id {
            // `genesis-oracle-ceremony:<ceremony_id>:<commitment>` — report the
            // commitment the proof was actually signed over.
            let found = proof
                .commit_id
                .rsplit(':')
                .next()
                .unwrap_or(&proof.commit_id)
                .to_string();
            return Err(AssembleError::WrongArtifact {
                oracle_id: proof.oracle_id.clone(),
                expected: expected_commitment,
                found,
            });
        }
        if assembled.contains_key(&proof.oracle_id) {
            return Err(AssembleError::DuplicateIdentity {
                oracle_id: proof.oracle_id.clone(),
            });
        }
        assembled.insert(proof.oracle_id.clone(), proof.clone());
    }
    Ok(GenesisOraclePossessionBundle {
        ceremony_id: bundle.ceremony_id.clone(),
        artifact: bundle.artifact.clone(),
        possession_proofs: assembled.into_values().collect(),
    })
}

/// Domain of the producer proof-of-possession challenge — the same string
/// `producer_key_pop` signs, so a proof made by either tool means the same
/// thing.
pub const PRODUCER_POSSESSION_DOMAIN: &[u8] =
    b"wrfcoin/core4/producer-key-proof-of-possession/v1";

/// A block-producer key's proof that it can sign for THIS ceremony bundle
/// (#3096). Ceremony-only: never a chain object, never gossiped. The
/// challenge binds the ceremony id, the artifact's registration commitment
/// and the timestamp, so a proof cannot be moved to another cut.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProducerPossessionProof {
    pub ceremony_id: String,
    pub registration_commitment: String,
    /// Lowercase hex ed25519 public key of the producer signer.
    pub producer_pubkey: String,
    pub timestamp: i64,
    /// Hex signature over [`producer_possession_message`].
    pub signature: String,
}

/// The exact bytes a producer signs.
pub fn producer_possession_message(
    ceremony_id: &str,
    registration_commitment: &str,
    producer_pubkey: &str,
    timestamp: i64,
) -> Vec<u8> {
    let mut message = Vec::with_capacity(160);
    message.extend_from_slice(PRODUCER_POSSESSION_DOMAIN);
    message.push(0);
    message.extend_from_slice(ceremony_id.as_bytes());
    message.push(0);
    message.extend_from_slice(registration_commitment.as_bytes());
    message.push(0);
    message.extend_from_slice(producer_pubkey.as_bytes());
    message.push(0);
    message.extend_from_slice(&timestamp.to_le_bytes());
    message
}

/// Operator step for the producer role: sign this bundle's challenge with
/// the production producer signer (key file or HSM).
pub fn sign_producer_possession_proof(
    bundle: &GenesisOraclePossessionBundle,
    signer: &dyn BlockSigner,
    timestamp: i64,
) -> Result<ProducerPossessionProof, OraclePossessionError> {
    let registration_commitment = bundle_registration_commitment(bundle)?;
    let producer_pubkey = signer.public_key().to_hex().to_ascii_lowercase();
    let message = producer_possession_message(
        &bundle.ceremony_id,
        &registration_commitment,
        &producer_pubkey,
        timestamp,
    );
    let signature = signer
        .sign(&message)
        .map_err(|_| OraclePossessionError::OracleSignerFailure)?;
    if !signer.public_key().verify(&message, &signature) {
        return Err(OraclePossessionError::OracleSignerFailure);
    }
    Ok(ProducerPossessionProof {
        ceremony_id: bundle.ceremony_id.clone(),
        registration_commitment,
        producer_pubkey,
        timestamp,
        signature: signature.to_hex(),
    })
}

/// Why a producer proof was refused.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProducerProofError {
    #[error("producer proof is for ceremony {found}, this bundle is {expected}")]
    WrongCeremony { expected: String, found: String },
    #[error("producer proof was signed over registration commitment {found}, this bundle's is {expected}")]
    WrongArtifact { expected: String, found: String },
    #[error("producer public key {producer_pubkey} is not in the expected producer set")]
    UnexpectedProducer { producer_pubkey: String },
    #[error("producer proof signature by {producer_pubkey} does not verify")]
    BadSignature {
        producer_pubkey: String,
    },
    #[error("bundle artifact is not verifiable: {0}")]
    InvalidBundle(OraclePossessionError),
}

/// Coordinator step: a producer proof is for this bundle, by an expected
/// producer key (when `expected_producers` is non-empty), and its signature
/// verifies.
pub fn verify_producer_possession_proof(
    bundle: &GenesisOraclePossessionBundle,
    proof: &ProducerPossessionProof,
    expected_producers: &[String],
) -> Result<(), ProducerProofError> {
    if proof.ceremony_id != bundle.ceremony_id {
        return Err(ProducerProofError::WrongCeremony {
            expected: bundle.ceremony_id.clone(),
            found: proof.ceremony_id.clone(),
        });
    }
    let expected_commitment =
        bundle_registration_commitment(bundle).map_err(ProducerProofError::InvalidBundle)?;
    if proof.registration_commitment != expected_commitment {
        return Err(ProducerProofError::WrongArtifact {
            expected: expected_commitment,
            found: proof.registration_commitment.clone(),
        });
    }
    let producer_pubkey = proof.producer_pubkey.trim().to_ascii_lowercase();
    if !expected_producers.is_empty()
        && !expected_producers
            .iter()
            .any(|k| k.trim().eq_ignore_ascii_case(&producer_pubkey))
    {
        return Err(ProducerProofError::UnexpectedProducer { producer_pubkey });
    }
    let bad = || ProducerProofError::BadSignature {
        producer_pubkey: producer_pubkey.clone(),
    };
    let public_key = PublicKey::from_hex(&producer_pubkey).map_err(|_| bad())?;
    let signature = Signature::from_hex(proof.signature.trim()).map_err(|_| bad())?;
    let message = producer_possession_message(
        &proof.ceremony_id,
        &proof.registration_commitment,
        &producer_pubkey,
        proof.timestamp,
    );
    if !public_key.verify(&message, &signature) {
        return Err(bad());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::domain_signing::{
        ORACLE_GOVERNANCE_ENV_TEST_LOCK,
        ORACLE_GOVERNANCE_PUBKEYS_ENV,
    };

    const CHAIN: &str = "wrfcoin-cut";
    const NETWORK: &str = "testnet";
    const EPOCH: u64 = 1;
    const ACTIVATED_AT: i64 = 1_753_660_800;
    const SUBMITTED_AT: i64 = 1_753_660_900;

    /// Deterministic keypair so every assertion below is reproducible.
    fn keypair(seed: u8) -> (PrivateKey, String) {
        let sk = PrivateKey::from_bytes(&[seed; 32]).expect("seed is a valid ed25519 scalar");
        let pk = PublicKey::from_private_key(&sk).to_hex();
        (sk, pk)
    }

    /// The settled genesis shape: three operators, quorum two (#724 owner
    /// decision). N and K are fixture values here, never defaults in the code.
    fn three_node_ceremony() -> (GenesisOracleCeremony, PrivateKey, Vec<String>) {
        let (_, id_a) = keypair(0xA1);
        let (_, id_b) = keypair(0xB2);
        let (_, id_c) = keypair(0xC3);
        let (_, sign_a) = keypair(0x1A);
        let (_, sign_b) = keypair(0x2B);
        let (_, sign_c) = keypair(0x3C);
        let (gov_sk, _) = keypair(0x60);

        let set = vec![id_a.clone(), id_b.clone(), id_c.clone()];
        let keys = BTreeMap::from([
            (id_a.clone(), sign_a),
            (id_b.clone(), sign_b),
            (id_c.clone(), sign_c),
        ]);
        let ceremony = GenesisOracleCeremony::new(
            CHAIN,
            NETWORK,
            set.clone(),
            2,
            keys,
            EPOCH,
            ACTIVATED_AT,
            SUBMITTED_AT,
        )
        .expect("the settled 3-node / 2-of-3 genesis shape must validate");
        (ceremony, gov_sk, set)
    }

    /// The #2496 contract, from the producing side: every block-1 registration
    /// carries the identical FINAL absolute set and quorum, and each registers
    /// only its own operator.
    ///
    /// This is the whole point of the issue. #2496 rejects blocks whose
    /// registrations disagree; until now nothing outside a test binary could
    /// build a block that agrees.
    #[test]
    fn every_registration_carries_the_identical_final_set_and_quorum() {
        let (ceremony, gov_sk, set) = three_node_ceremony();
        let txs = ceremony.build(&gov_sk);
        assert_eq!(txs.len(), 3, "one registration per operator");

        let mut expected_set = set.clone();
        expected_set.sort();

        let mut registered_identities = Vec::new();
        for tx in &txs {
            let payload = tx
                .parse_oracle_registration()
                .expect("every emitted tx must parse as an oracle registration");
            assert_eq!(
                payload.oracle_set, expected_set,
                "every registration must carry the identical final absolute set"
            );
            assert_eq!(
                payload.oracle_quorum, 2,
                "every registration must carry the identical final quorum"
            );
            assert!(
                payload.oracle_set.contains(&payload.identity),
                "an operator must register itself into the set it declares"
            );
            registered_identities.push(payload.identity);
        }
        registered_identities.sort();
        assert_eq!(
            registered_identities, expected_set,
            "the ceremony must register every operator exactly once"
        );
    }

    /// The hazard shape, named so its absence is asserted rather than assumed.
    ///
    /// The incremental ceremony — A alone at quorum 1, then `[A, B]` at quorum
    /// 2 — is correct only if the transactions apply in the order written,
    /// and block-1 order is not stable across processes (#2488). No emitted
    /// registration may carry a proper subset of the final set, and none may
    /// carry a quorum below the final one.
    #[test]
    fn no_emitted_registration_carries_an_incremental_set_or_quorum() {
        let (ceremony, gov_sk, _) = three_node_ceremony();
        let full = ceremony.oracle_set().len();
        for tx in ceremony.build(&gov_sk) {
            let payload = tx.parse_oracle_registration().expect("parses");
            assert_eq!(
                payload.oracle_set.len(),
                full,
                "a registration carrying a proper subset seats an oracle behind \
                 a quorum that depends on apply order"
            );
            assert_eq!(
                payload.oracle_quorum,
                ceremony.oracle_quorum(),
                "a registration carrying a lower quorum ratchets K below the \
                 intended value if it applies last"
            );
        }
    }

    /// Determinism: the property that makes a multi-node ceremony possible.
    /// Two hosts running the identical ceremony must emit byte-identical
    /// transactions, or they seat different genesis content.
    ///
    /// The timestamp assertion is the load-bearing one and it is deliberately
    /// not "build twice and compare". `calculate_hash` folds
    /// `timestamp_millis()`, so two builds in the same millisecond agree even
    /// when the value is wall-clock — comparing two builds catches the defect
    /// only when they happen to straddle a millisecond boundary. Measured: with
    /// the pin removed, the comparison-only form passed. Pin the mechanism
    /// instead of sampling its output.
    #[test]
    fn identical_inputs_produce_byte_identical_transactions() {
        let (ceremony, gov_sk, _) = three_node_ceremony();
        let first = ceremony.build(&gov_sk);
        let second = ceremony.build(&gov_sk);
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.id, b.id, "transaction ids must be reproducible");
            assert_eq!(a.data, b.data, "signed payload bytes must be reproducible");
            assert_eq!(
                a.timestamp.timestamp(),
                SUBMITTED_AT,
                "the emitted timestamp must be the declared submitted_at, not \
                 the wall clock — a wall-clock stamp folds into the txid and \
                 forks block 1 between two hosts running the same ceremony"
            );
        }
    }

    /// `submitted_at` reaches the txid, so two ceremonies differing only in it
    /// are different transactions. Pairs with the assertion above: that one
    /// pins the value, this one pins that the value matters.
    #[test]
    fn submitted_at_binds_the_emitted_registrations() {
        let (ceremony, gov_sk, set) = three_node_ceremony();
        let baseline = ceremony.build(&gov_sk);

        let mut keys = BTreeMap::new();
        for (i, id) in set.iter().enumerate() {
            let (_, signing) = keypair([0x1A, 0x2B, 0x3C][i]);
            keys.insert(id.clone(), signing);
        }
        let later = GenesisOracleCeremony::new(
            CHAIN,
            NETWORK,
            set,
            2,
            keys,
            EPOCH,
            ACTIVATED_AT,
            SUBMITTED_AT + 1,
        )
        .expect("a later submission time is still a valid ceremony");
        for (base, moved) in baseline.iter().zip(later.build(&gov_sk).iter()) {
            assert_ne!(
                base.id, moved.id,
                "a one-second difference in submitted_at must change the txid"
            );
        }
    }

    /// The chain identity is folded into the signature preimage and into the
    /// txid, but appears nowhere in the payload — so a mismatch between the
    /// ceremony host and the nodes is invisible until every registration fails
    /// authorization on every peer. Pin that it actually binds.
    #[test]
    fn chain_identity_binds_the_emitted_registrations() {
        let (ceremony, gov_sk, set) = three_node_ceremony();
        let baseline = ceremony.build(&gov_sk);

        for (chain, network) in [(CHAIN, "mainnet"), ("wrfcoin-other", NETWORK)] {
            let mut keys = BTreeMap::new();
            for (i, id) in set.iter().enumerate() {
                let (_, signing) = keypair([0x1A, 0x2B, 0x3C][i]);
                keys.insert(id.clone(), signing);
            }
            let other = GenesisOracleCeremony::new(
                chain,
                network,
                set.clone(),
                2,
                keys,
                EPOCH,
                ACTIVATED_AT,
                SUBMITTED_AT,
            )
            .expect("a different chain identity is still a valid ceremony");
            let shifted = other.build(&gov_sk);
            for (base, moved) in baseline.iter().zip(shifted.iter()) {
                assert_ne!(
                    base.data, moved.data,
                    "chain identity {chain}/{network} must change the signature"
                );
                assert_ne!(
                    base.id, moved.id,
                    "chain identity {chain}/{network} must change the txid"
                );
            }
        }
    }

    /// `activated_at` lands in `state_root`, so two hosts choosing it
    /// independently fork genesis. Pin that it binds, which is what makes
    /// requiring it (rather than defaulting to wall clock) load-bearing.
    #[test]
    fn activated_at_binds_the_emitted_registrations() {
        let (ceremony, gov_sk, set) = three_node_ceremony();
        let baseline = ceremony.build(&gov_sk);

        let mut keys = BTreeMap::new();
        for (i, id) in set.iter().enumerate() {
            let (_, signing) = keypair([0x1A, 0x2B, 0x3C][i]);
            keys.insert(id.clone(), signing);
        }
        let later = GenesisOracleCeremony::new(
            CHAIN,
            NETWORK,
            set,
            2,
            keys,
            EPOCH,
            ACTIVATED_AT + 1,
            SUBMITTED_AT,
        )
        .expect("a later activation is still a valid ceremony");
        for (base, moved) in baseline.iter().zip(later.build(&gov_sk).iter()) {
            assert_ne!(
                base.data, moved.data,
                "a one-second difference must change the signed payload"
            );
        }
    }

    /// Restores an env var on drop, including on the panic path — a helper that
    /// restores only on success turns one failing assertion into a cascade of
    /// unrelated failures in the rest of the process.
    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            // SAFETY: the caller holds ORACLE_GOVERNANCE_ENV_TEST_LOCK, so no
            // other test writes the environment concurrently.
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: as above — the lock outlives this guard.
            unsafe {
                match self.previous.take() {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    /// The emitted registrations must authorize on a node that allowlists the
    /// governance key — otherwise the tool produces transactions block 1 will
    /// refuse. Verification resolves the chain identity from the environment,
    /// so this also pins that the tool's explicit domain matches the default
    /// one a node with no override would use.
    #[test]
    fn emitted_registrations_authorize_under_an_allowlisted_governance_key() {
        let _lock = ORACLE_GOVERNANCE_ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let (_, id_a) = keypair(0xA1);
        let (_, id_b) = keypair(0xB2);
        let (_, sign_a) = keypair(0x1A);
        let (_, sign_b) = keypair(0x2B);
        let (gov_sk, gov_pk) = keypair(0x60);
        let _env = EnvGuard::set(ORACLE_GOVERNANCE_PUBKEYS_ENV, &gov_pk);

        let ceremony = GenesisOracleCeremony::new(
            crate::crypto::domain_signing::signing_chain_id(),
            crate::crypto::domain_signing::signing_network_id(),
            vec![id_a.clone(), id_b.clone()],
            2,
            BTreeMap::from([(id_a, sign_a), (id_b, sign_b)]),
            EPOCH,
            ACTIVATED_AT,
            SUBMITTED_AT,
        )
        .expect("two-operator ceremony validates");

        for tx in ceremony.build(&gov_sk) {
            assert!(
                tx.verify_oracle_registration_authorization(),
                "an emitted registration must authorize under the allowlisted \
                 governance key"
            );
        }
    }

    /// An unallowlisted governance key must not authorize — the fail-closed
    /// direction, so the test above cannot pass by the check being inert.
    #[test]
    fn registrations_signed_by_an_unallowlisted_key_do_not_authorize() {
        let _lock = ORACLE_GOVERNANCE_ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let (_, id_a) = keypair(0xA1);
        let (_, sign_a) = keypair(0x1A);
        let (rogue_sk, _) = keypair(0x99);
        let (_, allowlisted_pk) = keypair(0x60);
        let _env = EnvGuard::set(ORACLE_GOVERNANCE_PUBKEYS_ENV, &allowlisted_pk);

        let ceremony = GenesisOracleCeremony::new(
            crate::crypto::domain_signing::signing_chain_id(),
            crate::crypto::domain_signing::signing_network_id(),
            vec![id_a.clone()],
            1,
            BTreeMap::from([(id_a, sign_a)]),
            EPOCH,
            ACTIVATED_AT,
            SUBMITTED_AT,
        )
        .expect("single-operator ceremony validates");

        for tx in ceremony.build(&rogue_sk) {
            assert!(
                !tx.verify_oracle_registration_authorization(),
                "a registration signed by an unallowlisted key must be refused"
            );
        }
    }

    fn valid_inputs() -> (Vec<String>, BTreeMap<String, String>) {
        let (_, id_a) = keypair(0xA1);
        let (_, sign_a) = keypair(0x1A);
        (vec![id_a.clone()], BTreeMap::from([(id_a, sign_a)]))
    }

    /// Each rejection names the input the operator has to fix. Every row here
    /// is a condition that would otherwise surface as a cut-day abort or a
    /// silent divergence, including the two the rehearsal actually hit: an
    /// operator name where a pubkey belongs, and a quorum that cannot be met.
    #[test]
    fn invalid_ceremonies_are_refused_with_the_offending_input_named() {
        let (set, keys) = valid_inputs();
        let (_, other) = keypair(0xB2);

        let build = |set: Vec<String>,
                     quorum: usize,
                     keys: BTreeMap<String, String>,
                     epoch: u64,
                     activated_at: i64,
                     chain: &str,
                     network: &str| {
            GenesisOracleCeremony::new(
                chain,
                network,
                set,
                quorum,
                keys,
                epoch,
                activated_at,
                SUBMITTED_AT,
            )
            .expect_err("this ceremony must be refused")
        };

        assert_eq!(
            build(vec![], 1, keys.clone(), EPOCH, ACTIVATED_AT, CHAIN, NETWORK),
            CeremonyError::EmptyOracleSet
        );
        // The rehearsal's actual abort: an operator name where a key belongs.
        assert_eq!(
            build(
                vec!["oracle-n5-pro".to_string()],
                1,
                keys.clone(),
                EPOCH,
                ACTIVATED_AT,
                CHAIN,
                NETWORK
            ),
            CeremonyError::NotAPublicKey {
                field: "oracle_set",
                value: "oracle-n5-pro".to_string()
            }
        );
        assert_eq!(
            build(
                vec![set[0].clone(), set[0].clone()],
                1,
                keys.clone(),
                EPOCH,
                ACTIVATED_AT,
                CHAIN,
                NETWORK
            ),
            CeremonyError::DuplicateOracle {
                identity: set[0].clone()
            }
        );
        assert_eq!(
            build(
                set.clone(),
                0,
                keys.clone(),
                EPOCH,
                ACTIVATED_AT,
                CHAIN,
                NETWORK
            ),
            CeremonyError::ZeroQuorum
        );
        // The #1775 route's unrecoverable state, refused up front.
        assert_eq!(
            build(
                set.clone(),
                2,
                keys.clone(),
                EPOCH,
                ACTIVATED_AT,
                CHAIN,
                NETWORK
            ),
            CeremonyError::QuorumExceedsSet {
                quorum: 2,
                set_size: 1
            }
        );
        assert_eq!(
            // secret-leak-ok: `identity` is an ed25519 PUBLIC key, not a secret
            build(
                set.clone(),
                1,
                BTreeMap::new(),
                EPOCH,
                ACTIVATED_AT,
                CHAIN,
                NETWORK
            ),
            CeremonyError::MissingSigningKey {
                identity: set[0].clone()
            }
        );
        let mut extra = keys.clone();
        extra.insert(other.clone(), other.clone());
        assert_eq!(
            // secret-leak-ok: `identity` is an ed25519 PUBLIC key, not a secret
            build(set.clone(), 1, extra, EPOCH, ACTIVATED_AT, CHAIN, NETWORK),
            CeremonyError::SigningKeyNotInSet { identity: other }
        );
        assert_eq!(
            build(
                set.clone(),
                1,
                keys.clone(),
                0,
                ACTIVATED_AT,
                CHAIN,
                NETWORK
            ),
            CeremonyError::ReservedEpoch
        );
        assert_eq!(
            build(set.clone(), 1, keys.clone(), EPOCH, 0, CHAIN, NETWORK),
            CeremonyError::NonPositiveActivatedAt { activated_at: 0 }
        );
        assert_eq!(
            GenesisOracleCeremony::new(
                CHAIN,
                NETWORK,
                set.clone(),
                1,
                keys.clone(),
                EPOCH,
                ACTIVATED_AT,
                0,
            )
            .expect_err("a defaulted submission timestamp must be refused"),
            CeremonyError::NonPositiveSubmittedAt { submitted_at: 0 }
        );
        assert_eq!(
            build(
                set.clone(),
                1,
                keys.clone(),
                EPOCH,
                ACTIVATED_AT,
                "  ",
                NETWORK
            ),
            CeremonyError::EmptyChainIdentity { field: "chain_id" }
        );
        assert_eq!(
            build(set, 1, keys, EPOCH, ACTIVATED_AT, CHAIN, ""),
            CeremonyError::EmptyChainIdentity {
                field: "network_id"
            }
        );
    }

    /// The artifact echoes every input a signature binds, including the chain
    /// identity that appears nowhere in the payload. Without it a reviewer
    /// cannot tell a correctly-domained ceremony from one that will fail on
    /// every node, because the bytes look the same either way.
    #[test]
    fn the_artifact_records_every_input_the_signatures_bind() {
        let (ceremony, gov_sk, _) = three_node_ceremony();
        let artifact = ceremony.build_artifact(&gov_sk);

        assert_eq!(artifact.version, CEREMONY_ARTIFACT_VERSION);
        assert_eq!(artifact.chain_id, CHAIN);
        assert_eq!(artifact.network_id, NETWORK);
        assert_eq!(artifact.oracle_quorum, 2);
        assert_eq!(artifact.oracle_set.len(), 3);
        assert_eq!(artifact.epoch, EPOCH);
        assert_eq!(artifact.activated_at, ACTIVATED_AT);
        assert_eq!(
            // secret-leak-ok: compares derived PUBLIC keys; no secret in the message
            artifact.governance_pubkey,
            PublicKey::from_private_key(&gov_sk).to_hex()
        );
        assert_eq!(artifact.transactions.len(), 3);

        let json = serde_json::to_string(&artifact).expect("artifact serializes");
        let restored: GenesisOracleCeremonyArtifact =
            serde_json::from_str(&json).expect("artifact round-trips");
        assert_eq!(restored.chain_id, artifact.chain_id);
        assert_eq!(restored.transactions.len(), 3);
        for (before, after) in artifact
            .transactions
            .iter()
            .zip(restored.transactions.iter())
        {
            assert_eq!(before.id, after.id, "txids survive the round trip");
            assert_eq!(
                before.data, after.data,
                "signed payloads survive the round trip"
            );
        }
    }
}

#[cfg(test)]
mod possession_tests {
    use super::*;
    use crate::crypto::signature::PrivateKey;
    use crate::crypto::signer::EnvKeySigner;

    const CHAIN: &str = "wrfcoin-cut";
    const NETWORK: &str = "testnet";
    const CEREMONY_ID: &str = "genesis-cut-2026-07";

    fn key(seed: u8) -> PrivateKey {
        PrivateKey::from_bytes(&[seed; 32]).expect("fixed test key")
    }

    fn artifact(
        activated_at: i64,
        submitted_at: i64,
    ) -> (GenesisOracleCeremonyArtifact, Vec<PrivateKey>) {
        let mut identities = (1..=3)
            .map(|seed| PublicKey::from_private_key(&key(seed)).to_hex())
            .collect::<Vec<_>>();
        identities.sort();
        let signing_keys = vec![key(11), key(12), key(13)];
        let signing_pubkeys = identities
            .iter()
            .zip(&signing_keys)
            .map(|(identity, signing_key)| {
                (
                    identity.clone(),
                    PublicKey::from_private_key(signing_key).to_hex(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let ceremony = GenesisOracleCeremony::new(
            CHAIN,
            NETWORK,
            identities,
            2,
            signing_pubkeys,
            1,
            activated_at,
            submitted_at,
        )
        .expect("fixture ceremony is valid");
        (ceremony.build_artifact(&key(90)), signing_keys)
    }

    fn signed_bundle() -> GenesisOraclePossessionBundle {
        let (artifact, signing_keys) = artifact(100, 101);
        let governance = vec![artifact.governance_pubkey.clone()];
        let mut bundle = GenesisOraclePossessionBundle {
            ceremony_id: CEREMONY_ID.to_string(),
            artifact,
            possession_proofs: Vec::new(),
        };
        bundle.possession_proofs = signing_keys
            .into_iter()
            .map(|private_key| {
                let signer = EnvKeySigner::from_private_key(private_key, "test-only-oracle-key");
                sign_oracle_possession_proof(&bundle, CHAIN, NETWORK, &governance, &signer, 102)
                    .expect("fixture signer matches one registration")
            })
            .collect();
        bundle
    }

    fn governance_allowlist(bundle: &GenesisOraclePossessionBundle) -> Vec<String> {
        vec![bundle.artifact.governance_pubkey.clone()]
    }

    #[test]
    fn all_three_operator_nodes_verify_at_two_of_three_runtime_quorum() {
        let bundle = signed_bundle();
        let verified =
            verify_oracle_possession(&bundle, CHAIN, NETWORK, &governance_allowlist(&bundle))
                .expect("all three operator nodes prove their artifact signing keys");
        assert_eq!(verified.oracle_set.len(), 3);
        assert_eq!(verified.oracle_quorum, 2);
        assert_eq!(verified.verified_operator_count, 3);
        assert_eq!(verified.transaction_ids.len(), 3);
        assert_eq!(verified.registration_commitment.len(), 64);
    }

    #[test]
    fn runtime_quorum_is_not_enough_for_ceremony_key_verification() {
        let mut bundle = signed_bundle();
        bundle.possession_proofs.pop();
        assert_eq!(
            verify_oracle_possession(&bundle, CHAIN, NETWORK, &governance_allowlist(&bundle),),
            Err(OraclePossessionError::ProofCoverage)
        );
    }

    #[test]
    fn duplicate_proof_identity_is_rejected_by_coverage_guard() {
        let mut bundle = signed_bundle();
        bundle.possession_proofs[2] = bundle.possession_proofs[1].clone();
        assert_eq!(
            verify_oracle_possession(&bundle, CHAIN, NETWORK, &governance_allowlist(&bundle),),
            Err(OraclePossessionError::ProofCoverage)
        );
    }

    #[test]
    fn unapproved_proof_shape_is_rejected_before_signature_verification() {
        let mut bundle = signed_bundle();
        bundle.possession_proofs[0].approved = false;
        assert_eq!(
            verify_oracle_possession(&bundle, CHAIN, NETWORK, &governance_allowlist(&bundle),),
            Err(OraclePossessionError::InvalidProofShape)
        );
    }

    #[test]
    fn missing_or_nonfinite_proof_shape_maps_to_invalid_shape() {
        let mut missing_signature = signed_bundle();
        missing_signature.possession_proofs[0].signature = None;
        assert_eq!(
            verify_oracle_possession(
                &missing_signature,
                CHAIN,
                NETWORK,
                &governance_allowlist(&missing_signature),
            ),
            Err(OraclePossessionError::InvalidProofShape)
        );

        let mut nonfinite_confidence = signed_bundle();
        nonfinite_confidence.possession_proofs[0].confidence = Some(f64::NAN);
        assert_eq!(
            verify_oracle_possession(
                &nonfinite_confidence,
                CHAIN,
                NETWORK,
                &governance_allowlist(&nonfinite_confidence),
            ),
            Err(OraclePossessionError::InvalidProofShape)
        );
    }

    #[test]
    fn possession_confidence_requires_exact_positive_one_bits() {
        let exact = signed_bundle();
        assert_eq!(
            verify_oracle_possession(&exact, CHAIN, NETWORK, &governance_allowlist(&exact))
                .expect("exact positive one confidence must verify")
                .verified_operator_count,
            3
        );

        // Every rejection case is built from an explicit binary64 bit pattern,
        // so the test cannot accidentally exercise approximate float arithmetic.
        let confidence_from_bits = |bits: u64| {
            bincode::deserialize(&bits.to_le_bytes()).expect("binary64 test fixture")
        };
        for bits in [
            FULL_CONFIDENCE_IEEE754_BITS - 1, // immediately below +1.0
            FULL_CONFIDENCE_IEEE754_BITS + 1, // immediately above +1.0
            0x0000_0000_0000_0000,            // +0.0
            0x8000_0000_0000_0000,            // -0.0
            0xbff0_0000_0000_0000,            // -1.0
            0x7ff0_0000_0000_0000,            // +infinity
            0xfff0_0000_0000_0000,            // -infinity
            0x7ff8_0000_0000_0000,            // quiet NaN
        ] {
            let mut invalid = signed_bundle();
            invalid.possession_proofs[0].confidence = Some(confidence_from_bits(bits));
            assert_eq!(
                verify_oracle_possession(
                    &invalid,
                    CHAIN,
                    NETWORK,
                    &governance_allowlist(&invalid),
                ),
                Err(OraclePossessionError::InvalidProofShape),
                "confidence bits {bits:#018x} must be rejected"
            );
        }
    }

    #[test]
    fn missing_possession_confidence_is_rejected_by_real_verifier() {
        let mut missing = signed_bundle();
        missing.possession_proofs[0].confidence = None;

        assert_eq!(
            verify_oracle_possession(
                &missing,
                CHAIN,
                NETWORK,
                &governance_allowlist(&missing),
            ),
            Err(OraclePossessionError::InvalidProofShape),
            "missing confidence must fail closed at the possession verifier",
        );
    }

    #[test]
    fn relabeled_registered_proofs_with_original_signatures_are_rejected() {
        let mut bundle = signed_bundle();
        let context = possession_context(&bundle, CHAIN, NETWORK, &governance_allowlist(&bundle))
            .expect("fixture has a valid registration context");
        let first_identity = bundle.possession_proofs[0].oracle_id.clone();
        let second_identity = bundle.possession_proofs[1].oracle_id.clone();
        let (first_proof, remaining_proofs) = bundle.possession_proofs.split_at_mut(1);
        for (proof, replacement) in [
            (&mut first_proof[0], second_identity),
            (&mut remaining_proofs[0], first_identity),
        ] {
            let challenge = possession_challenge(&context, &replacement);
            proof.oracle_id = replacement;
            proof.commit_id = challenge.commit_id;
            proof.nonce = Some(challenge.nonce);
        }
        assert_eq!(
            verify_oracle_possession(&bundle, CHAIN, NETWORK, &governance_allowlist(&bundle),),
            Err(OraclePossessionError::InvalidPossessionSignature)
        );
    }

    #[test]
    fn proof_and_registration_order_do_not_change_verified_result() {
        let baseline = signed_bundle();
        let expected = verify_oracle_possession(
            &baseline,
            CHAIN,
            NETWORK,
            &governance_allowlist(&baseline),
        )
        .expect("baseline verifies");
        let mut reordered = baseline;
        reordered.possession_proofs.reverse();
        reordered.artifact.transactions.reverse();
        assert_eq!(
            verify_oracle_possession(
                &reordered,
                CHAIN,
                NETWORK,
                &governance_allowlist(&reordered),
            ),
            Ok(expected)
        );
    }

    #[test]
    fn corrupted_registration_authorization_signature_is_rejected() {
        let mut bundle = signed_bundle();
        let mut registration = bundle.artifact.transactions[0]
            .parse_oracle_registration()
            .expect("fixture has an oracle registration");
        registration.auth_signature = Some("00".repeat(64));
        bundle.artifact.transactions[0].data = Some(
            serde_json::to_string(&registration).expect("registration serializes"),
        );
        bundle.artifact.transactions[0].set_id();
        assert_eq!(
            verify_oracle_possession(&bundle, CHAIN, NETWORK, &governance_allowlist(&bundle),),
            Err(OraclePossessionError::InvalidGovernanceSignature)
        );
    }

    #[test]
    fn refreshed_submitted_at_and_transaction_ids_preserve_key_possession_proofs() {
        let mut bundle = signed_bundle();
        let old_report =
            verify_oracle_possession(&bundle, CHAIN, NETWORK, &governance_allowlist(&bundle))
                .expect("initial artifact verifies");
        let (changed_artifact, _) = artifact(100, 103);
        bundle.artifact = changed_artifact;
        let refreshed =
            verify_oracle_possession(&bundle, CHAIN, NETWORK, &governance_allowlist(&bundle))
                .expect("refreshing submitted_at must not require three operators to re-sign");
        assert_ne!(refreshed.transaction_ids, old_report.transaction_ids);
        assert_eq!(
            refreshed.registration_commitment,
            old_report.registration_commitment
        );
    }

    #[test]
    fn proofs_are_bound_to_stable_governance_signed_registration_content() {
        let mut bundle = signed_bundle();
        let (changed_artifact, _) = artifact(101, 103);
        bundle.artifact = changed_artifact;
        assert_eq!(
            verify_oracle_possession(&bundle, CHAIN, NETWORK, &governance_allowlist(&bundle),),
            Err(OraclePossessionError::WrongChallenge),
            "changing activated_at must invalidate proofs even after governance re-signing"
        );
    }

    #[test]
    fn corrupted_artifact_transaction_id_is_rejected_before_proof_verification() {
        let mut bundle = signed_bundle();
        bundle.artifact.transactions[0].fee += 1;
        assert_eq!(
            verify_oracle_possession(&bundle, CHAIN, NETWORK, &governance_allowlist(&bundle),),
            Err(OraclePossessionError::InvalidTransactionId)
        );
    }

    #[test]
    fn substituted_operator_signature_is_rejected() {
        let mut bundle = signed_bundle();
        bundle.possession_proofs[2].signature = bundle.possession_proofs[1].signature.clone();
        assert_eq!(
            verify_oracle_possession(&bundle, CHAIN, NETWORK, &governance_allowlist(&bundle),),
            Err(OraclePossessionError::InvalidPossessionSignature)
        );
    }

    #[test]
    fn local_runtime_signer_must_match_the_final_artifact() {
        let bundle = signed_bundle();
        let unknown_public = PublicKey::from_private_key(&key(44)).to_hex();
        assert_eq!(
            oracle_possession_challenge_for_public_key(
                &bundle,
                CHAIN,
                NETWORK,
                &governance_allowlist(&bundle),
                &unknown_public,
            ),
            Err(OraclePossessionError::SignerNotRegistered)
        );
    }

    #[test]
    fn artifact_domain_must_match_the_node_domain() {
        let bundle = signed_bundle();
        assert_eq!(
            verify_oracle_possession(
                &bundle,
                "another-chain",
                NETWORK,
                &governance_allowlist(&bundle),
            ),
            Err(OraclePossessionError::DomainMismatch)
        );
    }

    #[test]
    fn artifact_governance_key_must_be_in_the_node_allowlist() {
        let bundle = signed_bundle();
        assert_eq!(
            verify_oracle_possession(&bundle, CHAIN, NETWORK, &[]),
            Err(OraclePossessionError::UnauthorizedGovernanceSigner)
        );
    }
}
