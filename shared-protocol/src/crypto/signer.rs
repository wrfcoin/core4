//! Block-signing abstraction backed by either an env-var key (debug) or a
//! PKCS#11 token (release).
//!
//! Resolves core4#1394: before this module, the Rust block producer loaded
//! validator private keys from env vars (`PrivateKey::from_hex(...)`) in
//! every build profile. This module adds a `BlockSigner` trait with two
//! implementations:
//!
//! - [`EnvKeySigner`] — `cfg(debug_assertions)`-gated. Reads a hex-encoded
//!   ed25519 key from an env var. The release profile refuses to compile
//!   this path into production binaries.
//! - [`Pkcs11Signer`] — gated behind the `hsm` cargo feature. Delegates
//!   signing to a PKCS#11 token (YubiKey via `opensc-pkcs11.so`,
//!   `softhsm2` for CI, AWS CloudHSM for mainnet). The private key never
//!   leaves the token.
//!
//! See `docs/operator/hsm-migration.md` for provisioning recipes.

use crate::crypto::signature::{PrivateKey, PublicKey, Signature};
use anyhow::{Result, anyhow, bail};
use std::fmt;

/// Canonical env var names for signer configuration. Single source of truth
/// so docs, tests, and call sites cannot drift.
pub mod env_vars {
    /// Path to the PKCS#11 shared object (e.g. `/usr/lib/opensc-pkcs11.so`).
    pub const HSM_MODULE_PATH: &str = "WRFCOIN_HSM_MODULE_PATH";
    /// Token label inside the PKCS#11 module (e.g. `wrfcoin-validator`).
    pub const HSM_TOKEN_LABEL: &str = "WRFCOIN_HSM_TOKEN_LABEL";
    /// Label of the ed25519 private key object inside the token.
    pub const HSM_KEY_LABEL: &str = "WRFCOIN_HSM_KEY_LABEL";
    /// Optional override for the public key object label. Defaults to
    /// `HSM_KEY_LABEL` when unset. Yubico's `libykcs11.so` exposes the PIV
    /// keypair under different labels (`Private key for Digital Signature`
    /// vs `Public key for Digital Signature`); this env var lets operators
    /// point each lookup at the right label without changing the runtime.
    pub const HSM_PUBKEY_LABEL: &str = "WRFCOIN_HSM_PUBKEY_LABEL";
    /// Name of the env var that holds the token PIN (double-env indirection
    /// so the PIN is never written into systemd unit files or process listings).
    pub const HSM_PIN_ENV: &str = "WRFCOIN_HSM_PIN_ENV";
    /// Legacy hex-encoded ed25519 private key. Debug builds only.
    pub const SNAPSHOT_SIGNER_PRIVATE_KEY: &str = "WRFCOIN_SNAPSHOT_SIGNER_PRIVATE_KEY";

    /// Producer signing key (#2290) — the key that authenticates blocks this node
    /// produces. Distinct from the snapshot signer. Release builds resolve it via
    /// the `WRFCOIN_HSM_*` PKCS#11 path; this env var is the debug-only
    /// hex-encoded ed25519 private-key fallback.
    pub const PRODUCER_SIGNER_PRIVATE_KEY: &str = "WRFCOIN_PRODUCER_SIGNER_PRIVATE_KEY";

    /// Path to a file holding the hex-encoded producer private key (#2290 P1-4).
    ///
    /// The dev/staging path between "hex in the environment" and "HSM": a
    /// process environment is visible to anything that can read `/proc`, and
    /// leaks into systemd units, shell history and crash dumps. A file can at
    /// least be mode 0600 — which is enforced, not assumed
    /// ([`crate::security::key_file`]). Debug builds only, like the hex var.
    pub const PRODUCER_SIGNER_KEY_FILE: &str = "WRFCOIN_PRODUCER_SIGNER_KEY_FILE";

    /// Producer-specific PKCS#11 **key label** (#2290 P1-4).
    ///
    /// Without this, the producer signer resolves the same global
    /// [`HSM_KEY_LABEL`] as the snapshot signer, so one HSM key would sign both
    /// snapshot manifests and blocks. Distinct roles want distinct keys: they
    /// have different blast radii and different rotation schedules. Setting this
    /// selects a producer-specific key object; leaving it unset reuses the
    /// shared label and logs a loud warning, because key reuse across roles is a
    /// decision an owner should make explicitly rather than inherit by default.
    pub const HSM_PRODUCER_KEY_LABEL: &str = "WRFCOIN_HSM_PRODUCER_KEY_LABEL";

    /// Optional producer-specific PKCS#11 **public key label**, mirroring
    /// [`HSM_PUBKEY_LABEL`] for the producer role. Defaults to
    /// [`HSM_PRODUCER_KEY_LABEL`] when unset.
    pub const HSM_PRODUCER_PUBKEY_LABEL: &str = "WRFCOIN_HSM_PRODUCER_PUBKEY_LABEL";

    // -- Oracle attestation signing (#2464) --------------------------------
    //
    // A third signing role, alongside the snapshot signer and the block
    // producer. It signs the `wrfcoin.oracle_attestation.v1` domain: the votes
    // that decide whether a weather contribution is paid.
    //
    // CUSTODY RULE: an oracle key MUST NOT be a producer key. See
    // [`SignerEnv::oracle`] for the rule and for where it is (and is not)
    // enforced. The env vars are separate from the producer's precisely so that
    // "these are different keys" is expressible in configuration rather than
    // only in a runbook.
    /// Oracle signing key (#2464) — the key that signs oracle attestations.
    /// Debug-only hex-encoded ed25519 private-key fallback, mirroring
    /// [`PRODUCER_SIGNER_PRIVATE_KEY`].
    pub const ORACLE_SIGNER_PRIVATE_KEY: &str = "WRFCOIN_ORACLE_SIGNER_PRIVATE_KEY";

    /// Path to a file holding the hex-encoded oracle private key (#2464),
    /// mirroring [`PRODUCER_SIGNER_KEY_FILE`]. Mode 0600 is enforced, not
    /// assumed ([`crate::security::key_file`]). Debug builds only.
    pub const ORACLE_SIGNER_KEY_FILE: &str = "WRFCOIN_ORACLE_SIGNER_KEY_FILE";

    /// Oracle-specific PKCS#11 **key label** (#2464), mirroring
    /// [`HSM_PRODUCER_KEY_LABEL`].
    ///
    /// The #2464 oracle-key decision recorded that no such variable existed, so
    /// "put another key on the token" was not a configurable arrangement: an
    /// oracle signer would have resolved the *shared* [`HSM_KEY_LABEL`] — the
    /// snapshot key — and no amount of ceremony discipline could have expressed
    /// otherwise. Setting this selects an oracle-specific key object.
    pub const HSM_ORACLE_KEY_LABEL: &str = "WRFCOIN_HSM_ORACLE_KEY_LABEL";

    /// Optional oracle-specific PKCS#11 **public key label** (#2464), mirroring
    /// [`HSM_PRODUCER_PUBKEY_LABEL`]. Defaults to [`HSM_ORACLE_KEY_LABEL`] when
    /// unset — the shared [`HSM_PUBKEY_LABEL`] is NOT consulted once an oracle
    /// key label is in force (#2405).
    pub const HSM_ORACLE_PUBKEY_LABEL: &str = "WRFCOIN_HSM_ORACLE_PUBKEY_LABEL";

    // -- Validator key rotation (#2130) -----------------------------------
    //
    // Presence of ROTATION_NEW_PUBKEY is what puts a node into rotating mode.
    // All four are then required: a partially-specified rotation is a refusal
    // to start, never a best-effort guess, because the failure it would cause
    // is a revoked key signing a payload it is no longer authorised for.
    //
    // SCOPE (#2290): this governs every payload a `BlockSigner` signs —
    // snapshot-sync manifests, slashing reports, AND blocks. The trait's name is
    // finally accurate: `attest_block_producer` signs the block-producer
    // preimage through `sign_at_height`, so the rotation boundary is enforced on
    // the block path too. The `ROTATION_SCOPE_ACK` gate that used to make
    // operators affirm the opposite was removed here.
    /// Hex-encoded ed25519 public key being rotated **away from**.
    pub const ROTATION_OLD_PUBKEY: &str = "WRFCOIN_ROTATION_OLD_PUBKEY";
    /// Hex-encoded ed25519 public key being rotated **to**.
    pub const ROTATION_NEW_PUBKEY: &str = "WRFCOIN_ROTATION_NEW_PUBKEY";
    /// Height at which the new key becomes solely authoritative.
    pub const ROTATION_EFFECTIVE_HEIGHT: &str = "WRFCOIN_ROTATION_EFFECTIVE_HEIGHT";
    /// Blocks before `effective_height` during which both keys are accepted.
    pub const ROTATION_OVERLAP_BLOCKS: &str = "WRFCOIN_ROTATION_OVERLAP_BLOCKS";
    /// Hex-encoded proof-of-possession signature by the new key.
    pub const ROTATION_POP: &str = "WRFCOIN_ROTATION_POP";
    // `WRFCOIN_ROTATION_SCOPE_ACK` was REMOVED in #2290.
    //
    // It forced the operator to affirm `payload-signing-only`: that rotation did
    // not cover block production, because a `Block` carried no producer
    // signature. That was accurate under #2130 and is false now — blocks carry
    // `producer_pubkey`/`producer_sig` and are signed at their height. A startup
    // gate demanding assent to a false statement is worse than no gate, so it
    // was deleted rather than reworded. Nothing reads this var; setting it is
    // inert.
}

/// Verified `CKA_ID` -> PIV slot annotation for the mismatch diagnostic (#2405).
///
/// Mirrors `pkcs11::piv_slot_for_cka_id`, duplicated here only so the diagnostic
/// renders identically in non-`hsm` builds (where that module is compiled out).
/// Confirmed by a read-only dump of the standby YubiKey, serial 35278471; `03`
/// added from the 2026-08-04 oracle-ceremony dump of the same token.
///
/// **Both copies must stay in step.** They are asserted equal over every id they
/// map, plus a sample of unmapped ones, by
/// `pkcs11::tests::piv_slot_mirror_matches_pkcs11_table` — the only thing that
/// stops this duplication drifting into two different diagnostics for the same
/// token.
pub(crate) fn pkcs11_piv_slot(id: &str) -> Option<&'static str> {
    match id {
        "01" => Some("9A (PIV Authentication)"),
        "02" => Some("9C (Digital Signature)"),
        "03" => Some("9D (Key Management)"),
        "19" => Some("attestation"),
        _ => None,
    }
}

/// Fixed, non-secret payload the startup keypair self-check signs (#2405).
///
/// Constant and domain-tagged on purpose: it is signed at every startup with the
/// production private key, so it must never collide with a real signable message
/// (a block preimage, a snapshot manifest, a slashing report). Anything an
/// attacker could induce a node to sign is a signing oracle; this string is not
/// a valid encoding of any of them.
pub const KEYPAIR_SELFCHECK_PAYLOAD: &[u8] = b"wrfcoin.signer.keypair-selfcheck.v1";

