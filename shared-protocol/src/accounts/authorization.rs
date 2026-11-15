//! Per-account authorization dispatch (#2168).
//!
//! # The gap this closes
//!
//! Today nothing on this chain asks an *account* how it wants to be
//! authorized. Which rule authorizes a spend is chosen by the transaction
//! **type** — an ed25519 signature over a domain-separated preimage for
//! ordinary spends, MLSAG ring signatures for anonymous reward claims and
//! confidential transfers. `Account::code`, `Account::storage` and
//! `Account::is_contract` exist in the state, are hashed into the state-root
//! material and parsed back, and are read by nothing else: the state-root
//! encoder is the only reader of `Account::code`.
//!
//! So there is no place for an account to say "my signatures are validated
//! differently". Every story in epic #2167 — passkeys (#2169), session keys
//! (#2170), sponsorship (#2171), recovery (#2172), delegation (#2175) — is an
//! authorization policy, and all of them need somewhere to be dispatched
//! from. This module is that place.
//!
//! # Why it is its own story
//!
//! Adding the first conditional branch to the spending path of every
//! transaction, where the condition is attacker-reachable account state, is
//! the single most invasive change in the epic. It gets its own story, its own
//! activation height and its own review rather than arriving as a side effect
//! of whichever policy story lands first — see
//! `docs/adr/ADR-account-delegation.md`.
//!
//! # Dormant until scheduled
//!
//! Every entry point here is gated on
//! [`ConsensusFeature::AccountAuthorizationDispatch`]. Below the activation
//! height [`authorization_for`] returns [`AuthorizationRule::SignerIsSender`]
//! for every account without reading account state at all, which is exactly
//! what the chain does today. The rule is compiled in and inert until its
//! scheduled height, so a binary carrying it produces byte-identical blocks to
//! one without it — the property that lets this merge before a genesis cut.

use crate::common::types::{Address, Timestamp};
use crate::crypto::chain_binding::{ConsensusFeature, feature_active};
use crate::crypto::signature::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DECLARE_AUTHORIZATION_DOMAIN_TAG: &[u8] = b"wrfcoin.declare_authorization.v1";

/// Deliberate inert sentinel for debit paths whose WAL transition carries no
/// header timestamp. This is safe only because unsupported time-sensitive
/// rules are refused at declaration time. Scheduling any time-based rule must
/// first plumb the block-header timestamp (never `tx.timestamp`) through
/// `ProcessGovernanceTransaction`, including WAL and producer/applier parity.
pub const DEBIT_TIME_UNPLUMBED_2168: Timestamp = Timestamp(0);

/// Read-only account authorization declarations consumed by the debit seam.
///
/// Caller map: transfer apply is covered by
/// `authorization_debit_height_2168`; stake/unstake extraction by
/// `stake_and_unstake_extractor_use_the_inclusion_height_for_the_state_view_2168`.
/// Verify-block, API and mempool are EmptyView-by-design, HEIGHT-INERT
/// pre-block sites with no post-application State. The
/// `pre_block_empty_view_agrees_with_state_view_for_every_storable_rule`
/// tripwire enforces that they stay safe while refuse-at-declare leaves
/// SignerIsSender as the sole declarable rule. `SignerPlusSecurityKey`
/// (`security_key_spend`) is storable through its own actions; it is safe for
/// these EmptyView sites because the State view only ever ADDS a requirement
/// (verify-block accepts a superset, the governance sink drops the rest
/// deterministically), pinned by `security_key_spend_state_view_is_stricter`.
/// Any rule that could accept what the ed25519 check refuses still needs
/// verify-block to gain this State-backed view first.
pub trait AuthorizationView {
    fn declared(&self, account: &Address) -> AccountAuthorization;
}

/// View used by today's transaction paths, before declarations are stored.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyAuthorizationView;

impl AuthorizationView for EmptyAuthorizationView {
    fn declared(&self, _account: &Address) -> AccountAuthorization {
        AccountAuthorization::none()
    }
}

/// Signature material supplied by the transaction authorizing a debit.
#[derive(Clone, Copy, Debug)]
pub struct SpendAuthorization<'a> {
    pub signer_public_key: &'a str,
    pub signature: &'a str,
}

