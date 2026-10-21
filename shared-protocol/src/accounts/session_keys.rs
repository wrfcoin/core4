//! Temporary, scoped session keys (#2170).
//!
//! A session key lets an account authorize a narrow, expiring set of actions
//! without exposing the primary credential that can move the account's
//! funds — the driving case is a weather station that signs thousands of
//! observations unattended and must never hold a key that can also drain the
//! wallet.
//!
//! ## The one decision that matters: deny-by-default
//!
//! [`SessionScope`] is an allowlist on **both** axes (`allowed_methods` and
//! `allowed_targets`). An empty list on either axis denies every use — never
//! "empty means unrestricted." A session key with a scope nobody filled in
//! is a key that can do nothing, not a key that can do everything. This is
//! checked in [`check_scope`] before any other condition, so a
//! misconfigured-but-present scope can never accidentally degrade into
//! full account authority.
//!
//! ## Everything else, briefly
//!
//! - `valid_from` / `valid_until` are **block heights**, not wall-clock
//!   durations or timestamps — the same reasoning as `RecoveryPolicy` in
//!   `recovery.rs` (#2172): every validating node must reach the identical
//!   accept/reject verdict from identical input, and height is the one clock
//!   every node already agrees on byte-for-byte.
//! - `max_value_per_use` bounds a single use; `max_total_value` bounds the
//!   **cumulative** value across every use of the key — enforced by
//!   comparing against [`SessionKeyRecord::cumulative_value_used`], which
//!   only advances on a successful, applied use.
//! - `max_uses` is an optional hard cap on the number of successful uses.
//! - Registration and revocation are both actions on the account **itself**,
//!   proven by the primary key exactly the way every other privileged
//!   account action is proven in this codebase
//!   (`Address::from_public_key(&owner_public_key) == owner`, then
//!   `owner_public_key.verify(...)`) — a session key can never authorize its
//!   own registration or revocation.
//! - Revocation is a normal consensus state mutation like any other: it sets
//!   [`SessionKeyRecord::revoked`], and [`apply`] with [`SessionKeyAction::Use`]
//!   always reads the **current** committed record, never a
//!   submission-time snapshot. So a `Use` transition built and mempool-broadcast
//!   before a `Revoke` transition is committed is rejected the instant
//!   `Revoke` lands — whichever transition a block actually orders first
//!   wins, which is exactly "revocation invalidates already-submitted
//!   mempool operations" (see `mempool_resident_operation_is_invalidated_by_revocation`
//!   below: the point isn't a special code path, it's that there is no
//!   snapshot to go stale in the first place).
//! - Each successful [`SessionKeyAction::Use`] is signed by the session key
//!   over a domain-separated message binding the chain, owner, session key's
//!   own address, method, target, value, and a strictly-sequential use index
//!   ([`SessionKeyRecord::uses_so_far`] at signing time) — so a signed use
//!   message can never be replayed to double-spend the same authorization.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::common::types::Address;
use crate::crypto::domain_signing::push_length_prefixed;
use crate::crypto::signature::{PublicKey, Signature};

/// Maximum live session-key grants retained for one owner.
pub const MAX_SESSION_KEYS_PER_OWNER: usize = 64;

/// Domain tag for registering a new session key (signed by the account's
/// primary key).
pub const SESSION_KEY_REGISTER_DOMAIN_TAG: &[u8] = b"wrfcoin.session_key_register.v1";
/// Domain tag for revoking a session key (signed by the account's primary key).
pub const SESSION_KEY_REVOKE_DOMAIN_TAG: &[u8] = b"wrfcoin.session_key_revoke.v1";
/// Domain tag for one use of a session key (signed by the session key itself).
pub const SESSION_KEY_USE_DOMAIN_TAG: &[u8] = b"wrfcoin.session_key_use.v1";

/// A named, application-level operation a session key may be allowed to
/// invoke (e.g. `"submit_weather_observation"`). Deliberately just a stable
/// string: this codebase has no existing structured "method" concept
/// (`UserOperation` in `abstraction.rs` carries only an opaque
/// `call_data_hash`), so this is the smallest self-contained identifier that
/// satisfies the story without inventing call-routing machinery this PR has
/// no mandate to build.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MethodId(pub String);

/// Deny-by-default allowlist. Empty `allowed_methods` or empty
/// `allowed_targets` denies every use — see the module-level docs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionScope {
    pub allowed_methods: Vec<MethodId>,
    pub allowed_targets: Vec<Address>,
    pub max_value_per_use: u128,
    pub max_total_value: u128,
}

/// The immutable terms fixed at registration. `valid_from`/`valid_until` are
/// block heights (see module docs for why, not `Duration`/`Timestamp` as the
/// issue's own sketch writes them).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionKeyGrant {
    /// Canonical ed25519 public key of the subordinate session key.
    pub public_key: PublicKey,
    pub scope: SessionScope,
    pub valid_from: u64,
    pub valid_until: u64,
    pub max_uses: Option<u32>,
}

/// Per-session-key consensus state: the immutable [`SessionKeyGrant`] plus
/// everything that changes as the key is used or revoked. Keyed in
/// `State::session_keys` by owner and then by
/// `Address::from_public_key(&grant.public_key)`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionKeyRecord {
    /// The account that registered this key and can revoke it.
    pub owner: Address,
    /// Strictly increasing generation assigned by the owner's registration nonce.
    pub registration_nonce: u64,
    pub grant: SessionKeyGrant,
    pub uses_so_far: u32,
    pub cumulative_value_used: u128,
    pub revoked: bool,
}

/// Durable session-key state for one owner.
///
/// The wrapper is retained even when `records` becomes empty so revocation or
/// expiry pruning can never reset `next_nonce` and make an old registration
/// signature valid again.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerSessionKeys {
    pub next_nonce: u64,
    pub records: BTreeMap<Address, SessionKeyRecord>,
}

/// What a single [`apply`] call asks the state machine to do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionKeyAction {
    /// Register a new session key under `owner`. Proven jointly by the
    /// owner's primary key and the session key itself.
    Register {
        owner: Address,
        registration_nonce: u64,
        grant: SessionKeyGrant,
        owner_public_key: String,
        owner_signature: String,
        session_signature: String,
    },
    /// Revoke an existing session key. Proven by the recorded owner's
    /// primary key — a session key can never revoke itself or another key.
    Revoke {
        owner: Address,
        session_key: Address,
        owner_public_key: String,
        owner_signature: String,
    },
    /// Spend one use of a session key against `method`/`target`/`value`.
    /// Proven by the session key's own signature.
    Use {
        owner: Address,
        session_key: Address,
        method: MethodId,
        target: Address,
        value: u128,
        session_signature: String,
    },
}

