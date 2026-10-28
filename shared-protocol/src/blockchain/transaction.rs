use crate::accounts::authorization::{EmptyAuthorizationView, SpendAuthorization, authorize_debit};
use crate::common::types::{Address, Timestamp};
use crate::crypto::domain_signing::{
    DomainSigningAction, DomainSigningContext, canonical_signing_message,
    is_authorized_gamification_operator, is_authorized_oracle_governance, payload_digest_hex,
    signing_chain_id, signing_network_id,
};
use crate::crypto::hash::{Hash, hash_bytes};
use crate::crypto::signature::{PrivateKey, PublicKey, Signature};
use crate::security::key_registry::{SigningKeyRegistry, SigningRole};
use crate::station_registry::{ContributionProvenanceClass, DataQualityTier};
use chrono::{DateTime, Utc};
use ed25519_dalek::Verifier;
use crate::weather::quality_tier::ReadingQualityTier;
use serde::{Deserialize, Serialize};
use std::fmt;

pub type TransactionHash = Hash;
pub type TransactionData = Option<String>;

/// Transaction operations kept for compatibility with modules that still use the legacy operation model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Operation {
    /// Transfer tokens
    Transfer { amount: u64 },

    /// Create NFT
    CreateNFT {
        nft_id: String,
        metadata: serde_json::Value,
    },

    /// Transfer NFT
    TransferNFT { nft_id: String },

    /// Stake tokens
    Stake { amount: u64 },

    /// Unstake tokens
    Unstake { amount: u64 },

    /// Register validator
    RegisterValidator,

    /// Execute smart contract
    ExecuteContract {
        contract_address: Address,
        method: String,
        args: Vec<u8>,
    },

    /// Deploy smart contract
    DeployContract { code: Vec<u8>, init_args: Vec<u8> },

    /// Vote on governance proposal
    Vote { proposal_id: u64, approve: bool },

    /// Create governance proposal
    CreateProposal {
        title: String,
        description: String,
        actions: Vec<u8>,
    },

    /// Cross-chain transfer
    CrossChainTransfer {
        target_chain: String,
        target_address: String,
        amount: u64,
    },
}

/// Canonical transaction type enum shared by consensus, mempool, and execution paths.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TransactionType {
    Regular,
    Coinbase,
    Transfer,
    Stake,
    Unstake,
    NftCreation,
    NftTransfer,
    NftExecution,
    DataAnalysis,
    DataCollection,
    DataStorage,
    EnvironmentalOffset,
    BridgeLock,
    BridgeUnlock,
    ContributionCommit,
    ContributionReveal,
    ForecastCommit,
    ForecastReveal,
    OracleAttestation,
    GovernanceProposal,
    GovernanceVote,
    /// Execute an approved governance proposal.
    GovernanceExecute,
    /// Finalize a completed governance proposal (close voting period).
    GovernanceFinalize,
    /// Delegate voting power to another address.
    GovernanceDelegate,
    /// Deposit funds into the governance treasury.
    TreasuryDeposit,
    /// Withdraw funds from the governance treasury.
    TreasuryWithdraw,
    Reward,
    /// Privacy-enhanced transfer with hidden amounts (Pedersen commitments + range proofs).
    ConfidentialTransfer,
    /// Deposit into a shielded privacy pool.
    ShieldedDeposit,
    /// Withdrawal from a shielded privacy pool.
    ShieldedWithdrawal,
    /// Submit a layer2 rollup batch with a ZK proof for on-chain finalization.
    Layer2BatchSubmit,
    /// Anonymous reward claim using MLSAG ring signature over eligible station keys.
    RewardClaim,
    /// On-chain slashing report carrying a [`SlashingReportPayload`]
    /// serialized into `Transaction::data`. Dispatched by
    /// `blockchain::state::State::apply_protocol_transaction` through
    /// `StateTransition::Slash`. Evidence-hash replay protection lives in
    /// `State::slashing_evidence_seen` (see #1393).
    SlashingReport,
    /// Fork-safe on-chain oracle operator registration / config update
    /// (#2055 Option A, oracle gate #724). Carries an
    /// [`OracleRegistrationPayload`] in `data`, authorized by an allowlisted
    /// governance-operator signature and applied deterministically on every
    /// node via `StateTransition::ApplyOracleRegistration` — never the
    /// out-of-band admin route, which forks a live node and is height-0 only.
    OracleRegistration,
    /// Fork-safe on-chain weather-station registration (#2537). Carries a
    /// [`StationRegistrationPayload`] in `data`, authorized by an allowlisted
    /// oracle-governance signature plus an owner possession proof, and applied
    /// deterministically on every node via
    /// `StateTransition::ApplyStationRegistration`.
    ///
    /// This is the production entry point the earn loop was missing: until it
    /// existed, `station_registry` was empty on every live chain, so every
    /// commit resolved to `Unclassified` and no contribution could ever be
    /// reward-eligible.
    StationRegistration,
    /// Governance-signed live feature-schedule extension (#3277 S4). Carries a
    /// [`crate::governance::schedule_extension::SignedFeatureScheduleExtension`]
    /// as JSON in `data` and is applied on every node through
    /// `StateTransition::ApplyFeatureScheduleExtension`, whose arm re-runs
    /// V1–V9 against the parent state and fails closed.
    ///
    /// APPENDED as the last variant: blocks are bincode, which is positional,
    /// so inserting it anywhere else would renumber every later variant.
    FeatureScheduleExtension,
    /// Physical-range evidence; appended index 36. Admission/apply remain disabled
    /// until the complete new unscheduled consensus feature is implemented.
    SlashEvidenceReport,
    /// Owner-authorized funded Node bond lifecycle, append-only index 37.
    NodeBondLifecycle,
    /// Bonded SnapshotJoin batch; append-only index 38.
    NodeServiceBatch,
    /// Attributable bonded-service evidence, append-only index 39.
    NodeFraudReport,
}

impl fmt::Display for TransactionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransactionType::Regular => write!(f, "Regular"),
            TransactionType::Coinbase => write!(f, "Coinbase"),
            TransactionType::Transfer => write!(f, "Transfer"),
            TransactionType::Stake => write!(f, "Stake"),
            TransactionType::Unstake => write!(f, "Unstake"),
            TransactionType::NftCreation => write!(f, "NftCreation"),
            TransactionType::NftTransfer => write!(f, "NftTransfer"),
            TransactionType::NftExecution => write!(f, "NftExecution"),
            TransactionType::DataAnalysis => write!(f, "DataAnalysis"),
            TransactionType::DataCollection => write!(f, "DataCollection"),
            TransactionType::DataStorage => write!(f, "DataStorage"),
            TransactionType::EnvironmentalOffset => write!(f, "EnvironmentalOffset"),
            TransactionType::BridgeLock => write!(f, "BridgeLock"),
            TransactionType::BridgeUnlock => write!(f, "BridgeUnlock"),
            TransactionType::ContributionCommit => write!(f, "ContributionCommit"),
            TransactionType::ContributionReveal => write!(f, "ContributionReveal"),
            TransactionType::ForecastCommit => write!(f, "ForecastCommit"),
            TransactionType::ForecastReveal => write!(f, "ForecastReveal"),
            TransactionType::OracleAttestation => write!(f, "OracleAttestation"),
            TransactionType::GovernanceProposal => write!(f, "GovernanceProposal"),
            TransactionType::GovernanceVote => write!(f, "GovernanceVote"),
            TransactionType::GovernanceExecute => write!(f, "GovernanceExecute"),
            TransactionType::GovernanceFinalize => write!(f, "GovernanceFinalize"),
            TransactionType::GovernanceDelegate => write!(f, "GovernanceDelegate"),
            TransactionType::TreasuryDeposit => write!(f, "TreasuryDeposit"),
            TransactionType::TreasuryWithdraw => write!(f, "TreasuryWithdraw"),
            TransactionType::Reward => write!(f, "Reward"),
            TransactionType::ConfidentialTransfer => write!(f, "ConfidentialTransfer"),
            TransactionType::ShieldedDeposit => write!(f, "ShieldedDeposit"),
            TransactionType::ShieldedWithdrawal => write!(f, "ShieldedWithdrawal"),
            TransactionType::Layer2BatchSubmit => write!(f, "Layer2BatchSubmit"),
            TransactionType::RewardClaim => write!(f, "RewardClaim"),
            TransactionType::SlashingReport => write!(f, "SlashingReport"),
            TransactionType::OracleRegistration => write!(f, "OracleRegistration"),
            TransactionType::StationRegistration => write!(f, "StationRegistration"),
            TransactionType::FeatureScheduleExtension => write!(f, "FeatureScheduleExtension"),
            TransactionType::NodeBondLifecycle => write!(f, "NodeBondLifecycle"),
            TransactionType::NodeServiceBatch => write!(f, "NodeServiceBatch"),
            TransactionType::NodeFraudReport => write!(f, "NodeFraudReport"),
            TransactionType::SlashEvidenceReport => write!(f, "SlashEvidenceReport"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransactionInput {
    /// Previous transaction hash.
    pub txid: Hash,
    /// Output index of the previous transaction.
    pub vout: u32,
    /// Signature over this transaction.
    pub signature: Option<String>,
    /// Public key used for signature validation.
    pub public_key: Option<String>,
    /// Coinbase marker data for coinbase transactions.
    pub coinbase_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransactionOutput {
    /// Value to transfer.
    pub amount: u64,
    /// Destination address.
    pub address: String,
    /// Locking script for output validation.
    pub script: String,
    /// Optional NFT payload.
    pub nft_data: Option<serde_json::Value>,
}

/// Payload embedded in `Transaction::data` for an authorized on-chain
/// gamification-reward payout (mobile#215).
///
/// The backend calls `POST /rewards/payout` when a user claims an achievement
/// reward; that endpoint mints a [`TransactionType::Reward`] transaction whose
/// `data` field carries this payload. The `claim_id` is the backend's single
/// dedup source of truth: it is folded into the consensus state set
/// `State::gamification_claims_seen` (hashed into `state_root`) so a duplicate
/// claim is a deterministic no-op on every peer — never a double-mint.
///
/// `reward_type` is a fixed discriminant (`"gamification_claim"`) so that
/// ordinary role/mining `Reward` transactions (which carry a different `data`
/// shape, or none) are NOT mistaken for gamification payouts by
/// [`Transaction::parse_gamification_claim`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GamificationClaimPayload {
    /// Fixed discriminant identifying a gamification payout. Must equal
    /// [`GamificationClaimPayload::REWARD_TYPE`].
    pub reward_type: String,
    /// Opaque, backend-generated idempotency key. Same `claim_id` → same
    /// (single) payout, no matter how many times it is submitted.
    pub claim_id: String,
    /// Hex-encoded ed25519 public key of the operator that authorized this
    /// payout (core4#2081 review). Must be in the deployment allowlist
    /// ([`gamification_operator_pubkeys`]) for the payout to be valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_pubkey: Option<String>,
    /// Hex-encoded ed25519 signature over
    /// [`gamification_authorization_message`] (claim_id, recipient, amount,
    /// chain identity), produced by the holder of `auth_pubkey`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_signature: Option<String>,
}

impl GamificationClaimPayload {
    /// The only accepted value of the `reward_type` discriminant.
    pub const REWARD_TYPE: &'static str = "gamification_claim";

    /// Build an UNSIGNED payload for the given claim id.
    ///
    /// An unsigned payload never authorizes a payout: [`Transaction::
    /// verify_gamification_authorization`] fails closed without both auth
    /// fields. Use [`GamificationClaimPayload::sign`] to authorize it.
    pub fn new(claim_id: impl Into<String>) -> Self {
        Self {
            reward_type: Self::REWARD_TYPE.to_string(),
            claim_id: claim_id.into(),
            auth_pubkey: None,
            auth_signature: None,
        }
    }

    /// Sign this payload as operator `operator_key`, binding it to the exact
    /// `recipient` and `amount` the payout credits (core4#2081 review).
    ///
    /// The signature commits to `(claim_id, recipient, amount)` under a
    /// dedicated domain tag plus the runtime chain identity, so it cannot be
    /// replayed onto a different recipient, amount, claim, or network.
    pub fn sign(&mut self, recipient: &str, amount: u64, operator_key: &PrivateKey) {
        let message =
            gamification_authorization_message(&self.claim_id, recipient, amount);
        let signature = operator_key.sign(&message);
        self.auth_pubkey = Some(PublicKey::from_private_key(operator_key).to_hex());
        self.auth_signature = Some(signature.to_hex());
    }
}

/// Domain separator for gamification-payout operator authorization signatures
/// (core4#2081 review). Distinct from every other money-path domain so a
/// signature harvested elsewhere cannot be replayed as a payout authorization.
pub const GAMIFICATION_AUTH_DOMAIN: &[u8] = b"wrfcoin_gamification_auth_v1";

/// Canonical message an operator signs to authorize a gamification payout.
///
/// Binds `claim_id`, `recipient`, and `amount` under [`GAMIFICATION_AUTH_DOMAIN`]
/// plus the runtime chain identity (for cross-network replay protection, #2034).
/// Every component is length-prefixed so no two distinct `(claim, recipient,
/// amount)` triples can share a preimage.
pub fn gamification_authorization_message(
    claim_id: &str,
    recipient: &str,
    amount: u64,
) -> Vec<u8> {
    gamification_authorization_message_with_domain(
        &signing_chain_id(),
        &signing_network_id(),
        claim_id,
        recipient,
        amount,
    )
}

/// Pure form of [`gamification_authorization_message`] with the chain identity
/// passed explicitly — the deterministic core used by golden-vector parity
/// tests and cross-repo signer mirrors.
pub fn gamification_authorization_message_with_domain(
    chain_id: &str,
    network_id: &str,
    claim_id: &str,
    recipient: &str,
    amount: u64,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(GAMIFICATION_AUTH_DOMAIN.len() + 96);
    buf.extend_from_slice(GAMIFICATION_AUTH_DOMAIN);
    push_chain_domain(&mut buf, chain_id, network_id);
    buf.extend_from_slice(&(claim_id.len() as u32).to_le_bytes());
    buf.extend_from_slice(claim_id.as_bytes());
    buf.extend_from_slice(&(recipient.len() as u32).to_le_bytes());
    buf.extend_from_slice(recipient.as_bytes());
    buf.extend_from_slice(&amount.to_le_bytes());
    buf
}

/// Fork-safe on-chain oracle registration / config update payload (#2055
/// Option A). Carried in a [`TransactionType::OracleRegistration`] tx's `data`.
///
/// Mirrors the #2081 gamification payout: an **absolute** target config +
/// signing-key epoch, authorized by an allowlisted governance-operator
/// signature bound into the tx bytes, applied deterministically on every node.
/// The set/quorum are absolute (not a delta) so producer, applier and WAL
/// replay reach the same `oracle_set` regardless of any local read.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OracleRegistrationPayload {
    /// Fixed discriminant. Must equal [`OracleRegistrationPayload::TX_KIND`].
    pub tx_kind: String,
    /// Absolute desired oracle set after this registration (identity strings).
    pub oracle_set: Vec<String>,
    /// Absolute desired quorum.
    pub oracle_quorum: usize,
    /// Operator identity being (re)keyed — an oracle_id / pubkey-hex string that
    /// must be present in `oracle_set`.
    pub identity: String,
    /// Hex ed25519 signing public key registered for `identity` at `epoch`.
    pub public_key: String,
    /// Signing-key epoch (0 is reserved for legacy identity keys).
    pub epoch: u64,
    /// Activation timestamp for the signing-key epoch.
    pub activated_at: i64,
    /// Hex ed25519 public key of the governance operator that authorized this
    /// registration. Must be in [`oracle_governance_pubkeys`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_pubkey: Option<String>,
    /// Hex ed25519 signature over [`oracle_registration_authorization_message`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_signature: Option<String>,
}

impl OracleRegistrationPayload {
    /// The only accepted value of the `tx_kind` discriminant.
    pub const TX_KIND: &'static str = "oracle_registration_v1";

    /// Build an UNSIGNED payload. Unsigned never authorizes — see
    /// [`Transaction::verify_oracle_registration_authorization`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        oracle_set: Vec<String>,
        oracle_quorum: usize,
        identity: impl Into<String>,
        public_key: impl Into<String>,
        epoch: u64,
        activated_at: i64,
    ) -> Self {
        Self {
            tx_kind: Self::TX_KIND.to_string(),
            oracle_set,
            oracle_quorum,
            identity: identity.into(),
            public_key: public_key.into(),
            epoch,
            activated_at,
            auth_pubkey: None,
            auth_signature: None,
        }
    }

    /// Canonical authorization message for this payload.
    pub fn authorization_message(&self) -> Vec<u8> {
        oracle_registration_authorization_message(self)
    }

    /// Sign this payload as governance operator `governance_key`, resolving the
    /// chain identity from process configuration.
    ///
    /// Prefer [`Self::sign_with_domain`] for anything that must be reproducible
    /// off-node: the chain identity folded in here comes from the environment
    /// with a silent default, so a signer whose environment differs from the
    /// verifying nodes' produces a signature that fails on every peer with no
    /// error at signing time.
    pub fn sign(&mut self, governance_key: &PrivateKey) {
        self.sign_with_domain(&signing_chain_id(), &signing_network_id(), governance_key);
    }

    /// Sign this payload with the chain identity passed **explicitly** (#2530).
    ///
    /// This is the deterministic form: identical inputs produce byte-identical
    /// signatures on any machine, regardless of local environment. That is the
    /// property that makes a multi-node genesis ceremony possible at all — the
    /// ceremony host and the nodes must agree on the domain, and the only way
    /// to audit that agreement is for the domain to be an explicit input.
    pub fn sign_with_domain(
        &mut self,
        chain_id: &str,
        network_id: &str,
        governance_key: &PrivateKey,
    ) {
        // Clear any prior signature so it is never folded into its own preimage.
        self.auth_pubkey = None;
        self.auth_signature = None;
        let message =
            oracle_registration_authorization_message_with_domain(chain_id, network_id, self);
        self.auth_signature = Some(governance_key.sign(&message).to_hex());
        self.auth_pubkey = Some(PublicKey::from_private_key(governance_key).to_hex());
    }

    /// Verify the embedded governance signature against the runtime chain
    /// domain. This proves signature integrity only; callers that authorize a
    /// registration must additionally require `auth_pubkey` to be in the
    /// configured governance allowlist.
    pub fn has_valid_authorization_signature(&self) -> bool {
        self.has_valid_authorization_signature_with_domain(
            &signing_chain_id(),
            &signing_network_id(),
        )
    }

    /// Pure-domain form used by offline ceremony verification and golden
    /// vectors. The signed bytes are identical to [`Self::sign`].
    pub fn has_valid_authorization_signature_with_domain(
        &self,
        chain_id: &str,
        network_id: &str,
    ) -> bool {
        let (Some(pk_hex), Some(sig_hex)) =
            (self.auth_pubkey.as_deref(), self.auth_signature.as_deref())
        else {
            return false;
        };
        let (Ok(public_key), Ok(signature)) =
            (PublicKey::from_hex(pk_hex), Signature::from_hex(sig_hex))
        else {
            return false;
        };
        let mut unsigned = self.clone();
        unsigned.auth_pubkey = None;
        unsigned.auth_signature = None;
        let message =
            oracle_registration_authorization_message_with_domain(chain_id, network_id, &unsigned);
        public_key.verify(&message, &signature)
    }
}

/// Domain separator for oracle-registration governance signatures (#2055).
pub const ORACLE_REGISTRATION_AUTH_DOMAIN: &[u8] = b"wrfcoin_oracle_registration_auth_v1";

/// Canonical message a governance operator signs to authorize an oracle
/// registration. Binds every consensus-relevant field (set, quorum, identity,
/// public_key, epoch, activated_at) under [`ORACLE_REGISTRATION_AUTH_DOMAIN`]
/// plus the runtime chain identity (#2034). Each field is length-prefixed, and
/// the oracle set is folded element-by-element in order, so no two distinct
/// registrations share a preimage. The auth fields themselves are excluded.
pub fn oracle_registration_authorization_message(p: &OracleRegistrationPayload) -> Vec<u8> {
    oracle_registration_authorization_message_with_domain(
        &signing_chain_id(),
        &signing_network_id(),
        p,
    )
}

/// Pure form with the chain identity passed explicitly (golden-vector core).
pub fn oracle_registration_authorization_message_with_domain(
    chain_id: &str,
    network_id: &str,
    p: &OracleRegistrationPayload,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(ORACLE_REGISTRATION_AUTH_DOMAIN.len() + 160);
    buf.extend_from_slice(ORACLE_REGISTRATION_AUTH_DOMAIN);
    push_chain_domain(&mut buf, chain_id, network_id);
    buf.extend_from_slice(&(p.oracle_set.len() as u32).to_le_bytes());
    for id in &p.oracle_set {
        buf.extend_from_slice(&(id.len() as u32).to_le_bytes());
        buf.extend_from_slice(id.as_bytes());
    }
    buf.extend_from_slice(&(p.oracle_quorum as u64).to_le_bytes());
    buf.extend_from_slice(&(p.identity.len() as u32).to_le_bytes());
    buf.extend_from_slice(p.identity.as_bytes());
    buf.extend_from_slice(&(p.public_key.len() as u32).to_le_bytes());
    buf.extend_from_slice(p.public_key.as_bytes());
    buf.extend_from_slice(&p.epoch.to_le_bytes());
    buf.extend_from_slice(&p.activated_at.to_le_bytes());
    buf
}

/// Fork-safe on-chain weather-station registration payload (#2537). Carried in a
/// [`TransactionType::StationRegistration`] tx's `data`.
///
/// Mirrors the #2055 Option A oracle registration exactly: an **absolute**
/// station record authorized by an allowlisted governance-operator signature
/// bound into the tx bytes, applied deterministically on every node through
/// [`crate::blockchain::state::StateTransition::ApplyStationRegistration`].
/// The record is absolute (not a delta) so producer, applier and WAL replay
/// reach the same `station_registry` regardless of any local read.
///
/// Two independent signatures, because two different things are being claimed:
///
/// - `owner_pubkey`/`owner_signature` — **possession**. The station owner proves
///   control of a key over `station_id + owner_address`.
/// - `auth_pubkey`/`auth_signature` — **authorisation**. An allowlisted oracle
///   governance operator signs the whole record, including `provenance_class`.
///   This is the reward-authorising act: `#2537` requires that `Real` is never
///   self-asserted by the station, and possession alone must not confer it.
///
/// `reputation_score` is deliberately **not** a payload field. A registration
/// seeds a station at zero and reputation is earned through the normal
/// reputation path; otherwise a governance signature could mint standing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StationRegistrationPayload {
    /// Fixed discriminant. Must equal [`StationRegistrationPayload::TX_KIND`].
    pub tx_kind: String,
    pub station_id: String,
    pub owner_address: String,
    // determinism-ok: validated finite coordinates are encoded by canonical IEEE-754 bits in the signed preimage; no float arithmetic occurs.
    pub latitude: f64,
    // determinism-ok: validated finite coordinates are encoded by canonical IEEE-754 bits in the signed preimage; no float arithmetic occurs.
    pub longitude: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    // determinism-ok: optional elevation is validated finite and encoded by canonical IEEE-754 bits in the signed preimage; no float arithmetic occurs.
    pub elevation_meters: Option<f64>,
    pub hardware_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration_date: Option<String>,
    pub is_active: bool,
    pub data_quality_tier: DataQualityTier,
    /// The reward-authorising field. Fail-closed by construction: the serde
    /// default is [`ContributionProvenanceClass::Unclassified`], never `Real`.
    #[serde(default)]
    pub provenance_class: ContributionProvenanceClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ring_public_key: Option<String>,
    /// Hex ed25519 public key of the station owner (possession proof).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_pubkey: Option<String>,
    /// Hex ed25519 signature over [`station_ownership_message`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_signature: Option<String>,
    /// Hex ed25519 public key of the governance operator that authorized this
    /// registration. Must be in [`crate::crypto::domain_signing::oracle_governance_pubkeys`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_pubkey: Option<String>,
    /// Hex ed25519 signature over [`station_registration_authorization_message`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_signature: Option<String>,
    /// v2 only (`tx_kind = station_registration_v2`): the station's owner-ledger
    /// nonce this registration is signed for, bound into the GOVERNANCE
    /// preimage. Must be `None` for v1 and `Some` for v2. See
    /// `ConsensusFeature::StationRegistrationNonce`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_nonce: Option<u64>,
}

impl StationRegistrationPayload {
    /// The v1 value of the `tx_kind` discriminant.
    pub const TX_KIND: &'static str = "station_registration_v1";
    /// The v2 value (carries a signed `expected_nonce`).
    pub const TX_KIND_V2: &'static str = "station_registration_v2";

    /// Is this the v2 (nonce-bound) format?
    pub fn is_v2(&self) -> bool {
        self.tx_kind == Self::TX_KIND_V2
    }

    /// Turn an unsigned payload into a v2 payload bound to `expected_nonce`.
    /// Call BEFORE signing: the nonce is part of the governance preimage.
    pub fn with_expected_nonce(mut self, expected_nonce: u64) -> Self {
        self.tx_kind = Self::TX_KIND_V2.to_string();
        self.expected_nonce = Some(expected_nonce);
        self
    }

    /// Build an UNSIGNED payload. Unsigned never authorizes — see
    /// [`Transaction::verify_station_registration_authorization`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        station_id: impl Into<String>,
        owner_address: impl Into<String>,
        // determinism-ok: these values are carried unchanged into the validated, canonical bit-encoded signed payload.
        latitude: f64,
        // determinism-ok: these values are carried unchanged into the validated, canonical bit-encoded signed payload.
        longitude: f64,
        hardware_model: impl Into<String>,
        data_quality_tier: DataQualityTier,
        provenance_class: ContributionProvenanceClass,
    ) -> Self {
        Self {
            tx_kind: Self::TX_KIND.to_string(),
            station_id: station_id.into(),
            owner_address: owner_address.into(),
            latitude,
            longitude,
            elevation_meters: None,
            hardware_model: hardware_model.into(),
            calibration_date: None,
            is_active: true,
            data_quality_tier,
            provenance_class,
            ring_public_key: None,
            owner_pubkey: None,
            owner_signature: None,
            auth_pubkey: None,
            auth_signature: None,
            expected_nonce: None,
        }
    }

    /// Structural validity, checked **before** any signature is verified and
    /// again at parse time on every peer.
    ///
    /// Coordinates must be finite and in range. That is not cosmetic: a NaN
    /// latitude has many bit patterns and no ordering, so it would make the
    /// signature preimage and the `state_root` encoding ambiguous, and
    /// `cell_for_station` would bucket it unpredictably. Rejecting non-finite
    /// coordinates here removes them from the signed surface entirely.
    pub fn validate(&self) -> Result<(), String> {
        match (self.tx_kind.as_str(), self.expected_nonce) {
            (Self::TX_KIND, None) | (Self::TX_KIND_V2, Some(_)) => {},
            (Self::TX_KIND, Some(_)) => {
                return Err("a v1 registration cannot carry expected_nonce".to_string());
            },
            (Self::TX_KIND_V2, None) => {
                return Err("a v2 registration must carry expected_nonce".to_string());
            },
            (other, _) => return Err(format!("unexpected tx_kind '{other}'")),
        }
        if self.station_id.trim().is_empty() || self.station_id.trim() != self.station_id {
            return Err("station_id must be non-empty and untrimmed-whitespace-free".to_string());
        }
        if self.station_id.chars().any(|c| c.is_whitespace()) {
            return Err("station_id cannot contain whitespace".to_string());
        }
        if self.owner_address.trim().is_empty() {
            return Err("owner_address cannot be empty".to_string());
        }
        // determinism-ok: range literals are validation constants; the accepted value is encoded as canonical bits, never computed by float arithmetic.
        if !self.latitude.is_finite() || !(-90.0..=90.0).contains(&self.latitude) {
            return Err(format!("latitude {} out of range", self.latitude));
        }
        // determinism-ok: range literals are validation constants; the accepted value is encoded as canonical bits, never computed by float arithmetic.
        if !self.longitude.is_finite() || !(-180.0..=180.0).contains(&self.longitude) {
            return Err(format!("longitude {} out of range", self.longitude));
        }
        if let Some(elevation) = self.elevation_meters {
            if !elevation.is_finite() {
                return Err("elevation_meters must be finite".to_string());
            }
        }
        Ok(())
    }

    /// Canonical authorization message for this payload.
    pub fn authorization_message(&self) -> Vec<u8> {
        station_registration_authorization_message(self)
    }

    /// Canonical ownership-possession message for this payload.
    pub fn ownership_message(&self) -> Vec<u8> {
        station_ownership_message(&self.station_id, &self.owner_address)
    }

    /// Sign the governance authorisation with the chain identity passed
    /// **explicitly**. Deterministic: identical inputs produce byte-identical
    /// signatures on any machine, which is what makes an off-node ceremony
    /// auditable (same reasoning as #2530).
    pub fn sign_with_domain(
        &mut self,
        chain_id: &str,
        network_id: &str,
        governance_key: &PrivateKey,
    ) {
        // Clear any prior signature so it is never folded into its own preimage.
        self.auth_pubkey = None;
        self.auth_signature = None;
        let message =
            station_registration_authorization_message_with_domain(chain_id, network_id, self);
        self.auth_signature = Some(governance_key.sign(&message).to_hex());
        self.auth_pubkey = Some(PublicKey::from_private_key(governance_key).to_hex());
    }

    /// Sign the governance authorisation, resolving the chain identity from
    /// process configuration. Prefer [`Self::sign_with_domain`] off-node.
    pub fn sign(&mut self, governance_key: &PrivateKey) {
        self.sign_with_domain(&signing_chain_id(), &signing_network_id(), governance_key);
    }

    /// Sign the owner possession proof with the chain identity passed explicitly.
    pub fn sign_ownership_with_domain(
        &mut self,
        chain_id: &str,
        network_id: &str,
        owner_key: &PrivateKey,
    ) {
        let message = station_ownership_message_with_domain(
            chain_id,
            network_id,
            &self.station_id,
            &self.owner_address,
        );
        self.owner_signature = Some(owner_key.sign(&message).to_hex());
        self.owner_pubkey = Some(PublicKey::from_private_key(owner_key).to_hex());
    }

    /// Sign the owner possession proof using process chain identity.
    pub fn sign_ownership(&mut self, owner_key: &PrivateKey) {
        self.sign_ownership_with_domain(&signing_chain_id(), &signing_network_id(), owner_key);
    }

    /// #2742: does `owner_pubkey` actually derive to `owner_address`?
    ///
    /// The canonical derivation is [`PublicKey::to_address`] — SHA3-256 over
    /// the raw ed25519 public key, first 20 bytes. That derivation is
    /// canonical because it is the one consensus already executes on-chain:
    /// slashing maps an oracle identity to its slashable **account** with it
    /// (`state.rs`, `apply_slashing_event`), and the PoP producer↔coinbase
    /// binding (#2226) is defined by it. The CLI keystore's SHA-256/`Wrf`
    /// derivation is a wallet-local file-naming convention whose output
    /// cannot even parse as a chain [`Address`] (`from_hex` strips only
    /// `0x`), so it has never been load-bearing on-chain and is not eligible.
    ///
    /// Comparison is on the 20 raw bytes, accepting the two presentations
    /// that occur in the tree (`0x…` per the ceremony CLI flag, `wrf…` as
    /// `to_address` emits) plus bare hex. Any parse failure is `false` —
    /// fail-closed, like every other check on this payload.
    ///
    /// [`Address`]: crate::common::types::Address
    pub fn owner_address_is_bound_to_owner_pubkey(&self) -> bool {
        let Some(owner_pk_hex) = self.owner_pubkey.as_deref() else {
            return false;
        };
        owner_address_bound_to_pubkey(&self.owner_address, owner_pk_hex)
    }
}