/// How an account's spends are authorized.
///
/// Deliberately a closed enum rather than a trait object: what may authorize a
/// spend is consensus, so the set of possibilities must be enumerable,
/// reviewable and identical on every node. A new variant is a protocol change
/// with its own activation, which is the point.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorizationRule {
    /// The chain's only rule today: the transaction's signature must verify
    /// against the sender's own key. Every account resolves to this before the
    /// dispatch activates, and every account without a declared rule resolves
    /// to it afterwards.
    SignerIsSender,
    /// The account has declared a smart-account program that validates its
    /// operations (#2168's `SmartAccount`).
    SmartAccount {
        /// Hash of the authorization program this account runs.
        program: [u8; 32],
    },
    /// The account has delegated authorization to a program without changing
    /// its address (#2175). Carried here so delegation is an additive variant
    /// rather than a second dispatch mechanism.
    Delegated {
        /// Hash of the delegate program.
        program: [u8; 32],
        /// Delegation stops being honoured at this time, if set.
        valid_until: Option<Timestamp>,
    },
    /// A passphrase (ed25519) account that also requires a registered FIDO2 /
    /// WebAuthn P-256 security key to spend (`security_key_spend`). Written
    /// only by `SecurityKeyAction` transactions
    /// ([`crate::accounts::security_key::apply_action`]), never through
    /// [`apply_declaration`]. Appended LAST so earlier bincode variant indices
    /// are unchanged.
    SignerPlusSecurityKey {
        policy: crate::accounts::security_key::SecurityKeyPolicy,
    },
}

impl AuthorizationRule {
    /// Whether this rule is the chain's pre-#2168 behaviour.
    pub fn is_default(&self) -> bool {
        matches!(self, Self::SignerIsSender)
    }

    /// The program this rule dispatches to, if any.
    pub fn program(&self) -> Option<[u8; 32]> {
        match self {
            Self::SignerIsSender | Self::SignerPlusSecurityKey { .. } => None,
            Self::SmartAccount { program } | Self::Delegated { program, .. } => Some(*program),
        }
    }

    /// Whether the rule is still in force at `now`. A rule with no expiry
    /// always is; an expired delegation falls back to the account's own key
    /// rather than locking the account out.
    pub fn in_force_at(&self, now: Timestamp) -> bool {
        match self {
            Self::Delegated {
                valid_until: Some(valid_until),
                ..
            } => now <= *valid_until,
            _ => true,
        }
    }
}

/// What an account has declared about its own authorization.
///
/// Read from account state by the caller and passed in, so this module stays
/// pure and is testable without a `State`. The declaration is data the account
/// owner wrote; it is never trusted beyond selecting which rule runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountAuthorization {
    /// The rule the account declared, if it declared one.
    pub declared: Option<AuthorizationRule>,
}

/// Durable, account-owned declaration state. The wrapper is intentionally
/// retained after revocation: `next_nonce` must never reset.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountAuthorizationDeclaration {
    pub next_nonce: u64,
    pub current: Option<AccountAuthorization>,
    pub generation: u64,
}

/// A signed declaration/update/revocation transition. This is a state-machine
/// surface only; no transaction extractor constructs it until scheduling work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclareAuthorization {
    pub account: Address,
    pub authorization: Option<AccountAuthorization>,
    pub nonce: u64,
    pub generation: u64,
    pub signer_public_key: String,
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclarationError {
    StaleNonce { expected: u64, actual: u64 },
    StaleGeneration { expected: u64, actual: u64 },
    NonceExhausted,
    GenerationExhausted,
    Authorization(AuthorizationError),
}

impl From<AuthorizationError> for DeclarationError {
    fn from(value: AuthorizationError) -> Self { Self::Authorization(value) }
}

