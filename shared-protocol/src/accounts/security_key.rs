//! Security-key wallets: a passphrase (ed25519) account that also requires a
//! FIDO2 / WebAuthn P-256 security key to spend (`security_key_spend`).
//!
//! Design note: `docs/design/security-key-wallets.md`.
//!
//! # The rule
//!
//! [`crate::accounts::authorization::AuthorizationRule::SignerPlusSecurityKey`]
//! carries a [`SecurityKeyPolicy`]: one or two registered keys, the escape
//! delay, and a pending escape if one was started. A debit from such an account
//! needs the account's own ed25519 signature (the signer-is-sender rule,
//! unchanged) AND a WebAuthn assertion from one registered key whose challenge
//! is [`spend_challenge`] over the exact ed25519 debit preimage.
//!
//! # House patterns this follows
//!
//! - **Deny by default.** Every action refuses unless the feature is active at
//!   the action's height, the nonce is the account's next nonce, the phrase key
//!   derives the account, its signature verifies over the domain-separated
//!   action preimage, and exactly the assertions the action needs are present
//!   and verify. Validation precedes mutation; a refused action changes nothing.
//! - **Block-height windows.** The escape matures at a height, never a time
//!   (same reasoning as `recovery.rs`): every node agrees on heights.
//! - **Domain separation.** [`SECURITY_KEY_ACTION_DOMAIN_TAG`] and
//!   [`SECURITY_KEY_SPEND_DOMAIN_TAG`] are distinct from each other and from the
//!   passkey transfer tag, so no assertion made for one purpose verifies for
//!   another. Action preimages bind chain id, network id, account and nonce.
//! - **Byte-absent when unused.** The rule lives in
//!   `State::authorization_declarations`, which no live chain populates, and can
//!   only be written once [`security_key_active`] holds. Before activation the
//!   state-root section stays absent and every spend path is unchanged.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::accounts::authorization::{
    AccountAuthorization, AccountAuthorizationDeclaration, AuthorizationRule, dispatch_active,
};
use crate::accounts::passkey::{
    CONSENSUS_WEBAUTHN_POLICY, MAX_WEBAUTHN_FIELD_LEN, P256PublicKey, PasskeyCredential,
    PasskeyError, RAW_XY_LEN, RP_ID_HASH_LEN, WebAuthnAssertion, verify_webauthn,
};
use crate::common::types::Address;
use crate::crypto::chain_binding::{ConsensusFeature, feature_active, feature_scheduled};
use crate::crypto::hash::hash_bytes;
use crate::crypto::signature::{PublicKey, Signature};

/// Domain tag of the owner-signed action preimage ([`action_message`]).
pub const SECURITY_KEY_ACTION_DOMAIN_TAG: &[u8] = b"wrfcoin.security_key_action.v1";
/// Domain tag of the spend challenge ([`spend_challenge`]).
pub const SECURITY_KEY_SPEND_DOMAIN_TAG: &[u8] = b"wrfcoin.security_key_spend.v1";
/// The JSON member of a transaction's `data` that carries a spend assertion.
pub const SECURITY_KEY_AUTH_MEMBER: &str = "security_key_auth";
/// At most two registered keys: a main key and a spare.
pub const MAX_SECURITY_KEYS: usize = 2;
/// Upper bound on a stored credential id (a public handle; YubiKeys use 64-128).
pub const MAX_CREDENTIAL_ID_LEN: usize = 255;
/// Shortest allowed escape delay (~24 h at 77 s/block).
pub const ESCAPE_DELAY_MIN_BLOCKS: u64 = 1_123;
/// Escape delay clients offer by default (~7 days at 77 s/block).
pub const ESCAPE_DELAY_DEFAULT_BLOCKS: u64 = 7_855;
/// Longest allowed escape delay (~30 days at 77 s/block).
pub const ESCAPE_DELAY_MAX_BLOCKS: u64 = 33_663;
/// The most assertions any action carries (`add_key`: an existing key and the new key).
pub const MAX_ACTION_ASSERTIONS: usize = 2;
/// Upper bound on a DER P-256 signature (72 bytes; a little slack).
const MAX_DER_SIGNATURE_LEN: usize = 80;

