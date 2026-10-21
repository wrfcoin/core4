//! #3345 / #3347 — owner-consent station transfer, owner-signed station
//! actions (report stolen / unfreeze / deactivate / reactivate), the
//! time-locked owner-key recovery, and the per-station OWNER LEDGER they all
//! share (owner rulings 2026-09-27, infra#1095).
//!
//! Designs: `docs/architecture/station-ownership-transfer.md` §3.2 (transfer),
//! §3.3 (ledger), §3.4 (recovery); `docs/architecture/station-freeze-and-deactivation.md`
//! §3.2 (actions), §3.4 (freeze suspends penalties), §3.5 (recovery lifts a
//! freeze).
//!
//! Two halves:
//!
//! - STATELESS (payloads, signed preimages, signature / key-binding /
//!   validity-window checks) — run once, in the ONE shared extractor
//!   [`Transaction::station_owner_ledger_transition`] that the PoP producer
//!   arm and the P2P / WAL applier (`transition_extractor`) both call, so
//!   producer == applier == WAL replay by construction.
//! - STATEFUL (nonce match, registered owner, frozen / pending-recovery
//!   preconditions) — applied by `State` against
//!   [`StationOwnerLedgerEntry`] (`State::station_owner_ledger`). Every
//!   stateful refusal is an INERT, deterministic no-op, never a
//!   block-failing error (a failing transition fails the whole block, the
//!   #2537 reason `upsert` exists).
//!
//! Every mechanism here is behind an UNSCHEDULED feature: transfer and
//! recovery behind `station_owner_consent`, the owner actions behind
//! `station_owner_actions`. Below activation the extractor emits nothing for
//! these `tx_kind`s and the appended transitions are refused, so the ledger
//! stays empty and — because it is folded into `state_root` only when
//! non-empty — every root is byte-identical to main.
//!
//! Encoding rule for every preimage here: explicit `u32le` length prefixes for
//! strings, fixed-width little-endian integers, the chain identity folded in
//! through the same `push_chain_domain` every other station preimage uses. No
//! floats, no bincode (#3163), no wall-clock time — windows are block heights,
//! and every height sum saturates.

use serde::{Deserialize, Serialize};

use crate::blockchain::state::same_station_owner;
use crate::blockchain::state::{State, StateTransition};
use crate::blockchain::transaction::{
    STATION_OWNER_TRANSFER_ACCEPT_DOMAIN, STATION_OWNER_TRANSFER_DOMAIN, StationOwnerActionKind,
    Transaction, TransactionType, owner_address_bound_to_pubkey, push_chain_domain,
    station_owner_action_message_with_domain, station_owner_transfer_message_with_domain,
};
use crate::crypto::chain_binding::{ChainIdentity, ConsensusFeature, feature_active};
use crate::crypto::domain_signing::is_authorized_oracle_governance;
use crate::crypto::signature::{PrivateKey, PublicKey, Signature};
use crate::station_registry::WeatherStation;

/// Longest a signed-but-unsubmitted transfer may stay valid, in blocks
/// (≈4 days at the observed ~1.7 blocks/min). A transfer is applicable only
/// while `block_height <= expires_at_height <= block_height + this`.
pub const STATION_OWNER_TRANSFER_MAX_VALIDITY_BLOCKS: u64 = 10_000;

/// #3345 Q1 / #3347 Q1 (owner ruling 2026-09-27): the public notice window of
/// a governance-filed owner recovery, in blocks (≈14 days at ~1.7 blocks/min).
/// A recovery filed at height `F` is executable from `F + this` and not
/// before; until then the CURRENT owner key can cancel it.
pub const STATION_OWNER_RECOVERY_NOTICE_BLOCKS: u64 = 34_000;

/// Governance's filing of a time-locked owner recovery (#3345 Q1).
pub const STATION_OWNER_RECOVERY_FILE_DOMAIN: &[u8] = b"wrfcoin_station_owner_recovery_file_v1";

/// The recovery's NEW owner accepting it (same body as the filing, different
/// tag) — the #3345 Q4 countersignature rationale applied to recovery.
pub const STATION_OWNER_RECOVERY_ACCEPT_DOMAIN: &[u8] = b"wrfcoin_station_owner_recovery_accept_v1";

/// The CURRENT owner cancelling a pending recovery (#3345 Q1).
pub const STATION_OWNER_RECOVERY_CANCEL_DOMAIN: &[u8] = b"wrfcoin_station_owner_recovery_cancel_v1";

fn push_str(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
}

fn verify_hex_signature(pubkey_hex: &str, signature_hex: Option<&str>, message: &[u8]) -> bool {
    let Some(signature_hex) = signature_hex else {
        return false;
    };
    let (Ok(pk), Ok(sig)) = (
        PublicKey::from_hex(pubkey_hex),
        Signature::from_hex(signature_hex),
    ) else {
        return false;
    };
    pk.verify(message, &sig)
}

// ─── Transfer (#3345 §3.2; Q4 countersignature, Q5 record preserved) ───────

/// Why a transfer failed its stateless checks. Every variant is an inert
/// refusal; none is a block-failing error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationOwnerTransferRefusal {
    /// Wrong `tx_kind`, or a malformed field.
    Malformed,
    /// `to_owner_address` names the owner already named by
    /// `from_owner_address`.
    SameOwner,
    /// `from_owner_pubkey` does not derive to `from_owner_address`.
    FromKeyNotBound,
    /// `to_owner_pubkey` does not derive to `to_owner_address`.
    ToKeyNotBound,
    /// The current owner's signature is missing or does not verify.
    MissingOrBadOwnerSignature,
    /// #3345 Q4: the new owner's countersignature is missing or does not verify.
    MissingOrBadCountersignature,
    /// `block_height > expires_at_height`.
    Expired,
    /// `expires_at_height > block_height +
    /// STATION_OWNER_TRANSFER_MAX_VALIDITY_BLOCKS`.
    ValidityTooLong,
}

/// An owner-signed, new-owner-countersigned station transfer (#3345), carried
/// in a `TransactionType::StationRegistration` tx's `data` with
/// `tx_kind = "station_owner_transfer_v1"`. There is no governance signature:
/// the owner, not governance, moves the station.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StationOwnerTransferPayload {
    /// Must equal [`StationOwnerTransferPayload::TX_KIND`].
    pub tx_kind: String,
    pub station_id: String,
    /// The owner registered NOW (pinned into both signed preimages).
    pub from_owner_address: String,
    /// Hex ed25519; must derive (`PublicKey::to_address`, #2742) to
    /// `from_owner_address`.
    pub from_owner_pubkey: String,
    pub to_owner_address: String,
    /// Hex ed25519; must derive to `to_owner_address`.
    pub to_owner_pubkey: String,
    /// Must equal the station's next owner nonce in the owner ledger (stateful;
    /// not checked here).
    pub transfer_nonce: u64,
    /// Last height at which the transfer may be applied.
    pub expires_at_height: u64,
    /// Current owner, over the preimage under
    /// [`STATION_OWNER_TRANSFER_DOMAIN`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_owner_signature: Option<String>,
    /// New owner (#3345 Q4), over the SAME body under
    /// [`STATION_OWNER_TRANSFER_ACCEPT_DOMAIN`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_owner_signature: Option<String>,
}

impl StationOwnerTransferPayload {
    pub const TX_KIND: &'static str = "station_owner_transfer_v1";