/// Canonical declaration signature preimage. Every component, including the
/// little-endian counters, is independently length-prefixed.
pub fn declare_authorization_message(
    chain_id: &str, account: &Address, authorization: &Option<AccountAuthorization>, nonce: u64, generation: u64,
) -> Vec<u8> {
    fn push(out: &mut Vec<u8>, bytes: &[u8]) { out.extend_from_slice(&(bytes.len() as u64).to_le_bytes()); out.extend_from_slice(bytes); }
    let encoded = encode_authorization_for_preimage(authorization);
    let mut message = Vec::new();
    push(&mut message, DECLARE_AUTHORIZATION_DOMAIN_TAG);
    push(&mut message, chain_id.as_bytes());
    push(&mut message, &account.0);
    push(&mut message, &encoded);
    push(&mut message, &nonce.to_le_bytes());
    push(&mut message, &generation.to_le_bytes());
    message
}

/// Stable, non-serde encoding for a declaration signing surface. Persistence
/// encoding is deliberately not reused: changing an in-memory layout must not
/// silently change signed bytes.
fn encode_authorization_for_preimage(authorization: &Option<AccountAuthorization>) -> Vec<u8> {
    fn push(out: &mut Vec<u8>, bytes: &[u8]) {
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(bytes);
    }

    let mut encoded = Vec::new();
    match authorization {
        None => push(&mut encoded, &[0]),
        Some(account) => {
            push(&mut encoded, &[1]);
            match &account.declared {
                None => push(&mut encoded, &[0]),
                Some(AuthorizationRule::SignerIsSender) => {
                    push(&mut encoded, &[1]);
                    push(&mut encoded, &[0]);
                },
                Some(AuthorizationRule::SmartAccount { program }) => {
                    push(&mut encoded, &[1]);
                    push(&mut encoded, &[1]);
                    push(&mut encoded, program);
                },
                Some(AuthorizationRule::Delegated { program, valid_until }) => {
                    push(&mut encoded, &[1]);
                    push(&mut encoded, &[2]);
                    push(&mut encoded, program);
                    match valid_until {
                        None => push(&mut encoded, &[0]),
                        Some(timestamp) => {
                            push(&mut encoded, &[1]);
                            push(&mut encoded, &timestamp.into_inner().to_le_bytes());
                        },
                    }
                },
                Some(AuthorizationRule::SignerPlusSecurityKey { policy }) => {
                    // Refused by `check_declaration` before any signature is
                    // checked; encoded only so the preimage stays total.
                    push(&mut encoded, &[1]);
                    push(&mut encoded, &[3]);
                    push(&mut encoded, &(policy.keys.len() as u64).to_le_bytes());
                    for key in &policy.keys {
                        push(&mut encoded, &key.public_key_p256);
                        push(&mut encoded, &key.rp_id_hash);
                        push(&mut encoded, &key.credential_id);
                    }
                    push(&mut encoded, &policy.escape_delay_blocks.to_le_bytes());
                },
            }
        },
    }
    encoded
}

/// Atomically apply a signed declaration. All validation precedes mutation.
pub fn apply_declaration(
    declarations: &mut BTreeMap<Address, AccountAuthorizationDeclaration>,
    declaration: DeclareAuthorization, height: u64, now: Timestamp, chain_id: &str,
) -> Result<(), DeclarationError> {
    let existing = declarations.get(&declaration.account).cloned().unwrap_or_default();
    if declaration.nonce != existing.next_nonce { return Err(DeclarationError::StaleNonce { expected: existing.next_nonce, actual: declaration.nonce }); }
    if declaration.generation != existing.generation { return Err(DeclarationError::StaleGeneration { expected: existing.generation, actual: declaration.generation }); }
    let next_nonce = existing.next_nonce.checked_add(1).ok_or(DeclarationError::NonceExhausted)?;
    let next_generation = existing.generation.checked_add(1).ok_or(DeclarationError::GenerationExhausted)?;
    let public_key = PublicKey::from_hex(&declaration.signer_public_key).map_err(|_| AuthorizationError::MalformedSignature)?;
    let signature = Signature::from_hex(&declaration.signature).map_err(|_| AuthorizationError::MalformedSignature)?;
    if Address::from_public_key(&public_key) != declaration.account { return Err(AuthorizationError::SignerIsNotSender.into()); }
    // A security-key policy in force can only be changed by a key-signed
    // `SecurityKeyAction` (or after its phrase-only escape matures): letting
    // the phrase key alone overwrite it here would bypass the key entirely.
    if existing.current.as_ref().and_then(|c| c.declared.as_ref()).is_some_and(|rule| {
        matches!(rule, AuthorizationRule::SignerPlusSecurityKey { policy } if policy.enforced_at(height))
    }) {
        return Err(AuthorizationError::SecurityKeyRequired.into());
    }
    if let Some(ref authorization) = declaration.authorization {
        if let Some(ref rule) = authorization.declared { check_declaration(rule, height, now)?; }
    }
    let message = declare_authorization_message(chain_id, &declaration.account, &declaration.authorization, declaration.nonce, declaration.generation);
    if !public_key.verify(&message, &signature) { return Err(AuthorizationError::InvalidSignature.into()); }
    declarations.insert(declaration.account, AccountAuthorizationDeclaration { next_nonce, current: declaration.authorization, generation: next_generation });
    Ok(())
}