/// Whether `security_key_spend` is in force at `height`. It rides on the
/// per-account dispatch, so both must be active.
pub fn security_key_active(height: u64) -> bool {
    feature_active(ConsensusFeature::SecurityKeySpend, height) && dispatch_active(height)
}

/// Whether this chain schedules `security_key_spend` at all (mempool policy
/// only; block validity and apply use [`security_key_active`]).
pub fn security_key_scheduled() -> bool {
    feature_scheduled(ConsensusFeature::SecurityKeySpend)
}

/// One registered security key, as stored in consensus state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] // secret-leak-ok: a WebAuthn credential id and a P-256 public key are public by specification; no field is secret material
pub struct RegisteredSecurityKey {
    /// Raw `X || Y` P-256 public key (64 bytes).
    pub public_key_p256: Vec<u8>,
    /// `SHA-256(rpId)` the credential was created for (32 bytes).
    pub rp_id_hash: Vec<u8>,
    /// The authenticator's credential id: a PUBLIC handle a client passes in
    /// `allowCredentials`. Stored so any wallet holding the phrase can ask the
    /// key for an assertion without a local copy.
    pub credential_id: Vec<u8>,
}

/// An escape started with the phrase key alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingEscape {
    pub initiated_at_height: u64,
    /// From this height the rule lapses and the account is signer-is-sender.
    pub executable_at_height: u64,
}

/// The security-key policy of one account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityKeyPolicy {
    pub keys: Vec<RegisteredSecurityKey>,
    pub escape_delay_blocks: u64,
    pub pending_escape: Option<PendingEscape>,
}

impl SecurityKeyPolicy {
    /// Whether a phrase-only escape has run its full delay at `height`.
    pub fn escape_matured(&self, height: u64) -> bool {
        self.pending_escape
            .is_some_and(|escape| height >= escape.executable_at_height)
    }

    /// Whether the policy still binds spends at `height`.
    pub fn enforced_at(&self, height: u64) -> bool {
        !self.escape_matured(height)
    }

    /// The registered key with this raw public key, if any.
    pub fn key(&self, public_key_p256: &[u8]) -> Option<&RegisteredSecurityKey> {
        self.keys.iter().find(|k| k.public_key_p256 == public_key_p256)
    }
}

/// Hex wire form of a key, as clients send it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] // secret-leak-ok: public key and public credential handle only
pub struct SecurityKeyWire {
    pub public_key_p256: String,
    pub rp_id_hash: String,
    pub credential_id: String,
}

/// Hex wire form of one WebAuthn assertion by a registered key. Used both as
/// the `security_key_auth` member of a spend and inside actions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityKeyAssertionWire {
    /// Which key made it (raw `X || Y`, hex). Must be registered (or, for a
    /// key being enabled/added, the new key).
    pub public_key_p256: String,
    pub authenticator_data: String,
    pub client_data_json: String,
    /// DER, low-s.
    pub signature: String,
}

/// What an action asks the state machine to do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SecurityKeyActionBody {
    /// Turn the rule on with its first key (assertion: the new key).
    Enable { key: SecurityKeyWire, escape_delay_blocks: u64 },
    /// Add a spare key (assertions: an existing key AND the new key).
    AddKey { key: SecurityKeyWire },
    /// Remove one key (assertion: any registered key). Removing the last key
    /// turns the rule off.
    RemoveKey { public_key_p256: String },
    /// Turn the rule off now (assertion: any registered key).
    Disable,
    /// Phrase-only: start the time lock. No assertion.
    BeginEscape,
    /// Cancel a pending escape (assertion: any registered key).
    CancelEscape,
}

/// A signed security-key action: the JSON `data` of a `SecurityKeyAction`
/// transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityKeyActionPayload {
    /// The protected account (hex address).
    pub account: String,
    /// Must equal the account's declaration `next_nonce`.
    pub nonce: u64,
    pub action: SecurityKeyActionBody,
    /// Hex ed25519 public key; must derive `account`.
    pub owner_public_key: String,
    /// Hex ed25519 signature over [`action_message`].
    pub owner_signature: String,
    #[serde(default)]
    pub key_assertions: Vec<SecurityKeyAssertionWire>,
}

