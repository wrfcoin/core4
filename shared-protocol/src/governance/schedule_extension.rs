//! Governance-signed feature-schedule extensions (#3277 S1).
//!
//! A [`FeatureScheduleExtension`] appends future `(feature, activation
//! height)` entries to a running chain's consensus feature schedule, so a
//! feature can be activated without a genesis cut (design:
//! `docs/plans/2026-09-24-live-feature-schedule-extension-design.md`, §1.2).
//!
//! This module is the artifact only: the type, its [`DomainSeparated`]
//! preimage under its OWN tag (`wrfcoin.feature_schedule_extension.v1`,
//! [`DomainSigningAction::FeatureScheduleExtension`]), sign / verify, and the
//! state-free validity rules. Carrying it in a transaction, applying it to
//! `State` and consulting the extended schedule are later stories (S2–S5).
//!
//! **Why a new tag and not `upgrade_notice.v1`.** A notice is advisory,
//! off-chain and transport-untrusted; an extension is consensus. Sharing a
//! tag would let a harvested notice signature be presented as an extension,
//! or the reverse — the cross-action replay `DomainSigningAction` exists to
//! prevent. The [`GovernanceSignature`] container and the `Vec` of signatures
//! ARE reused, so #2156 threshold signing stays additive.
//!
//! **Which rules live here.** The design's validation table (§1.3) splits
//! into rules that need only the artifact plus the verifier's context, and
//! rules that need the parent `State`:
//!
//! | Rule | Here | Where the rest lives |
//! |---|---|---|
//! | V2 chain / network / genesis-hash binding | [`verify_feature_schedule_extension`] | the durable block-1 hash is supplied by the caller |
//! | V4 `1 ≤ entries + version_steps`, `entries ≤ 8`, `version_steps ≤ 1`, a version step alone | [`FeatureScheduleExtension::check_well_formed`] | — |
//! | V5 name grammar, not a version token, not `signing_v2`, no duplicate | [`FeatureScheduleExtension::check_well_formed`] | — |
//! | V7 `h + MIN_RUNUP ≤ activation ≤ h + MAX_HORIZON` | [`FeatureScheduleExtension::check_activation_window`] | `h` is the carrying block's height |
//! | V8 (in-tx half) entries sorted by `(height, name)` | [`FeatureScheduleExtension::check_well_formed`] | the cross-extension half needs `State` (S2) |
//! | V9 signatures | [`verify_feature_schedule_extension`] | — |
//! | V1, V3, V6, V8 (cross-extension) | — | `State` / the effective schedule (S2, S3, S5) |
//! | V10 version-step semantics (`MIN_RUNUP_SIGNING`, one version forward) | — | S11; the `version_steps` field is already in the signed preimage |
//!
//! **What is deliberately NOT a rule here: "known feature names only".**
//! Consensus accepts any well-formed name, exactly as a genesis schedule
//! does, so an old binary never rejects a block for naming a feature it
//! lacks (it halts cleanly instead, #3093). "Known only" is admission policy
//! (mempool / producer / the signing CLI), never validity.
//!
//! The preimage is an explicit, length-prefixed [`BodyBuilder`] encoding —
//! never bincode — bound to the chain and network NAMED IN THE EXTENSION at
//! [`SigningProtocolVersion::V2`], like [`super::upgrade_notice`].

use serde::{Deserialize, Serialize};

use crate::blockchain::chain::MAX_REORG_DEPTH;
use crate::common::types::Address;
use crate::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, FAUCET_SINK_BINDING_PREFIX, SigningProtocolVersion,
    faucet_binding_row_for, is_feature_name, is_version_token,
};
use crate::crypto::domain_impls::BodyBuilder;
use crate::crypto::domain_signing::{DomainSeparated, DomainSigningAction};
use crate::crypto::signature::{PrivateKey, PublicKey, Signature};
use crate::crypto::signer::BlockSigner;

use super::faucet_sink_pop::{
    FaucetSinkPossession, FaucetSinkPossessionProof, verify_faucet_sink_possession,
};
pub use super::upgrade_notice::GovernanceSignature;

/// The signing protocol version every extension is bound at. Extensions are
/// chain-bound by construction; there is no unbound (V1) form — even on a
/// chain whose own signing schedule is still v1.
pub const SCHEDULE_EXTENSION_SIGNING_VERSION: SigningProtocolVersion = SigningProtocolVersion::V2;

/// Minimum run-up, in blocks, between the block carrying an extension and
/// any activation it schedules (owner ruling D2, 2026-09-24).
///
/// Must exceed [`MAX_REORG_DEPTH`]: every entry active at an evaluable
/// height was then carried below every possible fork point, which is what
/// makes `feature_active` branch-independent (design §1.5). The
/// compile-time assertion below enforces it.
pub const MIN_RUNUP_BLOCKS: u64 = 2_000;

/// Maximum distance, in blocks, from the carrying block to any activation
/// it schedules (owner ruling D9) — guards a fat-fingered far-future height.
pub const MAX_HORIZON_BLOCKS: u64 = 500_000;

/// Maximum entries one extension may carry (owner ruling D9): bounded work
/// and bounded state per transaction.
pub const MAX_ENTRIES_PER_EXTENSION: usize = 8;

/// Maximum signing-version steps one extension may carry (V4). A step is
/// also the only content of its extension, so its risk never shares a
/// transaction (or, through V8, a height) with a feature activation.
pub const MAX_VERSION_STEPS_PER_EXTENSION: usize = 1;

/// Maximum byte length of one feature name. The longest name this binary
/// implements is 30 bytes; the cap bounds the preimage and state an
/// extension can force, since the name grammar alone is unbounded.
pub const MAX_FEATURE_NAME_LEN: usize = 64;

/// Maximum byte length of the informational `release_commit` (a full SHA-1
/// or SHA-256 git object id fits with room to spare).
pub const MAX_RELEASE_COMMIT_LEN: usize = 128;

/// Byte length of a canonical genesis-hash hex string (32 bytes, lowercase).
pub const GENESIS_HASH_HEX_LEN: usize = 64;

// The determinism argument (design §1.5) needs the run-up to outlast any
// reorg the node will perform, and the window to be non-empty.
const _: () = assert!(MIN_RUNUP_BLOCKS > MAX_REORG_DEPTH);
const _: () = assert!(MIN_RUNUP_BLOCKS <= MAX_HORIZON_BLOCKS);

/// One scheduled activation: `feature` switches on at `activation_height`
/// (the FIRST height at which it applies, as in a genesis schedule).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScheduleEntry {
    pub feature: String,
    pub activation_height: u64,
}

impl ScheduleEntry {
    pub fn new(feature: impl Into<String>, activation_height: u64) -> Self {
        Self {
            feature: feature.into(),
            activation_height,
        }
    }
}

/// One signing-protocol version step (design §1.7): the chain's signing
/// version becomes `version` from `activation_height` on.
///
/// S1 carries and signs this field so the preimage never has to change
/// (ruling 2026-09-24 on #3281): an extension signed before signing-step
/// support exists (E0) must keep verifying on apply and WAL replay after it
/// arrives. Its SEMANTICS — V10 (exactly one version forward, height
/// `≥ h + MIN_RUNUP_SIGNING` and ≥ every scheduled feature) and the
/// refreshable version table — are story S11 and are NOT implemented here.
/// `version` is a raw `u32` so a binary that lacks the version still parses
/// it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VersionStep {
    pub activation_height: u64,
    pub version: u32,
}

/// The governance-authorized append to a chain's feature schedule.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FeatureScheduleExtension {
    /// Must equal the carrying chain's `ChainIdentity`.
    pub chain_id: String,
    pub network_id: String,
    /// Lowercase hex of the chain's durable block-1 hash. Binds the
    /// extension to THIS chain instance, so a signature is useless on a
    /// re-cut chain that reuses the same id.
    pub genesis_hash: String,
    /// Must equal the number of extensions the chain has already applied
    /// (replay protection and ordering; enforced against `State`, S2).
    pub sequence: u64,
    /// Sorted ascending by `(activation_height, feature)`.
    pub entries: Vec<ScheduleEntry>,
    /// At most one signing-version step, and only in an extension that
    /// carries no feature entries (V4). Empty for every feature extension,
    /// including E0. `#[serde(default)]` lets a JSON artifact omit it; the
    /// signed preimage always encodes it (count-prefixed).
    #[serde(default)]
    pub version_steps: Vec<VersionStep>,
    /// The `PROTOCOL_VERSION` of the release that implements `entries`.
    pub min_protocol_version: u32,
    /// Informational, but signed: the core4 commit of that release (ties the
    /// extension to its `UpgradeNotice`).
    pub release_commit: String,
}

impl FeatureScheduleExtension {
    /// The identity this extension is bound to.
    pub fn identity(&self) -> ChainIdentity {
        ChainIdentity::new(self.chain_id.clone(), self.network_id.clone())
    }

    /// The exact bytes a governance signer signs.
    pub fn preimage(&self) -> Vec<u8> {
        self.signing_preimage_for(&self.identity(), SCHEDULE_EXTENSION_SIGNING_VERSION)
    }

    /// State-free well-formedness: V4, V5, the in-transaction half of V8,
    /// and the shape of `genesis_hash` / `release_commit`. Pure function of
    /// the artifact.
    pub fn check_well_formed(&self) -> Result<(), ScheduleExtensionError> {
        if self.chain_id.trim().is_empty() || self.network_id.trim().is_empty() {
            return Err(ScheduleExtensionError::MissingIdentity);
        }
        if !is_canonical_genesis_hash(&self.genesis_hash) {
            return Err(ScheduleExtensionError::MalformedGenesisHash {
                found: self.genesis_hash.clone(),
            });
        }
        if self.release_commit.len() > MAX_RELEASE_COMMIT_LEN {
            return Err(ScheduleExtensionError::ReleaseCommitTooLong {
                len: self.release_commit.len(),
                max: MAX_RELEASE_COMMIT_LEN,
            });
        }
        // V4: at least one entry or version step; entries ≤ 8; version
        // steps ≤ 1; a version step is the only content of its extension.
        if self.entries.is_empty() && self.version_steps.is_empty() {
            return Err(ScheduleExtensionError::NoEntries);
        }
        if self.version_steps.len() > MAX_VERSION_STEPS_PER_EXTENSION {
            return Err(ScheduleExtensionError::TooManyVersionSteps {
                count: self.version_steps.len(),
                max: MAX_VERSION_STEPS_PER_EXTENSION,
            });
        }
        if !self.version_steps.is_empty() && !self.entries.is_empty() {
            return Err(ScheduleExtensionError::VersionStepNotAlone {
                entries: self.entries.len(),
            });
        }
        if self.entries.len() > MAX_ENTRIES_PER_EXTENSION {
            return Err(ScheduleExtensionError::TooManyEntries {
                count: self.entries.len(),
                max: MAX_ENTRIES_PER_EXTENSION,
            });
        }
        for entry in &self.entries {
            check_feature_name(&entry.feature)?;
        }
        for (index, pair) in self.entries.windows(2).enumerate() {
            let (a, b) = (&pair[0], &pair[1]);
            let order = a
                .activation_height
                .cmp(&b.activation_height)
                .then_with(|| a.feature.cmp(&b.feature));
            if order != std::cmp::Ordering::Less {
                return Err(if a.feature == b.feature {
                    ScheduleExtensionError::DuplicateFeature {
                        feature: b.feature.clone(),
                    }
                } else {
                    ScheduleExtensionError::EntriesNotSorted { index: index + 1 }
                });
            }
        }
        // Sorted with no equal neighbours still allows the same name at two
        // different heights; refuse that too.
        let mut names: Vec<&str> = self.entries.iter().map(|e| e.feature.as_str()).collect();
        names.sort_unstable();
        if let Some(pair) = names.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(ScheduleExtensionError::DuplicateFeature {
                feature: pair[0].to_string(),
            });
        }
        Ok(())
    }

    /// V7: every FEATURE activation lies in
    /// `[h + MIN_RUNUP_BLOCKS, h + MAX_HORIZON_BLOCKS]`, where
    /// `carrying_height` (`h`) is the height of the block carrying the
    /// extension. Overflow-safe: a window bound past `u64::MAX` saturates.
    /// Version steps have their own window (V10, `MIN_RUNUP_SIGNING`), which
    /// is story S11's and is not checked here.
    pub fn check_activation_window(
        &self,
        carrying_height: u64,
    ) -> Result<(), ScheduleExtensionError> {
        let earliest = carrying_height.saturating_add(MIN_RUNUP_BLOCKS);
        let latest = carrying_height.saturating_add(MAX_HORIZON_BLOCKS);
        for entry in &self.entries {
            if entry.activation_height < earliest {
                return Err(ScheduleExtensionError::ActivationTooSoon {
                    feature: entry.feature.clone(),
                    activation_height: entry.activation_height,
                    earliest,
                });
            }
            if entry.activation_height > latest {
                return Err(ScheduleExtensionError::ActivationTooFar {
                    feature: entry.feature.clone(),
                    activation_height: entry.activation_height,
                    latest,
                });
            }
        }
        Ok(())
    }

    /// The first entry naming a feature this binary does not implement.
    ///
    /// NOT a validity rule (see the module docs): admission policy and the
    /// signing CLI refuse such an extension; a block carrying one is still
    /// valid and the node halts cleanly at its height instead.
    pub fn first_unknown_feature(&self, known: &[ConsensusFeature]) -> Option<&ScheduleEntry> {
        self.entries
            .iter()
            .find(|entry| !known.iter().any(|k| k.name() == entry.feature))
    }
}