/// The two halves of a resolved signing keypair, for the #2405 startup check.
///
/// Carries the labels AND the env vars they came from, because the operator
/// action on failure is "set this variable to that label" — an error that names
/// only the labels leaves them guessing which knob produced it.
///
/// Every field is an *identifier*, never key material: PKCS#11 object labels
/// (`wrfcoin-signing-key`), the env-var names those labels came from
/// (`WRFCOIN_HSM_KEY_LABEL`), `CKA_ID` object ids, and the token slot handle.
/// `private_label`/`private_id` name the private *object*; they do not carry
/// its value, and no PKCS#11 read here requests one — `read_object_id` asks
/// only for `AttributeType::Id`. `Debug` is load-bearing rather than
/// incidental: the #2405 failure path must tell the operator which knob to set.
#[derive(Debug, Clone)] // secret-leak-ok: labels, env-var names and CKA_IDs only — no field holds key material (#2287)
pub struct ResolvedKeypairLabels {
    /// e.g. "producer" — the role whose pair is being checked.
    pub role: String,
    /// PKCS#11 slot / token the pair was resolved from.
    pub slot: String,
    pub token_label: String,
    pub private_label: String,
    pub private_label_var: String,
    /// The private object's `CKA_ID`, hex. `None` when the token does not expose
    /// one. This is the field that actually distinguishes two PIV slots — the
    /// token-level slot handle is shared by every object on the token, so it
    /// cannot (#2405).
    pub private_id: Option<String>,
    pub public_label: String,
    pub public_label_var: String,
    /// The public object's `CKA_ID`, hex. Differs from `private_id` exactly when
    /// the two halves came from different slots — the misconfiguration this
    /// whole check exists to catch.
    pub public_id: Option<String>,
}

/// Name the signing role that a key-label **variable** designates.
///
/// Derived from which env var supplied the label, never from the label's value:
/// the label is operator-chosen and carries no role information, whereas the
/// variable *is* the role's seam. `None` means no role-specific variable was
/// used, i.e. the shared [`env_vars::HSM_KEY_LABEL`] path.
///
/// #2458 shipped this mapping inline in the PKCS#11 constructor with a
/// producer arm and an `else "signer"`, and the `"oracle"` arm was deferred
/// until that PR landed. It landed; this closes it.
///
/// Lives here rather than inline so it is testable without a token — the
/// mapping had no test at all, and a one-line arm omitted from an untested
/// `if` is exactly what went missing the first time.
///
/// The string is operator-facing: the fatal keypair-mismatch diagnostic uses it
/// to say *which variable* to fix, so a role rendering as the generic
/// `"signer"` sends them to the wrong knob. For #2464 it matters more than
/// that — producer/oracle custody distinctness is the point of the issue, and a
/// role that cannot name itself at runtime makes that model unauditable from
/// the logs.
pub fn role_for_key_label_var(key_label_var: Option<&str>) -> &'static str {
    match key_label_var {
        Some(var) if var == env_vars::HSM_PRODUCER_KEY_LABEL => "producer",
        Some(var) if var == env_vars::HSM_ORACLE_KEY_LABEL => "oracle",
        _ => "signer",
    }
}

impl ResolvedKeypairLabels {
    /// Render both halves for an operator: label, source variable, and object id.
    pub fn render(&self) -> String {
        // The PIV annotation is verified against the real ceremony hardware, not
        // guessed — see `pkcs11::piv_slot_for_cka_id`. An unknown id still prints
        // the raw value rather than nothing.
        let id = |i: &Option<String>| match i {
            Some(id) => match crate::crypto::signer::pkcs11_piv_slot(id) {
                Some(slot) => format!("CKA_ID={id} -> PIV {slot}"),
                None => format!("CKA_ID={id}"),
            },
            None => "CKA_ID=<not exposed by token>".to_string(),
        };
        format!(
            "  private: {private_var} = '{private_label}'  ({private_id})\n  \
             public:  {public_var} = '{public_label}'  ({public_id})",
            private_var = self.private_label_var,
            private_label = self.private_label,
            private_id = id(&self.private_id),
            public_var = self.public_label_var,
            public_label = self.public_label,
            public_id = id(&self.public_id),
        )
    }

    /// Whether the two halves demonstrably came from different objects.
    ///
    /// `Some(true)` is a FACT read off the token, not an inference — which is the
    /// difference between telling an operator "these point at different slots"
    /// and merely guessing that they might.
    pub fn ids_differ(&self) -> Option<bool> {
        match (&self.private_id, &self.public_id) {
            (Some(a), Some(b)) => Some(a != b),
            _ => None,
        }
    }
}

/// Fail-closed startup self-check: does the resolved PUBLIC key actually verify
/// a signature made by the resolved PRIVATE key? (#2405)
///
/// The failure this exists to catch is not exotic. `Pkcs11Config` resolves the
/// two labels independently, so a half-provisioned role can pair a private key
/// from one slot with a public key from another. Nothing downstream notices: the
/// node starts, signs block 1 with 9A, advertises the 9C key, and every peer
/// rejects the block. The node looks healthy the whole time, and the operator
/// finds out only after the launch window is open.
///
/// One signature at startup collapses that entire class into a startup refusal.
/// The check is cheap, runs once, and is the only thing that actually proves the
/// two halves belong together — comparing labels cannot, because the labels are
/// exactly what was misconfigured.
pub fn assert_keypair_consistency(
    signature: &Signature,
    public_key: &PublicKey,
    labels: &ResolvedKeypairLabels,
) -> Result<()> {
    if public_key.verify(KEYPAIR_SELFCHECK_PAYLOAD, signature) {
        return Ok(());
    }
    bail!(
        "FATAL: the {role} signing keypair is MISMATCHED — the private key and the \
         public key resolved from this token do not belong to the same pair.\n\n\
         A signature made with the private key does not verify under the public key, \
         so this node would sign blocks with one key while advertising another. It \
         would start cleanly, produce block 1, and have every peer reject it.\n\n\
         Resolved on token '{token}' (token slot {slot}):\n\
{halves}\n  \
         public key: {pubkey_hex}\n\n\
         {verdict}\n\n\
         Check both against the key ceremony record and set them to this role's own \
         pair (#2405).",
        role = labels.role,
        token = labels.token_label,
        slot = labels.slot,
        halves = labels.render(),
        pubkey_hex = public_key.to_hex(),
        verdict = match labels.ids_differ() {
            // Read off the token, so state it as fact.
            Some(true) => "The two objects have DIFFERENT CKA_IDs — they are different keys \
                           on the same token, which is exactly the confusable-slot \
                           misconfiguration this check exists to catch."
                .to_string(),
            // Same id but the signature still failed: not a slot mix-up. Say so,
            // rather than sending the operator to re-check labels that are right.
            Some(false) => "The two objects report the SAME CKA_ID, so this is not a \
                            slot mix-up — the token returned a public key that does not \
                            match its own private key. Suspect a corrupted or \
                            partially-regenerated key object."
                .to_string(),
            None => "This token does not expose CKA_ID, so the two objects cannot be \
                     distinguished here. Compare the labels above against the ceremony \
                     record by hand."
                .to_string(),
        },
    )
}

/// Identity of a configured signer, for logs and audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignerProvenance {
    /// Env-var-backed debug signer. The key material is in the operator's
    /// process environment — unsuitable for production.
    EnvKey { var_name: String },
    /// File-backed debug signer (#2290 P1-4). The key material is on disk, in a
    /// file whose mode was verified to be 0600 before it was read. Better than
    /// the environment, still not an HSM.
    KeyFile { path: String },
    /// PKCS#11-backed signer. The private key is resident in a hardware or
    /// software token and never exported.
    Pkcs11 {
        module_path: String,
        token_label: String,
        key_label: String,
    },
}

impl fmt::Display for SignerProvenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EnvKey { var_name } => write!(f, "EnvKeySigner({var_name})"),
            Self::KeyFile { path } => write!(f, "KeyFileSigner({path})"),
            Self::Pkcs11 {
                module_path,
                token_label,
                key_label,
            } => write!(
                f,
                "Pkcs11Signer(module={module_path}, token={token_label}, key={key_label})" // secret-leak-ok: provenance contains only a public module path and PKCS#11 token/object labels, never PIN or key bytes
            ),
        }
    }
}

/// Block-signing abstraction. Implementations MUST NOT leak private key
/// material outside their own module, and MUST be safe to share across
/// threads via `Arc`.
pub trait BlockSigner: Send + Sync + fmt::Debug {
    /// Sign a pre-hashed or raw payload. Ed25519 is a one-pass signature
    /// scheme, so callers pass the exact bytes to sign (typically the
    /// domain-separated block header hash).
    ///
    /// **Prefer [`sign_at_height`](BlockSigner::sign_at_height) on any path
    /// that knows the chain height.** A signer under an active rotation plan
    /// cannot decide whether its key is authoritative without a height, and
    /// refuses this heightless call rather than signing something it cannot
    /// vouch for (#2130).
    fn sign(&self, payload: &[u8]) -> Result<Signature>;

    /// Sign a payload that belongs to a specific chain height.
    ///
    /// The height is what makes validator key rotation enforceable *at the
    /// point of signing*: a signer mid-rotation compares its key against the
    /// plan's boundary and refuses when the key is not authoritative there.
    ///
    /// The default implementation ignores `height` and delegates to
    /// [`sign`](BlockSigner::sign), which is correct for every signer with no
    /// rotation concept (`EnvKeySigner`, `Pkcs11Signer`). Only
    /// [`FailClosedRemoteSigner`](crate::crypto::rotation::FailClosedRemoteSigner)
    /// overrides it.
    ///
    /// Callers that have a height MUST use this. The pairing is deliberate:
    /// heightless signing stays available for payloads that genuinely have no
    /// height, but a rotating signer turns that path into an error instead of
    /// a silent bypass — so a caller cannot lose rotation enforcement by
    /// forgetting which method to call.
    fn sign_at_height(&self, payload: &[u8], height: u64) -> Result<Signature> {
        let _ = height;
        self.sign(payload)
    }

    /// Return the signer's public key for inclusion in block headers and
    /// startup banners.
    fn public_key(&self) -> PublicKey;

    /// Provenance string for logs and audit trails. Impls must not include
    /// private material.
    fn provenance(&self) -> &SignerProvenance;
}

// --------- EnvKeySigner (debug only) -------------------------------------

/// Env-var-backed signer. Only available in debug builds; release builds
/// cannot instantiate this signer and must use [`Pkcs11Signer`] instead.
#[cfg(debug_assertions)]
pub struct EnvKeySigner {
    private_key: PrivateKey,
    public_key: PublicKey,
    provenance: SignerProvenance,
}

#[cfg(debug_assertions)]
impl fmt::Debug for EnvKeySigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never Debug-print the private key.
        f.debug_struct("EnvKeySigner")
            .field("provenance", &self.provenance)
            .field("public_key", &self.public_key.to_hex())
            .finish_non_exhaustive()
    }
}

#[cfg(debug_assertions)]
impl EnvKeySigner {
    /// Build from an already-loaded `PrivateKey`. Intended for tests.
    pub fn from_private_key(private_key: PrivateKey, var_name: impl Into<String>) -> Self {
        let public_key = PublicKey::from_private_key(&private_key);
        Self {
            private_key,
            public_key,
            provenance: SignerProvenance::EnvKey {
                var_name: var_name.into(),
            },
        }
    }