    /// An UNSIGNED transfer. Unsigned never verifies.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        station_id: impl Into<String>,
        from_owner_address: impl Into<String>,
        from_owner_pubkey: impl Into<String>,
        to_owner_address: impl Into<String>,
        to_owner_pubkey: impl Into<String>,
        transfer_nonce: u64,
        expires_at_height: u64,
    ) -> Self {
        Self {
            tx_kind: Self::TX_KIND.to_string(),
            station_id: station_id.into(),
            from_owner_address: from_owner_address.into(),
            from_owner_pubkey: from_owner_pubkey.into(),
            to_owner_address: to_owner_address.into(),
            to_owner_pubkey: to_owner_pubkey.into(),
            transfer_nonce,
            expires_at_height,
            from_owner_signature: None,
            to_owner_signature: None,
        }
    }

    /// The signed body under `domain` (sender or acceptor tag).
    pub fn message_with_domain(&self, domain: &[u8], chain_id: &str, network_id: &str) -> Vec<u8> {
        station_owner_transfer_message_with_domain(
            domain,
            chain_id,
            network_id,
            &self.station_id,
            &self.from_owner_address,
            &self.to_owner_address,
            &self.to_owner_pubkey,
            self.transfer_nonce,
            self.expires_at_height,
        )
    }

    /// The CURRENT owner signs.
    pub fn sign_as_owner_with_domain(
        &mut self,
        chain_id: &str,
        network_id: &str,
        key: &PrivateKey,
    ) {
        let message = self.message_with_domain(STATION_OWNER_TRANSFER_DOMAIN, chain_id, network_id);
        self.from_owner_signature = Some(key.sign(&message).to_hex());
    }

    /// The NEW owner countersigns (#3345 Q4).
    pub fn countersign_with_domain(&mut self, chain_id: &str, network_id: &str, key: &PrivateKey) {
        let message =
            self.message_with_domain(STATION_OWNER_TRANSFER_ACCEPT_DOMAIN, chain_id, network_id);
        self.to_owner_signature = Some(key.sign(&message).to_hex());
    }

    /// Every check that does not read `State`: structure, both key bindings,
    /// BOTH signatures (the Q4 countersignature is mandatory), and the expiry
    /// window at `block_height`. The stateful checks (station exists, its
    /// registered owner is `from_owner_address`, nonce matches, not frozen)
    /// belong to the owner-ledger apply step.
    pub fn verify_stateless_with_domain(
        &self,
        chain_id: &str,
        network_id: &str,
        block_height: u64,
    ) -> Result<(), StationOwnerTransferRefusal> {
        use StationOwnerTransferRefusal as R;
        if self.tx_kind != Self::TX_KIND
            || self.station_id.is_empty()
            || self.station_id.chars().any(char::is_whitespace)
        {
            return Err(R::Malformed);
        }
        if same_station_owner(&self.from_owner_address, &self.to_owner_address) {
            return Err(R::SameOwner);
        }
        if !owner_address_bound_to_pubkey(&self.from_owner_address, &self.from_owner_pubkey) {
            return Err(R::FromKeyNotBound);
        }
        if !owner_address_bound_to_pubkey(&self.to_owner_address, &self.to_owner_pubkey) {
            return Err(R::ToKeyNotBound);
        }
        if block_height > self.expires_at_height {
            return Err(R::Expired);
        }
        if self.expires_at_height
            > block_height.saturating_add(STATION_OWNER_TRANSFER_MAX_VALIDITY_BLOCKS)
        {
            return Err(R::ValidityTooLong);
        }
        let owner_message =
            self.message_with_domain(STATION_OWNER_TRANSFER_DOMAIN, chain_id, network_id);
        if !verify_hex_signature(
            &self.from_owner_pubkey,
            self.from_owner_signature.as_deref(),
            &owner_message,
        ) {
            return Err(R::MissingOrBadOwnerSignature);
        }
        let accept_message =
            self.message_with_domain(STATION_OWNER_TRANSFER_ACCEPT_DOMAIN, chain_id, network_id);
        if !verify_hex_signature(
            &self.to_owner_pubkey,
            self.to_owner_signature.as_deref(),
            &accept_message,
        ) {
            return Err(R::MissingOrBadCountersignature);
        }
        Ok(())
    }
}

/// #3345 Q5 (owner ruling 2026-09-27): the record a transfer (or an executed
/// recovery) produces. ONLY `owner_address` changes; provenance class,
/// reputation, location, hardware, `is_active` and the ring key all belong to
/// the device and are kept.
pub fn transferred_station_record(
    existing: &WeatherStation,
    to_owner_address: &str,
) -> WeatherStation {
    let mut record = existing.clone();
    record.owner_address = to_owner_address.to_string();
    record
}

// ─── Time-locked recovery (#3345 Q1, #3347 Q1) ─────────────────────────────

/// Canonical preimage of a recovery filing (governance) or its acceptance
/// (new owner), selected by `domain`.
#[allow(clippy::too_many_arguments)]
pub fn station_owner_recovery_message_with_domain(
    domain: &[u8],
    chain_id: &str,
    network_id: &str,
    station_id: &str,
    from_owner_address: &str,
    to_owner_address: &str,
    to_owner_pubkey: &str,
    recovery_nonce: u64,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(domain.len() + 256);
    buf.extend_from_slice(domain);
    push_chain_domain(&mut buf, chain_id, network_id);
    push_str(&mut buf, station_id);
    push_str(&mut buf, from_owner_address);
    push_str(&mut buf, to_owner_address);
    push_str(&mut buf, to_owner_pubkey);
    buf.extend_from_slice(&recovery_nonce.to_le_bytes());
    buf
}

/// Canonical preimage of the CURRENT owner cancelling the pending recovery
/// identified by `(station_id, recovery_nonce)`.
pub fn station_owner_recovery_cancel_message_with_domain(
    chain_id: &str,
    network_id: &str,
    station_id: &str,
    owner_address: &str,
    recovery_nonce: u64,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(STATION_OWNER_RECOVERY_CANCEL_DOMAIN.len() + 128);
    buf.extend_from_slice(STATION_OWNER_RECOVERY_CANCEL_DOMAIN);
    push_chain_domain(&mut buf, chain_id, network_id);
    push_str(&mut buf, station_id);
    push_str(&mut buf, owner_address);
    buf.extend_from_slice(&recovery_nonce.to_le_bytes());
    buf
}

/// Why a recovery filing or cancel failed its stateless checks (inert
/// refusals).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationOwnerRecoveryRefusal {
    Malformed,
    SameOwner,
    /// The filing's `auth_pubkey` is not in the oracle-governance allowlist.
    NotGovernance,
    MissingOrBadGovernanceSignature,
    ToKeyNotBound,
    MissingOrBadAcceptance,
    OwnerKeyNotBound,
    MissingOrBadOwnerSignature,
}

/// Governance files a recovery naming a new owner (#3345 Q1). It does NOT
/// change the owner: applied, it records a [`PendingStationOwnerRecovery`]
/// that becomes executable only after [`STATION_OWNER_RECOVERY_NOTICE_BLOCKS`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StationOwnerRecoveryFilingPayload {
    /// Must equal [`StationOwnerRecoveryFilingPayload::TX_KIND`].
    pub tx_kind: String,
    pub station_id: String,
    /// The owner registered at filing time (pinned).
    pub from_owner_address: String,
    pub to_owner_address: String,
    pub to_owner_pubkey: String,
    /// Must equal the station's next owner nonce at filing (stateful; not
    /// checked here).
    pub recovery_nonce: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_owner_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_signature: Option<String>,
}

impl StationOwnerRecoveryFilingPayload {
    pub const TX_KIND: &'static str = "station_owner_recovery_file_v1";

    pub fn new(
        station_id: impl Into<String>,
        from_owner_address: impl Into<String>,
        to_owner_address: impl Into<String>,
        to_owner_pubkey: impl Into<String>,
        recovery_nonce: u64,
    ) -> Self {
        Self {
            tx_kind: Self::TX_KIND.to_string(),
            station_id: station_id.into(),
            from_owner_address: from_owner_address.into(),
            to_owner_address: to_owner_address.into(),
            to_owner_pubkey: to_owner_pubkey.into(),
            recovery_nonce,
            to_owner_signature: None,
            auth_pubkey: None,
            auth_signature: None,
        }
    }

    pub fn message_with_domain(&self, domain: &[u8], chain_id: &str, network_id: &str) -> Vec<u8> {
        station_owner_recovery_message_with_domain(
            domain,
            chain_id,
            network_id,
            &self.station_id,
            &self.from_owner_address,
            &self.to_owner_address,
            &self.to_owner_pubkey,
            self.recovery_nonce,
        )
    }

    pub fn sign_as_governance_with_domain(
        &mut self,
        chain_id: &str,
        network_id: &str,
        governance_key: &PrivateKey,
    ) {
        let message =
            self.message_with_domain(STATION_OWNER_RECOVERY_FILE_DOMAIN, chain_id, network_id);
        self.auth_signature = Some(governance_key.sign(&message).to_hex());
        self.auth_pubkey = Some(PublicKey::from_private_key(governance_key).to_hex());
    }

    pub fn accept_with_domain(
        &mut self,
        chain_id: &str,
        network_id: &str,
        new_owner_key: &PrivateKey,
    ) {
        let message =
            self.message_with_domain(STATION_OWNER_RECOVERY_ACCEPT_DOMAIN, chain_id, network_id);
        self.to_owner_signature = Some(new_owner_key.sign(&message).to_hex());
    }

