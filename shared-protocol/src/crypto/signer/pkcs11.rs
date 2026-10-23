//! PKCS#11-backed block signer.
//!
//! The private key lives inside a PKCS#11 token (softhsm2 for CI, YubiKey
//! via `opensc-pkcs11.so` for testnet validators, AWS CloudHSM /
//! Thales Luna for mainnet) and is **never** exported. The signer holds
//! only a handle to the key object plus a logged-in session; signing is
//! performed by the token.
//!
//! For operator provisioning, see `docs/operator/hsm-migration.md`.

use super::{BlockSigner, SignerProvenance, env_vars};
use crate::crypto::signature::{PublicKey, Signature};
use anyhow::{Context, Result, anyhow, bail};
use cryptoki::context::{CInitializeArgs, Pkcs11};
use cryptoki::mechanism::Mechanism;
use cryptoki::object::{Attribute, AttributeType, ObjectClass, ObjectHandle};
use cryptoki::session::{Session, UserType};
use cryptoki::slot::Slot;
use cryptoki::types::AuthPin;
use std::collections::HashMap;
use std::path::Path;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};
use zeroize::Zeroizing;

/// Config parsed from the `WRFCOIN_HSM_*` env vars.
#[derive(Debug, Clone)]
pub struct Pkcs11Config {
    pub module_path: String,
    pub token_label: String,
    pub key_label: String,
    /// Optional override label for the public key object. When `None`,
    /// the public key lookup uses [`key_label`]. Set this when the PKCS#11
    /// module exposes the keypair under different labels — e.g. Yubico's
    /// `libykcs11.so` reports `"Private key for Digital Signature"` and
    /// `"Public key for Digital Signature"` for the same PIV slot.
    pub pubkey_label: Option<String>,
    /// The *name* of the env var holding the PIN. Double indirection keeps
    /// the PIN out of systemd unit files and `ps` output: the unit file
    /// names `WRFCOIN_HSM_PIN_ENV=WRFCOIN_HSM_PIN`, and the PIN itself is
    /// placed into `WRFCOIN_HSM_PIN` via a systemd `LoadCredential=`.
    pub pin_env: String,
    /// Name of the env var the private label came from, when role-specific.
    /// Carried so the #2405 startup self-check can name the exact variables an
    /// operator has to fix, rather than the generic globals.
    pub key_label_var: Option<String>,
    /// Name of the env var the public label came from, when role-specific.
    pub pubkey_label_var: Option<String>,
}

pub use crate::crypto::signer::SharedKeyFallback;

impl Pkcs11Config {
    pub fn from_env() -> Result<Self> {
        Self::from_env_for_role(None, None, SharedKeyFallback::Allowed)
    }

    /// [`from_env`](Self::from_env) with optional **role-specific** key-label
    /// env vars (#2290 P1-4).
    ///
    /// The block-producer signer and the snapshot signer used to resolve the
    /// same `WRFCOIN_HSM_KEY_LABEL`, so an HSM deployment signed blocks and
    /// snapshot manifests with one key object. That couples two roles with
    /// different blast radii and different rotation schedules: rotating the
    /// producer key would silently invalidate snapshot signing, and a
    /// compromise of either compromises both.
    ///
    /// When `key_label_var` is set and non-empty, it selects a producer-specific
    /// key object. When it is unset, the shared label is used and a warning is
    /// logged — key reuse across roles stays *possible* (some deployments have
    /// one slot) but never *silent*, so an owner signs off on it deliberately.
    pub fn from_env_for_role(
        key_label_var: Option<&str>,
        pubkey_label_var: Option<&str>,
        shared_fallback: SharedKeyFallback,
    ) -> Result<Self> {
        Self::from_env_for_role_inner(key_label_var, pubkey_label_var, true, shared_fallback)
    }

    /// Resolve production role selection without PIN env indirection.
    ///
    /// Used by one-shot ceremony callers that inject a PIN held only in
    /// zeroizing memory. Module, token and object labels still use the normal
    /// production environment contract.
    pub(crate) fn from_env_for_role_without_pin(
        key_label_var: Option<&str>,
        pubkey_label_var: Option<&str>,
        shared_fallback: SharedKeyFallback,
    ) -> Result<Self> {
        Self::from_env_for_role_inner(key_label_var, pubkey_label_var, false, shared_fallback)
    }

    fn from_env_for_role_inner(
        key_label_var: Option<&str>,
        pubkey_label_var: Option<&str>,
        require_pin_env: bool,
        shared_fallback: SharedKeyFallback,
    ) -> Result<Self> {
        fn req(name: &str) -> Result<String> {
            // determinism-ok: startup credential/config read, not a verifier, replay or state-transition input. Sibling of `opt()` below and identical in kind: it selects WHICH key material this node loads, and the value never enters a preimage, a state transition or the state root, so two nodes with different values still compute identical roots (#2290, per #2414).
            let v = std::env::var(name).map_err(|_| anyhow!("{name} is not set"))?;
            if v.trim().is_empty() {
                bail!("{name} is empty");
            }
            Ok(v)
        }
        fn opt(name: Option<&str>) -> Option<String> {
            // determinism-ok: startup credential/config read, not a verifier, replay or state-transition input. It selects WHICH key material this node loads at boot; it never enters a preimage, a state transition or the state root, so two nodes with different values still compute identical roots (#2290, per #2414/#2416).
            name.and_then(|n| std::env::var(n).ok())
                .filter(|v| !v.trim().is_empty())
        }

        let role_key_label = opt(key_label_var);
        let role_pubkey_label = opt(pubkey_label_var);

        // #2405 — a role PUBLIC label with NO role PRIVATE label is still a
        // cross-pair, and is still refused.
        //
        // The mirror case (role private set, role public unset) is NOT an error:
        // Dan proved on #2399 that it is the NORMAL ceremony shape — the operator
        // packet documents the role pubkey label as usually unset, defaulting to
        // that role's own key label. An earlier revision of this file rejected it,
        // which would have turned the standard configuration into a startup
        // refusal. The resolution below handles it correctly instead, so the rule
        // here is both narrower and no longer redundant with it.
        //
        // This direction has no such defence: with the role private label unset
        // the private half comes from the SHARED key label while the public half
        // comes from the role's own, pairing one role's private key with
        // another's public key. Nothing downstream can resolve that, so refuse.
        if role_key_label.is_none() {
            if let (Some(public), Some(private_var), Some(public_var)) =
                (&role_pubkey_label, key_label_var, pubkey_label_var)
            {
                bail!(
                    "{public_var} is set to '{public}' but {private_var} is NOT set.\n\n\
                     The private half would be inherited from {global}, pairing one role's \
                     private key with another role's public key — the node would sign with \
                     one key and advertise another, and every peer would reject the block. \
                     Set {private_var} too, or unset {public_var} (#2405).",
                    global = env_vars::HSM_KEY_LABEL,
                    // secret-leak-ok: interpolates public PKCS#11 object-label text and env-var names, never a PIN or key value
                );
            }
        }

        // #2405 (Frank round 1, blocker 2): record which env var ACTUALLY supplied
        // each label, not which one was *requested*. Storing the requested role
        // var unconditionally meant that in the fallback cases the startup log
        // named `WRFCOIN_HSM_PRODUCER_KEY_LABEL` as the source of a value that
        // variable never provided. A diagnostic that misattributes its own inputs
        // is worse than one that says less, because the operator acts on it.
        let private_source = Some(match (&role_key_label, key_label_var) {
            (Some(_), Some(var)) => var.to_string(),
            _ => env_vars::HSM_KEY_LABEL.to_string(),
        });

        // Captured before the match consumes `role_key_label`: which KEY label is
        // in force decides where the PUBLIC half may come from (#2405).
        let role_key_selected = role_key_label.is_some();
        let key_label = match role_key_label {
            Some(label) => label,
            None => {
                // #2464 HIGH — fail closed for a role whose key must be its own.
                //
                // Falling through to the shared label here is what let
                // `SignerEnv::oracle()` resolve `snapshot-keypair` when
                // WRFCOIN_HSM_ORACLE_KEY_LABEL was absent. With the producer
                // also falling back, producer and oracle became the SAME
                // credential. A warning is the wrong instrument: the node still
                // starts and still signs, so the only evidence is a log line
                // nobody reads until an attestation is questioned.
                if shared_fallback == SharedKeyFallback::Forbidden {
                    let var = key_label_var.unwrap_or(env_vars::HSM_ORACLE_KEY_LABEL);
                    bail!(
                        "{var} is not set.\n\n\
                         This role must name its OWN PKCS#11 key object. Without it the \
                         private half would be inherited from {shared} — the snapshot key \
                         object — so this role would not merely share a key, it would BE \
                         another role. #2464 requires oracle key material to be distinct \
                         from every other signing role's, and a shared key object cannot \
                         satisfy that however the labels are named.\n\n\
                         Set {var} to the oracle key object's label. Refusing to start.",
                        shared = env_vars::HSM_KEY_LABEL,
                        // secret-leak-ok: interpolates env-var names only, never a PIN or key value
                    );
                }
                if let Some(var) = key_label_var {
                    log::warn!(
                        "[signer] {var} is not set — this role will use the SHARED HSM key \
                         label {shared}, the same key object the snapshot signer uses. One \
                         key for two roles means a compromise or rotation of either affects \
                         both. Provision a role-specific key and set {var}, or record that \
                         this reuse is intentional and owner-approved (#2290).",
                        var = var,
                        shared = env_vars::HSM_KEY_LABEL,
                    );
                }
                req(env_vars::HSM_KEY_LABEL)?
            },
        };

        // #2405 — the public half must follow the SAME pair as the private half.
        //
        // This used to fall through to the shared `WRFCOIN_HSM_PUBKEY_LABEL`
        // whenever the role's own public override was absent — even when a
        // role-specific PRIVATE label had been selected. On the N5 that shared
        // variable names the snapshot key, so the node loaded the producer
        // private key and the snapshot public key, signed block 1 with one and
        // advertised the other, and every peer rejected it as BadSignature.
        //
        // That is not an exotic misconfiguration: the operator packet documents
        // the role pubkey label as usually unset, defaulting to the role's own
        // key label. Following the ceremony produced the broken pairing.
        //
        // So the rule is: whichever KEY label is in force determines where the
        // public half comes from.
        //   - role key label selected -> role public override, else `None`.
        //     `Pkcs11Signer::new` resolves `None` against `config.key_label`,
        //     i.e. this role's own label. The shared override is NOT consulted.
        //   - shared key label in force -> the shared public override is correct,
        //     because both halves then come from the same pair.
        let role_public_set = role_pubkey_label.is_some();
        let global_public = opt(Some(env_vars::HSM_PUBKEY_LABEL));
        let global_public_set = global_public.is_some();
        let pubkey_label = if role_key_selected {
            role_pubkey_label
        } else {
            role_pubkey_label.or(global_public)
        };

        // Attribution for the public half, reflecting the rule just applied.
        let public_source = Some(match (role_public_set, pubkey_label_var) {
            (true, Some(var)) => var.to_string(),
            _ if !role_key_selected && global_public_set => {
                env_vars::HSM_PUBKEY_LABEL.to_string()
            },
            // Role key selected with no role public override: `new()` looks the
            // public object up under THIS role's key label. Say that, rather than
            // naming a variable that supplied nothing.
            _ => format!(
                "<unset; public object looked up under the private label from {}>",
                private_source.as_deref().unwrap_or(env_vars::HSM_KEY_LABEL) // secret-leak-ok: this is the name of the env var that supplied a public object label
            ),
        });

        Ok(Self {
            module_path: req(env_vars::HSM_MODULE_PATH)?,
            token_label: req(env_vars::HSM_TOKEN_LABEL)?,
            key_label,
            pubkey_label,
            pin_env: if require_pin_env {
                req(env_vars::HSM_PIN_ENV)?
            } else {
                String::new()
            },
            key_label_var: private_source,
            pubkey_label_var: public_source,
        })
    }