impl DomainSeparated for FeatureScheduleExtension {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.feature_schedule_extension.v1";

    fn canonical_body(&self) -> Vec<u8> {
        // Every field, length-prefixed by BodyBuilder; `entries` is
        // count-prefixed and encoded in the order given (the order is part
        // of what was signed, and V8 requires it canonical anyway).
        let mut builder = BodyBuilder::new()
            .str_field(&self.chain_id)
            .str_field(&self.network_id)
            .str_field(&self.genesis_hash)
            .u64_field(self.sequence)
            .u64_field(self.entries.len() as u64);
        for entry in &self.entries {
            builder = builder
                .str_field(&entry.feature)
                .u64_field(entry.activation_height);
        }
        // Always present, count-prefixed (0 for a feature extension), so the
        // preimage is final from S1 on and S11 changes no signed byte.
        builder = builder.u64_field(self.version_steps.len() as u64);
        for step in &self.version_steps {
            builder = builder
                .u64_field(step.activation_height)
                .u32_field(step.version);
        }
        builder
            .u32_field(self.min_protocol_version)
            .str_field(&self.release_commit)
            .finish()
    }
}

/// An extension plus its governance signatures — the artifact a transaction
/// will carry (S4).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SignedFeatureScheduleExtension {
    pub extension: FeatureScheduleExtension,
    pub signatures: Vec<GovernanceSignature>,
}

/// What a verifier checks an extension against. `genesis_hash` is the
/// lowercase hex of the chain's durable block-1 hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleExtensionContext<'a> {
    pub chain_id: &'a str,
    pub network_id: &'a str,
    pub genesis_hash: &'a str,
    /// Governance public keys (hex, any case).
    pub allowlist: &'a [String],
}

/// Why an extension was refused. Every case is a named error; none panics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleExtensionError {
    /// `chain_id` or `network_id` is empty.
    MissingIdentity,
    /// The extension names another chain or network than the verifier's.
    WrongChain {
        expected: ChainIdentity,
        found: ChainIdentity,
    },
    /// `genesis_hash` is not 64 lowercase hex characters.
    MalformedGenesisHash { found: String },
    /// The extension is bound to another chain instance (block-1 hash).
    WrongGenesis { expected: String, found: String },
    /// `release_commit` exceeds [`MAX_RELEASE_COMMIT_LEN`].
    ReleaseCommitTooLong { len: usize, max: usize },
    /// Neither a feature entry nor a version step.
    NoEntries,
    /// More than [`MAX_VERSION_STEPS_PER_EXTENSION`] version steps.
    TooManyVersionSteps { count: usize, max: usize },
    /// A version step shares its extension with feature entries.
    VersionStepNotAlone { entries: usize },
    /// More than [`MAX_ENTRIES_PER_EXTENSION`] entries.
    TooManyEntries { count: usize, max: usize },
    /// A name is not `[a-z][a-z0-9_]*`, is empty, or exceeds
    /// [`MAX_FEATURE_NAME_LEN`].
    MalformedFeatureName { feature: String },
    /// A name is a signing-version token (`vN`); versions are not scheduled
    /// by extension.
    VersionTokenAsFeature { feature: String },
    /// `signing_v2` is scheduled through the version table, never by name.
    SigningVersionAsFeature,
    /// The same feature appears twice in one extension.
    DuplicateFeature { feature: String },
    /// Entries are not strictly ascending by `(activation_height, feature)`;
    /// `index` is the first out-of-order entry.
    EntriesNotSorted { index: usize },
    /// An activation is closer than [`MIN_RUNUP_BLOCKS`] to the carrying block.
    ActivationTooSoon {
        feature: String,
        activation_height: u64,
        earliest: u64,
    },
    /// An activation is further than [`MAX_HORIZON_BLOCKS`] from the carrying
    /// block.
    ActivationTooFar {
        feature: String,
        activation_height: u64,
        latest: u64,
    },
    /// No signatures at all.
    NoSignatures,
    /// A signer is not on the governance allowlist.
    SignerNotAuthorized { signer_pubkey: String },
    /// A signature does not verify over the extension — it was altered after
    /// signing, the key or signature hex is malformed, or a signer is listed
    /// twice.
    BadSignature {
        signer_pubkey: String,
        reason: String,
    },
    /// The signer could not produce a signature (an HSM refusal, say).
    SignerFailed { reason: String },
    /// V3: `sequence` is not the number of extensions already applied.
    WrongSequence { expected: u64, found: u64 },
    /// V6: the feature is already scheduled (genesis, bootstrap or an earlier
    /// extension). A rule change to a scheduled feature needs a NEW name.
    AlreadyScheduled { feature: String },
    /// V8 (cross-extension): the first new activation is below an
    /// activation already in the effective schedule (strict append, D5).
    NotAppendOrder {
        activation_height: u64,
        latest_scheduled: u64,
    },
    /// A signing-version step: its validity rules (V10) are story S11 and
    /// this binary refuses every step until they exist behind their own
    /// activation (ruling 2026-09-24).
    VersionStepsNotSupported,
    /// ADMISSION POLICY, not validity: a feature this binary does not
    /// implement. Refused by mempool, producer and the signing CLI; a block
    /// carrying it is still valid (the node halts cleanly at its height).
    UnknownFeature { feature: String },
    /// ADMISSION POLICY, not validity: a feature this binary implements but
    /// marks not schedulable ([`ConsensusFeature::schedulable`]).
    FeatureNotSchedulable { feature: String },
    /// SIGNING policy: a name held by [`HELD_FEATURE_PREFIXES`] is refused by
    /// every library signer that takes no faucet proof of possession
    /// ([`sign_feature_schedule_extension`] and `…_with_key`); it is signed
    /// only through [`sign_faucet_sink_binding_extension`] (#3361 F3).
    FeatureHeld { feature: String, until: &'static str },
    /// ADMISSION / SIGNING POLICY, not validity (#3361 F3): a
    /// `faucet_sink_binding*` name with no [`LEGACY_FAUCET_BINDINGS`] row
    /// for the node's (or, when signing, the extension's) chain instance.
    ///
    /// [`LEGACY_FAUCET_BINDINGS`]: crate::crypto::chain_binding::LEGACY_FAUCET_BINDINGS
    FaucetBindingNotSchedulable { feature: String, reason: String },
    /// SIGNING policy (#3361 F3): the faucet binding cannot be signed as
    /// presented — more than one binding entry, a chain id that is not the
    /// row's, a missing or invalid faucet-key proof of possession, or a
    /// proof handed in for an extension that binds nothing.
    FaucetBindingUnsignable { feature: String, reason: String },
}

impl std::fmt::Display for ScheduleExtensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingIdentity => {
                write!(f, "schedule extension must name chain_id and network_id")
            },
            Self::WrongChain { expected, found } => write!(
                f,
                "schedule extension is for chain {found:?}, this node is on {expected:?}"
            ),
            Self::MalformedGenesisHash { found } => write!(
                f,
                "schedule extension genesis_hash {found:?} is not {GENESIS_HASH_HEX_LEN} lowercase hex characters"
            ),
            Self::WrongGenesis { expected, found } => write!(
                f,
                "schedule extension is bound to genesis {found}, this chain's block 1 is {expected}"
            ),
            Self::ReleaseCommitTooLong { len, max } => write!(
                f,
                "schedule extension release_commit is {len} bytes, the limit is {max}"
            ),
            Self::NoEntries => write!(
                f,
                "schedule extension carries neither a feature entry nor a version step"
            ),
            Self::TooManyVersionSteps { count, max } => write!(
                f,
                "schedule extension carries {count} version steps, the limit is {max}"
            ),
            Self::VersionStepNotAlone { entries } => write!(
                f,
                "a version step must be the only content of its extension, found {entries} feature entr{}",
                if *entries == 1 { "y" } else { "ies" }
            ),
            Self::TooManyEntries { count, max } => write!(
                f,
                "schedule extension carries {count} entries, the limit is {max}"
            ),
            Self::MalformedFeatureName { feature } => write!(
                f,
                "feature name {feature:?} is not [a-z][a-z0-9_]* of at most {MAX_FEATURE_NAME_LEN} bytes"
            ),
            Self::VersionTokenAsFeature { feature } => write!(
                f,
                "{feature:?} is a signing-version token; versions are not scheduled by extension"
            ),
            Self::SigningVersionAsFeature => write!(
                f,
                "signing_v2 is scheduled through the version table, never as a feature name"
            ),
            Self::DuplicateFeature { feature } => {
                write!(f, "feature {feature} appears twice in one extension")
            },
            Self::EntriesNotSorted { index } => write!(
                f,
                "entry {index} is out of order; entries must be strictly ascending by (activation_height, feature)"
            ),
            Self::ActivationTooSoon {
                feature,
                activation_height,
                earliest,
            } => write!(
                f,
                "feature {feature} activates at {activation_height}, earlier than the minimum run-up allows ({earliest})"
            ),
            Self::ActivationTooFar {
                feature,
                activation_height,
                latest,
            } => write!(
                f,
                "feature {feature} activates at {activation_height}, beyond the maximum horizon ({latest})"
            ),
            Self::NoSignatures => write!(f, "schedule extension carries no signatures"),
            Self::SignerNotAuthorized { signer_pubkey } => write!(
                f,
                "schedule extension signer {signer_pubkey} is not in the governance allowlist"
            ),
            Self::BadSignature {
                signer_pubkey,
                reason,
            } => write!(
                f,
                "schedule extension signature by {signer_pubkey} does not verify: {reason}"
            ),
            Self::SignerFailed { reason } => {
                write!(f, "schedule extension signer failed: {reason}")
            },
            Self::WrongSequence { expected, found } => write!(
                f,
                "schedule extension sequence {found} is not the next slot ({expected})"
            ),
            Self::AlreadyScheduled { feature } => write!(
                f,
                "feature {feature} is already scheduled; a rule change needs a new feature name"
            ),
            Self::NotAppendOrder {
                activation_height,
                latest_scheduled,
            } => write!(
                f,
                "activation {activation_height} is below the latest scheduled activation \
                 {latest_scheduled}; extensions append in activation order"
            ),
            Self::VersionStepsNotSupported => write!(
                f,
                "signing-version steps are not supported by this binary (#3277 S11)"
            ),
            Self::UnknownFeature { feature } => write!(
                f,
                "feature {feature} is not implemented by this binary (known: {})",
                ConsensusFeature::ALL
                    .iter()
                    .map(|f| f.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::FeatureNotSchedulable { feature } => write!(
                f,
                "feature {feature} is not schedulable by this binary"
            ),
            Self::FeatureHeld { feature, until } => write!(
                f,
                "feature {feature} is not signed without {until}"
            ),
            Self::FaucetBindingNotSchedulable { reason, .. } => f.write_str(reason),
            Self::FaucetBindingUnsignable { feature, reason } => write!(
                f,
                "refusing to sign faucet binding {feature}: {reason} (#3361 F3)"
            ),
        }
    }
}

impl std::error::Error for ScheduleExtensionError {}

