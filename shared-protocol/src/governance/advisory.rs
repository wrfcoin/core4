//! Advisory governance: signed, text-only proposals with no execution path
//! (unscheduled `advisory_governance`).
//!
//! A wallet puts a short public text on chain (a title and a summary), wallets
//! that held at least 1 WRF when it was proposed vote on it, and after the
//! voting period the chain records the outcome. That is all. Nothing in this
//! module, and nothing it calls, can change a protocol parameter, a law, a
//! reward rule or any account other than the proposer's own deposit.
//!
//! ## Why a new transaction type
//!
//! The legacy `TransactionType::GovernanceProposal` carries no signature, takes
//! any `parameter`/`value`, lets the payload choose its deadline and deposit,
//! and debits whatever `sender_address` names. It is refused by mempool
//! admission and by the block classifier (#2024) and stays refused at every
//! height, whatever this feature's state. This type shares nothing with it:
//! - its payload has no parameter, value, law id, category, deadline or
//!   deposit member, and unknown members are refused (`deny_unknown_fields`);
//! - it is applied by [`State`](crate::blockchain::state::State) arms that
//!   never call `parse_governance_update`, `apply_governance_update` or the
//!   law book;
//! - its records live in their own map, so `GovernanceVote` and
//!   `FinalizeGovernanceProposal` (which read `governance.proposals`) cannot
//!   reach them, and their finalize ([`finalize`]) only sets a status and
//!   returns the deposit.
//!
//! ## Signing
//!
//! Each action is signed by the ed25519 key of the account that sends it, over
//! [`advisory_action_message`]: every field length-prefixed (u64 LE), under
//! [`ADVISORY_ACTION_DOMAIN_TAG`], binding the chain id, the network id, the
//! account, the action and the transaction nonce. The nonce binding stops a
//! relay from moving a signed action to another nonce, and the account's
//! nonce advances when the action applies, so neither action replays. The
//! signature covers bytes, not JSON.
//!
//! ### Passkey accounts
//!
//! An account controlled by a passkey (`accounts::passkey`, P-256 WebAuthn)
//! signs the SAME message: its assertion's challenge is
//! [`advisory_passkey_challenge`] = `sha3_256(advisory_action_message(..))`,
//! so it binds exactly the chain, network, account, action and nonce the
//! ed25519 signature does. The payload then carries a `passkey` member (a
//! `PasskeyAuthorization`) and no `public_key`/`signature`; exactly one of the
//! two forms is accepted. Verification is the shared
//! `accounts::passkey::verify_webauthn` under `CONSENSUS_WEBAUTHN_POLICY`
//! (type `webauthn.get`, challenge, RP ID hash, user present AND verified,
//! low-s P-256), the same path `passkey_bond_payout` and `security_key_spend`
//! use, and the credential (P-256 key + RP ID hash) must derive the account
//! (`passkey_account_address`), so an assertion made for another relying
//! party names a different account. Replay is stopped by the account nonce,
//! as for ed25519 (no per-credential counter is stored, as on the passkey
//! transfer path). Same feature flag: nothing about it is reachable below
//! the `advisory_governance` activation. An ed25519 payload serializes
//! byte-identically to one built before this option existed.
//!
//! Neither signature covers the outer transaction (its id is malleable;
//! no funds are at risk). Clients track an action by proposal id or by
//! `(account, nonce)`, never by tx id.
//!
//! ## Activation
//!
//! Below the activation height of `ConsensusFeature::AdvisoryGovernance`
//! mempool admission refuses the type, a block carrying one is invalid, and
//! the apply arm refuses it, so `State::advisory_proposals` is provably empty.
//! While empty the map is absent from `state_root_material`, checkpoints and
//! snapshots, so every root is byte-identical to a binary without this module.
//!
//! Design: `docs/architecture/advisory-governance.md`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::accounts::passkey::{
    CONSENSUS_WEBAUTHN_POLICY, PasskeyAuthorization, PasskeyError, passkey_account_address,
    verify_webauthn,
};
use crate::blockchain::transaction::{Transaction, TransactionType};
use crate::common::constants::WRF_UNITS_PER_COIN;
use crate::common::types::{Address, Balance};
use crate::crypto::signature::{PrivateKey, PublicKey, Signature};

/// Domain tag of every advisory-governance signature.
pub const ADVISORY_ACTION_DOMAIN_TAG: &[u8] = b"wrfcoin.advisory_governance_action.v1";

/// Title length, in UTF-8 bytes (1..=this).
pub const MAX_TITLE_BYTES: usize = 160;
/// Summary length, in UTF-8 bytes (1..=this).
pub const MAX_SUMMARY_BYTES: usize = 1_024;
/// Largest `Transaction::data` this module parses (after JSON escaping).
pub const MAX_ADVISORY_PAYLOAD_BYTES: usize = 8 * 1024;
/// Deposit debited from the proposer: 1 WRF. Fixed; the payload has no
/// deposit member.
pub const ADVISORY_DEPOSIT: u64 = WRF_UNITS_PER_COIN;
/// A wallet may vote if its spendable balance was at least this (1 WRF)
/// immediately before the proposal applied.
pub const MIN_VOTER_SPENDABLE: u64 = WRF_UNITS_PER_COIN;
/// Voting period: 1,120 blocks, about 24 h at 77 s per block.
pub const ADVISORY_VOTING_PERIOD_BLOCKS: u64 = 1_120;
/// A proposal needs at least this many votes (for + against) to be decided
/// (owner decision 2026-11-02). Otherwise it fails quorum.
pub const ADVISORY_QUORUM_MIN_VOTES: u64 = 3;
/// With quorum, a proposal passes when `for * 100 >= this * (for + against)`.
pub const ADVISORY_APPROVAL_PERCENT: u64 = 51;
/// Active proposals one wallet may have at once.
pub const MAX_ACTIVE_PER_PROPOSER: usize = 1;
/// Active proposals on chain at once.
pub const MAX_ACTIVE_PROPOSALS: usize = 20;
/// Proposals one block may carry (each one walks every account for its
/// electorate snapshot). Enforced by the block classifier.
pub const MAX_PROPOSALS_PER_BLOCK: usize = 2;
/// Proposal ids are `adv-` followed by 16 lowercase hex characters.
pub const PROPOSAL_ID_PREFIX: &str = "adv-";

/// One signed action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AdvisoryAction {
    /// Put a text on chain. The chain chooses the id, the deadline and the
    /// deposit.
    Propose { title: String, summary: String },
    /// For (`approve: true`) or against an active proposal.
    Vote { proposal_id: String, approve: bool },
}