    /// Read hex from the given env var and build a signer. Trims whitespace
    /// and rejects empty strings.
    pub fn from_env(var_name: &str) -> Result<Self> {
        // determinism-ok: startup-only key configuration is copied into an owned signer; verification and state transitions never read this env var
        let raw = std::env::var(var_name).map_err(|_| anyhow!("{var_name} is not set"))?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("{var_name} is empty");
        }
        let private_key = PrivateKey::from_hex(trimmed)
            .map_err(|e| anyhow!("{var_name} is not a valid hex-encoded ed25519 key: {e}"))?;
        Ok(Self::from_private_key(private_key, var_name))
    }

    /// Read a hex-encoded ed25519 private key from a **file** (#2290 P1-4).
    ///
    /// The permission check is not advisory: a key file that group or other can
    /// read is a compromised key, so
    /// [`assert_key_file_permissions`](crate::security::key_file::assert_key_file_permissions)
    /// refuses it before the bytes are read — the same posture `ssh` takes
    /// toward a loose `id_ed25519`. Checking after reading would be theatre.
    pub fn from_key_file(path: &std::path::Path) -> Result<Self> {
        // "signing key file", not "producer key file": since #2464 the oracle
        // role loads through this same function, and a refusal that names the
        // wrong role sends the operator to the wrong variable.
        crate::security::key_file::assert_key_file_permissions(path)
            .map_err(|e| anyhow!("refusing to load signing key file: {e}"))?;
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("cannot read key file {}: {e}", path.display()))?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("key file {} is empty", path.display());
        }
        let private_key = PrivateKey::from_hex(trimmed).map_err(|e| {
            anyhow!(
                "key file {} does not contain a valid hex-encoded ed25519 key: {e}",
                path.display()
            )
        })?;
        let public_key = PublicKey::from_private_key(&private_key);
        Ok(Self {
            private_key,
            public_key,
            provenance: SignerProvenance::KeyFile {
                path: path.display().to_string(),
            },
        })
    }
}

#[cfg(debug_assertions)]
impl BlockSigner for EnvKeySigner {
    fn sign(&self, payload: &[u8]) -> Result<Signature> {
        Ok(self.private_key.sign(payload))
    }
    fn public_key(&self) -> PublicKey {
        self.public_key.clone()
    }
    fn provenance(&self) -> &SignerProvenance {
        &self.provenance
    }
}

// --------- Pkcs11Signer (feature-gated) ----------------------------------

#[cfg(feature = "hsm")]
mod pkcs11;

#[cfg(feature = "hsm")]
pub use pkcs11::{enumerate_public_keys, Pkcs11Config, Pkcs11Signer, TokenPublicKey};

// --------- Startup configuration -----------------------------------------

/// How the signer was resolved at startup. Returned alongside the signer so
/// callers can log the right banner and gate behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignerMode {
    /// Debug-build env-var signer. Expect a loud WARN banner.
    DebugEnvKey,
    /// PKCS#11 token (softhsm2 / YubiKey / CloudHSM).
    Hsm,
}

/// Outcome of [`load_block_signer`]. Holds the boxed trait object plus the
/// resolved mode so callers can render the correct startup banner.
#[derive(Debug)]
pub struct ResolvedSigner {
    pub signer: std::sync::Arc<dyn BlockSigner>,
    pub mode: SignerMode,
}

/// Load a block signer at startup, applying fail-closed semantics.
///
/// Resolution order:
///
/// 1. If any `WRFCOIN_HSM_*` env var is set and the `hsm` feature is
///    compiled in, build a [`Pkcs11Signer`].
/// 2. Else, on **debug** builds, fall back to [`EnvKeySigner`] reading
///    `var_name` (typically `WRFCOIN_SNAPSHOT_SIGNER_PRIVATE_KEY`).
/// 3. Else, on **release** builds, return `Ok(None)` so the caller can
///    `exit(1)` with a fail-closed error message.
///
/// The env-var signer path is *compiled out* of release binaries. A
/// release build with no HSM config has no way to produce blocks — which
/// is the point.
///
/// # Rotation wrapping (#2130)
///
/// Whatever this resolves, it is returned **already wrapped** in
/// [`FailClosedRemoteSigner`](crate::crypto::rotation::FailClosedRemoteSigner),
/// with any configured [`RotationPlan`](crate::crypto::rotation::RotationPlan)
/// attached. There is no supported way for a production caller to obtain the
/// raw inner signer.
///
/// That is deliberate, and it is the correction to the previous revision. The
/// wrapper existed but nothing constructed it, so rotation and revocation were
/// described rather than enforced. A wrapper that only works when someone
/// remembers to apply it is one more thing to forget — the same reasoning that
/// made #2232 read `SigningProtocolVersion::active()` instead of adding a
/// config field. Wrapping here means "unwrapped" is not a reachable state.
///
/// An invalid or partially-specified rotation configuration is an `Err`, never
/// a silent downgrade to unrotated signing.
pub fn load_block_signer(env_key_var: &str) -> Result<Option<ResolvedSigner>> {
    load_block_signer_for(SignerEnv::with_hex_var(env_key_var))
}

/// Whether a signing role may fall back to the shared [`env_vars::HSM_KEY_LABEL`]
/// when its own role-specific key label is unset.
///
/// Stated per role rather than inferred from the variable's name, so the policy
/// is visible at the call site instead of buried in a comparison here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedKeyFallback {
    /// The role may inherit the shared key object, with a loud warning (#2290).
    /// Correct for the snapshot signer (it *is* the shared key) and permitted
    /// for the producer, where some deployments genuinely have one slot.
    Allowed,
    /// The role must name its own key object; an unset label is a refusal.
    ///
    /// Required for the oracle role (#2464). The custody rule is that oracle
    /// key material is distinct from every other role's, and the shared label
    /// names the snapshot key object — so inheriting it does not merely couple
    /// two roles, it makes the oracle *be* another role. With the producer also
    /// falling back, producer and oracle become one credential, and an
    /// attestation would carry block-production authority into the oracle
    /// quorum. A warning cannot express that, because the resulting node still
    /// starts and still signs.
    Forbidden,
}

/// Which env vars a signing **role** resolves its credentials from (#2290 P1-4).
///
/// Roles exist because the snapshot signer and the block-producer signer are
/// different credentials with different blast radii, and previously they
/// resolved the *same* PKCS#11 key label — so an HSM deployment silently used
/// one key for both. Naming the vars per role makes "these are separate keys"
/// expressible instead of aspirational.
#[derive(Debug, Clone, Copy)]
pub struct SignerEnv<'a> {
    /// Debug-only hex-encoded private key var.
    pub hex_key_var: &'a str,
    /// Optional debug-only key-file path var, tried before the hex var.
    pub key_file_var: Option<&'a str>,
    /// Optional role-specific PKCS#11 key-label var. Falls back to the shared
    /// [`env_vars::HSM_KEY_LABEL`] (with a warning) when unset.
    pub hsm_key_label_var: Option<&'a str>,
    /// Optional role-specific PKCS#11 public-key-label var.
    pub hsm_pubkey_label_var: Option<&'a str>,
    /// Whether this role may inherit the shared [`env_vars::HSM_KEY_LABEL`]
    /// when its own label is unset (#2464).
    ///
    /// Carried on the role rather than inferred from the variable's name inside
    /// the PKCS#11 layer, so the custody policy is declared where the role is
    /// defined and cannot be lost by a rename.
    pub hsm_shared_fallback: SharedKeyFallback,
}

impl<'a> SignerEnv<'a> {
    /// A role with only the debug hex var — the pre-#2290 behaviour, and what
    /// [`load_block_signer`] uses so existing callers are unchanged.
    pub const fn with_hex_var(hex_key_var: &'a str) -> Self {
        Self {
            hex_key_var,
            key_file_var: None,
            hsm_key_label_var: None,
            hsm_pubkey_label_var: None,
            // The snapshot role IS the shared key; there is nothing to fall back from.
            hsm_shared_fallback: SharedKeyFallback::Allowed,
        }
    }

    /// The block-producer role (#2290): its own hex var, its own key file, and
    /// its own optional HSM key label.
    pub const fn producer() -> Self {
        Self {
            hex_key_var: env_vars::PRODUCER_SIGNER_PRIVATE_KEY,
            key_file_var: Some(env_vars::PRODUCER_SIGNER_KEY_FILE),
            hsm_key_label_var: Some(env_vars::HSM_PRODUCER_KEY_LABEL),
            hsm_pubkey_label_var: Some(env_vars::HSM_PRODUCER_PUBKEY_LABEL),
            // #2290 deliberately allows reuse with a loud warning: some
            // deployments genuinely have one slot, and the owner signs off.
            hsm_shared_fallback: SharedKeyFallback::Allowed,
        }
    }

    /// The oracle-attestation role (#2464): its own hex var, its own key file,
    /// and its own optional HSM key label.
    ///
    /// # Custody rule: an oracle key must not be a producer key
    ///
    /// The #2464 oracle-key decision rejects reusing either slot-9A producer
    /// keypair as an oracle key. Consensus would accept it — nothing compares
    /// `oracle_set` against `producer_set_history`, and the two signatures use
    /// different domains (`wrfcoin.oracle_attestation.v1` vs
    /// `wrfcoin.block_header.v1`), so neither can be replayed as the other. But
    /// domain separation only stops *replay*; it does not limit what the holder
    /// of one private key may deliberately sign. One compromised credential that
    /// both produces blocks and approves contribution rewards is the outcome the
    /// separation exists to prevent.
    ///
    /// # Where that rule lives, and where it does not
    ///
    /// It is **policy, expressed in configuration** — this role resolves its own
    /// env vars, so an operator who wants distinct keys can say so. It is not
    /// enforced here: a signer holds one credential and cannot see another
    /// role's.
    ///
    /// It is **declared** as a role invariant in
    /// [`crate::security::key_roles`], which names [`KeyRole::Oracle`] and
    /// [`KeyRole::Consensus`] as separate roles and whose
    /// [`assert_loaded_key_roles_distinct`] refuses a boot where two roles
    /// fingerprint to the same material.
    ///
    /// It is **not** enforced for the case this role is built for. That check
    /// inventories only the key material `launch_api` loads as bytes, so a
    /// non-exportable PKCS#11 oracle handle never reaches it, and today the
    /// inventory registers only `StorageEncryption` and the snapshot hex key.
    /// A PKCS#11 producer/oracle collision would therefore pass it. The #2464
    /// decision records that gap as work owed before the genesis cut — the cut
    /// gate must fail if either oracle key equals either producer key — and this
    /// role deliberately does not invent an enforcement point it cannot honour.
    ///
    /// [`KeyRole::Oracle`]: crate::security::key_roles::KeyRole::Oracle
    /// [`KeyRole::Consensus`]: crate::security::key_roles::KeyRole::Consensus
    /// [`assert_loaded_key_roles_distinct`]: crate::security::key_roles::assert_loaded_key_roles_distinct
    pub const fn oracle() -> Self {
        Self {
            hex_key_var: env_vars::ORACLE_SIGNER_PRIVATE_KEY,
            key_file_var: Some(env_vars::ORACLE_SIGNER_KEY_FILE),
            hsm_key_label_var: Some(env_vars::HSM_ORACLE_KEY_LABEL),
            hsm_pubkey_label_var: Some(env_vars::HSM_ORACLE_PUBKEY_LABEL),
            // #2464: the oracle key must be its own object. Inheriting the
            // shared label would make the oracle BE the snapshot role, and with
            // the producer also inheriting, producer and oracle become one
            // credential.
            hsm_shared_fallback: SharedKeyFallback::Forbidden,
        }
    }
}