    fn read_pin(&self) -> Result<Zeroizing<String>> {
        read_pin_from_env(&self.pin_env)
    }
}

/// Process-wide PKCS#11 contexts, keyed by module path (#2724).
///
/// `C_Initialize` is defined **per library, per process** — not per slot and not
/// per session. A second `C_Initialize` on an already-initialised module returns
/// `CKR_CRYPTOKI_ALREADY_INITIALIZED`, which is a hard error, not a no-op.
///
/// `launch_api` resolves two signers in one process (the producer signer and the
/// snapshot signer, `load_snapshot_signer`). Before this cache each built its own
/// context, so the second one aborted the process at startup — a release binary
/// with an HSM producer signer could not boot at all (#2724).
///
/// Caching by module path is what makes the second signer reuse the first
/// context. Note this is deliberately NOT keyed by token label or slot: those
/// live *inside* the module, and initialising the same library twice fails no
/// matter which slot each caller wants. Sessions stay per-signer.
///
/// Contexts are never evicted. A `Pkcs11` context is process-scoped by the
/// PKCS#11 lifecycle, and dropping one while a `Session` borrowed from it is
/// still alive would invalidate that session.
static PKCS11_CONTEXTS: OnceLock<Mutex<HashMap<String, Arc<Pkcs11>>>> = OnceLock::new();

/// Load and initialise `module_path` once per process, or return the context
/// already initialised for it.
fn shared_context(module_path: &str) -> Result<Arc<Pkcs11>> {
    let contexts = PKCS11_CONTEXTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = contexts
        .lock()
        .map_err(|_| anyhow!("PKCS#11 context registry mutex poisoned"))?;

    if let Some(existing) = guard.get(module_path) {
        return Ok(Arc::clone(existing));
    }

    let ctx = Pkcs11::new(module_path)
        .with_context(|| format!("failed to load PKCS#11 module at {module_path}"))?;
    ctx.initialize(CInitializeArgs::OsThreads)
        .context("PKCS#11 C_Initialize failed")?;

    let ctx = Arc::new(ctx);
    guard.insert(module_path.to_owned(), Arc::clone(&ctx));
    Ok(ctx)
}

/// PKCS#11-backed signer. The `Session` is kept alive inside a `Mutex` so
/// concurrent sign calls serialise on a single logged-in session.
pub struct Pkcs11Signer {
    session: Mutex<Session>,
    private_key_handle: ObjectHandle,
    context_login: Option<ContextLogin>,
    public_key: PublicKey,
    provenance: SignerProvenance,
    // Keep the context alive for the lifetime of the signer. Dropping it
    // would invalidate `session`. Shared process-wide via `shared_context`
    // (#2724) so a second signer on the same module reuses this context
    // instead of calling `C_Initialize` again.
    _ctx: Arc<Pkcs11>,
}

impl fmt::Debug for Pkcs11Signer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pkcs11Signer")
            .field("provenance", &self.provenance)
            .field("public_key", &self.public_key.to_hex())
            .finish_non_exhaustive()
    }
}

impl Pkcs11Signer {
    /// Load the module, find the token and key, log in, and resolve the
    /// public key. All failure modes surface as `Err` so the caller can
    /// fail-closed at startup.
    pub fn new(config: Pkcs11Config) -> Result<Self> {
        let pin = config.read_pin()?;
        let context_credential = ContextCredential::Environment(config.pin_env.clone());
        Self::new_with_credential(config, pin, context_credential)
    }

    /// Construct through the production PKCS#11 path with a PIN held only in
    /// zeroizing process memory.
    pub fn new_with_pin(config: Pkcs11Config, pin: Zeroizing<String>) -> Result<Self> {
        Self::new_with_credential(config, pin, ContextCredential::RetainInMemory)
    }