/// `Transaction::data` of a `TransactionType::AdvisoryGovernance` transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedAdvisoryAction {
    /// Must equal the transaction's `sender_address`.
    pub account: Address,
    pub action: AdvisoryAction,
    /// Must equal the transaction's `nonce`, which must equal the account's
    /// nonce when the action applies.
    pub nonce: u64,
    /// Lowercase hex ed25519 public key; must hash to `account`. Absent (empty
    /// here, never serialized) when `passkey` authorizes the action; an
    /// explicit `""` is refused, so each action has one encoding.
    #[serde(default, deserialize_with = "deserialize_present_nonempty", skip_serializing_if = "String::is_empty")]
    pub public_key: String,
    /// Lowercase hex ed25519 signature over [`advisory_action_message`].
    /// Absent when `passkey` authorizes the action; an explicit `""` is refused.
    #[serde(default, deserialize_with = "deserialize_present_nonempty", skip_serializing_if = "String::is_empty")]
    pub signature: String,
    /// Passkey (P-256 WebAuthn) authorization instead of the ed25519 key and
    /// signature: an assertion over [`advisory_passkey_challenge`] from the
    /// credential that derives `account`. Absent members are never
    /// serialized, so ed25519 payloads are unchanged. Parsed strictly:
    /// unknown members inside it are refused, like everywhere in this payload,
    /// and so is an explicit `null` (absent is the only "no passkey").
    #[serde(
        default,
        deserialize_with = "deserialize_strict_passkey",
        skip_serializing_if = "Option::is_none"
    )]
    pub passkey: Option<PasskeyAuthorization>,
}

/// The shared `PasskeyAuthorization` tolerates unknown members (it is
/// embedded in older payloads). The advisory payload refuses them, so a
/// `parameter`-shaped member cannot ride inside `passkey` either.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictPasskeyAuthorization {
    public_key_p256: String,
    rp_id_hash: String,
    authenticator_data: String,
    client_data_json: String,
    signature: String,
}

/// Only called when the member is present (`default` covers absence), so
/// `"passkey": null` fails here: a `StrictPasskeyAuthorization` is an object.
fn deserialize_strict_passkey<'de, D>(deserializer: D) -> Result<Option<PasskeyAuthorization>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let p = StrictPasskeyAuthorization::deserialize(deserializer)?;
    Ok(Some(PasskeyAuthorization {
        public_key_p256: p.public_key_p256,
        rp_id_hash: p.rp_id_hash,
        authenticator_data: p.authenticator_data,
        client_data_json: p.client_data_json,
        signature: p.signature,
    }))
}

/// A present `public_key`/`signature` member must not be `""` (absent is the
/// only "not this form").
fn deserialize_present_nonempty<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.is_empty() {
        return Err(serde::de::Error::custom("an authorization member, when present, must not be empty"));
    }
    Ok(value)
}

/// Non-empty, even-length, lowercase hex: the one accepted spelling of every
/// hex member (ed25519 key and signature, each passkey field).
fn is_canonical_hex(value: &str) -> bool {
    !value.is_empty() && value.len() % 2 == 0 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// How a proposal ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvisoryStatus {
    Active,
    /// Quorum met and approved. Advisory: nothing is executed.
    Passed,
    /// Quorum met and not approved.
    Rejected,
    /// Fewer than [`ADVISORY_QUORUM_MIN_VOTES`] votes. The deposit stays held.
    FailedQuorum,
}

/// One proposal as stored in `State::advisory_proposals`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryProposal {
    pub id: String,
    pub proposer: Address,
    pub title: String,
    pub summary: String,
    pub created_at_block: u64,
    /// Votes count while `height < voting_deadline`; finalized at the first
    /// block with `height >= voting_deadline`.
    pub voting_deadline: u64,
    /// Held deposit. Zero once returned.
    pub deposit: Balance,
    /// Wallets that may vote (snapshot). Cleared at finalize.
    pub eligible_voters: BTreeSet<Address>,
    /// Size of the snapshot; kept after finalize.
    pub eligible_count: u64,
    /// Votes cast, `true` = for. Cleared at finalize (the votes stay in the
    /// blocks' transactions).
    pub votes: BTreeMap<Address, bool>,
    pub yes_votes: u64,
    pub no_votes: u64,
    pub status: AdvisoryStatus,
    pub finalized_at_block: Option<u64>,
}

/// What applying one checked action writes: the signer's nonce advances,
/// the deposit (proposals only) is debited from the signer, and `proposal` is
/// stored under its id (a new record, or the voted-on record).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvisoryEffect {
    pub signer: Address,
    pub next_nonce: u64,
    pub deposit: Option<Balance>,
    pub proposal: AdvisoryProposal,
}

/// Every way an action is refused. No panic anywhere in this module.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AdvisoryError {
    #[error("malformed key or signature: {0}")]
    Malformed(&'static str),
    #[error("public key does not hash to the account")]
    KeyAddressMismatch,
    #[error("signature does not verify")]
    BadSignature,
    #[error("title: {0}")]
    Title(TextError),
    #[error("summary: {0}")]
    Summary(TextError),
    #[error("proposal id is not a chain-derived advisory id")]
    BadProposalId,
    /// The payload carries both an ed25519 key/signature and a passkey
    /// authorization, or neither.
    #[error("exactly one of an ed25519 signature or a passkey authorization is required")]
    AuthorizationForm,
    /// The passkey assertion failed WebAuthn verification.
    #[error("passkey assertion: {0}")]
    Passkey(PasskeyError),
}

/// Why a title or summary is refused.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TextError {
    #[error("empty")]
    Empty,
    #[error("{got} bytes is over the {max}-byte limit")]
    TooLong { got: usize, max: usize },
    #[error("character U+{0:04X} is not allowed")]
    ForbiddenChar(u32),
}

/// Whether `c` is refused in proposal text. A byte rule only, with no Unicode
/// normalization (which could vary between library versions):
/// C0 controls (U+0000..=U+001F; `\n` is allowed in a summary), DEL and the
/// C1 controls (U+007F..=U+009F), and the bidirectional embedding, override
/// and isolate characters (U+202A..=U+202E, U+2066..=U+2069).
pub fn forbidden_char(c: char, allow_newline: bool) -> bool {
    let cp = c as u32;
    if c == '\n' {
        return !allow_newline;
    }
    cp <= 0x1F
        || (0x7F..=0x9F).contains(&cp)
        || (0x202A..=0x202E).contains(&cp)
        || (0x2066..=0x2069).contains(&cp)
}