/// #2742 binding rule as a free function (shared with the #3345 transfer and
/// recovery payloads): does the hex ed25519 `owner_pubkey_hex` derive
/// (`PublicKey::to_address`) to `owner_address`? Comparison is on the 20 raw
/// bytes after stripping `0x`/`wrf`; any parse failure is `false`.
pub fn owner_address_bound_to_pubkey(owner_address: &str, owner_pubkey_hex: &str) -> bool {
    let Ok(owner_pk) = PublicKey::from_hex(owner_pubkey_hex) else {
        return false;
    };
    let derived = owner_pk.to_address();
    let derived_hex = derived.trim_start_matches("wrf");
    let claimed_hex = owner_address
        .trim_start_matches("0x")
        .trim_start_matches("wrf");
    let (Ok(derived_bytes), Ok(claimed_bytes)) =
        (hex::decode(derived_hex), hex::decode(claimed_hex))
    else {
        return false;
    };
    derived_bytes.len() == 20 && derived_bytes == claimed_bytes
}

/// Domain separator for station-registration governance signatures (#2537).
pub const STATION_REGISTRATION_AUTH_DOMAIN: &[u8] = b"wrfcoin_station_registration_auth_v1";

/// Domain separator of the v2 (nonce-bound) governance registration signature.
/// A different tag, so a v1 signature can never verify as a v2 one or back.
pub const STATION_REGISTRATION_AUTH_V2_DOMAIN: &[u8] = b"wrfcoin_station_registration_auth_v2";

/// Domain separator for station-ownership possession signatures (#2537).
pub const STATION_OWNERSHIP_DOMAIN: &[u8] = b"wrfcoin_station_ownership_v1";

/// Canonical message a governance operator signs to authorize a station
/// registration. Binds every consensus-relevant field of the station record
/// under [`STATION_REGISTRATION_AUTH_DOMAIN`] plus the runtime chain identity
/// (#2034). Each field is length-prefixed, so no two distinct registrations
/// share a preimage. The auth and owner-signature fields are excluded.
pub fn station_registration_authorization_message(p: &StationRegistrationPayload) -> Vec<u8> {
    station_registration_authorization_message_with_domain(
        &signing_chain_id(),
        &signing_network_id(),
        p,
    )
}

/// Pure form with the chain identity passed explicitly (golden-vector core).
pub fn station_registration_authorization_message_with_domain(
    chain_id: &str,
    network_id: &str,
    p: &StationRegistrationPayload,
) -> Vec<u8> {
    // -0.0 and +0.0 compare equal but have distinct bit patterns; normalise so
    // a signature cannot be invalidated by a sign-of-zero round trip.
    // determinism-ok: this helper accepts a validated payload float solely for canonical bit encoding, never arithmetic.
    fn f64_bits(v: f64) -> u64 {
        // determinism-ok: this canonicalizes signed zero before raw IEEE-754 bit encoding; it performs no platform-dependent float arithmetic.
        if v == 0.0 { 0f64.to_bits() } else { v.to_bits() }
    }
    fn push_str(buf: &mut Vec<u8>, s: &str) {
        buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
        buf.extend_from_slice(s.as_bytes());
    }
    fn push_opt_str(buf: &mut Vec<u8>, s: Option<&str>) {
        match s {
            Some(value) => {
                buf.push(1);
                push_str(buf, value);
            },
            None => buf.push(0),
        }
    }

    let mut buf = Vec::with_capacity(STATION_REGISTRATION_AUTH_DOMAIN.len() + 192);
    // v1 keeps its original tag and layout byte for byte; v2 uses its own tag
    // and appends the signed expected nonce.
    buf.extend_from_slice(if p.is_v2() {
        STATION_REGISTRATION_AUTH_V2_DOMAIN
    } else {
        STATION_REGISTRATION_AUTH_DOMAIN
    });
    push_chain_domain(&mut buf, chain_id, network_id);
    push_str(&mut buf, &p.station_id);
    push_str(&mut buf, &p.owner_address);
    buf.extend_from_slice(&f64_bits(p.latitude).to_le_bytes());
    buf.extend_from_slice(&f64_bits(p.longitude).to_le_bytes());
    match p.elevation_meters {
        Some(value) => {
            buf.push(1);
            buf.extend_from_slice(&f64_bits(value).to_le_bytes());
        },
        None => buf.push(0),
    }
    push_str(&mut buf, &p.hardware_model);
    push_opt_str(&mut buf, p.calibration_date.as_deref());
    buf.push(u8::from(p.is_active));
    buf.push(match p.data_quality_tier {
        DataQualityTier::Unverified => 0,
        DataQualityTier::Bronze => 1,
        DataQualityTier::Silver => 2,
        DataQualityTier::Gold => 3,
    });
    // The reward-authorising byte. Reuses the same discriminant the state-root
    // encoding uses, so the signed value and the hashed value cannot drift.
    buf.push(p.provenance_class.state_root_discriminant());
    push_opt_str(&mut buf, p.ring_public_key.as_deref());
    // The owner possession key is bound into the governance preimage: governance
    // authorizes THIS owner key for this station, so a harvested governance
    // signature cannot be re-paired with a different owner's possession proof.
    push_opt_str(&mut buf, p.owner_pubkey.as_deref());
    if p.is_v2() {
        // The station's owner-ledger nonce this registration is signed for.
        buf.extend_from_slice(&p.expected_nonce.unwrap_or(0).to_le_bytes());
    }
    buf
}

/// Canonical message a station owner signs to prove possession of the key
/// bound to `owner_address` for `station_id`.
pub fn station_ownership_message(station_id: &str, owner_address: &str) -> Vec<u8> {
    station_ownership_message_with_domain(
        &signing_chain_id(),
        &signing_network_id(),
        station_id,
        owner_address,
    )
}

/// Pure form with the chain identity passed explicitly.
pub fn station_ownership_message_with_domain(
    chain_id: &str,
    network_id: &str,
    station_id: &str,
    owner_address: &str,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(STATION_OWNERSHIP_DOMAIN.len() + 96);
    buf.extend_from_slice(STATION_OWNERSHIP_DOMAIN);
    push_chain_domain(&mut buf, chain_id, network_id);
    buf.extend_from_slice(&(station_id.len() as u32).to_le_bytes());
    buf.extend_from_slice(station_id.as_bytes());
    buf.extend_from_slice(&(owner_address.len() as u32).to_le_bytes());
    buf.extend_from_slice(owner_address.as_bytes());
    buf
}

/// Domain separator for the CURRENT owner's station-transfer authorization
/// (#3345). Design: `docs/architecture/station-ownership-transfer.md`.
pub const STATION_OWNER_TRANSFER_DOMAIN: &[u8] = b"wrfcoin_station_owner_transfer_v1";

/// Domain separator for the NEW owner's acceptance of a specific station
/// transfer (#3345). Same body as [`STATION_OWNER_TRANSFER_DOMAIN`], different
/// tag, so neither signature can stand in for the other.
pub const STATION_OWNER_TRANSFER_ACCEPT_DOMAIN: &[u8] = b"wrfcoin_station_owner_transfer_accept_v1";

/// Canonical preimage for a station owner transfer (#3345), with the chain
/// identity passed explicitly.
///
/// `domain` must be [`STATION_OWNER_TRANSFER_DOMAIN`] (signed by the current
/// owner) or [`STATION_OWNER_TRANSFER_ACCEPT_DOMAIN`] (signed by the new
/// owner). Every field is explicitly length-prefixed or fixed-width
/// little-endian — no floats and no bincode, so the bytes are stable across
/// struct-layout changes (see #3163). Replay safety comes from
/// `transfer_nonce`, which must equal the station's next owner-action nonce in
/// state, and from `expires_at_height`; `from_owner_address` pins the owner
/// being replaced so a signature cannot be reused after the station changes
/// hands.
///
/// The payload and stateless verification live in
/// [`crate::blockchain::station_owner`]. Both signatures are mandatory (owner
/// ruling Q4 2026-09-27). The shared extractor emits the owner-ledger transfer
/// transition only after `station_owner_consent` activates; `State` then checks
/// the registered owner and per-station nonce before applying it.
#[allow(clippy::too_many_arguments)]
pub fn station_owner_transfer_message_with_domain(
    domain: &[u8],
    chain_id: &str,
    network_id: &str,
    station_id: &str,
    from_owner_address: &str,
    to_owner_address: &str,
    to_owner_pubkey: &str,
    transfer_nonce: u64,
    expires_at_height: u64,
) -> Vec<u8> {
    fn push_str(buf: &mut Vec<u8>, s: &str) {
        buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
        buf.extend_from_slice(s.as_bytes());
    }
    let mut buf = Vec::with_capacity(domain.len() + 256);
    buf.extend_from_slice(domain);
    push_chain_domain(&mut buf, chain_id, network_id);
    push_str(&mut buf, station_id);
    push_str(&mut buf, from_owner_address);
    push_str(&mut buf, to_owner_address);
    push_str(&mut buf, to_owner_pubkey);
    buf.extend_from_slice(&transfer_nonce.to_le_bytes());
    buf.extend_from_slice(&expires_at_height.to_le_bytes());
    buf
}

/// Domain separator for owner-signed station actions (#3347): report
/// stolen/lost (freeze), unfreeze, self-deactivate, reactivate. Design:
/// `docs/architecture/station-freeze-and-deactivation.md`.
pub const STATION_OWNER_ACTION_DOMAIN: &[u8] = b"wrfcoin_station_owner_action_v1";

/// An owner-signed station action (#3347). The discriminant is part of the
/// signed preimage, so a signature for one action can never authorize
/// another. Values are frozen: append new actions, never renumber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StationOwnerActionKind {
    /// Report the station stolen or lost: rewards stop and (with #3346) its
    /// hardware commitment is frozen so nobody else can register it.
    ReportStolen,
    /// Lift a report-stolen freeze. Owner-signed only (or the owner-decided
    /// recovery path).
    Unfreeze,
    /// Owner self-deactivation (e.g. the station is being moved or retired).
    Deactivate,
    /// Owner reactivation of a self-deactivated (not frozen) station.
    Reactivate,
}

impl StationOwnerActionKind {
    /// Frozen preimage discriminant.
    pub const fn discriminant(self) -> u8 {
        match self {
            Self::ReportStolen => 1,
            Self::Unfreeze => 2,
            Self::Deactivate => 3,
            Self::Reactivate => 4,
        }
    }
}

/// Canonical preimage for an owner-signed station action (#3347), with the
/// chain identity passed explicitly.
///
/// Explicit length-prefixed / fixed-width little-endian encoding — no floats
/// and no bincode (see #3163). `owner_address` pins the owner who signs, so a
/// signature does not survive a transfer; `action_nonce` must equal the
/// station's next owner-action nonce in state (the SAME per-station counter
/// the #3345 transfer consumes, so actions and transfers cannot be replayed
/// or reordered against each other); `expires_at_height` bounds how long a
/// signed but unsubmitted action stays valid.
///
/// The signed payload and shared extractor live in
/// [`crate::blockchain::station_owner`]. Extraction and owner-ledger application
/// require `station_owner_actions`; `State` checks the registered owner,
/// per-station nonce and action preconditions before committing the action.
pub fn station_owner_action_message_with_domain(
    chain_id: &str,
    network_id: &str,
    station_id: &str,
    owner_address: &str,
    action: StationOwnerActionKind,
    action_nonce: u64,
    expires_at_height: u64,
) -> Vec<u8> {
    fn push_str(buf: &mut Vec<u8>, s: &str) {
        buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
        buf.extend_from_slice(s.as_bytes());
    }
    let mut buf = Vec::with_capacity(STATION_OWNER_ACTION_DOMAIN.len() + 160);
    buf.extend_from_slice(STATION_OWNER_ACTION_DOMAIN);
    push_chain_domain(&mut buf, chain_id, network_id);
    push_str(&mut buf, station_id);
    push_str(&mut buf, owner_address);
    buf.push(action.discriminant());
    buf.extend_from_slice(&action_nonce.to_le_bytes());
    buf.extend_from_slice(&expires_at_height.to_le_bytes());
    buf
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommitPayload {
    #[serde(alias = "commit_hash")]
    pub commit_id: String,
    pub station_id: String,
    pub timestamp: i64,
    #[serde(alias = "type")]
    pub contribution_type: String,
    /// Hex-encoded ed25519 public key that authorized this commit submission (#1848).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_pubkey: Option<String>,
    /// Hex-encoded ed25519 signature over the canonical commit-authorization
    /// message (`commit_authorization_message`), produced by the holder of
    /// `auth_pubkey` (#1848).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_signature: Option<String>,
}

/// Fold the runtime chain identity `(chain_id, network_id)` into a signing
/// preimage buffer, length-prefixed, for cross-network replay protection
/// (#2034). Both fields are included so a signature is bound to *both* the
/// specific chain id (e.g. `777`) and the network family (e.g. `testnet`): a
/// signature produced on one network cannot verify on another (the EIP-155
/// class the money-path preimages previously lacked).
pub(crate) fn push_chain_domain(buf: &mut Vec<u8>, chain_id: &str, network_id: &str) {
    buf.extend_from_slice(&(chain_id.len() as u32).to_le_bytes());
    buf.extend_from_slice(chain_id.as_bytes());
    buf.extend_from_slice(&(network_id.len() as u32).to_le_bytes());
    buf.extend_from_slice(network_id.as_bytes());
}

/// Domain separator for commit sender authorization signatures (#1848).
/// Bumped `v1`→`v2` in #2034 when the chain identity was folded into the
/// preimage; the tag change alone already prevents a v1 signature from
/// verifying against the v2 message.
pub const COMMIT_AUTH_DOMAIN: &[u8] = b"wrfcoin_commit_auth_v2";

/// Build the canonical message a contributor signs to authorize a commit submission (#1848).
///
/// Binds `station_id`, `commit_id`, and `contribution_type` under a dedicated domain tag,
/// plus the runtime chain identity for cross-network replay protection (#2034). The
/// identity is resolved from deployment config ([`signing_chain_id`]/[`signing_network_id`]);
/// use [`commit_authorization_message_with_domain`] to pass it explicitly.
/// Binding `commit_id` (a hash of the actual data + nonce) makes each authorization
/// unique per submission. `auth_pubkey`/`auth_signature` are excluded from the message —
/// they are the signature *over* this message.
pub fn commit_authorization_message(
    station_id: &str,
    commit_id: &str,
    contribution_type: &str,
) -> Vec<u8> {
    commit_authorization_message_with_domain(
        &signing_chain_id(),
        &signing_network_id(),
        station_id,
        commit_id,
        contribution_type,
    )
}

/// Pure form of [`commit_authorization_message`] with the chain identity passed
/// explicitly — the deterministic core used by golden-vector parity tests (and
/// by cross-repo signer mirrors) and by the env-resolving public wrapper (#2034).
pub fn commit_authorization_message_with_domain(
    chain_id: &str,
    network_id: &str,
    station_id: &str,
    commit_id: &str,
    contribution_type: &str,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(COMMIT_AUTH_DOMAIN.len() + 160);
    buf.extend_from_slice(COMMIT_AUTH_DOMAIN);
    push_chain_domain(&mut buf, chain_id, network_id);
    buf.extend_from_slice(&(station_id.len() as u32).to_le_bytes());
    buf.extend_from_slice(station_id.as_bytes());
    buf.extend_from_slice(&(commit_id.len() as u32).to_le_bytes());
    buf.extend_from_slice(commit_id.as_bytes());
    buf.extend_from_slice(&(contribution_type.len() as u32).to_le_bytes());
    buf.extend_from_slice(contribution_type.as_bytes());
    buf
}

impl CommitPayload {
    /// Canonical authorization message for this payload (#1848).
    pub fn authorization_message(&self) -> Vec<u8> {
        commit_authorization_message(&self.station_id, &self.commit_id, &self.contribution_type)
    }

    /// Sign this commit payload with the contributor's private key (#1848).
    pub fn sign_authorization(&mut self, private_key: &PrivateKey) {
        let message = self.authorization_message();
        let signature = private_key.sign(&message);
        let public_key = PublicKey::from_private_key(private_key);
        self.auth_pubkey = Some(public_key.to_hex());
        self.auth_signature = Some(signature.to_hex());
    }

    /// Verify the embedded commit authorization (#1848).
    ///
    /// Fail-closed: returns `false` unless both `auth_pubkey` and `auth_signature` are
    /// present, hex-decodable, and the signature verifies over the canonical authorization
    /// message for this payload.
    pub fn verify_authorization(&self) -> bool {
        let (Some(pk_hex), Some(sig_hex)) =
            (self.auth_pubkey.as_deref(), self.auth_signature.as_deref())
        else {
            return false;
        };
        let Ok(public_key) = PublicKey::from_hex(pk_hex) else {
            return false;
        };
        let Ok(signature) = Signature::from_hex(sig_hex) else {
            return false;
        };
        public_key.verify(&self.authorization_message(), &signature)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RevealPayload {
    #[serde(alias = "commit_hash")]
    pub commit_id: String,
    pub station_id: String,
    pub timestamp: i64,
    #[serde(alias = "type")]
    pub contribution_type: String,
    #[serde(alias = "payload")]
    pub data: serde_json::Value,
    pub nonce: String,

    /// CID reference to off-chain stored data (e.g. "ipfs:QmXyz..." or "arweave:abc123...")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_cid: Option<String>,

    /// SHA-256 hex digest of the full observation payload for integrity verification
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_hash: Option<String>,

    /// Storage provider name ("ipfs", "filecoin", "arweave")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_provider: Option<String>,

    /// Optional sensor/source provenance metadata used by deterministic trust inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<SensorProvenance>,

    /// Optional forecast artifact manifest for forecast reveals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<ForecastArtifactManifest>,

    /// Optional deterministic scoring receipt from forecast verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_receipt: Option<ForecastVerificationReceipt>,

    /// Optional compute receipt for fee-less weather data submissions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compute_receipt: Option<ComputeReceipt>,

    /// Collector-assigned quality tier from Option-C hybrid NWS comparison.
    /// `None` means the collector predates this field (treat as `Good`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_tier: Option<ReadingQualityTier>,
}

pub use wrfcoin_forecast_score::{
    ChainBinding, ChainBindingError, ForecastAccuracyMetrics, ForecastArtifactManifest,
    ForecastManifestValidationError, ForecastMetricsValidationError,
    ForecastReceiptValidationError, ForecastVerificationReceipt, ScoreError,
};

/// Compute the deterministic digest of a forecast manifest.
pub fn forecast_manifest_digest(manifest: &ForecastArtifactManifest) -> String {
    manifest.manifest_digest()
}

/// Compute the deterministic digest of a forecast verification receipt.
pub fn forecast_receipt_digest(receipt: &ForecastVerificationReceipt) -> String {
    receipt.receipt_digest()
}

/// Build the canonical signing message for a forecast verification receipt
/// under the process's declared chain identity. With none declared the
/// message is built under `ChainBinding::UNBOUND`, never mainnet (#3064);
/// signers that want the configuration error use
/// `receipt.checked_signing_message()`.
pub fn forecast_receipt_signing_message(receipt: &ForecastVerificationReceipt) -> String {
    receipt.signing_message()
}

/// Verify the signature on a forecast verification receipt.
pub fn forecast_receipt_has_valid_signature(receipt: &ForecastVerificationReceipt) -> bool {
    receipt.has_valid_signature()
}

/// Receipt proving useful computational work was performed.
/// Weather data submissions can use this instead of token fees.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeReceipt {
    /// Task identifier for the compute job.
    pub task_id: String,
    /// Worker node that performed the computation.
    pub worker_id: String,
    /// Blake3 hash of the compute output.
    pub work_hash: String,
    /// Difficulty/cost factor of work performed (higher = more work).
    pub work_factor: u64,
    /// Unix timestamp when work was completed.
    pub completed_at: i64,
    /// Anti-replay nonce.
    pub nonce: String,
    /// Hex-encoded signature over the receipt fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl ComputeReceipt {
    /// Build the canonical domain-separated signing message for this receipt.
    fn canonical_signing_message(&self) -> String {
        #[derive(Serialize)]
        struct SigningPayload<'a> {
            task_id: &'a str,
            worker_id: &'a str,
            work_hash: &'a str,
            work_factor: u64,
            completed_at: i64,
            nonce: &'a str,
        }
        let signing_payload = SigningPayload {
            task_id: &self.task_id,
            worker_id: &self.worker_id,
            work_hash: &self.work_hash,
            work_factor: self.work_factor,
            completed_at: self.completed_at,
            nonce: &self.nonce,
        };
        let digest = payload_digest_hex(&signing_payload);
        let cid = signing_chain_id();
        let nid = signing_network_id();
        let context = DomainSigningContext {
            action: DomainSigningAction::ComputeReceipt,
            signer_id: &self.worker_id,
            timestamp: self.completed_at,
            nonce: Some(&self.nonce),
            chain_id: &cid,
            network_id: &nid,
        };
        canonical_signing_message(&context, &digest)
    }

    /// Canonical domain-separated signing message for compute receipts.
    pub fn signing_message(&self) -> String {
        self.canonical_signing_message()
    }

    /// Verify the receipt's signature against the worker_id public key.
    pub fn has_valid_signature(&self) -> bool {
        let Some(signature_hex) = self.signature.as_deref() else {
            return false;
        };
        let public_key = match PublicKey::from_hex(&self.worker_id) {
            Ok(pk) => pk,
            Err(_) => return false,
        };
        let signature = match Signature::from_hex(signature_hex) {
            Ok(sig) => sig,
            Err(_) => return false,
        };
        let message = self.signing_message();
        public_key.verify(message.as_bytes(), &signature)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SensorProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firmware_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration_attestation_cid: Option<String>,
}

const VALID_STORAGE_PROVIDERS: &[&str] = &["ipfs", "filecoin", "arweave"];
const MAX_DATA_CID_LEN: usize = 256;
const ARWEAVE_TX_ID_LEN: usize = 43;
const MAX_PROVENANCE_SOURCE_ID_LEN: usize = 128;
const MAX_PROVENANCE_DEVICE_TEXT_LEN: usize = 128;
const MAX_PROVENANCE_CALIBRATION_CID_LEN: usize = 256;

/// Deterministic CID validation errors for reveal payloads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, thiserror::Error)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RevealCidValidationError {
    #[error(
        "SP_TX_CID_FIELDS_PARTIAL: data_cid, data_hash, and storage_provider must all be present or all absent"
    )]
    CidFieldsPartial,
    #[error("SP_TX_INV_CID_TUPLE_BROKEN: CID tuple invariant broken after presence check")]
    CidTupleInvariantBroken,
    #[error("SP_TX_CID_EMPTY: data_cid must not be empty")]
    EmptyCid,
    #[error(
        "SP_TX_CID_SURROUNDING_WHITESPACE: data_cid must not contain leading or trailing whitespace"
    )]
    CidHasSurroundingWhitespace,
    #[error("SP_TX_CID_CONTROL_CHARS: data_cid contains control characters")]
    CidContainsControlCharacters,
    #[error("SP_TX_CID_TOO_LONG: data_cid exceeds maximum length of 256 characters")]
    CidTooLong,
    #[error("SP_TX_CID_PREFIX_PROVIDER_MISMATCH: CID prefix does not match storage_provider")]
    CidPrefixProviderMismatch,
    #[error(
        "SP_TX_CID_FORMAT_IPFS_FILECOIN: expected CIDv0 base58btc (Qm...) or CIDv1 base32 (b...) format"
    )]
    InvalidIpfsLikeCidFormat,
    #[error("SP_TX_CID_FORMAT_ARWEAVE: expected 43-character base64url Arweave transaction id")]
    InvalidArweaveTxIdFormat,
    #[error("SP_TX_DATA_HASH_FORMAT: data_hash must be a 64-character hex string")]
    InvalidDataHashFormat,
    #[error(
        "SP_TX_STORAGE_PROVIDER_INVALID: storage_provider must be one of: ipfs, filecoin, arweave"
    )]
    InvalidStorageProvider,
}

impl RevealCidValidationError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::CidFieldsPartial => "SP_TX_CID_FIELDS_PARTIAL",
            Self::CidTupleInvariantBroken => "SP_TX_INV_CID_TUPLE_BROKEN",
            Self::EmptyCid => "SP_TX_CID_EMPTY",
            Self::CidHasSurroundingWhitespace => "SP_TX_CID_SURROUNDING_WHITESPACE",
            Self::CidContainsControlCharacters => "SP_TX_CID_CONTROL_CHARS",
            Self::CidTooLong => "SP_TX_CID_TOO_LONG",
            Self::CidPrefixProviderMismatch => "SP_TX_CID_PREFIX_PROVIDER_MISMATCH",
            Self::InvalidIpfsLikeCidFormat => "SP_TX_CID_FORMAT_IPFS_FILECOIN",
            Self::InvalidArweaveTxIdFormat => "SP_TX_CID_FORMAT_ARWEAVE",
            Self::InvalidDataHashFormat => "SP_TX_DATA_HASH_FORMAT",
            Self::InvalidStorageProvider => "SP_TX_STORAGE_PROVIDER_INVALID",
        }
    }
}

impl RevealPayload {
    /// Validate CID-related fields: all three must be present together or all absent.
    pub fn validate_cid_fields(&self) -> Result<(), RevealCidValidationError> {
        let has_cid = self.data_cid.is_some();
        let has_hash = self.data_hash.is_some();
        let has_provider = self.storage_provider.is_some();

        match (has_cid, has_hash, has_provider) {
            (false, false, false) => return Ok(()),
            (true, true, true) => {},
            _ => return Err(RevealCidValidationError::CidFieldsPartial),
        }

        let (cid, hash, provider) = match (
            self.data_cid.as_deref(),
            self.data_hash.as_deref(),
            self.storage_provider.as_deref(),
        ) {
            (Some(cid), Some(hash), Some(provider)) => (cid, hash, provider),
            _ => return Err(RevealCidValidationError::CidTupleInvariantBroken),
        };

        if cid.is_empty() {
            return Err(RevealCidValidationError::EmptyCid);
        }
        if cid.trim() != cid {
            return Err(RevealCidValidationError::CidHasSurroundingWhitespace);
        }
        if cid.len() > MAX_DATA_CID_LEN {
            return Err(RevealCidValidationError::CidTooLong);
        }
        if cid.chars().any(char::is_control) {
            return Err(RevealCidValidationError::CidContainsControlCharacters);
        }

        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(RevealCidValidationError::InvalidDataHashFormat);
        }

        if !VALID_STORAGE_PROVIDERS.contains(&provider) {
            return Err(RevealCidValidationError::InvalidStorageProvider);
        }

        let stripped_cid = strip_provider_prefix(cid, provider)?;
        validate_provider_specific_cid(stripped_cid, provider)
    }

    /// Returns true if this reveal references off-chain data via CID.
    pub fn is_cid_referenced(&self) -> bool {
        self.data_cid.is_some()
    }

    /// Validate optional provenance metadata with deterministic bounds.
    pub fn validate_provenance_fields(&self) -> Result<(), RevealProvenanceValidationError> {
        let Some(provenance) = &self.provenance else {
            return Ok(());
        };
        provenance.validate()
    }
}

/// Deterministic provenance validation errors for reveal payloads.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, thiserror::Error)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RevealProvenanceValidationError {
    #[error("SP_TX_PROV_EMPTY: provenance payload has no populated fields")]
    EmptyProvenance,
    #[error("SP_TX_PROV_SOURCE_ID_INVALID: source_id is empty or invalid")]
    InvalidSourceId,
    #[error("SP_TX_PROV_MANUFACTURER_INVALID: manufacturer is empty or invalid")]
    InvalidManufacturer,
    #[error("SP_TX_PROV_MODEL_INVALID: model is empty or invalid")]
    InvalidModel,
    #[error("SP_TX_PROV_FIRMWARE_HASH_FORMAT: firmware_hash must be a 64-character hex string")]
    InvalidFirmwareHash,
    #[error("SP_TX_PROV_CALIBRATION_CID_INVALID: calibration_attestation_cid is empty or invalid")]
    InvalidCalibrationAttestationCid,
}

