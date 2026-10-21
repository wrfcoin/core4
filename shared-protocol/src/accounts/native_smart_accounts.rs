//! Canonical signing subjects for the unscheduled native smart-account design.
//!
//! These types do not add a transaction variant, admit transactions, or mutate
//! State. The bounded wire decoder and state-backed authority are separate gates.
use crate::common::types::Address;
use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion, domain_preimage_bound};
use crate::crypto::domain_impls::BodyBuilder;
use crate::crypto::domain_signing::DomainSeparated;
use crate::crypto::hash::hash_bytes;
use crate::crypto::signature::{PublicKey, Signature};

pub const MAX_NATIVE_CALLS: usize = 16;
pub const MAX_NATIVE_GUARDIANS: usize = 16;
pub const MAX_NATIVE_TARGETS: usize = 64;
pub const NATIVE_ADDRESS_DOMAIN: &[u8] = b"wrfcoin.native_smart_account.address.v1";

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)] // secret-leak-ok: contains only a static error label, never key or signature material
#[error("malformed native account signing subject: {0}")]
pub struct NativeSigningError(pub &'static str);

/// Stable tags; adding a role must never change an existing value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum NativeActorRole {
    Owner = 0,
    Session = 1,
    External = 2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeTransfer {
    pub to: Address,
    pub amount: u64,
}

/// V1 permits the Transfer method only. Target scopes are strictly increasing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeSessionGrant {
    pub public_key: [u8; 32],
    pub generation: u64,
    pub targets: Vec<Address>,
    pub per_use_cap: u64,
    pub total_cap: u64,
    pub valid_from: u64,
    pub valid_until: u64,
    pub max_uses: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeRecoveryPolicy {
    pub guardians: Vec<[u8; 32]>,
    pub threshold: u32,
    pub time_lock_blocks: u64,
    pub cooldown_blocks: u64,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeRecoveryTarget {
    pub request_nonce: u64,
    pub policy_generation: u64,
    pub new_owner: [u8; 32],
}

/// Explicit action tags are encoded below, independently of Rust enum layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeAccountAction {
    Create {
        owner: [u8; 32],
    },
    Batch(Vec<NativeTransfer>),
    RegisterSession(NativeSessionGrant),
    RevokeSession {
        public_key: [u8; 32],
        generation: u64,
    },
    SetRecoveryPolicy(NativeRecoveryPolicy),
    ApproveRecovery(NativeRecoveryTarget),
    CancelRecovery(NativeRecoveryTarget),
    ExecuteRecovery(NativeRecoveryTarget),
    SponsorDeposit {
        amount: u64,
    },
    SponsorRequestWithdrawal {
        amount: u64,
        request_nonce: u64,
    },
    SponsorWithdrawal {
        amount: u64,
        request_nonce: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeSponsorTerms {
    pub public_key: [u8; 32],
    pub nonce: u64,
    pub fee_cap: u64,
    pub valid_from: u64,
    pub valid_until: u64,
}

/// Signature-free subject; all actor, batch and sponsor terms are authenticated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeAccountSubject {
    pub chain_id: String,
    pub network_id: String,
    pub genesis_hash: [u8; 32],
    pub schema_version: u32,
    pub target: Address,
    pub actor: [u8; 32],
    pub role: NativeActorRole,
    pub nonce: u64,
    pub owner_epoch: u64,
    pub valid_from: u64,
    pub valid_until: u64,
    pub fee_cap: u64,
    pub action: NativeAccountAction,
    pub sponsor: Option<NativeSponsorTerms>,
}

fn identity(chain: &str, network: &str) -> Result<ChainIdentity, NativeSigningError> {
    for s in [chain, network] {
        if s.is_empty()
            || s.len() > 64
            || !s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(NativeSigningError("identity"));
        }
    }
    Ok(ChainIdentity::new(chain, network))
}

fn key(k: &[u8; 32]) -> Result<(), NativeSigningError> {
    PublicKey::from_bytes(k)
        .map(|_| ())
        .map_err(|_| NativeSigningError("public key"))
}

fn window(from: u64, until: u64) -> Result<(), NativeSigningError> {
    if from > until {
        return Err(NativeSigningError("height window"));
    }
    Ok(())
}

fn ordered<T: Ord>(items: &[T], max: usize) -> Result<(), NativeSigningError> {
    if items.is_empty() || items.len() > max || items.windows(2).any(|w| w[0] >= w[1]) {
        return Err(NativeSigningError("scope/set bounds or ordering"));
    }
    Ok(())
}

impl NativeAccountSubject {
    /// Cheap shape checks precede signing; authority/nonce/fee checks need State.
    pub fn validate_shape(&self) -> Result<(), NativeSigningError> {
        identity(&self.chain_id, &self.network_id)?;
        if self.schema_version != 1 {
            return Err(NativeSigningError("schema"));
        }
        key(&self.actor)?;
        window(self.valid_from, self.valid_until)?;
        match &self.action {
            NativeAccountAction::Create { owner } => key(owner)?,
            NativeAccountAction::Batch(calls) => {
                if calls.is_empty() || calls.len() > MAX_NATIVE_CALLS {
                    return Err(NativeSigningError("batch bounds"));
                }
                calls.iter().try_fold(0u64, |sum, c| {
                    if c.amount == 0 {
                        return Err(NativeSigningError("zero amount"));
                    }
                    sum.checked_add(c.amount)
                        .ok_or(NativeSigningError("batch sum overflow"))
                })?;
            },
            NativeAccountAction::RegisterSession(g) => {
                key(&g.public_key)?;
                ordered(&g.targets, MAX_NATIVE_TARGETS)?;
                window(g.valid_from, g.valid_until)?;
                if g.per_use_cap == 0 || g.per_use_cap > g.total_cap || g.max_uses == 0 {
                    return Err(NativeSigningError("session caps"));
                }
            },
            NativeAccountAction::RevokeSession { public_key, .. } => key(public_key)?,
            NativeAccountAction::SetRecoveryPolicy(p) => {
                ordered(&p.guardians, MAX_NATIVE_GUARDIANS)?;
                for guardian in &p.guardians {
                    key(guardian)?;
                }
                if p.threshold == 0 || p.threshold as usize > p.guardians.len() {
                    return Err(NativeSigningError("guardian threshold"));
                }
            },
            NativeAccountAction::ApproveRecovery(r)
            | NativeAccountAction::CancelRecovery(r)
            | NativeAccountAction::ExecuteRecovery(r) => key(&r.new_owner)?,
            NativeAccountAction::SponsorDeposit { amount }
            | NativeAccountAction::SponsorRequestWithdrawal { amount, .. }
            | NativeAccountAction::SponsorWithdrawal { amount, .. } => {
                if *amount == 0 {
                    return Err(NativeSigningError("zero amount"));
                }
            },
        }
        if let Some(s) = &self.sponsor {
            key(&s.public_key)?;
            window(s.valid_from, s.valid_until)?;
        }
        Ok(())
    }

    /// Always explicit V2; never the process's ambient identity or version.
    pub fn preimage(&self) -> Result<Vec<u8>, NativeSigningError> {
        self.validate_shape()?;
        Ok(self.signing_preimage_for(
            &identity(&self.chain_id, &self.network_id)?,
            SigningProtocolVersion::V2,
        ))
    }
}

impl NativeAccountAction {
    fn body(&self) -> Vec<u8> {
        let b = BodyBuilder::new();
        match self {
            Self::Create { owner } => b.u32_field(0).bytes_field(owner),
            Self::Batch(calls) => b.u32_field(1).seq_field(calls.iter().map(|c| {
                BodyBuilder::new()
                    .bytes_field(&c.to.0)
                    .u64_field(c.amount)
                    .finish()
            })),
            Self::RegisterSession(g) => b
                .u32_field(2)
                .bytes_field(&g.public_key)
                .u64_field(g.generation)
                .seq_field(g.targets.iter().map(|t| t.0.to_vec()))
                .u64_field(g.per_use_cap)
                .u64_field(g.total_cap)
                .u64_field(g.valid_from)
                .u64_field(g.valid_until)
                .u64_field(g.max_uses),
            Self::RevokeSession {
                public_key,
                generation,
            } => b
                .u32_field(3)
                .bytes_field(public_key)
                .u64_field(*generation),
            Self::SetRecoveryPolicy(p) => b
                .u32_field(4)
                .seq_field(p.guardians.iter().map(|k| k.to_vec()))
                .u32_field(p.threshold)
                .u64_field(p.time_lock_blocks)
                .u64_field(p.cooldown_blocks)
                .u64_field(p.generation),
            Self::ApproveRecovery(r) => recovery_body(b.u32_field(5), r),
            Self::CancelRecovery(r) => recovery_body(b.u32_field(6), r),
            Self::ExecuteRecovery(r) => recovery_body(b.u32_field(7), r),
            Self::SponsorDeposit { amount } => b.u32_field(8).u64_field(*amount),
            Self::SponsorRequestWithdrawal {
                amount,
                request_nonce,
            } => b.u32_field(9).u64_field(*amount).u64_field(*request_nonce),
            Self::SponsorWithdrawal {
                amount,
                request_nonce,
            } => b.u32_field(10).u64_field(*amount).u64_field(*request_nonce),
        }
        .finish()
    }
}

fn recovery_body(b: BodyBuilder, r: &NativeRecoveryTarget) -> BodyBuilder {
    b.u64_field(r.request_nonce)
        .u64_field(r.policy_generation)
        .bytes_field(&r.new_owner)
}

impl NativeSponsorTerms {
    fn body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .bytes_field(&self.public_key)
            .u64_field(self.nonce)
            .u64_field(self.fee_cap)
            .u64_field(self.valid_from)
            .u64_field(self.valid_until)
            .finish()
    }
}