impl AccountAuthorization {
    /// An account that has declared nothing — the state of every account today.
    pub fn none() -> Self {
        Self { declared: None }
    }

    pub fn declaring(rule: AuthorizationRule) -> Self {
        Self {
            declared: Some(rule),
        }
    }
}

/// The one question the spend path asks: **how is this account authorized at
/// this height?**
///
/// Pure, total, and defaulting to today's behaviour in every direction that
/// could go wrong:
///
/// - below the activation height, [`AuthorizationRule::SignerIsSender`]
///   without consulting `declaration` at all;
/// - for an account that declared nothing, the same;
/// - for a declaration that has expired, the same — an account whose delegation
///   lapses falls back to its own key rather than becoming unspendable.
///
/// Failing back to the sender's own key is the safe direction: the worst case
/// is that an account is authorized the way it was before it declared
/// anything, which is a rule it necessarily consented to by existing.
pub fn authorization_for(
    declaration: &AccountAuthorization,
    height: u64,
    now: Timestamp,
) -> AuthorizationRule {
    if !dispatch_active(height) {
        return AuthorizationRule::SignerIsSender;
    }
    match &declaration.declared {
        // A matured phrase-only escape lapses the security-key rule back to
        // the account's own key, exactly like an expired delegation.
        Some(AuthorizationRule::SignerPlusSecurityKey { policy }) if !policy.enforced_at(height) => {
            AuthorizationRule::SignerIsSender
        },
        Some(rule) if rule.in_force_at(now) => rule.clone(),
        _ => AuthorizationRule::SignerIsSender,
    }
}

/// Whether per-account authorization dispatch is in force at `height`.
///
/// The single gate. Producer, applier and replay all ask this one function, so
/// the three can never disagree about whether an account's declaration counts.
pub fn dispatch_active(height: u64) -> bool {
    feature_active(ConsensusFeature::AccountAuthorizationDispatch, height)
}

/// Why a declaration was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorizationError {
    /// The dispatch is not active at this height, so an account may not
    /// declare a rule yet. Refusing rather than storing keeps pre-activation
    /// state byte-identical to a chain without this feature.
    DispatchNotActive { height: u64 },
    /// A program hash of all zeroes is not a program.
    EmptyProgram,
    /// The declared expiry is already past.
    AlreadyExpired {
        valid_until: Timestamp,
        now: Timestamp,
    },
    /// The public key or signature was not valid hex-encoded ed25519 material.
    MalformedSignature,
    /// The supplied key does not derive to the account being debited.
    SignerIsNotSender,
    /// The signature does not verify over the transaction's canonical message.
    InvalidSignature,
    /// A declared policy exists, but its validator has not landed yet.
    RuleNotImplemented,
    /// The account requires a security-key assertion and none was supplied
    /// (or the action needs one).
    SecurityKeyRequired,
    /// The supplied security-key assertion did not verify for this debit.
    SecurityKeyInvalid,
    /// Security-key rules are written by `SecurityKeyAction` transactions, not
    /// by a generic declaration.
    SecurityKeyRuleIsActionManaged,
}

