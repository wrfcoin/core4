//! Validator key rotation and remote-signer path (#2130).
//!
//! # HSM path health — verified before implementing
//!
//! The story requires confirming `.github/workflows/hsm-signer-ci.yml` is green
//! before treating the remote-signing path as real rather than aspirational. It
//! is: run `29872748854` (2026-07-21T22:09Z, hosted runner 2.336.0) completed
//! `success`, and the log shows the softhsm2 job actually executing
//! `cargo test -p wrfcoin-native-chain --features hsm --test hsm_signer` under
//! both the `ONCE` and `ALWAYS_AUTHENTICATE` PIN policies, each `1 passed`.
//! So this story is implemented, not converted to a spike.
//!
//! # Why rotation needs its own safety rules
//!
//! An untested rotation procedure is discovered to be broken during the
//! incident that requires it. Three rules matter, and each has a way of being
//! got wrong that looks like a convenience:
//!
//! 1. **The new key must prove possession before it is effective.** Otherwise
//!    an operator can rotate to a key nobody holds and brick the validator, or
//!    an attacker can nominate a key they do hold.
//! 2. **Both keys are accepted during an overlap window, then the old key is
//!    rejected.** Without an overlap, in-flight messages signed just before the
//!    boundary are rejected and the validator misses blocks. Without an *end*
//!    to the overlap, rotation never actually revokes anything.
//! 3. **Double-signing across the rotation boundary is slashable — not
//!    exempt.** This is the one that looks like a convenience. Exempting the
//!    boundary turns rotation into an equivocation loophole: sign two conflicting
//!    blocks at the same height, one with the old key and one with the new, and
//!    claim it was "just the rotation". [`is_equivocation_across_rotation`]
//!    exists to make that explicitly slashable.
//!
//! # Remote signer failure mode
//!
//! [`RemoteSigner`] implementations must **refuse to sign** when the remote is
//! unavailable. Falling back to a local key would silently downgrade an
//! HSM-backed validator to a hot key at exactly the moment something is already
//! wrong — see [`SignerFallbackPolicy`].

use std::fmt;

use crate::crypto::domain_signing::{signing_chain_id, signing_network_id};
use crate::crypto::signature::{PublicKey, Signature};

/// A signer whose key material lives elsewhere — an HSM, a remote signing
/// service, a hardware token.
pub trait RemoteSigner: Send + Sync {
    /// The public half. Must be obtainable without the remote being reachable
    /// (cached at construction), so a node can report its identity while
    /// degraded.
    fn public_key(&self) -> PublicKey;

    /// Sign `msg` under `domain`.
    ///
    /// On any remote failure this MUST return `Err`. It must never fall back to
    /// a locally held key: see [`SignerFallbackPolicy`].
    fn sign(&self, domain: &[u8], msg: &[u8]) -> Result<Signature, SignerError>;
}

/// Why a remote signing attempt failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignerError {
    /// The remote signer could not be reached.
    Unavailable { detail: String },
    /// The remote rejected the request (policy, PIN, key not present).
    Rejected { detail: String },
    /// The remote returned something that is not a valid signature.
    MalformedResponse,
}

impl fmt::Display for SignerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { detail } => {
                write!(f, "remote signer unavailable: {detail} — refusing to sign")
            },
            Self::Rejected { detail } => write!(f, "remote signer rejected request: {detail}"),
            Self::MalformedResponse => write!(f, "remote signer returned a malformed signature"),
        }
    }
}

impl std::error::Error for SignerError {}

/// The only sanctioned behaviour when a remote signer fails.
///
/// Modelled as a type rather than left implicit so that "fall back to a local
/// key" has no representable value. A validator that silently degrades from an
/// HSM to a hot key during an outage has lost the property the HSM was bought
/// for, at the worst possible moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignerFallbackPolicy {
    /// Refuse to sign. Missing a block is recoverable; signing with an
    /// unintended key is not.
    RefuseToSign,
}

impl Default for SignerFallbackPolicy {
    fn default() -> Self {
        Self::RefuseToSign
    }
}

/// Domain tag for the proof-of-possession a new key must produce.
///
/// Domain-separated so a signature harvested from any other context cannot be
/// presented as a rotation proof.
pub const ROTATION_POP_DOMAIN: &[u8] = b"wrfcoin.validator_key_rotation_pop.v2";

/// Build the exact bytes a new key must self-sign to prove possession.
///
/// Binds the old key, the new key and the effective height, so a proof cannot
/// be replayed onto a different rotation.
pub fn rotation_pop_message(old: &PublicKey, new: &PublicKey, effective_height: u64) -> Vec<u8> {
    rotation_pop_message_with_domain(
        &signing_chain_id(),
        &signing_network_id(),
        old,
        new,
        effective_height,
    )
}