/// Whether `hash` is the canonical form of a 32-byte hash: exactly 64
/// lowercase hex characters, no prefix. Canonical so one chain instance has
/// exactly one signable spelling.
pub fn is_canonical_genesis_hash(hash: &str) -> bool {
    hash.len() == GENESIS_HASH_HEX_LEN
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// V5 for one name: bounded, `[a-z][a-z0-9_]*`, not a version token, not
/// `signing_v2`. The same grammar a genesis schedule accepts.
pub fn check_feature_name(name: &str) -> Result<(), ScheduleExtensionError> {
    if name.is_empty() || name.len() > MAX_FEATURE_NAME_LEN || !is_feature_name(name) {
        return Err(ScheduleExtensionError::MalformedFeatureName {
            feature: name.to_string(),
        });
    }
    if is_version_token(name) {
        return Err(ScheduleExtensionError::VersionTokenAsFeature {
            feature: name.to_string(),
        });
    }
    if name == ConsensusFeature::SigningV2.name() {
        return Err(ScheduleExtensionError::SigningVersionAsFeature);
    }
    Ok(())
}

/// Sign an extension with a [`BlockSigner`] (env key, key file, or HSM
/// under `--features hsm`). The signature binds the chain and network named
/// in the extension, so signing reads no ambient configuration.
pub fn sign_feature_schedule_extension(
    extension: &FeatureScheduleExtension,
    signer: &dyn BlockSigner,
) -> Result<GovernanceSignature, ScheduleExtensionError> {
    // A faucet binding (#3361 F3) is never signed without the faucet key's
    // proof of possession: that path is `sign_faucet_sink_binding_extension`.
    check_held_features(extension)?;
    sign_checked(extension, signer)
}

/// Sign an extension through a [`BlockSigner`] with no policy check: every
/// public signer runs its own before calling this.
fn sign_checked(
    extension: &FeatureScheduleExtension,
    signer: &dyn BlockSigner,
) -> Result<GovernanceSignature, ScheduleExtensionError> {
    let signature =
        signer
            .sign(&extension.preimage())
            .map_err(|e| ScheduleExtensionError::SignerFailed {
                reason: e.to_string(),
            })?;
    Ok(GovernanceSignature {
        signer_pubkey: signer.public_key().to_hex().to_ascii_lowercase(),
        signature: signature.to_hex(),
    })
}

/// [`sign_feature_schedule_extension`] with a raw key — the governance key
/// file the oracle ceremony and upgrade notices already use.
///
/// Refuses a held name ([`HELD_FEATURE_PREFIXES`], #3361 F3) exactly as the
/// `BlockSigner` path does (core4#3364 review P3-1): NO library signer
/// produces a governance signature for a faucet binding without the faucet
/// key's proof of possession.
pub fn sign_feature_schedule_extension_with_key(
    extension: &FeatureScheduleExtension,
    key: &PrivateKey,
) -> Result<GovernanceSignature, ScheduleExtensionError> {
    check_held_features(extension)?;
    Ok(sign_checked_with_key(extension, key))
}

fn sign_checked_with_key(extension: &FeatureScheduleExtension, key: &PrivateKey) -> GovernanceSignature {
    GovernanceSignature {
        signer_pubkey: PublicKey::from_private_key(key)
            .to_hex()
            .to_ascii_lowercase(),
        signature: key.sign(&extension.preimage()).to_hex(),
    }
}

/// The faucet sink a signable faucet-binding extension resolves to (#3361
/// F3): what the signer is shown before signing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedFaucetBinding {
    pub feature: String,
    pub activation_height: u64,
    pub chain_id: String,
    pub genesis_hash: String,
    /// Canonical `0x…` address, or `None` for an explicit unbind row.
    pub address: Option<String>,
}

/// The #3361 F3 SIGNING gate for the faucet-sink binding. `Ok(None)` for an
/// extension that binds no faucet (and was handed no proof). Otherwise the
/// extension must carry exactly one `faucet_sink_binding*` entry and:
/// - (i) a [`LEGACY_FAUCET_BINDINGS`] row carries exactly that name for the
///   extension's own chain instance (`genesis_hash`) and chain id;
/// - (ii) for a row that binds an address, `proof` is a valid faucet-key
///   proof of possession over (`chain_id`, `network_id`, the row's block-1
///   hash, the row's address) — see [`super::faucet_sink_pop`]. An unbind
///   row points emission at nobody, so it takes no proof.
///
/// Returns the resolved sink so the signer can confirm it.
///
/// [`LEGACY_FAUCET_BINDINGS`]: crate::crypto::chain_binding::LEGACY_FAUCET_BINDINGS
pub fn check_faucet_sink_binding_signable(
    extension: &FeatureScheduleExtension,
    proof: Option<&FaucetSinkPossessionProof>,
) -> Result<Option<ResolvedFaucetBinding>, ScheduleExtensionError> {
    check_faucet_sink_binding_signable_against(
        crate::crypto::chain_binding::LEGACY_FAUCET_BINDINGS,
        extension,
        proof,
    )
}

/// [`check_faucet_sink_binding_signable`] over an explicit row table: the
/// committed [`LEGACY_FAUCET_BINDINGS`] in production, a synthetic row whose
/// sink key a test holds elsewhere (nobody but the faucet service holds the
/// committed row's key).
///
/// [`LEGACY_FAUCET_BINDINGS`]: crate::crypto::chain_binding::LEGACY_FAUCET_BINDINGS
pub fn check_faucet_sink_binding_signable_against(
    rows: &[crate::crypto::chain_binding::LegacyFaucetBinding],
    extension: &FeatureScheduleExtension,
    proof: Option<&FaucetSinkPossessionProof>,
) -> Result<Option<ResolvedFaucetBinding>, ScheduleExtensionError> {
    let bindings: Vec<&ScheduleEntry> = extension
        .entries
        .iter()
        .filter(|e| e.feature.starts_with(FAUCET_SINK_BINDING_PREFIX))
        .collect();
    let entry = match bindings.as_slice() {
        [] => {
            return match proof {
                None => Ok(None),
                Some(_) => Err(ScheduleExtensionError::FaucetBindingUnsignable {
                    feature: String::new(),
                    reason: "a faucet proof of possession was given for an extension that \
                             binds no faucet"
                        .into(),
                }),
            };
        },
        [entry] => *entry,
        [first, ..] => {
            return Err(ScheduleExtensionError::FaucetBindingUnsignable {
                feature: first.feature.clone(),
                reason: format!(
                    "the extension carries {} faucet-binding entries; bind one sink per extension",
                    bindings.len()
                ),
            });
        },
    };
    let unsignable = |reason: String| ScheduleExtensionError::FaucetBindingUnsignable {
        feature: entry.feature.clone(),
        reason,
    };
    let row = crate::crypto::chain_binding::faucet_binding_row_in(
        rows,
        &entry.feature,
        Some(&extension.genesis_hash),
    )
    .map_err(|e| {
        ScheduleExtensionError::FaucetBindingNotSchedulable {
            feature: entry.feature.clone(),
            reason: e.to_string(),
        }
    })?;
    if extension.chain_id != row.genesis_chain_id {
        return Err(unsignable(format!(
            "the extension names chain id {:?}, the binding row's chain is {:?}",
            extension.chain_id, row.genesis_chain_id
        )));
    }
    let address = match row.address {
        None => {
            if proof.is_some() {
                return Err(unsignable(
                    "the row is an explicit unbind; there is no key to prove".into(),
                ));
            }
            None
        },
        Some(hex) => {
            let address = Address::from_hex(hex)
                .map_err(|e| unsignable(format!("the row's address {hex:?} does not parse: {e}")))?;
            let proof = proof.ok_or_else(|| {
                unsignable(format!(
                    "no proof of possession from the faucet key for {address}; the faucet key \
                     holder signs one with `wrfcoin governance faucet-pop sign`"
                ))
            })?;
            let expected = FaucetSinkPossession::new(
                extension.chain_id.clone(),
                extension.network_id.clone(),
                row.genesis_hash,
                address,
            );
            verify_faucet_sink_possession(proof, &expected).map_err(|e| unsignable(e.to_string()))?;
            Some(address.to_string())
        },
    };
    Ok(Some(ResolvedFaucetBinding {
        feature: entry.feature.clone(),
        activation_height: entry.activation_height,
        chain_id: extension.chain_id.clone(),
        genesis_hash: row.genesis_hash.to_string(),
        address,
    }))
}

/// Sign an extension that may bind the faucet sink (#3361 F3) through a
/// [`BlockSigner`]: [`check_faucet_sink_binding_signable`] first, so a
/// governance signature over a faucet binding exists only for the row's own
/// chain instance and only with the faucet key's proof of possession.
pub fn sign_faucet_sink_binding_extension(
    extension: &FeatureScheduleExtension,
    proof: Option<&FaucetSinkPossessionProof>,
    signer: &dyn BlockSigner,
) -> Result<GovernanceSignature, ScheduleExtensionError> {
    check_faucet_sink_binding_signable(extension, proof)?;
    sign_checked(extension, signer)
}

/// [`sign_faucet_sink_binding_extension`] with a raw governance key.
pub fn sign_faucet_sink_binding_extension_with_key(
    extension: &FeatureScheduleExtension,
    proof: Option<&FaucetSinkPossessionProof>,
    key: &PrivateKey,
) -> Result<GovernanceSignature, ScheduleExtensionError> {
    sign_faucet_sink_binding_extension_with_key_against(
        crate::crypto::chain_binding::LEGACY_FAUCET_BINDINGS,
        extension,
        proof,
        key,
    )
    .map(|(signature, _)| signature)
}

/// [`sign_faucet_sink_binding_extension_with_key`] over an explicit row
/// table (see [`check_faucet_sink_binding_signable_against`]), also
/// returning the sink the signature approves so the signer can show it.
pub fn sign_faucet_sink_binding_extension_with_key_against(
    rows: &[crate::crypto::chain_binding::LegacyFaucetBinding],
    extension: &FeatureScheduleExtension,
    proof: Option<&FaucetSinkPossessionProof>,
    key: &PrivateKey,
) -> Result<(GovernanceSignature, Option<ResolvedFaucetBinding>), ScheduleExtensionError> {
    let resolved = check_faucet_sink_binding_signable_against(rows, extension, proof)?;
    Ok((sign_checked_with_key(extension, key), resolved))
}

/// V9 alone: at least one signature; every signer on the allowlist, listed
/// once, with a well-formed signature that verifies over the preimage.
pub fn verify_schedule_extension_signatures(
    signed: &SignedFeatureScheduleExtension,
    allowlist: &[String],
) -> Result<(), ScheduleExtensionError> {
    if signed.signatures.is_empty() {
        return Err(ScheduleExtensionError::NoSignatures);
    }
    let allow: Vec<String> = allowlist
        .iter()
        .map(|k| k.trim().to_ascii_lowercase())
        .filter(|k| !k.is_empty())
        .collect();
    // Allowlist membership and duplicates are checked for EVERY signer
    // before any curve operation, so the crypto work a hostile artifact can
    // force is bounded by the allowlist size.
    let mut seen: Vec<String> = Vec::with_capacity(signed.signatures.len());
    for entry in &signed.signatures {
        let signer_pubkey = entry.signer_pubkey.trim().to_ascii_lowercase();
        if !allow.contains(&signer_pubkey) {
            return Err(ScheduleExtensionError::SignerNotAuthorized { signer_pubkey });
        }
        if seen.contains(&signer_pubkey) {
            return Err(ScheduleExtensionError::BadSignature {
                signer_pubkey,
                reason: "signer listed twice".into(),
            });
        }
        seen.push(signer_pubkey);
    }
    let preimage = signed.extension.preimage();
    for (entry, signer_pubkey) in signed.signatures.iter().zip(seen) {
        let public_key = PublicKey::from_hex(&signer_pubkey).map_err(|e| {
            ScheduleExtensionError::BadSignature {
                signer_pubkey: signer_pubkey.clone(),
                reason: format!("signer public key is not valid hex ed25519: {e}"),
            }
        })?;
        let signature = Signature::from_hex(entry.signature.trim()).map_err(|e| {
            ScheduleExtensionError::BadSignature {
                signer_pubkey: signer_pubkey.clone(),
                reason: format!("signature is not valid hex: {e}"),
            }
        })?;
        if !public_key.verify(&preimage, &signature) {
            return Err(ScheduleExtensionError::BadSignature {
                signer_pubkey,
                reason: "signature does not verify over the extension preimage — the extension \
                         was modified after it was signed, or was signed for another chain"
                    .into(),
            });
        }
    }
    Ok(())
}

/// Every state-free check (V2, V4, V5, in-tx V8, V9), in order: chain
/// identity, genesis binding, well-formedness, signatures. `Ok(())` means
/// the artifact is a well-formed extension for THIS chain instance, signed
/// only by governance keys. The height window (V7) needs the carrying
/// height — [`FeatureScheduleExtension::check_activation_window`] — and the
/// sequence / prior-schedule rules (V3, V6, V8) need `State`.
pub fn verify_feature_schedule_extension(
    signed: &SignedFeatureScheduleExtension,
    context: &ScheduleExtensionContext<'_>,
) -> Result<(), ScheduleExtensionError> {
    let extension = &signed.extension;
    let expected = ChainIdentity::new(context.chain_id, context.network_id);
    let found = extension.identity();
    if found != expected {
        return Err(ScheduleExtensionError::WrongChain { expected, found });
    }
    if !is_canonical_genesis_hash(&extension.genesis_hash) {
        return Err(ScheduleExtensionError::MalformedGenesisHash {
            found: extension.genesis_hash.clone(),
        });
    }
    if extension.genesis_hash != context.genesis_hash {
        return Err(ScheduleExtensionError::WrongGenesis {
            expected: context.genesis_hash.to_string(),
            found: extension.genesis_hash.clone(),
        });
    }
    extension.check_well_formed()?;
    verify_schedule_extension_signatures(signed, context.allowlist)
}