/// Load the **oracle attestation** signer at startup (#2464).
///
/// [`load_block_signer`] for the [`SignerEnv::oracle`] role, and the entry point
/// an attester process should use. Resolution, fail-closed semantics and
/// rotation wrapping are identical to the producer path — deliberately so: this
/// is the same loader with a different set of variable names, not a second
/// implementation with its own idea of what "fail closed" means.
///
/// In particular the oracle role inherits, rather than re-states:
///
/// - **HSM-requested-but-unavailable is a refusal.** `WRFCOIN_HSM_MODULE_PATH`
///   set on a binary built without `--features hsm` errors instead of quietly
///   using a software key.
/// - **The #2405 confusable-pair rule.** Once [`env_vars::HSM_ORACLE_KEY_LABEL`]
///   is set, the shared [`env_vars::HSM_PUBKEY_LABEL`] is not consulted, so an
///   oracle private key can never be paired with the snapshot public key.
/// - **Rotation wrapping.** The returned signer is already wrapped in
///   [`FailClosedRemoteSigner`](crate::crypto::rotation::FailClosedRemoteSigner);
///   there is no supported way to obtain the unwrapped signer.
///
/// `Ok(None)` means nothing was configured. An attester MUST treat that as fatal
/// — an oracle that cannot sign cannot attest, and a node short of quorum pays
/// no contributor. That decision belongs to the caller, which is why it is
/// `None` here rather than an error, matching [`load_block_signer`].
///
/// See [`SignerEnv::oracle`] for the custody rule on oracle vs producer key
/// material.
pub fn load_oracle_signer() -> Result<Option<ResolvedSigner>> {
    load_block_signer_for(SignerEnv::oracle())
}

/// [`load_block_signer`] for an explicit [`SignerEnv`] role.
pub fn load_block_signer_for(env: SignerEnv<'_>) -> Result<Option<ResolvedSigner>> {
    let Some(raw) = resolve_raw_signer(env)? else {
        return Ok(None);
    };
    Ok(Some(finish_resolved_signer(raw)?))
}

/// Resolve the same production signing role with a PIN supplied in zeroizing
/// memory instead of an environment variable.
///
/// This is deliberately HSM-only and has no debug/file fallback. It exists for
/// interactive physical-key ceremonies where putting the PIN in env, argv or a
/// file is prohibited.
#[cfg(feature = "hsm")]
pub fn load_block_signer_for_with_pin(
    env: SignerEnv<'_>,
    pin: zeroize::Zeroizing<String>,
) -> Result<ResolvedSigner> {
    let raw = resolve_raw_hsm_signer_with_pin(env, pin)?;
    finish_resolved_signer(raw)
}

fn finish_resolved_signer(raw: ResolvedSigner) -> Result<ResolvedSigner> {
    let mut wrapper = crate::crypto::rotation::FailClosedRemoteSigner::new(raw.signer);

    if let Some(plan) = crate::crypto::rotation::RotationPlan::from_env()? {
        // #2290: the scope acknowledgement that used to gate this path is GONE.
        //
        // It required the operator to affirm `payload-signing-only` — that
        // rotation covered snapshot manifests and slashing reports but NOT block
        // production, "because a Block carries no producer signature, so peers
        // cannot reject a block produced under a revoked key". That was true
        // when #2130 shipped. It is false here: blocks now carry
        // `producer_pubkey`/`producer_sig`, `attest_block_producer` signs at
        // the block height, and a rotated-out key is refused *at signing time*.
        //
        // Demanding an acknowledgement of a false statement is worse than
        // demanding nothing. It trains operators to affirm text they should be
        // reading, and it would have them assert the opposite of what the
        // running code does. So the gate is removed rather than reworded: there
        // is no longer a scope limitation to acknowledge.
        //
        // Validate against the plan's own boundary rather than a live height:
        // `load_block_signer` runs at startup and has no chain handle. This
        // still enforces proof-of-possession, minimum overlap and key-changed,
        // which are the height-independent rules. The height-dependent rule —
        // which key is authoritative *now* — is enforced per-signature in
        // `sign_at_height`, which is where the height actually exists.
        plan.validate(plan.effective_height.saturating_sub(1))
            .map_err(|e| {
                anyhow!(
                    "validator key rotation is configured but the plan is invalid: {e}. \
                     Refusing to start rather than signing under an unvalidated rotation (#2130)."
                )
            })?;
        log::warn!(
            "[signer] validator key rotation ACTIVE — new key becomes authoritative at height {}, \
             overlap {} blocks. Heightless signing is refused until the rotation completes. \
             SCOPE: snapshot-sync manifests, slashing reports, AND block production (#2290) — \
             blocks carry producer_pubkey/producer_sig and are signed at their height, so a \
             rotated-out key is refused at signing time rather than reaching a block.",
            plan.effective_height,
            plan.overlap_blocks
        );
        wrapper = wrapper.with_rotation_plan(plan);
    }

    Ok(ResolvedSigner {
        signer: std::sync::Arc::new(wrapper),
        mode: raw.mode,
    })
}

#[cfg(feature = "hsm")]
fn resolve_raw_hsm_signer_with_pin(
    env: SignerEnv<'_>,
    pin: zeroize::Zeroizing<String>,
) -> Result<ResolvedSigner> {
    // determinism-ok: ceremony-startup signer selection only; the resolved signer is owned and consensus validation never reads this env var
    if std::env::var(env_vars::HSM_MODULE_PATH).is_err() {
        bail!(
            // secret-leak-ok: interpolates only the public environment-variable name, never a PIN or key value
            "{} is not set; interactive PIN injection is HSM-only and never falls back to a \
             software signing key",
            env_vars::HSM_MODULE_PATH
        );
    }
    let config = Pkcs11Config::from_env_for_role_without_pin(
        env.hsm_key_label_var,
        env.hsm_pubkey_label_var,
        env.hsm_shared_fallback,
    )?;
    let signer = Pkcs11Signer::new_with_pin(config, pin)?;
    Ok(ResolvedSigner {
        signer: std::sync::Arc::new(signer),
        mode: SignerMode::Hsm,
    })
}

/// Resolve the underlying signer, before rotation wrapping.
///
/// Private on purpose: everything outside this module goes through
/// [`load_block_signer`], so no caller can accidentally hold an unwrapped
/// signer and lose rotation enforcement.
fn resolve_raw_signer(env: SignerEnv<'_>) -> Result<Option<ResolvedSigner>> {
    let env_key_var = env.hex_key_var;
    // Priority 0 (#2130): HSM requested but unavailable is a REFUSAL, never a
    // downgrade.
    //
    // Without this, an operator who configures `WRFCOIN_HSM_*` on a binary
    // built without `--features hsm` gets the entire `#[cfg(feature = "hsm")]`
    // block compiled away, falls through to the debug env-key signer, and
    // silently signs blocks with a software key. The startup banner would even
    // say "DEBUG ENV KEY active" — but the operator asked for an HSM and got
    // software, which is precisely the silent fallback an HSM is bought to
    // prevent.
    //
    // Fail closed instead: if HSM configuration is present, an HSM signer is
    // the only acceptable outcome.
    // determinism-ok: startup-only signer selection; the resolved signer is owned and consensus validation never reads this env var
    let hsm_configured = std::env::var(env_vars::HSM_MODULE_PATH).is_ok();
    if hsm_configured {
        #[cfg(not(feature = "hsm"))]
        {
            anyhow::bail!(
                "{} is set but this binary was built without `--features hsm`. \
                 Refusing to fall back to a software signing key — that would \
                 silently downgrade an HSM-backed validator to a hot key. \
                 Rebuild with `--features hsm`, or unset the HSM configuration \
                 if software signing is genuinely intended (#2130).",
                env_vars::HSM_MODULE_PATH // secret-leak-ok: interpolates the public configuration variable name, not its value
            );
        }
    }

    // Priority 1: HSM config present.
    #[cfg(feature = "hsm")]
    if hsm_configured {
        let config =
            Pkcs11Config::from_env_for_role(
                env.hsm_key_label_var,
                env.hsm_pubkey_label_var,
                env.hsm_shared_fallback,
            )?;
        let signer = Pkcs11Signer::new(config)?;
        return Ok(Some(ResolvedSigner {
            signer: std::sync::Arc::new(signer),
            mode: SignerMode::Hsm,
        }));
    }

    // Priority 2: debug-only key FILE (#2290 P1-4). Ahead of the hex env var
    // because it is the strictly better of the two software paths — a file can
    // be mode 0600, a process environment cannot be anything.
    #[cfg(debug_assertions)]
    if let Some(file_var) = env.key_file_var {
        // determinism-ok: startup credential/config read, not a verifier, replay or state-transition input. It selects WHICH key material this node loads at boot; it never enters a preimage, a state transition or the state root, so two nodes with different values still compute identical roots (#2290, per #2414/#2416).
        if let Ok(path) = std::env::var(file_var) {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                let signer = EnvKeySigner::from_key_file(std::path::Path::new(trimmed))
                    .map_err(|e| anyhow!("{file_var}: {e}"))?;
                return Ok(Some(ResolvedSigner {
                    signer: std::sync::Arc::new(signer),
                    mode: SignerMode::DebugEnvKey,
                }));
            }
        }
    }

    // Priority 3: debug-only env-key fallback.
    #[cfg(debug_assertions)]
    {
        // determinism-ok: startup credential/config read, not a verifier, replay or state-transition input. Sibling of the priority-2 read above; additionally `#[cfg(debug_assertions)]`, so it is compiled out of release entirely. It selects WHICH key material this node loads; the value never enters a preimage, a state transition or the state root (#2414).
        if std::env::var(env_key_var).is_ok() {
            let signer = EnvKeySigner::from_env(env_key_var)?;
            return Ok(Some(ResolvedSigner {
                signer: std::sync::Arc::new(signer),
                mode: SignerMode::DebugEnvKey,
            }));
        }
    }

    // Priority 4: no path produced a signer — caller decides.
    let _ = env_key_var; // silence unused warning in release+no-hsm builds.
    let _ = &env;
    Ok(None)
}

/// Log a clear, loud startup banner describing which signer is active. Call
/// once, right after [`load_block_signer`] succeeds. Intentionally uses
/// `log::warn!` for the env path so operators never miss it in release-like
/// test environments.
pub fn log_signer_banner(resolved: &ResolvedSigner) {
    match resolved.mode {
        SignerMode::Hsm => {
            log::info!("[signer] HSM active — {}", resolved.signer.provenance());
        },
        SignerMode::DebugEnvKey => {
            log::warn!(
                "[signer] DEBUG ENV KEY active — {}. This path is compiled out of release builds. DO NOT run this binary in production.",
                resolved.signer.provenance()
            );
        },
    }
}

// --------- Tests ---------------------------------------------------------