impl std::fmt::Display for AuthorizationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DispatchNotActive { height } => write!(
                f,
                "per-account authorization dispatch is not active at height {height} (#2168)"
            ),
            Self::EmptyProgram => write!(f, "authorization program hash is empty (#2168)"),
            Self::AlreadyExpired { valid_until, now } => write!(
                f,
                "authorization already expired at {} (now {}) (#2168)",
                valid_until.into_inner(),
                now.into_inner()
            ),
            Self::MalformedSignature => write!(f, "malformed debit authorization (#2168)"),
            Self::SignerIsNotSender => write!(f, "debit signer is not the sender (#2168)"),
            Self::InvalidSignature => write!(f, "invalid debit authorization signature (#2168)"),
            Self::RuleNotImplemented => {
                write!(f, "account authorization rule is not implemented (#2168)")
            },
            Self::SecurityKeyRequired => write!(
                f,
                "this account requires a security-key assertion to spend (security_key_spend)"
            ),
            Self::SecurityKeyInvalid => {
                write!(f, "security-key assertion does not verify for this debit (security_key_spend)")
            },
            Self::SecurityKeyRuleIsActionManaged => write!(
                f,
                "security-key rules are managed by SecurityKeyAction transactions (security_key_spend)"
            ),
        }
    }
}

/// Authorize one account debit through the per-account policy seam.
///
/// This first foundation revision implements only the chain's existing
/// `SignerIsSender` rule. Callers use [`EmptyAuthorizationView`], so routing
/// through this function is byte-for-byte equivalent to their former inline
/// checks. State-backed declarations and additional policy arms land later.
pub fn authorize_debit<V: AuthorizationView>(
    view: &V,
    from: &Address,
    message: &[u8],
    auth: SpendAuthorization<'_>,
    height: u64,
    now: Timestamp,
) -> Result<(), AuthorizationError> {
    authorize_debit_with_security_key(view, from, message, auth, None, height, now)
}

fn verify_signer_is_sender(
    from: &Address,
    message: &[u8],
    auth: SpendAuthorization<'_>,
) -> Result<(), AuthorizationError> {
    let public_key = PublicKey::from_hex(auth.signer_public_key)
        .map_err(|_| AuthorizationError::MalformedSignature)?;
    let signature =
        Signature::from_hex(auth.signature).map_err(|_| AuthorizationError::MalformedSignature)?;
    if Address::from_public_key(&public_key) != *from {
        return Err(AuthorizationError::SignerIsNotSender);
    }
    if !public_key.verify(message, &signature) {
        return Err(AuthorizationError::InvalidSignature);
    }
    Ok(())
}

/// [`authorize_debit`] with the optional security-key assertion a
/// transaction carries (`security_key_auth`). The assertion is consulted only
/// when the account's rule at `height` is
/// [`AuthorizationRule::SignerPlusSecurityKey`]; for every other account this
/// is exactly [`authorize_debit`].
///
/// Pre-block callers ([`EmptyAuthorizationView`]) never see a security-key
/// rule, so they check the ed25519 half only. That is safe because this
/// State-backed check is strictly stronger: it never accepts a debit the
/// empty view refuses (pinned by `security_key_spend_state_view_is_stricter`).
pub fn authorize_debit_with_security_key<V: AuthorizationView>(
    view: &V,
    from: &Address,
    message: &[u8],
    auth: SpendAuthorization<'_>,
    security_key: Option<&crate::accounts::security_key::SecurityKeyAssertionWire>,
    height: u64,
    now: Timestamp,
) -> Result<(), AuthorizationError> {
    let declaration = view.declared(from);
    match authorization_for(&declaration, height, now) {
        AuthorizationRule::SignerIsSender => verify_signer_is_sender(from, message, auth),
        AuthorizationRule::SignerPlusSecurityKey { policy } => {
            // Deny by default: a stored rule whose feature is not in force is
            // refused, never weakened.
            if !crate::accounts::security_key::security_key_active(height) {
                return Err(AuthorizationError::RuleNotImplemented);
            }
            verify_signer_is_sender(from, message, auth)?;
            crate::accounts::security_key::verify_spend(&policy, from, message, security_key)
                .map_err(|error| match error {
                    crate::accounts::security_key::SecurityKeyError::ProofRequired => {
                        AuthorizationError::SecurityKeyRequired
                    },
                    _ => AuthorizationError::SecurityKeyInvalid,
                })
        },
        AuthorizationRule::SmartAccount { .. } | AuthorizationRule::Delegated { .. } => {
            Err(AuthorizationError::RuleNotImplemented)
        },
    }
}