// ---------------------------------------------------------------------------
// S2: the extension as applied to consensus state
// ---------------------------------------------------------------------------

/// Feature-name prefixes the PLAIN library signers
/// ([`sign_feature_schedule_extension`], `…_with_key`) never sign, whatever
/// the suffix (`_r2`, …): each needs a safeguard those signers do not take.
/// Signing policy only — never a block-validity rule.
///
/// - `faucet_sink_binding`: signed only with the faucet key's proof of
///   possession (#3361 Q4, owner ruling 2026-09-28), through
///   [`sign_faucet_sink_binding_extension`]. Admission (mempool, producer)
///   is gated separately by the binding row ([`check_admission_policy`]).
pub const HELD_FEATURE_PREFIXES: &[(&str, &str)] = &[(
    FAUCET_SINK_BINDING_PREFIX,
    "the faucet key's proof of possession (#3361 F3: sign_faucet_sink_binding_extension, \
     or `wrfcoin governance schedule-extension sign --faucet-pop`)",
)];

/// The safeguard `name` waits on, if a [`HELD_FEATURE_PREFIXES`] entry holds it.
pub fn held_feature(name: &str) -> Option<&'static str> {
    HELD_FEATURE_PREFIXES
        .iter()
        .find(|(prefix, _)| name.starts_with(prefix))
        .map(|(_, until)| *until)
}

fn check_held_features(extension: &FeatureScheduleExtension) -> Result<(), ScheduleExtensionError> {
    match extension
        .entries
        .iter()
        .find_map(|e| held_feature(&e.feature).map(|until| (e, until)))
    {
        Some((entry, until)) => Err(ScheduleExtensionError::FeatureHeld {
            feature: entry.feature.clone(),
            until,
        }),
        None => Ok(()),
    }
}

/// Non-consensus admission policy (design §1.3, T8): whether THIS binary
/// admits `extension` to its mempool, includes it in a block it produces,
/// or signs it. Refuses a feature it does not implement, a feature it marks
/// not schedulable on this chain instance
/// ([`ConsensusFeature::schedulable`]), and any version step (S11).
///
/// `node_genesis_hash` is this node's durable block-1 hash (lowercase hex)
/// — the installed authority's on a node, the extension's own on the
/// offline signing tool; `None` when there is none. It decides one thing
/// (#3361 F3): a `faucet_sink_binding*` name is admitted only when a
/// [`LEGACY_FAUCET_BINDINGS`] row carries exactly that name for this chain
/// instance AND the extension is bound to that same instance; every other
/// chain, and a node with no block-1 hash, refuses it.
///
/// [`LEGACY_FAUCET_BINDINGS`]: crate::crypto::chain_binding::LEGACY_FAUCET_BINDINGS
///
/// NEVER a block-validity rule: a block that already carries an extension
/// this refuses is valid on every node, exactly as a genesis schedule naming
/// an unknown feature is. Making it validity would make an old binary reject
/// such a block and, as a producer, build a competing one (the #1972 fork
/// class); instead the applying node halts cleanly at the activation.
pub fn check_admission_policy(
    extension: &FeatureScheduleExtension,
    node_genesis_hash: Option<&str>,
) -> Result<(), ScheduleExtensionError> {
    // The faucet-binding family first: known to the binary or not, a name
    // with the prefix is admitted only under a row for THIS chain instance.
    for entry in &extension.entries {
        if !entry.feature.starts_with(FAUCET_SINK_BINDING_PREFIX) {
            continue;
        }
        let not_here = |reason: String| ScheduleExtensionError::FaucetBindingNotSchedulable {
            feature: entry.feature.clone(),
            reason,
        };
        let row = faucet_binding_row_for(&entry.feature, node_genesis_hash)
            .map_err(|e| not_here(e.to_string()))?;
        if extension.genesis_hash != row.genesis_hash {
            return Err(not_here(format!(
                "feature {} is bound to the chain whose block 1 is {}, but the extension names \
                 genesis {}; it is not schedulable here (#3361 F3)",
                entry.feature, row.genesis_hash, extension.genesis_hash
            )));
        }
    }
    for entry in &extension.entries {
        match ConsensusFeature::parse(&entry.feature) {
            None => {
                return Err(ScheduleExtensionError::UnknownFeature {
                    feature: entry.feature.clone(),
                });
            },
            Some(feature) if !feature.schedulable(node_genesis_hash) => {
                return Err(ScheduleExtensionError::FeatureNotSchedulable {
                    feature: entry.feature.clone(),
                });
            },
            Some(_) => {},
        }
    }
    if !extension.version_steps.is_empty() {
        return Err(ScheduleExtensionError::VersionStepsNotSupported);
    }
    Ok(())
}

/// The name of the feature that gates the extension transaction itself (V1).
/// Scheduled through genesis on a new chain, or through the per-chain
/// bootstrap table on the legacy live chain (S5).
pub const SCHEDULE_EXTENSION_FEATURE: &str = "schedule_extension";

/// What `State` keeps of an applied extension (design §1.4). Part of the
/// bincode checkpoint layout and of `state_root` material once non-empty,
/// so the field list is FROZEN from checkpoint format 9 on: append-only
/// through a format bump. `version_steps` is here from the start (always
/// empty until S11) so S11 needs no layout change.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AppliedExtension {
    pub sequence: u64,
    /// Height of the block that carried the extension.
    pub carried_at_height: u64,
    pub entries: Vec<ScheduleEntry>,
    pub version_steps: Vec<VersionStep>,
    pub min_protocol_version: u32,
    pub release_commit: String,
    /// Lowercase hex governance keys whose signatures verified, in the order
    /// the extension listed them.
    pub signers: Vec<String>,
}

/// The node-level trust inputs a state-dependent check needs beyond the
/// parent `State`: this chain's durable block-1 hash and the governance
/// allowlist (D1). Installed once at boot (wired in S5); until then the
/// apply arm fails closed.
///
/// **DETERMINISM INVARIANT (ruling 2026-09-24).** Every field is consensus
/// input, so every field must come from a source all honest nodes share
/// byte-for-byte:
/// - `chain_id`, `network_id`, `genesis_hash`: the durable block-1 block;
/// - `allowlist`: the governance key(s) COMPILED INTO the binary for this
///   chain (the D1 pin, a `const` in the bootstrap table, S5).
///
/// NEVER an environment variable, a config file, or anything operator-set:
/// two nodes with different config would otherwise accept and reject the
/// same block and fork. This module deliberately has no constructor that
/// reads the environment; `authority_sources_are_never_the_environment`
/// pins that by scanning this file.
///
/// **F1 (#3277 S5).** The fields are private and the only production
/// constructor is [`install_schedule_extension_authority_from_block1`],
/// which reads nothing but the block-1 hash it is handed and the compiled-in
/// [`SCHEDULE_EXTENSION_GOVERNANCE_PINS`] / `LEGACY_CHAIN_BOOTSTRAP`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleExtensionAuthority {
    chain_id: String,
    network_id: String,
    genesis_hash: String,
    allowlist: Vec<String>,
    node_timing: Option<(u64, u64)>,
}

impl ScheduleExtensionAuthority {
    pub fn node_timing(&self) -> Option<(u64, u64)> { self.node_timing }

    pub fn context(&self) -> ScheduleExtensionContext<'_> {
        ScheduleExtensionContext {
            chain_id: &self.chain_id,
            network_id: &self.network_id,
            genesis_hash: &self.genesis_hash,
            allowlist: &self.allowlist,
        }
    }

    pub fn chain_id(&self) -> &str {
        &self.chain_id
    }

    pub fn network_id(&self) -> &str {
        &self.network_id
    }

    pub fn genesis_hash(&self) -> &str {
        &self.genesis_hash
    }

    pub fn allowlist(&self) -> &[String] {
        &self.allowlist
    }
}

/// The governance keys allowed to sign schedule extensions on one chain
/// instance, keyed by its block-1 hash (owner ruling D1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GovernancePin {
    /// Lowercase hex of the chain's block-1 hash.
    pub genesis_hash: &'static str,
    /// Lowercase hex ed25519 public keys.
    pub keys: &'static [&'static str],
}

/// The compiled-in D1 pins (#3277 S5).
///
/// **IMMUTABLE (F3).** Apply and WAL replay re-verify every historical
/// extension's signatures (V9) against this set, so removing or replacing a
/// key would make a full replay from genesis reject blocks it once accepted.
/// A key rotation is therefore an APPEND to a chain's `keys`, never an edit;
/// `governance_pins_are_immutable` pins the table byte for byte.
///
/// 777/testnet: the oracle governance key from the 2026-08-04 ceremony,
/// `docs/plans/2026-08-04-oracle-ceremony-record.md` (workspace docs) lines
/// 214 and 516 — the value the live nodes run as
/// `WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS`, recorded here as a constant because
/// consensus must never read it from the environment.
pub const SCHEDULE_EXTENSION_GOVERNANCE_PINS: &[GovernancePin] = &[GovernancePin {
    genesis_hash: "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8", // pragma: allowlist secret -- public genesis hash
    keys: &["e7bec166ad446314d26fb7526bfa8cec1b363b438440262fb001f7b04b20bf00"], // pragma: allowlist secret -- public governance KEY (ed25519 public key)
}];

static AUTHORITY: std::sync::OnceLock<ScheduleExtensionAuthority> = std::sync::OnceLock::new();

/// Build and install the authority for the chain whose durable block 1
/// hashes to `block1_hash` (#3277 S5, F1): its identity from the
/// `ScheduleExtension` row of `LEGACY_CHAIN_BOOTSTRAP`, its allowlist from
/// [`SCHEDULE_EXTENSION_GOVERNANCE_PINS`]. `None` — nothing installed, every
/// extension fails closed — for a chain without both. First install wins.
///
/// The ONE production constructor; `native-chain`'s
/// `upgrade::install_schedule_extension_authority_for_block1` is its one
/// production caller (pinned by a source-scan test there).
pub fn install_schedule_extension_authority_from_block1(
    block1_hash: &str,
    block1: Option<&crate::blockchain::block::Block>,
) -> Option<&'static ScheduleExtensionAuthority> {
    let candidate = authority_from_genesis(block1_hash, block1)?;
    let installed = AUTHORITY.get_or_init(|| candidate.clone());
    (installed == &candidate).then_some(installed)
}

/// Pure constructor: legacy pins retain precedence and bytes; new testnets
/// derive identity and governance keys only from their recorded block 1.
pub fn authority_from_genesis(
    block1_hash: &str,
    block1: Option<&crate::blockchain::block::Block>,
) -> Option<ScheduleExtensionAuthority> {
    use crate::crypto::chain_binding as cb;
    if let Some(row) = cb::legacy_chain_bootstrap_for(block1_hash)
        .into_iter().find(|row| row.feature == ConsensusFeature::ScheduleExtension) {
        let pin = SCHEDULE_EXTENSION_GOVERNANCE_PINS.iter()
            .find(|pin| pin.genesis_hash == row.genesis_hash)?;
        return Some(ScheduleExtensionAuthority {
            chain_id: row.chain_id.into(), network_id: row.network_id.into(),
            genesis_hash: row.genesis_hash.into(),
            allowlist: pin.keys.iter().map(|k| k.to_string()).collect(),
            node_timing: None,
        });
    }
    let block = block1?;
    if block.height != 1 || block.calculate_hash().to_hex() != block1_hash
        || block.metadata.get(cb::GENESIS_NETWORK_ID_KEY)?.as_str() != "testnet"
        || block.metadata.get(cb::GENESIS_CHAIN_ID_KEY)?.as_str() == "777" {
        return None;
    }
    cb::validate_testnet_genesis(block).ok()?;
    Some(ScheduleExtensionAuthority {
        chain_id: block.metadata.get(cb::GENESIS_CHAIN_ID_KEY)?.clone(),
        network_id: "testnet".into(), genesis_hash: block1_hash.into(),
        allowlist: cb::genesis_schedule_governance_keys(block).ok()?,
        node_timing: if block.metadata.contains_key(cb::GENESIS_NODE_EPOCH_KEY) { Some((512, 512)) } else { None },
    })
}

/// Tests only: install an arbitrary authority. Compiled only into test
/// builds (`test-support` is a dev-dependency feature), so no shipped
/// binary can construct one from anything but block 1 and the pins.
#[cfg(any(test, feature = "test-support"))]
pub fn install_schedule_extension_authority_for_tests(
    chain_id: &str,
    network_id: &str,
    genesis_hash: &str,
    allowlist: Vec<String>,
) -> &'static ScheduleExtensionAuthority {
    AUTHORITY.get_or_init(|| ScheduleExtensionAuthority {
        chain_id: chain_id.to_string(),
        network_id: network_id.to_string(),
        genesis_hash: genesis_hash.to_string(),
        allowlist,
        node_timing: None,
    })
}