/// Every way an action or a spend is refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SecurityKeyError {
    #[error("security_key_spend is not active at height {height}")]
    FeatureNotActive { height: u64 },
    #[error("malformed {0}")]
    Malformed(&'static str),
    #[error("stale action nonce: expected {expected}, got {got}")]
    StaleNonce { expected: u64, got: u64 },
    #[error("account nonce or generation exhausted")]
    CounterExhausted,
    #[error("owner public key does not derive the account")]
    OwnerKeyMismatch,
    #[error("owner signature does not verify over the action")]
    BadOwnerSignature,
    #[error("a security key is already enabled on this account")]
    AlreadyEnabled,
    #[error("no security key is enabled on this account")]
    NotEnabled,
    #[error("the account declared a different authorization rule")]
    ConflictingRule,
    #[error("escape delay {blocks} is outside {min}..={max} blocks")]
    InvalidEscapeDelay { blocks: u64, min: u64, max: u64 },
    #[error("at most {max} security keys")]
    TooManyKeys { max: usize },
    #[error("that security key is already registered")]
    DuplicateKey,
    #[error("that security key is not registered on this account")]
    KeyNotRegistered,
    #[error("an escape is pending; cancel it first")]
    EscapePending,
    #[error("no escape is pending")]
    NoPendingEscape,
    #[error("this action needs an assertion from {0}")]
    MissingAssertion(&'static str),
    #[error("an assertion this action does not use was supplied")]
    UnexpectedAssertion,
    #[error("two assertions from the same key")]
    DuplicateAssertion,
    #[error("security key assertion refused: {0}")]
    Assertion(PasskeyError),
    #[error("a security key assertion is required to spend from this account")]
    ProofRequired,
}

fn push(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(bytes);
}

fn decode_hex(value: &str, max_bytes: usize, field: &'static str) -> Result<Vec<u8>, SecurityKeyError> {
    let value = value.trim();
    if value.len() > max_bytes.saturating_mul(2) {
        return Err(SecurityKeyError::Malformed(field));
    }
    hex::decode(value).map_err(|_| SecurityKeyError::Malformed(field))
}

fn decode_public_key(value: &str) -> Result<Vec<u8>, SecurityKeyError> {
    let bytes = decode_hex(value, RAW_XY_LEN, "public_key_p256")?;
    let xy: [u8; RAW_XY_LEN] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| SecurityKeyError::Malformed("public_key_p256"))?;
    P256PublicKey::from_raw_xy(&xy).map_err(|_| SecurityKeyError::Malformed("public_key_p256"))?;
    Ok(bytes)
}

impl SecurityKeyWire {
    /// Validate and decode into the stored form.
    pub fn decode(&self) -> Result<RegisteredSecurityKey, SecurityKeyError> {
        let public_key_p256 = decode_public_key(&self.public_key_p256)?;
        let rp_id_hash = decode_hex(&self.rp_id_hash, RP_ID_HASH_LEN, "rp_id_hash")?;
        if rp_id_hash.len() != RP_ID_HASH_LEN {
            return Err(SecurityKeyError::Malformed("rp_id_hash"));
        }
        let credential_id = decode_hex(&self.credential_id, MAX_CREDENTIAL_ID_LEN, "credential_id")?;
        if credential_id.is_empty() {
            return Err(SecurityKeyError::Malformed("credential_id"));
        }
        Ok(RegisteredSecurityKey {
            public_key_p256,
            rp_id_hash,
            credential_id,
        })
    }
}

impl RegisteredSecurityKey {
    /// The hex wire form.
    pub fn to_wire(&self) -> SecurityKeyWire {
        SecurityKeyWire {
            public_key_p256: hex::encode(&self.public_key_p256),
            rp_id_hash: hex::encode(&self.rp_id_hash),
            credential_id: hex::encode(&self.credential_id),
        }
    }

    fn credential(&self) -> Result<PasskeyCredential, SecurityKeyError> {
        Ok(PasskeyCredential {
            credential_id: self.credential_id.clone(),
            public_key_p256: self
                .public_key_p256
                .as_slice()
                .try_into()
                .map_err(|_| SecurityKeyError::Malformed("stored public_key_p256"))?,
            rp_id_hash: self
                .rp_id_hash
                .as_slice()
                .try_into()
                .map_err(|_| SecurityKeyError::Malformed("stored rp_id_hash"))?,
            // No per-credential counter is kept on chain (design note): replay
            // is prevented by the nonce bound into every challenge.
            sign_count: 0,
        })
    }
}