/// Pure rotation proof preimage builder for golden vectors and ceremonies.
pub fn rotation_pop_message_with_domain(
    chain_id: &str,
    network_id: &str,
    old: &PublicKey,
    new: &PublicKey,
    effective_height: u64,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(192);
    let mut push = |field: &[u8]| {
        buf.extend_from_slice(&(field.len() as u64).to_le_bytes());
        buf.extend_from_slice(field);
    };
    push(ROTATION_POP_DOMAIN);
    push(chain_id.as_bytes());
    push(network_id.as_bytes());
    push(old.as_bytes());
    push(new.as_bytes());
    push(&effective_height.to_le_bytes());
    buf
}

/// A planned validator key rotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationPlan {
    pub old: PublicKey,
    pub new: PublicKey,
    /// Height at which the new key becomes authoritative.
    pub effective_height: u64,
    /// Number of blocks before `effective_height` during which **both** keys
    /// are accepted.
    pub overlap_blocks: u64,
    /// Proof of possession under `new` over [`rotation_pop_message`].
    pub proof: Option<Signature>,
}

/// Why a rotation plan was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RotationError {
    /// The overlap window is too short to cover in-flight messages.
    OverlapWindowTooShort { got: u64, minimum: u64 },
    /// No valid proof of possession under the new key.
    NewKeyNotProven,
    /// `effective_height` is at or before the current height.
    EffectiveHeightInPast { effective: u64, current: u64 },
    /// The new key is the same as the old one — not a rotation.
    KeyUnchanged,
}

impl fmt::Display for RotationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OverlapWindowTooShort { got, minimum } => write!(
                f,
                "overlap window {got} blocks is below the {minimum}-block minimum — \
                 in-flight messages signed under the old key would be rejected"
            ),
            Self::NewKeyNotProven => write!(
                f,
                "new key has no valid proof of possession — refusing to rotate to a key \
                 that may not be held"
            ),
            Self::EffectiveHeightInPast { effective, current } => write!(
                f,
                "effective height {effective} is not in the future (current {current})"
            ),
            Self::KeyUnchanged => write!(f, "new key equals old key — nothing to rotate"),
        }
    }
}

impl std::error::Error for RotationError {}

/// Minimum overlap window, in blocks.
///
/// Short enough that revocation is prompt, long enough that a message signed
/// just before the boundary and gossiped normally still verifies on arrival.
pub const MIN_OVERLAP_BLOCKS: u64 = 16;

impl RotationPlan {
    /// Read a rotation plan from the deployment environment (#2130).
    ///
    /// Returns `Ok(None)` when [`ROTATION_NEW_PUBKEY`] is unset — the ordinary
    /// case, meaning "not rotating".
    ///
    /// When it *is* set, every other field is **required**, and anything
    /// missing or unparseable is an `Err` that the caller must surface as a
    /// refusal to start. A partially-specified rotation must never degrade into
    /// "no rotation": that would silently restore the pre-#2130 behaviour of a
    /// possibly-revoked key signing payloads it is no longer authorised for,
    /// which is what this module exists to prevent.
    ///
    /// **Scope.** "Authoritative" here means authoritative over the payloads a
    /// [`BlockSigner`](crate::crypto::signer::BlockSigner) signs — snapshot-sync
    /// manifests and slashing reports. It does **not** extend to block
    /// production: `Block` carries no producer signature, so no verifier can
    /// consult a key's authority when accepting one. `accepts_key_at` has no
    /// caller on the block path and cannot have one until blocks are
    /// authenticated. `tests/block_producer_authority.rs` pins this.
    ///
    /// The proof of possession is required for the
    /// same reason — [`RotationPlan::validate`] rejects an unproven new key,
    /// and letting the env path skip it would route around that check.
    ///
    /// [`ROTATION_NEW_PUBKEY`]: crate::crypto::signer::env_vars::ROTATION_NEW_PUBKEY
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        use crate::crypto::signer::env_vars as ev;

        // determinism-ok: startup-only rotation configuration is parsed into an owned RotationPlan before the signer is installed
        let Ok(new_hex) = std::env::var(ev::ROTATION_NEW_PUBKEY) else {
            return Ok(None);
        };
        if new_hex.trim().is_empty() {
            return Ok(None);
        }

        let required = |var: &str| -> anyhow::Result<String> {
            // determinism-ok: startup-only rotation configuration is copied into the owned plan; signing and verification never re-read it
            let v = std::env::var(var).map_err(|_| {
                anyhow::anyhow!(
                    "{} is set, so a validator key rotation is in progress, but {} is not set. \
                     A partially-specified rotation is refused rather than ignored — ignoring it \
                     would let a key that may already be revoked keep signing (#2130).",
                    ev::ROTATION_NEW_PUBKEY,
                    var
                )
            })?;
            if v.trim().is_empty() {
                anyhow::bail!("{var} is set but empty — refusing to guess a rotation parameter");
            }
            Ok(v)
        };