/// Every way [`apply`] refuses an action. `Debug`+`PartialEq`, no panic
/// anywhere in this module.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum SessionKeyError {
    #[error("session key has expired (valid_until {valid_until}, height {height})")]
    Expired { valid_until: u64, height: u64 },
    #[error("session key is not yet valid (valid_from {valid_from}, height {height})")]
    NotYetValid { valid_from: u64, height: u64 },
    #[error("method {0:?} is not in the session key's allowed_methods")]
    MethodNotAllowed(MethodId),
    #[error("target {0} is not in the session key's allowed_targets")]
    TargetNotAllowed(Address),
    #[error("value limit exceeded: {reason}")]
    ValueLimitExceeded { reason: &'static str },
    #[error("session key has exhausted its max_uses")]
    UsesExhausted,
    #[error("session key has been revoked")]
    Revoked,
    #[error("no session key found at this address")]
    NotFound,
    #[error("a session key with this address is already registered")]
    AlreadyRegistered,
    #[error("account has reached its maximum number of live session keys")]
    TooManySessionKeys,
    #[error("stale registration nonce: expected {expected}, got {actual}")]
    StaleRegistrationNonce { expected: u64, actual: u64 },
    #[error("registration nonce exhausted")]
    RegistrationNonceExhausted,
    #[error("owner public key does not hash to the claimed owner address")]
    OwnerKeyAddressMismatch,
    #[error("owner signature does not verify")]
    BadOwnerSignature,
    #[error("session key does not belong to the account attempting to revoke it")]
    NotOwner,
    #[error("session key signature does not verify")]
    BadSessionSignature,
    #[error("malformed hex key or signature: {0}")]
    Malformed(&'static str),
    #[error("invalid grant: {0}")]
    InvalidGrant(&'static str),
}

/// The exact bytes the owner signs to register `grant` under `owner`. The
/// strictly advancing `registration_nonce` makes old registration bytes stale;
/// binding the full grant means one scope/expiry cannot become a looser one.
pub fn session_key_register_message(
    chain_id: &str,
    owner: &Address,
    registration_nonce: u64,
    grant: &SessionKeyGrant,
) -> Vec<u8> {
    let mut message = Vec::new();
    push_length_prefixed(&mut message, SESSION_KEY_REGISTER_DOMAIN_TAG);
    push_length_prefixed(&mut message, chain_id.as_bytes());
    push_length_prefixed(&mut message, &owner.0);
    push_length_prefixed(&mut message, &registration_nonce.to_le_bytes());
    push_length_prefixed(&mut message, grant.public_key.as_bytes());
    push_scope(&mut message, &grant.scope);
    message.extend_from_slice(&grant.valid_from.to_le_bytes());
    message.extend_from_slice(&grant.valid_until.to_le_bytes());
    match grant.max_uses {
        Some(n) => {
            message.push(1);
            message.extend_from_slice(&n.to_le_bytes());
        },
        None => message.push(0),
    }
    message
}

fn push_scope(message: &mut Vec<u8>, scope: &SessionScope) {
    message.extend_from_slice(&(scope.allowed_methods.len() as u64).to_le_bytes());
    for m in &scope.allowed_methods {
        push_length_prefixed(message, m.0.as_bytes());
    }
    message.extend_from_slice(&(scope.allowed_targets.len() as u64).to_le_bytes());
    for t in &scope.allowed_targets {
        message.extend_from_slice(&t.0);
    }
    message.extend_from_slice(&scope.max_value_per_use.to_le_bytes());
    message.extend_from_slice(&scope.max_total_value.to_le_bytes());
}

/// The exact bytes the owner signs to revoke one generation of `session_key`.
pub fn session_key_revoke_message(
    chain_id: &str,
    owner: &Address,
    session_key: &Address,
    registration_nonce: u64,
) -> Vec<u8> {
    let mut message = Vec::new();
    push_length_prefixed(&mut message, SESSION_KEY_REVOKE_DOMAIN_TAG);
    push_length_prefixed(&mut message, chain_id.as_bytes());
    push_length_prefixed(&mut message, &owner.0);
    push_length_prefixed(&mut message, &session_key.0);
    push_length_prefixed(&mut message, &registration_nonce.to_le_bytes());
    message
}

/// The exact bytes the session key signs to authorize one use. Binding both
/// `registration_nonce` and `use_index` prevents replay across re-registration
/// generations and across successful uses within one generation.
pub fn session_key_use_message(
    chain_id: &str,
    owner: &Address,
    session_key: &Address,
    method: &MethodId,
    target: &Address,
    value: u128,
    registration_nonce: u64,
    use_index: u32,
) -> Vec<u8> {
    let mut message = Vec::new();
    push_length_prefixed(&mut message, SESSION_KEY_USE_DOMAIN_TAG);
    push_length_prefixed(&mut message, chain_id.as_bytes());
    push_length_prefixed(&mut message, &owner.0);
    push_length_prefixed(&mut message, &session_key.0);
    push_length_prefixed(&mut message, method.0.as_bytes());
    push_length_prefixed(&mut message, &target.0);
    message.extend_from_slice(&value.to_le_bytes());
    push_length_prefixed(&mut message, &registration_nonce.to_le_bytes());
    message.extend_from_slice(&use_index.to_le_bytes());
    message
}

fn parse_hex_public_key(hex: &str, malformed: &'static str) -> Result<PublicKey, SessionKeyError> {
    PublicKey::from_hex(hex.trim()).map_err(|_| SessionKeyError::Malformed(malformed))
}

fn parse_hex_signature(hex: &str, malformed: &'static str) -> Result<Signature, SessionKeyError> {
    Signature::from_hex(hex.trim()).map_err(|_| SessionKeyError::Malformed(malformed))
}

/// Deny-by-default scope check: empty `allowed_methods` or empty
/// `allowed_targets` refuses unconditionally, before either allowlist is
/// even consulted for membership.
fn check_scope(
    scope: &SessionScope,
    method: &MethodId,
    target: &Address,
    value: u128,
) -> Result<(), SessionKeyError> {
    if scope.allowed_methods.is_empty() {
        return Err(SessionKeyError::MethodNotAllowed(method.clone()));
    }
    if scope.allowed_targets.is_empty() {
        return Err(SessionKeyError::TargetNotAllowed(*target));
    }
    if !scope.allowed_methods.contains(method) {
        return Err(SessionKeyError::MethodNotAllowed(method.clone()));
    }
    if !scope.allowed_targets.contains(target) {
        return Err(SessionKeyError::TargetNotAllowed(*target));
    }
    if value > scope.max_value_per_use {
        return Err(SessionKeyError::ValueLimitExceeded {
            reason: "exceeds max_value_per_use",
        });
    }
    Ok(())
}

/// Apply one [`SessionKeyAction`] against `states` at `height`. The single
/// entry point consensus code calls — see
/// `StateTransition::ApplySessionKeyAction` in `blockchain/state.rs`, whose
/// match arm is exactly a call into this function.
pub fn apply(
    states: &mut BTreeMap<Address, OwnerSessionKeys>,
    action: SessionKeyAction,
    height: u64,
    chain_id: &str,
) -> Result<(), SessionKeyError> {
    match action {
        SessionKeyAction::Register {
            owner,
            registration_nonce,
            grant,
            owner_public_key,
            owner_signature,
            session_signature,
        } => {
            let expected_nonce = states.get(&owner).map_or(0, |state| state.next_nonce);
            if registration_nonce != expected_nonce {
                return Err(SessionKeyError::StaleRegistrationNonce {
                    expected: expected_nonce,
                    actual: registration_nonce,
                });
            }
            if grant.valid_from > grant.valid_until {
                return Err(SessionKeyError::InvalidGrant(
                    "valid_from must not exceed valid_until",
                ));
            }
            let session_key_address = Address::from_public_key(&grant.public_key);
            let duplicate_is_live = states
                .get(&owner)
                .and_then(|state| state.records.get(&session_key_address))
                .is_some_and(|record| !record.revoked && record.grant.valid_until >= height);
            if duplicate_is_live {
                return Err(SessionKeyError::AlreadyRegistered);
            }
            let live_count = states
                .get(&owner)
                .into_iter()
                .flat_map(|state| state.records.values())
                .filter(|record| !record.revoked && record.grant.valid_until >= height)
                .count();
            if live_count >= MAX_SESSION_KEYS_PER_OWNER {
                return Err(SessionKeyError::TooManySessionKeys);
            }
            let owner_pk = parse_hex_public_key(
                &owner_public_key,
                "owner public key is not valid hex ed25519",
            )?;
            if Address::from_public_key(&owner_pk) != owner {
                return Err(SessionKeyError::OwnerKeyAddressMismatch);
            }
            let owner_sig =
                parse_hex_signature(&owner_signature, "owner signature is not valid hex")?;
            let message =
                session_key_register_message(chain_id, &owner, registration_nonce, &grant);
            if !owner_pk.verify(&message, &owner_sig) {
                return Err(SessionKeyError::BadOwnerSignature);
            }
            let session_sig =
                parse_hex_signature(&session_signature, "session signature is not valid hex")?;
            if !grant.public_key.verify(&message, &session_sig) {
                return Err(SessionKeyError::BadSessionSignature);
            }
            let next_nonce = registration_nonce
                .checked_add(1)
                .ok_or(SessionKeyError::RegistrationNonceExhausted)?;
            let owner_state = states.entry(owner).or_default();
            owner_state
                .records
                .retain(|_, record| !record.revoked && record.grant.valid_until >= height);
            owner_state.records.insert(
                session_key_address,
                SessionKeyRecord {
                    owner,
                    registration_nonce,
                    grant,
                    uses_so_far: 0,
                    cumulative_value_used: 0,
                    revoked: false,
                },
            );
            owner_state.next_nonce = next_nonce;
            Ok(())
        },
        SessionKeyAction::Revoke {
            owner,
            session_key,
            owner_public_key,
            owner_signature,
        } => {
            let record = states
                .get_mut(&owner)
                .and_then(|state| state.records.get_mut(&session_key))
                .ok_or(SessionKeyError::NotFound)?;
            let owner_pk = parse_hex_public_key(
                &owner_public_key,
                "owner public key is not valid hex ed25519",
            )?;
            if Address::from_public_key(&owner_pk) != record.owner {
                return Err(SessionKeyError::NotOwner);
            }
            let owner_sig =
                parse_hex_signature(&owner_signature, "owner signature is not valid hex")?;
            let message = session_key_revoke_message(
                chain_id,
                &record.owner,
                &session_key,
                record.registration_nonce,
            );
            if !owner_pk.verify(&message, &owner_sig) {
                return Err(SessionKeyError::BadOwnerSignature);
            }
            record.revoked = true;
            Ok(())
        },
        SessionKeyAction::Use {
            owner,
            session_key,
            method,
            target,
            value,
            session_signature,
        } => {
            let record = states
                .get_mut(&owner)
                .and_then(|state| state.records.get_mut(&session_key))
                .ok_or(SessionKeyError::NotFound)?;
            if record.revoked {
                return Err(SessionKeyError::Revoked);
            }
            if height < record.grant.valid_from {
                return Err(SessionKeyError::NotYetValid {
                    valid_from: record.grant.valid_from,
                    height,
                });
            }
            if height > record.grant.valid_until {
                return Err(SessionKeyError::Expired {
                    valid_until: record.grant.valid_until,
                    height,
                });
            }
            if let Some(max_uses) = record.grant.max_uses {
                if record.uses_so_far >= max_uses {
                    return Err(SessionKeyError::UsesExhausted);
                }
            }
            check_scope(&record.grant.scope, &method, &target, value)?;
            let new_cumulative = record.cumulative_value_used.checked_add(value).ok_or(
                SessionKeyError::ValueLimitExceeded {
                    reason: "cumulative value overflow",
                },
            )?;
            if new_cumulative > record.grant.scope.max_total_value {
                return Err(SessionKeyError::ValueLimitExceeded {
                    reason: "exceeds max_total_value",
                });
            }
            let sig =
                parse_hex_signature(&session_signature, "session signature is not valid hex")?;
            let message = session_key_use_message(
                chain_id,
                &owner,
                &session_key,
                &method,
                &target,
                value,
                record.registration_nonce,
                record.uses_so_far,
            );
            if !record.grant.public_key.verify(&message, &sig) {
                return Err(SessionKeyError::BadSessionSignature);
            }
            record.uses_so_far = record
                .uses_so_far
                .checked_add(1)
                .ok_or(SessionKeyError::UsesExhausted)?;
            record.cumulative_value_used = new_cumulative;
            Ok(())
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signature::PrivateKey;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    const TEST_CHAIN_ID: &str = "wrfcoin-session-key-test";

    fn apply(
        states: &mut BTreeMap<Address, OwnerSessionKeys>,
        action: SessionKeyAction,
        height: u64,
    ) -> Result<(), SessionKeyError> {
        super::apply(states, action, height, TEST_CHAIN_ID)
    }

    fn keypair(seed: u64) -> (PrivateKey, PublicKey, Address) {
        let sk = PrivateKey::generate(&mut StdRng::seed_from_u64(seed));
        let pk = PublicKey::from_private_key(&sk);
        let addr = Address::from_public_key(&pk);
        (sk, pk, addr)
    }

    fn signed_register(
        chain_id: &str,
        owner_sk: &PrivateKey,
        owner_pk: &PublicKey,
        owner: Address,
        session_sk: &PrivateKey,
        registration_nonce: u64,
        grant: SessionKeyGrant,
    ) -> SessionKeyAction {
        let message = session_key_register_message(chain_id, &owner, registration_nonce, &grant);
        SessionKeyAction::Register {
            owner,
            registration_nonce,
            grant,
            owner_public_key: owner_pk.to_hex(),
            owner_signature: owner_sk.sign(&message).to_hex(),
            session_signature: session_sk.sign(&message).to_hex(),
        }
    }

    struct Fixture {
        owner: Address,
        owner_sk: PrivateKey,
        owner_pk: PublicKey,
        session_sk: PrivateKey,
        session_pk: PublicKey,
        session_key: Address,
        target: Address,
        states: BTreeMap<Address, OwnerSessionKeys>,
    }

    fn register(
        scope: SessionScope,
        valid_from: u64,
        valid_until: u64,
        max_uses: Option<u32>,
    ) -> Fixture {
        let (owner_sk, owner_pk, owner) = keypair(0xAAAA);
        let (session_sk, session_pk, session_key) = keypair(0xBBBB);
        let (_, _, target) = keypair(0xCCCC);
        let grant = SessionKeyGrant {
            public_key: session_pk.clone(),
            scope,
            valid_from,
            valid_until,
            max_uses,
        };
        let message = session_key_register_message(TEST_CHAIN_ID, &owner, 0, &grant);
        let owner_signature = owner_sk.sign(&message);
        let session_signature = session_sk.sign(&message);
        let mut states = BTreeMap::new();
        apply(
            &mut states,
            SessionKeyAction::Register {
                owner,
                registration_nonce: 0,
                grant,
                owner_public_key: owner_pk.to_hex(),
                owner_signature: owner_signature.to_hex(),
                session_signature: session_signature.to_hex(),
            },
            0,
        )
        .expect("registration with a valid owner signature must succeed");
        Fixture {
            owner,
            owner_sk,
            owner_pk,
            session_sk,
            session_pk,
            session_key,
            target,
            states,
        }
    }

    impl Fixture {
        fn use_action(&self, method: &str, target: Address, value: u128) -> SessionKeyAction {
            let record = &self.states[&self.owner].records[&self.session_key];
            let method = MethodId(method.to_string());
            let message = session_key_use_message(
                TEST_CHAIN_ID,
                &self.owner,
                &self.session_key,
                &method,
                &target,
                value,
                record.registration_nonce,
                record.uses_so_far,
            );
            let signature = self.session_sk.sign(&message);
            SessionKeyAction::Use {
                owner: self.owner,
                session_key: self.session_key,
                method,
                target,
                value,
                session_signature: signature.to_hex(),
            }
        }

        fn revoke_action(&self) -> SessionKeyAction {
            let record = &self.states[&self.owner].records[&self.session_key];
            let message = session_key_revoke_message(
                TEST_CHAIN_ID,
                &self.owner,
                &self.session_key,
                record.registration_nonce,
            );
            let signature = self.owner_sk.sign(&message);
            SessionKeyAction::Revoke {
                owner: self.owner,
                session_key: self.session_key,
                owner_public_key: self.owner_pk.to_hex(),
                owner_signature: signature.to_hex(),
            }
        }

        fn scoped_action(&self) -> SessionKeyAction {
            self.use_action("submit_weather_observation", self.target, 0)
        }
    }

    fn station_scope(target: Address) -> SessionScope {
        SessionScope {
            allowed_methods: vec![MethodId("submit_weather_observation".to_string())],
            allowed_targets: vec![target],
            max_value_per_use: 0,
            max_total_value: 0,
        }
    }

    /// The story's own acceptance driver: an observation-only, zero-value,
    /// 30-day (in blocks: ~30 * 7200 assuming 12s blocks, but the exact
    /// number doesn't matter here) session key.
    #[test]
    fn station_scenario_observation_only_zero_value_expiring_key() {
        let thirty_days_in_blocks = 30 * 7_200;
        let f = register(
            station_scope(Address([9u8; 20])),
            0,
            thirty_days_in_blocks,
            None,
        );
        let target = Address([9u8; 20]);
        let action = f.use_action("submit_weather_observation", target, 0);
        apply(&mut { f.states.clone() }, action, 100)
            .expect("station key must be able to submit observations");
    }

    #[test]
    fn empty_scope_permits_nothing() {
        let empty_scope = SessionScope {
            allowed_methods: vec![],
            allowed_targets: vec![],
            max_value_per_use: u128::MAX,
            max_total_value: u128::MAX,
        };
        let mut f = register(empty_scope, 0, 1_000_000, None);
        let target = f.target;
        let action = f.use_action("anything", target, 0);
        let err = apply(&mut f.states, action, 10).unwrap_err();
        assert_eq!(
            err,
            SessionKeyError::MethodNotAllowed(MethodId("anything".to_string()))
        );
    }

    #[test]
    fn empty_allowed_targets_alone_permits_nothing() {
        let scope = SessionScope {
            allowed_methods: vec![MethodId("submit_weather_observation".to_string())],
            allowed_targets: vec![],
            max_value_per_use: 100,
            max_total_value: 100,
        };
        let mut f = register(scope, 0, 1_000, None);
        let target = f.target;
        let action = f.use_action("submit_weather_observation", target, 0);
        let err = apply(&mut f.states, action, 10).unwrap_err();
        assert_eq!(err, SessionKeyError::TargetNotAllowed(target));
    }

    #[test]
    fn expired_key_is_rejected() {
        let mut f = register(station_scope(Address([9u8; 20])), 0, 100, None);
        let target = Address([9u8; 20]);
        let action = f.use_action("submit_weather_observation", target, 0);
        let err = apply(&mut f.states, action, 101).unwrap_err();
        assert_eq!(
            err,
            SessionKeyError::Expired {
                valid_until: 100,
                height: 101
            }
        );
    }

    #[test]
    fn not_yet_valid_key_is_rejected() {
        let mut f = register(station_scope(Address([9u8; 20])), 50, 100, None);
        let target = Address([9u8; 20]);
        let action = f.use_action("submit_weather_observation", target, 0);
        let err = apply(&mut f.states, action, 10).unwrap_err();
        assert_eq!(
            err,
            SessionKeyError::NotYetValid {
                valid_from: 50,
                height: 10
            }
        );
    }

    #[test]
    fn method_outside_scope_is_rejected() {
        let mut f = register(station_scope(Address([9u8; 20])), 0, 1_000, None);
        let target = Address([9u8; 20]);
        let action = f.use_action("transfer_funds", target, 0);
        let err = apply(&mut f.states, action, 10).unwrap_err();
        assert_eq!(
            err,
            SessionKeyError::MethodNotAllowed(MethodId("transfer_funds".to_string()))
        );
    }

    #[test]
    fn target_outside_scope_is_rejected() {
        let mut f = register(station_scope(Address([9u8; 20])), 0, 1_000, None);
        let (_, _, other_target) = keypair(0xD00D);
        let action = f.use_action("submit_weather_observation", other_target, 0);
        let err = apply(&mut f.states, action, 10).unwrap_err();
        assert_eq!(err, SessionKeyError::TargetNotAllowed(other_target));
    }

    #[test]
    fn cumulative_value_cap_enforced_across_uses() {
        let target = Address([9u8; 20]);
        let scope = SessionScope {
            allowed_methods: vec![MethodId("pay".to_string())],
            allowed_targets: vec![target],
            max_value_per_use: 60,
            max_total_value: 100,
        };
        let mut f = register(scope, 0, 1_000, None);

        let a1 = f.use_action("pay", target, 60);
        apply(&mut f.states, a1, 10).expect("first use within both caps must succeed");

        // Second use is within max_value_per_use (60) but would push the
        // cumulative total to 120, over max_total_value (100).
        let a2 = f.use_action("pay", target, 60);
        let err = apply(&mut f.states, a2, 11).unwrap_err();
        assert_eq!(
            err,
            SessionKeyError::ValueLimitExceeded {
                reason: "exceeds max_total_value"
            }
        );

        // A smaller top-up that fits under the remaining budget succeeds.
        let a3 = f.use_action("pay", target, 40);
        apply(&mut f.states, a3, 12).expect("use within remaining cumulative budget must succeed");
        assert_eq!(
            f.states[&f.owner].records[&f.session_key].cumulative_value_used,
            100
        );
    }

    #[test]
    fn single_use_exceeding_max_value_per_use_is_rejected_even_under_total_cap() {
        let target = Address([9u8; 20]);
        let scope = SessionScope {
            allowed_methods: vec![MethodId("pay".to_string())],
            allowed_targets: vec![target],
            max_value_per_use: 10,
            max_total_value: 1_000,
        };
        let mut f = register(scope, 0, 1_000, None);
        let action = f.use_action("pay", target, 11);
        let err = apply(&mut f.states, action, 10).unwrap_err();
        assert_eq!(
            err,
            SessionKeyError::ValueLimitExceeded {
                reason: "exceeds max_value_per_use"
            }
        );
    }

    #[test]
    fn max_uses_exhaustion_is_enforced() {
        let target = Address([9u8; 20]);
        let mut f = register(station_scope(target), 0, 1_000, Some(2));

        let a1 = f.use_action("submit_weather_observation", target, 0);
        apply(&mut f.states, a1, 1).unwrap();
        let a2 = f.use_action("submit_weather_observation", target, 0);
        apply(&mut f.states, a2, 2).unwrap();
        let a3 = f.use_action("submit_weather_observation", target, 0);
        let err = apply(&mut f.states, a3, 3).unwrap_err();
        assert_eq!(err, SessionKeyError::UsesExhausted);
    }

    /// "Revocation invalidates already-submitted mempool operations": build a
    /// `Use` action (as if it were sitting, fully signed, in the mempool),
    /// then commit a `Revoke` transition, then attempt to apply the ORIGINAL
    /// (unmodified) `Use` action. There is no special "check the mempool"
    /// code path here — the point is structural: `apply` always reads the
    /// CURRENT record, so the pre-built action is rejected the moment
    /// `Revoke` has landed, with zero coordination between the two.
    #[test]
    fn mempool_resident_operation_is_invalidated_by_revocation() {
        let mut f = register(station_scope(Address([9u8; 20])), 0, 1_000, None);
        let target = Address([9u8; 20]);
        // "Signed and broadcast" before revocation.
        let pending_use = f.use_action("submit_weather_observation", target, 0);

        let revoke = f.revoke_action();
        apply(&mut f.states, revoke, 5).expect("revocation by the owner must succeed");

        let err = apply(&mut f.states, pending_use, 6).unwrap_err();
        assert_eq!(err, SessionKeyError::Revoked);
    }

    #[test]
    fn original_register_cannot_be_replayed_after_revoke() {
        let mut f = register(station_scope(Address([9u8; 20])), 0, 1_000, None);
        let original_register = signed_register(
            TEST_CHAIN_ID,
            &f.owner_sk,
            &f.owner_pk,
            f.owner,
            &f.session_sk,
            0,
            f.states[&f.owner].records[&f.session_key].grant.clone(),
        );
        let revoke = f.revoke_action();
        apply(&mut f.states, revoke, 5).unwrap();

        assert_eq!(
            apply(&mut f.states, original_register, 6),
            Err(SessionKeyError::StaleRegistrationNonce {
                expected: 1,
                actual: 0,
            })
        );
        assert!(f.states[&f.owner].records[&f.session_key].revoked);
        assert_eq!(f.states[&f.owner].next_nonce, 1);
    }

    #[test]
    fn refused_register_consumes_no_nonce() {
        let mut f = register(station_scope(Address([9u8; 20])), 0, 1_000, None);
        let (new_session_sk, new_session_pk, _) = keypair(0xDD01);
        let grant = SessionKeyGrant {
            public_key: new_session_pk,
            scope: station_scope(Address([9u8; 20])),
            valid_from: 0,
            valid_until: 1_000,
            max_uses: None,
        };
        let mut refused = signed_register(
            TEST_CHAIN_ID,
            &f.owner_sk,
            &f.owner_pk,
            f.owner,
            &new_session_sk,
            1,
            grant,
        );
        if let SessionKeyAction::Register {
            session_signature, ..
        } = &mut refused
        {
            *session_signature = "00".repeat(64);
        }
        assert_eq!(
            apply(&mut f.states, refused, 1),
            Err(SessionKeyError::BadSessionSignature)
        );
        assert_eq!(f.states[&f.owner].next_nonce, 1);
        assert_eq!(f.states[&f.owner].records.len(), 1);
    }

    #[test]
    fn registration_nonce_requires_exact_next_value_and_never_wraps() {
        let (owner_sk, owner_pk, owner) = keypair(0xAA10);
        let (session_sk, session_pk, _) = keypair(0xBB10);
        let grant = SessionKeyGrant {
            public_key: session_pk,
            scope: station_scope(Address([9u8; 20])),
            valid_from: 0,
            valid_until: 1_000,
            max_uses: None,
        };
        let mut states = BTreeMap::new();
        let skipped_zero = signed_register(
            TEST_CHAIN_ID,
            &owner_sk,
            &owner_pk,
            owner,
            &session_sk,
            1,
            grant.clone(),
        );
        assert_eq!(
            apply(&mut states, skipped_zero, 1),
            Err(SessionKeyError::StaleRegistrationNonce {
                expected: 0,
                actual: 1,
            })
        );
        assert!(states.is_empty());

        states.insert(
            owner,
            OwnerSessionKeys {
                next_nonce: u64::MAX,
                records: BTreeMap::new(),
            },
        );
        let would_wrap = signed_register(
            TEST_CHAIN_ID,
            &owner_sk,
            &owner_pk,
            owner,
            &session_sk,
            u64::MAX,
            grant,
        );
        assert_eq!(
            apply(&mut states, would_wrap, 1),
            Err(SessionKeyError::RegistrationNonceExhausted)
        );
        assert_eq!(states[&owner].next_nonce, u64::MAX);
        assert!(states[&owner].records.is_empty());
    }

    #[test]
    fn use_from_old_generation_fails_after_reregistration() {
        let target = Address([9u8; 20]);
        let mut f = register(station_scope(target), 0, 1_000, None);
        let old_use = f.use_action("submit_weather_observation", target, 0);
        let grant = f.states[&f.owner].records[&f.session_key].grant.clone();
        let revoke = f.revoke_action();
        apply(&mut f.states, revoke, 5).unwrap();
        let reregister = signed_register(
            TEST_CHAIN_ID,
            &f.owner_sk,
            &f.owner_pk,
            f.owner,
            &f.session_sk,
            1,
            grant,
        );
        apply(&mut f.states, reregister, 6).unwrap();

        assert_eq!(
            apply(&mut f.states, old_use, 7),
            Err(SessionKeyError::BadSessionSignature)
        );
        assert_eq!(
            f.states[&f.owner].records[&f.session_key].registration_nonce,
            1
        );
    }

    #[test]
    fn revoke_from_old_generation_fails_after_reregistration() {
        let mut f = register(station_scope(Address([9u8; 20])), 0, 1_000, None);
        let stale_revoke = f.revoke_action();
        let grant = f.states[&f.owner].records[&f.session_key].grant.clone();
        apply(&mut f.states, stale_revoke.clone(), 5).unwrap();
        let reregister = signed_register(
            TEST_CHAIN_ID,
            &f.owner_sk,
            &f.owner_pk,
            f.owner,
            &f.session_sk,
            1,
            grant,
        );
        apply(&mut f.states, reregister, 6).unwrap();

        assert_eq!(
            apply(&mut f.states, stale_revoke, 7),
            Err(SessionKeyError::BadOwnerSignature)
        );
        assert!(!f.states[&f.owner].records[&f.session_key].revoked);
    }

    #[test]
    fn registration_requires_a_valid_owner_signature() {
        let (owner, _owner_sk, owner_pk) = {
            let (sk, pk, addr) = keypair(0xAAAA);
            (addr, sk, pk)
        };
        let (wrong_sk, _, _) = keypair(0x1234);
        let (session_sk, session_pk, _) = keypair(0xBBBB);
        let grant = SessionKeyGrant {
            public_key: session_pk.clone(),
            scope: station_scope(Address([9u8; 20])),
            valid_from: 0,
            valid_until: 1_000,
            max_uses: None,
        };
        let message = session_key_register_message(TEST_CHAIN_ID, &owner, 0, &grant);
        // Signed by the WRONG key.
        let bad_signature = wrong_sk.sign(&message);
        let session_signature = session_sk.sign(&message);
        let mut states = BTreeMap::new();
        let err = apply(
            &mut states,
            SessionKeyAction::Register {
                owner,
                registration_nonce: 0,
                grant,
                owner_public_key: owner_pk.to_hex(),
                owner_signature: bad_signature.to_hex(),
                session_signature: session_signature.to_hex(),
            },
            0,
        )
        .unwrap_err();
        assert_eq!(err, SessionKeyError::BadOwnerSignature);
    }

    #[test]
    fn revocation_requires_a_valid_owner_signature() {
        let mut f = register(station_scope(Address([9u8; 20])), 0, 1_000, None);
        let (_, wrong_pk, _) = keypair(0xDEAD);
        let message = session_key_revoke_message(TEST_CHAIN_ID, &f.owner, &f.session_key, 0);
        let (wrong_sk, _, _) = keypair(0xDEAD);
        let bad_signature = wrong_sk.sign(&message);
        let err = apply(
            &mut f.states,
            SessionKeyAction::Revoke {
                owner: f.owner,
                session_key: f.session_key,
                owner_public_key: wrong_pk.to_hex(),
                owner_signature: bad_signature.to_hex(),
            },
            5,
        )
        .unwrap_err();
        assert_eq!(err, SessionKeyError::NotOwner);
    }

    #[test]
    fn a_stale_use_signature_cannot_be_replayed_after_a_successful_use() {
        let target = Address([9u8; 20]);
        let mut f = register(station_scope(target), 0, 1_000, None);
        let first_use = f.use_action("submit_weather_observation", target, 0);
        apply(&mut f.states, first_use.clone(), 1).unwrap();

        // Replaying the SAME signed action (still bound to use_index 0) must
        // fail: the record's uses_so_far has advanced to 1, so the message
        // the signature covers no longer matches what verification expects.
        let err = apply(&mut f.states, first_use, 2).unwrap_err();
        assert_eq!(err, SessionKeyError::BadSessionSignature);
    }

    #[test]
    fn duplicate_registration_at_the_same_session_key_is_rejected() {
        let f = register(station_scope(Address([9u8; 20])), 0, 1_000, None);
        let grant = SessionKeyGrant {
            public_key: f.session_pk.clone(),
            scope: station_scope(Address([9u8; 20])),
            valid_from: 0,
            valid_until: 500,
            max_uses: None,
        };
        let message = session_key_register_message(TEST_CHAIN_ID, &f.owner, 1, &grant);
        let owner_signature = f.owner_sk.sign(&message);
        let session_signature = f.session_sk.sign(&message);
        let mut states = f.states.clone();
        let err = apply(
            &mut states,
            SessionKeyAction::Register {
                owner: f.owner,
                registration_nonce: 1,
                grant,
                owner_public_key: f.owner_pk.to_hex(),
                owner_signature: owner_signature.to_hex(),
                session_signature: session_signature.to_hex(),
            },
            10,
        )
        .unwrap_err();
        assert_eq!(err, SessionKeyError::AlreadyRegistered);
    }

    #[test]
    fn session_key_possession_blocks_frontrun_squatting() {
        let (victim_sk, victim_pk, victim) = keypair(0xAAAA);
        let (mallory_sk, mallory_pk, mallory) = keypair(0x6666);
        let (session_sk, session_pk, session_key) = keypair(0xBBBB);
        let grant = SessionKeyGrant {
            public_key: session_pk,
            scope: station_scope(Address([9u8; 20])),
            valid_from: 0,
            valid_until: 1_000,
            max_uses: None,
        };
        let mut states = BTreeMap::new();

        let mallory_message = session_key_register_message(TEST_CHAIN_ID, &mallory, 0, &grant);
        let squat = SessionKeyAction::Register {
            owner: mallory,
            registration_nonce: 0,
            grant: grant.clone(),
            owner_public_key: mallory_pk.to_hex(),
            owner_signature: mallory_sk.sign(&mallory_message).to_hex(),
            session_signature: victim_sk.sign(&mallory_message).to_hex(),
        };
        assert_eq!(
            apply(&mut states, squat, 1),
            Err(SessionKeyError::BadSessionSignature)
        );
        assert!(states.is_empty(), "failed squat must not mutate state");

        let legitimate = signed_register(
            TEST_CHAIN_ID,
            &victim_sk,
            &victim_pk,
            victim,
            &session_sk,
            0,
            grant,
        );
        apply(&mut states, legitimate, 1).expect("victim remains able to register");
        assert_eq!(states[&victim].records[&session_key].owner, victim);
    }

    #[test]
    fn primary_public_key_cannot_be_squatted_without_its_private_key() {
        let (_victim_sk, victim_pk, victim) = keypair(0xAAAA);
        let (mallory_sk, mallory_pk, mallory) = keypair(0x6666);
        let grant = SessionKeyGrant {
            public_key: victim_pk,
            scope: station_scope(Address([9u8; 20])),
            valid_from: 0,
            valid_until: 1_000,
            max_uses: None,
        };
        let message = session_key_register_message(TEST_CHAIN_ID, &mallory, 0, &grant);
        let action = SessionKeyAction::Register {
            owner: mallory,
            registration_nonce: 0,
            grant,
            owner_public_key: mallory_pk.to_hex(),
            owner_signature: mallory_sk.sign(&message).to_hex(),
            session_signature: mallory_sk.sign(&message).to_hex(),
        };
        let mut states = BTreeMap::new();
        assert_eq!(
            apply(&mut states, action, 1),
            Err(SessionKeyError::BadSessionSignature)
        );
        assert!(states.get(&victim).is_none());
    }

    #[test]
    fn use_signature_is_bound_to_owner() {
        let target = Address([9u8; 20]);
        let mut f = register(station_scope(target), 0, 1_000, None);
        let (_, _, other_owner) = keypair(0xDDDD);
        let method = MethodId("submit_weather_observation".to_string());
        let wrong_message = session_key_use_message(
            TEST_CHAIN_ID,
            &other_owner,
            &f.session_key,
            &method,
            &target,
            0,
            0,
            0,
        );
        let action = SessionKeyAction::Use {
            owner: f.owner,
            session_key: f.session_key,
            method,
            target,
            value: 0,
            session_signature: f.session_sk.sign(&wrong_message).to_hex(),
        };
        assert_eq!(
            apply(&mut f.states, action, 1),
            Err(SessionKeyError::BadSessionSignature)
        );
    }

    #[test]
    fn same_session_key_can_be_scoped_to_distinct_owners() {
        let (alice_sk, alice_pk, alice) = keypair(0xAAAA);
        let (bob_sk, bob_pk, bob) = keypair(0xBBBB);
        let (session_sk, session_pk, session_key) = keypair(0xCCCC);
        let grant = SessionKeyGrant {
            public_key: session_pk,
            scope: station_scope(Address([9u8; 20])),
            valid_from: 0,
            valid_until: 1_000,
            max_uses: None,
        };
        let mut states = BTreeMap::new();
        apply(
            &mut states,
            signed_register(
                TEST_CHAIN_ID,
                &alice_sk,
                &alice_pk,
                alice,
                &session_sk,
                0,
                grant.clone(),
            ),
            1,
        )
        .unwrap();
        apply(
            &mut states,
            signed_register(TEST_CHAIN_ID, &bob_sk, &bob_pk, bob, &session_sk, 0, grant),
            1,
        )
        .unwrap();
        assert!(states[&alice].records.contains_key(&session_key));
        assert!(states[&bob].records.contains_key(&session_key));
    }

    #[test]
    fn method_names_are_length_prefixed_in_register_message() {
        let (_, session_pk, _) = keypair(0xBBBB);
        let target = Address([9u8; 20]);
        let grant = |methods: &[&str]| SessionKeyGrant {
            public_key: session_pk.clone(),
            scope: SessionScope {
                allowed_methods: methods
                    .iter()
                    .map(|method| MethodId((*method).to_string()))
                    .collect(),
                allowed_targets: vec![target],
                max_value_per_use: 0,
                max_total_value: 0,
            },
            valid_from: 0,
            valid_until: 1_000,
            max_uses: None,
        };
        let owner = Address([1u8; 20]);
        assert_ne!(
            session_key_register_message(TEST_CHAIN_ID, &owner, 0, &grant(&["a\0b", "c"])),
            session_key_register_message(TEST_CHAIN_ID, &owner, 0, &grant(&["a", "b\0c"]))
        );
    }

    #[test]
    fn use_counter_overflow_is_refused_without_mutation() {
        let target = Address([9u8; 20]);
        let mut f = register(station_scope(target), 0, 1_000, None);
        f.states
            .get_mut(&f.owner)
            .unwrap()
            .records
            .get_mut(&f.session_key)
            .unwrap()
            .uses_so_far = u32::MAX;
        let action = f.use_action("submit_weather_observation", target, 0);
        assert_eq!(
            apply(&mut f.states, action, 10),
            Err(SessionKeyError::UsesExhausted)
        );
        assert_eq!(
            f.states[&f.owner].records[&f.session_key].uses_so_far,
            u32::MAX
        );
    }

    #[test]
    fn signatures_are_bound_to_chain_id() {
        let target = Address([9u8; 20]);
        let f = register(station_scope(target), 0, 1_000, None);
        let action = f.use_action("submit_weather_observation", target, 0);
        let mut states = f.states;
        assert_eq!(
            super::apply(&mut states, action, 1, "another-chain"),
            Err(SessionKeyError::BadSessionSignature)
        );
        let record = &states[&f.owner].records[&f.session_key];
        assert_ne!(
            session_key_register_message(TEST_CHAIN_ID, &f.owner, 0, &record.grant),
            session_key_register_message("another-chain", &f.owner, 0, &record.grant)
        );
        assert_ne!(
            session_key_revoke_message(TEST_CHAIN_ID, &f.owner, &f.session_key, 0),
            session_key_revoke_message("another-chain", &f.owner, &f.session_key, 0)
        );
    }

    #[test]
    fn registration_cap_is_bounded_and_expired_records_are_pruned_atomically() {
        let (owner_sk, owner_pk, owner) = keypair(0xAAAA);
        let target = Address([9u8; 20]);
        let mut states = BTreeMap::new();
        for seed in 0..MAX_SESSION_KEYS_PER_OWNER {
            let (session_sk, session_pk, _) = keypair(10_000 + seed as u64);
            let grant = SessionKeyGrant {
                public_key: session_pk,
                scope: station_scope(target),
                valid_from: 0,
                valid_until: 100,
                max_uses: None,
            };
            apply(
                &mut states,
                signed_register(
                    TEST_CHAIN_ID,
                    &owner_sk,
                    &owner_pk,
                    owner,
                    &session_sk,
                    seed as u64,
                    grant,
                ),
                1,
            )
            .unwrap();
        }
        let (extra_sk, extra_pk, _) = keypair(99_999);
        let extra = SessionKeyGrant {
            public_key: extra_pk,
            scope: station_scope(target),
            valid_from: 0,
            valid_until: 200,
            max_uses: None,
        };
        let capped = signed_register(
            TEST_CHAIN_ID,
            &owner_sk,
            &owner_pk,
            owner,
            &extra_sk,
            MAX_SESSION_KEYS_PER_OWNER as u64,
            extra.clone(),
        );
        assert_eq!(
            apply(&mut states, capped, 50),
            Err(SessionKeyError::TooManySessionKeys)
        );
        assert_eq!(states[&owner].records.len(), MAX_SESSION_KEYS_PER_OWNER);
        assert_eq!(
            states[&owner].next_nonce, MAX_SESSION_KEYS_PER_OWNER as u64,
            "a refused capped registration must not consume its nonce"
        );

        let after_expiry = signed_register(
            TEST_CHAIN_ID,
            &owner_sk,
            &owner_pk,
            owner,
            &extra_sk,
            MAX_SESSION_KEYS_PER_OWNER as u64,
            extra,
        );
        apply(&mut states, after_expiry, 101).expect("expired records free bounded capacity");
        assert_eq!(states[&owner].records.len(), 1);
        assert_eq!(
            states[&owner].next_nonce,
            MAX_SESSION_KEYS_PER_OWNER as u64 + 1
        );
    }

    #[test]
    fn session_public_key_is_stored_in_canonical_bytes() {
        let f = register(station_scope(Address([9u8; 20])), 0, 1_000, None);
        let stored = &f.states[&f.owner].records[&f.session_key].grant.public_key;
        assert_eq!(stored.as_bytes(), f.session_pk.as_bytes());
        assert_eq!(stored.to_hex(), stored.to_hex().to_ascii_lowercase());
    }

    /// `ConsensusFeature::SessionKeys` flips exactly at its scheduled
    /// height: inactive the block before, active from the block at (and
    /// after) it. Exercises `FeatureSchedule::active` directly rather than
    /// the process-global `feature_active` wrapper (its `OnceLock` is
    /// first-install-wins for the whole test binary, so a unit test cannot
    /// install its own schedule without racing every other test that
    /// touches it) — `FeatureSchedule` is a plain value `apply_transition_inner`'s
    /// `feature_active` call ultimately delegates to, so this proves the
    /// same activation arithmetic without needing the global.
    #[test]
    fn session_keys_feature_flips_exactly_at_activation_height() {
        use crate::crypto::chain_binding::{ConsensusFeature, FeatureSchedule};

        let schedule = FeatureSchedule::from_entries(vec![(
            ConsensusFeature::SessionKeys.name().to_string(),
            2_000,
        )])
        .expect("single-feature schedule is valid");

        assert!(!schedule.active(ConsensusFeature::SessionKeys, 0));
        assert!(!schedule.active(ConsensusFeature::SessionKeys, 1_999));
        assert!(schedule.active(ConsensusFeature::SessionKeys, 2_000));
        assert!(schedule.active(ConsensusFeature::SessionKeys, 2_001));
        assert!(schedule.active(ConsensusFeature::SessionKeys, u64::MAX));
    }

    /// An unscheduled feature is inactive at every height, including height
    /// 0 — `apply_transition_inner` must refuse `ApplySessionKeyAction`
    /// unconditionally until an operator actually schedules the feature.
    #[test]
    fn session_keys_is_inactive_when_entirely_unscheduled() {
        use crate::crypto::chain_binding::{ConsensusFeature, FeatureSchedule};

        let schedule = FeatureSchedule::from_entries(vec![]).expect("empty schedule is valid");
        assert!(!schedule.active(ConsensusFeature::SessionKeys, 0));
        assert!(!schedule.active(ConsensusFeature::SessionKeys, u64::MAX));
    }

    /// `state_root_material`'s `session_keys` section must be byte-absent
    /// (not merely zero-length) while the map is empty — the exact property
    /// that keeps every historical `expected_roots.json` entry unchanged by
    /// this feature's addition (proven end-to-end by
    /// `cargo test -p test_server --test determinism_differential`, whose
    /// fixture never populates `session_keys`). This unit test pins the same
    /// property directly against `State`, independent of that fixture.
    #[test]
    fn state_root_material_unaffected_while_session_keys_is_empty() {
        use crate::blockchain::state::State;

        let with_empty_map = State::default().state_root_material();
        let mut state = State::default();
        assert!(state.session_keys.is_empty());
        let also_empty = state.state_root_material();
        assert_eq!(
            with_empty_map, also_empty,
            "an empty session_keys map must contribute zero bytes to state_root_material"
        );

        let nonce_owner = Address([0xEE; 20]);
        state.session_keys.insert(
            nonce_owner,
            OwnerSessionKeys {
                next_nonce: 1,
                records: BTreeMap::new(),
            },
        );
        assert_ne!(
            with_empty_map,
            state.state_root_material(),
            "a durable owner registration nonce must be committed even with no live records"
        );
        state.session_keys.clear();

        // Directly populate the map (bypassing the feature gate, which is
        // apply_transition_inner's job, not state_root_material's) to prove
        // the section DOES change the root once non-empty — otherwise this
        // test would pass vacuously by never emitting the section at all.
        let (_, pk, addr) = {
            let sk = PrivateKey::generate(&mut StdRng::seed_from_u64(1));
            let pk = PublicKey::from_private_key(&sk);
            let addr = Address::from_public_key(&pk);
            (sk, pk, addr)
        };
        state.session_keys.entry(addr).or_default().records.insert(
            addr,
            SessionKeyRecord {
                owner: addr,
                registration_nonce: 0,
                grant: SessionKeyGrant {
                    public_key: pk,
                    scope: SessionScope {
                        allowed_methods: vec![],
                        allowed_targets: vec![],
                        max_value_per_use: 0,
                        max_total_value: 0,
                    },
                    valid_from: 0,
                    valid_until: 0,
                    max_uses: None,
                },
                uses_so_far: 0,
                cumulative_value_used: 0,
                revoked: false,
            },
        );
        let non_empty = state.state_root_material();
        assert_ne!(
            with_empty_map, non_empty,
            "a non-empty session_keys map must change state_root_material"
        );
    }
}