impl std::error::Error for AuthorizationError {}

/// Validate a rule an account wants to declare, at `height`.
///
/// Refuses outright below the activation: an account that cannot be dispatched
/// must not be able to record a rule that would silently take effect later.
pub fn check_declaration(
    rule: &AuthorizationRule,
    height: u64,
    now: Timestamp,
) -> Result<(), AuthorizationError> {
    if !dispatch_active(height) {
        return Err(AuthorizationError::DispatchNotActive { height });
    }
    // Do not store a rule until its validator exists. In particular, retaining
    // a Delegated rule with an unplumbed debit timestamp could lock an account
    // into RuleNotImplemented after scheduling; refusal is atomic in
    // `apply_declaration`, before either durable counter advances.
    if matches!(
        rule,
        AuthorizationRule::SmartAccount { .. } | AuthorizationRule::Delegated { .. }
    ) {
        return Err(AuthorizationError::RuleNotImplemented);
    }
    if matches!(rule, AuthorizationRule::SignerPlusSecurityKey { .. }) {
        return Err(AuthorizationError::SecurityKeyRuleIsActionManaged);
    }
    if let Some(program) = rule.program() {
        if program == [0u8; 32] {
            return Err(AuthorizationError::EmptyProgram);
        }
    }
    if let AuthorizationRule::Delegated {
        valid_until: Some(valid_until),
        ..
    } = rule
    {
        if *valid_until < now {
            return Err(AuthorizationError::AlreadyExpired {
                valid_until: *valid_until,
                now,
            });
        }
    }
    Ok(())
}