    fn new_with_credential(
        config: Pkcs11Config,
        pin: Zeroizing<String>,
        context_credential: ContextCredential,
    ) -> Result<Self> {
        // #2724: one initialised context per module, per process. Constructing a
        // second signer here previously re-ran `C_Initialize` and failed.
        let ctx = shared_context(&config.module_path)?;

        let slot = find_slot_by_token_label(&ctx, &config.token_label)?;
        let session = ctx
            .open_ro_session(slot)
            .with_context(|| format!("failed to open read-only session on slot {slot:?}"))?;
        let auth_pin = AuthPin::new(pin.as_str().to_owned());
        match session.login(UserType::User, Some(&auth_pin)) {
            Ok(()) => {}
            // #2724: PKCS#11 login state is per-APPLICATION-per-token, not per
            // session — logging in on one session logs in every session this
            // process holds for that token. So when a second signer shares the
            // process context (see `shared_context`), its `C_Login` correctly
            // reports CKR_USER_ALREADY_LOGGED_IN. The session IS authenticated;
            // this is success, not failure.
            //
            // Treating it as an error is worse than the double-`C_Initialize`
            // bug it hides behind: the message below claims a PIN retry MAY HAVE
            // BEEN CONSUMED and tells the operator to STOP. No retry is consumed
            // in this case, and on a YubiKey with three tries that false alarm
            // would halt a ceremony for no reason.
            Err(cryptoki::error::Error::Pkcs11(
                cryptoki::error::RvError::UserAlreadyLoggedIn,
                _,
            )) => {}
            Err(error) => {
                return Err(anyhow!(
                    // secret-leak-ok: cryptoki error contains CK_RV/status only; AuthPin bytes are never formatted
                    "PIN LOGIN FAILED: wrong PIN, blocked PIN, or token login fault. ONE PIN RETRY \
                     MAY HAVE BEEN CONSUMED. STOP — do not re-run the harness. Underlying PKCS#11 \
                     error: {error}"
                ));
            }
        }

        let private_key_handle =
            find_key_object(&session, ObjectClass::PRIVATE_KEY, &config.key_label)?;
        let requires_context_login =
            private_key_requires_context_login(&session, private_key_handle)?;
        let pubkey_label = config.pubkey_label.as_deref().unwrap_or(&config.key_label);
        let public_key_handle = find_key_object(&session, ObjectClass::PUBLIC_KEY, pubkey_label)?;
        let public_key = read_ed25519_public_key(&session, public_key_handle)?;

        let provenance = SignerProvenance::Pkcs11 {
            module_path: config.module_path.clone(),
            token_label: config.token_label.clone(),
            key_label: config.key_label.clone(),
        };

        // #2405: log BOTH halves, with the variable each came from. Previously
        // only the private label was visible anywhere, so an operator reading the
        // startup log could not tell which public-key object had been loaded —
        // which is precisely the fact needed to spot a confusable-slot pairing.
        let private_label_var = config
            .key_label_var
            .clone()
            .unwrap_or_else(|| env_vars::HSM_KEY_LABEL.to_string());
        let public_label_var = config
            .pubkey_label_var
            .clone()
            .unwrap_or_else(|| env_vars::HSM_PUBKEY_LABEL.to_string());
        // #2405 blocker 1: read each object's own CKA_ID. The token-level slot is
        // shared by every object, so it cannot distinguish PIV 9A from 9C.
        let private_id = read_object_id(&session, private_key_handle);
        let public_id = read_object_id(&session, public_key_handle);

        let labels = crate::crypto::signer::ResolvedKeypairLabels {
            // Single source of truth, testable without a token. The `"oracle"`
            // arm it adds is #2464's, owed since #2458 landed this struct.
            role: crate::crypto::signer::role_for_key_label_var(config.key_label_var.as_deref())
                .to_string(),
            slot: format!("{slot:?}"),
            token_label: config.token_label.clone(),
            private_label: config.key_label.clone(),
            private_label_var: private_label_var.clone(),
            private_id,
            public_label: pubkey_label.to_string(),
            public_label_var: public_label_var.clone(),
            public_id,
        };

        log::info!(
            "[signer] PKCS#11 pair resolved on token '{token}' (token slot {slot:?}):\n{halves}\n  \
             public key: {pubkey}",
            token = config.token_label,
            slot = slot,
            halves = labels.render(),
            pubkey = public_key.to_hex(),
            // secret-leak-ok: emits only public token/object labels, env-var names, CKA_IDs, slot handle and public key; ResolvedKeypairLabels has no secret field
        );
        if labels.ids_differ() == Some(true) {
            // Not fatal on its own — distinct ids are correct when the two halves
            // are a genuine keypair provisioned as separate objects. The signature
            // check below is what decides. But surface it, because on a token
            // where the halves SHOULD share an id this is the early warning.
            log::warn!(
                "[signer] the private and public objects have different CKA_IDs — \
                 verifying they are actually a pair (#2405)"
                // secret-leak-ok: fixed warning text with no interpolated value
            );
        }

        let signer = Self {
            session: Mutex::new(session),
            private_key_handle,
            context_login: requires_context_login.then(|| ContextLogin {
                module_path: config.module_path.clone(),
                credential: match context_credential {
                    ContextCredential::Environment(pin_env) => ContextPin::Environment(pin_env),
                    ContextCredential::RetainInMemory => ContextPin::InMemory(pin),
                },
            }),
            public_key,
            provenance,
            _ctx: ctx,
        };

        // #2405 fail-closed startup self-check. Sign a fixed payload with the
        // resolved PRIVATE key and verify it under the resolved PUBLIC key. This
        // is the only check that can prove the two halves belong together —
        // comparing labels cannot, because the labels are what was misconfigured.
        let probe = signer
            .sign(crate::crypto::signer::KEYPAIR_SELFCHECK_PAYLOAD)
            .context(
                "PKCS#11 startup self-check could not sign with the resolved private key \
                 (#2405). Refusing to start rather than assume the key is usable.", // secret-leak-ok: fixed context string; the underlying error carries PKCS#11 status, not key bytes
            )?;
        crate::crypto::signer::assert_keypair_consistency(&probe, &signer.public_key, &labels)?;
        log::info!(
            "[signer] keypair self-check PASSED — the resolved private and public keys \
             are the same pair (#2405)" // secret-leak-ok: fixed success text with no formatted signer or credential data
        );

        Ok(signer)
    }
}

impl BlockSigner for Pkcs11Signer {
    fn sign(&self, payload: &[u8]) -> Result<Signature> {
        let session = self
            .session
            .lock()
            .map_err(|_| anyhow!("PKCS#11 session mutex poisoned"))?;
        let sig_bytes = if let Some(context_login) = &self.context_login {
            sign_with_context_login(&session, self.private_key_handle, payload, context_login)?
        } else {
            session
                .sign(&Mechanism::Eddsa, self.private_key_handle, payload)
                .context("PKCS#11 C_Sign (Eddsa) failed")?
        };
        let normalized = normalize_eddsa_signature(&sig_bytes);
        Signature::from_bytes(normalized)
            .context("PKCS#11 signer returned malformed signature bytes")
    }

    fn public_key(&self) -> PublicKey {
        self.public_key.clone()
    }

    fn provenance(&self) -> &SignerProvenance {
        &self.provenance
    }
}

struct ContextLogin {
    module_path: String,
    credential: ContextPin,
}

enum ContextCredential {
    Environment(String),
    RetainInMemory,
}

enum ContextPin {
    Environment(String),
    InMemory(Zeroizing<String>),
}

fn read_pin_from_env(pin_env: &str) -> Result<Zeroizing<String>> {
    // determinism-ok: HSM credential lookup only unlocks the configured key; PIN bytes never enter a signing preimage, verification, or a state transition
    let pin = std::env::var(pin_env).map_err(|_| {
        anyhow!(
            // secret-leak-ok: interpolates only the public environment-variable name, never its PIN value
            "HSM PIN env var {pin_env} is not set"
        )
    })?;
    if pin.is_empty() {
        bail!(
            // secret-leak-ok: interpolates only the public environment-variable name, never its PIN value
            "HSM PIN env var {pin_env} is empty"
        );
    }
    Ok(Zeroizing::new(pin))
}

fn context_specific_login(session: &Session, credential: &ContextPin) -> Result<()> {
    let pin = match credential {
        ContextPin::Environment(pin_env) => read_pin_from_env(pin_env)?,
        ContextPin::InMemory(pin) => Zeroizing::new(pin.as_str().to_owned()),
    };
    let auth_pin = AuthPin::new(pin.as_str().to_owned());
    session
        .login(UserType::ContextSpecific, Some(&auth_pin))
        .context(
            // secret-leak-ok: fixed status text only; neither AuthPin nor PIN bytes are formatted
            "PKCS#11 context-specific C_Login failed after user PIN login succeeded; STOP and do \
             not re-run the harness",
        )
}

fn sign_with_context_login(
    session: &Session,
    private_key_handle: ObjectHandle,
    payload: &[u8],
    context_login: &ContextLogin,
) -> Result<Vec<u8>> {
    // SAFETY: `ContextLogin` is private and `Pkcs11Signer::new` clones this path from the same
    // SAFETY: config used to create and retain `_ctx`; `raw` keeps that module loaded while its
    // SAFETY: symbols are called. Deployment must keep the path on that ABI-compatible PKCS#11
    // SAFETY: module; replacing it could make these symbols interpret the retained handles wrongly.
    let raw = unsafe {
        cryptoki_sys::Pkcs11::new(&context_login.module_path).with_context(|| {
            format!(
                "failed to load PKCS#11 module at {} for split signing",
                context_login.module_path
            )
        })?
    };
    let session_handle = raw_session_handle(session)?;
    let key_handle = raw_object_handle(private_key_handle)?;
    let mut mechanism = cryptoki_sys::CK_MECHANISM {
        mechanism: cryptoki_sys::CKM_EDDSA,
        pParameter: std::ptr::null_mut(),
        ulParameterLen: 0,
    };

    let sign_init = raw
        .C_SignInit
        .as_ref()
        .map(|f| *f)
        .map_err(|e| anyhow!("PKCS#11 module has no C_SignInit function: {e}"))?;
    let sign = raw
        .C_Sign
        .as_ref()
        .map(|f| *f)
        .map_err(|e| anyhow!("PKCS#11 module has no C_Sign function: {e}"))?;

    // SAFETY: `Pkcs11Signer` retains `_ctx`, and its `Mutex` guard keeps this session live and
    // SAFETY: exclusive; `key_handle` was resolved in that same session. `mechanism` is a fully
    // SAFETY: initialized stack `CKM_EDDSA` value with a null parameter and zero parameter length.
    // SAFETY: Alien/stale handles or a mismatched module/layout would expose invalid state to FFI.
    let rv = unsafe {
        sign_init(
            session_handle,
            &mut mechanism as cryptoki_sys::CK_MECHANISM_PTR,
            key_handle,
        )
    };
    ensure_ck_ok(rv, "C_SignInit")?;

    context_specific_login(session, &context_login.credential)?;

    let mut signature = vec![0u8; 128];
    let mut signature_len = signature.len() as cryptoki_sys::CK_ULONG;
    // SAFETY: the retained, mutex-guarded session has an initialized signing operation and a
    // SAFETY: successful context login. The payload pointer/length describes its live borrowed
    // SAFETY: slice; `signature` owns 128 writable bytes and `signature_len` starts at that capacity.
    // SAFETY: A module writing past that supplied bound would violate the trusted PKCS#11 ABI.
    let rv = unsafe {
        sign(
            session_handle,
            payload.as_ptr() as *mut u8,
            payload.len() as cryptoki_sys::CK_ULONG,
            signature.as_mut_ptr(),
            &mut signature_len,
        )
    };
    ensure_ck_ok(rv, "C_Sign")?;
    if signature_len as usize > signature.len() {
        bail!(
            "PKCS#11 C_Sign reported {} bytes after writing into a {} byte buffer",
            signature_len,
            signature.len()
        );
    }
    signature.resize(signature_len as usize, 0);
    Ok(signature)
}