    /// Stateless checks: allowlisted governance signature, new-owner key
    /// binding and acceptance, and `to != from`.
    pub fn verify_stateless_with_domain(
        &self,
        chain_id: &str,
        network_id: &str,
    ) -> Result<(), StationOwnerRecoveryRefusal> {
        use StationOwnerRecoveryRefusal as R;
        if self.tx_kind != Self::TX_KIND
            || self.station_id.is_empty()
            || self.station_id.chars().any(char::is_whitespace)
        {
            return Err(R::Malformed);
        }
        if same_station_owner(&self.from_owner_address, &self.to_owner_address) {
            return Err(R::SameOwner);
        }
        let Some(auth_pubkey) = self.auth_pubkey.as_deref() else {
            return Err(R::NotGovernance);
        };
        if !is_authorized_oracle_governance(auth_pubkey) {
            return Err(R::NotGovernance);
        }
        let filing =
            self.message_with_domain(STATION_OWNER_RECOVERY_FILE_DOMAIN, chain_id, network_id);
        if !verify_hex_signature(auth_pubkey, self.auth_signature.as_deref(), &filing) {
            return Err(R::MissingOrBadGovernanceSignature);
        }
        if !owner_address_bound_to_pubkey(&self.to_owner_address, &self.to_owner_pubkey) {
            return Err(R::ToKeyNotBound);
        }
        let accept =
            self.message_with_domain(STATION_OWNER_RECOVERY_ACCEPT_DOMAIN, chain_id, network_id);
        if !verify_hex_signature(
            &self.to_owner_pubkey,
            self.to_owner_signature.as_deref(),
            &accept,
        ) {
            return Err(R::MissingOrBadAcceptance);
        }
        Ok(())
    }
}

/// The CURRENT owner cancels a pending recovery (#3345 Q1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StationOwnerRecoveryCancelPayload {
    /// Must equal [`StationOwnerRecoveryCancelPayload::TX_KIND`].
    pub tx_kind: String,
    pub station_id: String,
    pub owner_address: String,
    pub owner_pubkey: String,
    /// Identifies the pending recovery being cancelled.
    pub recovery_nonce: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_signature: Option<String>,
}

impl StationOwnerRecoveryCancelPayload {
    pub const TX_KIND: &'static str = "station_owner_recovery_cancel_v1";

    pub fn new(
        station_id: impl Into<String>,
        owner_address: impl Into<String>,
        owner_pubkey: impl Into<String>,
        recovery_nonce: u64,
    ) -> Self {
        Self {
            tx_kind: Self::TX_KIND.to_string(),
            station_id: station_id.into(),
            owner_address: owner_address.into(),
            owner_pubkey: owner_pubkey.into(),
            recovery_nonce,
            owner_signature: None,
        }
    }

    pub fn message_with_domain(&self, chain_id: &str, network_id: &str) -> Vec<u8> {
        station_owner_recovery_cancel_message_with_domain(
            chain_id,
            network_id,
            &self.station_id,
            &self.owner_address,
            self.recovery_nonce,
        )
    }

    pub fn sign_with_domain(&mut self, chain_id: &str, network_id: &str, owner_key: &PrivateKey) {
        let message = self.message_with_domain(chain_id, network_id);
        self.owner_signature = Some(owner_key.sign(&message).to_hex());
    }

    pub fn verify_stateless_with_domain(
        &self,
        chain_id: &str,
        network_id: &str,
    ) -> Result<(), StationOwnerRecoveryRefusal> {
        use StationOwnerRecoveryRefusal as R;
        if self.tx_kind != Self::TX_KIND || self.station_id.is_empty() {
            return Err(R::Malformed);
        }
        if !owner_address_bound_to_pubkey(&self.owner_address, &self.owner_pubkey) {
            return Err(R::OwnerKeyNotBound);
        }
        let message = self.message_with_domain(chain_id, network_id);
        if !verify_hex_signature(
            &self.owner_pubkey,
            self.owner_signature.as_deref(),
            &message,
        ) {
            return Err(R::MissingOrBadOwnerSignature);
        }
        Ok(())
    }
}

/// A filed, not-yet-executed recovery, as the owner ledger will store it
/// (one per station at most). Pure block-height arithmetic; no clock.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingStationOwnerRecovery {
    pub from_owner_address: String,
    pub to_owner_address: String,
    pub recovery_nonce: u64,
    pub filed_at_height: u64,
}

impl PendingStationOwnerRecovery {
    /// Record a stateless-verified filing mined at `filed_at_height`.
    pub fn filed(filing: &StationOwnerRecoveryFilingPayload, filed_at_height: u64) -> Self {
        Self {
            from_owner_address: filing.from_owner_address.clone(),
            to_owner_address: filing.to_owner_address.clone(),
            recovery_nonce: filing.recovery_nonce,
            filed_at_height,
        }
    }

    /// First height at which the recovery may execute.
    pub fn executable_at_height(&self) -> u64 {
        self.filed_at_height
            .saturating_add(STATION_OWNER_RECOVERY_NOTICE_BLOCKS)
    }

    /// `true` only once the full public notice window has elapsed.
    pub fn is_executable_at(&self, block_height: u64) -> bool {
        block_height >= self.executable_at_height()
    }

    /// Does a stateless-verified `cancel` mined at `block_height` cancel THIS
    /// recovery? Only before execution becomes possible, only for this
    /// recovery's nonce, and only when signed by the owner the recovery would
    /// replace (the current owner).
    pub fn is_cancelled_by(
        &self,
        station_id: &str,
        cancel: &StationOwnerRecoveryCancelPayload,
        block_height: u64,
    ) -> bool {
        !self.is_executable_at(block_height)
            && cancel.station_id == station_id
            && cancel.recovery_nonce == self.recovery_nonce
            && same_station_owner(&cancel.owner_address, &self.from_owner_address)
    }
}

// ─── Owner actions (#3347 §3.2) ─────────────────────────────────────────────

/// Longest a signed-but-unsubmitted owner action may stay valid, in blocks
/// (same bound and reason as [`STATION_OWNER_TRANSFER_MAX_VALIDITY_BLOCKS`]).
pub const STATION_OWNER_ACTION_MAX_VALIDITY_BLOCKS: u64 = 10_000;

/// Why an owner action failed its stateless checks (inert refusals).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationOwnerActionRefusal {
    Malformed,
    /// `owner_pubkey` does not derive to `owner_address`.
    OwnerKeyNotBound,
    MissingOrBadOwnerSignature,
    /// `block_height > expires_at_height`.
    Expired,
    /// `expires_at_height > block_height + STATION_OWNER_ACTION_MAX_VALIDITY_BLOCKS`.
    ValidityTooLong,
}

/// An owner-signed station action (#3347): report stolen/lost (freeze),
/// unfreeze, self-deactivate, reactivate. Carried in a
/// `TransactionType::StationRegistration` tx's `data` with
/// `tx_kind = "station_owner_action_v1"`. No governance signature: only the
/// registered owner acts (owner ruling Q4: governance cannot lift a freeze).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StationOwnerActionPayload {
    /// Must equal [`StationOwnerActionPayload::TX_KIND`].
    pub tx_kind: String,
    pub station_id: String,
    /// The owner registered NOW (pinned into the preimage).
    pub owner_address: String,
    /// Hex ed25519; must derive (`PublicKey::to_address`, #2742) to
    /// `owner_address`.
    pub owner_pubkey: String,
    pub action: StationOwnerActionKind,
    /// Must equal the station's next owner nonce (the SAME per-station counter
    /// transfers and recovery cancels/executions consume).
    pub action_nonce: u64,
    pub expires_at_height: u64,
    /// Over `station_owner_action_message_with_domain`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_signature: Option<String>,
}

impl StationOwnerActionPayload {
    pub const TX_KIND: &'static str = "station_owner_action_v1";

    /// An UNSIGNED action. Unsigned never verifies.
    pub fn new(
        station_id: impl Into<String>,
        owner_address: impl Into<String>,
        owner_pubkey: impl Into<String>,
        action: StationOwnerActionKind,
        action_nonce: u64,
        expires_at_height: u64,
    ) -> Self {
        Self {
            tx_kind: Self::TX_KIND.to_string(),
            station_id: station_id.into(),
            owner_address: owner_address.into(),
            owner_pubkey: owner_pubkey.into(),
            action,
            action_nonce,
            expires_at_height,
            owner_signature: None,
        }
    }