/// Serialises every test in this module **tree** that mutates process
/// environment. `cargo test` runs test functions on multiple threads in one
/// process, so an unguarded `set_var`/`remove_var` pair races every other
/// env-reading test in this binary — a flaky security test gets muted, which is
/// worse than none.
///
/// One lock for the whole tree, not one per module (#2464). `signer::tests` and
/// `signer::pkcs11::tests` mutate an overlapping set of `WRFCOIN_HSM_*`
/// variables, so two independent locks serialised each module against itself
/// and neither against the other: under `--features hsm` a `pkcs11` test setting
/// `WRFCOIN_HSM_MODULE_PATH` could make a concurrent loader test in this module
/// attempt a real PKCS#11 open and fail. That is exactly the flake that gets a
/// loader test downgraded to `#[cfg(not(feature = "hsm"))]` and stops covering
/// the configuration it was written for.
#[cfg(test)]
pub(crate) fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::test_env_lock as env_lock;

    use super::*;

    // ------------------------------------------------------------------
    // #2290 P1-4 — the producer key-file path.
    // ------------------------------------------------------------------

    /// Write `contents` to a fresh temp file with mode `mode`.
    #[cfg(unix)]
    fn temp_key_file(name: &str, contents: &str, mode: u32) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(name);
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, contents).expect("write temp key file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .expect("chmod temp key file");
        path
    }

    /// The dev **file** signer path Elsa flagged as missing (#2290 P1-4): a
    /// 0600 key file loads, signs, and reports file provenance.
    #[cfg(unix)]
    #[test]
    fn producer_key_file_signer_loads_and_signs_2290_p1_4() {
        let private = PrivateKey::generate(&mut rand::thread_rng());
        let expected_pk = PublicKey::from_private_key(&private);
        let path = temp_key_file(
            "wrfcoin_producer_key_ok_2290.hex",
            &format!(
                // secret-leak-ok: this DOES format a real ed25519 private key, and that is the
                // system under test — `from_key_file` cannot be exercised without a key file to
                // load, so removing this would delete the #2290 P1-4 coverage rather than fix a
                // leak. The key is generated from `rand::thread_rng()` two lines above, lives only
                // in this `#[cfg(test)]` function, goes to a 0600 file under `temp_dir()` that the
                // test deletes, and is never a producer, genesis or ceremony key. The sink is
                // `std::fs::write`, not a log, metric, panic or stdout path.
                "{}\n",
                private.to_hex()
            ),
            0o600,
        );

        let signer = EnvKeySigner::from_key_file(&path).expect(
            "a mode-0600 key file holding a hex ed25519 key must load — this is the dev \
             file path the producer provisioning story requires (#2290 P1-4)",
        );
        assert_eq!(signer.public_key().to_hex(), expected_pk.to_hex());

        let payload = b"block preimage bytes";
        let sig = signer.sign(payload).expect("sign");
        assert!(signer.public_key().verify(payload, &sig));

        match signer.provenance() {
            SignerProvenance::KeyFile { path: p } => {
                assert_eq!(p, &path.display().to_string())
            },
            other => panic!("file-backed signer must report file provenance, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    /// A key file the group or world can read is a leaked key. Loading it
    /// anyway would make the file path *worse* than the env var it replaces,
    /// since it looks more secure. Refuse, and name the fix.
    #[cfg(unix)]
    #[test]
    fn producer_key_file_with_loose_permissions_is_refused_2290_p1_4() {
        let private = PrivateKey::generate(&mut rand::thread_rng());
        let path = temp_key_file(
            "wrfcoin_producer_key_loose_2290.hex",
            &private.to_hex(),
            0o644,
        );

        let err = EnvKeySigner::from_key_file(&path)
            .expect_err("a world-readable producer key file must be refused, not loaded");
        let msg = err.to_string();
        assert!(
            msg.contains("644") || msg.contains("chmod"),
            "the refusal must name the offending mode / the fix, got: {msg}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Garbage in the file is a configuration error, not a panic, and the
    /// message must identify the file.
    #[cfg(unix)]
    #[test]
    fn producer_key_file_with_invalid_contents_is_rejected_2290_p1_4() {
        let path = temp_key_file(
            "wrfcoin_producer_key_garbage_2290.hex",
            "not-a-hex-key",
            0o600,
        );
        let err = EnvKeySigner::from_key_file(&path).expect_err("garbage must not load");
        assert!(
            err.to_string().contains("valid hex-encoded ed25519 key"),
            "got: {err}"
        );
        let _ = std::fs::remove_file(&path);
    }

    // ------------------------------------------------------------------
    // #2405 — startup keypair self-check (Alice runbook gate 3).
    // ------------------------------------------------------------------

    /// The real key-ceremony producer public key, slot 9A.
    const CEREMONY_PRODUCER_PUBKEY_9A: &str =
        "1e5180e7add06bbe07cf2698ec9d4882ae1cfb54b13de67bae92a4e625ceec1a";

    fn n5_labels() -> ResolvedKeypairLabels {
        ResolvedKeypairLabels {
            role: "producer".to_string(),
            slot: "9A".to_string(),
            token_label: "wrfcoin-producer".to_string(),
            private_label: "Private key for PIV Authentication".to_string(),
            private_label_var: env_vars::HSM_PRODUCER_KEY_LABEL.to_string(),
            // Distinct per-object ids: the producer key (CKA_ID 01, PIV 9A) and
            // the snapshot key (CKA_ID 02, PIV 9C) are different objects on one
            // token, matching the measured N5 hardware. This is what the token
            // actually reports, and what makes the diagnostic a fact rather
            // than a guess.
            private_id: Some("01".to_string()),
            public_label: "Public key for Digital Signature".to_string(),
            public_label_var: env_vars::HSM_PRODUCER_PUBKEY_LABEL.to_string(),
            public_id: Some("02".to_string()),
        }
    }

    /// **The N5 confusable-pair trap, as a signature check.**
    ///
    /// Private key from slot 9A, public key from slot 9C (the snapshot signer's).
    /// Label comparison cannot catch this — the labels are exactly what was
    /// misconfigured — but one signature does.
    #[test]
    fn a_mismatched_keypair_is_refused_at_startup_2405() {
        let private = PrivateKey::generate(&mut rand::thread_rng());
        let signature = private.sign(KEYPAIR_SELFCHECK_PAYLOAD);

        // The public half is a DIFFERENT key: the real ceremony 9A public key,
        // which this randomly generated private key does not correspond to.
        let other_public = PublicKey::from_hex(CEREMONY_PRODUCER_PUBKEY_9A)
            .expect("ceremony public key is valid hex");

        let err = assert_keypair_consistency(&signature, &other_public, &n5_labels())
            .expect_err(
                "a private key from one slot paired with a public key from another must \
                 ABORT STARTUP — this pair signs block 1 and every peer rejects it (#2405)",
            );
        let msg = err.to_string();

        // The error must name BOTH label variables and the slot, because that is
        // the operator's actual next action.
        assert!(
            msg.contains(env_vars::HSM_PRODUCER_KEY_LABEL),
            "must name the private label var, got: {msg}"
        );
        assert!(
            msg.contains(env_vars::HSM_PRODUCER_PUBKEY_LABEL),
            "must name the public label var, got: {msg}"
        );
        assert!(msg.contains("9A"), "must name the slot, got: {msg}");
        assert!(
            msg.contains("Private key for PIV Authentication")
                && msg.contains("Public key for Digital Signature"),
            "must show both resolved labels so the confusion is visible, got: {msg}"
        );
        assert!(
            msg.contains(CEREMONY_PRODUCER_PUBKEY_9A),
            "must show the public key actually resolved, got: {msg}"
        );
    }

    /// #2405 (Frank round 1, blocker 1): the mismatch error must report **both
    /// objects' CKA_IDs**, not just the shared token slot.
    ///
    /// The token-level slot handle is identical for every object on the token,
    /// so it cannot distinguish a private key in PIV 9A from a public key in
    /// 9C — the exact confusion this check exists to catch. Frank's SoftHSM
    /// probe showed the old error printing one `Slot { slot_id: … }` and then
    /// *speculating* that the labels "almost certainly point at different
    /// slots". The per-object id turns that guess into a read fact.
    #[test]
    fn the_mismatch_error_reports_both_object_ids_2405() {
        let private = PrivateKey::generate(&mut rand::thread_rng());
        let signature = private.sign(KEYPAIR_SELFCHECK_PAYLOAD);
        let other_public = PublicKey::from_hex(CEREMONY_PRODUCER_PUBKEY_9A).unwrap();

        let labels = n5_labels();
        assert_eq!(labels.ids_differ(), Some(true), "test premise: distinct objects");

        let msg = assert_keypair_consistency(&signature, &other_public, &labels)
            .expect_err("mismatched pair must abort")
            .to_string();

        assert!(msg.contains("CKA_ID=01"), "must report the PRIVATE object id, got: {msg}");
        assert!(msg.contains("CKA_ID=02"), "must report the PUBLIC object id, got: {msg}");
        assert!(
            msg.contains("DIFFERENT CKA_IDs"),
            "with both ids known the error must state the divergence as fact, not \
             speculate that the labels 'almost certainly' differ, got: {msg}"
        );
    }

    /// Same id but a failing signature is NOT a slot mix-up, and must not be
    /// reported as one — that would send the operator to re-check labels that
    /// are correct.
    #[test]
    fn same_object_id_is_not_reported_as_a_slot_mixup_2405() {
        let private = PrivateKey::generate(&mut rand::thread_rng());
        let signature = private.sign(KEYPAIR_SELFCHECK_PAYLOAD);
        let other_public = PublicKey::from_hex(CEREMONY_PRODUCER_PUBKEY_9A).unwrap();

        let mut labels = n5_labels();
        labels.public_id = labels.private_id.clone();
        assert_eq!(labels.ids_differ(), Some(false));

        let msg = assert_keypair_consistency(&signature, &other_public, &labels)
            .expect_err("a failing pair must still abort")
            .to_string();
        assert!(
            msg.contains("not a slot mix-up"),
            "must distinguish a corrupted key object from a slot confusion, got: {msg}"
        );
    }

    /// A token that does not expose CKA_ID must still start (when the pair is
    /// good) and must say plainly that it could not distinguish the objects —
    /// degrading the diagnostic, never the security check.
    #[test]
    fn a_token_without_cka_id_degrades_the_diagnostic_only_2405() {
        let private = PrivateKey::generate(&mut rand::thread_rng());
        let signature = private.sign(KEYPAIR_SELFCHECK_PAYLOAD);
        let other_public = PublicKey::from_hex(CEREMONY_PRODUCER_PUBKEY_9A).unwrap();

        let mut labels = n5_labels();
        labels.private_id = None;
        labels.public_id = None;
        assert_eq!(labels.ids_differ(), None);

        let msg = assert_keypair_consistency(&signature, &other_public, &labels)
            .expect_err("the security check must still fire without CKA_ID")
            .to_string();
        assert!(
            msg.contains("does not expose CKA_ID"),
            "must say why the objects could not be distinguished, got: {msg}"
        );
        assert!(msg.contains("not exposed by token"), "got: {msg}");

        // and a MATCHING pair still starts on such a token.
        let good = PublicKey::from_private_key(&private);
        assert_keypair_consistency(&signature, &good, &labels)
            .expect("a good pair must start even when CKA_ID is unavailable");
    }

    /// The matching pair must start. A check that cannot pass is a check that
    /// gets deleted the first time it blocks a launch.
    #[test]
    fn a_matched_keypair_starts_2405() {
        let private = PrivateKey::generate(&mut rand::thread_rng());
        let public = PublicKey::from_private_key(&private);
        let signature = private.sign(KEYPAIR_SELFCHECK_PAYLOAD);
        assert_keypair_consistency(&signature, &public, &n5_labels())
            .expect("a correctly provisioned pair must start");
    }

    /// A signature over the WRONG payload must not satisfy the check either —
    /// otherwise a signer that returns a stale or replayed signature would pass.
    #[test]
    fn a_signature_over_a_different_payload_does_not_satisfy_the_check_2405() {
        let private = PrivateKey::generate(&mut rand::thread_rng());
        let public = PublicKey::from_private_key(&private);
        let wrong = private.sign(b"some other message");
        assert!(
            assert_keypair_consistency(&wrong, &public, &n5_labels()).is_err(),
            "the check must be bound to its own fixed payload" // secret-leak-ok: fixed unit-test assertion text; neither private key nor signature is formatted
        );
    }

    /// The self-check payload must never be a valid encoding of something the
    /// node would otherwise sign — it is signed with the production key at every
    /// startup, so a collision would make startup a signing oracle.
    #[test]
    fn the_selfcheck_payload_is_domain_separated_2405() {
        let payload = KEYPAIR_SELFCHECK_PAYLOAD;
        assert!(
            payload.starts_with(b"wrfcoin.signer.keypair-selfcheck"),
            "payload must be domain-tagged"
        );
        for foreign in [
            b"wrfcoin.block_header.v1".as_slice(),
            b"wrfcoin.snapshot".as_slice(),
            b"wrfcoin.slashing".as_slice(),
        ] {
            assert!(
                !payload.starts_with(foreign),
                "the self-check payload must not collide with a real signing domain"
            );
        }
    }

    // #2464 — the oracle attestation signing role.
    // ------------------------------------------------------------------

    /// Clear every variable either signing role resolves from, so a test
    /// observes only what it sets. Callers must hold [`env_lock`].
    ///
    /// This includes `HSM_MODULE_PATH`: with it set, `resolve_raw_signer` takes
    /// the PKCS#11 branch and never reaches the software paths under test.
    fn clear_role_signer_env() {
        // SAFETY: serialized by the module-tree `env_lock()` held by the caller,
        // so no other thread reads or writes the process environment here.
        unsafe {
            for name in [
                env_vars::HSM_MODULE_PATH,
                env_vars::ORACLE_SIGNER_PRIVATE_KEY,
                env_vars::ORACLE_SIGNER_KEY_FILE,
                env_vars::PRODUCER_SIGNER_PRIVATE_KEY,
                env_vars::PRODUCER_SIGNER_KEY_FILE,
            ] {
                std::env::remove_var(name);
            }
        }
    }

    // ------------------------------------------------------------------
    // #2464 — the `"oracle"` arm of the resolved-keypair role name.
    // ------------------------------------------------------------------

    /// The gap this closes: #2458 shipped `ResolvedKeypairLabels.role` mapping
    /// only the producer variable to `"producer"` and **everything else** to
    /// `"signer"`, so an oracle key reported itself as a generic signer.
    ///
    /// These run in both the `hsm` and non-`hsm` builds because
    /// `role_for_key_label_var` is deliberately not behind the feature gate —
    /// the mapping is a plain function of a variable name and gains nothing
    /// from requiring a token to observe.
    #[test]
    fn the_oracle_key_label_var_renders_as_the_oracle_role_2464() {
        assert_eq!(
            role_for_key_label_var(Some(env_vars::HSM_ORACLE_KEY_LABEL)),
            "oracle",
            "an oracle key must name its own role; rendering it as the generic \"signer\" \
             points an operator at the wrong variable and makes the #2464 producer/oracle \
             custody split unauditable at runtime"
        );
    }

    #[test]
    fn the_producer_arm_is_unchanged_by_the_oracle_arm_2464() {
        // Guards match ordering: a misplaced oracle arm could shadow producer's.
        assert_eq!(
            role_for_key_label_var(Some(env_vars::HSM_PRODUCER_KEY_LABEL)),
            "producer"
        );
    }

    #[test]
    fn the_shared_key_label_still_renders_as_a_generic_signer_2464() {
        // The shared path has no role of its own, and inventing one would claim
        // a custody separation that was never configured.
        assert_eq!(role_for_key_label_var(None), "signer");
        assert_eq!(role_for_key_label_var(Some(env_vars::HSM_KEY_LABEL)), "signer");
        assert_eq!(role_for_key_label_var(Some("WRFCOIN_SOMETHING_ELSE")), "signer");
    }

    /// Every role must render a *distinct* name, or the diagnostic cannot tell
    /// two roles apart — which is the failure being fixed, not a stricter
    /// version of it.
    #[test]
    fn the_three_roles_render_distinct_names_2464() {
        let producer = role_for_key_label_var(Some(env_vars::HSM_PRODUCER_KEY_LABEL));
        let oracle = role_for_key_label_var(Some(env_vars::HSM_ORACLE_KEY_LABEL));
        let shared = role_for_key_label_var(None);

        assert_ne!(producer, oracle, "producer and oracle must be distinguishable");
        assert_ne!(oracle, shared, "oracle must not fall back to the generic name");
        assert_ne!(producer, shared);
    }

    /// The role name comes from the **variable**, never the label's value. An
    /// operator may name their oracle object anything, and a label that merely
    /// looks producer-ish must not steal the role.
    #[test]
    fn the_role_comes_from_the_variable_not_the_label_value_2464() {
        assert_eq!(role_for_key_label_var(Some("producer")), "signer");
        assert_eq!(role_for_key_label_var(Some("oracle")), "signer");
    }

    /// The oracle role must resolve **its own four variables** — none shared
    /// with the producer role (#2464).
    ///
    /// This is the whole point of the role rather than a detail of it. The
    /// #2464 oracle-key decision requires oracle and producer key material to be
    /// distinct, and a role whose variables overlapped the producer's could not
    /// express that separation at all: an operator would have no way to name a
    /// different key, whatever the runbook said.
    #[test]
    fn the_oracle_role_resolves_its_own_variables_2464() {
        let oracle = SignerEnv::oracle();
        let producer = SignerEnv::producer();

        assert_eq!(oracle.hex_key_var, env_vars::ORACLE_SIGNER_PRIVATE_KEY); // secret-leak-ok: compares env-var NAMES, never their values
        assert_eq!(
            oracle.key_file_var,
            Some(env_vars::ORACLE_SIGNER_KEY_FILE),
            "the oracle role must offer the permission-checked key-file path too — \
             it is the strictly better of the two software paths (#2290 P1-4)"
        );
        assert_eq!(oracle.hsm_key_label_var, Some(env_vars::HSM_ORACLE_KEY_LABEL));
        assert_eq!(
            oracle.hsm_pubkey_label_var,
            Some(env_vars::HSM_ORACLE_PUBKEY_LABEL)
        );

        // Not one variable in common with the producer.
        assert_ne!(oracle.hex_key_var, producer.hex_key_var);
        assert_ne!(oracle.key_file_var, producer.key_file_var);
        assert_ne!(
            oracle.hsm_key_label_var, producer.hsm_key_label_var,
            "an oracle key that resolved the producer's HSM label would BE the \
             producer key — the reuse the #2464 decision rejects, made unavoidable \
             by configuration rather than merely permitted"
        );
        assert_ne!(oracle.hsm_pubkey_label_var, producer.hsm_pubkey_label_var);
    }

    /// `load_oracle_signer` must load the key named by the ORACLE variable, and
    /// must not fall through to the producer's (#2464).
    ///
    /// A loader that read the producer variable would silently make the two
    /// roles one credential: the node would attest with the key it produces
    /// blocks with, and every artefact would look correct while the custody
    /// separation the decision requires had quietly evaporated.
    #[test]
    fn load_oracle_signer_never_falls_through_to_the_producer_key_2464() {
        use rand::SeedableRng;

        let _guard = env_lock();
        clear_role_signer_env();

        let mut rng = rand::rngs::StdRng::seed_from_u64(2_464_001);
        let oracle_key = PrivateKey::generate(&mut rng);
        let producer_key = PrivateKey::generate(&mut rng);
        let oracle_public = PublicKey::from_private_key(&oracle_key);
        let producer_public = PublicKey::from_private_key(&producer_key);

        // Both roles configured, with DIFFERENT keys — the arrangement #2464
        // mandates.
        // SAFETY: serialized by the module-tree `env_lock()` held above.
        unsafe {
            std::env::set_var(env_vars::ORACLE_SIGNER_PRIVATE_KEY, oracle_key.to_hex());
            std::env::set_var(
                env_vars::PRODUCER_SIGNER_PRIVATE_KEY,
                producer_key.to_hex(),
            );
        }

        let resolved = load_oracle_signer()
            .expect("an oracle hex key is a valid configuration")
            .expect("a signer must be resolved when the oracle key is set");
        clear_role_signer_env();

        assert_eq!(
            // secret-leak-ok: compares PUBLIC keys; the message interpolates an env-var NAME
            resolved.signer.public_key(),
            oracle_public,
            "the oracle signer must hold the key named by {}, not the producer's",
            env_vars::ORACLE_SIGNER_PRIVATE_KEY
        );
        assert_ne!(resolved.signer.public_key(), producer_public);
        assert_eq!(resolved.mode, SignerMode::DebugEnvKey);

        // And it actually signs: a resolved-but-unusable signer would let an
        // attester start and then fail at the first attestation.
        let payload = b"wrfcoin.oracle_attestation.v1 preimage";
        let signature = resolved
            .signer
            .sign(payload)
            .expect("the resolved oracle signer must sign");
        assert!(oracle_public.verify(payload, &signature));
        assert!(!producer_public.verify(payload, &signature));
    }

    /// The oracle role gets the permission-checked **key file** path, and it
    /// takes priority over the hex env var — same ordering as the producer
    /// (#2464, #2290 P1-4).
    #[cfg(unix)]
    #[test]
    fn load_oracle_signer_prefers_the_key_file_over_the_hex_var_2464() {
        use rand::SeedableRng;

        let _guard = env_lock();
        clear_role_signer_env();

        let mut rng = rand::rngs::StdRng::seed_from_u64(2_464_002);
        let file_key = PrivateKey::generate(&mut rng);
        let hex_key = PrivateKey::generate(&mut rng);
        let file_public = PublicKey::from_private_key(&file_key);

        let path = temp_key_file(
            "wrfcoin_oracle_key_ok_2464.hex",
            // secret-leak-ok: this DOES format a real ed25519 private key, and that is the
            // system under test — the oracle key-file path cannot be exercised without a key
            // file to load. The key is generated from a seeded StdRng in this `#[cfg(test)]`
            // function, goes to a 0600 file under `temp_dir()` that the test deletes, and is
            // never an oracle, producer, genesis or ceremony key. The sink is `std::fs::write`,
            // not a log, metric, panic or stdout path.
            &format!("{}\n", file_key.to_hex()),
            0o600,
        );

        // SAFETY: serialized by the module-tree `env_lock()` held above.
        unsafe {
            std::env::set_var(env_vars::ORACLE_SIGNER_KEY_FILE, path.display().to_string());
            std::env::set_var(env_vars::ORACLE_SIGNER_PRIVATE_KEY, hex_key.to_hex());
        }

        let resolved = load_oracle_signer()
            .expect("a 0600 oracle key file is a valid configuration")
            .expect("a signer must be resolved from the oracle key file");
        clear_role_signer_env();
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            resolved.signer.public_key(),
            file_public,
            "the key FILE must win over the hex env var — a file can be mode 0600, a \
             process environment cannot be anything (#2290 P1-4)"
        );
        match resolved.signer.provenance() {
            SignerProvenance::KeyFile { path: p } => {
                assert_eq!(p, &path.display().to_string())
            },
            other => panic!("file-backed oracle signer must report file provenance, got {other:?}"),
        }
    }

    /// A world-readable oracle key file is a leaked oracle key, and must be
    /// refused rather than loaded (#2464).
    ///
    /// The refusal must not name the *producer*: this message is the operator's
    /// only pointer to which variable to fix, and sending an attester operator
    /// to the producer's key file is worse than saying nothing.
    #[cfg(unix)]
    #[test]
    fn an_oracle_key_file_with_loose_permissions_is_refused_2464() {
        let _guard = env_lock();
        clear_role_signer_env();

        let private = PrivateKey::generate(&mut rand::thread_rng());
        let path = temp_key_file("wrfcoin_oracle_key_loose_2464.hex", &private.to_hex(), 0o644);

        // SAFETY: serialized by the module-tree `env_lock()` held above.
        unsafe {
            std::env::set_var(env_vars::ORACLE_SIGNER_KEY_FILE, path.display().to_string());
        }
        let result = load_oracle_signer();
        clear_role_signer_env();
        let _ = std::fs::remove_file(&path);

        let msg = match result {
            Err(e) => e.to_string(),
            Ok(_) => panic!(
                "a world-readable oracle key file must be refused, not loaded — the file \
                 path is only an improvement on the env var while its mode is enforced"
            ),
        };
        assert!(
            msg.contains(env_vars::ORACLE_SIGNER_KEY_FILE),
            "the refusal must name the oracle variable the operator has to fix, got: {msg}"
        );
        assert!(
            !msg.contains("producer"),
            "the refusal must not misattribute an oracle key file to the producer role, \
             got: {msg}"
        );
    }

    /// Nothing configured is `Ok(None)`, not a signer and not an error (#2464).
    ///
    /// The caller decides what absence means — for an attester it is fatal,
    /// because an oracle that cannot sign cannot attest — but that policy lives
    /// at the call site, exactly as it does for the producer.
    #[test]
    fn load_oracle_signer_returns_none_when_nothing_is_configured_2464() {
        let _guard = env_lock();
        clear_role_signer_env();

        let resolved = load_oracle_signer().expect("an unconfigured oracle role is not an error");
        assert!(
            resolved.is_none(),
            "expected None when neither the HSM nor either software oracle path is configured"
        );
    }

    #[test]
    fn env_signer_roundtrip() {
        // Generate a fresh key, set env, load signer, sign+verify.
        let private = PrivateKey::generate(&mut rand::thread_rng());
        let expected_pk = PublicKey::from_private_key(&private);
        let var = "WRFCOIN_TEST_ENV_SIGNER_ROUNDTRIP";
        // SAFETY: test is single-threaded within the binary via `cargo test -- --test-threads=1`
        // for env-mutating tests; if parallelism is an issue we can switch to `temp_env`.
        // Using a uniquely-named var reduces collision risk.
        unsafe {
            std::env::set_var(var, private.to_hex());
        }
        let signer = EnvKeySigner::from_env(var).expect("env signer loads");
        unsafe {
            std::env::remove_var(var);
        }
        assert_eq!(signer.public_key().to_hex(), expected_pk.to_hex());

        let payload = b"block header bytes for test";
        let sig = signer.sign(payload).expect("sign");
        assert!(
            signer.public_key().verify(payload, &sig),
            "signature must verify against its public key"
        );

        match signer.provenance() {
            SignerProvenance::EnvKey { var_name } => assert_eq!(var_name, var),
            other => panic!("unexpected provenance: {other:?}"),
        }
    }

    #[test]
    fn env_change_after_resolution_does_not_change_signer() {
        use rand::SeedableRng;

        let _guard = env_lock();
        let var = "WRFCOIN_TEST_ENV_SIGNER_RESOLUTION_2416";
        let mut rng = rand::rngs::StdRng::seed_from_u64(2416);
        let key_a = PrivateKey::generate(&mut rng);
        let key_b = PrivateKey::generate(&mut rng);
        let public_a = PublicKey::from_private_key(&key_a);
        let public_b = PublicKey::from_private_key(&key_b);

        unsafe {
            std::env::set_var(var, key_a.to_hex());
        }
        let signer = EnvKeySigner::from_env(var).expect("env signer resolves key A once");
        unsafe {
            std::env::set_var(var, key_b.to_hex());
        }

        let payload = b"wrfcoin.signer.env-resolution.v1";
        let signature = signer.sign(payload).expect("resolved signer signs");
        assert_eq!(signer.public_key(), public_a);
        assert!(public_a.verify(payload, &signature));
        assert!(!public_b.verify(payload, &signature));

        unsafe {
            std::env::remove_var(var);
        }
    }

    #[test]
    fn env_signer_rejects_empty() {
        let var = "WRFCOIN_TEST_ENV_SIGNER_EMPTY";
        unsafe {
            std::env::set_var(var, "   ");
        }
        let err = EnvKeySigner::from_env(var).unwrap_err();
        unsafe {
            std::env::remove_var(var);
        }
        assert!(err.to_string().contains("empty"), "error was: {err}");
    }

    #[test]
    fn env_signer_rejects_bad_hex() {
        let var = "WRFCOIN_TEST_ENV_SIGNER_BAD";
        unsafe {
            std::env::set_var(var, "not-hex");
        }
        let err = EnvKeySigner::from_env(var).unwrap_err();
        unsafe {
            std::env::remove_var(var);
        }
        assert!(err.to_string().contains("hex"), "error was: {err}");
    }

    /// #2130: HSM configured but unavailable must REFUSE, never downgrade.
    ///
    /// This is the real production call site, not a wrapper around a fake. On a
    /// binary built without `--features hsm`, setting `WRFCOIN_HSM_MODULE_PATH`
    /// previously compiled the HSM branch away and fell through to the debug
    /// env-key signer — an operator asks for an HSM and silently gets software
    /// signing, which is the exact failure an HSM is bought to prevent.
    #[cfg(not(feature = "hsm"))]
    #[test]
    fn hsm_configured_without_the_feature_refuses_rather_than_downgrading() {
        // Guarded: this test mutates process env, so it must not run alongside
        // the other env-reading signer tests.
        let _guard = env_lock();
        unsafe {
            std::env::set_var(env_vars::HSM_MODULE_PATH, "/usr/lib/softhsm/libsofthsm2.so");
            // A debug env key is ALSO available — this is the trap. Without the
            // gate, resolution silently prefers it.
            std::env::set_var("WRFCOIN_TEST_FALLBACK_KEY", "11".repeat(32));
        }

        let result = load_block_signer("WRFCOIN_TEST_FALLBACK_KEY");

        unsafe {
            std::env::remove_var(env_vars::HSM_MODULE_PATH);
            std::env::remove_var("WRFCOIN_TEST_FALLBACK_KEY");
        }

        // NOTE: `.expect_err()` needs `Debug` on the Ok type and `ResolvedSigner`
        // deliberately has none (it holds a signer). Match instead.
        let rendered = match result {
            Err(e) => e.to_string(),
            Ok(_) => panic!(
                "HSM configured on a non-hsm build must refuse, not fall back to a \
                 software signing key"
            ),
        };
        assert!(
            rendered.contains("without `--features hsm`"),
            "the refusal must name the cause, got: {rendered}"
        );
        assert!(
            rendered.contains("silently downgrade"),
            "the refusal should say what it is preventing, got: {rendered}"
        );
    }

    /// Set up a structurally valid rotation in the process env, so the only
    /// thing under test is the scope acknowledgement.
    ///
    /// Returns a closure that clears everything it set.
    fn with_valid_rotation_env() -> impl FnOnce() {
        use crate::crypto::rotation::rotation_pop_message;
        use crate::crypto::signature::{PrivateKey, PublicKey};
        use rand::SeedableRng;

        let mut rng = rand::rngs::StdRng::seed_from_u64(2_130_777);
        let old_sk = PrivateKey::generate(&mut rng);
        let new_sk = PrivateKey::generate(&mut rng);
        let old = PublicKey::from_private_key(&old_sk);
        let new = PublicKey::from_private_key(&new_sk);
        let effective_height = 1_000u64;
        let proof = new_sk.sign(&rotation_pop_message(&old, &new, effective_height));

        // `std::env::set_var` is unsound only when another thread reads or writes
        // the environment concurrently. Every caller of this helper holds
        // `env_lock()` for the whole of its test, and every other test in this
        // module that touches these variables takes the same lock.
        // SAFETY: serialized by the module-wide `env_lock()` held by the caller,
        // so no other thread reads or writes the process environment here.
        unsafe {
            std::env::set_var(env_vars::ROTATION_OLD_PUBKEY, hex::encode(old.as_bytes()));
            std::env::set_var(env_vars::ROTATION_NEW_PUBKEY, hex::encode(new.as_bytes()));
            std::env::set_var(
                env_vars::ROTATION_EFFECTIVE_HEIGHT,
                effective_height.to_string(),
            );
            std::env::set_var(env_vars::ROTATION_OVERLAP_BLOCKS, "16");
            std::env::set_var(env_vars::ROTATION_POP, proof.to_hex());
            std::env::set_var("WRFCOIN_TEST_ROTATION_KEY", "22".repeat(32));
        }

        // The returned cleanup closure is invoked by the caller while it still
        // holds `env_lock()`. Callers must not defer it past the guard's lifetime.
        // SAFETY: same invariant as the set block above — serialized by the
        // module-wide `env_lock()` still held by the caller at invocation.
        || unsafe {
            std::env::remove_var(env_vars::ROTATION_OLD_PUBKEY);
            std::env::remove_var(env_vars::ROTATION_NEW_PUBKEY);
            std::env::remove_var(env_vars::ROTATION_EFFECTIVE_HEIGHT);
            std::env::remove_var(env_vars::ROTATION_OVERLAP_BLOCKS);
            std::env::remove_var(env_vars::ROTATION_POP);
            std::env::remove_var("WRFCOIN_ROTATION_SCOPE_ACK"); // removed in #2290; cleared anyway
            std::env::remove_var("WRFCOIN_TEST_ROTATION_KEY");
        }
    }

    /// #2290 blocker 3 (Elsa round 2): **a rotation must start without the old
    /// scope acknowledgement.**
    ///
    /// `WRFCOIN_ROTATION_SCOPE_ACK=payload-signing-only` used to be mandatory.
    /// It made the operator affirm that rotation did not cover block production,
    /// "because a Block carries no producer signature". At this head that is
    /// false: blocks carry `producer_pubkey`/`producer_sig` and
    /// `attest_block_producer` signs at the block height. A startup gate that
    /// demands assent to a false statement trains operators to affirm text they
    /// should be reading, so it was deleted rather than reworded.
    ///
    /// RED before the fix: `load_block_signer` returned `Err` naming the
    /// acknowledgement variable.
    #[cfg(not(feature = "hsm"))]
    #[test]
    fn rotation_starts_without_the_removed_scope_acknowledgement_2290_b3() {
        let _guard = env_lock();
        let cleanup = with_valid_rotation_env();
        // Explicitly ensure the obsolete variable is absent.
        unsafe {
            std::env::remove_var("WRFCOIN_ROTATION_SCOPE_ACK");
        }

        let result = load_block_signer("WRFCOIN_TEST_ROTATION_KEY");
        cleanup();

        match result {
            Ok(Some(_)) => {},
            Ok(None) => panic!("expected a resolved signer with rotation configured"),
            Err(e) => panic!(
                "a valid rotation must start without any scope acknowledgement — the \
                 acknowledgement asserted that block production is uncovered, which is \
                 false at this head (#2290 blocker 3). Got: {e}"
            ),
        }
    }

    /// The obsolete variable must be genuinely **inert**, not quietly re-honoured.
    ///
    /// Leaving a dead gate that still reacts to its old input is how a removed
    /// requirement comes back as a mystery startup failure. Whatever an operator
    /// left in their unit file — the old blessed value, a typo, or junk — must
    /// have no effect on whether the rotation starts.
    #[cfg(not(feature = "hsm"))]
    #[test]
    fn the_removed_scope_acknowledgement_variable_is_inert_2290_b3() {
        // Held across the WHOLE loop, not re-acquired per iteration: releasing it
        // between iterations would leave the ROTATION_* vars visible to other
        // tests in the gap, which is the race #2400 closed. (This scoping was
        // lost when the rebase onto #2398 skipped the commit whose sibling hunks
        // had already landed via #2402 — restored here.)
        let _guard = env_lock();
        for leftover in ["payload-signing-only", "1", "nonsense"] {
            let cleanup = with_valid_rotation_env();
            // SAFETY: serialized by the module-wide `env_lock()` held above for
            // the duration of this loop, so no other thread touches the env.
            unsafe {
                std::env::set_var("WRFCOIN_ROTATION_SCOPE_ACK", leftover);
            }

            let result = load_block_signer("WRFCOIN_TEST_ROTATION_KEY");
            // SAFETY: same held `env_lock()` as above.
            unsafe {
                std::env::remove_var("WRFCOIN_ROTATION_SCOPE_ACK");
            }
            cleanup();

            assert!(
                matches!(result, Ok(Some(_))),
                "a leftover {leftover:?} in the removed acknowledgement variable must not \
                 affect startup — the gate is gone, not renamed"
            );
        }
    }

    /// The claim the deleted acknowledgement denied, asserted as **behaviour**
    /// rather than prose: the signer this loader returns really does enforce the
    /// rotation boundary on the block-producer path.
    ///
    /// This is what makes the removal honest. Deleting a warning because it is
    /// inconvenient is different from deleting it because the thing it warned
    /// about no longer holds — so the replacement guard proves block production
    /// IS covered: a key that is not authoritative at a height is refused when
    /// signing a block-producer preimage at that height.
    #[cfg(not(feature = "hsm"))]
    #[test]
    fn rotation_now_covers_the_block_producer_path_2290_b3() {
        let _guard = env_lock();
        let cleanup = with_valid_rotation_env();

        let resolved = load_block_signer("WRFCOIN_TEST_ROTATION_KEY")
            .expect("rotation config is valid")
            .expect("a signer is configured");
        cleanup();

        // `with_valid_rotation_env` rotates to a DIFFERENT key at height 1000
        // with a 16-block overlap, while the loaded signer holds
        // WRFCOIN_TEST_ROTATION_KEY — neither half of the plan. At/after the
        // effective height only the new key is authoritative, so signing a
        // block-producer payload there must be refused.
        let err = resolved
            .signer
            .sign_at_height(b"block-producer preimage", 1_000)
            .expect_err(
                "block production IS covered by rotation at this head — a key that is not \
                 authoritative at the block's height must be refused at signing time. If \
                 this passes, the deleted scope acknowledgement was telling the truth and \
                 deleting it was wrong (#2290 blocker 3)",
            );
        assert!(
            err.to_string().contains("not authoritative at height 1000"),
            "the refusal must come from the rotation boundary, got: {err}"
        );
    }

    #[test]
    fn load_block_signer_returns_none_when_nothing_configured() {
        // `load_block_signer` reads the ROTATION_* vars, so this test races the
        // rotation tests that set them unless it takes the same process-env lock
        // they do (#2400).
        let _guard = env_lock();
        // Ensure our specific var isn't set. Other tests may set HSM vars
        // in parallel, so we can only assert the debug-env fallback path.
        let var = "WRFCOIN_TEST_NOTHING_CONFIGURED";
        unsafe {
            std::env::remove_var(var);
        }
        // Only assert the debug-env fallback is absent when no HSM feature.
        #[cfg(not(feature = "hsm"))]
        {
            let resolved = load_block_signer(var).expect("no err");
            assert!(
                resolved.is_none(),
                "expected None when neither HSM nor env key is configured"
            );
        }
    }

    // ── #1735 slice 4b: signer failure / timeout / boundary hardening ────────
    //
    // Gaps NOT covered by the existing suite:
    //  (a) `from_env` when the env var is completely absent (vs. empty/bad-hex).
    //  (b) A signature produced by signer A must NOT verify against signer B's
    //      public key (cross-key verification boundary).
    //  (c) `sign(&[])` — empty-payload Ed25519 signing must succeed.
    //  (d) Concurrent `sign` calls via `Arc<dyn BlockSigner>` both produce
    //      independently valid signatures (exercises the Send+Sync contract).
    //  (e) `load_block_signer` with an env key configured returns `DebugEnvKey`
    //      mode, not `Hsm`.
    //  (f) `provenance().to_string()` must never contain the private key hex.

    /// `EnvKeySigner::from_env` must return a clear `Err` — not panic — when the
    /// named env var is completely absent from the process environment (#1735).
    #[test]
    fn env_signer_from_env_missing_var_returns_err() {
        let var = "WRFCOIN_TEST_ENV_SIGNER_MISSING_4B";
        unsafe {
            std::env::remove_var(var);
        }
        let err = EnvKeySigner::from_env(var).unwrap_err();
        assert!(
            err.to_string().contains("not set"),
            "missing env var must produce an error mentioning 'not set': {err}"
        );
    }

    /// A signature produced by signer A must NOT verify against a different
    /// signer B's public key (#1735).
    #[test]
    fn env_signer_sign_does_not_verify_against_different_public_key() {
        let key_a = PrivateKey::generate(&mut rand::thread_rng());
        let key_b = PrivateKey::generate(&mut rand::thread_rng());
        let signer_a = EnvKeySigner::from_private_key(key_a, "4b-key-a");
        let pubkey_b = PublicKey::from_private_key(&key_b);
        let payload = b"cross-key verification boundary test";
        let sig = signer_a.sign(payload).expect("sign must succeed");
        assert!(
            !pubkey_b.verify(payload, &sig),
            "signature from key A must NOT verify against key B's public key (#1735)"
        );
    }

    /// Ed25519 is defined over arbitrary byte strings; signing an empty payload
    /// must succeed and the signature must verify (#1735).
    #[test]
    fn env_signer_sign_empty_payload_succeeds() {
        let private = PrivateKey::generate(&mut rand::thread_rng());
        let signer = EnvKeySigner::from_private_key(private, "4b-empty-payload");
        let sig = signer
            .sign(&[])
            .expect("signing an empty payload must succeed");
        assert!(
            signer.public_key().verify(&[], &sig),
            "empty-payload signature must verify against its own public key (#1735)"
        );
    }

    /// Two threads sharing the same `Arc<dyn BlockSigner>` must each produce a
    /// valid, independently verifiable signature. This exercises the `Send +
    /// Sync` contract on `EnvKeySigner` (#1735).
    #[test]
    fn env_signer_concurrent_sign_produces_valid_signatures() {
        use std::sync::Arc;
        let private = PrivateKey::generate(&mut rand::thread_rng());
        let signer: Arc<dyn BlockSigner> =
            Arc::new(EnvKeySigner::from_private_key(private, "4b-concurrent"));
        let pubkey = signer.public_key();

        let s1 = Arc::clone(&signer);
        let s2 = Arc::clone(&signer);

        let h1 = std::thread::spawn(move || s1.sign(b"payload-thread-1").expect("thread 1 sign"));
        let h2 = std::thread::spawn(move || s2.sign(b"payload-thread-2").expect("thread 2 sign"));

        let sig1 = h1.join().expect("thread 1 must not panic");
        let sig2 = h2.join().expect("thread 2 must not panic");

        assert!(
            pubkey.verify(b"payload-thread-1", &sig1),
            "thread 1 signature must verify (#1735)"
        );
        assert!(
            pubkey.verify(b"payload-thread-2", &sig2),
            "thread 2 signature must verify (#1735)"
        );
    }

    /// After `load_block_signer` resolves an env-key signer, the returned
    /// `SignerMode` must be `DebugEnvKey` — not `Hsm` (#1735).
    #[test]
    #[cfg(not(feature = "hsm"))]
    fn load_block_signer_with_env_key_returns_debug_mode() {
        // Same ROTATION_* race as the sibling test above (#2400).
        let _guard = env_lock();
        let private = PrivateKey::generate(&mut rand::thread_rng());
        let var = "WRFCOIN_TEST_LOAD_SIGNER_DEBUG_MODE_4B";
        unsafe {
            std::env::set_var(var, private.to_hex());
        }
        let result = load_block_signer(var).expect("no error");
        unsafe {
            std::env::remove_var(var);
        }
        let resolved = result.expect("signer must be Some when env key is set");
        assert_eq!(
            resolved.mode,
            SignerMode::DebugEnvKey,
            "env-key-backed signer must report DebugEnvKey mode, not Hsm (#1735)"
        );
    }

    /// `provenance().to_string()` must include the var-name label for audit
    /// trails, but must NEVER contain the private key hex (#1735).
    #[test]
    fn env_signer_provenance_display_does_not_leak_private_key() {
        let private = PrivateKey::generate(&mut rand::thread_rng());
        let hex = private.to_hex();
        let signer = EnvKeySigner::from_private_key(private, "WRFCOIN_TEST_PROVENANCE_4B");
        let display = signer.provenance().to_string();
        assert!(
            !display.contains(&hex),
            "provenance Display must not expose private key hex material (#1735): {display}"
        );
        assert!(
            display.contains("WRFCOIN_TEST_PROVENANCE_4B"),
            "provenance Display must include the var name for audit trails (#1735): {display}"
        );
    }
}

/// Key fixtures for integration consumers, including release test binaries.
/// No environment/file key loading exists here. Excluded from production
/// feature graphs; `test-support` is enabled by downstream dev-dependencies.
#[cfg(feature = "test-support")]
pub mod test_support {
    use super::BlockSigner;
    use crate::crypto::signature::{PrivateKey, PublicKey};
    pub struct FixtureSigner {
        private_key: PrivateKey,
        provenance: crate::crypto::signer::SignerProvenance,
    }
    impl std::fmt::Debug for FixtureSigner {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FixtureSigner").finish_non_exhaustive()
        }
    }
    impl FixtureSigner {
        pub fn from_private_key(private_key: PrivateKey, label: &str) -> Self {
            Self { private_key, provenance: crate::crypto::signer::SignerProvenance::EnvKey { var_name: label.into() } }
        }
    }
    impl BlockSigner for FixtureSigner {
        fn sign(&self, payload: &[u8]) -> anyhow::Result<crate::crypto::signature::Signature> { Ok(self.private_key.sign(payload)) }
        fn public_key(&self) -> PublicKey { PublicKey::from_private_key(&self.private_key) }
        fn provenance(&self) -> &crate::crypto::signer::SignerProvenance { &self.provenance }
    }
}