impl SecurityKeyAssertionWire {
    /// The raw public key this assertion names.
    pub fn public_key(&self) -> Result<Vec<u8>, SecurityKeyError> {
        decode_public_key(&self.public_key_p256)
    }

    fn assertion(&self) -> Result<WebAuthnAssertion, SecurityKeyError> {
        Ok(WebAuthnAssertion {
            authenticator_data: decode_hex(
                &self.authenticator_data,
                MAX_WEBAUTHN_FIELD_LEN,
                "authenticator_data",
            )?,
            client_data_json: decode_hex(
                &self.client_data_json,
                MAX_WEBAUTHN_FIELD_LEN,
                "client_data_json",
            )?,
            signature: decode_hex(&self.signature, MAX_DER_SIGNATURE_LEN, "signature")?,
        })
    }

    /// Verify this assertion as made by `key` over `challenge`, under the
    /// consensus WebAuthn policy (user present AND verified, low-s).
    pub fn verify(&self, key: &RegisteredSecurityKey, challenge: &[u8; 32]) -> Result<(), SecurityKeyError> {
        if self.public_key()? != key.public_key_p256 {
            return Err(SecurityKeyError::KeyNotRegistered);
        }
        verify_webauthn(
            &key.credential()?,
            &self.assertion()?,
            challenge,
            CONSENSUS_WEBAUTHN_POLICY,
        )
        .map(|_| ())
        .map_err(SecurityKeyError::Assertion)
    }
}

/// Stable, non-serde encoding of an action body for the signing preimage.
/// Hex fields are decoded first, so case and whitespace never change signed
/// bytes, and a malformed field refuses the action.
pub fn encode_action_body(body: &SecurityKeyActionBody) -> Result<Vec<u8>, SecurityKeyError> {
    let mut out = Vec::new();
    match body {
        SecurityKeyActionBody::Enable { key, escape_delay_blocks } => {
            let key = key.decode()?;
            push(&mut out, &[1]);
            push(&mut out, &key.public_key_p256);
            push(&mut out, &key.rp_id_hash);
            push(&mut out, &key.credential_id);
            push(&mut out, &escape_delay_blocks.to_le_bytes());
        },
        SecurityKeyActionBody::AddKey { key } => {
            let key = key.decode()?;
            push(&mut out, &[2]);
            push(&mut out, &key.public_key_p256);
            push(&mut out, &key.rp_id_hash);
            push(&mut out, &key.credential_id);
        },
        SecurityKeyActionBody::RemoveKey { public_key_p256 } => {
            push(&mut out, &[3]);
            push(&mut out, &decode_public_key(public_key_p256)?);
        },
        SecurityKeyActionBody::Disable => push(&mut out, &[4]),
        SecurityKeyActionBody::BeginEscape => push(&mut out, &[5]),
        SecurityKeyActionBody::CancelEscape => push(&mut out, &[6]),
    }
    Ok(out)
}

/// The exact bytes the phrase key signs for an action. Every component is
/// length-prefixed; the chain identity is bound so an action signed for one
/// network never applies on another.
pub fn action_message(
    chain_id: &str,
    network_id: &str,
    account: &Address,
    nonce: u64,
    body: &SecurityKeyActionBody,
) -> Result<Vec<u8>, SecurityKeyError> {
    let encoded = encode_action_body(body)?;
    let mut message = Vec::new();
    push(&mut message, SECURITY_KEY_ACTION_DOMAIN_TAG);
    push(&mut message, chain_id.as_bytes());
    push(&mut message, network_id.as_bytes());
    push(&mut message, &account.0);
    push(&mut message, &nonce.to_le_bytes());
    push(&mut message, &encoded);
    Ok(message)
}

/// The WebAuthn challenge a key signs to co-sign an action: `SHA3-256` of the
/// same preimage the phrase key signs.
pub fn action_challenge(action_message: &[u8]) -> [u8; 32] {
    *hash_bytes(action_message).as_bytes()
}