    pub fn message_with_domain(&self, chain_id: &str, network_id: &str) -> Vec<u8> {
        station_owner_action_message_with_domain(
            chain_id,
            network_id,
            &self.station_id,
            &self.owner_address,
            self.action,
            self.action_nonce,
            self.expires_at_height,
        )
    }

    pub fn sign_with_domain(&mut self, chain_id: &str, network_id: &str, owner_key: &PrivateKey) {
        let message = self.message_with_domain(chain_id, network_id);
        self.owner_signature = Some(owner_key.sign(&message).to_hex());
    }

    /// Structure, key binding, signature and the validity window at
    /// `block_height`. The stateful checks (registered owner, nonce, the
    /// per-action frozen / deactivated preconditions) belong to `State`.
    pub fn verify_stateless_with_domain(
        &self,
        chain_id: &str,
        network_id: &str,
        block_height: u64,
    ) -> Result<(), StationOwnerActionRefusal> {
        use StationOwnerActionRefusal as R;
        if self.tx_kind != Self::TX_KIND
            || self.station_id.is_empty()
            || self.station_id.chars().any(char::is_whitespace)
        {
            return Err(R::Malformed);
        }
        if !owner_address_bound_to_pubkey(&self.owner_address, &self.owner_pubkey) {
            return Err(R::OwnerKeyNotBound);
        }
        if block_height > self.expires_at_height {
            return Err(R::Expired);
        }
        if self.expires_at_height
            > block_height.saturating_add(STATION_OWNER_ACTION_MAX_VALIDITY_BLOCKS)
        {
            return Err(R::ValidityTooLong);
        }
        let message = self.message_with_domain(chain_id, network_id);
        if !verify_hex_signature(
            &self.owner_pubkey,
            self.owner_signature.as_deref(),
            &message,
        ) {
            return Err(R::MissingOrBadOwnerSignature);
        }
        Ok(())
    }
}

// ─── Recovery execution (#3345 Q1: permissionless, from filed + 34,000) ────

/// Execute a filed recovery once its notice window has elapsed (owner ruling
/// 2026-09-27). PERMISSIONLESS and unsigned: the filing already carries the
/// governance signature and the new owner's acceptance; anyone may submit
/// this from `filed_at_height + STATION_OWNER_RECOVERY_NOTICE_BLOCKS`. There
/// is no begin-block hook and no per-block scan — a recovery executes only
/// when this transaction is mined.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StationOwnerRecoveryExecutePayload {
    /// Must equal [`StationOwnerRecoveryExecutePayload::TX_KIND`].
    pub tx_kind: String,
    pub station_id: String,
    /// Identifies the pending recovery being executed.
    pub recovery_nonce: u64,
}

impl StationOwnerRecoveryExecutePayload {
    pub const TX_KIND: &'static str = "station_owner_recovery_execute_v1";

    pub fn new(station_id: impl Into<String>, recovery_nonce: u64) -> Self {
        Self {
            tx_kind: Self::TX_KIND.to_string(),
            station_id: station_id.into(),
            recovery_nonce,
        }
    }

    fn well_formed(&self) -> bool {
        self.tx_kind == Self::TX_KIND
            && !self.station_id.is_empty()
            && !self.station_id.chars().any(char::is_whitespace)
    }
}

// ─── The owner ledger (#3345 §3.3 / #3347 §3.3) ───────────────────────────

/// A report-stolen freeze (#3347). Lifted ONLY by an owner-signed `unfreeze`
/// or an executed recovery; governance has no path (owner ruling Q4).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct StationFreeze {
    /// Height of the block that applied the `report_stolen` action.
    pub since_height: u64,
}

/// Per-station owner state, keyed by `station_id` in
/// `State::station_owner_ledger`. Additive and absent until the first owner
/// action on that station is APPLIED (a refused action creates no entry).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StationOwnerLedgerEntry {
    /// The nonce the NEXT owner-signed transfer / action (and the next
    /// recovery filing) must carry. Per STATION, not per owner, so it
    /// survives transfers and orders transfers, actions and recoveries
    /// against each other.
    pub owner_nonce: u64,
    /// #3347 report-stolen freeze.
    pub frozen: Option<StationFreeze>,
    /// #3347 owner self-deactivation. Distinct from the registry's
    /// governance-controlled `is_active`.
    pub owner_deactivated: bool,
    /// #3345 Q1 time-locked recovery, at most one per station.
    pub pending_recovery: Option<PendingStationOwnerRecovery>,
}

impl StationOwnerLedgerEntry {
    /// Does this entry stop the station's rewards (frozen or
    /// owner-deactivated)? The caller applies the feature gate.
    pub fn suspends_rewards(&self) -> bool {
        self.frozen.is_some() || self.owner_deactivated
    }

    /// Consume the nonce. A nonce-consuming step also voids any pending
    /// recovery (design §3.4: "a live owner can defeat it by acting").
    /// `None` at `u64::MAX` — the caller refuses rather than wrap or stick.
    fn consume_nonce(&mut self) -> Option<()> {
        self.owner_nonce = self.owner_nonce.checked_add(1)?;
        self.pending_recovery = None;
        Some(())
    }
}

/// Why a stateful owner-ledger step was refused. Every variant is an inert,
/// deterministic no-op (state unchanged); exposed so tests can prove WHICH
/// guard fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationOwnerLedgerRefusal {
    UnknownStation,
    /// The registered owner is not the one the payload names.
    NotRegisteredOwner,
    NonceMismatch,
    NonceExhausted,
    Frozen,
    NotFrozen,
    AlreadyDeactivated,
    NotDeactivated,
    RecoveryAlreadyPending,
    NoPendingRecovery,
    /// Cancel at/after `filed + NOTICE`.
    NoticeWindowElapsed,
    /// Execute before `filed + NOTICE`.
    NoticeWindowNotElapsed,
}

impl State {
    /// The appended `ApplyStationOwner*` arms: refuse a transition whose
    /// governing feature is not active at its height, so a forged or
    /// mis-built transition can never write pre-activation state (the
    /// fail-closed shape of the AA feature arms).
    pub(crate) fn require_station_owner_feature(
        feature: ConsensusFeature,
        block_height: u64,
    ) -> Result<(), crate::blockchain::state::StateError> {
        if feature_active(feature, block_height) {
            Ok(())
        } else {
            Err(crate::blockchain::state::StateError::InvalidTransition)
        }
    }

    /// A stateful refusal is an inert, deterministic no-op: logged, never an
    /// error (an error would fail the block and stall the chain).
    pub(crate) fn log_station_owner_refusal(
        step: &str,
        station_id: &str,
        block_height: u64,
        result: Result<(), StationOwnerLedgerRefusal>,
    ) {
        if let Err(refusal) = result {
            tracing::warn!(
                station_id,
                block_height,
                ?refusal,
                "#3345/#3347: station owner {step} refused (inert no-op)"
            );
        }
    }

    /// #3347: does the owner ledger stop `station_id`'s rewards at
    /// `block_height`? `false` at every height until
    /// `ConsensusFeature::StationOwnerActions` activates.
    pub fn station_owner_ledger_refuses_rewards(
        &self,
        station_id: &str,
        block_height: u64,
    ) -> bool {
        feature_active(ConsensusFeature::StationOwnerActions, block_height)
            && self
                .station_owner_ledger
                .get(station_id)
                .is_some_and(StationOwnerLedgerEntry::suspends_rewards)
    }

    /// #3347 Q5: are `station_id`'s penalties suspended at `block_height`
    /// (reported stolen)? `false` at every height until
    /// `ConsensusFeature::StationOwnerActions` activates.
    pub fn station_penalties_suspended(&self, station_id: &str, block_height: u64) -> bool {
        feature_active(ConsensusFeature::StationOwnerActions, block_height)
            && self
                .station_owner_ledger
                .get(station_id)
                .is_some_and(|entry| entry.frozen.is_some())
    }

    /// The registered owner, and a working copy of the ledger entry (default
    /// when absent). Nothing is written until the caller commits.
    fn owner_ledger_view(
        &self,
        station_id: &str,
        claimed_owner: &str,
    ) -> Result<(WeatherStation, StationOwnerLedgerEntry), StationOwnerLedgerRefusal> {
        let station = self
            .station_registry
            .get(station_id)
            .ok_or(StationOwnerLedgerRefusal::UnknownStation)?;
        if !same_station_owner(&station.owner_address, claimed_owner) {
            return Err(StationOwnerLedgerRefusal::NotRegisteredOwner);
        }
        let entry = self
            .station_owner_ledger
            .get(station_id)
            .cloned()
            .unwrap_or_default();
        Ok((station.clone(), entry))
    }