impl RevealProvenanceValidationError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::EmptyProvenance => "SP_TX_PROV_EMPTY",
            Self::InvalidSourceId => "SP_TX_PROV_SOURCE_ID_INVALID",
            Self::InvalidManufacturer => "SP_TX_PROV_MANUFACTURER_INVALID",
            Self::InvalidModel => "SP_TX_PROV_MODEL_INVALID",
            Self::InvalidFirmwareHash => "SP_TX_PROV_FIRMWARE_HASH_FORMAT",
            Self::InvalidCalibrationAttestationCid => "SP_TX_PROV_CALIBRATION_CID_INVALID",
        }
    }
}

impl SensorProvenance {
    fn validate(&self) -> Result<(), RevealProvenanceValidationError> {
        let has_any_field = self.source_id.is_some()
            || self.manufacturer.is_some()
            || self.model.is_some()
            || self.firmware_hash.is_some()
            || self.calibration_attestation_cid.is_some();
        if !has_any_field {
            return Err(RevealProvenanceValidationError::EmptyProvenance);
        }

        validate_optional_provenance_text(
            self.source_id.as_deref(),
            MAX_PROVENANCE_SOURCE_ID_LEN,
            RevealProvenanceValidationError::InvalidSourceId,
        )?;
        validate_optional_provenance_text(
            self.manufacturer.as_deref(),
            MAX_PROVENANCE_DEVICE_TEXT_LEN,
            RevealProvenanceValidationError::InvalidManufacturer,
        )?;
        validate_optional_provenance_text(
            self.model.as_deref(),
            MAX_PROVENANCE_DEVICE_TEXT_LEN,
            RevealProvenanceValidationError::InvalidModel,
        )?;

        if let Some(firmware_hash) = self.firmware_hash.as_deref() {
            if firmware_hash.len() != 64 || !firmware_hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(RevealProvenanceValidationError::InvalidFirmwareHash);
            }
        }

        if let Some(cid) = self.calibration_attestation_cid.as_deref() {
            canonicalize_calibration_attestation_cid(cid)?;
        }

        Ok(())
    }
}

fn validate_optional_provenance_text(
    value: Option<&str>,
    max_len: usize,
    error: RevealProvenanceValidationError,
) -> Result<(), RevealProvenanceValidationError> {
    if let Some(value) = value {
        if value.trim() != value
            || value.is_empty()
            || value.len() > max_len
            || value.chars().any(char::is_control)
        {
            return Err(error);
        }
    }
    Ok(())
}

/// Canonicalize `provenance.calibration_attestation_cid` into a deterministic provider-bound form.
///
/// Canonical output always uses one of:
/// - `ipfs:<cid>`
/// - `filecoin:<cid>`
/// - `arweave:<tx_id>`
///
/// Bare CIDs are inferred as:
/// - IPFS/Filecoin-like multihash CID => `ipfs:<cid>`
/// - 43-char base64url Arweave tx id => `arweave:<tx_id>`
pub fn canonicalize_calibration_attestation_cid(
    cid: &str,
) -> Result<String, RevealProvenanceValidationError> {
    if cid.trim() != cid
        || cid.is_empty()
        || cid.len() > MAX_PROVENANCE_CALIBRATION_CID_LEN
        || cid.chars().any(char::is_control)
    {
        return Err(RevealProvenanceValidationError::InvalidCalibrationAttestationCid);
    }

    const PREFIXES: [(&str, &str); 8] = [
        ("ipfs://", "ipfs"),
        ("ipfs:", "ipfs"),
        ("/ipfs/", "ipfs"),
        ("filecoin://", "filecoin"),
        ("filecoin:", "filecoin"),
        ("arweave://", "arweave"),
        ("arweave:", "arweave"),
        ("ar://", "arweave"),
    ];

    for (prefix, provider) in PREFIXES {
        if let Some(stripped) = cid.strip_prefix(prefix) {
            if stripped.is_empty() {
                return Err(RevealProvenanceValidationError::InvalidCalibrationAttestationCid);
            }
            let is_valid = match provider {
                "ipfs" | "filecoin" => is_valid_ipfs_like_cid(stripped),
                "arweave" => is_valid_arweave_id(stripped),
                _ => false,
            };
            if !is_valid {
                return Err(RevealProvenanceValidationError::InvalidCalibrationAttestationCid);
            }
            return Ok(format!("{provider}:{stripped}"));
        }
    }

    if is_valid_ipfs_like_cid(cid) {
        return Ok(format!("ipfs:{cid}"));
    }
    if is_valid_arweave_id(cid) {
        return Ok(format!("arweave:{cid}"));
    }

    Err(RevealProvenanceValidationError::InvalidCalibrationAttestationCid)
}

fn strip_provider_prefix<'a>(
    cid: &'a str,
    provider: &str,
) -> Result<&'a str, RevealCidValidationError> {
    const PREFIXES: [(&str, &str); 8] = [
        ("ipfs://", "ipfs"),
        ("ipfs:", "ipfs"),
        ("/ipfs/", "ipfs"),
        ("filecoin://", "filecoin"),
        ("filecoin:", "filecoin"),
        ("arweave://", "arweave"),
        ("arweave:", "arweave"),
        ("ar://", "arweave"),
    ];

    for (prefix, expected_provider) in PREFIXES {
        if let Some(stripped) = cid.strip_prefix(prefix) {
            if expected_provider != provider {
                return Err(RevealCidValidationError::CidPrefixProviderMismatch);
            }
            if stripped.is_empty() {
                return Err(RevealCidValidationError::EmptyCid);
            }
            return Ok(stripped);
        }
    }

    Ok(cid)
}

fn validate_provider_specific_cid(
    cid: &str,
    provider: &str,
) -> Result<(), RevealCidValidationError> {
    match provider {
        "ipfs" | "filecoin" => validate_ipfs_like_cid(cid),
        "arweave" => validate_arweave_id(cid),
        _ => Err(RevealCidValidationError::InvalidStorageProvider),
    }
}

fn validate_ipfs_like_cid(cid: &str) -> Result<(), RevealCidValidationError> {
    if is_valid_ipfs_like_cid(cid) {
        Ok(())
    } else {
        Err(RevealCidValidationError::InvalidIpfsLikeCidFormat)
    }
}

fn validate_arweave_id(cid: &str) -> Result<(), RevealCidValidationError> {
    if is_valid_arweave_id(cid) {
        Ok(())
    } else {
        Err(RevealCidValidationError::InvalidArweaveTxIdFormat)
    }
}

fn is_valid_ipfs_like_cid(cid: &str) -> bool {
    let is_cid_v0 = cid.starts_with("Qm") && cid.len() == 46 && cid.chars().all(is_base58btc_char);
    let is_cid_v1 =
        cid.starts_with('b') && cid.len() >= 10 && cid.chars().all(is_lower_base32_char);
    is_cid_v0 || is_cid_v1
}

fn is_valid_arweave_id(cid: &str) -> bool {
    cid.len() == ARWEAVE_TX_ID_LEN && cid.chars().all(is_arweave_txid_char)
}

fn is_base58btc_char(c: char) -> bool {
    matches!(c, '1'..='9' | 'A'..='H' | 'J'..='N' | 'P'..='Z' | 'a'..='k' | 'm'..='z')
}

fn is_lower_base32_char(c: char) -> bool {
    matches!(c, 'a'..='z' | '2'..='7')
}

fn is_arweave_txid_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OracleAttestationPayload {
    #[serde(alias = "commit_hash")]
    pub commit_id: String,
    pub oracle_id: String,
    pub approved: bool,
    pub signature: Option<String>,
    pub confidence: Option<f64>,
    pub timestamp: i64,
    /// Unique nonce to prevent replay attacks. Included in the signing message.
    #[serde(default)]
    pub nonce: Option<String>,
}

impl OracleAttestationPayload {
    pub fn canonical_signing_message(&self) -> String {
        self.canonical_signing_message_with_domain(&signing_chain_id(), &signing_network_id())
    }

    /// Pure-domain form for offline ceremony verification. Production signing
    /// and verification continue to call [`Self::canonical_signing_message`],
    /// which delegates here using the runtime chain identity.
    pub fn canonical_signing_message_with_domain(
        &self,
        chain_id: &str,
        network_id: &str,
    ) -> String {
        #[derive(Serialize)]
        struct OracleAttestationSigningPayload<'a> {
            commit_id: &'a str,
            oracle_id: &'a str,
            approved: bool,
            confidence: Option<f64>,
            timestamp: i64,
            nonce: Option<&'a str>,
        }
        let signing_payload = OracleAttestationSigningPayload {
            commit_id: &self.commit_id,
            oracle_id: &self.oracle_id,
            approved: self.approved,
            confidence: self.confidence,
            timestamp: self.timestamp,
            nonce: self.nonce.as_deref(),
        };
        let digest = payload_digest_hex(&signing_payload);
        let context = DomainSigningContext {
            action: DomainSigningAction::OracleAttestation,
            signer_id: &self.oracle_id,
            timestamp: self.timestamp,
            nonce: self.nonce.as_deref(),
            chain_id,
            network_id,
        };
        canonical_signing_message(&context, &digest)
    }

    /// Canonical domain-separated signing message for oracle attestations.
    pub fn signing_message(&self) -> String {
        self.canonical_signing_message()
    }

    /// A signing payload is only well-formed if every `f64` field is finite:
    /// `serde_json` rejects NaN/Infinity, which now panics in
    /// `payload_digest_hex` (made loud in this PR). A `confidence` of
    /// NaN/Infinity is reachable over the bincode gossip path, so every
    /// signature-verification entry point must gate on this before computing
    /// `signing_message()`. A valid signer never signs a non-finite payload.
    pub fn is_signable(&self) -> bool {
        self.confidence.is_none_or(|c| c.is_finite())
    }

    pub fn has_valid_signature(&self) -> bool {
        if !self.is_signable() {
            return false;
        }
        let Some(signature_hex) = self.signature.as_deref() else {
            return false;
        };
        let public_key = match PublicKey::from_hex(&self.oracle_id) {
            Ok(pk) => pk,
            Err(_) => return false,
        };
        let signature = match Signature::from_hex(signature_hex) {
            Ok(sig) => sig,
            Err(_) => return false,
        };
        let message = self.signing_message();
        public_key.verify(message.as_bytes(), &signature)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationAttestationPayload {
    pub station_id: String,
    pub calibrator_id: String,
    pub issued_at: i64,
    pub expires_at: i64,
    #[serde(default)]
    pub calibrator_key_epoch: Option<u64>,
    pub signature: Option<String>,
    #[serde(default)]
    pub nonce: Option<String>,
}

#[derive(Serialize)]
struct CalibrationAttestationSigningPayload<'a> {
    station_id: &'a str,
    calibrator_id: &'a str,
    issued_at: i64,
    expires_at: i64,
    calibrator_key_epoch: Option<u64>,
    nonce: Option<&'a str>,
}

impl CalibrationAttestationPayload {
    pub fn canonical_signing_message(&self) -> String {
        let signing_payload = CalibrationAttestationSigningPayload {
            station_id: &self.station_id,
            calibrator_id: &self.calibrator_id,
            issued_at: self.issued_at,
            expires_at: self.expires_at,
            calibrator_key_epoch: self.calibrator_key_epoch,
            nonce: self.nonce.as_deref(),
        };
        let digest = payload_digest_hex(&signing_payload);
        let cid = signing_chain_id();
        let nid = signing_network_id();
        let context = DomainSigningContext {
            action: DomainSigningAction::CalibrationAttestation,
            signer_id: &self.calibrator_id,
            timestamp: self.issued_at,
            nonce: self.nonce.as_deref(),
            chain_id: &cid,
            network_id: &nid,
        };
        canonical_signing_message(&context, &digest)
    }

    pub fn signing_message(&self) -> String {
        self.canonical_signing_message()
    }

    pub fn has_valid_signature(&self) -> bool {
        let Some(signature_hex) = self.signature.as_deref() else {
            return false;
        };
        let public_key = match PublicKey::from_hex(&self.calibrator_id) {
            Ok(pk) => pk,
            Err(_) => return false,
        };
        let signature = match Signature::from_hex(signature_hex) {
            Ok(sig) => sig,
            Err(_) => return false,
        };
        let message = self.signing_message();
        public_key.verify(message.as_bytes(), &signature)
    }

    pub fn has_valid_signature_with_registry(&self, signing_keys: &SigningKeyRegistry) -> bool {
        let Some(signature_hex) = self.signature.as_deref() else {
            return false;
        };
        let message = self.signing_message();
        if let Some(epoch) = self.calibrator_key_epoch {
            return signing_keys.verify_signature_for_identity_role_epoch(
                &self.calibrator_id,
                SigningRole::Calibrator,
                epoch,
                self.issued_at,
                message.as_bytes(),
                signature_hex,
            );
        }
        signing_keys.verify_signature_for_identity_role(
            &self.calibrator_id,
            SigningRole::Calibrator,
            self.issued_at,
            message.as_bytes(),
            signature_hex,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GovernanceProposalPayload {
    pub proposal_id: String,
    pub parameter: String,
    pub value: String,
    pub voting_deadline: Option<u64>,
    pub deposit: Option<u128>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GovernanceVotePayload {
    pub proposal_id: String,
    pub approve: bool,
    pub voting_power: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer_public_key: Option<String>,
}

impl GovernanceVotePayload {
    /// Legacy V1 signing message (pre-#2123): `governance-vote-v1|<id>|<approve>`.
    /// Binds neither chain id nor network id. Used only while the chain runs at
    /// protocol version V1; superseded by the domain-separated preimage at V2
    /// (the #2034 cut). Kept so signatures produced before the cut keep
    /// verifying, and so the migration is a clean version gate rather than a
    /// rolling V1/V2 asymmetry (#2229).
    pub fn canonical_signing_message(&self) -> String {
        format!(
            "governance-vote-v1|{}|{}",
            self.proposal_id,
            if self.approve { "approve" } else { "reject" }
        )
    }

    /// Verify under the process's active protocol version and chain identity.
    pub fn verify_vote_signature(&self) -> bool {
        self.verify_vote_signature_for(
            crate::crypto::chain_binding::SigningProtocolVersion::active(),
            crate::crypto::chain_binding::ChainIdentity::active(),
        )
    }

    /// Verify using the signing protocol version scheduled for `height`.
    ///
    /// Consensus callers must use this form: a process-wide version is not
    /// sufficient once an activation schedule can select different preimages
    /// at different heights (#2500).
    pub fn verify_vote_signature_at_height(&self, height: u64) -> bool {
        self.verify_vote_signature_for(
            crate::crypto::chain_binding::protocol_version_at(height),
            crate::crypto::chain_binding::ChainIdentity::active(),
        )
    }

    /// #2229: version-gated verification. At **V1** the exact legacy
    /// `governance-vote-v1|<id>|<approve>` preimage is used, so V1 behaviour is
    /// unchanged and pre-cut signatures keep verifying. At **V2** — active from
    /// the #2034 fresh-genesis cut — the domain-separated preimage binds the tag,
    /// signer, chain id and network id, closing cross-network replay. Gating on
    /// the active version (rather than always taking the domain path) avoids the
    /// rolling asymmetry the always-domain form produced: at V1 the domain frame
    /// would both reject existing legacy signatures AND still omit the
    /// chain/network binding. Takes an explicit version + identity so callers and
    /// tests are not tied to ambient `OnceLock` state.
    pub fn verify_vote_signature_for(
        &self,
        version: crate::crypto::chain_binding::SigningProtocolVersion,
        identity: &crate::crypto::chain_binding::ChainIdentity,
    ) -> bool {
        use crate::crypto::chain_binding::SigningProtocolVersion;
        let (Some(sig_hex), Some(pk_hex)) = (&self.signature, &self.signer_public_key) else {
            return false;
        };
        let Ok(public_key) = crate::crypto::signature::PublicKey::from_hex(pk_hex) else {
            return false;
        };
        let Ok(signature) = crate::crypto::signature::Signature::from_hex(sig_hex) else {
            return false;
        };
        match version {
            SigningProtocolVersion::V1 => {
                public_key.verify(self.canonical_signing_message().as_bytes(), &signature)
            },
            SigningProtocolVersion::V2 => {
                use crate::crypto::domain_signing::DomainSeparated;
                public_key.verify(&self.signing_preimage_for(identity, version), &signature)
            },
        }
    }
}

/// Payload for executing an approved governance proposal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GovernanceExecutePayload {
    pub proposal_id: String,
}

/// Payload for finalizing a governance proposal (closing its voting period).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GovernanceFinalizePayload {
    pub proposal_id: String,
}

/// Payload for delegating voting power to another address.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GovernanceDelegatePayload {
    /// Address to delegate voting power to.
    pub delegate_to: String,
    /// Optional amount of voting power to delegate. If None, delegates all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<u64>,
}

/// Payload for depositing funds into the governance treasury.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TreasuryDepositPayload {
    /// Amount to deposit in base units.
    pub amount: u64,
    /// Optional memo for the deposit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
}

/// Payload for withdrawing funds from the governance treasury.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TreasuryWithdrawPayload {
    /// Amount to withdraw in base units.
    pub amount: u64,
    /// Recipient address.
    pub recipient: String,
    /// Proposal ID authorizing this withdrawal.
    pub proposal_id: String,
    /// Optional memo for the withdrawal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
}

/// Re-exported from `wrf-primitives` — single source of truth for `PrivacyLevel`.
pub use wrf_primitives::PrivacyLevel;

/// Payload for confidential transfer transactions.
///
/// Contains cryptographic proofs and privacy data needed to verify
/// a confidential transfer without revealing amounts or participants.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfidentialTransferPayload {
    /// Hex-encoded Pedersen commitment(s) for output amounts.
    pub output_commitments: Vec<String>,
    /// Hex-encoded range proof data.
    pub range_proof: Option<String>,
    /// Hex-encoded stealth address data (ephemeral key + one-time address).
    pub stealth_data: Option<String>,
    /// Hex-encoded ring signature data.
    pub ring_signature: Option<String>,
    /// Hex-encoded key image(s) for double-spend prevention.
    pub key_images: Vec<String>,
    /// Hex-encoded encrypted payload for recipient.
    pub encrypted_payload: Option<String>,
    /// Privacy level used for this transaction.
    pub privacy_level: PrivacyLevel,
    /// Optional selective disclosures for compliance-safe attestations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selective_disclosures: Vec<SelectiveDisclosureProofPayload>,
    /// Hex-encoded message signed by the ring signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_message: Option<String>,
}

/// Domain separator for confidential-transfer MLSAG authorization.
///
/// The legacy verifier accepted any caller-supplied `signed_message`, even when
/// it had no relationship to the payload. The v2 message covers the active
/// chain identity and every consensus-relevant payload field except the
/// signature containers themselves.
pub const CONFIDENTIAL_TRANSFER_AUTH_DOMAIN: &[u8] =
    b"wrfcoin_confidential_transfer_auth_v2";

impl ConfidentialTransferPayload {
    /// Canonical bytes the confidential-transfer ring signs.
    pub fn authorization_message(&self) -> Vec<u8> {
        confidential_transfer_authorization_message_with_domain(
            &signing_chain_id(),
            &signing_network_id(),
            self,
        )
    }
}

/// Pure confidential-transfer preimage builder for clients and golden vectors.
pub fn confidential_transfer_authorization_message_with_domain(
    chain_id: &str,
    network_id: &str,
    payload: &ConfidentialTransferPayload,
) -> Vec<u8> {
    fn push_field(buf: &mut Vec<u8>, field: &[u8]) {
        buf.extend_from_slice(&(field.len() as u32).to_le_bytes());
        buf.extend_from_slice(field);
    }
    fn push_optional_field(buf: &mut Vec<u8>, field: Option<&str>) {
        match field {
            Some(field) => {
                buf.push(1);
                push_field(buf, field.as_bytes());
            },
            None => buf.push(0),
        }
    }

    let mut buf = Vec::with_capacity(CONFIDENTIAL_TRANSFER_AUTH_DOMAIN.len() + 512);
    buf.extend_from_slice(CONFIDENTIAL_TRANSFER_AUTH_DOMAIN);
    push_chain_domain(&mut buf, chain_id, network_id);

    buf.extend_from_slice(&(payload.output_commitments.len() as u32).to_le_bytes());
    for commitment in &payload.output_commitments {
        push_field(&mut buf, commitment.as_bytes());
    }
    push_optional_field(&mut buf, payload.range_proof.as_deref());
    push_optional_field(&mut buf, payload.stealth_data.as_deref());
    buf.extend_from_slice(&(payload.key_images.len() as u32).to_le_bytes());
    for key_image in &payload.key_images {
        push_field(&mut buf, key_image.as_bytes());
    }
    push_optional_field(&mut buf, payload.encrypted_payload.as_deref());
    buf.push(match payload.privacy_level {
        PrivacyLevel::Standard => 0,
        PrivacyLevel::Basic => 1,
        PrivacyLevel::Medium => 2,
        PrivacyLevel::High => 3,
        PrivacyLevel::Maximum => 4,
    });

    buf.extend_from_slice(&(payload.selective_disclosures.len() as u32).to_le_bytes());
    for disclosure in &payload.selective_disclosures {
        push_field(&mut buf, disclosure.disclosure_id.as_bytes());
        let mut scope = disclosure
            .scope
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        scope.sort_unstable();
        buf.extend_from_slice(&(scope.len() as u32).to_le_bytes());
        for key in scope {
            push_field(&mut buf, key.as_bytes());
        }
        buf.extend_from_slice(&disclosure.expires_at_block.to_le_bytes());
        push_field(&mut buf, disclosure.disclosed_fields_hash.as_bytes());
        push_field(&mut buf, disclosure.verifier_attestation.as_bytes());
    }
    buf
}

/// Payload for moving transparent funds into shielded pool state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShieldedDepositPayload {
    /// New shielded commitment created by this deposit.
    pub shielded_commitment: String,
    /// Hex-encoded zero-knowledge range proof.
    pub range_proof: String,
    /// Amount removed from transparent balance and added to shielded pool.
    pub transparent_amount: u64,
}

/// Payload for moving shielded funds back into transparent balances.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShieldedWithdrawalPayload {
    /// Unique nullifier preventing double-withdrawals.
    pub nullifier: String,
    /// Existing shielded commitment being spent.
    pub shielded_commitment: String,
    /// Hex-encoded zero-knowledge range proof.
    pub range_proof: String,
    /// Transparent recipient account address as hex.
    pub transparent_recipient: String,
    /// Amount released to transparent recipient.
    pub transparent_amount: u64,
    /// Hex-encoded spend authorization proof.
    pub authorization_proof: String,
    /// Optional selective disclosure proof scoped to this withdrawal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selective_disclosure: Option<SelectiveDisclosureProofPayload>,
}

/// Selective disclosure proof payload for compliance-safe attestations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectiveDisclosureProofPayload {
    /// Stable identifier of this disclosure proof.
    pub disclosure_id: String,
    /// Disclosure scope keys (minimal, explicit fields only).
    pub scope: Vec<String>,
    /// Block height after which disclosure is no longer valid.
    pub expires_at_block: u64,
    /// Deterministic hash of disclosed fields.
    pub disclosed_fields_hash: String,
    /// Attestor signature/proof bytes encoded as hex.
    pub verifier_attestation: String,
}

/// Domain separator for selective-disclosure verifier attestations.
///
/// `v2` replaces the untagged `field||field` digest. The live verifier now
/// binds the proof to the active chain and network as well as every proof
/// field, so the same attestation cannot validate under another chain identity.
pub const SELECTIVE_DISCLOSURE_AUTH_DOMAIN: &[u8] =
    b"wrfcoin_selective_disclosure_auth_v2";

impl SelectiveDisclosureProofPayload {
    /// Compute the chain-bound digest covered by `verifier_attestation`.
    pub fn binding_digest(&self) -> Hash {
        selective_disclosure_authorization_digest_with_domain(
            &signing_chain_id(),
            &signing_network_id(),
            self,
        )
    }

    /// Check if this disclosure proof has expired at the given block height.
    pub fn is_expired(&self, current_height: u64) -> bool {
        current_height >= self.expires_at_block
    }

    /// Verify that `verifier_attestation` is a valid ed25519 signature over
    /// this proof's `binding_digest()`, produced by `verifier_public_key`.
    pub fn has_valid_attestation(&self, verifier_public_key: &[u8; 32]) -> bool {
        let attestation_bytes = match hex::decode(&self.verifier_attestation) {
            Ok(b) if b.len() == 64 => b,
            _ => return false,
        };
        let sig_bytes: [u8; 64] = match attestation_bytes.try_into() {
            Ok(b) => b,
            Err(_) => return false,
        };
        let vk = match ed25519_dalek::VerifyingKey::from_bytes(verifier_public_key) {
            Ok(k) => k,
            Err(_) => return false,
        };
        let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
        let digest = self.binding_digest();
        vk.verify_strict(digest.as_bytes(), &sig).is_ok()
    }

    /// Full verification: not expired, valid attestation, non-empty scope.
    pub fn verify(&self, current_height: u64, verifier_public_key: &[u8; 32]) -> bool {
        !self.is_expired(current_height)
            && self.has_valid_attestation(verifier_public_key)
            && !self.scope.is_empty()
            && !self.disclosure_id.is_empty()
    }

    /// Compute the canonical hash of disclosed fields.
    /// Fields are sorted by key for deterministic ordering.
    pub fn compute_disclosed_fields_hash(fields: &[(&str, &str)]) -> String {
        let mut sorted: Vec<_> = fields.to_vec();
        sorted.sort_by_key(|(k, _)| *k);
        let canonical = sorted
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(";");
        hash_bytes(canonical.as_bytes()).to_string()
    }

    /// Generate a new selective disclosure proof.
    pub fn generate(
        disclosure_id: String,
        scope: Vec<String>,
        expires_at_block: u64,
        disclosed_fields: &[(&str, &str)],
        verifier_attestation: String,
    ) -> Self {
        let disclosed_fields_hash = Self::compute_disclosed_fields_hash(disclosed_fields);
        Self {
            disclosure_id,
            scope,
            expires_at_block,
            disclosed_fields_hash,
            verifier_attestation,
        }
    }
}

/// Pure selective-disclosure preimage builder for cross-language fixtures.
///
/// Scope keys are sorted before framing because the state validator treats the
/// scope as a set. The attestation field is excluded because it contains the
/// signature over this digest.
pub fn selective_disclosure_authorization_digest_with_domain(
    chain_id: &str,
    network_id: &str,
    proof: &SelectiveDisclosureProofPayload,
) -> Hash {
    let mut scope = proof.scope.iter().map(String::as_str).collect::<Vec<_>>();
    scope.sort_unstable();

    let mut buf = Vec::with_capacity(SELECTIVE_DISCLOSURE_AUTH_DOMAIN.len() + 192);
    buf.extend_from_slice(SELECTIVE_DISCLOSURE_AUTH_DOMAIN);
    push_chain_domain(&mut buf, chain_id, network_id);
    buf.extend_from_slice(&(proof.disclosure_id.len() as u32).to_le_bytes());
    buf.extend_from_slice(proof.disclosure_id.as_bytes());
    buf.extend_from_slice(&(scope.len() as u32).to_le_bytes());
    for key in scope {
        buf.extend_from_slice(&(key.len() as u32).to_le_bytes());
        buf.extend_from_slice(key.as_bytes());
    }
    buf.extend_from_slice(&proof.expires_at_block.to_le_bytes());
    buf.extend_from_slice(&(proof.disclosed_fields_hash.len() as u32).to_le_bytes());
    buf.extend_from_slice(proof.disclosed_fields_hash.as_bytes());
    hash_bytes(&buf)
}

/// Payload for explicit transparent transfers (privacy opt-out).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransparentTransferPayload {
    /// Sender transparent address.
    pub from: String,
    /// Recipient transparent address.
    pub to: String,
    /// Amount transferred in transparent mode.
    pub amount: u64,
    /// Must be true when sender intentionally opts out of privacy defaults.
    #[serde(default)]
    pub transparent_opt_out: bool,
    /// Optional audit-safe hash describing opt-out context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opt_out_audit_tag: Option<String>,
    /// Hex-encoded ed25519 public key that authorizes debiting `from` (#1727).
    ///
    /// A transparent transfer is only authorized when this key's derived
    /// address equals `from` and `auth_signature` verifies over the canonical
    /// authorization message. This binds the authorization to the account being
    /// debited, so a third party cannot forge a `from=victim` transfer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_pubkey: Option<String>,
    /// Hex-encoded ed25519 signature over the canonical authorization message
    /// (`transparent_transfer_authorization_message`), produced by the holder of
    /// the `from` account key (#1727).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_signature: Option<String>,
    /// Governance-operator identity that authorized this system/governance
    /// transfer (#1727). Present only on system-originated transfers that carry
    /// verifiable provenance; absent (`None`) on ordinary user transfers and on
    /// legacy structural-only system transfers produced before provenance
    /// signing shipped. When present, [`Transaction::verify_system_authority`]
    /// checks `authority_signature` against this identity's `GovernanceOperator`
    /// key in the chain signing-key registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<String>,
    /// Hex-encoded ed25519 signature by `authority`'s `GovernanceOperator` key
    /// over [`Transaction::system_transfer_authorization_message`] (#1727).
    /// Binds the transfer's `from`/`to`/`amount`/`nonce` to a key the sync path
    /// can verify, so a forged unsigned transparent Transfer is no longer
    /// structurally indistinguishable from a legitimate governance transfer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_signature: Option<String>,
    /// WebAuthn P-256 passkey authorization of the debit (#2169), the
    /// alternative to `auth_pubkey`/`auth_signature` for a passkey-controlled
    /// `from` account. Verified only by
    /// [`Self::verify_authorization_at_height`] once
    /// `ConsensusFeature::PasskeyP256Auth` is active; [`Self::verify_authorization`]
    /// (the ed25519 rule) never reads it. Deserialised leniently — a malformed
    /// member reads as `None` — so a payload a pre-#2169 binary parses is parsed
    /// identically here (see `accounts::passkey::deserialize_lenient_passkey`).
    #[serde(
        default,
        deserialize_with = "crate::accounts::passkey::deserialize_lenient_passkey",
        skip_serializing_if = "Option::is_none"
    )]
    pub passkey_auth: Option<crate::accounts::passkey::PasskeyAuthorization>,
}