/// The WebAuthn challenge a key signs to co-sign a spend: binds the account
/// and the exact ed25519 debit preimage (which already binds chain, recipient,
/// amount and nonce).
pub fn spend_challenge(account: &Address, debit_message: &[u8]) -> [u8; 32] {
    let mut preimage = Vec::with_capacity(debit_message.len() + 96);
    push(&mut preimage, SECURITY_KEY_SPEND_DOMAIN_TAG);
    push(&mut preimage, &account.0);
    push(&mut preimage, debit_message);
    *hash_bytes(&preimage).as_bytes()
}

/// Check the security-key half of a spend under `policy`. The caller has
/// already checked the ed25519 half.
pub fn verify_spend(
    policy: &SecurityKeyPolicy,
    account: &Address,
    debit_message: &[u8],
    proof: Option<&SecurityKeyAssertionWire>,
) -> Result<(), SecurityKeyError> {
    let proof = proof.ok_or(SecurityKeyError::ProofRequired)?;
    let named = proof.public_key()?;
    let key = policy.key(&named).ok_or(SecurityKeyError::KeyNotRegistered)?;
    proof.verify(key, &spend_challenge(account, debit_message))
}

/// Gate for granting spending power to someone else (a session key) on an
/// account protected by a security key: while the account's rule is in force
/// at `height`, the grant's own signed preimage (`grant_message`, which carries
/// its own domain tag) must be co-signed by a registered key, exactly like a
/// spend. Accounts without an enforced rule, and every height below
/// [`security_key_active`], pass unchanged.
pub fn delegation_gate(
    declarations: &BTreeMap<Address, AccountAuthorizationDeclaration>,
    account: &Address,
    grant_message: &[u8],
    proof: Option<&SecurityKeyAssertionWire>,
    height: u64,
) -> Result<(), SecurityKeyError> {
    if !security_key_active(height) {
        return Ok(());
    }
    match declarations.get(account).and_then(stored_policy) {
        Some(policy) if policy.enforced_at(height) => {
            verify_spend(policy, account, grant_message, proof)
        },
        _ => Ok(()),
    }
}

/// Read the optional `security_key_auth` member of a transaction's JSON data.
/// Lenient like `passkey_auth`: anything that is not a well-formed assertion
/// reads as absent, so it can never make a payload unparseable.
pub fn spend_proof_from_tx_data(data: Option<&str>) -> Option<SecurityKeyAssertionWire> {
    let value: serde_json::Value = serde_json::from_str(data?).ok()?;
    serde_json::from_value(value.get(SECURITY_KEY_AUTH_MEMBER)?.clone()).ok()
}

/// The account's policy as stored, if its declared rule is a security-key rule.
pub fn stored_policy(declaration: &AccountAuthorizationDeclaration) -> Option<&SecurityKeyPolicy> {
    match declaration.current.as_ref()?.declared.as_ref()? {
        AuthorizationRule::SignerPlusSecurityKey { policy } => Some(policy),
        _ => None,
    }
}

/// Take exactly the assertions an action needs, in role order. Refuses a
/// duplicate key, a missing role, and any extra assertion.
fn take_assertions<'a>(
    supplied: &'a [SecurityKeyAssertionWire],
    roles: &[&dyn Fn(&[u8]) -> bool],
    role_names: &[&'static str],
) -> Result<Vec<&'a SecurityKeyAssertionWire>, SecurityKeyError> {
    if supplied.len() > MAX_ACTION_ASSERTIONS {
        return Err(SecurityKeyError::UnexpectedAssertion);
    }
    let mut keys = Vec::with_capacity(supplied.len());
    for assertion in supplied {
        let key = assertion.public_key()?;
        if keys.contains(&key) {
            return Err(SecurityKeyError::DuplicateAssertion);
        }
        keys.push(key);
    }
    let mut used = vec![false; supplied.len()];
    let mut picked = Vec::with_capacity(roles.len());
    for (role, name) in roles.iter().zip(role_names) {
        let index = keys
            .iter()
            .enumerate()
            .position(|(i, key)| !used[i] && role(key))
            .ok_or(SecurityKeyError::MissingAssertion(name))?;
        used[index] = true;
        picked.push(&supplied[index]);
    }
    if used.iter().any(|u| !u) {
        return Err(SecurityKeyError::UnexpectedAssertion);
    }
    Ok(picked)
}