    fn set_station_owner(&mut self, existing: &WeatherStation, to_owner_address: &str) {
        // Only `owner_address` changes (Q5). `upsert` of an existing id only
        // replaces the record: `station_counts` is untouched because the
        // location and `is_active` are unchanged.
        let _ = self
            .station_registry
            .upsert(transferred_station_record(existing, to_owner_address));
    }

    /// #3345 §3.2 — stateful half of an owner transfer (stateless half
    /// already verified by the extractor).
    pub fn apply_station_owner_transfer(
        &mut self,
        station_id: &str,
        from_owner_address: &str,
        to_owner_address: &str,
        transfer_nonce: u64,
    ) -> Result<(), StationOwnerLedgerRefusal> {
        use StationOwnerLedgerRefusal as R;
        let (station, mut entry) = self.owner_ledger_view(station_id, from_owner_address)?;
        if entry.owner_nonce != transfer_nonce {
            return Err(R::NonceMismatch);
        }
        if entry.frozen.is_some() {
            return Err(R::Frozen);
        }
        entry.consume_nonce().ok_or(R::NonceExhausted)?;
        self.set_station_owner(&station, to_owner_address);
        self.station_owner_ledger
            .insert(station_id.to_string(), entry);
        Ok(())
    }

    /// #3347 §3.2 — stateful half of an owner action.
    pub fn apply_station_owner_action(
        &mut self,
        station_id: &str,
        owner_address: &str,
        action: StationOwnerActionKind,
        action_nonce: u64,
        block_height: u64,
    ) -> Result<(), StationOwnerLedgerRefusal> {
        use StationOwnerLedgerRefusal as R;
        let (_, mut entry) = self.owner_ledger_view(station_id, owner_address)?;
        if entry.owner_nonce != action_nonce {
            return Err(R::NonceMismatch);
        }
        match action {
            StationOwnerActionKind::ReportStolen => {
                if entry.frozen.is_some() {
                    return Err(R::Frozen);
                }
                entry.frozen = Some(StationFreeze {
                    since_height: block_height,
                });
            },
            StationOwnerActionKind::Unfreeze => {
                if entry.frozen.is_none() {
                    return Err(R::NotFrozen);
                }
                entry.frozen = None;
            },
            StationOwnerActionKind::Deactivate => {
                if entry.frozen.is_some() {
                    return Err(R::Frozen);
                }
                if entry.owner_deactivated {
                    return Err(R::AlreadyDeactivated);
                }
                entry.owner_deactivated = true;
            },
            StationOwnerActionKind::Reactivate => {
                if entry.frozen.is_some() {
                    return Err(R::Frozen);
                }
                if !entry.owner_deactivated {
                    return Err(R::NotDeactivated);
                }
                entry.owner_deactivated = false;
            },
        }
        entry.consume_nonce().ok_or(R::NonceExhausted)?;
        self.station_owner_ledger
            .insert(station_id.to_string(), entry);
        Ok(())
    }

    /// #3345 §3.4 — record a governance-filed, new-owner-accepted recovery.
    /// Does NOT change the owner and does NOT consume the nonce (so any
    /// later owner step voids it). Allowed on a frozen station: it is the
    /// only non-owner way to lift a freeze (#3347 Q1).
    pub fn apply_station_owner_recovery_filing(
        &mut self,
        station_id: &str,
        from_owner_address: &str,
        to_owner_address: &str,
        recovery_nonce: u64,
        block_height: u64,
    ) -> Result<(), StationOwnerLedgerRefusal> {
        use StationOwnerLedgerRefusal as R;
        let (_, mut entry) = self.owner_ledger_view(station_id, from_owner_address)?;
        if entry.owner_nonce != recovery_nonce {
            return Err(R::NonceMismatch);
        }
        if entry.pending_recovery.is_some() {
            return Err(R::RecoveryAlreadyPending);
        }
        entry.pending_recovery = Some(PendingStationOwnerRecovery {
            from_owner_address: from_owner_address.to_string(),
            to_owner_address: to_owner_address.to_string(),
            recovery_nonce,
            filed_at_height: block_height,
        });
        self.station_owner_ledger
            .insert(station_id.to_string(), entry);
        Ok(())
    }

    /// #3345 §3.4 — the CURRENT owner cancels a pending recovery, only while
    /// `block_height < filed + NOTICE`.
    pub fn apply_station_owner_recovery_cancel(
        &mut self,
        station_id: &str,
        owner_address: &str,
        recovery_nonce: u64,
        block_height: u64,
    ) -> Result<(), StationOwnerLedgerRefusal> {
        use StationOwnerLedgerRefusal as R;
        let (_, mut entry) = self.owner_ledger_view(station_id, owner_address)?;
        let pending = entry
            .pending_recovery
            .as_ref()
            .ok_or(R::NoPendingRecovery)?;
        if pending.recovery_nonce != recovery_nonce || entry.owner_nonce != recovery_nonce {
            return Err(R::NonceMismatch);
        }
        if pending.is_executable_at(block_height) {
            return Err(R::NoticeWindowElapsed);
        }
        if !same_station_owner(owner_address, &pending.from_owner_address) {
            return Err(R::NotRegisteredOwner);
        }
        entry.consume_nonce().ok_or(R::NonceExhausted)?;
        self.station_owner_ledger
            .insert(station_id.to_string(), entry);
        Ok(())
    }

    /// #3345 §3.4 / #3347 §3.5 — execute a pending recovery from
    /// `filed + NOTICE`: `owner_address := to` (provenance and reputation
    /// kept, Q5), freeze and owner-deactivation cleared, nonce consumed.
    pub fn apply_station_owner_recovery_execute(
        &mut self,
        station_id: &str,
        recovery_nonce: u64,
        block_height: u64,
    ) -> Result<(), StationOwnerLedgerRefusal> {
        use StationOwnerLedgerRefusal as R;
        let mut entry = self
            .station_owner_ledger
            .get(station_id)
            .cloned()
            .ok_or(R::NoPendingRecovery)?;
        let pending = entry.pending_recovery.clone().ok_or(R::NoPendingRecovery)?;
        if pending.recovery_nonce != recovery_nonce || entry.owner_nonce != recovery_nonce {
            return Err(R::NonceMismatch);
        }
        if !pending.is_executable_at(block_height) {
            return Err(R::NoticeWindowNotElapsed);
        }
        let (station, _) = self.owner_ledger_view(station_id, &pending.from_owner_address)?;
        entry.frozen = None;
        entry.owner_deactivated = false;
        entry.consume_nonce().ok_or(R::NonceExhausted)?;
        self.set_station_owner(&station, &pending.to_owner_address);
        self.station_owner_ledger
            .insert(station_id.to_string(), entry);
        Ok(())
    }
}

// ─── The ONE shared extractor ──────────────────────────────────────────────

/// Only `tx_kind`, to route a `StationRegistration`-carried payload.
#[derive(Deserialize)]
struct TxKindProbe {
    tx_kind: String,
}

impl Transaction {
    /// Is this one of the owner-ledger transactions (transfer, owner action,
    /// recovery file / cancel / execute)? They ride `StationRegistration`; the
    /// routing is by `tx_kind` only, with no signature or state check.
    pub fn is_station_owner_ledger_tx(&self) -> bool {
        if self.tx_type != TransactionType::StationRegistration {
            return false;
        }
        let Some(data) = self.data.as_deref() else {
            return false;
        };
        serde_json::from_str::<TxKindProbe>(data)
            .map(|probe| {
                matches!(
                    probe.tx_kind.as_str(),
                    StationOwnerTransferPayload::TX_KIND
                        | StationOwnerActionPayload::TX_KIND
                        | StationOwnerRecoveryFilingPayload::TX_KIND
                        | StationOwnerRecoveryCancelPayload::TX_KIND
                        | StationOwnerRecoveryExecutePayload::TX_KIND
                )
            })
            .unwrap_or(false)
    }