fn raw_session_handle(session: &Session) -> Result<cryptoki_sys::CK_SESSION_HANDLE> {
    session
        .to_string()
        .parse()
        .context("failed to parse PKCS#11 session handle")
}

fn raw_object_handle(handle: ObjectHandle) -> Result<cryptoki_sys::CK_OBJECT_HANDLE> {
    handle
        .to_string()
        .parse()
        .context("failed to parse PKCS#11 object handle")
}

fn ensure_ck_ok(rv: cryptoki_sys::CK_RV, op: &str) -> Result<()> {
    if rv == cryptoki_sys::CKR_OK {
        Ok(())
    } else {
        Err(anyhow!("PKCS#11 {op} failed with CK_RV=0x{rv:08x}"))
    }
}

fn find_slot_by_token_label(ctx: &Pkcs11, label: &str) -> Result<Slot> {
    let slots = ctx
        .get_slots_with_token()
        .context("C_GetSlotList returned no token slots")?;
    for slot in slots {
        let info = ctx
            .get_token_info(slot)
            .with_context(|| format!("C_GetTokenInfo failed for slot {slot:?}"))?;
        if info.label().trim() == label {
            return Ok(slot);
        }
    }
    Err(anyhow!("no PKCS#11 token found with label {label:?}"))
}

fn find_key_object(session: &Session, class: ObjectClass, label: &str) -> Result<ObjectHandle> {
    let handles = session
        .find_objects(&[
            Attribute::Class(class),
            Attribute::Label(label.as_bytes().to_vec()),
        ])
        .with_context(|| format!("C_FindObjects failed for {class:?} label={label:?}"))?;
    match handles.len() {
        0 => Err(anyhow!(
            "no PKCS#11 object found for class={class:?} label={label:?}"
        )),
        1 => Ok(handles[0]),
        n => Err(anyhow!(
            "ambiguous PKCS#11 object: {n} matches for class={class:?} label={label:?}"
        )),
    }
}

/// Read an object's `CKA_ID` (#2405, Frank round 1 blocker 1).
///
/// The token-level `Slot` is shared by every object on the token, so reporting
/// it cannot distinguish a private key in one PIV slot from a public key in
/// another — which is the entire failure this self-check exists to catch. The
/// per-object `CKA_ID` is what actually differs, so it is read here and carried
/// into both the startup log and the mismatch error.
///
/// Best-effort by design: a token that does not expose `CKA_ID` yields `None`
/// and the diagnostic degrades to labels only, rather than failing to start over
/// a missing diagnostic attribute.
///
/// # Verified `CKA_ID` -> PIV slot mapping (`libykcs11`)
///
/// Confirmed by a read-only `pkcs11-tool -O` dump of the standby YubiKey
/// (serial 35278471) during the #2405 ceremony rehearsal — previously this was
/// left unmapped rather than guessed, because a wrong mapping printed in a fatal
/// key-ceremony error would send an operator to the wrong slot:
///
/// | `CKA_ID` | PIV slot | Role |
/// |---|---|---|
/// | `01` | 9A | PIV Authentication — the **producer** key |
/// | `02` | 9C | Digital Signature — the **snapshot/validator** key |
/// | `03` | 9D | Key Management — the **oracle 1** key |
/// | `19` | — | attestation |
///
/// `03` was added on the same class of evidence as the other three, not on the
/// numbering pattern: a read-only
/// `pkcs11-tool --list-objects --type pubkey` dump of the same token during the
/// 2026-08-04 oracle ceremony returned `ID: 03`,
/// `label: Public key for Key Management`. The sequential pattern (9A=1, 9C=2,
/// 9D=3, 9E=4, retired 82–95 = 5–24, attestation F9 = 25 = `0x19`) *predicted*
/// it; the dump *confirmed* it. This map still refuses to guess, for the reason
/// above.
///
/// `libykcs11` DOES expose `CKA_ID`, and object labels are unique per object
/// (`Public key for PIV Authentication` vs `Public key for Digital Signature`),
/// so both halves of the diagnostic are available on the real ceremony hardware.
pub fn piv_slot_for_cka_id(id: &str) -> Option<&'static str> {
    match id {
        "01" => Some("9A (PIV Authentication)"),
        "02" => Some("9C (Digital Signature)"),
        "03" => Some("9D (Key Management)"),
        "19" => Some("attestation"),
        _ => None,
    }
}

fn read_object_id(session: &Session, handle: ObjectHandle) -> Option<String> {
    let attrs = session
        .get_attributes(handle, &[AttributeType::Id])
        .ok()?;
    for attr in attrs {
        if let Attribute::Id(bytes) = attr {
            if bytes.is_empty() {
                return None;
            }
            return Some(bytes.iter().map(|b| format!("{b:02x}")).collect());
        }
    }
    None
}

fn private_key_requires_context_login(session: &Session, handle: ObjectHandle) -> Result<bool> {
    let attrs = session
        .get_attributes(handle, &[AttributeType::AlwaysAuthenticate])
        .context("C_GetAttributeValue(CKA_ALWAYS_AUTHENTICATE) failed")?;
    Ok(always_authenticate_from_attrs(attrs))
}

fn always_authenticate_from_attrs(attrs: Vec<Attribute>) -> bool {
    attrs
        .into_iter()
        .any(|attr| matches!(attr, Attribute::AlwaysAuthenticate(true)))
}

/// Read `CKA_EC_POINT` from a public-key object and decode it as an Ed25519
/// 32-byte public key.
///
/// Per PKCS#11 3.x, `CKA_EC_POINT` for an EdDSA public key is the raw
/// public point wrapped in a DER `OCTET STRING`. Some HSMs and some older
/// tools omit the wrapper. We accept both forms.
fn read_ed25519_public_key(session: &Session, handle: ObjectHandle) -> Result<PublicKey> {
    let attrs = session
        .get_attributes(handle, &[AttributeType::EcPoint])
        .context("C_GetAttributeValue(CKA_EC_POINT) failed")?;
    let ec_point = attrs
        .into_iter()
        .find_map(|a| match a {
            Attribute::EcPoint(bytes) => Some(bytes),
            _ => None,
        })
        .ok_or_else(|| anyhow!("PKCS#11 public key object has no CKA_EC_POINT"))?;

    let raw = strip_der_octet_string(&ec_point).unwrap_or(ec_point.as_slice());
    if raw.len() != 32 {
        bail!(
            "expected 32-byte Ed25519 public key, PKCS#11 returned {} bytes",
            raw.len()
        );
    }
    PublicKey::from_bytes(raw).context("CKA_EC_POINT did not decode as an Ed25519 public key")
}

/// If `bytes` starts with a DER `OCTET STRING` header (0x04 0x20), return
/// the inner 32-byte slice. Otherwise return `None`.
fn strip_der_octet_string(bytes: &[u8]) -> Option<&[u8]> {
    match bytes {
        [0x04, 0x20, rest @ ..] if rest.len() == 32 => Some(rest),
        _ => None,
    }
}