/// Apply one signed action to `declarations` at `height`. The single entry
/// point the state machine calls (`TransactionType::SecurityKeyAction` through
/// the governance sink). All validation precedes mutation.
pub fn apply_action(
    declarations: &mut BTreeMap<Address, AccountAuthorizationDeclaration>,
    payload: &SecurityKeyActionPayload,
    height: u64,
    chain_id: &str,
    network_id: &str,
) -> Result<(), SecurityKeyError> {
    if !security_key_active(height) {
        return Err(SecurityKeyError::FeatureNotActive { height });
    }
    let account: Address = payload
        .account
        .trim()
        .parse()
        .map_err(|_| SecurityKeyError::Malformed("account"))?;
    let existing = declarations.get(&account).cloned().unwrap_or_default();
    if payload.nonce != existing.next_nonce {
        return Err(SecurityKeyError::StaleNonce {
            expected: existing.next_nonce,
            got: payload.nonce,
        });
    }
    let next_nonce = existing
        .next_nonce
        .checked_add(1)
        .ok_or(SecurityKeyError::CounterExhausted)?;
    let next_generation = existing
        .generation
        .checked_add(1)
        .ok_or(SecurityKeyError::CounterExhausted)?;

    let owner = PublicKey::from_hex(payload.owner_public_key.trim())
        .map_err(|_| SecurityKeyError::Malformed("owner_public_key"))?;
    if Address::from_public_key(&owner) != account {
        return Err(SecurityKeyError::OwnerKeyMismatch);
    }
    let signature = Signature::from_hex(payload.owner_signature.trim())
        .map_err(|_| SecurityKeyError::Malformed("owner_signature"))?;
    let message = action_message(chain_id, network_id, &account, payload.nonce, &payload.action)?;
    if !owner.verify(&message, &signature) {
        return Err(SecurityKeyError::BadOwnerSignature);
    }
    let challenge = action_challenge(&message);

    // The policy in force: a matured escape counts as "off".
    let stored = match existing.current.as_ref().and_then(|c| c.declared.as_ref()) {
        None | Some(AuthorizationRule::SignerIsSender) => None,
        Some(AuthorizationRule::SignerPlusSecurityKey { policy }) => Some(policy),
        Some(_) => return Err(SecurityKeyError::ConflictingRule),
    };
    let effective = stored.filter(|policy| policy.enforced_at(height));

    let next_policy: Option<SecurityKeyPolicy> = match &payload.action {
        SecurityKeyActionBody::Enable { key, escape_delay_blocks } => {
            if effective.is_some() {
                return Err(SecurityKeyError::AlreadyEnabled);
            }
            if !(ESCAPE_DELAY_MIN_BLOCKS..=ESCAPE_DELAY_MAX_BLOCKS).contains(escape_delay_blocks) {
                return Err(SecurityKeyError::InvalidEscapeDelay {
                    blocks: *escape_delay_blocks,
                    min: ESCAPE_DELAY_MIN_BLOCKS,
                    max: ESCAPE_DELAY_MAX_BLOCKS,
                });
            }
            let new_key = key.decode()?;
            let is_new = |k: &[u8]| k == new_key.public_key_p256.as_slice();
            let picked = take_assertions(&payload.key_assertions, &[&is_new], &["the new key"])?;
            picked[0].verify(&new_key, &challenge)?;
            Some(SecurityKeyPolicy {
                keys: vec![new_key],
                escape_delay_blocks: *escape_delay_blocks,
                pending_escape: None,
            })
        },
        SecurityKeyActionBody::AddKey { key } => {
            let policy = effective.ok_or(SecurityKeyError::NotEnabled)?;
            if policy.pending_escape.is_some() {
                return Err(SecurityKeyError::EscapePending);
            }
            if policy.keys.len() >= MAX_SECURITY_KEYS {
                return Err(SecurityKeyError::TooManyKeys { max: MAX_SECURITY_KEYS });
            }
            let new_key = key.decode()?;
            if policy.key(&new_key.public_key_p256).is_some() {
                return Err(SecurityKeyError::DuplicateKey);
            }
            let is_existing = |k: &[u8]| policy.key(k).is_some();
            let is_new = |k: &[u8]| k == new_key.public_key_p256.as_slice();
            let picked = take_assertions(
                &payload.key_assertions,
                &[&is_existing, &is_new],
                &["a registered key", "the new key"],
            )?;
            let existing_key = policy
                .key(&picked[0].public_key()?)
                .ok_or(SecurityKeyError::KeyNotRegistered)?;
            picked[0].verify(existing_key, &challenge)?;
            picked[1].verify(&new_key, &challenge)?;
            let mut next = policy.clone();
            next.keys.push(new_key);
            Some(next)
        },
        SecurityKeyActionBody::RemoveKey { public_key_p256 } => {
            let policy = effective.ok_or(SecurityKeyError::NotEnabled)?;
            let target = decode_public_key(public_key_p256)?;
            if policy.key(&target).is_none() {
                return Err(SecurityKeyError::KeyNotRegistered);
            }
            verify_one_registered(policy, &payload.key_assertions, &challenge)?;
            let mut next = policy.clone();
            next.keys.retain(|k| k.public_key_p256 != target);
            (!next.keys.is_empty()).then_some(next)
        },
        SecurityKeyActionBody::Disable => {
            let policy = effective.ok_or(SecurityKeyError::NotEnabled)?;
            verify_one_registered(policy, &payload.key_assertions, &challenge)?;
            None
        },
        SecurityKeyActionBody::BeginEscape => {
            let policy = effective.ok_or(SecurityKeyError::NotEnabled)?;
            if !payload.key_assertions.is_empty() {
                return Err(SecurityKeyError::UnexpectedAssertion);
            }
            if policy.pending_escape.is_some() {
                return Err(SecurityKeyError::EscapePending);
            }
            let executable_at_height = height
                .checked_add(policy.escape_delay_blocks)
                .ok_or(SecurityKeyError::CounterExhausted)?;
            let mut next = policy.clone();
            next.pending_escape = Some(PendingEscape {
                initiated_at_height: height,
                executable_at_height,
            });
            Some(next)
        },
        SecurityKeyActionBody::CancelEscape => {
            let policy = effective.ok_or(SecurityKeyError::NotEnabled)?;
            if policy.pending_escape.is_none() {
                return Err(SecurityKeyError::NoPendingEscape);
            }
            verify_one_registered(policy, &payload.key_assertions, &challenge)?;
            let mut next = policy.clone();
            next.pending_escape = None;
            Some(next)
        },
    };

    declarations.insert(
        account,
        AccountAuthorizationDeclaration {
            next_nonce,
            current: next_policy.map(|policy| {
                AccountAuthorization::declaring(AuthorizationRule::SignerPlusSecurityKey { policy })
            }),
            generation: next_generation,
        },
    );
    Ok(())
}