    /// The station a `recovery_execute` tx names, or `None` if this is not one.
    /// Used by node admission to keep at most one pending execute per station.
    pub fn recovery_execute_station(&self) -> Option<String> {
        self.parse_station_owner_payload::<StationOwnerRecoveryExecutePayload>(
            StationOwnerRecoveryExecutePayload::TX_KIND,
        )
        .filter(StationOwnerRecoveryExecutePayload::well_formed)
        .map(|p| p.station_id)
    }

    /// Parse a #3345 owner transfer from a `StationRegistration` tx.
    pub fn parse_station_owner_transfer(&self) -> Option<StationOwnerTransferPayload> {
        self.parse_station_owner_payload(StationOwnerTransferPayload::TX_KIND)
    }

    fn parse_station_owner_payload<T: serde::de::DeserializeOwned>(
        &self,
        tx_kind: &str,
    ) -> Option<T> {
        if self.tx_type != TransactionType::StationRegistration {
            return None;
        }
        let data = self.data.as_deref()?;
        let probe: TxKindProbe = serde_json::from_str(data).ok()?;
        if probe.tx_kind != tx_kind {
            return None;
        }
        serde_json::from_str(data).ok()
    }

    /// The ONE constructor every extractor uses for an owner-ledger step
    /// (#3345 / #3347): the PoP producer arm, the P2P / WAL
    /// `transition_extractor`, and therefore WAL replay. `None` — the tx is
    /// mined but inert — when the `tx_kind` is not an owner-ledger kind, the
    /// governing feature is not active at `block_height`, or any stateless
    /// check fails. The transition carries only the verified FIELDS the
    /// stateful step needs (no signatures), so WAL rows and snapshots never
    /// re-verify and never depend on the signing chain identity.
    pub fn station_owner_ledger_transition(&self, block_height: u64) -> Option<StateTransition> {
        if self.tx_type != TransactionType::StationRegistration {
            return None;
        }
        let probe: TxKindProbe = serde_json::from_str(self.data.as_deref()?).ok()?;
        let consent = feature_active(ConsensusFeature::StationOwnerConsent, block_height);
        let actions = feature_active(ConsensusFeature::StationOwnerActions, block_height);
        let identity = ChainIdentity::active();
        let (chain_id, network_id) = (identity.chain_id.as_str(), identity.network_id.as_str());
        match probe.tx_kind.as_str() {
            StationOwnerTransferPayload::TX_KIND if consent => {
                let p: StationOwnerTransferPayload =
                    self.parse_station_owner_payload(StationOwnerTransferPayload::TX_KIND)?;
                p.verify_stateless_with_domain(chain_id, network_id, block_height)
                    .ok()?;
                Some(StateTransition::ApplyStationOwnerTransfer {
                    station_id: p.station_id,
                    from_owner_address: p.from_owner_address,
                    to_owner_address: p.to_owner_address,
                    transfer_nonce: p.transfer_nonce,
                    block_height,
                })
            },
            StationOwnerRecoveryFilingPayload::TX_KIND if consent => {
                let p: StationOwnerRecoveryFilingPayload =
                    self.parse_station_owner_payload(StationOwnerRecoveryFilingPayload::TX_KIND)?;
                p.verify_stateless_with_domain(chain_id, network_id).ok()?;
                Some(StateTransition::ApplyStationOwnerRecoveryFiling {
                    station_id: p.station_id,
                    from_owner_address: p.from_owner_address,
                    to_owner_address: p.to_owner_address,
                    recovery_nonce: p.recovery_nonce,
                    block_height,
                })
            },
            StationOwnerRecoveryCancelPayload::TX_KIND if consent => {
                let p: StationOwnerRecoveryCancelPayload =
                    self.parse_station_owner_payload(StationOwnerRecoveryCancelPayload::TX_KIND)?;
                p.verify_stateless_with_domain(chain_id, network_id).ok()?;
                Some(StateTransition::ApplyStationOwnerRecoveryCancel {
                    station_id: p.station_id,
                    owner_address: p.owner_address,
                    recovery_nonce: p.recovery_nonce,
                    block_height,
                })
            },
            StationOwnerRecoveryExecutePayload::TX_KIND if consent => {
                let p: StationOwnerRecoveryExecutePayload =
                    self.parse_station_owner_payload(StationOwnerRecoveryExecutePayload::TX_KIND)?;
                if !p.well_formed() {
                    return None;
                }
                Some(StateTransition::ApplyStationOwnerRecoveryExecute {
                    station_id: p.station_id,
                    recovery_nonce: p.recovery_nonce,
                    block_height,
                })
            },
            StationOwnerActionPayload::TX_KIND if actions => {
                let p: StationOwnerActionPayload =
                    self.parse_station_owner_payload(StationOwnerActionPayload::TX_KIND)?;
                p.verify_stateless_with_domain(chain_id, network_id, block_height)
                    .ok()?;
                Some(StateTransition::ApplyStationOwnerAction {
                    station_id: p.station_id,
                    owner_address: p.owner_address,
                    action: p.action,
                    action_nonce: p.action_nonce,
                    block_height,
                })
            },
            _ => None,
        }
    }
}

// ─── Admission precheck (node submit route; read-only, never consensus) ─────

/// Why [`State::precheck_station_owner_tx`] refused an owner tx at admission.
/// ADMISSION POLICY, NOT CONSENSUS: a mined tx that would fail any of these is
/// still an inert no-op in a block; this only lets the node refuse it while it
/// is still fixable. Every variant carries a stable machine code
/// ([`StationOwnerTxRejection::code`]) that is the API error `code`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationOwnerTxRejection {
    /// Not a `StationRegistration` tx carrying an owner-ledger `tx_kind`.
    NotOwnerTx,
    /// The governing feature is not active at the admission height, so a mined
    /// copy would be inert.
    FeatureNotActive,
    /// `data` is not valid JSON for the named `tx_kind`.
    Malformed,
    Transfer(StationOwnerTransferRefusal),
    Action(StationOwnerActionRefusal),
    Recovery(StationOwnerRecoveryRefusal),
    Ledger(StationOwnerLedgerRefusal),
}

impl StationOwnerTxRejection {
    /// Stable API error code.
    pub fn code(self) -> &'static str {
        use StationOwnerActionRefusal as A;
        use StationOwnerLedgerRefusal as L;
        use StationOwnerRecoveryRefusal as C;
        use StationOwnerTransferRefusal as T;
        match self {
            Self::NotOwnerTx => "OWNER_TX_NOT_OWNER_TX",
            Self::FeatureNotActive => "OWNER_TX_FEATURE_NOT_ACTIVE",
            Self::Malformed => "OWNER_TX_MALFORMED",
            Self::Transfer(T::Malformed) => "OWNER_TX_MALFORMED",
            Self::Transfer(T::SameOwner) => "OWNER_TX_SAME_OWNER",
            Self::Transfer(T::FromKeyNotBound) => "OWNER_TX_FROM_KEY_NOT_BOUND",
            Self::Transfer(T::ToKeyNotBound) => "OWNER_TX_TO_KEY_NOT_BOUND",
            Self::Transfer(T::MissingOrBadOwnerSignature) => "OWNER_TX_BAD_OWNER_SIGNATURE",
            Self::Transfer(T::MissingOrBadCountersignature) => "OWNER_TX_BAD_COUNTERSIGNATURE",
            Self::Transfer(T::Expired) => "OWNER_TX_EXPIRED",
            Self::Transfer(T::ValidityTooLong) => "OWNER_TX_VALIDITY_TOO_LONG",
            Self::Action(A::Malformed) => "OWNER_TX_MALFORMED",
            Self::Action(A::OwnerKeyNotBound) => "OWNER_TX_OWNER_KEY_NOT_BOUND",
            Self::Action(A::MissingOrBadOwnerSignature) => "OWNER_TX_BAD_OWNER_SIGNATURE",
            Self::Action(A::Expired) => "OWNER_TX_EXPIRED",
            Self::Action(A::ValidityTooLong) => "OWNER_TX_VALIDITY_TOO_LONG",
            Self::Recovery(C::Malformed) => "OWNER_TX_MALFORMED",
            Self::Recovery(C::SameOwner) => "OWNER_TX_SAME_OWNER",
            Self::Recovery(C::NotGovernance) => "OWNER_TX_NOT_GOVERNANCE",
            Self::Recovery(C::MissingOrBadGovernanceSignature) => {
                "OWNER_TX_BAD_GOVERNANCE_SIGNATURE"
            },
            Self::Recovery(C::ToKeyNotBound) => "OWNER_TX_TO_KEY_NOT_BOUND",
            Self::Recovery(C::MissingOrBadAcceptance) => "OWNER_TX_BAD_COUNTERSIGNATURE",
            Self::Recovery(C::OwnerKeyNotBound) => "OWNER_TX_OWNER_KEY_NOT_BOUND",
            Self::Recovery(C::MissingOrBadOwnerSignature) => "OWNER_TX_BAD_OWNER_SIGNATURE",
            Self::Ledger(L::UnknownStation) => "OWNER_TX_UNKNOWN_STATION",
            Self::Ledger(L::NotRegisteredOwner) => "OWNER_TX_NOT_REGISTERED_OWNER",
            Self::Ledger(L::NonceMismatch) => "OWNER_TX_NONCE_MISMATCH",
            Self::Ledger(L::NonceExhausted) => "OWNER_TX_NONCE_EXHAUSTED",
            Self::Ledger(L::Frozen) => "OWNER_TX_STATION_FROZEN",
            Self::Ledger(L::NotFrozen) => "OWNER_TX_NOT_FROZEN",
            Self::Ledger(L::AlreadyDeactivated) => "OWNER_TX_ALREADY_DEACTIVATED",
            Self::Ledger(L::NotDeactivated) => "OWNER_TX_NOT_DEACTIVATED",
            Self::Ledger(L::RecoveryAlreadyPending) => "OWNER_TX_RECOVERY_ALREADY_PENDING",
            Self::Ledger(L::NoPendingRecovery) => "OWNER_TX_NO_PENDING_RECOVERY",
            Self::Ledger(L::NoticeWindowElapsed) => "OWNER_TX_NOTICE_WINDOW_ELAPSED",
            Self::Ledger(L::NoticeWindowNotElapsed) => "OWNER_TX_NOTICE_WINDOW_NOT_ELAPSED",
        }
    }
}