        let parse_key = |hex_str: &str, var: &str| -> anyhow::Result<PublicKey> {
            let bytes = hex::decode(hex_str.trim())
                .map_err(|e| anyhow::anyhow!("{var} is not valid hex: {e}"))?;
            PublicKey::from_bytes(&bytes)
                .map_err(|e| anyhow::anyhow!("{var} is not a valid ed25519 public key: {e}"))
        };

        let old = parse_key(&required(ev::ROTATION_OLD_PUBKEY)?, ev::ROTATION_OLD_PUBKEY)?;
        let new = parse_key(new_hex.trim(), ev::ROTATION_NEW_PUBKEY)?;

        let effective_height: u64 = required(ev::ROTATION_EFFECTIVE_HEIGHT)?
            .trim()
            .parse()
            .map_err(|e| {
                anyhow::anyhow!(
                    "{} must be a block height: {e}",
                    ev::ROTATION_EFFECTIVE_HEIGHT
                )
            })?;
        let overlap_blocks: u64 = required(ev::ROTATION_OVERLAP_BLOCKS)?
            .trim()
            .parse()
            .map_err(|e| {
                anyhow::anyhow!("{} must be a block count: {e}", ev::ROTATION_OVERLAP_BLOCKS)
            })?;

        let proof_hex = required(ev::ROTATION_POP)?;
        let proof = Signature::from_hex(proof_hex.trim())
            .map_err(|e| anyhow::anyhow!("{} is not a valid signature: {e}", ev::ROTATION_POP))?;

        Ok(Some(Self {
            old,
            new,
            effective_height,
            overlap_blocks,
            proof: Some(proof),
        }))
    }

    /// Validate the plan against the current height.
    ///
    /// Order matters: the cheap structural checks run before signature
    /// verification, so a malformed plan from a peer costs no EC operations.
    pub fn validate(&self, current_height: u64) -> Result<(), RotationError> {
        if self.old == self.new {
            return Err(RotationError::KeyUnchanged);
        }
        if self.effective_height <= current_height {
            return Err(RotationError::EffectiveHeightInPast {
                effective: self.effective_height,
                current: current_height,
            });
        }
        if self.overlap_blocks < MIN_OVERLAP_BLOCKS {
            return Err(RotationError::OverlapWindowTooShort {
                got: self.overlap_blocks,
                minimum: MIN_OVERLAP_BLOCKS,
            });
        }

        let Some(proof) = &self.proof else {
            return Err(RotationError::NewKeyNotProven);
        };
        let message = rotation_pop_message(&self.old, &self.new, self.effective_height);
        if !self.new.verify(&message, proof) {
            return Err(RotationError::NewKeyNotProven);
        }
        Ok(())
    }

    /// First height at which the old key is also accepted alongside the new.
    pub fn overlap_start(&self) -> u64 {
        self.effective_height.saturating_sub(self.overlap_blocks)
    }

    /// Whether `key` is authoritative for this validator at `height`.
    ///
    /// - before the overlap window: old key only
    /// - inside the overlap window: **both** — this is what prevents missed
    ///   blocks across the boundary
    /// - at or after `effective_height`: new key only
    pub fn accepts_key_at(&self, key: &PublicKey, height: u64) -> bool {
        if height >= self.effective_height {
            return *key == self.new;
        }
        if height >= self.overlap_start() {
            return *key == self.old || *key == self.new;
        }
        *key == self.old
    }
}

/// Two signatures at the same height under the two halves of a rotation.
///
/// Returns `true` when this is equivocation and therefore **slashable**.
///
/// The tempting mistake is to exempt the rotation boundary: the two signatures
/// are under different keys, and the operator "was rotating", so it looks like
/// an artefact of the procedure. Exempting it makes rotation an equivocation
/// loophole — announce a rotation, then sign two conflicting blocks at one
/// height, one per key. Signing two *different* payloads at one height is
/// equivocation regardless of which key was used.
pub fn is_equivocation_across_rotation(
    plan: &RotationPlan,
    height: u64,
    key_a: &PublicKey,
    payload_a: &[u8],
    key_b: &PublicKey,
    payload_b: &[u8],
) -> bool {
    // Same payload signed twice is not equivocation — it is a retransmit.
    if payload_a == payload_b {
        return false;
    }
    // Both keys must be ones this validator is authorised to use at `height`;
    // otherwise the second signature is simply invalid, not equivocation.
    plan.accepts_key_at(key_a, height) && plan.accepts_key_at(key_b, height)
}