fn verify_one_registered(
    policy: &SecurityKeyPolicy,
    supplied: &[SecurityKeyAssertionWire],
    challenge: &[u8; 32],
) -> Result<(), SecurityKeyError> {
    let is_registered = |k: &[u8]| policy.key(k).is_some();
    let picked = take_assertions(supplied, &[&is_registered], &["a registered key"])?;
    let key = policy
        .key(&picked[0].public_key()?)
        .ok_or(SecurityKeyError::KeyNotRegistered)?;
    picked[0].verify(key, challenge)
}

/// Stateless check of an action's phrase-key half under the runtime chain
/// identity: the key derives the account and signs the action preimage. Used
/// by mempool admission as a spam filter; consensus runs [`apply_action`].
pub fn owner_signature_valid(payload: &SecurityKeyActionPayload) -> bool {
    let identity = crate::crypto::chain_binding::ChainIdentity::active();
    let Ok(account) = payload.account.trim().parse::<Address>() else {
        return false;
    };
    let Ok(owner) = PublicKey::from_hex(payload.owner_public_key.trim()) else {
        return false;
    };
    let Ok(signature) = Signature::from_hex(payload.owner_signature.trim()) else {
        return false;
    };
    let Ok(message) = action_message(
        &identity.chain_id,
        &identity.network_id,
        &account,
        payload.nonce,
        &payload.action,
    ) else {
        return false;
    };
    Address::from_public_key(&owner) == account && owner.verify(&message, &signature)
}