impl DomainSeparated for NativeAccountSubject {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.native_smart_account.tx.v1";
    fn canonical_body(&self) -> Vec<u8> {
        let sponsor = self.sponsor.as_ref().map(NativeSponsorTerms::body);
        BodyBuilder::new()
            .str_field(&self.chain_id)
            .str_field(&self.network_id)
            .bytes_field(&self.genesis_hash)
            .u32_field(self.schema_version)
            .bytes_field(&self.target.0)
            .bytes_field(&self.actor)
            .u32_field(self.role as u32)
            .u64_field(self.nonce)
            .u64_field(self.owner_epoch)
            .u64_field(self.valid_from)
            .u64_field(self.valid_until)
            .u64_field(self.fee_cap)
            .bytes_field(&self.action.body())
            .opt_bytes_field(sponsor.as_deref())
            .finish()
    }
}

/// Sponsor signature binds the unsigned account preimage digest and its terms.
/// Construct from the actual account subject, never from untrusted supplied hashes.
pub struct NativeSponsorSubject {
    account_digest: [u8; 32],
    terms: NativeSponsorTerms,
    identity: ChainIdentity,
}

impl NativeSponsorSubject {
    pub fn from_account(s: &NativeAccountSubject) -> Result<Self, NativeSigningError> {
        Ok(Self {
            account_digest: *hash_bytes(&s.preimage()?).as_bytes(),
            terms: s
                .sponsor
                .clone()
                .ok_or(NativeSigningError("absent sponsor"))?,
            identity: identity(&s.chain_id, &s.network_id)?,
        })
    }
    pub fn preimage(&self) -> Vec<u8> {
        self.signing_preimage_for(&self.identity, SigningProtocolVersion::V2)
    }
}