/// Domain separator for transparent-transfer authorization signatures (#1727).
///
/// Distinct from `wrfcoin_tx_sighash_v2` so an authorization signature can never
/// be replayed as an input signature (or vice versa).
pub const TRANSPARENT_TRANSFER_AUTH_DOMAIN: &[u8] = b"wrfcoin_transparent_transfer_auth_v2";
/// Domain separator for governance/system transparent-transfer provenance.
///
/// This is distinct from the sender's [`TRANSPARENT_TRANSFER_AUTH_DOMAIN`]:
/// the two signatures authorize different roles even when they cover the same
/// economic fields.
pub const SYSTEM_TRANSFER_AUTH_DOMAIN: &[u8] = b"wrfcoin_system_transfer_auth_v2";

/// Build the canonical message a `from`-account holder signs to authorize a
/// transparent transfer (#1727).
///
/// Binds `from`, `to`, `amount`, and the transaction `nonce` under a dedicated
/// domain tag. The `auth_pubkey`/`auth_signature` fields are deliberately
/// excluded — they are the signature *over* this message, not part of it.
pub fn transparent_transfer_authorization_message(
    from: &str,
    to: &str,
    amount: u64,
    nonce: u64,
) -> Vec<u8> {
    transparent_transfer_authorization_message_with_domain(
        &signing_chain_id(),
        &signing_network_id(),
        from,
        to,
        amount,
        nonce,
    )
}

/// Pure form of [`transparent_transfer_authorization_message`] with the chain
/// identity passed explicitly (#2034 golden-vector / cross-signer parity).
pub fn transparent_transfer_authorization_message_with_domain(
    chain_id: &str,
    network_id: &str,
    from: &str,
    to: &str,
    amount: u64,
    nonce: u64,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(TRANSPARENT_TRANSFER_AUTH_DOMAIN.len() + 96);
    buf.extend_from_slice(TRANSPARENT_TRANSFER_AUTH_DOMAIN);
    push_chain_domain(&mut buf, chain_id, network_id);
    buf.extend_from_slice(&(from.len() as u32).to_le_bytes());
    buf.extend_from_slice(from.as_bytes());
    buf.extend_from_slice(&(to.len() as u32).to_le_bytes());
    buf.extend_from_slice(to.as_bytes());
    buf.extend_from_slice(&amount.to_le_bytes());
    buf.extend_from_slice(&nonce.to_le_bytes());
    buf
}

/// Pure system/governance transfer preimage builder for golden vectors.
pub fn system_transfer_authorization_message_with_domain(
    chain_id: &str,
    network_id: &str,
    from: &str,
    to: &str,
    amount: u64,
    nonce: u64,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(SYSTEM_TRANSFER_AUTH_DOMAIN.len() + 96);
    buf.extend_from_slice(SYSTEM_TRANSFER_AUTH_DOMAIN);
    push_chain_domain(&mut buf, chain_id, network_id);
    buf.extend_from_slice(&(from.len() as u32).to_le_bytes());
    buf.extend_from_slice(from.as_bytes());
    buf.extend_from_slice(&(to.len() as u32).to_le_bytes());
    buf.extend_from_slice(to.as_bytes());
    buf.extend_from_slice(&amount.to_le_bytes());
    buf.extend_from_slice(&nonce.to_le_bytes());
    buf
}

impl TransparentTransferPayload {
    /// Canonical authorization message for this payload at `nonce`.
    pub fn authorization_message(&self, nonce: u64) -> Vec<u8> {
        transparent_transfer_authorization_message(&self.from, &self.to, self.amount, nonce)
    }

    /// Sign this payload's authorization with the `from`-account private key,
    /// populating `auth_pubkey`/`auth_signature` (#1727).
    ///
    /// The caller is responsible for ensuring `private_key` corresponds to the
    /// `from` address; `verify_authorization` independently re-checks that
    /// binding, so a mismatched key produces a payload that fails verification.
    pub fn sign_authorization(&mut self, private_key: &PrivateKey, nonce: u64) {
        let message = self.authorization_message(nonce);
        let signature = private_key.sign(&message);
        let public_key = PublicKey::from_private_key(private_key);
        self.auth_pubkey = Some(public_key.to_hex());
        self.auth_signature = Some(signature.to_hex());
    }

    /// Verify the embedded authorization is valid and bound to `from` (#1727).
    ///
    /// Fail-closed: returns `false` unless both `auth_pubkey` and
    /// `auth_signature` are present, hex-decodable, the public key's derived
    /// address equals the (hex-parseable) `from` account, and the signature
    /// verifies over the canonical authorization message at `nonce`.
    pub fn verify_authorization(&self, nonce: u64) -> bool {
        self.verify_authorization_with_view(&EmptyAuthorizationView, nonce, 0, Timestamp::new(0))
    }

    /// Consensus callers supply the pre-block declaration view and the block
    /// coordinates; API/mempool callers retain the dormant default above.
    pub fn verify_authorization_with_view<V: crate::accounts::authorization::AuthorizationView>(
        &self, view: &V, nonce: u64, height: u64, timestamp: Timestamp,
    ) -> bool {
        let (Some(pk_hex), Some(sig_hex)) =
            (self.auth_pubkey.as_deref(), self.auth_signature.as_deref())
        else {
            return false;
        };
        // Bind the signer to the account being debited. `from` must be a real
        // hex address; protocol transfers debit a keyed account, never a label.
        let Ok(from_address) = self.from.parse::<Address>() else {
            return false;
        };
        authorize_debit(
            view,
            &from_address,
            &self.authorization_message(nonce),
            SpendAuthorization {
                signer_public_key: pk_hex,
                signature: sig_hex,
            },
            height,
            timestamp,
        )
        .is_ok()
    }

    /// Whether this payload carries a (well-formed) passkey authorization
    /// (#2169). Says nothing about validity.
    pub fn has_passkey_authorization(&self) -> bool {
        self.passkey_auth.is_some()
    }

    /// Verify the embedded WebAuthn P-256 passkey authorization, bound to
    /// `from` (#2169). **Ungated**: this is the cryptographic check alone.
    /// Fail-closed: `false` unless `passkey_auth` is present, its credential
    /// derives `from`, and the assertion verifies for this transfer's
    /// challenge under the consensus policy.
    pub fn verify_passkey_authorization(&self, nonce: u64) -> bool {
        match &self.passkey_auth {
            Some(auth) => auth
                .verify_for_transfer(&self.from, &self.to, self.amount, nonce)
                .is_ok(),
            None => false,
        }
    }

    /// Authorization at a consensus height (#2169): the ed25519 rule
    /// ([`Self::verify_authorization`], unchanged at every height) OR, once
    /// `ConsensusFeature::PasskeyP256Auth` is active at `height`, a valid
    /// passkey authorization. Below activation this is exactly
    /// `verify_authorization`, so a passkey-only payload is refused as it is
    /// today. Used by block validation and state apply, which both know
    /// their height.
    pub fn verify_authorization_at_height(&self, nonce: u64, height: u64) -> bool {
        self.verify_authorization_at_height_with_view(
            &EmptyAuthorizationView,
            nonce,
            height,
            Timestamp::new(0),
        )
    }

    /// Height-aware authorization using the consensus declaration view
    /// (#2168 + #2169). The declared ed25519 rule and the activated passkey
    /// alternative are evaluated without dropping either policy path.
    pub fn verify_authorization_at_height_with_view<
        V: crate::accounts::authorization::AuthorizationView,
    >(
        &self,
        view: &V,
        nonce: u64,
        height: u64,
        timestamp: Timestamp,
    ) -> bool {
        self.verify_authorization_with_view(view, nonce, height, timestamp)
            || (crate::accounts::passkey::passkey_auth_active(height)
                && self.verify_passkey_authorization(nonce))
    }

    /// Authorization for mempool admission, which has no height (#2169): the
    /// ed25519 rule OR, on a chain that schedules `passkey_p256_auth` at all
    /// (genesis/bootstrap base OR an applied governance schedule extension,
    /// via `passkey_auth_scheduled`), a valid passkey authorization. The exact
    /// activation height is enforced downstream by the producer self-screen,
    /// block validation and state apply; on a chain that never schedules the
    /// feature this is exactly `verify_authorization`.
    pub fn verify_authorization_for_admission(&self, nonce: u64) -> bool {
        self.verify_authorization(nonce)
            || (crate::accounts::passkey::passkey_auth_scheduled()
                && self.verify_passkey_authorization(nonce))
    }
}

/// Payload carried by `Stake` / `Unstake` transactions binding the staking
/// action to a verifiable, `account`-bound ed25519 authorization (#1763).
///
/// Pre-#1763, Stake/Unstake were applied purely structurally — the extractor
/// read `tx.sender_address` (spoofable) and `tx.outputs[0].amount` with no
/// signature and no nonce, so anyone who landed a transaction could freeze any
/// account's spendable balance (`LockToStake`) or manipulate stake-derived
/// voting power / validator eligibility. This payload binds the mutation to the
/// holder of the `account` key, exactly as `TransparentTransferPayload` binds a
/// transparent transfer to `from` (#1727).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StakeAuthPayload {
    /// Account whose stake is locked (`Stake`) or released (`Unstake`), as a hex
    /// address. The authorization public key must derive to this address.
    pub account: String,
    /// Amount to lock to stake (`Stake`) or release to spendable (`Unstake`).
    pub amount: u64,
    /// Hex-encoded ed25519 public key that authorizes the stake mutation on
    /// `account` (#1763). Authorization is valid only when this key's derived
    /// address equals `account` and `auth_signature` verifies over the canonical
    /// authorization message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_pubkey: Option<String>,
    /// Hex-encoded ed25519 signature over the canonical stake-authorization
    /// message (`stake_authorization_message`), produced by the holder of the
    /// `account` key (#1763).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_signature: Option<String>,
}

/// Domain separator for stake/unstake authorization signatures (#1763).
///
/// Distinct from `wrfcoin_tx_sighash_v2` and the transparent-transfer auth tag
/// so a stake authorization can never be replayed as an input signature or a
/// transfer authorization (or vice versa).
pub const STAKE_AUTH_DOMAIN: &[u8] = b"wrfcoin_stake_auth_v2";

/// Build the canonical message an `account` holder signs to authorize a stake
/// mutation (#1763).
///
/// Binds `account`, `amount`, the action (`is_unstake`), and the transaction
/// `nonce` under a dedicated domain tag, plus the runtime chain identity for
/// cross-network replay protection (#2034). Binding `is_unstake` prevents a `Stake`
/// authorization from being replayed as an `Unstake` (or vice versa); binding
/// `nonce` makes each authorization single-use under account-nonce / mempool
/// replay enforcement. The `auth_pubkey`/`auth_signature` fields are
/// deliberately excluded — they are the signature *over* this message. Use
/// [`stake_authorization_message_with_domain`] to pass the identity explicitly.
pub fn stake_authorization_message(
    account: &str,
    amount: u64,
    is_unstake: bool,
    nonce: u64,
) -> Vec<u8> {
    stake_authorization_message_with_domain(
        &signing_chain_id(),
        &signing_network_id(),
        account,
        amount,
        is_unstake,
        nonce,
    )
}

/// Pure form of [`stake_authorization_message`] with the chain identity passed
/// explicitly (#2034 golden-vector / cross-signer parity).
pub fn stake_authorization_message_with_domain(
    chain_id: &str,
    network_id: &str,
    account: &str,
    amount: u64,
    is_unstake: bool,
    nonce: u64,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(STAKE_AUTH_DOMAIN.len() + 80);
    buf.extend_from_slice(STAKE_AUTH_DOMAIN);
    push_chain_domain(&mut buf, chain_id, network_id);
    buf.extend_from_slice(&(account.len() as u32).to_le_bytes());
    buf.extend_from_slice(account.as_bytes());
    buf.extend_from_slice(&amount.to_le_bytes());
    buf.push(is_unstake as u8);
    buf.extend_from_slice(&nonce.to_le_bytes());
    buf
}

impl StakeAuthPayload {
    /// Canonical authorization message for this payload at `nonce` for the given
    /// action (`is_unstake`).
    pub fn authorization_message(&self, is_unstake: bool, nonce: u64) -> Vec<u8> {
        stake_authorization_message(&self.account, self.amount, is_unstake, nonce)
    }

    /// Sign this payload's authorization with the `account` private key,
    /// populating `auth_pubkey`/`auth_signature` (#1763).
    ///
    /// The caller is responsible for ensuring `private_key` corresponds to the
    /// `account` address; `verify_authorization` independently re-checks that
    /// binding, so a mismatched key produces a payload that fails verification.
    pub fn sign_authorization(&mut self, private_key: &PrivateKey, is_unstake: bool, nonce: u64) {
        let message = self.authorization_message(is_unstake, nonce);
        let signature = private_key.sign(&message);
        let public_key = PublicKey::from_private_key(private_key);
        self.auth_pubkey = Some(public_key.to_hex());
        self.auth_signature = Some(signature.to_hex());
    }

    /// Verify the embedded authorization is valid and bound to `account` (#1763).
    ///
    /// Fail-closed: returns `false` unless both `auth_pubkey` and
    /// `auth_signature` are present, hex-decodable, the public key's derived
    /// address equals the (hex-parseable) `account`, and the signature verifies
    /// over the canonical authorization message for `is_unstake` at `nonce`.
    pub fn verify_authorization(&self, is_unstake: bool, nonce: u64) -> bool {
        // Pre-block callers have no State, so they can only prove the legacy
        // signer-is-sender rule; block application supplies a State view below.
        self.verify_authorization_with_view(
            &EmptyAuthorizationView,
            is_unstake,
            nonce,
            0,
            Timestamp::new(0),
        )
    }

    pub fn verify_authorization_with_view<V: crate::accounts::authorization::AuthorizationView>(
        &self,
        view: &V,
        is_unstake: bool,
        nonce: u64,
        height: u64,
        timestamp: Timestamp,
    ) -> bool {
        let (Some(pk_hex), Some(sig_hex)) =
            (self.auth_pubkey.as_deref(), self.auth_signature.as_deref())
        else {
            return false;
        };
        // Bind the signer to the account whose stake is mutated. `account` must
        // be a real hex address; staking locks/releases a keyed account balance.
        let Ok(account_address) = self.account.parse::<Address>() else {
            return false;
        };
        authorize_debit(
            view,
            &account_address,
            &self.authorization_message(is_unstake, nonce),
            SpendAuthorization {
                signer_public_key: pk_hex,
                signature: sig_hex,
            },
            height,
            timestamp,
        )
        .is_ok()
    }
}

/// Payload for submitting a layer2 rollup batch with a ZK proof.
///
/// The `proof_hex` field must be the hex-encoded raw Groth16 proof bytes
/// (i.e. `hex::encode(&zk_proof.proof_bytes)`).
/// The `commitment_hex` field is the hex-encoded little-endian BN254 Fr commitment
/// stored in `Proof::public_inputs[0]` by `ZkVerifier::generate_batch_proof`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Layer2BatchSubmitPayload {
    /// Identifier of the rollup batch being finalized.
    pub batch_id: u64,
    /// State root before this batch was applied.
    pub prev_state_root: String,
    /// State root after this batch was applied.
    pub state_root: String,
    /// Hex-encoded raw Groth16 proof bytes (`zk_proof.proof_bytes` in hex).
    pub proof_hex: String,
    /// Hex-encoded little-endian BN254 Fr commitment (`Proof::public_inputs[0]`).
    pub commitment_hex: String,
    /// Address or identifier of the rollup operator submitting this batch.
    pub operator: String,
}

/// Number of blocks per reward claim epoch.
pub const REWARD_CLAIM_EPOCH_BLOCKS: u64 = 1000;
/// Minimum ring size for anonymous reward claims.
pub const REWARD_CLAIM_MIN_RING_SIZE: usize = 7;
/// Block height at which anonymous reward claims activate.
pub const REWARD_CLAIM_ACTIVATION_HEIGHT: u64 = 0;

/// Payload for anonymous reward claims using MLSAG ring signatures.
///
/// The claimant proves membership in a set of eligible station keys without
/// revealing which station they control. A Pedersen commitment hides the reward
/// amount, and the key image prevents double-claims within an epoch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RewardClaimPayload {
    /// Epoch number (`current_block / REWARD_CLAIM_EPOCH_BLOCKS`).
    pub epoch: u64,
    /// Hex-encoded JSON-serialized MLSAG ring signature.
    pub ring_signature: String,
    /// Hex-encoded 32-byte key image for double-claim prevention.
    pub key_image: String,
    /// Hex-encoded Pedersen commitment for the reward amount.
    pub amount_commitment: String,
    /// Hex-encoded range proof JSON.
    pub range_proof: String,
    /// Destination address for the reward.
    pub destination: String,
}

/// Domain separator for anonymous reward-claim MLSAG signatures.
///
/// `v2` replaces the legacy `reward_claim_v1:<epoch>:<commitment>` string,
/// which did not bind the chain identity or the remaining claim fields.
pub const REWARD_CLAIM_AUTH_DOMAIN: &[u8] = b"wrfcoin_reward_claim_auth_v2";

impl RewardClaimPayload {
    /// Canonical bytes covered by the reward-claim ring signature.
    ///
    /// The ring signature itself is excluded. Every other consensus-relevant
    /// field is length-framed and bound to the active chain and network.
    pub fn authorization_message(&self) -> Vec<u8> {
        reward_claim_authorization_message_with_domain(
            &signing_chain_id(),
            &signing_network_id(),
            self,
        )
    }
}

/// Pure reward-claim preimage builder for cross-language golden vectors.
pub fn reward_claim_authorization_message_with_domain(
    chain_id: &str,
    network_id: &str,
    payload: &RewardClaimPayload,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(REWARD_CLAIM_AUTH_DOMAIN.len() + 256);
    buf.extend_from_slice(REWARD_CLAIM_AUTH_DOMAIN);
    push_chain_domain(&mut buf, chain_id, network_id);
    buf.extend_from_slice(&payload.epoch.to_le_bytes());
    for field in [
        payload.key_image.as_bytes(),
        payload.amount_commitment.as_bytes(),
        payload.range_proof.as_bytes(),
        payload.destination.as_bytes(),
    ] {
        buf.extend_from_slice(&(field.len() as u32).to_le_bytes());
        buf.extend_from_slice(field);
    }
    buf
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Canonical transaction identifier.
    pub id: Option<Hash>,
    /// Canonical transaction type.
    pub tx_type: TransactionType,
    /// Protocol version for serialization compatibility.
    pub version: u32,
    /// Creation timestamp.
    pub timestamp: DateTime<Utc>,
    /// UTXO-style inputs.
    pub inputs: Vec<TransactionInput>,
    /// UTXO-style outputs.
    pub outputs: Vec<TransactionOutput>,
    /// Optional typed payload (JSON string or protocol payload).
    pub data: Option<String>,
    /// Optional lock time.
    pub lock_time: Option<u64>,
    /// Transaction fee (legacy-friendly direct field).
    pub fee: u64,
    /// Legacy sender field kept for compatibility.
    pub sender_address: Address,
    /// Legacy recipient field kept for compatibility.
    pub recipient_address: Option<Address>,
    /// Legacy amount field kept for compatibility.
    pub amount: u64,
    /// Legacy operation field kept for compatibility.
    pub operation: Operation,
    /// Legacy nonce field kept for compatibility.
    pub nonce: u64,

    // -----------------------------------------------------------------
    // Per-transaction parsed-payload memoization (#1378).
    //
    // The consensus reveal-verification path calls `parse_reveal_payload`
    // 14+ times for a single reveal transaction (extract_metrics,
    // verify_zk_weather_model, multidomain reward classification, quality
    // enforcement, compute-fee validation, state-transition application,
    // etc.). Without memoization each call re-parses `data` JSON into a
    // fresh `RevealPayload`. The same pattern applies to the commit-id
    // chain (commit + oracle_attestation), the zk-proof scrape path
    // (confidential / shielded_deposit / shielded_withdrawal — see
    // `count_privacy_proofs_in_transaction` in `launch_api/routes/chain.rs`),
    // and the MLSAG reward-claim pre-pass.
    //
    // `OnceLock` so first access parses, subsequent accesses memoize.
    // `#[serde(skip)]` keeps wire format unchanged: the cells are reset
    // to empty after deserialization and rewarm on first access.
    //
    // Correctness: the cache assumes `data` and `tx_type` are immutable
    // after the first parse call. All in-tree builders set both fields
    // before any `parse_*_payload` invocation; mutating them after a
    // parse would silently use stale cached output.
    // -----------------------------------------------------------------
    #[serde(skip)]
    parsed_commit: std::sync::OnceLock<Option<CommitPayload>>,
    #[serde(skip)]
    parsed_reveal: std::sync::OnceLock<Option<RevealPayload>>,
    #[serde(skip)]
    parsed_oracle_attestation: std::sync::OnceLock<Option<OracleAttestationPayload>>,
    #[serde(skip)]
    parsed_confidential_transfer: std::sync::OnceLock<Option<ConfidentialTransferPayload>>,
    #[serde(skip)]
    parsed_shielded_deposit: std::sync::OnceLock<Option<ShieldedDepositPayload>>,
    #[serde(skip)]
    parsed_shielded_withdrawal: std::sync::OnceLock<Option<ShieldedWithdrawalPayload>>,
    #[serde(skip)]
    parsed_reward_claim: std::sync::OnceLock<Option<RewardClaimPayload>>,
}

/// Push an optional string into a binary buffer: 0u8 for None, 1u8 + length-prefixed bytes for Some.
fn push_optional_str(buf: &mut Vec<u8>, value: Option<&str>) {
    match value {
        None => buf.push(0),
        Some(s) => {
            buf.push(1);
            let bytes = s.as_bytes();
            buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(bytes);
        },
    }
}

impl Transaction {
    /// Compute commit id as hash(data || nonce || station_id || timestamp || type).
    pub fn compute_commit_id(
        data: &serde_json::Value,
        nonce: &str,
        station_id: &str,
        timestamp: i64,
        contribution_type: &str,
    ) -> String {
        let data_str = serde_json::to_string(data).unwrap_or_default();
        let preimage =
            format!("{data_str}||{nonce}||{station_id}||{timestamp}||{contribution_type}");
        hash_bytes(preimage.as_bytes()).to_string()
    }

    /// Parse commit payload from tx data when tx type is commit.
    ///
    /// Memoized via `parsed_commit` (#1378): the JSON parse runs at most
    /// once per `Transaction`; subsequent calls clone the cached value.
    pub fn parse_commit_payload(&self) -> Option<CommitPayload> {
        self.parsed_commit
            .get_or_init(|| match self.tx_type {
                TransactionType::ContributionCommit | TransactionType::ForecastCommit => self
                    .data
                    .as_ref()
                    .and_then(|d| serde_json::from_str::<CommitPayload>(d).ok()),
                _ => None,
            })
            .clone()
    }

    /// Parse reveal payload from tx data when tx type is reveal.
    ///
    /// Memoized via `parsed_reveal` (#1378). The consensus reveal-verify
    /// path calls this 14+ times per reveal transaction; without the cache
    /// each call re-parses `data` JSON.
    pub fn parse_reveal_payload(&self) -> Option<RevealPayload> {
        self.parsed_reveal
            .get_or_init(|| match self.tx_type {
                TransactionType::ContributionReveal | TransactionType::ForecastReveal => self
                    .data
                    .as_ref()
                    .and_then(|d| serde_json::from_str::<RevealPayload>(d).ok()),
                _ => None,
            })
            .clone()
    }

    /// Parse oracle attestation payload from tx data when tx type is attestation.
    ///
    /// Memoized via `parsed_oracle_attestation` (#1378).
    pub fn parse_oracle_attestation_payload(&self) -> Option<OracleAttestationPayload> {
        self.parsed_oracle_attestation
            .get_or_init(|| {
                if self.tx_type != TransactionType::OracleAttestation {
                    return None;
                }
                self.data
                    .as_ref()
                    .and_then(|d| serde_json::from_str::<OracleAttestationPayload>(d).ok())
            })
            .clone()
    }

    pub fn parse_governance_proposal_payload(&self) -> Option<GovernanceProposalPayload> {
        if self.tx_type != TransactionType::GovernanceProposal {
            return None;
        }
        self.data
            .as_ref()
            .and_then(|d| serde_json::from_str::<GovernanceProposalPayload>(d).ok())
    }

    pub fn parse_governance_vote_payload(&self) -> Option<GovernanceVotePayload> {
        if self.tx_type != TransactionType::GovernanceVote {
            return None;
        }
        self.data
            .as_ref()
            .and_then(|d| serde_json::from_str::<GovernanceVotePayload>(d).ok())
    }

    /// Parse confidential transfer payload from tx data.
    ///
    /// Memoized via `parsed_confidential_transfer` (#1378). Called once per
    /// in-memory block from `count_privacy_proofs_in_transaction`; the per-block
    /// cache in `launch_api/routes/chain.rs` (#1377) and this per-tx cache
    /// together drop the Prometheus scrape cost from O(blocks*txs*parse) to
    /// O(blocks_added_since_last_scrape).
    pub fn parse_confidential_transfer_payload(&self) -> Option<ConfidentialTransferPayload> {
        self.parsed_confidential_transfer
            .get_or_init(|| {
                if self.tx_type != TransactionType::ConfidentialTransfer {
                    return None;
                }
                self.data
                    .as_ref()
                    .and_then(|d| serde_json::from_str::<ConfidentialTransferPayload>(d).ok())
            })
            .clone()
    }

    /// Parse shielded deposit payload from tx data.
    ///
    /// Memoized via `parsed_shielded_deposit` (#1378).
    pub fn parse_shielded_deposit_payload(&self) -> Option<ShieldedDepositPayload> {
        self.parsed_shielded_deposit
            .get_or_init(|| {
                if self.tx_type != TransactionType::ShieldedDeposit {
                    return None;
                }
                self.data
                    .as_ref()
                    .and_then(|d| serde_json::from_str::<ShieldedDepositPayload>(d).ok())
            })
            .clone()
    }

    /// Parse shielded withdrawal payload from tx data.
    ///
    /// Memoized via `parsed_shielded_withdrawal` (#1378).
    pub fn parse_shielded_withdrawal_payload(&self) -> Option<ShieldedWithdrawalPayload> {
        self.parsed_shielded_withdrawal
            .get_or_init(|| {
                if self.tx_type != TransactionType::ShieldedWithdrawal {
                    return None;
                }
                self.data
                    .as_ref()
                    .and_then(|d| serde_json::from_str::<ShieldedWithdrawalPayload>(d).ok())
            })
            .clone()
    }

    /// Parse transparent transfer payload from tx data.
    pub fn parse_transparent_transfer_payload(&self) -> Option<TransparentTransferPayload> {
        if self.tx_type != TransactionType::Transfer {
            return None;
        }
        self.data
            .as_ref()
            .and_then(|d| serde_json::from_str::<TransparentTransferPayload>(d).ok())
    }

    /// Parse the stake authorization payload from tx data, for `Stake` /
    /// `Unstake` transactions (#1763). Returns `None` for any other tx type or
    /// when the payload is absent/malformed.
    pub fn parse_stake_auth_payload(&self) -> Option<StakeAuthPayload> {
        if self.tx_type != TransactionType::Stake && self.tx_type != TransactionType::Unstake {
            return None;
        }
        self.data
            .as_ref()
            .and_then(|d| serde_json::from_str::<StakeAuthPayload>(d).ok())
    }

    /// Parse the [`GamificationClaimPayload`] carried in `data` for an
    /// authorized gamification-reward payout (mobile#215).
    ///
    /// Returns `Some` ONLY for a [`TransactionType::Reward`] transaction whose
    /// `data` deserializes to a payload with the fixed
    /// `reward_type == "gamification_claim"` discriminant and a non-empty
    /// `claim_id`. Ordinary role/mining `Reward` transactions return `None`
    /// and continue to map to a plain `ApplyReward` transition.
    ///
    /// This is the SINGLE parse used by every reconstruction path (block
    /// producer, P2P applier, and WAL replay) so all peers derive byte-identical
    /// state from the same serialized transaction — the determinism invariant
    /// this feature depends on.
    pub fn parse_gamification_claim(&self) -> Option<GamificationClaimPayload> {
        if self.tx_type != TransactionType::Reward {
            return None;
        }
        let payload: GamificationClaimPayload =
            serde_json::from_str(self.data.as_deref()?).ok()?;
        if payload.reward_type != GamificationClaimPayload::REWARD_TYPE {
            return None;
        }
        if payload.claim_id.trim().is_empty() {
            return None;
        }
        Some(payload)
    }