/// What an admitted owner tx will do, for the API response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedStationOwnerStep {
    pub station_id: String,
    /// The `tx_kind`.
    pub kind: &'static str,
    /// The ledger nonce the tx carries (`recovery_nonce` for recovery steps).
    pub nonce: u64,
}

/// The owner-ledger entry as the read route reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StationOwnerLedgerView {
    pub owner_nonce: u64,
    pub frozen: Option<StationFreeze>,
    pub owner_deactivated: bool,
    pub pending_recovery: Option<PendingRecoveryView>,
}

/// A pending recovery plus its derived notice-window heights.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingRecoveryView {
    pub from_owner_address: String,
    pub to_owner_address: String,
    pub recovery_nonce: u64,
    pub filed_at_height: u64,
    /// `filed_at_height + STATION_OWNER_RECOVERY_NOTICE_BLOCKS` (saturating).
    pub executable_at_height: u64,
}

impl State {
    /// The ledger entry for `station_id` as the API reports it (the default,
    /// empty entry when the station has none).
    pub fn station_owner_ledger_view(&self, station_id: &str) -> StationOwnerLedgerView {
        let entry = self
            .station_owner_ledger
            .get(station_id)
            .cloned()
            .unwrap_or_default();
        StationOwnerLedgerView {
            owner_nonce: entry.owner_nonce,
            frozen: entry.frozen,
            owner_deactivated: entry.owner_deactivated,
            pending_recovery: entry.pending_recovery.map(|p| PendingRecoveryView {
                executable_at_height: p.executable_at_height(),
                from_owner_address: p.from_owner_address,
                to_owner_address: p.to_owner_address,
                recovery_nonce: p.recovery_nonce,
                filed_at_height: p.filed_at_height,
            }),
        }
    }

    /// Admission precheck of ONE owner-ledger tx at `block_height` (the height
    /// it would mine at, `tip + 1`): the feature gate, the stateless checks the
    /// extractor runs, and a dry run of the real stateful step against a
    /// scratch state holding only this station's record and ledger entry — so
    /// the answer cannot drift from [`State::apply_station_owner_transfer`] and
    /// its siblings. Read-only; `self` is never modified.
    pub fn precheck_station_owner_tx(
        &self,
        tx: &Transaction,
        block_height: u64,
        chain_id: &str,
        network_id: &str,
    ) -> Result<AdmittedStationOwnerStep, StationOwnerTxRejection> {
        self.precheck_station_owner_tx_with_features(
            tx,
            block_height,
            chain_id,
            network_id,
            feature_active(ConsensusFeature::StationOwnerConsent, block_height),
            feature_active(ConsensusFeature::StationOwnerActions, block_height),
        )
    }

    /// [`State::precheck_station_owner_tx`] with the two governing feature
    /// states supplied, so a caller (and the route-level tests, which cannot
    /// install a process-wide schedule) can evaluate every refusal reason.
    pub fn precheck_station_owner_tx_with_features(
        &self,
        tx: &Transaction,
        block_height: u64,
        chain_id: &str,
        network_id: &str,
        consent: bool,
        actions: bool,
    ) -> Result<AdmittedStationOwnerStep, StationOwnerTxRejection> {
        use StationOwnerTxRejection as Rej;
        if tx.tx_type != TransactionType::StationRegistration {
            return Err(Rej::NotOwnerTx);
        }
        let data = tx.data.as_deref().ok_or(Rej::NotOwnerTx)?;
        let probe: TxKindProbe = serde_json::from_str(data).map_err(|_| Rej::NotOwnerTx)?;
        let is_known = matches!(
            probe.tx_kind.as_str(),
            StationOwnerTransferPayload::TX_KIND
                | StationOwnerRecoveryFilingPayload::TX_KIND
                | StationOwnerRecoveryCancelPayload::TX_KIND
                | StationOwnerRecoveryExecutePayload::TX_KIND
                | StationOwnerActionPayload::TX_KIND
        );
        if !is_known {
            return Err(Rej::NotOwnerTx);
        }
        match probe.tx_kind.as_str() {
            StationOwnerTransferPayload::TX_KIND => {
                if !consent {
                    return Err(Rej::FeatureNotActive);
                }
                let p: StationOwnerTransferPayload = tx
                    .parse_station_owner_payload(StationOwnerTransferPayload::TX_KIND)
                    .ok_or(Rej::Malformed)?;
                p.verify_stateless_with_domain(chain_id, network_id, block_height)
                    .map_err(Rej::Transfer)?;
                let mut scratch = self.owner_scratch(&p.station_id);
                scratch
                    .apply_station_owner_transfer(
                        &p.station_id,
                        &p.from_owner_address,
                        &p.to_owner_address,
                        p.transfer_nonce,
                    )
                    .map_err(Rej::Ledger)?;
                Ok(AdmittedStationOwnerStep {
                    station_id: p.station_id,
                    kind: StationOwnerTransferPayload::TX_KIND,
                    nonce: p.transfer_nonce,
                })
            },
            StationOwnerRecoveryFilingPayload::TX_KIND => {
                if !consent {
                    return Err(Rej::FeatureNotActive);
                }
                let p: StationOwnerRecoveryFilingPayload = tx
                    .parse_station_owner_payload(StationOwnerRecoveryFilingPayload::TX_KIND)
                    .ok_or(Rej::Malformed)?;
                p.verify_stateless_with_domain(chain_id, network_id)
                    .map_err(Rej::Recovery)?;
                let mut scratch = self.owner_scratch(&p.station_id);
                scratch
                    .apply_station_owner_recovery_filing(
                        &p.station_id,
                        &p.from_owner_address,
                        &p.to_owner_address,
                        p.recovery_nonce,
                        block_height,
                    )
                    .map_err(Rej::Ledger)?;
                Ok(AdmittedStationOwnerStep {
                    station_id: p.station_id,
                    kind: StationOwnerRecoveryFilingPayload::TX_KIND,
                    nonce: p.recovery_nonce,
                })
            },
            StationOwnerRecoveryCancelPayload::TX_KIND => {
                if !consent {
                    return Err(Rej::FeatureNotActive);
                }
                let p: StationOwnerRecoveryCancelPayload = tx
                    .parse_station_owner_payload(StationOwnerRecoveryCancelPayload::TX_KIND)
                    .ok_or(Rej::Malformed)?;
                p.verify_stateless_with_domain(chain_id, network_id)
                    .map_err(Rej::Recovery)?;
                let mut scratch = self.owner_scratch(&p.station_id);
                scratch
                    .apply_station_owner_recovery_cancel(
                        &p.station_id,
                        &p.owner_address,
                        p.recovery_nonce,
                        block_height,
                    )
                    .map_err(Rej::Ledger)?;
                Ok(AdmittedStationOwnerStep {
                    station_id: p.station_id,
                    kind: StationOwnerRecoveryCancelPayload::TX_KIND,
                    nonce: p.recovery_nonce,
                })
            },
            StationOwnerRecoveryExecutePayload::TX_KIND => {
                if !consent {
                    return Err(Rej::FeatureNotActive);
                }
                let p: StationOwnerRecoveryExecutePayload = tx
                    .parse_station_owner_payload(StationOwnerRecoveryExecutePayload::TX_KIND)
                    .ok_or(Rej::Malformed)?;
                if !p.well_formed() {
                    return Err(Rej::Malformed);
                }
                let mut scratch = self.owner_scratch(&p.station_id);
                scratch
                    .apply_station_owner_recovery_execute(
                        &p.station_id,
                        p.recovery_nonce,
                        block_height,
                    )
                    .map_err(Rej::Ledger)?;
                Ok(AdmittedStationOwnerStep {
                    station_id: p.station_id,
                    kind: StationOwnerRecoveryExecutePayload::TX_KIND,
                    nonce: p.recovery_nonce,
                })
            },
            _ => {
                if !actions {
                    return Err(Rej::FeatureNotActive);
                }
                let p: StationOwnerActionPayload = tx
                    .parse_station_owner_payload(StationOwnerActionPayload::TX_KIND)
                    .ok_or(Rej::Malformed)?;
                p.verify_stateless_with_domain(chain_id, network_id, block_height)
                    .map_err(Rej::Action)?;
                let mut scratch = self.owner_scratch(&p.station_id);
                scratch
                    .apply_station_owner_action(
                        &p.station_id,
                        &p.owner_address,
                        p.action,
                        p.action_nonce,
                        block_height,
                    )
                    .map_err(Rej::Ledger)?;
                Ok(AdmittedStationOwnerStep {
                    station_id: p.station_id,
                    kind: StationOwnerActionPayload::TX_KIND,
                    nonce: p.action_nonce,
                })
            },
        }
    }