impl DomainSeparated for NativeSponsorSubject {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.native_smart_account.sponsor.v1";
    fn canonical_body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .bytes_field(&self.account_digest)
            .bytes_field(&self.terms.body())
            .finish()
    }
}

/// Session-key possession binds the entire owner-approved registration subject.
pub struct NativeSessionPossession {
    account_digest: [u8; 32],
    identity: ChainIdentity,
}

impl NativeSessionPossession {
    pub fn from_account(s: &NativeAccountSubject) -> Result<Self, NativeSigningError> {
        if !matches!(s.action, NativeAccountAction::RegisterSession(_)) {
            return Err(NativeSigningError("not session registration"));
        }
        Ok(Self {
            account_digest: *hash_bytes(&s.preimage()?).as_bytes(),
            identity: identity(&s.chain_id, &s.network_id)?,
        })
    }
    pub fn preimage(&self) -> Vec<u8> {
        self.signing_preimage_for(&self.identity, SigningProtocolVersion::V2)
    }
}

impl DomainSeparated for NativeSessionPossession {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.native_smart_account.session_possession.v1";
    fn canonical_body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .bytes_field(&self.account_digest)
            .finish()
    }
}

/// Signature container, not another signed subject. No signature-signature cycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeSignedAccount {
    pub subject: NativeAccountSubject,
    pub signature: Signature,
    pub sponsor_signature: Option<Signature>,
    pub session_signature: Option<Signature>,
}

/// SHA3-256, first 20 bytes, matching Address::from_public_key's conversion.
/// The address tag is a derivation domain, not an additional signed-object kind.
pub fn native_account_address(
    chain: &ChainIdentity,
    genesis: &[u8; 32],
    creator: &[u8; 32],
    nonce: u64,
) -> Result<Address, NativeSigningError> {
    identity(&chain.chain_id, &chain.network_id)?;
    key(creator)?;
    let body = BodyBuilder::new()
        .bytes_field(genesis)
        .bytes_field(creator)
        .u64_field(nonce)
        .finish();
    let bytes = domain_preimage_bound(
        NATIVE_ADDRESS_DOMAIN,
        chain,
        SigningProtocolVersion::V2,
        &body,
    );
    let hash = hash_bytes(&bytes);
    Ok(Address(
        hash.as_bytes()[..20]
            .try_into()
            .expect("20-byte hash prefix"),
    ))
}