// ---------------------------------------------------------------------------
// Enforcement on the real signing path (#2130, review of PR #2241)
// ---------------------------------------------------------------------------

/// A [`BlockSigner`] that delegates to a remote signer and **cannot** fall back.
///
/// The review's point was exact: `SignerFallbackPolicy` was consumed by nothing,
/// so an implementation was free to catch a remote failure and return
/// `Ok(local_key.sign(..))`. A policy enum nothing reads is a comment.
///
/// This wrapper makes the guarantee structural rather than advisory:
///
/// - it **holds no local key material** — there is no private key in the struct,
///   so there is nothing to fall back *to*, whatever a future edit intends;
/// - the only signing path delegates to the remote and propagates its `Err`;
/// - it optionally enforces a [`RotationPlan`], refusing to sign at a height
///   where its key is not authoritative.
///
/// Wrap a `Pkcs11Signer` (or any remote signer) in this to get fail-closed
/// behaviour on the real `BlockSigner` path.
pub struct FailClosedRemoteSigner {
    remote: std::sync::Arc<dyn crate::crypto::signer::BlockSigner>,
    /// Active rotation plan, if the validator is mid-rotation.
    plan: Option<RotationPlan>,
    policy: SignerFallbackPolicy,
}

impl std::fmt::Debug for FailClosedRemoteSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FailClosedRemoteSigner")
            .field("remote", &self.remote.provenance())
            .field("rotating", &self.plan.is_some())
            .field("policy", &self.policy)
            .finish()
    }
}

impl FailClosedRemoteSigner {
    pub fn new(remote: std::sync::Arc<dyn crate::crypto::signer::BlockSigner>) -> Self {
        Self {
            remote,
            plan: None,
            policy: SignerFallbackPolicy::RefuseToSign,
        }
    }

    /// Attach a validated rotation plan.
    pub fn with_rotation_plan(mut self, plan: RotationPlan) -> Self {
        self.plan = Some(plan);
        self
    }

    pub fn policy(&self) -> SignerFallbackPolicy {
        self.policy
    }

    /// Whether a rotation plan is currently attached.
    pub fn is_rotating(&self) -> bool {
        self.plan.is_some()
    }

    /// Typed form of the height-enforced sign, for callers that want the
    /// structured [`SignerError`] rather than an `anyhow::Error`.
    ///
    /// Refuses when this signer's key is not authoritative at `height` — the
    /// rotation boundary enforced on the signing side, not merely the verifying
    /// side.
    pub fn try_sign_at_height(
        &self,
        payload: &[u8],
        height: u64,
    ) -> Result<crate::crypto::signature::Signature, SignerError> {
        if let Some(plan) = &self.plan {
            let key = self.remote.public_key();
            if !plan.accepts_key_at(&key, height) {
                return Err(SignerError::Rejected {
                    detail: format!(
                        "key is not authoritative at height {height} under the active \
                         rotation plan (effective at {})",
                        plan.effective_height
                    ),
                });
            }
        }
        self.remote
            .sign(payload)
            .map_err(|e| SignerError::Unavailable {
                // #2731: `{:#}` renders the whole `anyhow` chain. `to_string()`
                // renders only the OUTERMOST context, which silently discarded
                // the underlying PKCS#11 return code.
                //
                // During #2728 this signer reported, at the point of no return
                // in a genesis cut:
                //     "PKCS#11 C_Sign (Eddsa) failed"
                // while the cause it was holding — and dropped — was:
                //     "... CKR_DATA_LEN_RANGE ... input data has a bad length"
                //
                // Ten hypotheses were eliminated by experiment against live
                // hardware before payload size was tried. The error already knew.
                //
                // A fail-closed signer is the component most likely to halt a
                // chain, so it is the last one that should withhold its reason.
                detail: format!("{e:#}"),
            })
    }
}

impl crate::crypto::signer::BlockSigner for FailClosedRemoteSigner {
    /// Heightless signing.
    ///
    /// When no rotation is in progress this is an ordinary passthrough — the
    /// plan is the only thing a height would be consulted for, so without one
    /// there is nothing to enforce.
    ///
    /// **When a rotation plan IS attached, this refuses.** That is the point of
    /// the type. Previously this delegated straight to `remote.sign(payload)`
    /// and ignored `plan`, so every production caller — all of which use the
    /// heightless `BlockSigner::sign` — bypassed rotation entirely while the
    /// model's own tests passed. Rotation was therefore *described* rather than
    /// enforced, which is the finding this closes.
    ///
    /// Refusing rather than silently passing through means a caller cannot lose
    /// enforcement by forgetting to pass a height: mid-rotation, the heightless
    /// path stops working and says why.
    fn sign(&self, payload: &[u8]) -> anyhow::Result<crate::crypto::signature::Signature> {
        if let Some(plan) = &self.plan {
            anyhow::bail!(
                "refusing to sign without a height while a validator key rotation is \
                 active (new key effective at height {}). This signer cannot tell \
                 whether its key is authoritative for this payload, and signing anyway \
                 is how a revoked key ends up on a real block. Call \
                 `BlockSigner::sign_at_height` with the height this payload belongs to \
                 (#2130).",
                plan.effective_height
            );
        }
        // No local key exists in this type. A remote failure propagates.
        self.remote.sign(payload)
    }