    /// Construct a gamification-reward payout transaction (mobile#215).
    ///
    /// The single input's `txid` is DETERMINISTICALLY derived from the
    /// `claim_id` (blake3), so the tx occupies a claim-specific mempool
    /// outpoint: distinct claims never collide, while a re-submission of the
    /// *same* pending claim collides on that outpoint and is rejected as a
    /// duplicate by mempool double-spend detection. This is the intra-mempool
    /// dedup; the authoritative cross-block dedup is the consensus set
    /// `State::gamification_claims_seen`.
    ///
    /// The `timestamp` is wall-clock (`Utc::now`), exactly like every other
    /// user-submitted transaction: the producer's submission-time window
    /// (`classify_block_transaction`) would drop a tx timestamped far in the
    /// past, so a pinned timestamp would keep the payout out of every block.
    /// Determinism does NOT depend on the timestamp — the P2P applier and WAL
    /// replay reconstruct the transition from the *received* tx bytes, and the
    /// dedup key is `claim_id`, not the tx hash.
    ///
    /// NOTE: no `coinbase_data` is set, so `is_coinbase()` stays `false` — the
    /// payout rides the block as an ordinary regular-lane transaction, never in
    /// the reserved coinbase position.
    pub fn new_gamification_reward(
        recipient: &str,
        amount: u64,
        claim_id: &str,
        operator_key: &PrivateKey,
    ) -> Self {
        let input = TransactionInput {
            txid: hash_bytes(format!("gamification_claim:{claim_id}").as_bytes()),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        };
        let output = TransactionOutput {
            amount,
            address: recipient.to_string(),
            script: format!("OP_DUP OP_HASH160 {recipient} OP_EQUALVERIFY OP_CHECKSIG"),
            nft_data: None,
        };
        let mut tx = Transaction::new(vec![input], vec![output]);
        tx.tx_type = TransactionType::Reward;
        tx.fee = 0;
        // Operator authorization is bound to the exact recipient/amount/claim
        // and folded into the tx bytes, so every syncing peer verifies it from
        // the transaction alone (core4#2081 review). A payout whose signature
        // does not verify against an allowlisted operator key is rejected by
        // `verify_block` and emits no transition in the extractor.
        let mut payload = GamificationClaimPayload::new(claim_id);
        payload.sign(recipient, amount, operator_key);
        tx.data = serde_json::to_string(&payload).ok();
        tx.set_id();
        tx
    }

    /// Verify the embedded operator authorization on a gamification-payout
    /// `Reward` transaction (core4#2081 review).
    ///
    /// Fail-closed: returns `false` unless this is a `Reward` tx carrying a
    /// parseable gamification claim whose `auth_signature` verifies, under an
    /// **allowlisted** `auth_pubkey`, over the canonical message binding the
    /// claim id and THIS tx's first output (recipient + amount). Binding the
    /// output means a peer cannot keep a harvested signature while swapping in
    /// its own address or a larger amount — the signature stops verifying.
    ///
    /// The allowlist is deployment config ([`gamification_operator_pubkeys`]),
    /// resolved identically by producer, mempool, and every syncing peer, so
    /// they reach the same verdict from the transaction alone.
    pub fn verify_gamification_authorization(&self) -> bool {
        if self.tx_type != TransactionType::Reward {
            return false;
        }
        let Some(claim) = self.parse_gamification_claim() else {
            return false;
        };
        let (Some(pk_hex), Some(sig_hex)) =
            (claim.auth_pubkey.as_deref(), claim.auth_signature.as_deref())
        else {
            return false;
        };
        // Authority check first: an otherwise-valid signature from a key nobody
        // authorized is not an authorization.
        if !is_authorized_gamification_operator(pk_hex) {
            return false;
        }
        let Some(output) = self.outputs.first() else {
            return false;
        };
        let (Ok(public_key), Ok(signature)) =
            (PublicKey::from_hex(pk_hex), Signature::from_hex(sig_hex))
        else {
            return false;
        };
        let message = gamification_authorization_message(
            &claim.claim_id,
            &output.address,
            output.amount,
        );
        public_key.verify(&message, &signature)
    }

    /// Construct a governance-authorized oracle-registration transaction
    /// (#2055 Option A). Mirrors [`Transaction::new_gamification_reward`]: the
    /// authorization is bound to the exact registration fields and folded into
    /// the tx bytes, so every syncing peer verifies it from the tx alone.
    ///
    /// The input `txid` is derived from the registration content so distinct
    /// registrations occupy distinct mempool outpoints while an identical
    /// re-submission collides and is deduped.
    #[allow(clippy::too_many_arguments)]
    pub fn new_oracle_registration(
        oracle_set: Vec<String>,
        oracle_quorum: usize,
        identity: &str,
        public_key: &str,
        epoch: u64,
        activated_at: i64,
        governance_key: &PrivateKey,
    ) -> Self {
        Self::new_oracle_registration_with_domain(
            &signing_chain_id(),
            &signing_network_id(),
            oracle_set,
            oracle_quorum,
            identity,
            public_key,
            epoch,
            activated_at,
            governance_key,
        )
    }

    /// [`Self::new_oracle_registration`] with the chain identity passed
    /// **explicitly** (#2530), so construction is reproducible off-node.
    ///
    /// The environment-resolving form above is correct for a node signing on
    /// its own chain. It is wrong for a ceremony host, which must produce
    /// byte-identical transactions on any machine and must be auditable against
    /// the domain the nodes will verify under. Note the txid derives from the
    /// authorization message, so it is chain-bound too — a domain mismatch is
    /// not merely a bad signature, it is a different transaction.
    #[allow(clippy::too_many_arguments)]
    pub fn new_oracle_registration_with_domain(
        chain_id: &str,
        network_id: &str,
        oracle_set: Vec<String>,
        oracle_quorum: usize,
        identity: &str,
        public_key: &str,
        epoch: u64,
        activated_at: i64,
        governance_key: &PrivateKey,
    ) -> Self {
        let mut payload = OracleRegistrationPayload::new(
            oracle_set,
            oracle_quorum,
            identity,
            public_key,
            epoch,
            activated_at,
        );
        payload.sign_with_domain(chain_id, network_id, governance_key);
        let fingerprint = hash_bytes(&payload.authorization_message());
        let input = TransactionInput {
            txid: hash_bytes(format!("oracle_registration:{}", fingerprint.to_hex()).as_bytes()),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        };
        // A single inert, zero-value output whose address is the operator
        // identity. This carries NO funds — the registration mutates
        // `OracleRegistry` via `ApplyOracleRegistration`, which reads the signed
        // payload, not this output. It exists only so the tx satisfies
        // `validate_structure` (every non-coinbase tx needs a non-empty output)
        // and is admitted to the mempool like any other tx, so it gossips and
        // reaches every peer's block-extraction path (#2055 fork-safety).
        let output = TransactionOutput {
            amount: 0,
            address: identity.to_string(),
            script: String::new(),
            nft_data: None,
        };
        let mut tx = Transaction::new(vec![input], vec![output]);
        tx.tx_type = TransactionType::OracleRegistration;
        tx.fee = 0;
        tx.data = serde_json::to_string(&payload).ok();
        tx.set_id();
        tx
    }

    /// Build a signed `StationRegistration` transaction with the chain identity
    /// passed **explicitly** (#2537), so construction is reproducible off-node.
    ///
    /// The station-registration analogue of
    /// [`Self::new_oracle_registration_with_domain`], and deliberately the same
    /// shape: the txid derives from the governance authorization message, so it
    /// is chain-bound — a domain mismatch is not merely a bad signature, it is a
    /// different transaction.
    ///
    /// `payload` must already carry its owner possession proof
    /// ([`StationRegistrationPayload::sign_ownership_with_domain`]); the owner
    /// key is folded into the governance preimage, so signing order matters and
    /// governance must sign last. This function performs the governance signing.
    pub fn new_station_registration_with_domain(
        chain_id: &str,
        network_id: &str,
        mut payload: StationRegistrationPayload,
        governance_key: &PrivateKey,
    ) -> Self {
        payload.sign_with_domain(chain_id, network_id, governance_key);
        let fingerprint = hash_bytes(&payload.authorization_message());
        let input = TransactionInput {
            txid: hash_bytes(format!("station_registration:{}", fingerprint.to_hex()).as_bytes()),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        };
        // A single inert, zero-value output addressed to the station owner.
        // Carries NO funds — the registration mutates `station_registry` via
        // `ApplyStationRegistration`, which reads the signed payload, not this
        // output. It exists only so the tx satisfies `validate_structure` (every
        // non-coinbase tx needs a non-empty output) and is admitted to the
        // mempool like any other tx, so it gossips and reaches every peer's
        // block-extraction path.
        let output = TransactionOutput {
            amount: 0,
            address: payload.owner_address.clone(),
            script: String::new(),
            nft_data: None,
        };
        let mut tx = Transaction::new(vec![input], vec![output]);
        tx.tx_type = TransactionType::StationRegistration;
        tx.fee = 0;
        tx.data = serde_json::to_string(&payload).ok();
        tx.set_id();
        tx
    }

    /// Build a `FeatureScheduleExtension` transaction carrying `signed`
    /// (#3277 S4). Same envelope as the registration transactions: the
    /// authorization lives inside the payload, and one inert zero-value
    /// output exists only to satisfy `validate_structure` so the tx is
    /// admitted and gossiped. The txid derives from the extension preimage
    /// and its signatures, so it is chain-bound and changes with any field.
    pub fn new_feature_schedule_extension(
        signed: &crate::governance::schedule_extension::SignedFeatureScheduleExtension,
    ) -> Self {
        let mut fingerprint_material = signed.extension.preimage();
        for signature in &signed.signatures {
            fingerprint_material.extend_from_slice(signature.signer_pubkey.as_bytes());
            fingerprint_material.extend_from_slice(signature.signature.as_bytes());
        }
        let fingerprint = hash_bytes(&fingerprint_material);
        let input = TransactionInput {
            txid: hash_bytes(
                format!("feature_schedule_extension:{}", fingerprint.to_hex()).as_bytes(),
            ),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        };
        // Carries NO funds: the extension mutates `feature_schedule_extensions`
        // through `ApplyFeatureScheduleExtension`, which reads the signed
        // payload, never this output.
        let output = TransactionOutput {
            amount: 0,
            address: format!("feature_schedule_extension:{}", signed.extension.sequence),
            script: String::new(),
            nft_data: None,
        };
        let mut tx = Transaction::new(vec![input], vec![output]);
        tx.tx_type = TransactionType::FeatureScheduleExtension;
        tx.fee = 0;
        tx.data = serde_json::to_string(signed).ok();
        tx.set_id();
        tx
    }

    /// Parse the signed extension carried in `data` (#3277 S4). `Some` only
    /// for a [`TransactionType::FeatureScheduleExtension`] tx whose `data`
    /// deserializes. This is the SINGLE parse used by producer, P2P applier,
    /// WAL replay, `verify_block` and mempool admission. It checks no rule:
    /// every validity rule is re-run by the apply arm against the parent
    /// state, so an extension that parses always reaches it.
    pub fn parse_feature_schedule_extension(
        &self,
    ) -> Option<crate::governance::schedule_extension::SignedFeatureScheduleExtension> {
        if self.tx_type != TransactionType::FeatureScheduleExtension {
            return None;
        }
        serde_json::from_str(self.data.as_deref()?).ok()
    }

    /// Parse the [`OracleRegistrationPayload`] carried in `data`. `Some` only for
    /// a [`TransactionType::OracleRegistration`] tx whose `data` deserializes to
    /// a payload with the fixed `tx_kind` discriminant. This is the SINGLE parse
    /// used by producer, P2P applier and WAL replay so all peers derive
    /// byte-identical state.
    pub fn parse_oracle_registration(&self) -> Option<OracleRegistrationPayload> {
        if self.tx_type != TransactionType::OracleRegistration {
            return None;
        }
        let payload: OracleRegistrationPayload = serde_json::from_str(self.data.as_deref()?).ok()?;
        if payload.tx_kind != OracleRegistrationPayload::TX_KIND {
            return None;
        }
        Some(payload)
    }

    /// Verify the governance authorization on an oracle-registration tx (#2055).
    ///
    /// Fail-closed: `false` unless this is an `OracleRegistration` tx whose
    /// `auth_signature` verifies, under an **allowlisted** `auth_pubkey`, over
    /// the canonical message binding every registration field. A peer cannot
    /// forge, redirect (swap identity/public_key), widen the set, or lower the
    /// quorum while keeping a harvested signature — any change breaks it. The
    /// allowlist is deployment config ([`oracle_governance_pubkeys`]), resolved
    /// identically by producer, mempool, and every syncing peer.
    pub fn verify_oracle_registration_authorization(&self) -> bool {
        let Some(payload) = self.parse_oracle_registration() else {
            return false;
        };
        let Some(pk_hex) = payload.auth_pubkey.as_deref() else {
            return false;
        };
        if payload.auth_signature.is_none() {
            return false;
        }
        if !is_authorized_oracle_governance(pk_hex) {
            return false;
        }
        // `has_valid_authorization_signature` is also the offline ceremony
        // verifier's cryptographic seam, so block admission and the ceremony
        // cannot drift onto different registration preimages.
        payload.has_valid_authorization_signature()
    }

    /// Parse the [`StationRegistrationPayload`] carried in `data` (#2537).
    ///
    /// `Some` only for a [`TransactionType::StationRegistration`] tx whose
    /// `data` deserializes to a payload with the fixed `tx_kind` discriminant
    /// AND passes [`StationRegistrationPayload::validate`]. This is the SINGLE
    /// parse used by producer, P2P applier and WAL replay, so all peers derive
    /// byte-identical state.
    pub fn parse_station_registration(&self) -> Option<StationRegistrationPayload> {
        if self.tx_type != TransactionType::StationRegistration {
            return None;
        }
        let payload: StationRegistrationPayload =
            serde_json::from_str(self.data.as_deref()?).ok()?;
        // `validate` accepts exactly the v1 and v2 `tx_kind`s (with the matching
        // `expected_nonce` shape) and nothing else.
        payload.validate().ok()?;
        Some(payload)
    }

    /// Verify BOTH authorizations on a station-registration tx (#2537).
    ///
    /// Fail-closed: `false` unless every one of these holds.
    ///
    /// 1. The tx parses and structurally validates.
    /// 2. `auth_signature` verifies under an **allowlisted** `auth_pubkey`
    ///    (`oracle_governance_pubkeys`) over the canonical message binding every
    ///    field of the station record — including `provenance_class` and
    ///    `owner_pubkey`. A peer cannot forge a registration, flip a station to
    ///    `Real`, move its coordinates, or re-pair a harvested governance
    ///    signature with a different owner key: any change breaks the signature.
    /// 3. `owner_signature` verifies under `owner_pubkey` over the ownership
    ///    message, proving the registrant holds the station's key.
    ///
    /// Possession alone never authorizes, and governance alone never authorizes:
    /// #2537 requires that `Real` — the reward-authorising classification — is
    /// not self-asserted by the station, and an owner cannot be enrolled
    /// without proving they hold a key.
    pub fn verify_station_registration_authorization(&self) -> bool {
        self.verify_station_registration_authorization_with_domain(
            &signing_chain_id(),
            &signing_network_id(),
        )
    }

    /// Pure-domain form of
    /// [`verify_station_registration_authorization`](Self::verify_station_registration_authorization)
    /// (#2744).
    ///
    /// The signing half of this contract already took the chain identity
    /// explicitly (`sign_ownership_with_domain`,
    /// `new_station_registration_with_domain`) while verification resolved it
    /// from the environment — two sources of truth for one value. An offline
    /// builder invoked with correct `--chain-id`/`--network-id` flags but no
    /// matching env therefore failed its own self-check, and the error blamed
    /// the governance key. This form lets a caller verify against the same
    /// identity it signed with; the env-reading wrapper above remains the
    /// node's verification path, unchanged.
    pub fn verify_station_registration_authorization_with_domain(
        &self,
        chain_id: &str,
        network_id: &str,
    ) -> bool {
        let Some(payload) = self.parse_station_registration() else {
            return false;
        };
        let (Some(auth_pk_hex), Some(auth_sig_hex)) = (
            payload.auth_pubkey.as_deref(),
            payload.auth_signature.as_deref(),
        ) else {
            return false;
        };
        let (Some(owner_pk_hex), Some(owner_sig_hex)) = (
            payload.owner_pubkey.as_deref(),
            payload.owner_signature.as_deref(),
        ) else {
            return false;
        };
        if !is_authorized_oracle_governance(auth_pk_hex) {
            return false;
        }
        let (Ok(auth_pk), Ok(auth_sig)) = (
            PublicKey::from_hex(auth_pk_hex),
            Signature::from_hex(auth_sig_hex),
        ) else {
            return false;
        };
        let (Ok(owner_pk), Ok(owner_sig)) = (
            PublicKey::from_hex(owner_pk_hex),
            Signature::from_hex(owner_sig_hex),
        ) else {
            return false;
        };
        // Owner possession first — cheap, and independent of governance.
        let ownership_message = station_ownership_message_with_domain(
            chain_id,
            network_id,
            &payload.station_id,
            &payload.owner_address,
        );
        if !owner_pk.verify(&ownership_message, &owner_sig) {
            return false;
        }
        // Recompute the governance message from the payload with the auth
        // fields cleared, matching how `sign_with_domain` produced it. Note
        // `owner_pubkey` is deliberately NOT cleared: it is part of what
        // governance authorized.
        let mut unsigned = payload.clone();
        unsigned.auth_pubkey = None;
        unsigned.auth_signature = None;
        let authorization_message =
            station_registration_authorization_message_with_domain(chain_id, network_id, &unsigned);
        auth_pk.verify(&authorization_message, &auth_sig)
    }

    /// [`Self::verify_station_registration_authorization`] plus the #2742
    /// ownership **binding** rule, active from
    /// [`STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT`]: the possession proof
    /// only proves ownership of `owner_address` if `owner_pubkey` actually
    /// derives to it. Before this rule, `owner_address` was a free-form string
    /// the CLI accepted as a flag — governance-attested, but bound to nothing;
    /// with #2743 routing payment by the field, an unbound value would move
    /// real money on an unverified claim.
    ///
    /// Below the activation height the legacy checks apply unchanged, so the
    /// registrations already on chain (block 674) replay byte-identically.
    /// This is the predicate ALL consensus paths (producer, P2P applier, WAL
    /// replay) and admission must gate on; the un-suffixed form exists only as
    /// the shared legacy core.
    ///
    /// [`STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT`]: crate::consensus::proof_of_publishing::STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT
    pub fn verify_station_registration_authorization_at_height(&self, block_height: u64) -> bool {
        if !self.verify_station_registration_authorization() {
            return false;
        }
        if block_height
            < crate::consensus::proof_of_publishing::STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT
        {
            return true;
        }
        self.parse_station_registration()
            .is_some_and(|payload| payload.owner_address_is_bound_to_owner_pubkey())
    }

    /// The ONE entry point every extractor uses for a station-registration tx
    /// (the PoP producer arm and the P2P/WAL `transition_extractor`, so
    /// producer == applier == WAL replay): parse, then fail closed on the
    /// format-vs-activation rule, then authorization, then pick the variant.
    ///
    /// `ConsensusFeature::StationRegistrationNonce` decides which format is
    /// admissible at `block_height`: BELOW activation only v1 (a v2 payload is
    /// inert); FROM activation only v2 (a v1 payload is refused entirely, so no
    /// already-mined v1 registration can be applied again). `None` means the tx
    /// yields no transition. Callers still use `parse_station_registration()`
    /// to tell a registration from the owner-ledger kinds.
    pub fn station_registration_step(
        &self,
        block_height: u64,
    ) -> Option<crate::blockchain::state::StateTransition> {
        let payload = self.parse_station_registration()?;
        let nonce_active = crate::crypto::chain_binding::feature_active(
            crate::crypto::chain_binding::ConsensusFeature::StationRegistrationNonce,
            block_height,
        );
        if payload.is_v2() != nonce_active {
            return None;
        }
        if !self.verify_station_registration_authorization_at_height(block_height) {
            return None;
        }
        Some(if payload.is_v2() {
            crate::blockchain::state::StateTransition::ApplyStationRegistrationV2 {
                station: Self::station_from_registration(&payload),
                expected_nonce: payload.expected_nonce.unwrap_or(0),
                block_height,
            }
        } else {
            Self::station_registration_transition(&payload, block_height)
        })
    }

    /// The ONE constructor every extractor uses for a verified station
    /// registration (#2537, #3345): the PoP producer arm, the P2P/WAL
    /// `transition_extractor`, and therefore WAL replay. Below
    /// `ConsensusFeature::StationOwnerConsent`'s activation it is the legacy
    /// `ApplyStationRegistration` (byte-identical blocks, WAL and roots); from
    /// activation it is `ApplyStationRegistrationWithOwnerConsent`, which
    /// refuses an owner change. One function, so producer and applier cannot
    /// pick different variants at the same height.
    pub fn station_registration_transition(
        payload: &StationRegistrationPayload,
        block_height: u64,
    ) -> crate::blockchain::state::StateTransition {
        let station = Self::station_from_registration(payload);
        if crate::crypto::chain_binding::feature_active(
            crate::crypto::chain_binding::ConsensusFeature::StationOwnerConsent,
            block_height,
        ) {
            crate::blockchain::state::StateTransition::ApplyStationRegistrationWithOwnerConsent {
                station,
                block_height,
            }
        } else {
            crate::blockchain::state::StateTransition::ApplyStationRegistration { station }
        }
    }

    /// Build the canonical [`WeatherStation`] a verified registration produces.
    ///
    /// Shared by producer and applier so both derive a byte-identical record
    /// (the #2537 analogue of the single-parse rule). `reputation_score` is
    /// always seeded at 0 — see [`StationRegistrationPayload`].
    pub fn station_from_registration(
        payload: &StationRegistrationPayload,
    ) -> crate::station_registry::WeatherStation {
        crate::station_registry::WeatherStation {
            station_id: payload.station_id.clone(),
            owner_address: payload.owner_address.clone(),
            location: crate::station_registry::StationLocation {
                latitude: payload.latitude,
                longitude: payload.longitude,
                elevation_meters: payload.elevation_meters,
            },
            hardware_model: payload.hardware_model.clone(),
            calibration_date: payload.calibration_date.clone(),
            is_active: payload.is_active,
            reputation_score: 0,
            data_quality_tier: payload.data_quality_tier.clone(),
            provenance_class: payload.provenance_class,
            ring_public_key: payload.ring_public_key.clone(),
        }
    }

    /// Whether this is a `Stake`/`Unstake` transaction carrying a parseable
    /// stake-authorization payload, regardless of whether the authorization is
    /// valid (#1763).
    ///
    /// Used by verification paths to decide that the staking mutation's
    /// authorization is the embedded `account`-bound signature, so an
    /// unauthorized stake/unstake is rejected rather than silently applied.
    pub fn has_stake_auth_payload(&self) -> bool {
        (self.tx_type == TransactionType::Stake || self.tx_type == TransactionType::Unstake)
            && self.parse_stake_auth_payload().is_some()
    }

    /// Verify the sender authorization on a `ContributionCommit`/`ForecastCommit`
    /// transaction (#1848). Returns `false` for non-commit types or any commit whose
    /// payload does not contain a valid `auth_pubkey`/`auth_signature` pair.
    ///
    /// Fail-closed: a commit without a valid authorization emits no state transition
    /// (transition extractor) and is rejected at mempool admission.
    pub fn verify_commit_authorization(&self) -> bool {
        match self.tx_type {
            TransactionType::ContributionCommit | TransactionType::ForecastCommit => {},
            _ => return false,
        }
        match self.parse_commit_payload() {
            Some(payload) => payload.verify_authorization(),
            None => false,
        }
    }

    /// #3290: whether this `ForecastCommit` names the forecaster its
    /// authorization key controls.
    ///
    /// The forecaster is the 20-byte address derived from `auth_pubkey`
    /// (`Address::from_public_key`). The transaction's `sender_address` and
    /// EVERY output address must parse to exactly those bytes: a display or
    /// case variant parses to the same bytes, a label that is not an address
    /// (or names someone else) does not. Signature validity is NOT checked
    /// here; [`Self::verify_commit_authorization_at_height`] composes both.
    /// Returns `false` for every other transaction type.
    pub fn forecast_commit_key_bound(&self) -> bool {
        if self.tx_type != TransactionType::ForecastCommit {
            return false;
        }
        let Some(payload) = self.parse_commit_payload() else {
            return false;
        };
        let Some(Ok(public_key)) = payload.auth_pubkey.as_deref().map(PublicKey::from_hex) else {
            return false;
        };
        let forecaster = Address::from_public_key(&public_key);
        self.sender_address == forecaster
            && !self.outputs.is_empty()
            && self
                .outputs
                .iter()
                .all(|output| output.address.parse::<Address>().is_ok_and(|a| a == forecaster))
    }

    /// Height-aware commit authorization: [`Self::verify_commit_authorization`],
    /// plus, for a `ForecastCommit` once
    /// `ConsensusFeature::ForecasterKeyBinding` is active at `height`, the
    /// #3290 key binding ([`Self::forecast_commit_key_bound`]).
    ///
    /// The ONE check mempool admission, the shared producer/sync block
    /// classifier and transition extraction ask, so the three cannot disagree.
    /// With the feature inactive this is `verify_commit_authorization` exactly.
    pub fn verify_commit_authorization_at_height(&self, height: u64) -> bool {
        self.verify_commit_authorization()
            && (self.tx_type != TransactionType::ForecastCommit
                || !crate::crypto::chain_binding::feature_active(
                    crate::crypto::chain_binding::ConsensusFeature::ForecasterKeyBinding,
                    height,
                )
                || self.forecast_commit_key_bound())
    }

    /// Verify the embedded `account`-bound authorization on a `Stake`/`Unstake`
    /// transaction (#1763). Returns `false` for non-stake types, transactions
    /// without a stake-authorization payload, or any payload whose authorization
    /// does not verify against `account` for this tx's action at its `nonce`.
    ///
    /// The action (`is_unstake`) is derived from `tx_type` and bound into the
    /// signed message, so a `Stake` authorization cannot be reused for `Unstake`.
    pub fn verify_stake_authorization(&self) -> bool {
        // Mempool/pre-block validation has no State-backed declaration view.
        self.verify_stake_authorization_with_view(&EmptyAuthorizationView, 0, Timestamp::new(0))
    }

    pub fn verify_stake_authorization_with_view<V: crate::accounts::authorization::AuthorizationView>(
        &self, view: &V, height: u64, timestamp: Timestamp,
    ) -> bool {
        let is_unstake = match self.tx_type {
            TransactionType::Stake => false,
            TransactionType::Unstake => true,
            _ => return false,
        };
        match self.parse_stake_auth_payload() {
            Some(payload) => payload.verify_authorization_with_view(
                view, is_unstake, self.nonce, height, timestamp,
            ),
            None => false,
        }
    }

    /// Parse layer2 batch submit payload from tx data.
    pub fn parse_layer2_batch_submit_payload(&self) -> Option<Layer2BatchSubmitPayload> {
        if self.tx_type != TransactionType::Layer2BatchSubmit {
            return None;
        }
        self.data
            .as_ref()
            .and_then(|d| serde_json::from_str::<Layer2BatchSubmitPayload>(d).ok())
    }

    /// Parse anonymous reward claim payload from tx data.
    ///
    /// Memoized via `parsed_reward_claim` (#1378). The MLSAG pre-pass in
    /// `verify_block_transactions_fused` calls this once per claim; quality
    /// enforcement and reward processing call it again on the same tx.
    pub fn parse_reward_claim_payload(&self) -> Option<RewardClaimPayload> {
        self.parsed_reward_claim
            .get_or_init(|| {
                if self.tx_type != TransactionType::RewardClaim {
                    return None;
                }
                self.data
                    .as_ref()
                    .and_then(|d| serde_json::from_str::<RewardClaimPayload>(d).ok())
            })
            .clone()
    }

    /// Parse slashing report payload from tx data when tx type is `SlashingReport`.
    ///
    /// Added for #1393. The payload is produced by the evidence-submitter task
    /// in `native-chain/src/bin/launch_api/main.rs`, serialized as JSON into
    /// `Transaction::data`, and consumed by the `SlashingReport` dispatch arm
    /// in `blockchain::state::State::apply_protocol_transaction`.
    pub fn parse_slashing_report_payload(
        &self,
    ) -> Option<crate::security::slashing::SlashingReportPayload> {
        if self.tx_type != TransactionType::SlashingReport {
            return None;
        }
        self.data.as_ref().and_then(|d| {
            serde_json::from_str::<crate::security::slashing::SlashingReportPayload>(d).ok()
        })
    }

    /// Extract commit id from typed payloads.
    pub fn commit_id(&self) -> Option<String> {
        if let Some(payload) = self.parse_commit_payload() {
            return Some(payload.commit_id);
        }
        if let Some(payload) = self.parse_reveal_payload() {
            return Some(payload.commit_id);
        }
        if let Some(payload) = self.parse_oracle_attestation_payload() {
            return Some(payload.commit_id);
        }
        None
    }

    /// Returns true if reveal payload recomputes to the same commit id.
    pub fn has_valid_reveal_commit(&self) -> bool {
        let Some(reveal) = self.parse_reveal_payload() else {
            return false;
        };
        let computed = Self::compute_commit_id(
            &reveal.data,
            &reveal.nonce,
            &reveal.station_id,
            reveal.timestamp,
            &reveal.contribution_type,
        );
        computed == reveal.commit_id
    }

    /// Create a new UTXO-style transaction.
    pub fn new(inputs: Vec<TransactionInput>, outputs: Vec<TransactionOutput>) -> Self {
        // #2025: sum with saturating_add rather than `.sum()`, which panics on
        // u64 overflow under `overflow-checks = true` (the release profile) and
        // would crash any code path that builds a Transaction from crafted
        // outputs. The `amount` field is legacy/informational — it is excluded
        // from `calculate_hash`/`signing_hash`, so this cannot change any tx id,
        // signature, or state root for non-overflowing inputs (the sum is
        // identical); it only avoids the panic for an absurd/crafted output set.
        let amount = outputs
            .iter()
            .map(|o| o.amount)
            .fold(0u64, u64::saturating_add);
        let mut tx = Self {
            id: None,
            tx_type: TransactionType::Regular,
            version: 1,
            timestamp: Utc::now(),
            inputs,
            outputs,
            data: None,
            lock_time: None,
            fee: 0,
            sender_address: Address([0; 20]),
            recipient_address: None,
            amount,
            operation: Operation::Transfer { amount },
            nonce: 0,
            parsed_commit: std::sync::OnceLock::new(),
            parsed_reveal: std::sync::OnceLock::new(),
            parsed_oracle_attestation: std::sync::OnceLock::new(),
            parsed_confidential_transfer: std::sync::OnceLock::new(),
            parsed_shielded_deposit: std::sync::OnceLock::new(),
            parsed_shielded_withdrawal: std::sync::OnceLock::new(),
            parsed_reward_claim: std::sync::OnceLock::new(),
        };
        tx.set_id();
        tx
    }