/// Normalize a `CKM_EDDSA` signature return value into the raw 64-byte
/// `r || s` form that [`Signature::from_bytes`] expects.
///
/// PKCS#11 v3.x specifies that `CKM_EDDSA` returns the raw 64 bytes, and
/// every YubiKey + OpenSC build we have tested complies. The defensive
/// branch handles a `0x04 0x40 <64 bytes>` DER `OCTET STRING` wrapper —
/// historically observed in some `softhsm2` 4.x builds and possible in
/// future modules. Falling through silently when the input is already
/// 64 bytes keeps the happy path zero-cost.
fn normalize_eddsa_signature(bytes: &[u8]) -> &[u8] {
    match bytes {
        [0x04, 0x40, rest @ ..] if rest.len() == 64 => rest,
        _ => bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the tests that mutate the process-global `HSM_*` environment
    /// variables. Without this, `config_from_env_requires_all_vars` (which
    /// removes the required vars) races `config_from_env_pubkey_label_is_optional`
    /// (which calls `from_env()` expecting them set), making the latter flakily
    /// `Err` mid-call. Both tests take this lock before touching env state.
    ///
    /// It is now the **parent module's** lock rather than a private one (#2464).
    /// A guard local to this module serialised these tests against each other but
    /// not against `signer::tests`, which mutates the same `WRFCOIN_HSM_*`
    /// variables — so under `--features hsm` the `HSM_MODULE_PATH` set here was
    /// visible to a concurrent loader test there, sending it down a real PKCS#11
    /// open. Same discipline, one lock.
    fn hsm_env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::crypto::signer::test_env_lock()
    }

    /// `03` is the oracle-1 Key Management slot, measured on the ceremony token
    /// (2026-08-04 `pkcs11-tool --list-objects --type pubkey`: `ID: 03`,
    /// `label: Public key for Key Management`).
    ///
    /// The unmapped assertions matter as much as the mapped ones: an unknown id
    /// must stay `None` so the diagnostic degrades to printing the raw value
    /// rather than naming a slot that was guessed.
    #[test]
    fn piv_slot_for_cka_id_maps_the_measured_ids_only() {
        assert_eq!(piv_slot_for_cka_id("01"), Some("9A (PIV Authentication)"));
        assert_eq!(piv_slot_for_cka_id("02"), Some("9C (Digital Signature)"));
        assert_eq!(piv_slot_for_cka_id("03"), Some("9D (Key Management)"));
        assert_eq!(piv_slot_for_cka_id("19"), Some("attestation"));

        // Never guessed: 9E and the retired slots were not dumped, so they are
        // deliberately absent even though the numbering pattern predicts them.
        assert_eq!(piv_slot_for_cka_id("04"), None);
        assert_eq!(piv_slot_for_cka_id("05"), None);
        assert_eq!(piv_slot_for_cka_id(""), None);
        assert_eq!(piv_slot_for_cka_id("3"), None, "unpadded id is not the same id");
        assert_eq!(piv_slot_for_cka_id("03 "), None, "no trimming happens here");
    }

    /// The `signer::pkcs11_piv_slot` mirror exists only so the diagnostic renders
    /// identically in non-`hsm` builds. Two hand-maintained copies of the same
    /// table drift; this is the assertion that stops it, and it is the reason a
    /// new id must be added in both places or fail here.
    #[test]
    fn piv_slot_mirror_matches_pkcs11_table() {
        for id in [
            "01", "02", "03", "19", // mapped
            "04", "05", "18", "1a", "99", "ff", "", "3", "03 ", "0x03", "ID", // unmapped
        ] {
            assert_eq!(
                piv_slot_for_cka_id(id),
                crate::crypto::signer::pkcs11_piv_slot(id),
                "CKA_ID {id:?} renders differently in the two copies of the PIV slot table"
            );
        }
    }

    #[test]
    fn strip_der_wraps_raw() {
        let raw = [7u8; 32];
        assert_eq!(strip_der_octet_string(&raw), None);
        let mut wrapped = vec![0x04, 0x20];
        wrapped.extend_from_slice(&raw);
        assert_eq!(strip_der_octet_string(&wrapped).unwrap(), &raw[..]);
    }

    #[test]
    fn normalize_eddsa_signature_passes_through_raw() {
        let raw = [9u8; 64];
        assert_eq!(normalize_eddsa_signature(&raw), &raw[..]);
    }

    #[test]
    fn normalize_eddsa_signature_strips_der_wrapper() {
        let raw = [9u8; 64];
        let mut wrapped = vec![0x04, 0x40];
        wrapped.extend_from_slice(&raw);
        assert_eq!(normalize_eddsa_signature(&wrapped), &raw[..]);
    }

    #[test]
    fn normalize_eddsa_signature_leaves_unrecognized_alone() {
        // Wrong length: shouldn't be touched. Signature::from_bytes will
        // reject it, surfacing the right error to operators.
        let weird = [0x04u8, 0x40, 0xff, 0xff];
        assert_eq!(normalize_eddsa_signature(&weird), &weird[..]);
        let near_wrapper_wrong_tag = {
            let mut v = vec![0x05, 0x40];
            v.extend_from_slice(&[7u8; 64]);
            v
        };
        assert_eq!(
            normalize_eddsa_signature(&near_wrapper_wrong_tag),
            &near_wrapper_wrong_tag[..]
        );
    }

    #[test]
    fn always_authenticate_from_attrs_detects_required_context_login() {
        assert!(always_authenticate_from_attrs(vec![
            Attribute::AlwaysAuthenticate(true)
        ]));
        assert!(!always_authenticate_from_attrs(vec![
            Attribute::AlwaysAuthenticate(false)
        ]));
        assert!(!always_authenticate_from_attrs(Vec::new()));
    }

    #[test]
    fn config_from_env_requires_all_vars() {
        // Hold the env guard so we never race the other HSM env-mutating test.
        let _guard = hsm_env_guard();
        // Ensure all four vars are unset, then ensure each individually
        // causes from_env to fail.
        for name in [
            env_vars::HSM_MODULE_PATH,
            env_vars::HSM_TOKEN_LABEL,
            env_vars::HSM_KEY_LABEL,
            env_vars::HSM_PUBKEY_LABEL,
            env_vars::HSM_PIN_ENV,
        ] {
            // SAFETY: the HSM_ENV_GUARD lock serializes all HSM env mutation in
            // this test module, so no other thread touches these vars here.
            unsafe {
                std::env::remove_var(name);
            }
        }
        assert!(Pkcs11Config::from_env().is_err());
    }

    #[test]
    fn interactive_ceremony_config_does_not_require_a_pin_environment() {
        let _guard = hsm_env_guard();
        // SAFETY: HSM_ENV_GUARD serializes all HSM env mutation in this module.
        unsafe {
            std::env::set_var(env_vars::HSM_MODULE_PATH, "/lib/test.so");
            std::env::set_var(env_vars::HSM_TOKEN_LABEL, "test-token");
            std::env::set_var(env_vars::HSM_PRODUCER_KEY_LABEL, "producer-private");
            std::env::set_var(env_vars::HSM_PRODUCER_PUBKEY_LABEL, "producer-public");
            std::env::remove_var(env_vars::HSM_PIN_ENV);
        }

        let config = Pkcs11Config::from_env_for_role_without_pin(
            Some(env_vars::HSM_PRODUCER_KEY_LABEL),
            Some(env_vars::HSM_PRODUCER_PUBKEY_LABEL),
            SharedKeyFallback::Allowed,
        )
        .expect("interactive ceremony config must not source a PIN from the environment");
        assert!(config.pin_env.is_empty()); // secret-leak-ok: checks only that no PIN environment-variable name was retained
        assert_eq!(config.key_label, "producer-private");
        assert_eq!(config.pubkey_label.as_deref(), Some("producer-public"));

        unsafe {
            for name in [
                env_vars::HSM_MODULE_PATH,
                env_vars::HSM_TOKEN_LABEL,
                env_vars::HSM_PRODUCER_KEY_LABEL,
                env_vars::HSM_PRODUCER_PUBKEY_LABEL,
                env_vars::HSM_PIN_ENV,
            ] {
                std::env::remove_var(name);
            }
        }
    }

    /// #2405 / Dan #2399 blocker 1 — **the normal ceremony shape must not pair
    /// the producer private key with the snapshot public key.**
    ///
    /// A role-specific PRIVATE label is selected, and the role PUBLIC override is
    /// absent — which the operator packet says is the *usual* configuration,
    /// because an unset role pubkey label is documented to default to that role's
    /// own key label. The resolution instead fell through to the shared
    /// `WRFCOIN_HSM_PUBKEY_LABEL`, which on the N5 names the snapshot key.
    ///
    /// `Pkcs11Signer::new` then loads the private object under the producer label
    /// and the public object under the snapshot label: the node signs with the
    /// producer key while stamping the snapshot public key, and every peer
    /// rejects the block with BadSignature. This is not an exotic
    /// misconfiguration — it is what following the ceremony packet produces.
    ///
    /// `new()` already defaults a `None` pubkey_label to `config.key_label`, so
    /// the fix is to stop manufacturing a value here: a role-specific key label
    /// must never inherit the shared public override.
    ///
    /// RED before the fix: `pubkey_label == Some("snapshot-public")`.
    #[test]
    fn a_role_key_label_never_inherits_the_shared_public_label_2405() {
        let _guard = hsm_env_guard();
        // SAFETY: HSM_ENV_GUARD serializes all HSM env mutation in this module.
        unsafe {
            std::env::set_var(env_vars::HSM_MODULE_PATH, "/lib/test.so");
            std::env::set_var(env_vars::HSM_TOKEN_LABEL, "wrfcoin-producer");
            std::env::set_var(env_vars::HSM_PIN_ENV, "WRFCOIN_HSM_PIN_TEST");
            // The mixed global+producer shape the N5 actually has:
            std::env::set_var(env_vars::HSM_KEY_LABEL, "snapshot-keypair");
            std::env::set_var(env_vars::HSM_PUBKEY_LABEL, "snapshot-public");
            std::env::set_var(env_vars::HSM_PRODUCER_KEY_LABEL, "producer-keypair");
            std::env::remove_var(env_vars::HSM_PRODUCER_PUBKEY_LABEL);
        }

        let cfg = Pkcs11Config::from_env_for_role(
            Some(env_vars::HSM_PRODUCER_KEY_LABEL),
            Some(env_vars::HSM_PRODUCER_PUBKEY_LABEL),
            SharedKeyFallback::Allowed,
        )
        .expect("the documented ceremony shape must resolve, not refuse");

        assert_eq!(cfg.key_label, "producer-keypair");
        assert_ne!(
            // secret-leak-ok: no value here can be secret. The interpolated positions are
            // `cfg.pubkey_label` and a literal — both PKCS#11 *object labels*, which are public
            // token metadata read back by `pkcs11-tool --list-objects` without a login. No key
            // byte, PIN or handle is in scope. The scanner matches the word "PRIVATE" in the
            // assertion's prose, which its own docstring says should not count; `\`-continued
            // string literals defeat STRING_LITERAL, so the prose lands outside the literal.
            cfg.pubkey_label.as_deref(),
            Some("snapshot-public"),
            "a role-specific PRIVATE label must never be paired with the SHARED public \
             label — that loads the producer private key and the snapshot public key, \
             and every peer rejects the block as BadSignature (#2405)"
        );
        assert_eq!(
            cfg.pubkey_label, None,
            "with no role public override the public object must be looked up under \
             THIS ROLE's key label, which `new()` does when pubkey_label is None"
        );

        // An explicit role public override is still honoured (YubiKey exposes the
        // two halves under different labels, which is why the var exists).
        unsafe {
            std::env::set_var(env_vars::HSM_PRODUCER_PUBKEY_LABEL, "producer-public");
        }
        let cfg = Pkcs11Config::from_env_for_role(
            Some(env_vars::HSM_PRODUCER_KEY_LABEL),
            Some(env_vars::HSM_PRODUCER_PUBKEY_LABEL),
            SharedKeyFallback::Allowed,
        )
        .expect("explicit role public label is valid");
        assert_eq!(cfg.pubkey_label.as_deref(), Some("producer-public"));

        // And with NO role key label, the shared pair is used as before —
        // the global override must keep working for the snapshot signer.
        unsafe {
            std::env::remove_var(env_vars::HSM_PRODUCER_KEY_LABEL);
            std::env::remove_var(env_vars::HSM_PRODUCER_PUBKEY_LABEL);
        }
        let cfg = Pkcs11Config::from_env_for_role(
            Some(env_vars::HSM_PRODUCER_KEY_LABEL),
            Some(env_vars::HSM_PRODUCER_PUBKEY_LABEL),
            SharedKeyFallback::Allowed,
        )
        .expect("both unset falls back to the shared pair");
        assert_eq!(cfg.key_label, "snapshot-keypair");
        assert_eq!(
            cfg.pubkey_label.as_deref(),
            Some("snapshot-public"),
            "the shared public override is correct when the shared KEY label is also \
             in use — both halves come from the same pair"
        );

        // SAFETY: HSM_ENV_GUARD serializes all HSM env mutation in this module.
        unsafe {
            for name in [
                env_vars::HSM_MODULE_PATH,
                env_vars::HSM_TOKEN_LABEL,
                env_vars::HSM_KEY_LABEL,
                env_vars::HSM_PUBKEY_LABEL,
                env_vars::HSM_PIN_ENV,
                env_vars::HSM_PRODUCER_KEY_LABEL,
                env_vars::HSM_PRODUCER_PUBKEY_LABEL,
            ] {
                std::env::remove_var(name);
            }
        }
    }

    /// #2405 — the surviving half-provisioning rule: a role PUBLIC label with no
    /// role PRIVATE label is refused.
    ///
    /// The mirror case is deliberately NOT an error any more. An earlier revision
    /// of this file rejected role-private-set/role-public-unset, but Dan proved on
    /// #2399 that this is the documented ceremony shape — the operator packet has
    /// the producer pubkey label usually unset, defaulting to the producer key
    /// label — so rejecting it would refuse the standard configuration. The
    /// resolution handles that case correctly instead
    /// (`a_role_key_label_never_inherits_the_shared_public_label_2405`).
    ///
    /// THIS direction has no such defence: with the role private label unset the
    /// private half comes from the SHARED key label while the public half comes
    /// from the role's own, so the two halves belong to different pairs and no
    /// resolution rule can reconcile them.
    #[test]
    fn a_role_public_label_without_a_role_private_label_is_an_error_2405() {
        let _guard = hsm_env_guard();
        // SAFETY: HSM_ENV_GUARD serializes all HSM env mutation in this module.
        unsafe {
            std::env::set_var(env_vars::HSM_MODULE_PATH, "/lib/test.so");
            std::env::set_var(env_vars::HSM_TOKEN_LABEL, "wrfcoin-producer");
            std::env::set_var(env_vars::HSM_PIN_ENV, "WRFCOIN_HSM_PIN_TEST");
            std::env::set_var(env_vars::HSM_KEY_LABEL, "snapshot-keypair");
            std::env::set_var(env_vars::HSM_PRODUCER_PUBKEY_LABEL, "producer-public");
            std::env::remove_var(env_vars::HSM_PRODUCER_KEY_LABEL);
        }

        let err = match Pkcs11Config::from_env_for_role(
            Some(env_vars::HSM_PRODUCER_KEY_LABEL),
            Some(env_vars::HSM_PRODUCER_PUBKEY_LABEL),
            SharedKeyFallback::Allowed,
        ) {
            Err(e) => e.to_string(),
            Ok(cfg) => panic!(
                "a role PUBLIC label with no role PRIVATE label pairs the shared private \
                 key with the role public key. Got private={:?} public={:?} (#2405)",
                cfg.key_label, cfg.pubkey_label // secret-leak-ok: unit-test panic prints only public PKCS#11 object-label configuration, never key material
            ),
        };
        assert!(err.contains(env_vars::HSM_PRODUCER_KEY_LABEL), "got: {err}");
        assert!(err.contains(env_vars::HSM_PRODUCER_PUBKEY_LABEL), "got: {err}");

        unsafe {
            for name in [
                env_vars::HSM_MODULE_PATH,
                env_vars::HSM_TOKEN_LABEL,
                env_vars::HSM_KEY_LABEL,
                env_vars::HSM_PUBKEY_LABEL,
                env_vars::HSM_PIN_ENV,
                env_vars::HSM_PRODUCER_KEY_LABEL,
                env_vars::HSM_PRODUCER_PUBKEY_LABEL,
            ] {
                std::env::remove_var(name);
            }
        }
    }

    /// #2405 (Frank round 1, blocker 2): the recorded source variable must be the
    /// one that ACTUALLY supplied each label, not the one that was requested.
    ///
    /// With both producer variables unset, resolution falls back to the globals.
    /// The config used to record `WRFCOIN_HSM_PRODUCER_KEY_LABEL` anyway, so the
    /// startup log presented an inherited global as producer-specific
    /// resolution. An operator debugging a mismatch would go check a variable
    /// that was never in play — a diagnostic that misattributes its own inputs is
    /// worse than one that says less, because it is acted upon.
    #[test]
    fn the_recorded_source_var_is_the_one_that_supplied_the_label_2405() {
        let _guard = hsm_env_guard();
        unsafe {
            std::env::set_var(env_vars::HSM_MODULE_PATH, "/lib/test.so");
            std::env::set_var(env_vars::HSM_TOKEN_LABEL, "wrfcoin-producer");
            std::env::set_var(env_vars::HSM_KEY_LABEL, "shared-signing-key");
            std::env::set_var(env_vars::HSM_PIN_ENV, "WRFCOIN_HSM_PIN_TEST");
            std::env::set_var(env_vars::HSM_PUBKEY_LABEL, "shared-public-key");
            // BOTH producer vars unset -> everything comes from the globals.
            std::env::remove_var(env_vars::HSM_PRODUCER_KEY_LABEL);
            std::env::remove_var(env_vars::HSM_PRODUCER_PUBKEY_LABEL);
        }

        let cfg = Pkcs11Config::from_env_for_role(
            Some(env_vars::HSM_PRODUCER_KEY_LABEL),
            Some(env_vars::HSM_PRODUCER_PUBKEY_LABEL),
            SharedKeyFallback::Allowed,
        )
        .expect("both-unset falls back to the shared labels");

        assert_eq!(cfg.key_label, "shared-signing-key");
        assert_eq!(
            cfg.key_label_var.as_deref(),
            Some(env_vars::HSM_KEY_LABEL),
            "the private label came from the GLOBAL var, so that is what must be \
             recorded — naming the unset producer var claims it supplied a value it \
             never did (#2405)" // secret-leak-ok: fixed test assertion about an env-var name; no runtime value is formatted
        );
        assert_eq!(
            cfg.pubkey_label_var.as_deref(),
            Some(env_vars::HSM_PUBKEY_LABEL),
            "same for the public half"
        );

        // And when the role vars ARE set, they are correctly recorded.
        unsafe {
            std::env::set_var(env_vars::HSM_PRODUCER_KEY_LABEL, "producer-private");
            std::env::set_var(env_vars::HSM_PRODUCER_PUBKEY_LABEL, "producer-public");
        }
        let cfg = Pkcs11Config::from_env_for_role(
            Some(env_vars::HSM_PRODUCER_KEY_LABEL),
            Some(env_vars::HSM_PRODUCER_PUBKEY_LABEL),
            SharedKeyFallback::Allowed,
        )
        .expect("both producer vars set is valid");
        assert_eq!(cfg.key_label_var.as_deref(), Some(env_vars::HSM_PRODUCER_KEY_LABEL));
        assert_eq!(cfg.pubkey_label_var.as_deref(), Some(env_vars::HSM_PRODUCER_PUBKEY_LABEL));

        unsafe {
            for name in [
                env_vars::HSM_MODULE_PATH,
                env_vars::HSM_TOKEN_LABEL,
                env_vars::HSM_KEY_LABEL,
                env_vars::HSM_PUBKEY_LABEL,
                env_vars::HSM_PIN_ENV,
                env_vars::HSM_PRODUCER_KEY_LABEL,
                env_vars::HSM_PRODUCER_PUBKEY_LABEL,
            ] {
                std::env::remove_var(name);
            }
        }
    }

    /// Both role labels set is the correct provisioning, and must work.
    #[test]
    fn producer_role_with_both_labels_resolves_the_producer_pair_2405() {
        let _guard = hsm_env_guard();
        unsafe {
            std::env::set_var(env_vars::HSM_MODULE_PATH, "/lib/test.so");
            std::env::set_var(env_vars::HSM_TOKEN_LABEL, "wrfcoin-producer");
            std::env::set_var(env_vars::HSM_KEY_LABEL, "SIGN key");
            std::env::set_var(env_vars::HSM_PIN_ENV, "WRFCOIN_HSM_PIN_TEST");
            std::env::set_var(env_vars::HSM_PUBKEY_LABEL, "Public key for Key Management");
            std::env::set_var(env_vars::HSM_PRODUCER_KEY_LABEL, "Private key for Digital Signature");
            std::env::set_var(env_vars::HSM_PRODUCER_PUBKEY_LABEL, "Public key for Digital Signature");
        }
        let cfg = Pkcs11Config::from_env_for_role(
            Some(env_vars::HSM_PRODUCER_KEY_LABEL),
            Some(env_vars::HSM_PRODUCER_PUBKEY_LABEL),
            SharedKeyFallback::Allowed,
        )
        .expect("both producer labels set is valid provisioning");
        assert_eq!(cfg.key_label, "Private key for Digital Signature");
        assert_eq!(
            cfg.pubkey_label.as_deref(),
            Some("Public key for Digital Signature"),
            "the producer public label must NOT be the global 9C label"
        );
        unsafe {
            for name in [
                env_vars::HSM_MODULE_PATH,
                env_vars::HSM_TOKEN_LABEL,
                env_vars::HSM_KEY_LABEL,
                env_vars::HSM_PUBKEY_LABEL,
                env_vars::HSM_PIN_ENV,
                env_vars::HSM_PRODUCER_KEY_LABEL,
                env_vars::HSM_PRODUCER_PUBKEY_LABEL,
            ] {
                std::env::remove_var(name);
            }
        }
    }

    // ------------------------------------------------------------------
    // #2464 — the oracle attestation role's PKCS#11 label resolution.
    // ------------------------------------------------------------------

    /// Resolve the oracle role the way `SignerEnv::oracle()` does.
    fn oracle_config() -> Result<Pkcs11Config> {
        Pkcs11Config::from_env_for_role(
            Some(env_vars::HSM_ORACLE_KEY_LABEL),
            Some(env_vars::HSM_ORACLE_PUBKEY_LABEL),
            SharedKeyFallback::Forbidden,
        )
    }

    /// Every variable the oracle-role tests touch, cleared. Callers hold
    /// [`hsm_env_guard`].
    fn clear_oracle_env() {
        // SAFETY: the module-tree env lock serializes all env mutation here.
        unsafe {
            for name in [
                env_vars::HSM_MODULE_PATH,
                env_vars::HSM_TOKEN_LABEL,
                env_vars::HSM_KEY_LABEL,
                env_vars::HSM_PUBKEY_LABEL,
                env_vars::HSM_PIN_ENV,
                env_vars::HSM_ORACLE_KEY_LABEL,
                env_vars::HSM_ORACLE_PUBKEY_LABEL,
            ] {
                std::env::remove_var(name);
            }
        }
    }

    /// Set the module/token/PIN vars every role needs. Callers hold
    /// [`hsm_env_guard`].
    fn set_oracle_base_env() {
        // SAFETY: the module-tree env lock serializes all env mutation here.
        unsafe {
            std::env::set_var(env_vars::HSM_MODULE_PATH, "/lib/test.so");
            std::env::set_var(env_vars::HSM_TOKEN_LABEL, "wrfcoin-oracle");
            std::env::set_var(env_vars::HSM_PIN_ENV, "WRFCOIN_HSM_PIN_TEST");
        }
    }

    /// #2464 — **the confusable-pair rule holds for the oracle role.**
    ///
    /// This is the #2405 protection, asserted for the role that did not exist
    /// when #2405 was written. The trap is identical and so is the blast radius:
    /// with an oracle-specific PRIVATE label selected and the oracle PUBLIC
    /// override absent — which the operator packet documents as the *usual*
    /// shape — inheriting the shared `WRFCOIN_HSM_PUBKEY_LABEL` would pair the
    /// oracle private key with the snapshot public key. The attester would sign
    /// attestations with one key and register the other, every attestation would
    /// fail verification against the registered `SigningRole::Oracle` key, and
    /// quorum would never form — the same zero-reward outcome #2464 exists to
    /// end, arriving through a fresh cause.
    ///
    /// RED without the `role_key_selected` branch in `from_env_for_role_inner`:
    /// `pubkey_label == Some("snapshot-public")`.
    #[test]
    fn an_oracle_key_label_never_inherits_the_shared_public_label_2464() {
        let _guard = hsm_env_guard();
        clear_oracle_env();
        set_oracle_base_env();
        // SAFETY: the module-tree env lock serializes all env mutation here.
        unsafe {
            // The mixed global+role shape a real deployment has: the shared pair
            // names the SNAPSHOT key, the oracle names its own private half.
            std::env::set_var(env_vars::HSM_KEY_LABEL, "snapshot-keypair");
            std::env::set_var(env_vars::HSM_PUBKEY_LABEL, "snapshot-public");
            std::env::set_var(env_vars::HSM_ORACLE_KEY_LABEL, "oracle-keypair");
            std::env::remove_var(env_vars::HSM_ORACLE_PUBKEY_LABEL);
        }

        let cfg = oracle_config().expect("the documented ceremony shape must resolve, not refuse");

        assert_eq!(cfg.key_label, "oracle-keypair");
        assert_ne!(
            // secret-leak-ok: no value here can be secret. The interpolated positions are
            // `cfg.pubkey_label` and a literal — both PKCS#11 *object labels*, public token
            // metadata readable by `pkcs11-tool --list-objects` without a login. No key byte,
            // PIN or handle is in scope.
            cfg.pubkey_label.as_deref(),
            Some("snapshot-public"),
            "an oracle-specific key label must never be paired with the SHARED public \
             label — that signs attestations with the oracle key while advertising the \
             snapshot key, and no attestation ever verifies (#2405, #2464)"
        );
        assert_eq!(
            cfg.pubkey_label, None,
            "with no oracle public override the public object must be looked up under \
             THIS role's key label, which `new()` does when pubkey_label is None"
        );

        clear_oracle_env();
    }

    /// An explicit oracle public override is honoured (#2464).
    ///
    /// The override exists because YubiKey's `libykcs11.so` exposes the two
    /// halves of one PIV slot under different labels. A rule that made the
    /// oracle role unable to express that would push operators back onto the
    /// shared pair — reintroducing the reuse the role was added to prevent.
    ///
    /// **Changed by the #2464 HIGH fix.** The second half of this test used to
    /// assert that removing both oracle labels FELL BACK to the shared
    /// `snapshot-keypair` / `snapshot-public` pair, and called that correct.
    /// It was the defect, written down as an expectation — which is why the
    /// fallback survived review. It now asserts the refusal instead.
    #[test]
    fn the_oracle_role_honours_an_explicit_public_override_2464() {
        let _guard = hsm_env_guard();
        clear_oracle_env();
        set_oracle_base_env();
        // SAFETY: the module-tree env lock serializes all env mutation here.
        unsafe {
            std::env::set_var(env_vars::HSM_KEY_LABEL, "snapshot-keypair");
            std::env::set_var(env_vars::HSM_PUBKEY_LABEL, "snapshot-public");
            std::env::set_var(env_vars::HSM_ORACLE_KEY_LABEL, "oracle-keypair");
            std::env::set_var(env_vars::HSM_ORACLE_PUBKEY_LABEL, "oracle-public");
        }
        let cfg = oracle_config().expect("an explicit oracle public label is valid");
        assert_eq!(cfg.key_label, "oracle-keypair");
        assert_eq!(cfg.pubkey_label.as_deref(), Some("oracle-public"));

        // With NO oracle key label the role must REFUSE rather than inherit the
        // shared pair. This is the assertion that was inverted before the fix.
        // SAFETY: the module-tree env lock serializes all env mutation here.
        unsafe {
            std::env::remove_var(env_vars::HSM_ORACLE_KEY_LABEL);
            std::env::remove_var(env_vars::HSM_ORACLE_PUBKEY_LABEL);
        }
        let err = oracle_config()
            .expect_err("an unset oracle key label must refuse, never inherit the shared key");
        let msg = err.to_string();
        assert!(
            msg.contains(env_vars::HSM_ORACLE_KEY_LABEL),
            "the refusal must name the variable the operator has to set; got: {msg}"
        );

        clear_oracle_env();
    }

    /// Elsa's #2472 RED control, kept as a permanent regression (#2464 HIGH).
    ///
    /// Exactly her configuration: HSM mode fully active, the SHARED key pair
    /// present and resolvable, and **both oracle labels deliberately omitted**.
    /// At the reviewed head this returned
    /// `Pkcs11Config { key_label: "snapshot-keypair", pubkey_label: Some("snapshot-public") }`
    /// where it must return `Err`.
    ///
    /// Why it is the whole custody rule in one test: with the shared label
    /// resolvable, the oracle role did not fail — it silently *became* the
    /// snapshot role. And because the producer role is also permitted to fall
    /// back (#2290, by design), producer and oracle could resolve to one
    /// credential, so an oracle attestation would carry block-production
    /// authority into the quorum that decides who gets paid.
    #[test]
    fn an_unset_oracle_label_never_resolves_the_shared_snapshot_key_2464() {
        let _guard = hsm_env_guard();
        clear_oracle_env();
        set_oracle_base_env();
        // SAFETY: the module-tree env lock serializes all env mutation here.
        unsafe {
            // The shared pair is present and perfectly resolvable — that is the
            // point. The refusal must come from the custody rule, not from an
            // absent key.
            std::env::set_var(env_vars::HSM_KEY_LABEL, "snapshot-keypair");
            std::env::set_var(env_vars::HSM_PUBKEY_LABEL, "snapshot-public");
            std::env::remove_var(env_vars::HSM_ORACLE_KEY_LABEL);
            std::env::remove_var(env_vars::HSM_ORACLE_PUBKEY_LABEL);
        }

        let err = match oracle_config() {
            Err(err) => err,
            Ok(cfg) => panic!(
                "the oracle role resolved key_label={:?} instead of refusing — it has silently \
                 become the snapshot role (#2464 custody rule)",
                cfg.key_label
            ),
        };
        let msg = err.to_string();
        assert!(
            msg.contains(env_vars::HSM_ORACLE_KEY_LABEL),
            "the refusal must name the variable to set; got: {msg}"
        );

        // The producer role deliberately KEEPS its fallback (#2290): some
        // deployments have one slot and the owner signs off with a warning.
        // Asserted here so a future tightening of the oracle rule cannot be
        // mistaken for a licence to break the producer ceremony.
        // SAFETY: as above.
        unsafe {
            std::env::remove_var(env_vars::HSM_PRODUCER_KEY_LABEL);
            std::env::remove_var(env_vars::HSM_PRODUCER_PUBKEY_LABEL);
        }
        let producer = Pkcs11Config::from_env_for_role(
            Some(env_vars::HSM_PRODUCER_KEY_LABEL),
            Some(env_vars::HSM_PRODUCER_PUBKEY_LABEL),
            SharedKeyFallback::Allowed,
        )
        .expect("the producer role still permits documented shared-key reuse");
        assert_eq!(producer.key_label, "snapshot-keypair");

        clear_oracle_env();
    }

    /// With neither the oracle key label nor the shared key label set, there is
    /// no key to resolve and startup must **refuse** (#2464).
    ///
    /// The fallback chain bottoms out in a required variable, not in a guess. An
    /// attester that started here would hold no signing key at all.
    #[test]
    fn an_oracle_role_with_no_key_label_anywhere_is_refused_2464() {
        let _guard = hsm_env_guard();
        clear_oracle_env();
        set_oracle_base_env();

        let err = oracle_config()
            .expect_err("no oracle key label and no shared key label must refuse, not resolve");
        assert!(
            err.to_string().contains(env_vars::HSM_KEY_LABEL),
            "the refusal must name the variable that was missing, got: {err}"
        );

        clear_oracle_env();
    }

    #[test]
    fn config_from_env_pubkey_label_is_optional() {
        // Hold the env guard so we never race the other HSM env-mutating test.
        let _guard = hsm_env_guard();
        // Set the four required vars; leave HSM_PUBKEY_LABEL unset.
        // SAFETY: the HSM_ENV_GUARD lock serializes all HSM env mutation in this
        // test module, so no other thread touches these vars here.
        unsafe {
            std::env::set_var(env_vars::HSM_MODULE_PATH, "/lib/test.so");
            std::env::set_var(env_vars::HSM_TOKEN_LABEL, "test-token");
            std::env::set_var(env_vars::HSM_KEY_LABEL, "test-key");
            std::env::set_var(env_vars::HSM_PIN_ENV, "WRFCOIN_HSM_PIN_TEST");
            std::env::remove_var(env_vars::HSM_PUBKEY_LABEL);
        }
        let cfg = Pkcs11Config::from_env().expect("required vars set; pubkey_label is optional");
        assert!(cfg.pubkey_label.is_none(), "unset HSM_PUBKEY_LABEL → None");
        assert_eq!(cfg.key_label, "test-key");

        // Setting HSM_PUBKEY_LABEL surfaces it on the config.
        unsafe {
            std::env::set_var(
                env_vars::HSM_PUBKEY_LABEL,
                "Public key for Digital Signature",
            );
        }
        let cfg = Pkcs11Config::from_env().unwrap();
        assert_eq!(
            cfg.pubkey_label.as_deref(),
            Some("Public key for Digital Signature"),
        );

        // Empty / whitespace-only value is treated as unset (matches required-var trimming).
        unsafe {
            std::env::set_var(env_vars::HSM_PUBKEY_LABEL, "   ");
        }
        let cfg = Pkcs11Config::from_env().unwrap();
        assert!(cfg.pubkey_label.is_none(), "whitespace → treated as unset");

        // Cleanup.
        unsafe {
            for name in [
                env_vars::HSM_MODULE_PATH,
                env_vars::HSM_TOKEN_LABEL,
                env_vars::HSM_KEY_LABEL,
                env_vars::HSM_PUBKEY_LABEL,
                env_vars::HSM_PIN_ENV,
            ] {
                std::env::remove_var(name);
            }
        }
    }
}