fn validate_text(text: &str, max: usize, allow_newline: bool) -> Result<(), TextError> {
    if text.trim().is_empty() {
        return Err(TextError::Empty);
    }
    if text.len() > max {
        return Err(TextError::TooLong {
            got: text.len(),
            max,
        });
    }
    if let Some(c) = text.chars().find(|c| forbidden_char(*c, allow_newline)) {
        return Err(TextError::ForbiddenChar(c as u32));
    }
    Ok(())
}

/// Title rules: 1..=[`MAX_TITLE_BYTES`] bytes, not blank, one line.
pub fn validate_title(title: &str) -> Result<(), TextError> {
    validate_text(title, MAX_TITLE_BYTES, false)
}

/// Summary rules: 1..=[`MAX_SUMMARY_BYTES`] bytes, not blank, `\n` allowed.
pub fn validate_summary(summary: &str) -> Result<(), TextError> {
    validate_text(summary, MAX_SUMMARY_BYTES, true)
}

/// Whether `id` has the shape of a chain-derived advisory id.
pub fn is_advisory_proposal_id(id: &str) -> bool {
    id.strip_prefix(PROPOSAL_ID_PREFIX).is_some_and(|hex| {
        hex.len() == 16
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// The id the chain gives a proposal: `adv-` + the first 8 bytes (16 hex) of
/// SHA-256(tag, proposer bytes, nonce LE). The proposer does not choose it,
/// and the account nonce makes it unique per proposer.
pub fn proposal_id(proposer: &Address, nonce: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ADVISORY_ACTION_DOMAIN_TAG);
    hasher.update(proposer.0);
    hasher.update(nonce.to_le_bytes());
    let digest = hasher.finalize();
    format!("{PROPOSAL_ID_PREFIX}{}", hex::encode(&digest[..8]))
}

fn push(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// Stable, non-serde encoding of an action for the preimage.
///
/// ```text
/// propose: 0x01 || len title || title || len summary || summary
/// vote:    0x02 || len proposal_id || proposal_id || 0x01 (for) | 0x00 (against)
/// ```
pub fn encode_action_for_preimage(action: &AdvisoryAction) -> Vec<u8> {
    let mut encoded = Vec::new();
    match action {
        AdvisoryAction::Propose { title, summary } => {
            encoded.push(0x01);
            push(&mut encoded, title.as_bytes());
            push(&mut encoded, summary.as_bytes());
        },
        AdvisoryAction::Vote {
            proposal_id,
            approve,
        } => {
            encoded.push(0x02);
            push(&mut encoded, proposal_id.as_bytes());
            encoded.push(u8::from(*approve));
        },
    }
    encoded
}

/// The exact bytes the account key signs. Every component is length-prefixed
/// (u64 LE): tag, chain id, network id, 20 account bytes, encoded action,
/// 8 nonce bytes LE.
///
/// Built explicitly rather than with `DomainSeparated::signing_preimage()`:
/// at signing V1 that construction drops the chain and network binding.
pub fn advisory_action_message(
    chain_id: &str,
    network_id: &str,
    account: &Address,
    action: &AdvisoryAction,
    nonce: u64,
) -> Vec<u8> {
    let mut message = Vec::new();
    push(&mut message, ADVISORY_ACTION_DOMAIN_TAG);
    push(&mut message, chain_id.as_bytes());
    push(&mut message, network_id.as_bytes());
    push(&mut message, &account.0);
    push(&mut message, &encode_action_for_preimage(action));
    push(&mut message, &nonce.to_le_bytes());
    message
}

/// The 32-byte WebAuthn challenge a passkey account's assertion commits to:
/// `sha3_256` of the exact [`advisory_action_message`] the ed25519 path signs
/// (the same hash the passkey transfer challenge uses). The message opens with
/// the length-prefixed [`ADVISORY_ACTION_DOMAIN_TAG`], so it cannot equal a
/// passkey transfer, security-key spend or bond-payout challenge preimage.
pub fn advisory_passkey_challenge(
    chain_id: &str,
    network_id: &str,
    account: &Address,
    action: &AdvisoryAction,
    nonce: u64,
) -> [u8; 32] {
    *crate::crypto::hash::hash_bytes(&advisory_action_message(
        chain_id, network_id, account, action, nonce,
    ))
    .as_bytes()
}

impl SignedAdvisoryAction {
    /// Wrap a passkey authorization (the browser's assertion over
    /// [`advisory_passkey_challenge`]) for `account`. No verification here.
    pub fn with_passkey(
        account: Address,
        action: AdvisoryAction,
        nonce: u64,
        passkey: PasskeyAuthorization,
    ) -> Self {
        Self {
            account,
            action,
            nonce,
            public_key: String::new(),
            signature: String::new(),
            passkey: Some(passkey),
        }
    }

    /// Sign `action` with a software passkey (tests and fixtures only; the
    /// chain never signs P-256).
    #[cfg(any(test, feature = "test-support"))]
    pub fn sign_passkey(
        chain_id: &str,
        network_id: &str,
        authenticator: &crate::accounts::passkey::test_support::SoftwareAuthenticator,
        action: AdvisoryAction,
        nonce: u64,
    ) -> Self {
        let account = authenticator.account();
        let challenge = advisory_passkey_challenge(chain_id, network_id, &account, &action, nonce);
        let passkey = authenticator.authorization_for(&authenticator.assert(&challenge));
        Self::with_passkey(account, action, nonce, passkey)
    }

    /// Whether a passkey authorizes this action (for pricing).
    pub fn is_passkey(&self) -> bool {
        self.passkey.is_some()
    }

    /// Sign `action` with `key` (CLI and tests).
    pub fn sign(
        chain_id: &str,
        network_id: &str,
        key: &PrivateKey,
        action: AdvisoryAction,
        nonce: u64,
    ) -> Self {
        let public_key = PublicKey::from_private_key(key);
        let account = Address::from_public_key(&public_key);
        let message = advisory_action_message(chain_id, network_id, &account, &action, nonce);
        Self {
            account,
            action,
            nonce,
            public_key: public_key.to_hex(),
            signature: key.sign(&message).to_hex(),
            passkey: None,
        }
    }

    /// Stateless check, run by mempool admission, the block classifier and
    /// apply: text rules, id shape, the key hashes to the account, and the
    /// signature verifies over the preimage for this chain and network.
    pub fn verify(&self, chain_id: &str, network_id: &str) -> Result<(), AdvisoryError> {
        match &self.action {
            AdvisoryAction::Propose { title, summary } => {
                validate_title(title).map_err(AdvisoryError::Title)?;
                validate_summary(summary).map_err(AdvisoryError::Summary)?;
            },
            AdvisoryAction::Vote { proposal_id, .. } => {
                if !is_advisory_proposal_id(proposal_id) {
                    return Err(AdvisoryError::BadProposalId);
                }
            },
        }
        if let Some(passkey) = &self.passkey {
            if !self.public_key.is_empty() || !self.signature.is_empty() {
                return Err(AdvisoryError::AuthorizationForm);
            }
            if !self.has_canonical_hex() {
                return Err(AdvisoryError::Malformed("passkey fields must be lowercase hex"));
            }
            return self.verify_passkey(passkey, chain_id, network_id);
        }
        if self.public_key.is_empty() || self.signature.is_empty() {
            return Err(AdvisoryError::AuthorizationForm);
        }
        if !self.has_canonical_hex() {
            return Err(AdvisoryError::Malformed("public key and signature must be lowercase hex"));
        }
        let public_key = PublicKey::from_hex(self.public_key.trim())
            .map_err(|_| AdvisoryError::Malformed("public key is not hex ed25519"))?;
        if Address::from_public_key(&public_key) != self.account {
            return Err(AdvisoryError::KeyAddressMismatch);
        }
        let signature = Signature::from_hex(self.signature.trim())
            .map_err(|_| AdvisoryError::Malformed("signature is not hex ed25519"))?;
        let message = advisory_action_message(
            chain_id,
            network_id,
            &self.account,
            &self.action,
            self.nonce,
        );
        if !public_key.verify(&message, &signature) {
            return Err(AdvisoryError::BadSignature);
        }
        Ok(())
    }

    /// Every hex member present is in its one accepted spelling (lowercase,
    /// no whitespace, even length), so each action has one encoding.
    fn has_canonical_hex(&self) -> bool {
        match &self.passkey {
            Some(p) => [&p.public_key_p256, &p.rp_id_hash, &p.authenticator_data, &p.client_data_json, &p.signature]
                .into_iter()
                .all(|v| is_canonical_hex(v)),
            None => is_canonical_hex(&self.public_key) && is_canonical_hex(&self.signature),
        }
    }

    /// The passkey arm of [`Self::verify`], built from the same pieces as
    /// `node_bond_registry::verify_payout_passkey`: the credential must derive
    /// the account, then full WebAuthn verification under the consensus
    /// policy for this action's challenge.
    fn verify_passkey(
        &self,
        passkey: &PasskeyAuthorization,
        chain_id: &str,
        network_id: &str,
    ) -> Result<(), AdvisoryError> {
        let credential = passkey
            .credential()
            .map_err(|_| AdvisoryError::Malformed("passkey key or rp_id_hash is not hex of the right length"))?;
        if passkey_account_address(&credential) != self.account {
            return Err(AdvisoryError::KeyAddressMismatch);
        }
        let assertion = passkey
            .assertion()
            .map_err(|_| AdvisoryError::Malformed("passkey assertion field is not hex"))?;
        let challenge =
            advisory_passkey_challenge(chain_id, network_id, &self.account, &self.action, self.nonce);
        verify_webauthn(&credential, &assertion, &challenge, CONSENSUS_WEBAUTHN_POLICY)
            .map_err(AdvisoryError::Passkey)?;
        Ok(())
    }

    /// Parse a `TransactionType::AdvisoryGovernance` transaction. `None` for
    /// any other type, an oversized or unparsable payload, an unknown member,
    /// a payload whose account is not the transaction's `sender_address`, or
    /// whose nonce is not the transaction's `nonce`.
    pub fn parse(tx: &Transaction) -> Option<Self> {
        if tx.tx_type != TransactionType::AdvisoryGovernance {
            return None;
        }
        let data = tx.data.as_ref()?;
        if data.len() > MAX_ADVISORY_PAYLOAD_BYTES {
            return None;
        }
        let parsed: Self = serde_json::from_str(data).ok()?;
        // One encoding per action: an ed25519 payload needs both members (as
        // before the passkey option existed), a passkey payload carries
        // neither, and every hex member is lowercase (`"passkey": null` and
        // explicit `""` members already failed deserialization).
        let form_ok = match parsed.passkey {
            None => !parsed.public_key.is_empty() && !parsed.signature.is_empty(),
            Some(_) => parsed.public_key.is_empty() && parsed.signature.is_empty(),
        } && parsed.has_canonical_hex();
        (form_ok && parsed.account == tx.sender_address && parsed.nonce == tx.nonce).then_some(parsed)
    }

    /// Whether this is a proposal (for the per-block cap).
    pub fn is_propose(&self) -> bool {
        matches!(self.action, AdvisoryAction::Propose { .. })
    }
}

/// The outcome rule, a pure function of the tallies.
pub fn outcome(yes_votes: u64, no_votes: u64) -> AdvisoryStatus {
    let total = u128::from(yes_votes) + u128::from(no_votes);
    if total < u128::from(ADVISORY_QUORUM_MIN_VOTES) {
        return AdvisoryStatus::FailedQuorum;
    }
    if u128::from(yes_votes) * 100 >= u128::from(ADVISORY_APPROVAL_PERCENT) * total {
        AdvisoryStatus::Passed
    } else {
        AdvisoryStatus::Rejected
    }
}

/// Active proposals, and active proposals of `proposer`.
pub fn active_counts(
    proposals: &BTreeMap<String, AdvisoryProposal>,
    proposer: &Address,
) -> (usize, usize) {
    let mut all = 0;
    let mut mine = 0;
    for proposal in proposals.values() {
        if proposal.status == AdvisoryStatus::Active {
            all += 1;
            if proposal.proposer == *proposer {
                mine += 1;
            }
        }
    }
    (all, mine)
}

/// Ids of active proposals due for finalize at `height`, in id order.
pub fn due_proposal_ids(
    proposals: &BTreeMap<String, AdvisoryProposal>,
    height: u64,
) -> Vec<String> {
    proposals
        .values()
        .filter(|p| p.status == AdvisoryStatus::Active && p.voting_deadline <= height)
        .map(|p| p.id.clone())
        .collect()
}

/// Record a vote on an active proposal. All checks precede the write.
pub fn record_vote(
    proposal: &mut AdvisoryProposal,
    voter: &Address,
    approve: bool,
    height: u64,
) -> Result<(), String> {
    if proposal.status != AdvisoryStatus::Active {
        return Err("proposal is not active".into());
    }
    if height >= proposal.voting_deadline {
        return Err("voting period has ended".into());
    }
    if !proposal.eligible_voters.contains(voter) {
        return Err("wallet held under 1 WRF spendable when the proposal was created".into());
    }
    if proposal.votes.contains_key(voter) {
        return Err("wallet already voted".into());
    }
    if approve {
        proposal.yes_votes = proposal.yes_votes.checked_add(1).ok_or("tally overflow")?;
    } else {
        proposal.no_votes = proposal.no_votes.checked_add(1).ok_or("tally overflow")?;
    }
    proposal.votes.insert(*voter, approve);
    Ok(())
}

/// Finalize: set the outcome and drop the per-voter maps. Returns the deposit
/// to give back to the proposer (zero for a failed quorum, whose deposit stays
/// held). The caller credits it. Sets no parameter, touches no law: the only
/// effects of an advisory proposal are this record and that deposit.
pub fn finalize(proposal: &mut AdvisoryProposal, height: u64) -> Result<Balance, String> {
    if proposal.status != AdvisoryStatus::Active {
        return Err("proposal is not active".into());
    }
    if height < proposal.voting_deadline {
        return Err("voting period not finished".into());
    }
    let status = outcome(proposal.yes_votes, proposal.no_votes);
    proposal.status = status;
    proposal.finalized_at_block = Some(height);
    proposal.eligible_voters.clear();
    proposal.votes.clear();
    Ok(match status {
        AdvisoryStatus::Passed | AdvisoryStatus::Rejected => proposal.deposit,
        AdvisoryStatus::FailedQuorum | AdvisoryStatus::Active => Balance::new(0),
    })
}

/// Canonical state-root encoding of the map. Explicit, length-prefixed, in
/// BTreeMap/BTreeSet order. Only called while the map is non-empty.
pub fn root_material(proposals: &BTreeMap<String, AdvisoryProposal>, out: &mut Vec<u8>) {
    fn u64_(out: &mut Vec<u8>, v: u64) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(b"wrfcoin_advisory_proposals_v1");
    u64_(out, proposals.len() as u64);
    for (id, p) in proposals {
        push(out, id.as_bytes());
        push(out, &p.proposer.0);
        push(out, p.title.as_bytes());
        push(out, p.summary.as_bytes());
        u64_(out, p.created_at_block);
        u64_(out, p.voting_deadline);
        out.extend_from_slice(&p.deposit.into_inner().to_le_bytes());
        u64_(out, p.eligible_count);
        u64_(out, p.eligible_voters.len() as u64);
        for voter in &p.eligible_voters {
            out.extend_from_slice(&voter.0);
        }
        u64_(out, p.votes.len() as u64);
        for (voter, approve) in &p.votes {
            out.extend_from_slice(&voter.0);
            out.push(u8::from(*approve));
        }
        u64_(out, p.yes_votes);
        u64_(out, p.no_votes);
        out.push(match p.status {
            AdvisoryStatus::Active => 0,
            AdvisoryStatus::Passed => 1,
            AdvisoryStatus::Rejected => 2,
            AdvisoryStatus::FailedQuorum => 3,
        });
        match p.finalized_at_block {
            None => out.push(0),
            Some(h) => {
                out.push(1);
                u64_(out, h);
            },
        }
    }
}

/// Read model for the node API (no per-voter list for a finalized proposal:
/// it was cleared).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryProposalView {
    pub id: String,
    pub kind: &'static str,
    pub proposer: String,
    pub title: String,
    pub summary: String,
    pub created_at_block: u64,
    pub voting_deadline: u64,
    pub status: AdvisoryStatus,
    pub yes_votes: u64,
    pub no_votes: u64,
    pub eligible_count: u64,
    pub quorum_min_votes: u64,
    pub approval_percent: u64,
    pub deposit_held: String,
    pub finalized_at_block: Option<u64>,
}

impl AdvisoryProposalView {
    pub fn from_record(p: &AdvisoryProposal) -> Self {
        Self {
            id: p.id.clone(),
            kind: "advisory",
            proposer: p.proposer.to_string(),
            title: p.title.clone(),
            summary: p.summary.clone(),
            created_at_block: p.created_at_block,
            voting_deadline: p.voting_deadline,
            status: p.status,
            yes_votes: p.yes_votes,
            no_votes: p.no_votes,
            eligible_count: p.eligible_count,
            quorum_min_votes: ADVISORY_QUORUM_MIN_VOTES,
            approval_percent: ADVISORY_APPROVAL_PERCENT,
            deposit_held: p.deposit.into_inner().to_string(),
            finalized_at_block: p.finalized_at_block,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHAIN: &str = "888";
    const NET: &str = "testnet";

    fn key(seed: u8) -> PrivateKey {
        PrivateKey::from_bytes(&[seed; 32]).expect("key")
    }

    fn propose(title: &str, summary: &str) -> AdvisoryAction {
        AdvisoryAction::Propose {
            title: title.into(),
            summary: summary.into(),
        }
    }

    #[test]
    fn a_signed_action_verifies_and_every_bound_field_breaks_it() {
        let k = key(7);
        let signed = SignedAdvisoryAction::sign(CHAIN, NET, &k, propose("Welcome", "Hello."), 4);
        signed.verify(CHAIN, NET).expect("verifies");
        assert_eq!(signed.verify("777", NET), Err(AdvisoryError::BadSignature));
        assert_eq!(
            signed.verify(CHAIN, "devnet"),
            Err(AdvisoryError::BadSignature)
        );
        let mut other_nonce = signed.clone();
        other_nonce.nonce = 5;
        assert_eq!(
            other_nonce.verify(CHAIN, NET),
            Err(AdvisoryError::BadSignature)
        );
        let mut other_action = signed.clone();
        other_action.action = propose("Welcome!", "Hello.");
        assert_eq!(
            other_action.verify(CHAIN, NET),
            Err(AdvisoryError::BadSignature)
        );
        let mut other_account = signed.clone();
        other_account.account = Address([9; 20]);
        assert_eq!(
            other_account.verify(CHAIN, NET),
            Err(AdvisoryError::KeyAddressMismatch)
        );
        let mut other_key = signed;
        other_key.public_key = PublicKey::from_private_key(&key(8)).to_hex();
        assert_eq!(
            other_key.verify(CHAIN, NET),
            Err(AdvisoryError::KeyAddressMismatch)
        );
    }

    #[test]
    fn a_vote_signature_cannot_be_reused_for_the_other_side() {
        let k = key(3);
        let id = proposal_id(&Address([1; 20]), 0);
        let mut signed = SignedAdvisoryAction::sign(
            CHAIN,
            NET,
            &k,
            AdvisoryAction::Vote {
                proposal_id: id,
                approve: true,
            },
            0,
        );
        signed.verify(CHAIN, NET).expect("verifies");
        if let AdvisoryAction::Vote { approve, .. } = &mut signed.action {
            *approve = false;
        }
        assert_eq!(signed.verify(CHAIN, NET), Err(AdvisoryError::BadSignature));
    }

    #[test]
    fn length_caps_are_byte_caps_at_exactly_the_limit() {
        assert!(validate_title(&"a".repeat(MAX_TITLE_BYTES)).is_ok());
        assert_eq!(
            validate_title(&"a".repeat(MAX_TITLE_BYTES + 1)),
            Err(TextError::TooLong {
                got: MAX_TITLE_BYTES + 1,
                max: MAX_TITLE_BYTES
            })
        );
        assert!(validate_summary(&"a".repeat(MAX_SUMMARY_BYTES)).is_ok());
        assert!(validate_summary(&"a".repeat(MAX_SUMMARY_BYTES + 1)).is_err());
        // Bytes, not characters: 54 three-byte characters = 162 bytes.
        assert!(validate_title(&"€".repeat(54)).is_err());
        assert!(validate_title(&"€".repeat(53)).is_ok());
        assert_eq!(validate_title(""), Err(TextError::Empty));
        assert_eq!(validate_title("   "), Err(TextError::Empty));
        assert_eq!(validate_summary(" \n "), Err(TextError::Empty));
    }

    #[test]
    fn control_and_bidi_characters_are_refused() {
        for cp in [
            0x00u32, 0x07, 0x09, 0x0D, 0x1B, 0x1F, 0x7F, 0x85, 0x9F, 0x202A, 0x202E, 0x2066, 0x2069,
        ] {
            let c = char::from_u32(cp).unwrap();
            let text = format!("a{c}b");
            assert_eq!(
                validate_title(&text),
                Err(TextError::ForbiddenChar(cp)),
                "title U+{cp:04X}"
            );
            assert_eq!(
                validate_summary(&text),
                Err(TextError::ForbiddenChar(cp)),
                "summary U+{cp:04X}"
            );
        }
        assert!(validate_summary("line one\nline two").is_ok());
        assert_eq!(
            validate_title("line one\nline two"),
            Err(TextError::ForbiddenChar(0x0A))
        );
        assert!(validate_title("Café, 東京, emoji 🌧").is_ok());
    }

    #[test]
    fn signed_actions_with_bad_text_do_not_verify() {
        let k = key(2);
        let signed = SignedAdvisoryAction::sign(CHAIN, NET, &k, propose(&"x".repeat(161), "ok"), 0);
        assert!(matches!(
            signed.verify(CHAIN, NET),
            Err(AdvisoryError::Title(TextError::TooLong { .. }))
        ));
        let vote = SignedAdvisoryAction::sign(
            CHAIN,
            NET,
            &k,
            AdvisoryAction::Vote {
                proposal_id: "LAW-001".into(),
                approve: true,
            },
            0,
        );
        assert_eq!(vote.verify(CHAIN, NET), Err(AdvisoryError::BadProposalId));
    }

    fn tx_with(data: String, sender: Address, nonce: u64) -> Transaction {
        let mut tx = Transaction::new(Vec::new(), Vec::new());
        tx.tx_type = TransactionType::AdvisoryGovernance;
        tx.sender_address = sender;
        tx.nonce = nonce;
        tx.data = Some(data);
        tx
    }

    #[test]
    fn parse_refuses_unknown_members_wrong_sender_wrong_nonce_and_size() {
        let k = key(5);
        let signed = SignedAdvisoryAction::sign(CHAIN, NET, &k, propose("T", "S"), 3);
        let json = serde_json::to_string(&signed).unwrap();
        let account = signed.account;
        assert!(SignedAdvisoryAction::parse(&tx_with(json.clone(), account, 3)).is_some());
        assert!(SignedAdvisoryAction::parse(&tx_with(json.clone(), Address([4; 20]), 3)).is_none());
        assert!(SignedAdvisoryAction::parse(&tx_with(json.clone(), account, 4)).is_none());
        // A parameter-shaped member is refused at the top level and inside the action.
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v["parameter"] = serde_json::json!("emission_rate");
        assert!(SignedAdvisoryAction::parse(&tx_with(v.to_string(), account, 3)).is_none());
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v["action"]["propose"]["value"] = serde_json::json!("0");
        assert!(SignedAdvisoryAction::parse(&tx_with(v.to_string(), account, 3)).is_none());
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v["action"]["propose"]["deposit"] = serde_json::json!(1);
        assert!(SignedAdvisoryAction::parse(&tx_with(v.to_string(), account, 3)).is_none());
        let big = format!("{json}{}", " ".repeat(MAX_ADVISORY_PAYLOAD_BYTES));
        assert!(SignedAdvisoryAction::parse(&tx_with(big, account, 3)).is_none());
        let mut wrong_type = tx_with(json, account, 3);
        wrong_type.tx_type = TransactionType::GovernanceProposal;
        assert!(SignedAdvisoryAction::parse(&wrong_type).is_none());
    }

    #[test]
    fn proposal_ids_are_chain_derived_and_well_formed() {
        let a = proposal_id(&Address([1; 20]), 0);
        assert!(is_advisory_proposal_id(&a));
        assert_ne!(a, proposal_id(&Address([1; 20]), 1));
        assert_ne!(a, proposal_id(&Address([2; 20]), 0));
        assert!(!is_advisory_proposal_id("adv-0123"));
        assert!(!is_advisory_proposal_id("adv-0123456789ABCDEF"));
        assert!(!is_advisory_proposal_id("LAW-ADV-0123456789abcdef"));
    }

    #[test]
    fn outcome_rule() {
        assert_eq!(outcome(0, 0), AdvisoryStatus::FailedQuorum);
        assert_eq!(outcome(2, 0), AdvisoryStatus::FailedQuorum);
        assert_eq!(outcome(3, 0), AdvisoryStatus::Passed);
        assert_eq!(outcome(2, 1), AdvisoryStatus::Passed); // 66%
        assert_eq!(outcome(1, 2), AdvisoryStatus::Rejected);
        assert_eq!(outcome(51, 49), AdvisoryStatus::Passed);
        assert_eq!(outcome(50, 50), AdvisoryStatus::Rejected);
        assert_eq!(outcome(u64::MAX, u64::MAX), AdvisoryStatus::Rejected);
    }

    fn record(deadline: u64) -> AdvisoryProposal {
        AdvisoryProposal {
            id: proposal_id(&Address([1; 20]), 0),
            proposer: Address([1; 20]),
            title: "T".into(),
            summary: "S".into(),
            created_at_block: 10,
            voting_deadline: deadline,
            deposit: Balance::new(u128::from(ADVISORY_DEPOSIT)),
            eligible_voters: [Address([1; 20]), Address([2; 20]), Address([3; 20])]
                .into_iter()
                .collect(),
            eligible_count: 3,
            votes: BTreeMap::new(),
            yes_votes: 0,
            no_votes: 0,
            status: AdvisoryStatus::Active,
            finalized_at_block: None,
        }
    }

    #[test]
    fn votes_need_snapshot_membership_once_before_the_deadline() {
        let mut p = record(100);
        assert!(record_vote(&mut p, &Address([9; 20]), true, 50).is_err());
        record_vote(&mut p, &Address([2; 20]), true, 50).unwrap();
        assert!(record_vote(&mut p, &Address([2; 20]), false, 51).is_err());
        record_vote(&mut p, &Address([3; 20]), false, 99).unwrap();
        assert!(record_vote(&mut p, &Address([1; 20]), true, 100).is_err());
        assert_eq!((p.yes_votes, p.no_votes), (1, 1));
    }

    #[test]
    fn finalize_records_only_status_and_returns_only_the_deposit() {
        let mut p = record(100);
        assert!(finalize(&mut p, 99).is_err());
        for v in 1..=3u8 {
            record_vote(&mut p, &Address([v; 20]), true, 20).unwrap();
        }
        let refund = finalize(&mut p, 100).unwrap();
        assert_eq!(refund, Balance::new(u128::from(ADVISORY_DEPOSIT)));
        assert_eq!(p.status, AdvisoryStatus::Passed);
        assert!(p.eligible_voters.is_empty() && p.votes.is_empty());
        assert_eq!((p.yes_votes, p.eligible_count), (3, 3));
        assert!(finalize(&mut p, 101).is_err());

        let mut q = record(100);
        assert_eq!(finalize(&mut q, 150).unwrap(), Balance::new(0));
        assert_eq!(q.status, AdvisoryStatus::FailedQuorum);
    }

    mod passkey {
        use super::*;
        use crate::accounts::passkey::test_support::SoftwareAuthenticator;
        use crate::accounts::passkey::{FLAG_USER_PRESENT, P256Error};

        fn authenticator(seed: u8) -> SoftwareAuthenticator {
            let mut secret = [seed; 32];
            secret[0] = 0x01; // keep the scalar in range
            SoftwareAuthenticator::new(&secret, "wrfcoin.org")
        }

        fn vote(id: &str, approve: bool) -> AdvisoryAction {
            AdvisoryAction::Vote {
                proposal_id: id.into(),
                approve,
            }
        }

        #[test]
        fn a_passkey_action_verifies_and_every_bound_field_breaks_it() {
            let a = authenticator(7);
            let signed = SignedAdvisoryAction::sign_passkey(CHAIN, NET, &a, propose("Welcome", "Hello."), 4);
            assert_eq!(signed.account, a.account());
            signed.verify(CHAIN, NET).expect("verifies");
            let challenge_mismatch = Err(AdvisoryError::Passkey(PasskeyError::ChallengeMismatch));
            assert_eq!(signed.verify("777", NET), challenge_mismatch);
            assert_eq!(signed.verify(CHAIN, "devnet"), challenge_mismatch);
            let mut other_nonce = signed.clone();
            other_nonce.nonce = 5;
            assert_eq!(other_nonce.verify(CHAIN, NET), challenge_mismatch);
            let mut other_action = signed.clone();
            other_action.action = propose("Welcome!", "Hello.");
            assert_eq!(other_action.verify(CHAIN, NET), challenge_mismatch);
            // Another account (an ed25519 one, or another passkey's).
            let mut other_account = signed.clone();
            other_account.account = Address([9; 20]);
            assert_eq!(other_account.verify(CHAIN, NET), Err(AdvisoryError::KeyAddressMismatch));
            let mut other_key = signed.clone();
            other_key.passkey.as_mut().unwrap().public_key_p256 =
                hex::encode(authenticator(8).key.public_key().to_raw_xy());
            assert_eq!(other_key.verify(CHAIN, NET), Err(AdvisoryError::KeyAddressMismatch));
            // A vote cannot be flipped or moved to another proposal.
            let id = proposal_id(&Address([1; 20]), 0);
            let v = SignedAdvisoryAction::sign_passkey(CHAIN, NET, &a, vote(&id, true), 0);
            v.verify(CHAIN, NET).unwrap();
            let mut flipped = v.clone();
            flipped.action = vote(&id, false);
            assert_eq!(flipped.verify(CHAIN, NET), challenge_mismatch);
            let mut moved = v;
            moved.action = vote(&proposal_id(&Address([1; 20]), 1), true);
            assert_eq!(moved.verify(CHAIN, NET), challenge_mismatch);
        }

        #[test]
        fn the_challenge_is_the_hash_of_the_ed25519_message() {
            let account = Address([3; 20]);
            let action = propose("T", "S");
            let message = advisory_action_message(CHAIN, NET, &account, &action, 2);
            assert_eq!(
                advisory_passkey_challenge(CHAIN, NET, &account, &action, 2),
                crate::accounts::passkey::passkey_transfer_challenge(&message)
            );
        }

        #[test]
        fn wrong_relying_party_or_bad_assertion_is_refused() {
            let a = authenticator(7);
            let action = propose("T", "S");
            let challenge = advisory_passkey_challenge(CHAIN, NET, &a.account(), &action, 0);
            // A phishing origin can only obtain assertions for its own RP ID.
            // Named with the victim's account: the credential derives another
            // account (the RP ID hash is folded into the address).
            let mut evil = authenticator(7);
            evil.rp_id_hash = Sha256::digest(b"wrfcoin.org.evil.example").into();
            evil.origin = "https://wrfcoin.org.evil.example".into();
            let mut phished = SignedAdvisoryAction::with_passkey(
                a.account(),
                action.clone(),
                0,
                evil.authorization_for(&evil.assert(&challenge)),
            );
            assert_eq!(phished.verify(CHAIN, NET), Err(AdvisoryError::KeyAddressMismatch));
            // ...and claiming the real RP ID hash over evil authenticatorData
            // fails the RP ID check.
            phished.passkey.as_mut().unwrap().rp_id_hash = hex::encode(a.rp_id_hash);
            assert_eq!(
                phished.verify(CHAIN, NET),
                Err(AdvisoryError::Passkey(PasskeyError::RpIdMismatch))
            );
            // Registration ceremony type.
            let mut created = a.client_data_json(&challenge);
            created = String::from_utf8(created).unwrap().replace("webauthn.get", "webauthn.create").into_bytes();
            let wrong_type = SignedAdvisoryAction::with_passkey(
                a.account(), action.clone(), 0, a.authorization_for(&a.assert_with(a.authenticator_data(), created)),
            );
            assert!(matches!(
                wrong_type.verify(CHAIN, NET),
                Err(AdvisoryError::Passkey(PasskeyError::WrongCeremonyType { .. }))
            ));
            // User present but not verified.
            let mut no_uv = authenticator(7);
            no_uv.flags = FLAG_USER_PRESENT;
            let s = SignedAdvisoryAction::sign_passkey(CHAIN, NET, &no_uv, action.clone(), 0);
            assert_eq!(s.verify(CHAIN, NET), Err(AdvisoryError::Passkey(PasskeyError::UserNotVerified)));
            // A tampered signature.
            let mut bad_sig = SignedAdvisoryAction::sign_passkey(CHAIN, NET, &a, action.clone(), 0);
            let other = SignedAdvisoryAction::sign_passkey(CHAIN, NET, &a, propose("U", "S"), 0);
            bad_sig.passkey.as_mut().unwrap().signature = other.passkey.unwrap().signature;
            assert!(matches!(bad_sig.verify(CHAIN, NET), Err(AdvisoryError::Passkey(PasskeyError::Curve(_)))));
            // A high-s signature (the malleated twin) is refused.
            let good = SignedAdvisoryAction::sign_passkey(CHAIN, NET, &a, action.clone(), 0);
            let der = hex::decode(&good.passkey.as_ref().unwrap().signature).unwrap();
            let high_s = crate::accounts::passkey::P256Signature::from_der(&der).unwrap().malleable_twin().unwrap().to_der_bytes();
            let mut malleated = good.clone();
            malleated.passkey.as_mut().unwrap().signature = hex::encode(high_s);
            assert_eq!(
                malleated.verify(CHAIN, NET),
                Err(AdvisoryError::Passkey(PasskeyError::Curve(P256Error::HighS)))
            );
            // Malformed hex.
            let mut junk = good;
            junk.passkey.as_mut().unwrap().rp_id_hash = "zz".into();
            assert!(matches!(junk.verify(CHAIN, NET), Err(AdvisoryError::Malformed(_))));
        }

        #[test]
        fn exactly_one_authorization_form() {
            let a = authenticator(7);
            let mut both = SignedAdvisoryAction::sign_passkey(CHAIN, NET, &a, propose("T", "S"), 0);
            let ed = SignedAdvisoryAction::sign(CHAIN, NET, &key(1), propose("T", "S"), 0);
            both.public_key = ed.public_key.clone();
            both.signature = ed.signature.clone();
            assert_eq!(both.verify(CHAIN, NET), Err(AdvisoryError::AuthorizationForm));
            let mut neither = ed.clone();
            neither.signature.clear();
            assert_eq!(neither.verify(CHAIN, NET), Err(AdvisoryError::AuthorizationForm));
            // parse refuses both forms too.
            let account = both.account;
            assert!(SignedAdvisoryAction::parse(&tx_with(serde_json::to_string(&both).unwrap(), account, 0)).is_none());
            let only = SignedAdvisoryAction::sign_passkey(CHAIN, NET, &a, propose("T", "S"), 0);
            let parsed = SignedAdvisoryAction::parse(&tx_with(serde_json::to_string(&only).unwrap(), account, 0));
            assert_eq!(parsed, Some(only));
        }

        #[test]
        fn one_encoding_per_action() {
            let a = authenticator(7);
            let pk = SignedAdvisoryAction::sign_passkey(CHAIN, NET, &a, propose("T", "S"), 0);
            let ed = SignedAdvisoryAction::sign(CHAIN, NET, &key(1), propose("T", "S"), 0);
            let parses = |v: &serde_json::Value, account: Address| {
                SignedAdvisoryAction::parse(&tx_with(v.to_string(), account, 0)).is_some()
            };
            let pk_json = serde_json::to_value(&pk).unwrap();
            let ed_json = serde_json::to_value(&ed).unwrap();
            assert!(parses(&pk_json, pk.account) && parses(&ed_json, ed.account), "canonical forms parse");
            // Explicit `"passkey": null` on an ed25519 payload.
            let mut v = ed_json.clone();
            v["passkey"] = serde_json::Value::Null;
            assert!(!parses(&v, ed.account));
            // Explicit empty ed25519 members on a passkey payload.
            for member in ["public_key", "signature"] {
                let mut v = pk_json.clone();
                v[member] = serde_json::json!("");
                assert!(!parses(&v, pk.account), "{member}: \"\"");
            }
            // Uppercase hex anywhere: refused by parse and by verify.
            for field in ["public_key_p256", "rp_id_hash", "authenticator_data", "client_data_json", "signature"] {
                let mut v = pk_json.clone();
                let upper = v["passkey"][field].as_str().unwrap().to_uppercase();
                v["passkey"][field] = serde_json::json!(upper);
                assert!(!parses(&v, pk.account), "passkey.{field} uppercase");
                let mut s = pk.clone();
                let p = s.passkey.as_mut().unwrap();
                match field {
                    "public_key_p256" => p.public_key_p256 = upper,
                    "rp_id_hash" => p.rp_id_hash = upper,
                    "authenticator_data" => p.authenticator_data = upper,
                    "client_data_json" => p.client_data_json = upper,
                    _ => p.signature = upper,
                }
                assert!(matches!(s.verify(CHAIN, NET), Err(AdvisoryError::Malformed(_))), "verify passkey.{field}");
            }
            for member in ["public_key", "signature"] {
                let mut v = ed_json.clone();
                let upper = v[member].as_str().unwrap().to_uppercase();
                v[member] = serde_json::json!(upper.clone());
                assert!(!parses(&v, ed.account), "{member} uppercase");
                let mut v = ed_json.clone();
                v[member] = serde_json::json!(format!(" {}", ed_json[member].as_str().unwrap()));
                assert!(!parses(&v, ed.account), "{member} with whitespace");
            }
            let mut s = ed.clone();
            s.signature = s.signature.to_uppercase();
            assert!(matches!(s.verify(CHAIN, NET), Err(AdvisoryError::Malformed(_))));
        }

        #[test]
        fn passkey_payload_json_is_strict_and_ed25519_json_is_unchanged() {
            let a = authenticator(7);
            let signed = SignedAdvisoryAction::sign_passkey(CHAIN, NET, &a, propose("T", "S"), 0);
            let json = serde_json::to_string(&signed).unwrap();
            assert!(!json.contains("\"public_key\"") && !json.contains("\"signature\":\"\""));
            let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
            v["passkey"]["parameter"] = serde_json::json!("emission_rate");
            assert!(SignedAdvisoryAction::parse(&tx_with(v.to_string(), a.account(), 0)).is_none());
            // ed25519: exactly the member set it had before the passkey option.
            let ed = SignedAdvisoryAction::sign(CHAIN, NET, &key(1), propose("T", "S"), 0);
            let v: serde_json::Value = serde_json::to_value(&ed).unwrap();
            let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
            assert_eq!(keys, ["account", "action", "nonce", "public_key", "signature"]);
        }
    }
}