    /// A scratch `State` holding only `station_id`'s registry record and ledger
    /// entry (if any), for the admission dry run.
    fn owner_scratch(&self, station_id: &str) -> State {
        let mut scratch = State::new();
        if let Some(station) = self.station_registry.get(station_id) {
            let _ = scratch.station_registry.upsert(station.clone());
        }
        if let Some(entry) = self.station_owner_ledger.get(station_id) {
            scratch
                .station_owner_ledger
                .insert(station_id.to_string(), entry.clone());
        }
        scratch
    }
}

// ─── The owner-tx envelope (CLI / any builder) ──────────────────────────────

impl Transaction {
    /// Wrap a signed owner-ledger payload (`data_json`) in the
    /// `StationRegistration` envelope the node accepts (same shape as
    /// [`Transaction::new_station_registration_with_domain`]): one inert input
    /// whose txid derives from the payload, one zero-value output addressed to
    /// `output_address`, `fee = 0`, no tx-level signature (the payload
    /// signatures are the authorization).
    ///
    /// `timestamp_secs` is PINNED by the caller (not read from the clock) so the
    /// artifact and its id are reproducible; the submit route rejects anything
    /// more than 150 s from node time. `None` if the timestamp is not
    /// representable. Prefer [`Transaction::canonical_station_owner_tx`], which
    /// also fixes the output address; the node accepts only that form.
    pub fn new_station_owner_tx(
        data_json: String,
        output_address: &str,
        timestamp_secs: i64,
    ) -> Option<Self> {
        use crate::blockchain::transaction::{TransactionInput, TransactionOutput};
        use crate::crypto::hash::hash_bytes;
        let timestamp = chrono::DateTime::from_timestamp(timestamp_secs, 0)?;
        let fingerprint = hash_bytes(data_json.as_bytes());
        let input = TransactionInput {
            txid: hash_bytes(format!("station_owner_tx:{}", fingerprint.to_hex()).as_bytes()),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        };
        let output = TransactionOutput {
            amount: 0,
            address: output_address.to_string(),
            script: String::new(),
            nft_data: None,
        };
        let mut tx = Transaction::new(vec![input], vec![output]);
        tx.tx_type = TransactionType::StationRegistration;
        tx.fee = 0;
        tx.data = Some(data_json);
        tx.timestamp = timestamp;
        tx.set_id();
        Some(tx)
    }

    /// The envelope's (inert) output address: the payload's `owner_address` or
    /// `from_owner_address` when it names one, else a fixed placeholder. One
    /// rule, shared by the node, the CLI and the admission check.
    pub fn canonical_station_owner_output_address(data_json: &str) -> String {
        serde_json::from_str::<serde_json::Value>(data_json)
            .ok()
            .and_then(|v| {
                ["owner_address", "from_owner_address"]
                    .iter()
                    .find_map(|k| v.get(*k).and_then(serde_json::Value::as_str).map(str::to_string))
            })
            .unwrap_or_else(|| "wrf-owner-tx".to_string())
    }

    /// THE envelope for `data_json` at `timestamp_secs`: exactly what the node
    /// builds in payload form and what admission requires of an envelope.
    pub fn canonical_station_owner_tx(data_json: &str, timestamp_secs: i64) -> Option<Self> {
        let output = Self::canonical_station_owner_output_address(data_json);
        Self::new_station_owner_tx(data_json.to_string(), &output, timestamp_secs)
    }

    /// Does this transaction equal, field for field, the canonical envelope for
    /// its own payload and (whole-second) timestamp? A crafted input txid, a
    /// non-zero fee, different outputs or a sub-second timestamp all fail: they
    /// would defeat the mempool's double-spend dedupe or skew its fee ordering.
    pub fn is_canonical_station_owner_envelope(&self) -> bool {
        let Some(data) = self.data.as_deref() else {
            return false;
        };
        let Some(expected) = Self::canonical_station_owner_tx(data, self.timestamp.timestamp())
        else {
            return false;
        };
        match (serde_json::to_value(self), serde_json::to_value(&expected)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }
}

// ─── v2 governance registration (signed expected nonce) ─────────────────────

/// Why a v2 registration is an inert no-op (also the admission refusal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationRegistrationRefusal {
    /// `expected_nonce` is not the station's current owner-ledger nonce
    /// (a replay, or an owner action since signing).
    NonceMismatch,
    /// `station_owner_consent` is active and the registration would change the
    /// registered owner.
    OwnerChangeWithoutConsent,
    /// The ledger nonce is at `u64::MAX`.
    NonceExhausted,
}

impl StationRegistrationRefusal {
    /// Stable API error code.
    pub fn code(self) -> &'static str {
        match self {
            Self::NonceMismatch => "STATION_REGISTRATION_NONCE_MISMATCH",
            Self::OwnerChangeWithoutConsent => "STATION_OWNER_CHANGE_REQUIRES_CONSENT",
            Self::NonceExhausted => "STATION_REGISTRATION_NONCE_EXHAUSTED",
        }
    }
}

impl StationOwnerLedgerEntry {
    /// Advance the nonce after a v2 registration applied. A pending recovery is
    /// voided exactly as for any nonce-consuming step.
    pub(crate) fn advance_nonce_for_registration(&mut self) -> Option<()> {
        self.consume_nonce()
    }
}

impl State {
    /// The single decision for a v2 registration, shared by the state arm and by
    /// node admission: on success returns the station's ledger entry (default
    /// when it has none) for the caller to advance and store. Read-only.
    pub fn precheck_station_registration_v2(
        &self,
        station: &WeatherStation,
        expected_nonce: u64,
        block_height: u64,
    ) -> Result<StationOwnerLedgerEntry, StationRegistrationRefusal> {
        use StationRegistrationRefusal as R;
        let entry = self
            .station_owner_ledger
            .get(&station.station_id)
            .cloned()
            .unwrap_or_default();
        if entry.owner_nonce != expected_nonce {
            return Err(R::NonceMismatch);
        }
        if feature_active(ConsensusFeature::StationOwnerConsent, block_height)
            && self.station_registration_changes_owner(station)
        {
            return Err(R::OwnerChangeWithoutConsent);
        }
        if entry.owner_nonce == u64::MAX {
            return Err(R::NonceExhausted);
        }
        Ok(entry)
    }
}