// ---------------------------------------------------------------------------
// #3096 — custody check: every token visible through a module, WITHOUT a PIN
// ---------------------------------------------------------------------------

/// One public-key object on one token, read without logging in. Public data
/// only: token serial and label, object label and `CKA_ID`, and the ed25519
/// public key. No PIN is ever supplied, so no retry budget is consumed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)] // secret-leak-ok: token serial, labels, CKA_ID and PUBLIC keys only
pub struct TokenPublicKey {
    /// `CK_TOKEN_INFO.serialNumber`, trimmed.
    pub token_serial: String,
    /// `CK_TOKEN_INFO.label`, trimmed.
    pub token_label: String,
    /// Object `CKA_LABEL`, trimmed.
    pub object_label: String,
    /// Object `CKA_ID`, hex; `None` when the token does not expose one.
    pub cka_id: Option<String>,
    /// PIV slot the `CKA_ID` maps to on a YubiKey, when known.
    pub piv_slot: Option<String>,
    /// Lowercase hex ed25519 public key.
    pub public_key: String,
}

/// Enumerate every ed25519 public-key object on every token the module at
/// `module_path` presents, using read-only sessions and **no login** (#3096
/// `wrfcoin ceremony custody --check`). Objects that are not ed25519 public
/// keys are skipped, not errors. Uninitialized tokens are skipped too (SoftHSM
/// always presents one spare uninitialized slot, and `C_OpenSession` on it
/// fails with `CKR_TOKEN_NOT_RECOGNIZED`). An *initialized* token that refuses
/// a read-only session is an error so a wedged device is loud rather than
/// silently absent.
pub fn enumerate_public_keys(module_path: &Path) -> Result<Vec<TokenPublicKey>> {
    let ctx = Pkcs11::new(module_path)
        .with_context(|| format!("failed to load PKCS#11 module {}", module_path.display()))?;
    ctx.initialize(CInitializeArgs::OsThreads)
        .context("C_Initialize failed")?;
    let slots = ctx
        .get_slots_with_token()
        .context("C_GetSlotList returned no token slots")?;
    let mut found = Vec::new();
    for slot in slots {
        let info = ctx
            .get_token_info(slot)
            .with_context(|| format!("C_GetTokenInfo failed for slot {slot:?}"))?;
        if !info.token_initialized() {
            continue;
        }
        let token_serial = info.serial_number().trim().to_string();
        let token_label = info.label().trim().to_string();
        let session = ctx
            .open_ro_session(slot)
            .with_context(|| format!("C_OpenSession (read-only) failed for token {token_serial}"))?; // secret-leak-ok: PKCS#11 token serial is a public identifier (C_GetTokenInfo, no login)
        let handles = session
            .find_objects(&[Attribute::Class(ObjectClass::PUBLIC_KEY)])
            .with_context(|| format!("C_FindObjects failed on token {token_serial}"))?; // secret-leak-ok: PKCS#11 token serial is a public identifier (C_GetTokenInfo, no login)
        for handle in handles {
            let Ok(public_key) = read_ed25519_public_key(&session, handle) else {
                continue;
            };
            let object_label = session
                .get_attributes(handle, &[AttributeType::Label])
                .ok()
                .and_then(|attrs| {
                    attrs.into_iter().find_map(|a| match a {
                        Attribute::Label(bytes) => Some(String::from_utf8_lossy(&bytes).trim().to_string()),
                        _ => None,
                    })
                })
                .unwrap_or_default();
            let cka_id = read_object_id(&session, handle);
            let piv_slot = cka_id
                .as_deref()
                .and_then(piv_slot_for_cka_id)
                .map(str::to_string);
            found.push(TokenPublicKey {
                token_serial: token_serial.clone(),
                token_label: token_label.clone(),
                object_label,
                cka_id,
                piv_slot,
                public_key: public_key.to_hex().to_ascii_lowercase(),
            });
        }
    }
    found.sort_by(|a, b| {
        (&a.token_serial, &a.cka_id, &a.object_label).cmp(&(&b.token_serial, &b.cka_id, &b.object_label))
    });
    Ok(found)
}