/// Convenience for callers that hold an address but no declaration yet.
/// Every account is `SignerIsSender` until it says otherwise.
pub fn default_authorization(_account: &Address) -> AccountAuthorization {
    AccountAuthorization::none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signature::PrivateKey;
    use rand::{SeedableRng, rngs::StdRng};

    const PROGRAM: [u8; 32] = [7u8; 32];

    fn smart() -> AuthorizationRule {
        AuthorizationRule::SmartAccount { program: PROGRAM }
    }

    fn delegated(valid_until: Option<u64>) -> AuthorizationRule {
        AuthorizationRule::Delegated {
            program: PROGRAM,
            valid_until: valid_until.map(Timestamp::new),
        }
    }

    /// The dormancy property, stated over the pure resolver. The scheduled
    /// half is pinned in `tests/account_authorization_dispatch_2168.rs`, which
    /// installs a real schedule; this half proves the resolver itself never
    /// consults a declaration it must not.
    #[test]
    fn an_undeclared_account_always_resolves_to_the_senders_own_key() {
        let none = AccountAuthorization::none();
        for height in [0u64, 1, 14_000, u64::MAX] {
            assert_eq!(
                authorization_for(&none, height, Timestamp::new(0)),
                AuthorizationRule::SignerIsSender,
                "height {height}"
            );
        }
        assert!(AuthorizationRule::SignerIsSender.is_default());
        assert!(!smart().is_default());
    }

    /// An expired delegation falls BACK to the account's own key rather than
    /// leaving the account unspendable. Locking an account out because a rule
    /// lapsed would turn an expiry into a permanent loss of funds.
    #[test]
    fn an_expired_rule_falls_back_to_the_senders_own_key_not_to_a_locked_account() {
        let rule = delegated(Some(100));
        assert!(rule.in_force_at(Timestamp::new(100)));
        assert!(rule.in_force_at(Timestamp::new(99)));
        assert!(!rule.in_force_at(Timestamp::new(101)));

        // A rule with no expiry never lapses.
        assert!(delegated(None).in_force_at(Timestamp::new(u64::MAX)));
        assert!(smart().in_force_at(Timestamp::new(u64::MAX)));
    }

    #[test]
    fn a_rule_exposes_the_program_it_dispatches_to() {
        assert_eq!(AuthorizationRule::SignerIsSender.program(), None);
        assert_eq!(smart().program(), Some(PROGRAM));
        assert_eq!(delegated(None).program(), Some(PROGRAM));
    }

    /// Declaring is refused below the activation. An account that cannot be
    /// dispatched must not be able to record a rule that would silently take
    /// effect the moment the feature activates.
    #[test]
    fn a_declaration_is_refused_while_the_dispatch_is_dormant() {
        // No schedule is installed in this unit-test binary, so the dispatch
        // is inactive at every height — which is itself the default-off
        // property worth pinning.
        assert!(!dispatch_active(0));
        assert!(!dispatch_active(u64::MAX));
        assert_eq!(
            check_declaration(&smart(), 14_000, Timestamp::new(0)),
            Err(AuthorizationError::DispatchNotActive { height: 14_000 })
        );
        // And the resolver ignores a declaration it somehow holds.
        assert_eq!(
            authorization_for(
                &AccountAuthorization::declaring(smart()),
                14_000,
                Timestamp::new(0)
            ),
            AuthorizationRule::SignerIsSender,
            "a declaration must not take effect while the dispatch is dormant"
        );
    }

    #[test]
    fn error_messages_name_the_issue_and_the_values() {
        let e = AuthorizationError::DispatchNotActive { height: 9 };
        assert!(e.to_string().contains("#2168"), "{e}");
        assert!(e.to_string().contains('9'), "{e}");
        let e = AuthorizationError::AlreadyExpired {
            valid_until: Timestamp::new(5),
            now: Timestamp::new(7),
        };
        assert!(
            e.to_string().contains('5') && e.to_string().contains('7'),
            "{e}"
        );
        assert!(
            AuthorizationError::EmptyProgram
                .to_string()
                .contains("#2168")
        );
    }

    #[test]
    fn default_authorization_declares_nothing() {
        assert_eq!(
            default_authorization(&Address([1u8; 20])),
            AccountAuthorization::none()
        );
        assert_eq!(AccountAuthorization::none().declared, None);
    }

    #[test]
    fn signer_is_sender_seam_matches_the_legacy_predicate() {
        fn legacy(from: &Address, message: &[u8], key: &str, signature: &str) -> bool {
            let Ok(key) = PublicKey::from_hex(key) else {
                return false;
            };
            let Ok(signature) = Signature::from_hex(signature) else {
                return false;
            };
            Address::from_public_key(&key) == *from && key.verify(message, &signature)
        }

        let mut rng = StdRng::seed_from_u64(2_168);
        let private_key = PrivateKey::generate(&mut rng);
        let public_key = PublicKey::from_private_key(&private_key);
        let owner = Address::from_public_key(&public_key);
        let message = b"transparent transfer nonce 7";
        let wrong_nonce_message = b"transparent transfer nonce 8";
        let signature = private_key.sign(message);
        let key_hex = public_key.to_hex();
        let signature_hex = signature.to_hex();

        let cases = [
            (
                owner,
                message.as_slice(),
                key_hex.as_str(),
                signature_hex.as_str(),
            ),
            (
                Address([9u8; 20]),
                message.as_slice(),
                key_hex.as_str(),
                signature_hex.as_str(),
            ),
            (
                owner,
                wrong_nonce_message.as_slice(),
                key_hex.as_str(),
                signature_hex.as_str(),
            ),
            (
                owner,
                message.as_slice(),
                "not-a-public-key",
                signature_hex.as_str(),
            ),
        ];

        for (from, message, key, signature) in cases {
            let old = legacy(&from, message, key, signature);
            let new = authorize_debit(
                &EmptyAuthorizationView,
                &from,
                message,
                SpendAuthorization {
                    signer_public_key: key,
                    signature,
                },
                u64::MAX,
                Timestamp::new(0),
            )
            .is_ok();
            assert_eq!(old, new, "old/new authorization parity must hold");
        }
    }
}