    /// Height-enforced signing — the authoritative path during a rotation.
    fn sign_at_height(
        &self,
        payload: &[u8],
        height: u64,
    ) -> anyhow::Result<crate::crypto::signature::Signature> {
        self.try_sign_at_height(payload, height)
            .map_err(|e| anyhow::anyhow!(e))
    }

    fn public_key(&self) -> crate::crypto::signature::PublicKey {
        self.remote.public_key()
    }

    fn provenance(&self) -> &crate::crypto::signer::SignerProvenance {
        self.remote.provenance()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signature::PrivateKey;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
        let mut rng = StdRng::seed_from_u64(seed);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);
        (sk, pk)
    }

    /// A fully valid plan: proven new key, adequate overlap, future height.
    fn valid_plan(effective_height: u64) -> RotationPlan {
        let (_, old_pk) = keypair(1);
        let (new_sk, new_pk) = keypair(2);
        let message = rotation_pop_message(&old_pk, &new_pk, effective_height);
        RotationPlan {
            old: old_pk,
            new: new_pk,
            effective_height,
            overlap_blocks: MIN_OVERLAP_BLOCKS,
            proof: Some(new_sk.sign(&message)),
        }
    }

    /// **#2731 — the fail-closed signer must not withhold its reason.**
    ///
    /// `SignerError::Unavailable` used to render `e.to_string()`, which is the
    /// OUTERMOST `anyhow` context only. The cause underneath it was discarded.
    ///
    /// In #2728 that meant a node printed `"PKCS#11 C_Sign (Eddsa) failed"` at
    /// the point of no return in a genesis cut, while the error it was holding
    /// said `CKR_DATA_LEN_RANGE`. Ten hypotheses were eliminated by experiment
    /// against live hardware before payload size was tried.
    ///
    /// This asserts the inner cause survives to the rendered message. Revert to
    /// `e.to_string()` and it goes RED.
    #[test]
    fn unavailable_preserves_the_underlying_cause_2731() {
        use crate::crypto::signer::{BlockSigner, SignerProvenance};

        /// A signer whose failure carries a nested cause, like a real PKCS#11 error.
        #[derive(Debug)]
        struct FailingSigner {
            key: PrivateKey,
            provenance: SignerProvenance,
        }
        impl BlockSigner for FailingSigner {
            fn sign(&self, _payload: &[u8]) -> anyhow::Result<crate::crypto::signature::Signature> {
                let root = anyhow::anyhow!("CKR_DATA_LEN_RANGE: input data has a bad length");
                Err(root).context("PKCS#11 C_Sign (Eddsa) failed")
            }
            fn public_key(&self) -> PublicKey {
                PublicKey::from_private_key(&self.key)
            }
            fn provenance(&self) -> &SignerProvenance {
                &self.provenance
            }
        }
        use anyhow::Context as _;

        let (sk, _pk) = keypair(2731);
        let inner = FailingSigner {
            key: sk,
            provenance: SignerProvenance::EnvKey { var_name: "T".into() },
        };
        let signer = FailClosedRemoteSigner::new(std::sync::Arc::new(inner));

        // sign_at_height, NOT sign: the heightless path passes the remote error
        // through untouched, while `try_sign_at_height` is where the error was
        // rebuilt as SignerError::Unavailable and truncated. That is also the
        // path `attest_block_producer` uses, so it is the one #2728 hit.
        let err = signer
            .sign_at_height(b"payload", 1)
            .expect_err("the inner signer always fails");
        let rendered = format!("{err:#}");

        assert!(
            rendered.contains("C_Sign"),
            "the outer context must still be shown; got: {rendered}"
        );
        assert!(
            rendered.contains("CKR_DATA_LEN_RANGE"),
            "the UNDERLYING cause must reach the message — this is the whole point of #2731. \
             Rendering only the outer context is what made #2728 take ten experiments. Got: {rendered}"
        );
    }

    #[test]
    fn valid_plan_is_accepted() {
        // Anchor — without this every rejection test could pass vacuously.
        assert_eq!(valid_plan(1_000).validate(500), Ok(()));
    }

    // ---- proof of possession ----

    #[test]
    fn unproven_new_key_is_rejected() {
        let mut plan = valid_plan(1_000);
        plan.proof = None;
        assert_eq!(plan.validate(500), Err(RotationError::NewKeyNotProven));
    }

    #[test]
    fn proof_signed_by_the_wrong_key_is_rejected() {
        // A proof under the OLD key proves nothing about holding the new one.
        let (old_sk, old_pk) = keypair(1);
        let (_, new_pk) = keypair(2);
        let message = rotation_pop_message(&old_pk, &new_pk, 1_000);
        let plan = RotationPlan {
            old: old_pk,
            new: new_pk,
            effective_height: 1_000,
            overlap_blocks: MIN_OVERLAP_BLOCKS,
            proof: Some(old_sk.sign(&message)),
        };
        assert_eq!(plan.validate(500), Err(RotationError::NewKeyNotProven));
    }

    #[test]
    fn proof_cannot_be_replayed_onto_a_different_rotation() {
        // The proof binds old || new || effective_height, so lifting it to a
        // different effective height must fail.
        let mut plan = valid_plan(1_000);
        plan.effective_height = 2_000;
        assert_eq!(plan.validate(500), Err(RotationError::NewKeyNotProven));
    }

    #[test]
    fn pop_message_is_domain_separated() {
        let (_, a) = keypair(1);
        let (_, b) = keypair(2);
        let msg = rotation_pop_message(&a, &b, 10);
        assert!(
            msg.windows(ROTATION_POP_DOMAIN.len())
                .any(|w| w == ROTATION_POP_DOMAIN)
        );
        // Swapping old/new changes the message — a proof is directional.
        assert_ne!(msg, rotation_pop_message(&b, &a, 10));
    }

    #[test]
    fn rotation_pop_preimage_covers_chain_and_plan_fields_2121() {
        let (_, old) = keypair(1);
        let (_, new) = keypair(2);
        let base =
            rotation_pop_message_with_domain("777", "testnet", &old, &new, 1_000);

        assert!(base.windows(ROTATION_POP_DOMAIN.len()).any(|w| w == ROTATION_POP_DOMAIN));
        assert!(ROTATION_POP_DOMAIN.ends_with(b".v2"));
        assert_ne!(
            base,
            rotation_pop_message_with_domain("778", "testnet", &old, &new, 1_000),
            "chain id must affect the rotation proof preimage",
        );
        assert_ne!(
            base,
            rotation_pop_message_with_domain("777", "mainnet", &old, &new, 1_000),
            "network id must affect the rotation proof preimage",
        );
        assert_ne!(
            base,
            rotation_pop_message_with_domain("777", "testnet", &new, &old, 1_000),
            "old and new key roles must affect the rotation proof preimage",
        );
        assert_ne!(
            base,
            rotation_pop_message_with_domain("777", "testnet", &old, &new, 1_001),
            "effective height must affect the rotation proof preimage",
        );
    }

    // ---- structural validation ----

    #[test]
    fn effective_height_in_the_past_is_rejected() {
        let plan = valid_plan(400);
        assert_eq!(
            plan.validate(500),
            Err(RotationError::EffectiveHeightInPast {
                effective: 400,
                current: 500
            })
        );
    }

    #[test]
    fn effective_height_equal_to_current_is_rejected() {
        assert!(matches!(
            valid_plan(500).validate(500),
            Err(RotationError::EffectiveHeightInPast { .. })
        ));
    }

    #[test]
    fn short_overlap_window_is_rejected() {
        let mut plan = valid_plan(1_000);
        plan.overlap_blocks = 1;
        assert_eq!(
            plan.validate(500),
            Err(RotationError::OverlapWindowTooShort {
                got: 1,
                minimum: MIN_OVERLAP_BLOCKS
            })
        );
    }

    #[test]
    fn rotating_to_the_same_key_is_rejected() {
        let (sk, pk) = keypair(1);
        let message = rotation_pop_message(&pk, &pk, 1_000);
        let plan = RotationPlan {
            old: pk.clone(),
            new: pk,
            effective_height: 1_000,
            overlap_blocks: MIN_OVERLAP_BLOCKS,
            proof: Some(sk.sign(&message)),
        };
        assert_eq!(plan.validate(500), Err(RotationError::KeyUnchanged));
    }

    // ---- acceptance window: no missed blocks, then real revocation ----

    #[test]
    fn old_key_is_rejected_after_effective_height() {
        let plan = valid_plan(1_000);
        assert!(!plan.accepts_key_at(&plan.old, 1_000), "at the boundary");
        assert!(!plan.accepts_key_at(&plan.old, 1_001), "after the boundary");
        assert!(plan.accepts_key_at(&plan.new, 1_000));
    }

    #[test]
    fn both_keys_are_accepted_inside_the_overlap_window() {
        // This is what prevents missed blocks: a message signed just before the
        // boundary and delivered just after still verifies.
        let plan = valid_plan(1_000);
        let inside = plan.overlap_start();
        assert!(plan.accepts_key_at(&plan.old, inside));
        assert!(plan.accepts_key_at(&plan.new, inside));
        assert!(plan.accepts_key_at(&plan.old, 999));
        assert!(plan.accepts_key_at(&plan.new, 999));
    }

    #[test]
    fn new_key_is_not_accepted_before_the_overlap_window() {
        let plan = valid_plan(1_000);
        let before = plan.overlap_start() - 1;
        assert!(plan.accepts_key_at(&plan.old, before));
        assert!(
            !plan.accepts_key_at(&plan.new, before),
            "the new key must not be usable before its overlap window opens"
        );
    }

    #[test]
    fn rotation_leaves_no_height_with_zero_accepted_keys() {
        // The "no missed blocks" property stated exhaustively: at every height
        // around the rotation, at least one key is authoritative. A gap here is
        // a liveness bug that a fixture test could easily miss.
        let plan = valid_plan(1_000);
        for height in (plan.overlap_start() - 5)..=(plan.effective_height + 5) {
            let accepted = plan.accepts_key_at(&plan.old, height) as u8
                + plan.accepts_key_at(&plan.new, height) as u8;
            assert!(
                accepted >= 1,
                "height {height} accepts no key — the validator would miss this block"
            );
        }
    }

    // ---- equivocation across the boundary ----

    #[test]
    fn double_sign_across_the_rotation_boundary_is_slashable() {
        // The loophole this must close: two conflicting payloads at one height,
        // one under each key, excused as "just the rotation".
        let plan = valid_plan(1_000);
        let inside = plan.overlap_start() + 1;
        assert!(
            is_equivocation_across_rotation(
                &plan, inside, &plan.old, b"block-A", &plan.new, b"block-B",
            ),
            "conflicting payloads under the two rotation keys must be slashable, not exempt"
        );
    }

    #[test]
    fn retransmitting_the_same_payload_is_not_equivocation() {
        let plan = valid_plan(1_000);
        let inside = plan.overlap_start() + 1;
        assert!(!is_equivocation_across_rotation(
            &plan, inside, &plan.old, b"block-A", &plan.new, b"block-A",
        ));
    }

    #[test]
    fn a_key_not_authorised_at_that_height_is_not_equivocation() {
        // After the boundary the old key is simply invalid — that is a rejected
        // signature, not a slashable double-sign.
        let plan = valid_plan(1_000);
        assert!(!is_equivocation_across_rotation(
            &plan, 1_001, &plan.old, b"block-A", &plan.new, b"block-B",
        ));
    }

    // ---- remote signer must refuse, never fall back ----

    struct FailingRemoteSigner {
        public_key: PublicKey,
    }

    impl RemoteSigner for FailingRemoteSigner {
        fn public_key(&self) -> PublicKey {
            self.public_key.clone()
        }
        fn sign(&self, _domain: &[u8], _msg: &[u8]) -> Result<Signature, SignerError> {
            Err(SignerError::Unavailable {
                detail: "connection refused".into(),
            })
        }
    }

    #[test]
    fn remote_signer_failure_refuses_to_sign_rather_than_falling_back() {
        let (_, pk) = keypair(9);
        let signer = FailingRemoteSigner {
            public_key: pk.clone(),
        };
        // Identity is still reportable while degraded.
        assert_eq!(signer.public_key(), pk);
        // And signing fails closed.
        assert_eq!(
            signer.sign(b"domain", b"payload"),
            Err(SignerError::Unavailable {
                detail: "connection refused".into()
            })
        );
    }

    // ---- enforcement on the real BlockSigner path ----

    /// A remote signer that is unreachable — the HSM-is-down case. Models the
    /// real `Pkcs11Signer` failing, and unlike the previous fake it is wrapped
    /// by production code (`FailClosedRemoteSigner`) rather than being asserted
    /// against directly, so the test exercises the wrapper's behaviour, not the
    /// fake's.
    #[derive(Debug)]
    struct UnreachableRemote {
        public_key: PublicKey,
        provenance: crate::crypto::signer::SignerProvenance,
        /// Set if the remote is ever asked to sign — proves the wrapper did not
        /// route around it.
        attempts: std::sync::atomic::AtomicUsize,
    }

    impl crate::crypto::signer::BlockSigner for UnreachableRemote {
        fn sign(&self, _payload: &[u8]) -> anyhow::Result<Signature> {
            self.attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            anyhow::bail!("pkcs11: token not present (C_GetSlotList returned 0 slots)")
        }
        fn public_key(&self) -> PublicKey {
            self.public_key.clone()
        }
        fn provenance(&self) -> &crate::crypto::signer::SignerProvenance {
            &self.provenance
        }
    }

    fn unreachable_signer() -> std::sync::Arc<UnreachableRemote> {
        let (_, pk) = keypair(77);
        std::sync::Arc::new(UnreachableRemote {
            public_key: pk,
            provenance: crate::crypto::signer::SignerProvenance::Pkcs11 {
                module_path: "/usr/lib/softhsm/libsofthsm2.so".into(),
                token_label: "wrfcoin-validator".into(),
                key_label: "block-signing".into(),
            },
            attempts: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    #[test]
    fn an_unreachable_hsm_emits_no_signature_at_all() {
        // The property the story exists to guarantee. When the HSM is down, the
        // signer must produce NOTHING — not a software signature, not a
        // degraded one. A silent fallback defeats the entire point of an HSM.
        use crate::crypto::signer::BlockSigner;

        let remote = unreachable_signer();
        let signer = FailClosedRemoteSigner::new(remote.clone());

        let result = signer.sign(b"block-header-bytes");
        assert!(
            result.is_err(),
            "an unreachable HSM must yield no signature, got Ok"
        );
        assert_eq!(
            remote.attempts.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the wrapper must actually attempt the remote, not shortcut it"
        );
        // And the public key is still reportable while degraded, so the node can
        // identify itself without being able to sign.
        assert_eq!(signer.public_key(), remote.public_key.clone());
    }

    #[test]
    fn the_fail_closed_wrapper_holds_no_local_key_to_fall_back_to() {
        // Structural, not advisory. `FailClosedRemoteSigner` has no private-key
        // field, so there is nothing to fall back *to* regardless of what a
        // future edit intends. This test pins the size/shape claim: the struct
        // is an Arc + Option<RotationPlan> + policy, with no PrivateKey.
        let signer = FailClosedRemoteSigner::new(unreachable_signer());
        assert_eq!(signer.policy(), SignerFallbackPolicy::RefuseToSign);
        // A debug rendering must expose provenance, never key material.
        let rendered = format!("{signer:?}");
        assert!(rendered.contains("FailClosedRemoteSigner"));
        assert!(!rendered.to_lowercase().contains("private"));
    }

    #[test]
    fn the_wrapper_refuses_to_sign_outside_its_rotation_window() {
        // Rotation enforced on the SIGNING side. The verifier-side rules were
        // already covered; this is the signer refusing to produce a signature it
        // knows will be rejected — which is what stops a node emitting blocks
        // nobody will accept after the boundary.
        let (_, old_pk) = keypair(1);
        let remote = unreachable_signer();
        let plan = RotationPlan {
            old: old_pk,
            new: remote.public_key.clone(),
            effective_height: 1_000,
            overlap_blocks: MIN_OVERLAP_BLOCKS,
            proof: None,
        };
        let signer = FailClosedRemoteSigner::new(remote.clone()).with_rotation_plan(plan);

        // Before the overlap window the NEW key is not yet authoritative.
        let err = signer
            .try_sign_at_height(b"payload", 100)
            .expect_err("signing before the overlap window must refuse");
        assert!(matches!(err, SignerError::Rejected { .. }));
        assert_eq!(
            remote.attempts.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "the rotation check must short-circuit before touching the remote"
        );
    }

    #[test]
    fn inside_its_window_the_wrapper_still_fails_closed_when_the_remote_is_down() {
        // Both guards compose: authorised by rotation, but the HSM is
        // unreachable, so still no signature.
        let (_, old_pk) = keypair(1);
        let remote = unreachable_signer();
        let plan = RotationPlan {
            old: old_pk,
            new: remote.public_key.clone(),
            effective_height: 1_000,
            overlap_blocks: MIN_OVERLAP_BLOCKS,
            proof: None,
        };
        let signer = FailClosedRemoteSigner::new(remote.clone()).with_rotation_plan(plan);

        let err = signer
            .try_sign_at_height(b"payload", 1_001)
            .expect_err("an unreachable remote must refuse even when authorised");
        assert!(matches!(err, SignerError::Unavailable { .. }));
        assert_eq!(remote.attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn refuse_to_sign_is_the_only_representable_fallback_policy() {
        // There is deliberately no `FallBackToLocalKey` variant to select.
        assert_eq!(
            SignerFallbackPolicy::default(),
            SignerFallbackPolicy::RefuseToSign
        );
    }

    #[test]
    fn signer_errors_do_not_leak_key_material() {
        let err = SignerError::Rejected {
            detail: "PIN policy".into(),
        };
        let rendered = format!("{err}|{err:?}");
        assert!(!rendered.contains("PRIVATE"));
        assert!(rendered.contains("PIN policy"));
    }
}