/// The installed authority, if boot has installed one.
pub fn schedule_extension_authority() -> Option<&'static ScheduleExtensionAuthority> {
    AUTHORITY.get()
}

/// Whether the extension transaction is enabled at `height` under
/// `base_schedule` — V1. The gate is looked up BY NAME so this works before
/// the `ScheduleExtension` enum variant exists (S5).
pub fn extension_gate_active(base_schedule: &[(String, u64)], height: u64) -> bool {
    base_schedule
        .iter()
        .any(|(name, activation)| name == SCHEDULE_EXTENSION_FEATURE && height >= *activation)
}

/// V2–V9 against the parent state (design §1.3, §1.4): the full validity of
/// an extension carried in a block at `carrying_height`, except V1 (the
/// caller checks [`extension_gate_active`]).
///
/// Pure: the same function runs for producer inclusion, block apply and WAL
/// replay. `base_schedule` is the effective schedule WITHOUT applied
/// extensions (genesis ∪ bootstrap), `applied` is the parent state's list.
/// On success returns the record to append.
pub fn validate_extension_against_state(
    signed: &SignedFeatureScheduleExtension,
    carrying_height: u64,
    context: &ScheduleExtensionContext<'_>,
    base_schedule: &[(String, u64)],
    applied: &[AppliedExtension],
) -> Result<AppliedExtension, ScheduleExtensionError> {
    // V2, V4, V5, in-tx V8, V9.
    verify_feature_schedule_extension(signed, context)?;
    let extension = &signed.extension;
    // S11 owns version-step validity; until then every step is refused.
    if !extension.version_steps.is_empty() {
        return Err(ScheduleExtensionError::VersionStepsNotSupported);
    }
    // V3.
    let expected = applied.len() as u64;
    if extension.sequence != expected {
        return Err(ScheduleExtensionError::WrongSequence {
            expected,
            found: extension.sequence,
        });
    }
    // V7.
    extension.check_activation_window(carrying_height)?;
    // V6 and the cross-extension half of V8 over genesis ∪ bootstrap ∪ applied.
    let scheduled = base_schedule.iter().map(|(n, h)| (n.as_str(), *h)).chain(
        applied
            .iter()
            .flat_map(|a| a.entries.iter().map(|e| (e.feature.as_str(), e.activation_height))),
    );
    let mut latest_scheduled: Option<u64> = None;
    for (name, height) in scheduled {
        if extension.entries.iter().any(|e| e.feature == name) {
            return Err(ScheduleExtensionError::AlreadyScheduled {
                feature: name.to_string(),
            });
        }
        latest_scheduled = Some(latest_scheduled.map_or(height, |l| l.max(height)));
    }
    if let (Some(first), Some(latest)) = (extension.entries.first(), latest_scheduled) {
        if first.activation_height < latest {
            return Err(ScheduleExtensionError::NotAppendOrder {
                activation_height: first.activation_height,
                latest_scheduled: latest,
            });
        }
    }
    Ok(AppliedExtension {
        sequence: extension.sequence,
        carried_at_height: carrying_height,
        entries: extension.entries.clone(),
        version_steps: extension.version_steps.clone(),
        min_protocol_version: extension.min_protocol_version,
        release_commit: extension.release_commit.clone(),
        signers: signed
            .signatures
            .iter()
            .map(|s| s.signer_pubkey.trim().to_ascii_lowercase())
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::chain_binding::domain_preimage_bound;
    use crate::crypto::domain_signing::push_length_prefixed;
    use crate::governance::upgrade_notice::{UpgradeNotice, sign_upgrade_notice_with_key};
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    const CHAIN: &str = "777";
    const NETWORK: &str = "testnet";
    // A structurally valid block-1 hash; its value is irrelevant here.
    const GENESIS: &str = "abababababababababababababababababababababababababababababababab";
    const OTHER_GENESIS: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
    const TIP: u64 = 21_200;

    fn key(seed: u64) -> PrivateKey {
        PrivateKey::generate(&mut StdRng::seed_from_u64(seed))
    }

    fn pubkey_hex(key: &PrivateKey) -> String {
        PublicKey::from_private_key(key)
            .to_hex()
            .to_ascii_lowercase()
    }

    /// core4#3364 review P3-1: the RAW-KEY signer applies the #3361 F3 hold
    /// too, so no library signer path produces a governance signature for a
    /// held name before F3 — not only the `BlockSigner` path. (RED before
    /// the fix: it returned a signature that verified.)
    #[test]
    fn the_raw_key_signer_refuses_a_held_name_p3_1() {
        let governance = key(31);
        for held in ["faucet_sink_binding", "faucet_sink_binding_r2"] {
            let held_extension = FeatureScheduleExtension {
                entries: vec![ScheduleEntry::new(held, TIP + MIN_RUNUP_BLOCKS)],
                ..extension()
            };
            assert!(
                matches!(
                    sign_feature_schedule_extension_with_key(&held_extension, &governance),
                    Err(ScheduleExtensionError::FeatureHeld { .. })
                ),
                "{held} must not be signed by the raw-key signer"
            );
        }
        // A non-held extension still signs, and verifies.
        let fine = FeatureScheduleExtension {
            entries: vec![ScheduleEntry::new("verify_unit_pricing", TIP + MIN_RUNUP_BLOCKS)],
            ..extension()
        };
        let signature = sign_feature_schedule_extension_with_key(&fine, &governance).expect("signs");
        let signed = SignedFeatureScheduleExtension {
            extension: fine,
            signatures: vec![signature],
        };
        assert!(verify_schedule_extension_signatures(&signed, &[pubkey_hex(&governance)]).is_ok());
    }

    /// #3277 S5, F3 + D1: the governance pin is IMMUTABLE and exact. The
    /// value is the ceremony record's
    /// (`docs/plans/2026-08-04-oracle-ceremony-record.md` lines 214 and 516),
    /// bound to the live chain's block-1 hash — the same hash the bootstrap
    /// row carries — so neither a typo nor a silent rotation can slip in.
    /// Replay re-verifies historical extensions against this set forever.
    #[test]
    fn governance_pins_are_immutable_3277() {
        const LIVE_777_BLOCK1: &str =
            "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8"; // pragma: allowlist secret -- public genesis hash
        const D1_KEY: &str =
            "e7bec166ad446314d26fb7526bfa8cec1b363b438440262fb001f7b04b20bf00"; // pragma: allowlist secret -- public governance KEY
        assert_eq!( // secret-leak-ok: test; public block-1 hash and public ed25519 governance key (D1), no secret material
            SCHEDULE_EXTENSION_GOVERNANCE_PINS,
            &[GovernancePin {
                genesis_hash: LIVE_777_BLOCK1,
                keys: &[D1_KEY],
            }]
        );
        // Every pin is canonical and parses as an ed25519 key.
        for pin in SCHEDULE_EXTENSION_GOVERNANCE_PINS {
            assert!(is_canonical_genesis_hash(pin.genesis_hash)); // secret-leak-ok: test; the pin's genesis hash is the public block-1 hash
            for key in pin.keys {
                assert_eq!(*key, key.to_ascii_lowercase());
                PublicKey::from_hex(key).expect("a pinned key is a valid ed25519 public key");
            }
        }
        // The pin and the bootstrap row name the same chain instance.
        let row = crate::crypto::chain_binding::legacy_chain_bootstrap_for(LIVE_777_BLOCK1);
        assert_eq!(row.len(), 1);
        assert_eq!(row[0].genesis_hash, SCHEDULE_EXTENSION_GOVERNANCE_PINS[0].genesis_hash);
    }

    /// #3277 S4 T8 (policy half): known and schedulable names only, and no
    /// version step. Pure over the artifact — no signature or state is read,
    /// so the policy can never depend on anything a peer does not share.
    #[test]
    fn admission_policy_admits_only_known_schedulable_features_3277() {
        let named = |feature: &str| FeatureScheduleExtension {
            entries: vec![ScheduleEntry::new(feature, TIP + MIN_RUNUP_BLOCKS)],
            ..extension()
        };
        // The ruled E0 (#3131 + #3270) is admissible once this binary carries
        // #3270's feature (S9, #3280): policy follows the binary. Before S9
        // this was `UnknownFeature { "weather_skew_block_time_allowance" }`.
        assert_eq!(check_admission_policy(&extension(), Some(GENESIS)), Ok(()));
        assert_eq!(check_admission_policy(&named("reward_gross_outlier_guard"), None), Ok(()));
        assert_eq!(
            check_admission_policy(&named("a_b4_feature"), Some(GENESIS)),
            Err(ScheduleExtensionError::UnknownFeature {
                feature: "a_b4_feature".into()
            })
        );
        for held in ["paymaster", "social_recovery", "session_keys"] {
            assert_eq!(
                check_admission_policy(&named(held), Some(GENESIS)),
                Err(ScheduleExtensionError::FeatureNotSchedulable {
                    feature: held.into()
                })
            );
        }
        assert_eq!(
            check_admission_policy(&version_step_extension(), Some(GENESIS)),
            Err(ScheduleExtensionError::VersionStepsNotSupported)
        );
        // #3361 F3: a faucet-binding name is admitted only under a row for
        // THIS chain instance. `GENESIS` is not the row's: refused, known to
        // the binary or not; a node with no block-1 hash fails closed.
        for (held, needle) in [
            ("faucet_sink_binding", "not schedulable here"),
            ("faucet_sink_binding_r2", "has no faucet-binding row"),
        ] {
            for node in [Some(GENESIS), None] {
                let err = check_admission_policy(&named(held), node).unwrap_err();
                assert!(
                    matches!(&err, ScheduleExtensionError::FaucetBindingNotSchedulable { feature, .. } if feature == held),
                    "{held}: {err:?}"
                );
                if node.is_some() || held.ends_with("_r2") {
                    assert!(err.to_string().contains(needle), "{err}");
                }
            }
        }
        // On the row's own chain instance, with the extension bound to it,
        // the bare name is admitted; the suffix still is not.
        let row_chain = crate::crypto::chain_binding::LEGACY_FAUCET_BINDINGS[0].genesis_hash;
        let bound = |feature: &str| FeatureScheduleExtension {
            genesis_hash: row_chain.into(),
            ..named(feature)
        };
        assert_eq!(check_admission_policy(&bound("faucet_sink_binding"), Some(row_chain)), Ok(()));
        assert!(check_admission_policy(&bound("faucet_sink_binding_r2"), Some(row_chain)).is_err());
        // An extension bound to another instance, judged by the row's node,
        // is refused too (V2 would reject it anyway; policy says why).
        let err = check_admission_policy(&named("faucet_sink_binding"), Some(row_chain)).unwrap_err();
        assert!(err.to_string().contains("but the extension names genesis"), "{err}");
        assert_eq!(held_feature("a_faucet_sink_binding"), None, "a prefix, not a substring");
        // The policy is about THIS binary; validity is unaffected, which the
        // S2 apply tests and the S4 block tests pin.
        assert!(check_admission_policy(&named("verify_unit_pricing"), None).is_ok());
        let unknown = ScheduleExtensionError::UnknownFeature {
            feature: "x".into(),
        }
        .to_string();
        assert!(unknown.contains("not implemented by this binary"), "{unknown}");
    }

    fn extension() -> FeatureScheduleExtension {
        FeatureScheduleExtension {
            chain_id: CHAIN.into(),
            network_id: NETWORK.into(),
            genesis_hash: GENESIS.into(),
            sequence: 0,
            entries: vec![
                ScheduleEntry::new("reward_gross_outlier_guard", TIP + MIN_RUNUP_BLOCKS),
                ScheduleEntry::new("weather_skew_block_time_allowance", TIP + MIN_RUNUP_BLOCKS),
            ],
            version_steps: Vec::new(),
            min_protocol_version: 3,
            release_commit: "ab1b1a4930000000000000000000000000000000".into(),
        }
    }

    /// The only content of an E1-shaped extension (design R8): one step.
    fn version_step_extension() -> FeatureScheduleExtension {
        FeatureScheduleExtension {
            sequence: 1,
            entries: Vec::new(),
            version_steps: vec![VersionStep {
                activation_height: TIP + 5_000,
                version: 2,
            }],
            ..extension()
        }
    }

    fn signed_by(
        ext: FeatureScheduleExtension,
        keys: &[&PrivateKey],
    ) -> SignedFeatureScheduleExtension {
        let signatures = keys
            .iter()
            .map(|k| sign_feature_schedule_extension_with_key(&ext, k)
            .expect("governance signs a non-held extension"))
            .collect();
        SignedFeatureScheduleExtension {
            extension: ext,
            signatures,
        }
    }

    fn context(allowlist: &[String]) -> ScheduleExtensionContext<'_> {
        ScheduleExtensionContext {
            chain_id: CHAIN,
            network_id: NETWORK,
            genesis_hash: GENESIS,
            allowlist,
        }
    }

    #[test]
    fn the_domain_tag_is_the_action_domain_and_differs_from_the_notice_tag() {
        assert_eq!(
            // secret-leak-ok: test; `DomainSigningAction` / `DOMAIN_TAG` are enum and constant names, no key material
            FeatureScheduleExtension::DOMAIN_TAG,
            DomainSigningAction::FeatureScheduleExtension
                .domain()
                .as_bytes()
        );
        assert_ne!(
            FeatureScheduleExtension::DOMAIN_TAG,
            UpgradeNotice::DOMAIN_TAG
        );
    }

    #[test]
    fn min_runup_outlasts_every_reorg() {
        // The determinism argument of design §1.5, pinned at runtime as well
        // as by the const assertion.
        const { assert!(MIN_RUNUP_BLOCKS > MAX_REORG_DEPTH) };
        assert_eq!(MIN_RUNUP_BLOCKS, 2_000, "owner ruling D2");
        assert_eq!(MAX_HORIZON_BLOCKS, 500_000, "owner ruling D9");
        assert_eq!(MAX_ENTRIES_PER_EXTENSION, 8, "owner ruling D9");
    }

    /// Byte-pins the canonical body layout against an independent,
    /// hand-assembled encoding, and the full preimage against the bound
    /// frame. A reordered, dropped or re-typed field fails here, which is
    /// what keeps every signature ever produced verifiable.
    #[test]
    fn the_preimage_layout_is_pinned() {
        let ext = extension();
        let mut body = Vec::new();
        push_length_prefixed(&mut body, b"777");
        push_length_prefixed(&mut body, b"testnet");
        push_length_prefixed(&mut body, GENESIS.as_bytes());
        body.extend_from_slice(&0u64.to_le_bytes());
        body.extend_from_slice(&2u64.to_le_bytes());
        push_length_prefixed(&mut body, b"reward_gross_outlier_guard");
        body.extend_from_slice(&(TIP + MIN_RUNUP_BLOCKS).to_le_bytes());
        push_length_prefixed(&mut body, b"weather_skew_block_time_allowance");
        body.extend_from_slice(&(TIP + MIN_RUNUP_BLOCKS).to_le_bytes());
        body.extend_from_slice(&0u64.to_le_bytes()); // version_steps: none
        body.extend_from_slice(&3u32.to_le_bytes());
        push_length_prefixed(&mut body, ext.release_commit.as_bytes());
        assert_eq!(ext.canonical_body(), body);

        let expected = domain_preimage_bound(
            b"wrfcoin.feature_schedule_extension.v1",
            &ChainIdentity::new(CHAIN, NETWORK),
            SigningProtocolVersion::V2,
            &body,
        );
        assert_eq!(ext.preimage(), expected);
        // Bound at V2 even though the chain's own signing schedule may be v1.
        assert_ne!(
            // secret-leak-ok: test; compares public signing preimages, no key material
            ext.preimage(),
            ext.signing_preimage_for(&ext.identity(), SigningProtocolVersion::V1)
        );
    }

    #[test]
    fn sign_verify_round_trip_and_json_round_trip() {
        let governance = key(1);
        let allow = vec![pubkey_hex(&governance)];
        let signed = signed_by(extension(), &[&governance]);
        assert_eq!(
            verify_feature_schedule_extension(&signed, &context(&allow)),
            Ok(())
        );

        let json = serde_json::to_string_pretty(&signed).unwrap();
        let back: SignedFeatureScheduleExtension = serde_json::from_str(&json).unwrap();
        assert_eq!(back, signed);
        assert_eq!(
            verify_feature_schedule_extension(&back, &context(&allow)),
            Ok(())
        );

        // The allowlist comparison is case-insensitive, as the env list is.
        let upper = vec![pubkey_hex(&governance).to_ascii_uppercase()];
        assert_eq!(
            verify_feature_schedule_extension(&signed, &context(&upper)),
            Ok(())
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_block_signer_signs_the_same_bytes_as_the_raw_key() {
        let governance = key(9);
        let via_key = sign_feature_schedule_extension_with_key(&extension(), &governance)
            .expect("governance signs a non-held extension");
        let signer = crate::crypto::signer::EnvKeySigner::from_private_key(
            governance.clone(),
            "TEST_GOVERNANCE_KEY",
        );
        let via_signer = sign_feature_schedule_extension(&extension(), &signer).unwrap();
        assert_eq!(via_key, via_signer);

        // #3361 F3 hold: the library signer refuses a held name (any suffix),
        // so no governance signature for it can exist before its safeguard.
        for held in ["faucet_sink_binding", "faucet_sink_binding_r3"] {
            let held_extension = FeatureScheduleExtension {
                entries: vec![ScheduleEntry::new(held, TIP + MIN_RUNUP_BLOCKS)],
                ..extension()
            };
            assert!(
                matches!(
                    sign_feature_schedule_extension(&held_extension, &signer),
                    Err(ScheduleExtensionError::FeatureHeld { .. })
                ),
                "{held} must not be signed"
            );
        }
    }

    /// #3361 F3, every library signer path (P3-1 included): the PLAIN
    /// signers — `BlockSigner` and raw key — refuse any faucet-binding name
    /// whatever the chain; the faucet signers sign one only for the row's own
    /// chain instance and chain id, and only with a valid proof of possession
    /// by the key that controls the row's address.
    #[test]
    fn faucet_binding_signers_need_the_row_and_the_faucet_proof_3361() {
        use crate::common::types::Address;
        use crate::crypto::chain_binding::{LEGACY_FAUCET_BINDINGS, LegacyFaucetBinding};
        use crate::governance::faucet_sink_pop::{
            FaucetSinkPossession, sign_faucet_sink_possession,
        };
        let governance = key(41);
        let row_chain = LEGACY_FAUCET_BINDINGS[0].genesis_hash;
        let e0f = |feature: &str| FeatureScheduleExtension {
            genesis_hash: row_chain.into(),
            entries: vec![ScheduleEntry::new(feature, TIP + MIN_RUNUP_BLOCKS)],
            ..extension()
        };

        // The plain raw-key signer (core4#3364 review P3-1) refuses, on the
        // row's own chain too.
        for held in ["faucet_sink_binding", "faucet_sink_binding_r2"] {
            assert!(
                matches!(
                    sign_feature_schedule_extension_with_key(&e0f(held), &governance),
                    Err(ScheduleExtensionError::FeatureHeld { .. })
                ),
                "{held}"
            );
        }

        // The COMMITTED row: its key is the faucet service's, so no test can
        // produce its proof; without one, or with a stranger's, it is refused
        // and the refusal names the resolved address.
        let committed = LEGACY_FAUCET_BINDINGS[0].address.unwrap();
        let err = sign_faucet_sink_binding_extension_with_key(&e0f("faucet_sink_binding"), None, &governance)
            .unwrap_err();
        assert!(err.to_string().contains(&format!("0x{committed}")), "{err}");
        assert!(err.to_string().contains("no proof of possession"), "{err}");
        let stranger = key(42);
        let stranger_proof = sign_faucet_sink_possession(
            &FaucetSinkPossession::new(
                "777",
                NETWORK,
                row_chain,
                Address::from_hex(committed).unwrap(),
            ),
            &stranger,
        );
        let err = sign_faucet_sink_binding_extension_with_key(
            &e0f("faucet_sink_binding"),
            Some(&stranger_proof),
            &governance,
        )
        .unwrap_err();
        assert!(err.to_string().contains("key controls"), "{err}");

        // A synthetic row whose sink key this test holds: the accept path.
        let faucet = key(43);
        let faucet_address = Address::from_public_key(&PublicKey::from_private_key(&faucet));
        let hex_address: &'static str =
            Box::leak(faucet_address.to_string().trim_start_matches("0x").to_string().into_boxed_str());
        let rows = [LegacyFaucetBinding {
            address: Some(hex_address),
            ..LEGACY_FAUCET_BINDINGS[0]
        }];
        let claim = FaucetSinkPossession::new("777", NETWORK, row_chain, faucet_address);
        let proof = sign_faucet_sink_possession(&claim, &faucet);
        let resolved = check_faucet_sink_binding_signable_against(&rows, &e0f("faucet_sink_binding"), Some(&proof))
            .expect("the row's own chain, with the faucet key's proof")
            .expect("a binding");
        assert_eq!(resolved.address, Some(faucet_address.to_string()));
        assert_eq!(resolved.genesis_hash, row_chain);
        assert_eq!(resolved.feature, "faucet_sink_binding");

        let refused = |ext: &FeatureScheduleExtension, proof: Option<&FaucetSinkPossessionProof>, needle: &str| {
            let err = check_faucet_sink_binding_signable_against(&rows, ext, proof).unwrap_err();
            assert!(err.to_string().contains(needle), "{needle}: {err}");
        };
        // (i) genesis-hash match: the same extension for another instance.
        refused(
            &FeatureScheduleExtension { genesis_hash: GENESIS.into(), ..e0f("faucet_sink_binding") },
            Some(&proof),
            "not schedulable here",
        );
        // A suffix without a row.
        refused(&e0f("faucet_sink_binding_r2"), Some(&proof), "has no faucet-binding row");
        // The row's chain id is part of the binding.
        refused(
            &FeatureScheduleExtension { chain_id: "778".into(), ..e0f("faucet_sink_binding") },
            Some(&proof),
            "binding row's chain is",
        );
        // (ii) the proof: missing, wrong key, wrong chain, wrong genesis,
        // wrong address.
        refused(&e0f("faucet_sink_binding"), None, "no proof of possession");
        let by = |claim: &FaucetSinkPossession, signer: &PrivateKey| sign_faucet_sink_possession(claim, signer);
        refused(&e0f("faucet_sink_binding"), Some(&by(&claim, &stranger)), "key controls");
        refused(
            &e0f("faucet_sink_binding"),
            Some(&by(&FaucetSinkPossession { network_id: "mainnet".into(), ..claim.clone() }, &faucet)),
            "is for chain",
        );
        refused(
            &e0f("faucet_sink_binding"),
            Some(&by(&FaucetSinkPossession { genesis_hash: GENESIS.into(), ..claim.clone() }, &faucet)),
            "bound to genesis",
        );
        refused(
            &e0f("faucet_sink_binding"),
            Some(&by(&FaucetSinkPossession { address: Address([0x39; 20]).to_string(), ..claim.clone() }, &faucet)),
            "is for address",
        );
        // A replayed domain: the faucet key's signature over this very
        // extension's governance preimage is not a possession proof.
        let replayed = FaucetSinkPossessionProof {
            signature: faucet.sign(&e0f("faucet_sink_binding").preimage()).to_hex(),
            ..proof.clone()
        };
        refused(&e0f("faucet_sink_binding"), Some(&replayed), "signature is invalid");
        // Exactly one binding per extension; a proof for nothing is refused.
        refused(
            &FeatureScheduleExtension {
                entries: vec![
                    ScheduleEntry::new("faucet_sink_binding", TIP + MIN_RUNUP_BLOCKS),
                    ScheduleEntry::new("faucet_sink_binding_r2", TIP + MIN_RUNUP_BLOCKS),
                ],
                ..e0f("faucet_sink_binding")
            },
            Some(&proof),
            "2 faucet-binding entries",
        );
        refused(&extension(), Some(&proof), "binds no faucet");
        assert_eq!(check_faucet_sink_binding_signable_against(&rows, &extension(), None), Ok(None));

        // An explicit unbind row takes no proof (there is no key) and refuses one.
        let unbind = [LegacyFaucetBinding { address: None, ..rows[0] }];
        let resolved = check_faucet_sink_binding_signable_against(&unbind, &e0f("faucet_sink_binding"), None)
            .unwrap()
            .unwrap();
        assert_eq!(resolved.address, None);
        assert!(check_faucet_sink_binding_signable_against(&unbind, &e0f("faucet_sink_binding"), Some(&proof)).is_err());

        // The BlockSigner paths agree with the raw-key paths.
        #[cfg(debug_assertions)]
        {
            let signer = crate::crypto::signer::EnvKeySigner::from_private_key(
                governance.clone(),
                "TEST_GOVERNANCE_KEY",
            );
            assert!(matches!(
                sign_feature_schedule_extension(&e0f("faucet_sink_binding"), &signer),
                Err(ScheduleExtensionError::FeatureHeld { .. })
            ));
            assert!(sign_faucet_sink_binding_extension(&e0f("faucet_sink_binding"), Some(&stranger_proof), &signer).is_err());
            // A non-faucet extension signs identically through both faucet
            // signers and the plain ones.
            let plain = sign_feature_schedule_extension_with_key(&extension(), &governance).unwrap();
            assert_eq!(sign_faucet_sink_binding_extension_with_key(&extension(), None, &governance), Ok(plain.clone()));
            assert_eq!(sign_faucet_sink_binding_extension(&extension(), None, &signer), Ok(plain));
        }
    }

    /// T4 (S1 half): a tamper of every bound field breaks the signature.
    #[test]
    fn every_field_is_bound_by_the_signature() {
        let governance = key(2);
        let allow = vec![pubkey_hex(&governance)];
        let original = signed_by(extension(), &[&governance]);
        let tampers: Vec<(&str, Box<dyn Fn(&mut FeatureScheduleExtension)>)> = vec![
            (
                "genesis_hash",
                Box::new(|e| e.genesis_hash = OTHER_GENESIS.into()),
            ),
            ("sequence", Box::new(|e| e.sequence = 1)),
            (
                "feature name",
                Box::new(|e| e.entries[0].feature = String::from("reward_gross_outlier_guarx")),
            ),
            (
                "activation height",
                Box::new(|e| e.entries[1].activation_height += 1),
            ),
            (
                "dropped entry",
                Box::new(|e| {
                    e.entries.pop();
                }),
            ),
            (
                "min_protocol_version",
                Box::new(|e| e.min_protocol_version = 4),
            ),
            ("release_commit", Box::new(|e| e.release_commit.push('0'))),
        ];
        for (what, tamper) in tampers {
            let mut signed = original.clone();
            tamper(&mut signed.extension);
            // The verifier is pointed at whatever genesis the artifact now
            // names, so only the signature can catch the change.
            let genesis = signed.extension.genesis_hash.clone();
            let ctx = ScheduleExtensionContext {
                genesis_hash: &genesis,
                ..context(&allow)
            };
            assert!(
                matches!(
                    verify_feature_schedule_extension(&signed, &ctx),
                    Err(ScheduleExtensionError::BadSignature { .. })
                ),
                "tampered {what} must break the signature"
            );
        }

        // Chain and network are bound too: re-point the verifier at the
        // tampered identity and the signature still refuses.
        for (chain, network) in [("778", NETWORK), (CHAIN, "mainnet")] {
            let mut signed = original.clone();
            signed.extension.chain_id = chain.into();
            signed.extension.network_id = network.into();
            let ctx = ScheduleExtensionContext {
                chain_id: chain,
                network_id: network,
                ..context(&allow)
            };
            assert!(matches!(
                verify_feature_schedule_extension(&signed, &ctx),
                Err(ScheduleExtensionError::BadSignature { .. })
            ));
        }
    }

    /// V2: another chain, network or chain instance is refused before any
    /// signature is checked.
    #[test]
    fn wrong_chain_network_or_genesis_is_refused() {
        let governance = key(3);
        let allow = vec![pubkey_hex(&governance)];
        let signed = signed_by(extension(), &[&governance]);

        let other_chain = ScheduleExtensionContext {
            chain_id: "778",
            ..context(&allow)
        };
        assert!(matches!(
            verify_feature_schedule_extension(&signed, &other_chain),
            Err(ScheduleExtensionError::WrongChain { .. })
        ));
        let other_network = ScheduleExtensionContext {
            network_id: "mainnet",
            ..context(&allow)
        };
        assert!(matches!(
            verify_feature_schedule_extension(&signed, &other_network),
            Err(ScheduleExtensionError::WrongChain { .. })
        ));
        let recut = ScheduleExtensionContext {
            genesis_hash: OTHER_GENESIS,
            ..context(&allow)
        };
        assert_eq!(
            verify_feature_schedule_extension(&signed, &recut),
            Err(ScheduleExtensionError::WrongGenesis {
                expected: OTHER_GENESIS.into(),
                found: GENESIS.into(),
            })
        );
        // Non-canonical spellings of the same hash are refused, not folded.
        for bad in [
            GENESIS.to_ascii_uppercase(),
            format!("0x{GENESIS}"),
            GENESIS[..63].to_string(),
        ] {
            let mut ext = extension();
            ext.genesis_hash = bad.clone();
            let signed = signed_by(ext, &[&governance]);
            let ctx = ScheduleExtensionContext {
                genesis_hash: &bad,
                ..context(&allow)
            };
            assert!(matches!(
                verify_feature_schedule_extension(&signed, &ctx),
                Err(ScheduleExtensionError::MalformedGenesisHash { .. })
            ));
        }
    }

    /// V9: no signatures, an unlisted signer, a duplicated signer, and
    /// malformed hex are each refused.
    #[test]
    fn signature_rules_fail_closed() {
        let governance = key(4);
        let outsider = key(5);
        let allow = vec![pubkey_hex(&governance)];

        let unsigned = signed_by(extension(), &[]);
        assert_eq!(
            verify_feature_schedule_extension(&unsigned, &context(&allow)),
            Err(ScheduleExtensionError::NoSignatures)
        );

        let by_outsider = signed_by(extension(), &[&outsider]);
        assert!(matches!(
            verify_feature_schedule_extension(&by_outsider, &context(&allow)),
            Err(ScheduleExtensionError::SignerNotAuthorized { .. })
        ));
        // One good and one unlisted signature is still refused.
        let mixed = signed_by(extension(), &[&governance, &outsider]);
        assert!(matches!(
            verify_feature_schedule_extension(&mixed, &context(&allow)),
            Err(ScheduleExtensionError::SignerNotAuthorized { .. })
        ));

        let twice = signed_by(extension(), &[&governance, &governance]);
        assert!(matches!(
            verify_feature_schedule_extension(&twice, &context(&allow)),
            Err(ScheduleExtensionError::BadSignature { reason, .. }) if reason.contains("twice")
        ));

        let mut garbled = signed_by(extension(), &[&governance]);
        garbled.signatures[0].signature = "zz".into();
        assert!(matches!(
            verify_feature_schedule_extension(&garbled, &context(&allow)),
            Err(ScheduleExtensionError::BadSignature { .. })
        ));

        // An empty allowlist authorizes nobody.
        let signed = signed_by(extension(), &[&governance]);
        assert!(matches!(
            verify_feature_schedule_extension(&signed, &context(&[])),
            Err(ScheduleExtensionError::SignerNotAuthorized { .. })
        ));
    }

    /// Cross-domain replay: a governance signature over an `UpgradeNotice`
    /// (same key, same chain) never verifies as an extension signature.
    #[test]
    fn an_upgrade_notice_signature_is_not_an_extension_signature() {
        let governance = key(6);
        let allow = vec![pubkey_hex(&governance)];
        let notice = UpgradeNotice {
            notice_id: UpgradeNotice::notice_id_for(TIP + MIN_RUNUP_BLOCKS, "ab1b1a493"),
            chain_id: CHAIN.into(),
            network_id: NETWORK.into(),
            activation_height: TIP + MIN_RUNUP_BLOCKS,
            min_protocol_version: 3,
            features: vec!["reward_gross_outlier_guard".into()],
            release_commit: "ab1b1a493".into(),
            image_digests: Default::default(),
            issued_at: 0,
            expires_at: None,
        };
        let harvested = sign_upgrade_notice_with_key(&notice, &governance);
        let forged = SignedFeatureScheduleExtension {
            extension: extension(),
            signatures: vec![harvested],
        };
        assert!(matches!(
            verify_feature_schedule_extension(&forged, &context(&allow)),
            Err(ScheduleExtensionError::BadSignature { .. })
        ));
        // And the reverse: an extension signature over the notice preimage.
        let ext_sig = sign_feature_schedule_extension_with_key(&extension(), &governance)
            .expect("governance signs a non-held extension");
        let sig = Signature::from_hex(&ext_sig.signature).unwrap();
        assert!(!PublicKey::from_private_key(&governance).verify(&notice.preimage(), &sig)); // secret-leak-ok: test; asserts a bool verify result over a fixed test key; no key material reaches the message
    }

    /// V4 / V5 / in-tx V8, each with an accept and a reject case.
    #[test]
    fn well_formedness_matrix() {
        assert_eq!(extension().check_well_formed(), Ok(()));

        let mut e = extension();
        e.entries.clear();
        assert_eq!(
            e.check_well_formed(),
            Err(ScheduleExtensionError::NoEntries)
        );

        let mut e = extension();
        e.entries = (0..MAX_ENTRIES_PER_EXTENSION as u64)
            .map(|i| ScheduleEntry::new(format!("feature_{i}"), TIP + MIN_RUNUP_BLOCKS + i))
            .collect();
        assert_eq!(e.check_well_formed(), Ok(()), "exactly the cap is fine");
        e.entries
            .push(ScheduleEntry::new("feature_z", TIP + MIN_RUNUP_BLOCKS + 99));
        assert_eq!(
            e.check_well_formed(),
            Err(ScheduleExtensionError::TooManyEntries {
                count: MAX_ENTRIES_PER_EXTENSION + 1,
                max: MAX_ENTRIES_PER_EXTENSION,
            })
        );

        for bad in ["", "Upper", "9lives", "has-dash", "has space", "_lead", "é"] {
            assert_eq!(
                check_feature_name(bad),
                Err(ScheduleExtensionError::MalformedFeatureName {
                    feature: bad.to_string()
                }),
                "{bad:?}"
            );
        }
        let long = "a".repeat(MAX_FEATURE_NAME_LEN + 1);
        assert!(matches!(
            check_feature_name(&long),
            Err(ScheduleExtensionError::MalformedFeatureName { .. })
        ));
        assert_eq!(
            check_feature_name(&"a".repeat(MAX_FEATURE_NAME_LEN)),
            Ok(())
        );
        for version in ["v2", "v10"] {
            assert!(matches!(
                // secret-leak-ok: test; `SigningVersion`-named error variants and literal feature names, no key material
                check_feature_name(version),
                Err(ScheduleExtensionError::VersionTokenAsFeature { .. })
            ));
        }
        assert_eq!(
            // secret-leak-ok: test; `SigningVersionAsFeature` is an error variant name, no key material
            check_feature_name("signing_v2"),
            Err(ScheduleExtensionError::SigningVersionAsFeature)
        );
        // Unknown-to-this-binary names are well-formed: "known only" is
        // admission policy, never validity.
        assert_eq!(check_feature_name("some_future_feature"), Ok(()));

        // Duplicates: same height (adjacent) and different heights.
        let mut e = extension();
        e.entries[1].feature = e.entries[0].feature.clone();
        assert!(matches!(
            e.check_well_formed(),
            Err(ScheduleExtensionError::DuplicateFeature { .. })
        ));
        let mut e = extension();
        e.entries[1] = ScheduleEntry::new("reward_gross_outlier_guard", TIP + MIN_RUNUP_BLOCKS + 5);
        assert!(matches!(
            e.check_well_formed(),
            Err(ScheduleExtensionError::DuplicateFeature { .. })
        ));

        // Order: descending height, and equal height with names descending.
        let mut e = extension();
        e.entries[0].activation_height += 10;
        assert_eq!(
            e.check_well_formed(),
            Err(ScheduleExtensionError::EntriesNotSorted { index: 1 })
        );
        let mut e = extension();
        e.entries.reverse();
        assert_eq!(
            e.check_well_formed(),
            Err(ScheduleExtensionError::EntriesNotSorted { index: 1 })
        );

        let mut e = extension();
        e.network_id = " ".into();
        assert_eq!(
            e.check_well_formed(),
            Err(ScheduleExtensionError::MissingIdentity)
        );
        let mut e = extension();
        e.release_commit = "f".repeat(MAX_RELEASE_COMMIT_LEN + 1);
        assert!(matches!(
            e.check_well_formed(),
            Err(ScheduleExtensionError::ReleaseCommitTooLong { .. })
        ));
    }

    /// A correctly signed but malformed extension is still refused: the
    /// signature is not a licence to break the grammar.
    #[test]
    fn a_signed_malformed_extension_is_refused() {
        let governance = key(7);
        let allow = vec![pubkey_hex(&governance)];
        let mut ext = extension();
        ext.entries[0].feature = String::from("signing_v2");
        let signed = signed_by(ext, &[&governance]);
        assert_eq!(
            // secret-leak-ok: test; `SigningVersionAsFeature` is an error variant name, no key material
            verify_feature_schedule_extension(&signed, &context(&allow)),
            Err(ScheduleExtensionError::SigningVersionAsFeature)
        );
    }

    /// V7 boundaries: `h + MIN_RUNUP` and `h + MAX_HORIZON` are inclusive.
    #[test]
    fn activation_window_boundaries() {
        let at = |height: u64| FeatureScheduleExtension {
            entries: vec![ScheduleEntry::new("reward_gross_outlier_guard", height)],
            ..extension()
        };
        assert_eq!(
            at(TIP + MIN_RUNUP_BLOCKS).check_activation_window(TIP),
            Ok(())
        );
        assert_eq!(
            at(TIP + MAX_HORIZON_BLOCKS).check_activation_window(TIP),
            Ok(())
        );
        assert!(matches!(
            at(TIP + MIN_RUNUP_BLOCKS - 1).check_activation_window(TIP),
            Err(ScheduleExtensionError::ActivationTooSoon { earliest, .. }) if earliest == TIP + MIN_RUNUP_BLOCKS
        ));
        assert!(matches!(
            at(TIP).check_activation_window(TIP),
            Err(ScheduleExtensionError::ActivationTooSoon { .. })
        ));
        assert!(matches!(
            at(TIP + MAX_HORIZON_BLOCKS + 1).check_activation_window(TIP),
            Err(ScheduleExtensionError::ActivationTooFar { latest, .. }) if latest == TIP + MAX_HORIZON_BLOCKS
        ));
        // Window bounds saturate near u64::MAX instead of overflowing.
        assert_eq!(at(u64::MAX).check_activation_window(u64::MAX - 10), Ok(()));
        assert!(matches!(
            at(u64::MAX - 1).check_activation_window(u64::MAX - 10),
            Err(ScheduleExtensionError::ActivationTooSoon {
                earliest: u64::MAX,
                ..
            })
        ));
        // A later entry out of window is caught even when the first is fine.
        let mut e = extension();
        e.entries[1].activation_height = TIP + MAX_HORIZON_BLOCKS + 1;
        assert!(matches!(
            e.check_activation_window(TIP),
            Err(ScheduleExtensionError::ActivationTooFar { .. })
        ));
    }

    #[test]
    fn first_unknown_feature_is_admission_information_only() {
        // A name that can never be a feature this binary implements, so the
        // test does not depend on which features have landed.
        let e = FeatureScheduleExtension {
            entries: vec![
                ScheduleEntry::new("session_keys", TIP + MIN_RUNUP_BLOCKS),
                ScheduleEntry::new("not_a_feature_this_binary_has", TIP + MIN_RUNUP_BLOCKS + 1),
            ],
            ..extension()
        };
        assert_eq!(
            e.first_unknown_feature(ConsensusFeature::ALL)
                .map(|x| x.feature.as_str()),
            Some("not_a_feature_this_binary_has")
        );
        let known = FeatureScheduleExtension {
            entries: vec![ScheduleEntry::new("session_keys", TIP + MIN_RUNUP_BLOCKS)],
            ..extension()
        };
        assert_eq!(known.first_unknown_feature(ConsensusFeature::ALL), None);
        // ...and an unknown name does not make the artifact malformed.
        assert_eq!(e.check_well_formed(), Ok(()));
    }

    /// V4 with version steps: at most one, and only on its own.
    #[test]
    fn version_step_v4_rules() {
        assert_eq!(version_step_extension().check_well_formed(), Ok(()));

        let mut two = version_step_extension();
        two.version_steps.push(VersionStep {
            activation_height: TIP + 6_000,
            version: 3,
        });
        assert_eq!(
            two.check_well_formed(),
            Err(ScheduleExtensionError::TooManyVersionSteps {
                count: 2,
                max: MAX_VERSION_STEPS_PER_EXTENSION,
            })
        );

        let mut shared = version_step_extension();
        shared.entries = extension().entries;
        assert_eq!(
            shared.check_well_formed(),
            Err(ScheduleExtensionError::VersionStepNotAlone { entries: 2 })
        );

        let mut empty = version_step_extension();
        empty.version_steps.clear();
        assert_eq!(
            empty.check_well_formed(),
            Err(ScheduleExtensionError::NoEntries)
        );

        // A step is signed and bound like every other field.
        let governance = key(8);
        let allow = vec![pubkey_hex(&governance)];
        let signed = signed_by(version_step_extension(), &[&governance]);
        assert_eq!(
            verify_feature_schedule_extension(&signed, &context(&allow)),
            Ok(())
        );
        for tamper in [
            |s: &mut VersionStep| s.activation_height += 1,
            |s: &mut VersionStep| s.version = 3,
        ] {
            let mut t = signed.clone();
            tamper(&mut t.extension.version_steps[0]);
            assert!(matches!(
                verify_feature_schedule_extension(&t, &context(&allow)),
                Err(ScheduleExtensionError::BadSignature { .. })
            ));
        }
        // Dropping the step (and so emptying the extension) is refused
        // before any signature is checked; adding an entry is caught by V4.
        let mut dropped = signed.clone();
        dropped.extension.version_steps.clear();
        assert!(verify_feature_schedule_extension(&dropped, &context(&allow)).is_err());

        // An extension JSON without the field parses as "no version steps"
        // and keeps the exact preimage it was signed with.
        let mut json = serde_json::to_value(extension()).unwrap();
        json.as_object_mut().unwrap().remove("version_steps");
        let back: FeatureScheduleExtension = serde_json::from_value(json).unwrap();
        assert_eq!(back.preimage(), extension().preimage());
    }

    // ---------------------------------------------------------------------
    // GOLDEN PREIMAGES — never edit these bytes.
    //
    // An extension is signed once and its signature is re-verified (V9) on
    // every block apply and WAL replay for the life of the chain. If these
    // bytes change, every extension already on chain stops verifying and
    // replay forks. A change to the encoding needs a NEW domain tag, not an
    // edit here. Decimal, not hex, so the secret scanner reads no
    // high-entropy literal.
    // ---------------------------------------------------------------------

    /// The E0 shape: two feature entries, `version_steps` empty.
    fn golden_e0() -> FeatureScheduleExtension {
        FeatureScheduleExtension {
            chain_id: "777".into(),
            network_id: "testnet".into(),
            genesis_hash: GENESIS.into(),
            sequence: 0,
            entries: vec![
                ScheduleEntry::new("reward_gross_outlier_guard", 23_200),
                ScheduleEntry::new("weather_skew_block_time_allowance", 23_200),
            ],
            version_steps: Vec::new(),
            min_protocol_version: 3,
            release_commit: "ab1b1a4930000000000000000000000000000000".into(),
        }
    }

    /// The E1 shape: one version step alone.
    fn golden_e1() -> FeatureScheduleExtension {
        FeatureScheduleExtension {
            sequence: 1,
            entries: Vec::new(),
            version_steps: vec![VersionStep {
                activation_height: 30_000,
                version: 2,
            }],
            ..golden_e0()
        }
    }

    const GOLDEN_E0_PREIMAGE: &[u8] = &[
        37, 0, 0, 0, 0, 0, 0, 0, 119, 114, 102, 99, 111, 105, 110, 46, 102, 101, 97, 116, 117, 114,
        101, 95, 115, 99, 104, 101, 100, 117, 108, 101, 95, 101, 120, 116, 101, 110, 115, 105, 111,
        110, 46, 118, 49, 3, 0, 0, 0, 0, 0, 0, 0, 55, 55, 55, 7, 0, 0, 0, 0, 0, 0, 0, 116, 101,
        115, 116, 110, 101, 116, 2, 0, 0, 0, 9, 1, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 55,
        55, 55, 7, 0, 0, 0, 0, 0, 0, 0, 116, 101, 115, 116, 110, 101, 116, 64, 0, 0, 0, 0, 0, 0, 0,
        97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97,
        98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98,
        97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 0, 0, 0, 0, 0, 0,
        0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 26, 0, 0, 0, 0, 0, 0, 0, 114, 101, 119, 97, 114, 100, 95,
        103, 114, 111, 115, 115, 95, 111, 117, 116, 108, 105, 101, 114, 95, 103, 117, 97, 114, 100,
        160, 90, 0, 0, 0, 0, 0, 0, 33, 0, 0, 0, 0, 0, 0, 0, 119, 101, 97, 116, 104, 101, 114, 95,
        115, 107, 101, 119, 95, 98, 108, 111, 99, 107, 95, 116, 105, 109, 101, 95, 97, 108, 108,
        111, 119, 97, 110, 99, 101, 160, 90, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0,
        40, 0, 0, 0, 0, 0, 0, 0, 97, 98, 49, 98, 49, 97, 52, 57, 51, 48, 48, 48, 48, 48, 48, 48,
        48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48,
        48,
    ];
    const GOLDEN_E1_PREIMAGE: &[u8] = &[
        37, 0, 0, 0, 0, 0, 0, 0, 119, 114, 102, 99, 111, 105, 110, 46, 102, 101, 97, 116, 117, 114,
        101, 95, 115, 99, 104, 101, 100, 117, 108, 101, 95, 101, 120, 116, 101, 110, 115, 105, 111,
        110, 46, 118, 49, 3, 0, 0, 0, 0, 0, 0, 0, 55, 55, 55, 7, 0, 0, 0, 0, 0, 0, 0, 116, 101,
        115, 116, 110, 101, 116, 2, 0, 0, 0, 186, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 55,
        55, 55, 7, 0, 0, 0, 0, 0, 0, 0, 116, 101, 115, 116, 110, 101, 116, 64, 0, 0, 0, 0, 0, 0, 0,
        97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97,
        98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98,
        97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 97, 98, 1, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 48, 117, 0, 0, 0, 0, 0, 0, 2, 0, 0,
        0, 3, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, 97, 98, 49, 98, 49, 97, 52, 57, 51, 48, 48, 48, 48,
        48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48,
        48, 48, 48, 48,
    ];

    #[test]
    fn golden_preimages_are_frozen() {
        assert_eq!(
            golden_e0().preimage(),
            GOLDEN_E0_PREIMAGE,
            "E0-shape preimage drifted"
        );
        assert_eq!(
            golden_e1().preimage(),
            GOLDEN_E1_PREIMAGE,
            "E1-shape preimage drifted"
        );
        assert_eq!(golden_e0().check_well_formed(), Ok(()));
        assert_eq!(golden_e1().check_well_formed(), Ok(()));
    }

    /// The approved validity caps (2026-09-24) are part of what the golden
    /// tests freeze: a signer relies on them as much as on the byte layout.
    #[test]
    fn golden_validity_caps_are_frozen() {
        assert_eq!(MAX_ENTRIES_PER_EXTENSION, 8);
        assert_eq!(MAX_VERSION_STEPS_PER_EXTENSION, 1);
        assert_eq!(MAX_FEATURE_NAME_LEN, 64);
        assert_eq!(MAX_RELEASE_COMMIT_LEN, 128);
        assert_eq!(GENESIS_HASH_HEX_LEN, 64);

        let at_caps = FeatureScheduleExtension {
            entries: vec![ScheduleEntry::new("a".repeat(MAX_FEATURE_NAME_LEN), 23_200)],
            release_commit: "f".repeat(MAX_RELEASE_COMMIT_LEN),
            ..golden_e0()
        };
        assert_eq!(at_caps.check_well_formed(), Ok(()));
        let long_name = FeatureScheduleExtension {
            entries: vec![ScheduleEntry::new(
                "a".repeat(MAX_FEATURE_NAME_LEN + 1),
                23_200,
            )],
            ..golden_e0()
        };
        assert!(matches!(
            long_name.check_well_formed(),
            Err(ScheduleExtensionError::MalformedFeatureName { .. })
        ));
        let long_commit = FeatureScheduleExtension {
            release_commit: "f".repeat(MAX_RELEASE_COMMIT_LEN + 1),
            ..golden_e0()
        };
        assert!(matches!(
            long_commit.check_well_formed(),
            Err(ScheduleExtensionError::ReleaseCommitTooLong { .. })
        ));
        for spelling in [GENESIS.to_ascii_uppercase(), format!("0x{GENESIS}")] {
            let e = FeatureScheduleExtension {
                genesis_hash: spelling,
                ..golden_e0()
            };
            assert!(matches!(
                e.check_well_formed(),
                Err(ScheduleExtensionError::MalformedGenesisHash { .. })
            ));
        }
    }

    /// Prints the golden bytes. Run only to CREATE the constants above for
    /// the first time; never to "fix" a drift.
    #[test]
    #[ignore = "generator for the golden constants; never regenerate to fix a drift"]
    fn print_golden_preimages() {
        let fmt = |b: Vec<u8>| {
            b.iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        println!("E0: {}", fmt(golden_e0().preimage()));
        println!("E1: {}", fmt(golden_e1().preimage()));
    }

    /// The determinism invariant on [`ScheduleExtensionAuthority`]: nothing
    /// in this module's non-test code reads the environment or the
    /// env-configured oracle allowlist. S5's installer must build the
    /// authority from block 1 and a compiled-in pin.
    #[test]
    fn authority_sources_are_never_the_environment() {
        let source = include_str!("schedule_extension.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("module has production code");
        for forbidden in [
            "std::env",
            "env::var",
            "oracle_governance_pubkeys",
            "ORACLE_GOVERNANCE_PUBKEYS_ENV",
            "std::fs",
        ] {
            assert!(
                !production.contains(forbidden),
                "schedule_extension.rs production code must not use {forbidden}: the \
                 authority is consensus input and must come from block 1 and a compiled-in pin"
            );
        }
    }
}