/// How many P-256 verifications an action can force (pricing, fail-expensive).
pub const ACTION_PRICED_P256_VERIFICATIONS: u32 = MAX_ACTION_ASSERTIONS as u32;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::passkey::test_support::SoftwareAuthenticator;
    use crate::crypto::signature::PrivateKey;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn domain_tags_are_distinct_from_each_other_and_from_passkey() {
        let tags: [&[u8]; 3] = [
            SECURITY_KEY_ACTION_DOMAIN_TAG,
            SECURITY_KEY_SPEND_DOMAIN_TAG,
            crate::accounts::passkey::PASSKEY_TRANSFER_AUTH_DOMAIN,
        ];
        for (i, a) in tags.iter().enumerate() {
            for b in tags.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn escape_bounds_are_ordered() {
        const _: () = assert!(ESCAPE_DELAY_MIN_BLOCKS < ESCAPE_DELAY_DEFAULT_BLOCKS);
        const _: () = assert!(ESCAPE_DELAY_DEFAULT_BLOCKS < ESCAPE_DELAY_MAX_BLOCKS);
    }

    #[test]
    fn inactive_feature_refuses_every_action_without_touching_state() {
        // No schedule is installed in the unit-test binary.
        let mut rng = StdRng::seed_from_u64(7);
        let owner = PrivateKey::generate(&mut rng);
        let public = PublicKey::from_private_key(&owner);
        let account = Address::from_public_key(&public);
        let key = SoftwareAuthenticator::new(&[0x11; 32], "wrfcoin.com");
        let wire = SecurityKeyWire {
            public_key_p256: hex::encode(key.key.public_key().to_raw_xy()),
            rp_id_hash: hex::encode(key.rp_id_hash),
            credential_id: "01".into(),
        };
        let body = SecurityKeyActionBody::Enable { key: wire, escape_delay_blocks: ESCAPE_DELAY_DEFAULT_BLOCKS };
        let message = action_message("888", "testnet", &account, 0, &body).unwrap();
        let payload = SecurityKeyActionPayload {
            account: account.to_string(),
            nonce: 0,
            action: body,
            owner_public_key: public.to_hex(),
            owner_signature: owner.sign(&message).to_hex(),
            key_assertions: vec![],
        };
        let mut map = BTreeMap::new();
        for height in [0, 1, 3_400, u64::MAX] {
            assert_eq!(
                apply_action(&mut map, &payload, height, "888", "testnet"),
                Err(SecurityKeyError::FeatureNotActive { height })
            );
        }
        assert!(map.is_empty());
    }

    #[test]
    fn lenient_proof_parse_never_fails_a_payload() {
        assert_eq!(spend_proof_from_tx_data(None), None);
        assert_eq!(spend_proof_from_tx_data(Some("not json")), None);
        assert_eq!(spend_proof_from_tx_data(Some("{\"from\":\"x\"}")), None);
        assert_eq!(spend_proof_from_tx_data(Some("{\"security_key_auth\":{\"public_key_p256\":1}}")), None);
        let ok = spend_proof_from_tx_data(Some(
            "{\"security_key_auth\":{\"public_key_p256\":\"aa\",\"authenticator_data\":\"bb\",\"client_data_json\":\"cc\",\"signature\":\"dd\"}}",
        ));
        assert_eq!(ok.map(|p| p.public_key_p256), Some("aa".to_string()));
    }

    #[test]
    fn encoding_ignores_hex_case_and_refuses_bad_keys() {
        let key = SoftwareAuthenticator::new(&[0x22; 32], "wrfcoin.com");
        let pk = hex::encode(key.key.public_key().to_raw_xy());
        let lower = encode_action_body(&SecurityKeyActionBody::RemoveKey { public_key_p256: pk.clone() }).unwrap();
        let upper = encode_action_body(&SecurityKeyActionBody::RemoveKey { public_key_p256: pk.to_uppercase() }).unwrap();
        assert_eq!(lower, upper);
        // 64 bytes that are not a curve point.
        let bad = SecurityKeyActionBody::RemoveKey { public_key_p256: "00".repeat(64) };
        assert_eq!(encode_action_body(&bad), Err(SecurityKeyError::Malformed("public_key_p256")));
        // Oversized hex is refused before decoding.
        let huge = SecurityKeyActionBody::RemoveKey { public_key_p256: "ab".repeat(10_000) };
        assert_eq!(encode_action_body(&huge), Err(SecurityKeyError::Malformed("public_key_p256")));
    }
}