    /// Create a coinbase transaction paying the miner.
    pub fn new_coinbase(miner_address: &str, reward: u64, block_height: u64) -> Self {
        let coinbase_input = TransactionInput {
            txid: Hash::default(),
            vout: u32::MAX,
            signature: None,
            public_key: None,
            coinbase_data: Some(format!("coinbase:{block_height}")),
        };

        let coinbase_output = TransactionOutput {
            amount: reward,
            address: miner_address.to_string(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        };

        let mut tx = Self::new(vec![coinbase_input], vec![coinbase_output]);
        tx.tx_type = TransactionType::Coinbase;
        tx.amount = reward;
        tx.operation = Operation::Transfer { amount: reward };
        tx.set_id();
        tx
    }

    /// Returns the effective privacy level for this transaction.
    ///
    /// `ConfidentialTransfer` transactions parse their payload; shielded
    /// operations are inherently `High`; everything else is `Standard`.
    pub fn effective_privacy_level(&self) -> PrivacyLevel {
        match self.tx_type {
            TransactionType::ConfidentialTransfer => self
                .parse_confidential_transfer_payload()
                .map(|p| p.privacy_level)
                .unwrap_or(PrivacyLevel::Standard),
            TransactionType::ShieldedDeposit | TransactionType::ShieldedWithdrawal => {
                PrivacyLevel::High
            },
            _ => PrivacyLevel::Standard,
        }
    }

    /// Returns true when transaction is coinbase-like.
    pub fn is_coinbase(&self) -> bool {
        self.tx_type == TransactionType::Coinbase
            || self
                .inputs
                .first()
                .and_then(|i| i.coinbase_data.as_ref())
                .is_some()
    }

    /// Compute deterministic hash from canonical transaction fields.
    ///
    /// Uses field-by-field binary serialization (little-endian integers,
    /// length-prefixed strings) so the hash is independent of serde_json
    /// version, chrono formatting, or Hash Serde representation.
    pub fn calculate_hash(&self) -> Hash {
        let mut buf = Vec::with_capacity(256);
        // Version tag for future upgradability.
        buf.extend_from_slice(b"wrfcoin_tx_hash_v1");

        // tx_type: use Display string for stability across enum reordering.
        let tx_type_str = self.tx_type.to_string();
        buf.extend_from_slice(&(tx_type_str.len() as u32).to_le_bytes());
        buf.extend_from_slice(tx_type_str.as_bytes());

        // version
        buf.extend_from_slice(&self.version.to_le_bytes());

        // timestamp: Unix milliseconds (matches Block::calculate_hash).
        buf.extend_from_slice(&self.timestamp.timestamp_millis().to_le_bytes());

        // inputs
        buf.extend_from_slice(&(self.inputs.len() as u64).to_le_bytes());
        for input in &self.inputs {
            buf.extend_from_slice(input.txid.as_bytes());
            buf.extend_from_slice(&input.vout.to_le_bytes());
            push_optional_str(&mut buf, input.signature.as_deref());
            push_optional_str(&mut buf, input.public_key.as_deref());
            push_optional_str(&mut buf, input.coinbase_data.as_deref());
        }

        // outputs
        buf.extend_from_slice(&(self.outputs.len() as u64).to_le_bytes());
        for output in &self.outputs {
            buf.extend_from_slice(&output.amount.to_le_bytes());
            let addr = output.address.as_bytes();
            buf.extend_from_slice(&(addr.len() as u32).to_le_bytes());
            buf.extend_from_slice(addr);
            let script = output.script.as_bytes();
            buf.extend_from_slice(&(script.len() as u32).to_le_bytes());
            buf.extend_from_slice(script);
            // nft_data: canonical JSON bytes for the Value (deterministic).
            match &output.nft_data {
                None => buf.push(0),
                Some(val) => {
                    buf.push(1);
                    let json_bytes = serde_json::to_vec(val).unwrap_or_default();
                    buf.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
                    buf.extend_from_slice(&json_bytes);
                },
            }
        }

        // data
        push_optional_str(&mut buf, self.data.as_deref());

        // lock_time
        match self.lock_time {
            None => buf.push(0),
            Some(lt) => {
                buf.push(1);
                buf.extend_from_slice(&lt.to_le_bytes());
            },
        }

        // fee
        buf.extend_from_slice(&self.fee.to_le_bytes());

        // nonce
        buf.extend_from_slice(&self.nonce.to_le_bytes());

        hash_bytes(&buf)
    }

    /// Canonical signing message (#1279).
    ///
    /// Same shape as `calculate_hash`, but EXCLUDES `input.signature` fields so the
    /// hash is deterministic before signing (chicken-and-egg safe: `calculate_hash`
    /// folds `input.signature` into the hash, so a signer cannot sign that hash).
    /// Input `public_key` is retained so a forged public key cannot sign the same
    /// payload and impersonate a different sender. Domain-separated by a distinct
    /// tag (`wrfcoin_tx_sighash_v2`) to prevent accidental cross-use with `calculate_hash`.
    ///
    /// #2034: the preimage now folds the runtime chain identity
    /// (`chain_id`, `network_id`) right after the tag, so a value transfer signed
    /// on one network (e.g. testnet `777`) cannot be replayed on another (e.g.
    /// mainnet). The identity is resolved from deployment config; use
    /// [`signing_hash_with_domain`](Self::signing_hash_with_domain) to pass it
    /// explicitly (golden-vector / cross-signer parity).
    pub fn signing_hash(&self) -> Hash {
        self.signing_hash_with_domain(&signing_chain_id(), &signing_network_id())
    }

    /// Pure form of [`signing_hash`](Self::signing_hash) with the chain identity
    /// passed explicitly (#2034).
    pub fn signing_hash_with_domain(&self, chain_id: &str, network_id: &str) -> Hash {
        let mut buf = Vec::with_capacity(256);
        buf.extend_from_slice(b"wrfcoin_tx_sighash_v2");
        push_chain_domain(&mut buf, chain_id, network_id);

        let tx_type_str = self.tx_type.to_string();
        buf.extend_from_slice(&(tx_type_str.len() as u32).to_le_bytes());
        buf.extend_from_slice(tx_type_str.as_bytes());

        buf.extend_from_slice(&self.version.to_le_bytes());
        buf.extend_from_slice(&self.timestamp.timestamp_millis().to_le_bytes());

        buf.extend_from_slice(&(self.inputs.len() as u64).to_le_bytes());
        for input in &self.inputs {
            buf.extend_from_slice(input.txid.as_bytes());
            buf.extend_from_slice(&input.vout.to_le_bytes());
            // Signature DELIBERATELY excluded — this is the message to be signed.
            push_optional_str(&mut buf, input.public_key.as_deref());
            push_optional_str(&mut buf, input.coinbase_data.as_deref());
        }

        buf.extend_from_slice(&(self.outputs.len() as u64).to_le_bytes());
        for output in &self.outputs {
            buf.extend_from_slice(&output.amount.to_le_bytes());
            let addr = output.address.as_bytes();
            buf.extend_from_slice(&(addr.len() as u32).to_le_bytes());
            buf.extend_from_slice(addr);
            let script = output.script.as_bytes();
            buf.extend_from_slice(&(script.len() as u32).to_le_bytes());
            buf.extend_from_slice(script);
            match &output.nft_data {
                None => buf.push(0),
                Some(val) => {
                    buf.push(1);
                    let json_bytes = serde_json::to_vec(val).unwrap_or_default();
                    buf.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
                    buf.extend_from_slice(&json_bytes);
                },
            }
        }

        push_optional_str(&mut buf, self.data.as_deref());

        match self.lock_time {
            None => buf.push(0),
            Some(lt) => {
                buf.push(1);
                buf.extend_from_slice(&lt.to_le_bytes());
            },
        }

        buf.extend_from_slice(&self.fee.to_le_bytes());
        buf.extend_from_slice(&self.nonce.to_le_bytes());

        hash_bytes(&buf)
    }

    /// Verify ed25519 input signatures on this transaction (#1279).
    ///
    /// For each transaction input, requires that `signature` and `public_key` are
    /// both present and hex-decodable, and that the signature verifies against
    /// `signing_hash()` using the provided public key. Coinbase transactions have
    /// no signer and are always accepted — callers must filter coinbases at the
    /// call site if they want to require them explicitly.
    ///
    /// Returns `Ok(())` on success. Returns a `String` describing the first
    /// failure on error (plain `Result<(), String>` keeps this method free of
    /// cross-module error coupling — callers map the string into their own
    /// error type).
    pub fn verify_input_signatures(&self) -> Result<(), String> {
        if self.is_coinbase() {
            return Ok(());
        }
        let msg = self.signing_hash();
        let msg_bytes = msg.as_bytes();
        for (idx, input) in self.inputs.iter().enumerate() {
            let sig_hex = input
                .signature
                .as_deref()
                .ok_or_else(|| format!("input[{idx}]: missing signature"))?;
            let pk_hex = input
                .public_key
                .as_deref()
                .ok_or_else(|| format!("input[{idx}]: missing public_key"))?;
            let pk = PublicKey::from_hex(pk_hex)
                .map_err(|e| format!("input[{idx}]: invalid public_key hex: {e}"))?;
            let sig = Signature::from_hex(sig_hex)
                .map_err(|e| format!("input[{idx}]: invalid signature hex: {e}"))?;
            if !pk.verify(msg_bytes, &sig) {
                return Err(format!("input[{idx}]: ed25519 signature verification failed"));
            }
        }
        Ok(())
    }

    /// Whether this transparent Transfer carries a *verifiable* authorization
    /// to debit `from`, so the per-input ed25519 signature check is not its
    /// authorization model (#1725, hardened in #1727).
    ///
    /// Background: the producer/apply path routes a `Transfer` through
    /// `ProcessGovernanceTransaction` → `apply_transparent_transfer` →
    /// `BalanceTransfer`. That path debits the `TransparentTransferPayload`
    /// `from` account under nonce enforcement and never inspects per-input
    /// signatures; the block-time per-input check (#1279) lives only in the PoP
    /// sync validation (`verify_block` → `verify_block_transactions_fused`). For
    /// the producer and every syncing peer to agree, both must classify these
    /// transfers identically — but classification must be *cryptographic*, not
    /// structural, or a forged `from=victim` transfer is indistinguishable from
    /// a legitimate one (the #1727 forgery).
    ///
    /// Hardened authorization model (#1727). The pre-#1727 predicate was purely
    /// structural — `Transfer` + transparent payload + null input signatures —
    /// which a forged `from=victim` transfer satisfies identically to a
    /// legitimate one. Authorization now requires a verifiable, `from`-bound
    /// ed25519 signature embedded in the payload
    /// (`TransparentTransferPayload::verify_authorization`): the signer's
    /// derived address must equal `from`, and the signature must cover
    /// `from|to|amount|nonce` under a dedicated domain tag. This is checkable
    /// from the transaction alone, so the producer, mempool, and sync paths all
    /// reach the same verdict without any external key state.
    /// Whether this is a `Transfer` carrying a parseable transparent payload,
    /// regardless of whether its authorization is valid (#1727).
    ///
    /// Used by verification paths to decide that a transfer's authorization is
    /// the embedded `from`-bound signature (not per-input signatures), so an
    /// unauthorized transparent transfer is rejected rather than silently
    /// falling through to the per-input check.
    pub fn has_transparent_transfer_payload(&self) -> bool {
        self.tx_type == TransactionType::Transfer
            && self.parse_transparent_transfer_payload().is_some()
    }

    /// Verify the embedded `from`-bound authorization on a transparent transfer
    /// (#1727). Returns `false` for non-`Transfer` types, transfers without a
    /// transparent payload, or any payload whose authorization does not verify
    /// against `from` at this transaction's `nonce`.
    pub fn verify_transparent_transfer_authorization(&self) -> bool {
        // Mempool/pre-block validation has no State-backed declaration view.
        self.verify_transparent_transfer_authorization_with_view(
            &EmptyAuthorizationView,
            0,
            Timestamp::new(0),
        )
    }

    pub fn verify_transparent_transfer_authorization_with_view<V: crate::accounts::authorization::AuthorizationView>(
        &self, view: &V, height: u64, timestamp: Timestamp,
    ) -> bool {
        if self.tx_type != TransactionType::Transfer {
            return false;
        }
        match self.parse_transparent_transfer_payload() {
            Some(payload) => payload.verify_authorization_with_view(view, self.nonce, height, timestamp),
            None => false,
        }
    }

    /// [`Self::verify_transparent_transfer_authorization`] at a consensus
    /// height (#2169): additionally accepts a passkey-authorised transfer once
    /// `ConsensusFeature::PasskeyP256Auth` is active at `height`. Identical to
    /// the ed25519-only form below activation.
    pub fn verify_transparent_transfer_authorization_at_height(&self, height: u64) -> bool {
        if self.tx_type != TransactionType::Transfer {
            return false;
        }
        match self.parse_transparent_transfer_payload() {
            Some(payload) => payload.verify_authorization_at_height(self.nonce, height),
            None => false,
        }
    }

    /// [`Self::verify_transparent_transfer_authorization`] for mempool
    /// admission (#2169): additionally accepts a passkey-authorised transfer
    /// on a chain that schedules `passkey_p256_auth`. See
    /// `TransparentTransferPayload::verify_authorization_for_admission`.
    pub fn verify_transparent_transfer_authorization_for_admission(&self) -> bool {
        if self.tx_type != TransactionType::Transfer {
            return false;
        }
        match self.parse_transparent_transfer_payload() {
            Some(payload) => payload.verify_authorization_for_admission(self.nonce),
            None => false,
        }
    }

    /// Canonical message a `GovernanceOperator` key signs to authorize a
    /// system/governance transparent Transfer (#1727).
    ///
    /// Binds the economically meaningful fields — `from`, `to`, `amount` — and
    /// the transaction `nonce` so the authority signature cannot be lifted onto
    /// a different transfer or replayed at a different nonce. Returns `None`
    /// when the transaction does not carry a parseable
    /// [`TransparentTransferPayload`]. The signature deliberately does NOT cover
    /// the `authority`/`authority_signature` fields themselves (they are the
    /// signature container) nor per-input `signature` fields (which remain
    /// `None` on system transfers).
    pub fn system_transfer_authorization_message(&self) -> Option<Vec<u8>> {
        let payload = self.parse_transparent_transfer_payload()?;
        Some(system_transfer_authorization_message_with_domain(
            &signing_chain_id(),
            &signing_network_id(),
            &payload.from,
            &payload.to,
            payload.amount,
            self.nonce,
        ))
    }

    /// Whether this transfer carries verifiable governance provenance (#1727).
    ///
    /// Returns `true` only when the transparent payload names an `authority`
    /// and an `authority_signature` that verifies, via `registry`, against that
    /// identity's active `GovernanceOperator` key at the transaction timestamp,
    /// over [`Self::system_transfer_authorization_message`]. This is distinct
    /// from [`Self::verify_transparent_transfer_authorization`]: a sender can
    /// authorize debiting their own transparent account without proving that the
    /// transfer is system/governance-originated.
    pub fn verify_system_authority(&self, registry: &SigningKeyRegistry) -> bool {
        let Some(payload) = self.parse_transparent_transfer_payload() else {
            return false;
        };
        let (Some(authority), Some(signature_hex)) =
            (payload.authority.as_deref(), payload.authority_signature.as_deref())
        else {
            return false;
        };
        let authority = authority.trim();
        let signature_hex = signature_hex.trim();
        if authority.is_empty() || signature_hex.is_empty() {
            return false;
        }
        // Fail closed against the legacy identity-as-public-key fallback inside
        // `verify_signature_for_identity_role` (#1723): for an *unregistered*
        // identity that path treats the identity string itself as a public-key
        // hex, which would let an attacker self-certify a forged transfer by
        // setting `authority` to their own public key and signing with the
        // matching key. Require that `authority` is an explicitly registered
        // `GovernanceOperator` so the signature must come from a key the
        // attacker does not control — exactly the property the docstring claims.
        if !registry.has_identity_role(authority, SigningRole::GovernanceOperator) {
            return false;
        }
        let Some(message) = self.system_transfer_authorization_message() else {
            return false;
        };
        registry.verify_signature_for_identity_role(
            authority,
            SigningRole::GovernanceOperator,
            self.timestamp.timestamp(),
            &message,
            signature_hex,
        )
    }

    /// Whether the transparent payload *claims* governance authority, i.e. it
    /// carries an `authority` or `authority_signature` field (#1727).
    ///
    /// Used by the mempool defense-in-depth check: a transfer that claims
    /// authority but fails [`Self::verify_system_authority`] is rejected at
    /// admission rather than relayed to peers.
    pub fn claims_system_authority(&self) -> bool {
        self.parse_transparent_transfer_payload()
            .map(|payload| {
                payload.authority.is_some() || payload.authority_signature.is_some()
            })
            .unwrap_or(false)
    }

    /// Stamp governance provenance onto this transparent Transfer (#1727).
    ///
    /// Rewrites `data` so the [`TransparentTransferPayload`] carries `authority`
    /// and an `authority_signature` produced by `private_key` over
    /// [`Self::system_transfer_authorization_message`], then refreshes the
    /// transaction id. The caller is responsible for ensuring `private_key`
    /// corresponds to an active `GovernanceOperator` key registered for
    /// `authority` in the chain signing-key registry. Returns an error if the
    /// transaction does not already carry a transparent transfer payload.
    pub fn sign_system_authority(
        &mut self,
        authority: &str,
        private_key: &PrivateKey,
    ) -> Result<(), String> {
        let mut payload = self
            .parse_transparent_transfer_payload()
            .ok_or_else(|| "not a transparent transfer".to_string())?;
        // Clear any prior provenance so the signed message is computed over the
        // authorization fields only, never over a stale signature.
        payload.authority = None;
        payload.authority_signature = None;
        self.data = Some(
            serde_json::to_string(&payload)
                .map_err(|e| format!("failed to serialize transfer payload: {e}"))?,
        );
        // `parse_transparent_transfer_payload` is not memoized (no `OnceLock`
        // cache covers `Transfer` data), so mutating `data` here needs no cache
        // invalidation — the next parse re-reads the freshly written JSON.
        let message = self
            .system_transfer_authorization_message()
            .ok_or_else(|| "not a transparent transfer".to_string())?;
        payload.authority = Some(authority.to_string());
        payload.authority_signature = Some(private_key.sign(&message).to_hex());
        self.data = Some(
            serde_json::to_string(&payload)
                .map_err(|e| format!("failed to serialize transfer payload: {e}"))?,
        );
        self.set_id();
        Ok(())
    }

    /// Set transaction id from hash.
    pub fn set_id(&mut self) {
        self.id = Some(self.calculate_hash());
    }

    /// Hash accessor used by block builders and Merkle tree utilities.
    pub fn hash(&self) -> Hash {
        self.id.unwrap_or_else(|| self.calculate_hash())
    }

    /// Validate structural constraints (no business validation in PR1).
    pub fn validate_structure(&self) -> bool {
        if self.outputs.is_empty() {
            return false;
        }

        if self.outputs.iter().any(|o| o.address.is_empty()) {
            return false;
        }

        // #2025: reject a transaction whose output amounts overflow u64. No
        // legitimate transaction sums past u64::MAX (total supply is far below
        // it), so this only rejects crafted/absurd output sets — a
        // deterministic, admission-side robustness guard that pairs with the
        // saturating sum in `Transaction::new`.
        if self
            .outputs
            .iter()
            .try_fold(0u64, |acc, o| acc.checked_add(o.amount))
            .is_none()
        {
            return false;
        }

        if self.is_coinbase() {
            return self.inputs.len() == 1 && self.inputs[0].coinbase_data.is_some();
        }

        !self.inputs.is_empty()
    }

    /// Approximate serialized size for mempool admission and block packing.
    pub fn size(&self) -> usize {
        serde_json::to_vec(self)
            .map(|bytes| bytes.len())
            .unwrap_or(0)
    }
}

impl fmt::Display for Transaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Tx(id: {}, type: {}, inputs: {}, outputs: {}, amount: {}, fee: {})",
            self.id
                .as_ref()
                .map(|h| h.to_string())
                .unwrap_or_else(|| "none".to_string()),
            self.tx_type,
            self.inputs.len(),
            self.outputs.len(),
            self.amount,
            self.fee
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signature::{PrivateKey, PublicKey};
    use crate::security::key_registry::{SigningKeyRegistry, SigningRole};
    use rand::{SeedableRng, rngs::StdRng};

    // -----------------------------------------------------------------------
    // #2034 — chain-id / network-id domain separation on money-path preimages.
    //
    // Each money-path preimage must now bind the (chain_id, network_id) identity
    // so a signature produced on one network (e.g. testnet `777`) cannot be
    // replayed on another (e.g. mainnet). These tests exercise the pure
    // `*_with_domain` forms (deterministic — no env), assert cross-network
    // divergence + same-network determinism, and pin exact golden hex vectors so
    // the cross-repo signer mirrors (collector, backend faucet, TS/SDK) can be
    // validated byte-for-byte against the Rust source of truth.
    // -----------------------------------------------------------------------

    // Canonical testnet identity (matches the live deployment: chain_id 777,
    // network_id testnet) and the mainnet fallback, for the replay-barrier tests.
    const TESTNET_CID: &str = "777";
    const TESTNET_NID: &str = "testnet";
    const MAINNET_CID: &str = crate::crypto::domain_signing::DEFAULT_CHAIN_ID; // "wrfcoin-main"
    const MAINNET_NID: &str = crate::crypto::domain_signing::DEFAULT_NETWORK_ID; // "mainnet"

    /// #3277 S4: blocks are bincode, which encodes an enum variant by its
    /// POSITION. `FeatureScheduleExtension` must be appended last, so every
    /// variant a deployed block can carry keeps its wire index; inserting it
    /// anywhere else would make every later tx type decode as another type.
    #[test]
    fn transaction_type_wire_indices_are_append_only_3277() {
        let pinned = [
            TransactionType::Regular,
            TransactionType::Coinbase,
            TransactionType::Transfer,
            TransactionType::Stake,
            TransactionType::Unstake,
            TransactionType::NftCreation,
            TransactionType::NftTransfer,
            TransactionType::NftExecution,
            TransactionType::DataAnalysis,
            TransactionType::DataCollection,
            TransactionType::DataStorage,
            TransactionType::EnvironmentalOffset,
            TransactionType::BridgeLock,
            TransactionType::BridgeUnlock,
            TransactionType::ContributionCommit,
            TransactionType::ContributionReveal,
            TransactionType::ForecastCommit,
            TransactionType::ForecastReveal,
            TransactionType::OracleAttestation,
            TransactionType::GovernanceProposal,
            TransactionType::GovernanceVote,
            TransactionType::GovernanceExecute,
            TransactionType::GovernanceFinalize,
            TransactionType::GovernanceDelegate,
            TransactionType::TreasuryDeposit,
            TransactionType::TreasuryWithdraw,
            TransactionType::Reward,
            TransactionType::ConfidentialTransfer,
            TransactionType::ShieldedDeposit,
            TransactionType::ShieldedWithdrawal,
            TransactionType::Layer2BatchSubmit,
            TransactionType::RewardClaim,
            TransactionType::SlashingReport,
            TransactionType::OracleRegistration,
            TransactionType::StationRegistration,
            TransactionType::FeatureScheduleExtension,
        ];
        for (index, tx_type) in pinned.iter().enumerate() {
            let bytes = bincode::serialize(tx_type).expect("encode");
            assert_eq!(bytes, (index as u32).to_le_bytes(), "{tx_type} moved on the wire");
            let back: TransactionType = bincode::deserialize(&bytes).expect("decode");
            assert_eq!(back, *tx_type);
        }
        assert_eq!(
            bincode::serialize(&TransactionType::FeatureScheduleExtension).unwrap(),
            35u32.to_le_bytes(),
            "the extension is the LAST variant"
        );
    }

    // Pinned golden preimage for the transfer tuple below (see the golden-vector
    // test). Captured from the Rust source of truth; mirrors must match it.
    const TRANSFER_GOLDEN_HEX_2034: &str = "777266636f696e5f7472616e73706172656e745f7472616e736665725f617574685f76320300000037373707000000746573746e6574080000007772665f66726f6d060000007772665f746fe8030000000000000500000000000000";

    #[test]
    fn transparent_transfer_message_binds_network_identity_2034() {
        let testnet = transparent_transfer_authorization_message_with_domain(
            TESTNET_CID, TESTNET_NID, "wrf_from", "wrf_to", 1000, 5,
        );
        let mainnet = transparent_transfer_authorization_message_with_domain(
            MAINNET_CID, MAINNET_NID, "wrf_from", "wrf_to", 1000, 5,
        );
        // Cross-network replay barrier: identical transfer, different network → different bytes.
        assert_ne!(testnet, mainnet, "transfer preimage must differ across networks");
        // Determinism: same inputs → same bytes (cross-signer parity).
        assert_eq!(
            testnet,
            transparent_transfer_authorization_message_with_domain(
                TESTNET_CID, TESTNET_NID, "wrf_from", "wrf_to", 1000, 5,
            )
        );
        // Distinct chain_id alone (same network family) also diverges.
        let other_chain = transparent_transfer_authorization_message_with_domain(
            "778", TESTNET_NID, "wrf_from", "wrf_to", 1000, 5,
        );
        assert_ne!(testnet, other_chain, "chain_id must be bound, not just network_id");
        // v2 domain tag is present.
        assert!(testnet.starts_with(TRANSPARENT_TRANSFER_AUTH_DOMAIN));
        assert!(TRANSPARENT_TRANSFER_AUTH_DOMAIN.ends_with(b"_v2"));
    }

    // #2229 — governance-vote preimage coverage. The V2
    // `verify_vote_signature_for` path uses the type's `DomainSeparated` impl
    // so chain and network identity are fields of the signed preimage.

    #[test]
    fn governance_vote_v2_rejects_mismatched_chain_identity_2229() {
        use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
        use crate::crypto::domain_signing::{DomainSeparated, sign_domain_for};

        let mut rng = StdRng::seed_from_u64(2229);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);

        let mut vote = GovernanceVotePayload {
            proposal_id: "prop-42".to_string(),
            approve: true,
            voting_power: Some(10),
            signature: None,
            signer_public_key: Some(pk.to_hex()),
        };

        let testnet = ChainIdentity::new(TESTNET_CID, TESTNET_NID);
        let mainnet = ChainIdentity::new(MAINNET_CID, MAINNET_NID);

        // Under V2 (active from the #2034 fresh-genesis cut), a signature
        // computed with the testnet identity must validate with that identity
        // and must not validate when either identity field changes.
        let sig = sign_domain_for(&sk, &vote, &testnet, SigningProtocolVersion::V2);
        vote.signature = Some(sig.to_hex());
        assert!(
            vote.verify_vote_signature_for(SigningProtocolVersion::V2, &testnet),
            "the exact V2 identity must validate"
        );
        assert!(
            !vote.verify_vote_signature_for(SigningProtocolVersion::V2, &mainnet),
            "#2229: a signature computed with the testnet identity must not validate with mainnet identity"
        ); // secret-leak-ok: SigningProtocolVersion is a public enum; the assertion message is constant and no secret is formatted.
        // Changing only chain_id while retaining network_id must also fail.
        let other_chain = ChainIdentity::new("778", TESTNET_NID);
        assert!(
            !vote.verify_vote_signature_for(SigningProtocolVersion::V2, &other_chain),
            "#2229: chain_id must be bound, not just network_id"
        ); // secret-leak-ok: SigningProtocolVersion is a public enum; the assertion message is constant and no secret is formatted.

        // The preimage carries the domain tag first, so the bytes cannot be
        // reinterpreted as any other signed object (the #2065 cross-domain shape).
        let preimage = vote.signing_preimage_for(&testnet, SigningProtocolVersion::V2);
        let tag = GovernanceVotePayload::DOMAIN_TAG;
        assert_eq!(&preimage[0..8], &(tag.len() as u64).to_le_bytes());
        assert_eq!(&preimage[8..8 + tag.len()], tag);
    }

    #[test]
    fn vote_verification_is_version_gated_v1_legacy_v2_domain_2229() {
        // #2229: the migration is a clean version gate, not a rolling asymmetry.
        // A legacy-string signature verifies ONLY at V1; a domain signature
        // verifies ONLY at V2. Uses the explicit-version verifier so the test is
        // independent of ambient `OnceLock` protocol state.
        use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
        use crate::crypto::domain_signing::sign_domain_for;

        let mut rng = StdRng::seed_from_u64(22290);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);
        let testnet = ChainIdentity::new(TESTNET_CID, TESTNET_NID);

        let base = GovernanceVotePayload {
            proposal_id: "prop-7".to_string(),
            approve: false,
            voting_power: Some(3),
            signature: None,
            signer_public_key: Some(pk.to_hex()),
        };

        // Legacy V1 signature: verifies at V1, rejected under the V2 frame.
        let mut v1_vote = base.clone();
        v1_vote.signature = Some(sk.sign(v1_vote.canonical_signing_message().as_bytes()).to_hex());
        assert!(
            v1_vote.verify_vote_signature_for(SigningProtocolVersion::V1, &testnet),
            "a legacy V1 signature must verify at V1 (pre-cut compatibility)"
        ); // secret-leak-ok: SigningProtocolVersion is a public enum; the assertion message is constant and no secret is formatted.
        assert!(
            !v1_vote.verify_vote_signature_for(SigningProtocolVersion::V2, &testnet),
            "#2229: a legacy V1 signature must NOT verify under the V2 domain frame"
        ); // secret-leak-ok: SigningProtocolVersion is a public enum; the assertion message is constant and no secret is formatted.

        // V2 domain signature: verifies at V2, rejected under the legacy V1 form.
        let mut v2_vote = base.clone();
        let domain_sig = sign_domain_for(&sk, &v2_vote, &testnet, SigningProtocolVersion::V2);
        v2_vote.signature = Some(domain_sig.to_hex());
        assert!(
            v2_vote.verify_vote_signature_for(SigningProtocolVersion::V2, &testnet),
            "a V2 domain signature must verify at V2"
        ); // secret-leak-ok: SigningProtocolVersion is a public enum; the assertion message is constant and no secret is formatted.
        assert!(
            !v2_vote.verify_vote_signature_for(SigningProtocolVersion::V1, &testnet),
            "#2229: a V2 domain signature must NOT verify under the legacy V1 preimage"
        ); // secret-leak-ok: SigningProtocolVersion is a public enum; the assertion message is constant and no secret is formatted.
    }

    #[test]
    fn system_transfer_message_covers_identity_and_fields_2121() {
        let base = system_transfer_authorization_message_with_domain(
            TESTNET_CID,
            TESTNET_NID,
            "wrf_from",
            "wrf_to",
            1_000,
            5,
        );
        assert!(base.starts_with(SYSTEM_TRANSFER_AUTH_DOMAIN));
        assert!(SYSTEM_TRANSFER_AUTH_DOMAIN.ends_with(b"_v2"));
        for changed in [
            system_transfer_authorization_message_with_domain(
                MAINNET_CID,
                TESTNET_NID,
                "wrf_from",
                "wrf_to",
                1_000,
                5,
            ),
            system_transfer_authorization_message_with_domain(
                TESTNET_CID,
                MAINNET_NID,
                "wrf_from",
                "wrf_to",
                1_000,
                5,
            ),
            system_transfer_authorization_message_with_domain(
                TESTNET_CID,
                TESTNET_NID,
                "wrf_from_b",
                "wrf_to",
                1_000,
                5,
            ),
            system_transfer_authorization_message_with_domain(
                TESTNET_CID,
                TESTNET_NID,
                "wrf_from",
                "wrf_to_b",
                1_000,
                5,
            ),
            system_transfer_authorization_message_with_domain(
                TESTNET_CID,
                TESTNET_NID,
                "wrf_from",
                "wrf_to",
                1_001,
                5,
            ),
            system_transfer_authorization_message_with_domain(
                TESTNET_CID,
                TESTNET_NID,
                "wrf_from",
                "wrf_to",
                1_000,
                6,
            ),
        ] {
            assert_ne!(
                base, changed,
                "every system-transfer authority field must affect the signed preimage",
            );
        }
    }

    #[test]
    fn stake_message_binds_network_identity_2034() {
        let testnet = stake_authorization_message_with_domain(
            TESTNET_CID, TESTNET_NID, "wrf_acct", 500, false, 3,
        );
        let mainnet = stake_authorization_message_with_domain(
            MAINNET_CID, MAINNET_NID, "wrf_acct", 500, false, 3,
        );
        assert_ne!(testnet, mainnet, "stake preimage must differ across networks");
        assert!(STAKE_AUTH_DOMAIN.ends_with(b"_v2"));
    }

    #[test]
    fn commit_message_binds_network_identity_2034() {
        let testnet = commit_authorization_message_with_domain(
            TESTNET_CID, TESTNET_NID, "station-1", "commit-abc", "contribution",
        );
        let mainnet = commit_authorization_message_with_domain(
            MAINNET_CID, MAINNET_NID, "station-1", "commit-abc", "contribution",
        );
        assert_ne!(testnet, mainnet, "commit preimage must differ across networks");
        assert!(COMMIT_AUTH_DOMAIN.ends_with(b"_v2"));
    }

    #[test]
    fn reward_claim_preimage_covers_identity_and_claim_fields_2121() {
        let base = RewardClaimPayload {
            epoch: 7,
            ring_signature: "excluded-output".to_string(),
            key_image: "11".repeat(32),
            amount_commitment: "22".repeat(32),
            range_proof: "33".repeat(32),
            destination: "wrf_destination_a".to_string(),
        };
        let message = reward_claim_authorization_message_with_domain(
            TESTNET_CID,
            TESTNET_NID,
            &base,
        );
        assert!(message.starts_with(REWARD_CLAIM_AUTH_DOMAIN));
        assert!(REWARD_CLAIM_AUTH_DOMAIN.ends_with(b"_v2"));

        let mut variants = Vec::new();
        let mut changed = base.clone();
        changed.epoch += 1;
        variants.push(changed);
        let mut changed = base.clone();
        changed.key_image = "44".repeat(32);
        variants.push(changed);
        let mut changed = base.clone();
        changed.amount_commitment = "55".repeat(32);
        variants.push(changed);
        let mut changed = base.clone();
        changed.range_proof = "66".repeat(32);
        variants.push(changed);
        let mut changed = base.clone();
        changed.destination = "wrf_destination_b".to_string();
        variants.push(changed);

        for changed in variants {
            assert_ne!(
                message,
                reward_claim_authorization_message_with_domain(
                    TESTNET_CID,
                    TESTNET_NID,
                    &changed,
                ),
                "every consensus-relevant reward-claim field must affect the signed preimage",
            );
        }
        assert_ne!(
            message,
            reward_claim_authorization_message_with_domain(
                MAINNET_CID,
                TESTNET_NID,
                &base,
            ),
            "chain id must affect the signed preimage",
        );
        assert_ne!(
            message,
            reward_claim_authorization_message_with_domain(
                TESTNET_CID,
                MAINNET_NID,
                &base,
            ),
            "network id must affect the signed preimage",
        );

        let mut signature_only = base.clone();
        signature_only.ring_signature = "different-output".to_string();
        assert_eq!(
            message,
            reward_claim_authorization_message_with_domain(
                TESTNET_CID,
                TESTNET_NID,
                &signature_only,
            ),
            "the signature output must be excluded from its own preimage",
        );
    }

    #[test]
    fn signing_hash_binds_network_identity_2034() {
        // A minimal value-transfer transaction; the exact fields don't matter,
        // only that the resolved sighash differs across networks.
        let tx = Transaction::new(
            vec![],
            vec![TransactionOutput {
                amount: 1000,
                address: "wrf_to".to_string(),
                script: String::new(),
                nft_data: None,
            }],
        );
        let testnet = tx.signing_hash_with_domain(TESTNET_CID, TESTNET_NID);
        let mainnet = tx.signing_hash_with_domain(MAINNET_CID, MAINNET_NID);
        assert_ne!(
            testnet, mainnet,
            "value-transfer sighash must differ across networks (cross-network replay barrier)"
        );
        // Determinism for the same identity.
        assert_eq!(testnet, tx.signing_hash_with_domain(TESTNET_CID, TESTNET_NID));  // secret-leak-ok: a hash, not key material
    }

    #[test]
    fn transparent_transfer_golden_vector_testnet_2034() {
        // GOLDEN VECTOR — cross-repo signer mirrors (collector / backend faucet /
        // TS SDK) MUST reproduce these exact preimage bytes for this tuple, or a
        // signature they produce will not verify on the node. If this test needs
        // updating, the wire format changed and every mirror must be re-synced.
        // Tuple: chain_id="777", network_id="testnet",
        //        from="wrf_from", to="wrf_to", amount=1000, nonce=5.
        let msg = transparent_transfer_authorization_message_with_domain(
            "777", "testnet", "wrf_from", "wrf_to", 1000, 5,
        );
        assert_eq!(hex::encode(&msg), TRANSFER_GOLDEN_HEX_2034);
    }


    fn inline_reveal() -> RevealPayload {
        RevealPayload {
            commit_id: "abc".to_string(),
            station_id: "station-1".to_string(),
            timestamp: 1_700_000_000,
            contribution_type: "contribution".to_string(),
            data: serde_json::json!({"temp": 20.0}),
            nonce: "nonce-1".to_string(),
            data_cid: None,
            data_hash: None,
            storage_provider: None,
            provenance: None,
            manifest: None,
            verification_receipt: None,
            compute_receipt: None,
                quality_tier: None,
        }
    }

    fn cid_reveal() -> RevealPayload {
        let mut r = inline_reveal();
        r.data_cid = Some("QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG".to_string());
        r.data_hash =
            Some("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string());
        r.storage_provider = Some("ipfs".to_string());
        r
    }

    #[test]
    fn validate_cid_fields_all_none_valid() {
        let r = inline_reveal();
        assert!(r.validate_cid_fields().is_ok());
        assert!(!r.is_cid_referenced());
    }

    #[test]
    fn validate_cid_fields_all_present_valid() {
        let r = cid_reveal();
        assert!(r.validate_cid_fields().is_ok());
        assert!(r.is_cid_referenced());
    }

    #[test]
    fn validate_cid_fields_partial_rejected() {
        let mut r = inline_reveal();
        r.data_cid = Some("QmTest".to_string());
        let err = r.validate_cid_fields().unwrap_err();
        assert_eq!(err.code(), "SP_TX_CID_FIELDS_PARTIAL");
    }

    #[test]
    fn validate_cid_fields_invalid_hash_rejected() {
        let mut r = cid_reveal();
        r.data_hash = Some("not-a-hex-hash".to_string());
        let err = r.validate_cid_fields().unwrap_err();
        assert_eq!(err.code(), "SP_TX_DATA_HASH_FORMAT");
    }

    #[test]
    fn validate_cid_fields_unknown_provider_rejected() {
        let mut r = cid_reveal();
        r.storage_provider = Some("s3".to_string());
        let err = r.validate_cid_fields().unwrap_err();
        assert_eq!(err.code(), "SP_TX_STORAGE_PROVIDER_INVALID");
    }

    #[test]
    fn validate_cid_fields_empty_cid_rejected() {
        let mut r = cid_reveal();
        r.data_cid = Some("".to_string());
        let err = r.validate_cid_fields().unwrap_err();
        assert_eq!(err.code(), "SP_TX_CID_EMPTY");
    }

    #[test]
    fn validate_cid_fields_rejects_surrounding_whitespace() {
        let mut r = cid_reveal();
        r.data_cid = Some(" QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG ".to_string());
        let err = r.validate_cid_fields().unwrap_err();
        assert_eq!(err.code(), "SP_TX_CID_SURROUNDING_WHITESPACE");
    }

    #[test]
    fn validate_cid_fields_rejects_ipfs_cid_with_control_characters() {
        let mut r = cid_reveal();
        r.data_cid = Some("QmYwAPJzv5CZsnN625s3Xf2n\u{0007}emtYgPpHdWEz79ojWnPbdG".to_string());
        let err = r.validate_cid_fields().unwrap_err();
        assert_eq!(err.code(), "SP_TX_CID_CONTROL_CHARS");
    }

    #[test]
    fn validate_cid_fields_rejects_overly_long_cid() {
        let mut r = cid_reveal();
        r.data_cid = Some(format!("Qm{}", "A".repeat(300)));
        let err = r.validate_cid_fields().unwrap_err();
        assert_eq!(err.code(), "SP_TX_CID_TOO_LONG");
    }

    #[test]
    fn validate_cid_fields_rejects_mismatched_provider_prefix() {
        let mut r = cid_reveal();
        r.data_cid = Some("arweave:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
        let err = r.validate_cid_fields().unwrap_err();
        assert_eq!(err.code(), "SP_TX_CID_PREFIX_PROVIDER_MISMATCH");
    }

    #[test]
    fn validate_cid_fields_accepts_prefixed_ipfs_cid() {
        let mut r = cid_reveal();
        r.data_cid = Some("ipfs://QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG".to_string());
        assert!(r.validate_cid_fields().is_ok());
    }

    #[test]
    fn validate_cid_fields_rejects_invalid_ipfs_format() {
        let mut r = cid_reveal();
        r.data_cid = Some("QmNotAValidCIDBecauseContains0".to_string());
        let err = r.validate_cid_fields().unwrap_err();
        assert_eq!(err.code(), "SP_TX_CID_FORMAT_IPFS_FILECOIN");
    }

    #[test]
    fn validate_cid_fields_rejects_invalid_arweave_tx_id() {
        let mut r = cid_reveal();
        r.storage_provider = Some("arweave".to_string());
        r.data_cid = Some("too_short".to_string());
        let err = r.validate_cid_fields().unwrap_err();
        assert_eq!(err.code(), "SP_TX_CID_FORMAT_ARWEAVE");
    }

    #[test]
    fn validate_cid_fields_malformed_combinations_do_not_panic() {
        let malformed = vec![
            {
                let mut r = inline_reveal();
                r.data_cid = Some("QmOnlyCid".to_string());
                r
            },
            {
                let mut r = inline_reveal();
                r.data_hash = Some("abc".to_string());
                r
            },
            {
                let mut r = inline_reveal();
                r.storage_provider = Some("ipfs".to_string());
                r
            },
            {
                let mut r = cid_reveal();
                r.data_hash = None;
                r
            },
            {
                let mut r = cid_reveal();
                r.storage_provider = None;
                r
            },
        ];

        for payload in malformed {
            let result = std::panic::catch_unwind(|| payload.validate_cid_fields());
            match result {
                Ok(validation_result) => {
                    assert!(
                        validation_result.is_err(),
                        "malformed payload must be rejected"
                    );
                },
                Err(_) => panic!("validate_cid_fields should not panic"),
            }
        }
    }

    #[test]
    fn validate_cid_fields_filecoin_accepted() {
        let mut r = cid_reveal();
        r.storage_provider = Some("filecoin".to_string());
        assert!(r.validate_cid_fields().is_ok());
    }

    #[test]
    fn validate_cid_fields_arweave_accepted() {
        let mut r = cid_reveal();
        r.data_cid = Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
        r.storage_provider = Some("arweave".to_string());
        assert!(r.validate_cid_fields().is_ok());
    }

    #[test]
    fn reexported_forecast_manifest_preserves_inherent_digest_helper() {
        let manifest = ForecastArtifactManifest {
            schema_version: 1,
            model_id: "wrf-v1".to_string(),
            input_hash: "a".repeat(64),
            output_hash: "b".repeat(64),
            produced_at: 1_700_000_000,
            region: "us-west".to_string(),
            horizon_hours: 24,
            input_variables: vec!["humidity".to_string(), "pressure".to_string()],
            output_variables: vec!["temperature".to_string()],
        };

        assert_eq!(manifest.manifest_digest(), forecast_manifest_digest(&manifest));
    }

    #[test]
    fn reexported_forecast_receipt_preserves_signature_helpers() {
        let mut rng = StdRng::seed_from_u64(7);
        let private_key = PrivateKey::generate(&mut rng);
        let public_key = PublicKey::from_private_key(&private_key);
        let mut receipt = ForecastVerificationReceipt {
            manifest_digest: "a".repeat(64),
            commit_id: "commit-1".to_string(),
            oracle_id: public_key.to_hex(),
            metrics: ForecastAccuracyMetrics {
                mae: 1.0,
                rmse: 1.5,
                bias: 0.0,
                skill_score: 0.8,
            },
            verified_at: 1_700_000_001,
            nonce: "nonce-1".to_string(),
            zk_proof: None,
            signature: None,
        };

        let signature = private_key.sign(receipt.signing_message().as_bytes());
        receipt.signature = Some(signature.to_hex());

        assert_eq!(
            receipt.signing_message(),
            forecast_receipt_signing_message(&receipt)
        ); // secret-leak-ok: signing_message is a public canonical preimage; no private key or secret reaches the assertion.
        assert!(receipt.has_valid_signature());
        assert!(forecast_receipt_has_valid_signature(&receipt));
    }

    #[test]
    fn validate_provenance_fields_accepts_populated_metadata() {
        let mut r = inline_reveal();
        r.provenance = Some(SensorProvenance {
            source_id: Some("station-1".to_string()),
            manufacturer: Some("acme".to_string()),
            model: Some("wx-200".to_string()),
            firmware_hash: Some(
                "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string(),
            ),
            calibration_attestation_cid: Some(
                "ipfs://QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG".to_string(),
            ),
        });
        assert!(r.validate_provenance_fields().is_ok());
    }

    #[test]
    fn canonicalize_calibration_attestation_cid_equivalent_ipfs_forms_match() {
        let bare = canonicalize_calibration_attestation_cid(
            "QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG",
        )
        .expect("bare CID should canonicalize");
        let with_scheme = canonicalize_calibration_attestation_cid(
            "ipfs://QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG",
        )
        .expect("ipfs:// CID should canonicalize");
        let with_path = canonicalize_calibration_attestation_cid(
            "/ipfs/QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG",
        )
        .expect("/ipfs/ CID should canonicalize");

        assert_eq!(bare, "ipfs:QmYwAPJzv5CZsnN625s3Xf2nemtYgPpHdWEz79ojWnPbdG");
        assert_eq!(bare, with_scheme);
        assert_eq!(bare, with_path);
    }

    #[test]
    fn canonicalize_calibration_attestation_cid_accepts_arweave_aliases() {
        let bare =
            canonicalize_calibration_attestation_cid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .expect("bare arweave id should canonicalize");
        let prefixed = canonicalize_calibration_attestation_cid(
            "ar://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .expect("ar:// id should canonicalize");

        assert_eq!(bare, "arweave:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(bare, prefixed);
    }

    #[test]
    fn canonicalize_calibration_attestation_cid_rejects_malformed_value() {
        let err = canonicalize_calibration_attestation_cid("ipfs://QmNotAValidCIDBecauseContains0")
            .expect_err("malformed CID must be rejected");
        assert_eq!(err.code(), "SP_TX_PROV_CALIBRATION_CID_INVALID");
    }

    #[test]
    fn validate_provenance_fields_rejects_empty_object() {
        let mut r = inline_reveal();
        r.provenance = Some(SensorProvenance {
            source_id: None,
            manufacturer: None,
            model: None,
            firmware_hash: None,
            calibration_attestation_cid: None,
        });
        let err = r.validate_provenance_fields().unwrap_err();
        assert_eq!(err.code(), "SP_TX_PROV_EMPTY");
    }

    #[test]
    fn validate_provenance_fields_rejects_invalid_firmware_hash() {
        let mut r = inline_reveal();
        r.provenance = Some(SensorProvenance {
            source_id: Some("station-1".to_string()),
            manufacturer: None,
            model: None,
            firmware_hash: Some("not-a-hash".to_string()),
            calibration_attestation_cid: None,
        });
        let err = r.validate_provenance_fields().unwrap_err();
        assert_eq!(err.code(), "SP_TX_PROV_FIRMWARE_HASH_FORMAT");
    }

    #[test]
    fn calibration_attestation_registry_verification_respects_epoch() {
        let mut rng = StdRng::seed_from_u64(77);
        let identity_key = PrivateKey::generate(&mut rng);
        let rotated_key = PrivateKey::generate(&mut rng);
        let calibrator_id = PublicKey::from_private_key(&identity_key).to_string();
        let rotated_pk = PublicKey::from_private_key(&rotated_key).to_string();

        let mut registry = SigningKeyRegistry::default();
        registry
            .ensure_legacy_identity_key(&calibrator_id, SigningRole::Calibrator, 50)
            .unwrap();
        registry
            .register_key_epoch(&calibrator_id, SigningRole::Calibrator, 1, rotated_pk, 100)
            .unwrap();

        let mut payload = CalibrationAttestationPayload {
            station_id: "station-1".to_string(),
            calibrator_id: calibrator_id.clone(),
            issued_at: 150,
            expires_at: 250,
            calibrator_key_epoch: Some(1),
            signature: None,
            nonce: Some("cal-epoch".to_string()),
        };
        payload.signature = Some(
            rotated_key
                .sign(payload.signing_message().as_bytes())
                .to_string(),
        );
        assert!(payload.has_valid_signature_with_registry(&registry));

        registry
            .revoke_key_epoch(&calibrator_id, SigningRole::Calibrator, 1, 160)
            .unwrap();
        payload.issued_at = 170;
        payload.signature = Some(
            rotated_key
                .sign(payload.signing_message().as_bytes())
                .to_string(),
        );
        assert!(!payload.has_valid_signature_with_registry(&registry));
    }

    fn valid_manifest() -> ForecastArtifactManifest {
        ForecastArtifactManifest {
            schema_version: 1,
            model_id: "wrf-gfs-v3".to_string(),
            input_hash: "a".repeat(64),
            output_hash: "b".repeat(64),
            produced_at: 1_700_000_000,
            region: "us-east".to_string(),
            horizon_hours: 48,
            input_variables: vec![
                "humidity".to_string(),
                "pressure".to_string(),
                "temperature".to_string(),
            ],
            output_variables: vec!["precipitation".to_string(), "temperature".to_string()],
        }
    }

    fn valid_metrics() -> ForecastAccuracyMetrics {
        ForecastAccuracyMetrics {
            mae: 1.5,
            rmse: 2.0,
            bias: -0.3,
            skill_score: 0.85,
        }
    }

    fn valid_receipt(manifest: &ForecastArtifactManifest) -> ForecastVerificationReceipt {
        ForecastVerificationReceipt {
            manifest_digest: forecast_manifest_digest(manifest),
            commit_id: "test-commit".to_string(),
            oracle_id: "oracle-1".to_string(),
            metrics: valid_metrics(),
            verified_at: 1_700_000_100,
            nonce: "receipt-nonce".to_string(),
            zk_proof: None,
            signature: None,
        }
    }

    // zk_proof content is not validated by validate() — that check was removed
    // to prevent a consensus fork (old nodes accepted any zk_proof string; a
    // length/hex guard would reject replayed receipts on upgraded nodes).
    #[test]
    fn receipt_accepts_any_zk_proof_content() {
        let manifest = valid_manifest();
        let mut receipt = valid_receipt(&manifest);

        receipt.zk_proof = Some("not-hex".to_string());
        assert!(receipt.validate().is_ok(), "non-hex zk_proof must not fail validation");

        receipt.zk_proof = Some("ab".repeat(16));
        assert!(receipt.validate().is_ok(), "short zk_proof must not fail validation");

        receipt.zk_proof = Some("ab".repeat(192));
        assert!(receipt.validate().is_ok(), "full-length zk_proof must pass validation");
    }

    #[test]
    fn manifest_validate_accepts_valid() {
        assert!(valid_manifest().validate().is_ok());
    }

    #[test]
    fn manifest_rejects_wrong_schema_version() {
        let mut m = valid_manifest();
        m.schema_version = 2;
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::InvalidSchemaVersion)
        ));
    }

    #[test]
    fn manifest_rejects_empty_model_id() {
        let mut m = valid_manifest();
        m.model_id = String::new();
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::EmptyModelId)
        ));
    }

    #[test]
    fn manifest_rejects_long_model_id() {
        let mut m = valid_manifest();
        m.model_id = "x".repeat(129);
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::ModelIdTooLong)
        ));
    }

    #[test]
    fn manifest_rejects_invalid_hash_format() {
        let mut m = valid_manifest();
        m.input_hash = "not-a-valid-hex".to_string();
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::InvalidInputHash)
        ));

        let mut m2 = valid_manifest();
        m2.output_hash = "zz".repeat(32);
        assert!(matches!(
            m2.validate(),
            Err(ForecastManifestValidationError::InvalidOutputHash)
        ));
    }

    #[test]
    fn manifest_rejects_invalid_horizon() {
        let mut m = valid_manifest();
        m.horizon_hours = 0;
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::InvalidHorizonRange)
        ));

        let mut m2 = valid_manifest();
        m2.horizon_hours = 721;
        assert!(matches!(
            m2.validate(),
            Err(ForecastManifestValidationError::InvalidHorizonRange)
        ));
    }

    #[test]
    fn manifest_rejects_unsorted_variables() {
        let mut m = valid_manifest();
        m.input_variables = vec!["temperature".to_string(), "humidity".to_string()];
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::InputVariablesUnsorted)
        ));
    }

    #[test]
    fn manifest_rejects_duplicate_variables() {
        let mut m = valid_manifest();
        m.output_variables = vec!["temperature".to_string(), "temperature".to_string()];
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::OutputVariablesDuplicates)
        ));
    }

    #[test]
    fn manifest_rejects_empty_variables() {
        let mut m = valid_manifest();
        m.input_variables = vec![];
        assert!(matches!(
            m.validate(),
            Err(ForecastManifestValidationError::EmptyInputVariables)
        ));
    }

    #[test]
    fn manifest_deterministic_digest_is_stable() {
        let m1 = valid_manifest();
        let m2 = valid_manifest();
        assert_eq!(forecast_manifest_digest(&m1), forecast_manifest_digest(&m2));
    }

    #[test]
    fn manifest_field_change_changes_digest() {
        let m1 = valid_manifest();
        let mut m2 = valid_manifest();
        m2.horizon_hours = 72;
        assert_ne!(forecast_manifest_digest(&m1), forecast_manifest_digest(&m2));
    }

    #[test]
    fn metrics_validate_accepts_valid() {
        assert!(valid_metrics().validate().is_ok());
    }

    #[test]
    fn metrics_rejects_negative_mae() {
        let mut m = valid_metrics();
        m.mae = -1.0;
        assert!(matches!(
            m.validate(),
            Err(ForecastMetricsValidationError::InvalidMae)
        ));
    }

    #[test]
    fn metrics_rejects_nan_rmse() {
        let mut m = valid_metrics();
        m.rmse = f64::NAN;
        assert!(matches!(
            m.validate(),
            Err(ForecastMetricsValidationError::InvalidRmse)
        ));
    }

    #[test]
    fn metrics_rejects_skill_score_out_of_range() {
        let mut m = valid_metrics();
        m.skill_score = 1.1;
        assert!(matches!(
            m.validate(),
            Err(ForecastMetricsValidationError::InvalidSkillScore)
        ));

        let mut m2 = valid_metrics();
        m2.skill_score = -0.1;
        assert!(matches!(
            m2.validate(),
            Err(ForecastMetricsValidationError::InvalidSkillScore)
        ));
    }

    #[test]
    fn receipt_validate_accepts_valid() {
        let m = valid_manifest();
        let r = valid_receipt(&m);
        assert!(r.validate().is_ok());
    }

    #[test]
    fn receipt_rejects_invalid_manifest_digest() {
        let m = valid_manifest();
        let mut r = valid_receipt(&m);
        r.manifest_digest = "too-short".to_string();
        assert!(matches!(
            r.validate(),
            Err(ForecastReceiptValidationError::InvalidManifestDigest)
        ));
    }

    #[test]
    fn receipt_rejects_empty_commit_id() {
        let m = valid_manifest();
        let mut r = valid_receipt(&m);
        r.commit_id = String::new();
        assert!(matches!(
            r.validate(),
            Err(ForecastReceiptValidationError::EmptyCommitId)
        ));
    }

    #[test]
    fn receipt_rejects_invalid_verified_at() {
        let m = valid_manifest();
        let mut r = valid_receipt(&m);
        r.verified_at = 0;
        assert!(matches!(
            r.validate(),
            Err(ForecastReceiptValidationError::InvalidVerifiedAt)
        ));
    }

    #[test]
    fn receipt_rejects_invalid_metrics() {
        let m = valid_manifest();
        let mut r = valid_receipt(&m);
        r.metrics.skill_score = 2.0;
        assert!(matches!(
            r.validate(),
            Err(ForecastReceiptValidationError::InvalidMetrics(_))
        ));
    }

    // --- SelectiveDisclosureProofPayload tests ---

    /// Helper: deterministic ed25519 keypair for tests.
    fn test_verifier_keypair() -> (ed25519_dalek::SigningKey, [u8; 32]) {
        let secret = [42u8; 32];
        let sk = ed25519_dalek::SigningKey::from_bytes(&secret);
        let vk_bytes: [u8; 32] = ed25519_dalek::VerifyingKey::from(&sk).to_bytes();
        (sk, vk_bytes)
    }

    /// Helper: build a proof and sign its binding digest with the test keypair.
    fn signed_proof(
        disclosure_id: &str,
        scope: Vec<String>,
        expires_at_block: u64,
        disclosed_fields_hash: &str,
    ) -> (SelectiveDisclosureProofPayload, [u8; 32]) {
        use ed25519_dalek::Signer;
        let (sk, vk_bytes) = test_verifier_keypair();
        // Build a temporary proof to compute the binding digest.
        let mut proof = SelectiveDisclosureProofPayload {
            disclosure_id: disclosure_id.to_string(),
            scope,
            expires_at_block,
            disclosed_fields_hash: disclosed_fields_hash.to_string(),
            verifier_attestation: String::new(),
        };
        let digest = proof.binding_digest();
        let sig = sk.sign(digest.as_bytes());
        proof.verifier_attestation = hex::encode(sig.to_bytes());
        (proof, vk_bytes)
    }

    #[test]
    fn binding_digest_deterministic() {
        let (proof, _vk) = signed_proof("d1", vec!["jurisdiction".into()], 1000, "abc");
        let d1 = proof.binding_digest();
        let d2 = proof.binding_digest();
        assert_eq!(d1, d2, "binding_digest must be deterministic");
    }

    #[test]
    fn compute_disclosed_fields_hash_sorted() {
        let h1 = SelectiveDisclosureProofPayload::compute_disclosed_fields_hash(&[
            ("jurisdiction", "US"),
            ("sanction_status", "clear"),
        ]);
        let h2 = SelectiveDisclosureProofPayload::compute_disclosed_fields_hash(&[
            ("sanction_status", "clear"),
            ("jurisdiction", "US"),
        ]);
        assert_eq!(h1, h2, "field order must not affect hash");
    }

    #[test]
    fn is_expired_boundary() {
        let (proof, _vk) = signed_proof("d1", vec!["a".into()], 100, "h");
        assert!(!proof.is_expired(99), "should not be expired before block");
        assert!(proof.is_expired(100), "should be expired at exact block");
        assert!(proof.is_expired(101), "should be expired after block");
    }

    #[test]
    fn has_valid_attestation_rejects_short() {
        let (_sk, vk_bytes) = test_verifier_keypair();
        let proof = SelectiveDisclosureProofPayload {
            disclosure_id: "d1".to_string(),
            scope: vec!["a".to_string()],
            expires_at_block: 100,
            disclosed_fields_hash: "h".to_string(),
            verifier_attestation: "aabb".to_string(), // only 2 bytes
        };
        assert!(!proof.has_valid_attestation(&vk_bytes));
    }

    #[test]
    fn has_valid_attestation_rejects_non_hex() {
        let (_sk, vk_bytes) = test_verifier_keypair();
        let proof = SelectiveDisclosureProofPayload {
            disclosure_id: "d1".to_string(),
            scope: vec!["a".to_string()],
            expires_at_block: 100,
            disclosed_fields_hash: "h".to_string(),
            verifier_attestation: "not-hex-at-all".to_string(),
        };
        assert!(!proof.has_valid_attestation(&vk_bytes));
    }

    #[test]
    fn has_valid_attestation_rejects_wrong_key() {
        let (proof, _vk) = signed_proof("d1", vec!["a".into()], 100, "h");
        let wrong_key = [99u8; 32];
        // The proof was signed with the test keypair, not this key.
        assert!(!proof.has_valid_attestation(&wrong_key));
    }

    #[test]
    fn has_valid_attestation_rejects_tampered_payload() {
        let (mut proof, vk_bytes) = signed_proof("d1", vec!["a".into()], 100, "h");
        // Change disclosure_id after signing — digest changes.
        proof.disclosure_id = "tampered".to_string();
        assert!(!proof.has_valid_attestation(&vk_bytes));
    }

    #[test]
    fn selective_disclosure_digest_binds_chain_identity_2121() {
        let (proof, _) = signed_proof("d1", vec!["a".into()], 100, "h");
        let main = selective_disclosure_authorization_digest_with_domain(
            "wrfcoin-main",
            "mainnet",
            &proof,
        );
        let other_chain = selective_disclosure_authorization_digest_with_domain(
            "wrfcoin-test",
            "mainnet",
            &proof,
        );
        let other_network = selective_disclosure_authorization_digest_with_domain(
            "wrfcoin-main",
            "testnet",
            &proof,
        );
        assert_ne!(main, other_chain, "chain_id must be inside the signed preimage");
        assert_ne!(
            main, other_network,
            "network_id must be inside the signed preimage"
        );
    }

    #[test]
    fn confidential_transfer_preimage_binds_chain_and_payload_fields_2121() {
        let payload = ConfidentialTransferPayload {
            output_commitments: vec!["11".repeat(32)],
            range_proof: Some("22".repeat(32)),
            stealth_data: Some("33".repeat(32)),
            ring_signature: None,
            key_images: vec!["44".repeat(32)],
            encrypted_payload: Some("55".repeat(32)),
            privacy_level: PrivacyLevel::High,
            selective_disclosures: Vec::new(),
            signed_message: None,
        };
        let main = confidential_transfer_authorization_message_with_domain(
            "wrfcoin-main",
            "mainnet",
            &payload,
        );
        let other_chain = confidential_transfer_authorization_message_with_domain(
            "wrfcoin-test",
            "mainnet",
            &payload,
        );
        let mut changed = payload.clone();
        changed.output_commitments = vec!["66".repeat(32)];
        let changed_commitment = confidential_transfer_authorization_message_with_domain(
            "wrfcoin-main",
            "mainnet",
            &changed,
        );
        assert_ne!(main, other_chain, "chain_id must be inside the signed preimage");
        assert_ne!(
            main, changed_commitment,
            "output commitments must be inside the signed preimage"
        );
    }

    #[test]
    fn has_valid_attestation_accepts_valid_sig() {
        let (proof, vk_bytes) = signed_proof("d1", vec!["a".into()], 100, "h");
        assert!(proof.has_valid_attestation(&vk_bytes));
    }

    #[test]
    fn verify_rejects_empty_scope() {
        let (proof, vk_bytes) = signed_proof("d1", vec![], 100, "h");
        assert!(!proof.verify(50, &vk_bytes));
    }

    #[test]
    fn verify_rejects_empty_disclosure_id() {
        let (proof, vk_bytes) = signed_proof("", vec!["a".into()], 100, "h");
        assert!(!proof.verify(50, &vk_bytes));
    }

    #[test]
    fn selective_disclosure_generate_and_verify() {
        use ed25519_dalek::Signer;
        let (sk, vk_bytes) = test_verifier_keypair();
        // Build the proof without attestation first to get the binding digest.
        let mut proof = SelectiveDisclosureProofPayload::generate(
            "d1".to_string(),
            vec!["jurisdiction".to_string(), "sanction_status".to_string()],
            1000,
            &[("jurisdiction", "US"), ("sanction_status", "clear")],
            String::new(), // placeholder
        );
        let digest = proof.binding_digest();
        let sig = sk.sign(digest.as_bytes());
        proof.verifier_attestation = hex::encode(sig.to_bytes());

        assert!(
            proof.verify(999, &vk_bytes),
            "valid generated proof should pass verification"
        );
        assert!(
            !proof.verify(1000, &vk_bytes),
            "expired proof should fail verification"
        );
    }

    #[test]
    fn reveal_payload_backward_compatible_deserialization() {
        let json = r#"{
            "commit_id": "abc",
            "station_id": "s1",
            "timestamp": 100,
            "contribution_type": "forecast",
            "data": {},
            "nonce": "n1"
        }"#;
        let payload: RevealPayload = serde_json::from_str(json).unwrap();
        assert!(payload.manifest.is_none());
        assert!(payload.verification_receipt.is_none());
    }

    #[test]
    fn calculate_hash_is_deterministic() {
        use chrono::TimeZone;
        let ts = chrono::Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
        let mut tx = Transaction::new_coinbase("0xabc", 50, 1);
        tx.timestamp = ts;

        let h1 = tx.calculate_hash();
        let h2 = tx.calculate_hash();
        assert_eq!(h1, h2, "same transaction must always produce the same hash");

        // Changing the fee must change the hash.
        let mut tx2 = tx.clone();
        tx2.fee = 1;
        assert_ne!(
            tx.calculate_hash(),
            tx2.calculate_hash(),
            "different fee must produce different hash"
        );
    }

    #[test]
    fn calculate_hash_survives_bincode_round_trip() {
        use chrono::TimeZone;
        let ts = chrono::Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
        let mut tx = Transaction::new_coinbase("0xabc", 50, 1);
        tx.timestamp = ts;

        let expected = tx.calculate_hash();
        let bytes = bincode::serialize(&tx).expect("tx should serialize");
        let round_tripped: Transaction =
            bincode::deserialize(&bytes).expect("tx should deserialize");
        assert_eq!(round_tripped.calculate_hash(), expected);
    }

    // ── SlashingReport payload round-trip (#1393) ──────────────────────
    //
    // The evidence-submitter task wraps `SlashingReportPayload` in
    // `Transaction::data` as JSON. The apply path parses it back via
    // `parse_slashing_report_payload`. Both sides must agree on the wire
    // format; a round-trip test fails loudly if either side drifts.
    #[test]
    fn slashing_report_payload_round_trip() {
        use crate::security::slashing::{SlashingEvidenceType, SlashingReportPayload};

        let payload = SlashingReportPayload {
            validator_id: "validator-alice".to_string(),
            evidence_type: SlashingEvidenceType::DoubleSign,
            evidence_hash: [0xAB; 32],
            reported_at_height: 42_000,
            penalty_amount: 1_500_000_000,
            reporter_pubkey: "0xdeadbeef".to_string(),
            reporter_signature: Some("0xbeef".to_string()),
        };
        let data = serde_json::to_string(&payload).expect("serialize payload");

        let mut tx = Transaction::new(Vec::new(), Vec::new());
        tx.tx_type = TransactionType::SlashingReport;
        tx.data = Some(data);

        let parsed = tx
            .parse_slashing_report_payload()
            .expect("slashing report payload should parse");
        assert_eq!(parsed, payload);

        // Wrong tx_type must return None even with a well-formed data body.
        let mut wrong_type = tx.clone();
        wrong_type.tx_type = TransactionType::Regular;
        assert!(wrong_type.parse_slashing_report_payload().is_none());
    }

    #[test]
    fn slashing_report_tx_type_display_is_stable() {
        assert_eq!(TransactionType::SlashingReport.to_string(), "SlashingReport");
    }

    #[test]
    fn transparent_transfer_authorization_binds_to_from_account() {
        // #1727: a `from`-key signature authorizes; a wrong key, wrong nonce,
        // tampered amount, or a `from` mismatch all fail closed.
        let mut rng = StdRng::seed_from_u64(1_727);
        let from_key = PrivateKey::generate(&mut rng);
        let from = Address::from_public_key(&PublicKey::from_private_key(&from_key)).to_hex();
        let to = Address::new([0x22u8; 20]).to_hex();

        let mut payload = TransparentTransferPayload {
            from: from.clone(),
            to: to.clone(),
            amount: 100,
            transparent_opt_out: true,
            opt_out_audit_tag: None,
            auth_pubkey: None,
            auth_signature: None,
            authority: None,
            authority_signature: None,
            passkey_auth: None,
        };
        // Unsigned → not authorized.
        assert!(!payload.verify_authorization(0));

        payload.sign_authorization(&from_key, 0);
        assert!(payload.verify_authorization(0), "valid from-key signature authorizes");

        // Nonce is bound: same signature does not authorize a different nonce.
        assert!(!payload.verify_authorization(1), "authorization is nonce-bound");

        // Amount is bound: tampering invalidates the signature.
        let mut tampered = payload.clone();
        tampered.amount = 999;
        assert!(!tampered.verify_authorization(0), "amount is covered by the signature");

        // Foreign key (does not derive to `from`) → rejected even though the
        // signature is internally well-formed.
        let attacker = PrivateKey::generate(&mut rng);
        let mut forged = TransparentTransferPayload {
            from: from.clone(),
            to,
            amount: 100,
            transparent_opt_out: true,
            opt_out_audit_tag: None,
            auth_pubkey: None,
            auth_signature: None,
            authority: None,
            authority_signature: None,
            passkey_auth: None,
        };
        forged.sign_authorization(&attacker, 0);
        assert!(
            !forged.verify_authorization(0),
            "a signature from a non-`from` key must not authorize the debit"
        );
    }

    /// Build a minimal transparent `Transfer` carrying `payload` at `nonce`.
    fn system_transfer_tx(payload: &TransparentTransferPayload, nonce: u64) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_bytes(b"sys-transfer-input"),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: payload.amount,
                address: payload.to.clone(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::Transfer;
        tx.nonce = nonce;
        tx.data = Some(serde_json::to_string(payload).unwrap());
        tx.set_id();
        tx
    }

    /// Register `public_key_hex` as `identity`'s active GovernanceOperator key
    /// (epoch 0, activated at time 0) in a fresh registry.
    fn governance_registry(identity: &str, public_key_hex: &str) -> SigningKeyRegistry {
        let mut registry = SigningKeyRegistry::default();
        registry
            .register_key_epoch(
                identity,
                SigningRole::GovernanceOperator,
                0,
                public_key_hex.to_string(),
                0,
            )
            .expect("register governance operator key");
        registry
    }

    fn unsigned_system_transfer_payload() -> TransparentTransferPayload {
        TransparentTransferPayload {
            from: Address::new([0x11u8; 20]).to_hex(),
            to: Address::new([0x22u8; 20]).to_hex(),
            amount: 500,
            transparent_opt_out: true,
            opt_out_audit_tag: None,
            auth_pubkey: None,
            auth_signature: None,
            authority: None,
            authority_signature: None,
            passkey_auth: None,
        }
    }

    #[test]
    fn system_transfer_provenance_authorizes_only_registered_governance_key() {
        // #1727: a transparent Transfer that claims governance provenance is
        // authorized iff `authority_signature` verifies against the authority's
        // registered GovernanceOperator key over from|to|amount|nonce.
        let mut rng = StdRng::seed_from_u64(1_727_001);
        let gov_key = PrivateKey::generate(&mut rng);
        let gov_pubkey = PublicKey::from_private_key(&gov_key).to_hex();
        let identity = "gov-treasury";
        let registry = governance_registry(identity, &gov_pubkey);

        let payload = unsigned_system_transfer_payload();
        let nonce = 7;

        // No provenance attached → claims nothing, verifies false.
        let bare = system_transfer_tx(&payload, nonce);
        assert!(!bare.claims_system_authority());
        assert!(!bare.verify_system_authority(&registry));

        // Stamp valid provenance with the registered governance key.
        let mut signed = system_transfer_tx(&payload, nonce);
        signed.sign_system_authority(identity, &gov_key).expect("sign provenance");
        assert!(signed.claims_system_authority());
        assert!(
            signed.verify_system_authority(&registry),
            "registered governance key authorizes"
        );

        // A signature from a different (registered-identity-but-wrong) key fails.
        let other_key = PrivateKey::generate(&mut rng);
        let mut wrong = system_transfer_tx(&payload, nonce);
        wrong.sign_system_authority(identity, &other_key).expect("sign");
        assert!(
            !wrong.verify_system_authority(&registry),
            "a signature not from the registered governance key is rejected"
        );

        // Tamper amount after signing → message no longer matches → rejected.
        let mut tampered = signed.clone();
        let mut tp = tampered.parse_transparent_transfer_payload().unwrap();
        tp.amount = 999;
        tampered.data = Some(serde_json::to_string(&tp).unwrap());
        tampered.set_id();
        assert!(
            !tampered.verify_system_authority(&registry),
            "amount is bound by the provenance signature"
        );
    }

    #[test]
    fn system_transfer_provenance_rejects_unregistered_self_certified_authority() {
        // #1727 / #1723: the legacy identity-as-public-key fallback in
        // `verify_signature_for_identity_role` must NOT let an attacker
        // self-certify by naming their own public key as `authority` and signing
        // with the matching key. Only an *explicitly registered*
        // GovernanceOperator may authorize.
        let mut rng = StdRng::seed_from_u64(1_727_002);
        let attacker_key = PrivateKey::generate(&mut rng);
        let attacker_pubkey = PublicKey::from_private_key(&attacker_key).to_hex();

        let payload = unsigned_system_transfer_payload();
        // Attacker names their own public key as the authority identity and signs
        // with the matching key — internally well-formed but self-certifying.
        let mut forged = system_transfer_tx(&payload, 3);
        forged.sign_system_authority(&attacker_pubkey, &attacker_key).expect("sign");
        assert!(forged.claims_system_authority());

        // Empty registry: `authority` is not a registered GovernanceOperator, so
        // the self-certified signature is rejected despite being internally valid.
        let empty = SigningKeyRegistry::default();
        assert!(
            !forged.verify_system_authority(&empty),
            "self-certified unregistered authority must be rejected (#1723 fallback closed)"
        );
    }

    #[test]
    fn stake_authorization_binds_to_account_action_and_nonce() {
        // #1763: an `account`-key signature authorizes the stake mutation; a
        // wrong key, wrong nonce, tampered amount, mismatched action, or an
        // `account` mismatch all fail closed.
        let mut rng = StdRng::seed_from_u64(1_763);
        let account_key = PrivateKey::generate(&mut rng);
        let account =
            Address::from_public_key(&PublicKey::from_private_key(&account_key)).to_hex();

        let mut payload = StakeAuthPayload {
            account: account.clone(),
            amount: 2_000,
            auth_pubkey: None,
            auth_signature: None,
        };
        // Unsigned → not authorized.
        assert!(!payload.verify_authorization(false, 0));

        // Sign for Stake (is_unstake = false) at nonce 0.
        payload.sign_authorization(&account_key, false, 0);
        assert!(
            payload.verify_authorization(false, 0),
            "valid account-key signature authorizes the stake"
        );

        // Action is bound: the Stake signature does not authorize an Unstake.
        assert!(
            !payload.verify_authorization(true, 0),
            "authorization is action-bound (Stake sig must not authorize Unstake)"
        );

        // Nonce is bound: same signature does not authorize a different nonce.
        assert!(!payload.verify_authorization(false, 1), "authorization is nonce-bound");

        // Amount is bound: tampering invalidates the signature.
        let mut tampered = payload.clone();
        tampered.amount = 999_999;
        assert!(
            !tampered.verify_authorization(false, 0),
            "amount is covered by the signature"
        );

        // Foreign key (does not derive to `account`) → rejected even though the
        // signature is internally well-formed.
        let attacker = PrivateKey::generate(&mut rng);
        let mut forged = StakeAuthPayload {
            account,
            amount: 2_000,
            auth_pubkey: None,
            auth_signature: None,
        };
        forged.sign_authorization(&attacker, false, 0);
        assert!(
            !forged.verify_authorization(false, 0),
            "a signature from a non-`account` key must not authorize the stake"
        );
    }

    #[test]
    fn oracle_attestation_sign_verify_round_trip() {
        // Proves the signing recipe used by the `oracle_attest_sign` tool:
        // sign the payload's canonical_signing_message() with the oracle key,
        // set it as the signature, and has_valid_signature() must accept it.
        // Guards against drift between the attestation tooling and the verifier.
        use crate::crypto::signature::{PrivateKey, PublicKey};
        let sk = PrivateKey::generate(&mut rand::thread_rng());
        let oracle_id = PublicKey::from_private_key(&sk).to_hex();
        let mut p = OracleAttestationPayload {
            commit_id: "commit-abc".to_string(),
            oracle_id: oracle_id.clone(),
            approved: true,
            signature: None,
            confidence: Some(0.9),
            timestamp: 1_700_000_000,
            nonce: Some("7".to_string()),
        };
        p.signature = Some(sk.sign(p.canonical_signing_message().as_bytes()).to_hex());
        assert!(p.has_valid_signature(), "freshly-signed attestation must verify");

        // Any tamper to a signed field invalidates it.
        let mut tampered = p.clone();
        tampered.approved = false;
        assert!(!tampered.has_valid_signature(), "tampered approved must not verify");
        let mut wrong_commit = p.clone();
        wrong_commit.commit_id = "commit-xyz".to_string();
        assert!(!wrong_commit.has_valid_signature(), "tampered commit_id must not verify");
    }

    #[test]
    fn oracle_attestation_non_finite_confidence_is_rejected_without_panic() {
        // Iris crypto audit 2026-06-20: a non-finite `confidence` reaches the
        // verifier over the bincode gossip path. It must verify as invalid,
        // never panic the (now-loud) digest step.
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let attestation = OracleAttestationPayload {
                commit_id: "commit-1".to_string(),
                oracle_id: "0xdeadbeef".to_string(),
                approved: true,
                signature: Some("00".repeat(64)),
                confidence: Some(bad),
                timestamp: 1_700_000_000,
                nonce: Some("n1".to_string()),
            };
            assert!(!attestation.is_signable(), "{bad} must be unsignable");
            assert!(
                !attestation.has_valid_signature(),
                "{bad} confidence must verify as invalid, not panic"
            );
        }

        // A finite confidence remains signable and computes a digest cleanly.
        let ok = OracleAttestationPayload {
            commit_id: "commit-1".to_string(),
            oracle_id: "0xdeadbeef".to_string(),
            approved: true,
            signature: None,
            confidence: Some(0.87),
            timestamp: 1_700_000_000,
            nonce: Some("n1".to_string()),
        };
        assert!(ok.is_signable());
        let _ = ok.signing_message(); // must not panic
    }

    // ── mobile#215: gamification payout tx wire format ──────────────────────

    #[test]
    fn gamification_reward_roundtrips_and_dedups_by_outpoint() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(215);
        let op = PrivateKey::generate(&mut rng);
        let recipient = Address([9u8; 20]).to_hex();
        let a = Transaction::new_gamification_reward(&recipient, 500, "claim-x", &op);
        let b = Transaction::new_gamification_reward(&recipient, 500, "claim-x", &op);

        // The claim-derived outpoint is deterministic, so a re-submission of the
        // same claim collides in the mempool (double-spend reject) regardless of
        // the wall-clock timestamp that feeds the tx hash.
        assert_eq!(a.inputs[0].txid, b.inputs[0].txid, "same claim → same outpoint");
        assert_eq!(a.tx_type, TransactionType::Reward);
        assert_eq!(a.outputs[0].amount, 500);
        assert!(!a.is_coinbase(), "payout must not be classified as coinbase");

        // Round-trips through the parser used by every reconstruction path.
        let claim = a.parse_gamification_claim().expect("parses as gamification claim");
        assert_eq!(claim.claim_id, "claim-x");
        assert_eq!(claim.reward_type, GamificationClaimPayload::REWARD_TYPE);

        // Distinct claims occupy distinct outpoints.
        let c = Transaction::new_gamification_reward(&recipient, 500, "claim-y", &op);
        assert_ne!(a.inputs[0].txid, c.inputs[0].txid);
    }

    /// core4#2081 review, blocker 1: a gamification payout must carry an
    /// operator authorization bound to the tx, and it must fail closed against
    /// forgery, tampering, and unlisted operators.
    ///
    /// The allowlist is process env; no other test reads this variable, and it
    /// is restored on exit.
    #[test]
    fn gamification_payout_authorization_is_bound_and_fail_closed() {
        use crate::crypto::domain_signing::{
            GAMIFICATION_ENV_TEST_LOCK, GAMIFICATION_OPERATOR_PUBKEYS_ENV,
        };
        let _env = GAMIFICATION_ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let mut rng = rand::rngs::StdRng::seed_from_u64(2081);
        let operator = PrivateKey::generate(&mut rng);
        let attacker = PrivateKey::generate(&mut rng);
        let op_pk = PublicKey::from_private_key(&operator).to_hex();
        let recipient = Address([7u8; 20]).to_hex();

        let prev = std::env::var(GAMIFICATION_OPERATOR_PUBKEYS_ENV).ok();
        std::env::set_var(GAMIFICATION_OPERATOR_PUBKEYS_ENV, &op_pk);

        // Authorized by an allowlisted operator → valid.
        let good = Transaction::new_gamification_reward(&recipient, 500, "claim-x", &operator);
        assert!(good.verify_gamification_authorization(), "allowlisted operator authorizes");

        // Unsigned payload → fail closed.
        let mut unsigned = good.clone();
        unsigned.data = serde_json::to_string(&GamificationClaimPayload::new("claim-x")).ok();
        unsigned.set_id();
        assert!(!unsigned.verify_gamification_authorization(), "unsigned must not authorize");

        // Operator not on the allowlist → rejected even with a valid signature.
        let unlisted =
            Transaction::new_gamification_reward(&recipient, 500, "claim-x", &attacker);
        assert!(
            !unlisted.verify_gamification_authorization(),
            "a valid signature from a non-allowlisted key is not an authorization"
        );

        // Amount tamper: keep the operator signature but inflate the output.
        let mut inflated = good.clone();
        inflated.outputs[0].amount = 10_000;
        inflated.set_id();
        assert!(
            !inflated.verify_gamification_authorization(),
            "amount is covered by the signature — inflating the payout breaks it"
        );

        // Recipient swap: redirect the payout, keep the signature.
        let mut redirected = good.clone();
        redirected.outputs[0].address = Address([0x66; 20]).to_hex();
        redirected.set_id();
        assert!(
            !redirected.verify_gamification_authorization(),
            "recipient is covered by the signature — redirecting the payout breaks it"
        );

        // Empty allowlist authorizes no one (fail-closed default).
        std::env::remove_var(GAMIFICATION_OPERATOR_PUBKEYS_ENV);
        assert!(
            !good.verify_gamification_authorization(),
            "with no configured operators, every payout fails authorization"
        );

        // Restore.
        match prev {
            Some(v) => std::env::set_var(GAMIFICATION_OPERATOR_PUBKEYS_ENV, v),
            None => std::env::remove_var(GAMIFICATION_OPERATOR_PUBKEYS_ENV),
        }
    }

    /// #2055 Option A: an oracle-registration tx must carry a governance
    /// authorization bound to every registration field, fail-closed against
    /// forgery, tampering, and unlisted keys.
    #[test]
    fn oracle_registration_authorization_is_bound_and_fail_closed() {
        use crate::crypto::domain_signing::{
            ORACLE_GOVERNANCE_ENV_TEST_LOCK, ORACLE_GOVERNANCE_PUBKEYS_ENV,
        };
        let _env = ORACLE_GOVERNANCE_ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let mut rng = rand::rngs::StdRng::seed_from_u64(2055);
        let gov = PrivateKey::generate(&mut rng);
        let attacker = PrivateKey::generate(&mut rng);
        let gov_pk = PublicKey::from_private_key(&gov).to_hex();
        let identity = "0xoracle-1".to_string();

        let prev = std::env::var(ORACLE_GOVERNANCE_PUBKEYS_ENV).ok();
        std::env::set_var(ORACLE_GOVERNANCE_PUBKEYS_ENV, &gov_pk);

        let mk = |signer: &PrivateKey| {
            Transaction::new_oracle_registration(
                vec![identity.clone()],
                1,
                &identity,
                "aa".repeat(32).as_str(),
                1,
                1_700_000_000,
                signer,
            )
        };

        // Authorized by an allowlisted governance key → valid.
        let good = mk(&gov);
        assert!(good.verify_oracle_registration_authorization(), "allowlisted gov key authorizes");

        // Unsigned payload → fail closed.
        let mut unsigned = good.clone();
        let mut p = unsigned.parse_oracle_registration().unwrap();
        p.auth_pubkey = None;
        p.auth_signature = None;
        unsigned.data = serde_json::to_string(&p).ok();
        unsigned.set_id();
        assert!(!unsigned.verify_oracle_registration_authorization(), "unsigned must not authorize");

        // Non-allowlisted signer → rejected despite a valid signature.
        assert!(
            !mk(&attacker).verify_oracle_registration_authorization(),
            "a valid signature from a non-allowlisted key is not an authorization"
        );

        // Field tampering — each must break the signature (bound fields):
        // identity swap.
        let mut t = good.clone();
        let mut p = t.parse_oracle_registration().unwrap();
        p.identity = "0xattacker".to_string();
        t.data = serde_json::to_string(&p).ok();
        t.set_id();
        assert!(!t.verify_oracle_registration_authorization(), "identity is signature-bound");

        // public_key swap (key-redirect).
        let mut t = good.clone();
        let mut p = t.parse_oracle_registration().unwrap();
        p.public_key = "bb".repeat(32);
        t.data = serde_json::to_string(&p).ok();
        t.set_id();
        assert!(!t.verify_oracle_registration_authorization(), "public_key is signature-bound");

        // widen the oracle set.
        let mut t = good.clone();
        let mut p = t.parse_oracle_registration().unwrap();
        p.oracle_set.push("0xattacker".to_string());
        t.data = serde_json::to_string(&p).ok();
        t.set_id();
        assert!(!t.verify_oracle_registration_authorization(), "oracle_set is signature-bound");

        // lower the quorum.
        let mut t = good.clone();
        let mut p = t.parse_oracle_registration().unwrap();
        p.oracle_quorum = 0;
        t.data = serde_json::to_string(&p).ok();
        t.set_id();
        assert!(!t.verify_oracle_registration_authorization(), "oracle_quorum is signature-bound");

        // Empty allowlist authorizes no one (fail-closed default).
        std::env::remove_var(ORACLE_GOVERNANCE_PUBKEYS_ENV);
        assert!(
            !good.verify_oracle_registration_authorization(),
            "with no configured governance keys, every registration fails authorization"
        );

        match prev {
            Some(v) => std::env::set_var(ORACLE_GOVERNANCE_PUBKEYS_ENV, v),
            None => std::env::remove_var(ORACLE_GOVERNANCE_PUBKEYS_ENV),
        }
    }

    #[test]
    fn plain_reward_is_not_a_gamification_claim() {
        // A role/mining Reward tx carrying a different `data` shape must not be
        // mistaken for a gamification payout.
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: Hash::default(),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 100,
                address: Address([1u8; 20]).to_hex(),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::Reward;
        tx.data = Some("{\"reward_type\":\"role_reward\",\"role\":\"Collector\"}".to_string());
        tx.set_id();
        assert!(tx.parse_gamification_claim().is_none());

        // A non-Reward type with a matching payload is also rejected.
        let mut spoof = tx.clone();
        spoof.tx_type = TransactionType::Regular;
        spoof.data = serde_json::to_string(&GamificationClaimPayload::new("x")).ok();
        spoof.set_id();
        assert!(spoof.parse_gamification_claim().is_none());
    }

    #[test]
    fn station_registration_float_preimage_is_canonical_and_bit_deterministic() {
        let base = StationRegistrationPayload::new(
            "station-determinism",
            "owner-determinism",
            0.0,
            -0.0,
            "fixture",
            DataQualityTier::Gold,
            ContributionProvenanceClass::Synthetic,
        );
        assert!(base.validate().is_ok());

        // Signed zero has two IEEE-754 encodings, but the canonical preimage
        // deliberately gives them one protocol representation.
        let mut signed_zero_variant = base.clone();
        signed_zero_variant.latitude = -0.0;
        signed_zero_variant.longitude = 0.0;
        assert_eq!(
            station_registration_authorization_message_with_domain(
                TESTNET_CID,
                TESTNET_NID,
                &base,
            ),
            station_registration_authorization_message_with_domain(
                TESTNET_CID,
                TESTNET_NID,
                &signed_zero_variant,
            ),
            "signed zero must have one canonical station-registration preimage"
        );

        let mut changed = base.clone();
        changed.latitude = 1.0;
        assert_ne!(
            station_registration_authorization_message_with_domain(
                TESTNET_CID,
                TESTNET_NID,
                &base,
            ),
            station_registration_authorization_message_with_domain(
                TESTNET_CID,
                TESTNET_NID,
                &changed,
            ),
            "a coordinate change must remain visible in the signed preimage"
        );
    }
}
