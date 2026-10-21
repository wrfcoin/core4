//! Non-destructive proof-of-possession harness for an oracle attestation key.
//!
//! A close mirror of `producer_key_pop.rs`, deliberately: the producer harness
//! already encodes the hard-won ceremony properties (production loading seam,
//! HSM-only refusal, PIN-retry accounting, loud all-or-nothing transcript), and
//! a second design would only be a second set of mistakes. Read the two side by
//! side; the differences below are the whole of the intended divergence.
//!
//! # How this differs from the producer harness, and why
//!
//! **1. The expected key is an argument, not a compiled pin.**
//! `producer_key_pop` verifies against `genesis_producer_pins.rs`, which pins
//! two known genesis members. There is no oracle equivalent to pin: the live
//! chain reports `oracle_set: []`, no oracle key ceremony has been performed,
//! and the oracle-key route decision (#2464 S4) is still the owner's to make.
//! Compiling a placeholder oracle pin would fabricate a ceremony fact, so the
//! operator supplies the expected public key and the transcript records that it
//! was operator-asserted rather than compiled. When an oracle set is pinned at
//! genesis, this should switch to the compiled pin and the argument should be
//! removed.
//!
//! **2. It enforces the custody rule against the producer pins.**
//! Review of #2472 proved that `SignerEnv::oracle()` can silently resolve the
//! *snapshot* keypair when `WRFCOIN_HSM_ORACLE_KEY_LABEL` is unset, because
//! `Pkcs11Config::from_env_for_role_inner` falls through to the shared
//! `WRFCOIN_HSM_KEY_LABEL`. Producer and oracle can therefore become the same
//! credential — exactly what the #2464 custody rule forbids. A ceremony gate is
//! the right place to catch that, so this harness refuses to certify an oracle
//! key that matches any compiled genesis producer key. It is a real check here
//! precisely because it cannot yet be relied upon in the signer layer.
//!
//! **3. No hardware pre-flight against known token serials.**
//! The producer harness checks two known YubiKey serials. No oracle token has
//! been provisioned, so there is nothing to check against; the harness validates
//! that the oracle-role environment is fully specified instead.

#[path = "launch_api/genesis_producer_pins.rs"]
mod genesis_producer_pins;

use anyhow::{Context, Result, anyhow, bail};
use dialoguer::Password;
use serde::Deserialize;
use std::process::ExitCode;
use wrfcoin_shared_protocol::crypto::signature::{PublicKey, Signature};
use wrfcoin_shared_protocol::crypto::signer::{
    self as protocol_signer, SignerEnv, SignerMode, SignerProvenance,
};
use wrfcoin_shared_protocol::oracle::attester::ForbiddenRoleKeys;
use zeroize::Zeroizing;

const CHALLENGE_DOMAIN: &[u8] = b"wrfcoin/core4/oracle-key-proof-of-possession/v1";
/// Oracle attestations are not bound to a block height; sign at the genesis
/// height for the same reason the producer harness does — a rotating signer
/// must be given a height to evaluate rather than allowed the heightless path.
const GENESIS_BLOCK_HEIGHT: u64 = 1;
const BUILD_REVISION: &str = match option_env!("WRFCOIN_BUILD_REVISION") {
    Some(revision) => revision,
    None => "UNSTAMPED",
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PinnedSet {
    version: u32,
    members: Vec<PinnedMember>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PinnedMember {
    producer_id: String,
    #[allow(dead_code)]
    payout_address: String,
    public_key: String,
}

#[derive(Debug, Clone)]
struct Target {
    oracle_id: String,
    expected_public_key: PublicKey,
    /// Operator-supplied `role=pubkey` pairs for roles whose key cannot be read
    /// without a PIN. Without these the inventory is incomplete and this
    /// refuses — see `build_forbidden_inventory`.
    forbid: Vec<(String, PublicKey)>,
    /// Explicit operator acknowledgement that unavailable roles cannot be
    /// compared during this ceremony invocation.
    custody_inventory_waived: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PinRetryStatus {
    ZeroConsumed,
    OneMayHaveBeenConsumed,
}

impl PinRetryStatus {
    fn banner(self) -> &'static str {
        match self {
            Self::ZeroConsumed => "ZERO PIN RETRIES CONSUMED",
            Self::OneMayHaveBeenConsumed => {
                "ONE PIN RETRY MAY HAVE BEEN CONSUMED — STOP AND DO NOT RE-RUN"
            },
        }
    }
}

#[derive(Debug)] // secret-leak-ok: pin_retry_status is a two-variant enum (retry accounting), not PIN material; the PIN lives in Zeroizing<String> and is never stored here
struct RunFailure {
    source: anyhow::Error,
    pin_retry_status: PinRetryStatus,
}

impl RunFailure {
    fn before_login(source: anyhow::Error) -> Self {
        Self {
            source,
            pin_retry_status: PinRetryStatus::ZeroConsumed,
        }
    }

    fn after_prompt(source: anyhow::Error) -> Self {
        Self {
            source,
            pin_retry_status: PinRetryStatus::OneMayHaveBeenConsumed,
        }
    }
}

struct Pkcs11Provenance {
    module_path: String,
    token_label: String,
    key_label: String,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "--build-revision" {
        println!("{BUILD_REVISION}");
        return ExitCode::SUCCESS;
    }
    match run(args) {
        Ok(transcript) => {
            println!("{transcript}");
            ExitCode::SUCCESS
        },
        Err(error) => {
            eprintln!("{}", render_failure(&error));
            ExitCode::FAILURE
        },
    }
}

fn render_failure(error: &RunFailure) -> String {
    format!(
        // secret-leak-ok: banner() is a fixed &'static str and source is an anyhow message; no PIN or key material is interpolated
        "ORACLE KEY PROOF-OF-POSSESSION: FAIL\n{}\n{:#}\n\
         No proof-of-possession transcript was certified.",
        error.pin_retry_status.banner(),
        error.source
    )
}

fn usage() -> &'static str {
    "usage: oracle_key_pop --oracle-id <id> --expected-public-key <hex> \\\n\
            [--forbid-key <role>=<pubkey-hex> ...] [--custody-inventory-waived]\n\
     identity check: oracle_key_pop --build-revision\n\
     note: the expected key is operator-asserted — no genesis oracle set is compiled yet\n\
     note: --forbid-key supplies a non-oracle role key that cannot be read without a PIN\n\
           (e.g. an HSM-resident snapshot key). Every role in NON_ORACLE_SIGNING_ROLES\n\
           must be accounted for or this refuses: a label is not a key object.\n\
     warning: --custody-inventory-waived logs missing roles and certifies only the supplied inventory."
}

fn run(args: impl IntoIterator<Item = String>) -> std::result::Result<String, RunFailure> {
    // Every operation that cannot authenticate to the token stays inside this
    // closure, so its single error boundary keeps pre-login failures honestly
    // classified as zero-retry.
    let (target, forbidden, checked_env) = (|| -> Result<_> {
        let target = parse_target(args)?;
        if BUILD_REVISION == "UNSTAMPED" {
            bail!(
                // secret-leak-ok: static text; nothing is interpolated
                "UNSTAMPED BUILD — ZERO PIN RETRIES CONSUMED. Rebuild with \
                 WRFCOIN_BUILD_REVISION set to the frozen git SHA; refusing to certify a stale \
                 or unidentified binary"
            );
        }
        let forbidden = build_forbidden_inventory(&target)?;
        // Refuse before touching the token: if the operator has asserted an
        // expected key that is already a producer key, the ceremony is
        // misconceived and no PIN should be spent discovering that.
        reject_other_role_key_material(&target.expected_public_key, &forbidden, "asserted")?;
        let checked_env = validate_oracle_environment()?;
        Ok((target, forbidden, checked_env))
    })()
    .map_err(RunFailure::before_login)?;

    let pin = pin_from_env_or_tty(&target).map_err(RunFailure::before_login)?;
    run_after_prompt(target, forbidden, checked_env, pin).map_err(RunFailure::after_prompt)
}

fn run_after_prompt(
    target: Target,
    forbidden: ForbiddenRoleKeys,
    checked_env: CheckedEnvironment,
    pin: Zeroizing<String>,
) -> Result<String> {
    // The production loading seam. Do not replace with Pkcs11Config::from_env
    // or Pkcs11Signer::new: those bypass SignerEnv::oracle and would certify a
    // helper rather than the role, reading green while proving nothing.
    let resolved = protocol_signer::load_block_signer_for_with_pin(SignerEnv::oracle(), pin)
        .context(
            "production oracle-signer loading path failed. An oracle that cannot sign cannot \
             attest, and a node short of quorum pays no contributor",
        )?;

    if resolved.mode != SignerMode::Hsm {
        bail!(
            // secret-leak-ok: interpolates the resolved mode only; no private material is loaded into this process
            "production loader resolved {:?}, not HSM; refusing to certify an env or key-file \
             fallback",
            resolved.mode
        );
    }
    let provenance = match resolved.signer.provenance() {
        SignerProvenance::Pkcs11 {
            module_path,
            token_label,
            key_label,
        } => Pkcs11Provenance {
            module_path: module_path.clone(),
            token_label: token_label.clone(),
            key_label: key_label.clone(),
        },
        other => bail!(
            "production loader returned non-PKCS#11 provenance {other}; refusing to certify it"
        ),
    };

    let loaded_public_key = resolved.signer.public_key();

    // The #2464 custody rule, enforced where it is currently enforceable.
    // An unset WRFCOIN_HSM_ORACLE_KEY_LABEL resolves the shared snapshot key,
    // so this is a live failure mode, not a hypothetical one.
    reject_other_role_key_material(&loaded_public_key, &forbidden, "resolved")?;

    if loaded_public_key != target.expected_public_key {
        bail!(
            // secret-leak-ok: interpolates public keys and the oracle id only
            "KEY PRESENT BUT NOT THE ASSERTED PAIR: production loader returned public key {}, \
             but --expected-public-key asserted {} for {}. Check the inserted token, slot \
             labels, and the WRFCOIN_HSM_ORACLE_KEY_LABEL / WRFCOIN_HSM_ORACLE_PUBKEY_LABEL \
             pairing",
            loaded_public_key.to_hex(),
            target.expected_public_key.to_hex(),
            target.oracle_id,
        );
    }

    let challenge = challenge_bytes(&target);
    let signature = resolved
        .signer
        .sign_at_height(&challenge, GENESIS_BLOCK_HEIGHT)
        .context("token was found, but signing through the production signer failed")?;

    verify_proof(&target.expected_public_key, &challenge, &signature)?;

    Ok(render_transcript(
        &target,
        &checked_env,
        &provenance,
        &loaded_public_key,
        &challenge,
        &signature,
    ))
}

fn parse_target(args: impl IntoIterator<Item = String>) -> Result<Target> {
    let mut oracle_id: Option<String> = None;
    let mut expected: Option<String> = None;
    let mut forbid: Vec<(String, PublicKey)> = Vec::new();
    let mut custody_inventory_waived = false;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => bail!(usage()),
            "--oracle-id" => {
                oracle_id = Some(
                    args.next()
                        .ok_or_else(|| anyhow!("--oracle-id requires a value\n{}", usage()))?,
                );
            },
            "--expected-public-key" => {
                expected = Some(args.next().ok_or_else(|| {
                    anyhow!("--expected-public-key requires a value\n{}", usage())
                })?);
            },
            "--forbid-key" => {
                let raw = args
                    .next()
                    .ok_or_else(|| anyhow!("--forbid-key requires <role>=<hex>\n{}", usage()))?;
                let (role, hex) = raw.split_once('=').ok_or_else(|| {
                    anyhow!(
                        "--forbid-key expects <role>=<hex>, got {raw:?}\n{}",
                        usage()
                    )
                })?;
                let key = PublicKey::from_hex(hex).with_context(|| {
                    format!("--forbid-key {role}: not a valid ed25519 public key hex string")
                })?;
                forbid.push((role.to_string(), key));
            },
            "--custody-inventory-waived" => custody_inventory_waived = true,
            other => bail!("unknown argument {other:?}\n{}", usage()),
        }
    }

    let oracle_id = oracle_id.ok_or_else(|| anyhow!("--oracle-id is required\n{}", usage()))?;
    if oracle_id.trim().is_empty() {
        bail!("--oracle-id must not be empty\n{}", usage());
    }
    let expected =
        expected.ok_or_else(|| anyhow!("--expected-public-key is required\n{}", usage()))?;
    let expected_public_key = PublicKey::from_hex(&expected)
        .context("--expected-public-key is not a valid ed25519 public key hex string")?;

    Ok(Target {
        oracle_id,
        expected_public_key,
        forbid,
        custody_inventory_waived,
    })
}

struct CheckedEnvironment {
    module_path: String,
    token_label: String,
    private_key_label: String,
    public_key_label: String,
}

/// Require the oracle role to be *fully* specified.
///
/// The oracle label being merely optional is the #2472 defect: unset, it falls
/// through to the shared snapshot label. Until that is fixed in the signer
/// layer, this harness refuses to proceed without it rather than certifying
/// whichever key the fallthrough happened to select.
fn validate_oracle_environment() -> Result<CheckedEnvironment> {
    use protocol_signer::env_vars;

    let require = |name: &str| -> Result<String> {
        // determinism-ok: operator ceremony binary, not a consensus path; reading the HSM role configuration is its entire purpose and it never contributes to block state
        let value = std::env::var(name).unwrap_or_default();
        if value.trim().is_empty() {
            bail!(
                // secret-leak-ok: interpolates env-var NAMES and the caller-supplied var name, never their values
                "{name} is unset or empty. The oracle role must be explicitly labelled: an unset \
                 {} silently resolves the shared {} (the snapshot keypair), which would certify \
                 producer key material as an oracle key",
                env_vars::HSM_ORACLE_KEY_LABEL,
                env_vars::HSM_KEY_LABEL,
            );
        }
        Ok(value)
    };

    Ok(CheckedEnvironment {
        module_path: require(env_vars::HSM_MODULE_PATH)?,
        token_label: require(env_vars::HSM_TOKEN_LABEL)?,
        private_key_label: require(env_vars::HSM_ORACLE_KEY_LABEL)?,
        public_key_label: require(env_vars::HSM_ORACLE_PUBKEY_LABEL)?,
    })
}

/// Every compiled genesis producer key, as `(producer_id, key)`.
fn pinned_producer_keys() -> Result<Vec<(String, PublicKey)>> {
    let Some(raw) = genesis_producer_pins::TESTNET_GENESIS_PRODUCER_SET else {
        // Fail closed. Without the producer pins the custody rule cannot be
        // checked at all, and an unverifiable custody rule is worse than a
        // loud refusal because it reads as a pass.
        bail!(
            "no testnet genesis producer set is compiled into this binary, so the #2464 custody \
             rule (oracle key must differ from producer key material) cannot be verified. \
             Refusing to certify an oracle key that cannot be proven distinct"
        );
    };
    let set: PinnedSet = serde_json::from_str(raw)
        .context("compiled testnet genesis producer set is invalid JSON")?;
    if set.version != 1 {
        bail!(
            "compiled testnet genesis producer set has unsupported version {}",
            set.version
        );
    }
    set.members
        .into_iter()
        .map(|m| {
            let key = PublicKey::from_hex(&m.public_key).with_context(|| {
                format!(
                    "compiled genesis producer key for {} is not a valid ed25519 public key",
                    m.producer_id
                )
            })?;
            Ok((m.producer_id, key))
        })
        .collect()
}

/// Assemble the COMPLETE non-oracle role inventory this ceremony compares against.
///
/// Three sources, and the union must cover every role in
/// [`NON_ORACLE_SIGNING_ROLES`]:
///
/// 1. the compiled genesis producer pins — no hardware, no environment needed;
/// 2. software-resolvable roles from the environment ([`ForbiddenRoleKeys::from_env`]);
/// 3. `--forbid-key <role>=<hex>` for roles that live on a token and therefore
///    cannot be read without a PIN.
///
/// Anything still missing is a refusal. Review of #2474 found the previous
/// version compared against producer pins alone, so an explicit oracle label
/// pointing at the *snapshot* object was certified — the rule was checked
/// against a partial inventory and reported a pass. **A label is not a key
/// object**, so a role we cannot see is a role we cannot clear.
fn build_forbidden_inventory(target: &Target) -> Result<ForbiddenRoleKeys> {
    let mut inventory = ForbiddenRoleKeys::from_env();

    for (producer_id, key) in pinned_producer_keys()? {
        inventory = inventory.with_key("producer", key);
        let _ = producer_id;
    }
    for (role, key) in &target.forbid {
        inventory = inventory.with_key(role.clone(), key.clone());
    }

    let missing = inventory.uninventoried();
    if !missing.is_empty() && !target.custody_inventory_waived {
        bail!(
            // secret-leak-ok: interpolates role NAMES only; no key material of any kind
            "INCOMPLETE CUSTODY INVENTORY: no key available for role(s) [{}]. #2464 requires the \
             oracle key to be proven distinct from every other signing role, and a role that \
             cannot be compared cannot be cleared — a label is not a key object. Supply each \
             with --forbid-key <role>=<pubkey-hex> (the public half is enough, and is recorded \
             in the key ceremony). Refusing to certify against a partial inventory",
            missing.join(", ")
        );
    }
    if !missing.is_empty() {
        eprintln!(
            "WARNING: CUSTODY INVENTORY WAIVED — no key available for role(s) [{}]; \
             only the supplied inventory was compared. This is an operator-asserted exception, \
             not a custody-rule pass.",
            missing.join(", ")
        );
    }
    Ok(inventory)
}

fn reject_other_role_key_material(
    candidate: &PublicKey,
    forbidden: &ForbiddenRoleKeys,
    which: &str,
) -> Result<()> {
    if let Some(role) = forbidden.conflict(candidate) {
        bail!(
            // secret-leak-ok: interpolates a PUBLIC key and a role name; no private material exists in this process
            // secret-leak-ok: interpolates public keys and a role name only
            "CUSTODY VIOLATION: the {which} oracle public key {} is the {role} role's key. \
             #2464 requires oracle key material to be distinct from every other signing role; \
             an attestation signed with another role's key would carry that role's authority \
             into the oracle quorum. Refusing to certify",
            candidate.to_hex(),
        );
    }
    Ok(())
}

fn prompt_pin_from_tty(target: &Target) -> Result<Zeroizing<String>> {
    let pin = Password::new()
        .with_prompt(format!(
            "PIV PIN for oracle {} (input hidden; a wrong PIN consumes a retry)",
            target.oracle_id
        ))
        .interact()
        .context("failed to read PIN from the terminal")?;
    Ok(Zeroizing::new(pin))
}

/// Follow the production double-env contract when the ceremony wrapper has
/// supplied one; retain the hidden TTY prompt for an interactive ceremony.
fn pin_from_env_or_tty(target: &Target) -> Result<Zeroizing<String>> {
    use protocol_signer::env_vars;

    // determinism-ok: operator ceremony binary, not a consensus path; the HSM PIN env indirection is read once to open the token for a single proof
    match std::env::var(env_vars::HSM_PIN_ENV) {
        Ok(pin_env) if !pin_env.trim().is_empty() => {
            // determinism-ok: operator ceremony binary, not a consensus path; resolving the indirected HSM PIN is its entire purpose and never contributes to block state
            let pin = std::env::var(&pin_env)
                .with_context(|| format!("HSM PIN env var {pin_env} is not set"))?; // secret-leak-ok: interpolates the env-var NAME (double-env contract), never the PIN value
            if pin.is_empty() {
                bail!("HSM PIN env var {pin_env} is empty"); // secret-leak-ok: interpolates the env-var NAME (double-env contract), never the PIN value
            }
            Ok(Zeroizing::new(pin))
        },
        Ok(_) | Err(std::env::VarError::NotPresent) => prompt_pin_from_tty(target),
        Err(error) => Err(error).context("WRFCOIN_HSM_PIN_ENV is not valid unicode"),
    }
}

fn challenge_bytes(target: &Target) -> Vec<u8> {
    let key_bytes = target.expected_public_key.as_bytes();
    let mut challenge =
        Vec::with_capacity(CHALLENGE_DOMAIN.len() + 2 + target.oracle_id.len() + key_bytes.len());
    challenge.extend_from_slice(CHALLENGE_DOMAIN);
    challenge.push(0);
    challenge.extend_from_slice(target.oracle_id.as_bytes());
    challenge.push(0);
    challenge.extend_from_slice(key_bytes);
    challenge
}

fn verify_proof(
    expected_public_key: &PublicKey,
    challenge: &[u8],
    signature: &Signature,
) -> Result<()> {
    if !expected_public_key.verify(challenge, signature) {
        bail!(
            // secret-leak-ok: strict verification failure prints only the public key
            "KEY PRESENT BUT PRIVATE HALF DOES NOT MATCH THE ASSERTED KEY: signature failed \
             strict verification under public key {}",
            expected_public_key.to_hex()
        );
    }
    Ok(())
}

fn render_transcript(
    target: &Target,
    checked_env: &CheckedEnvironment,
    provenance: &Pkcs11Provenance,
    loaded_public_key: &PublicKey,
    challenge: &[u8],
    signature: &Signature,
) -> String {
    format!(
        // secret-leak-ok: transcript emits public keys, object labels, env-var names and a signature only; no private material is loaded into this process
        "ORACLE KEY PROOF-OF-POSSESSION: PASS\n\
         build_revision      {BUILD_REVISION}\n\
         oracle_id           {}\n\
         public_key          {}\n\
         key_source          OPERATOR-ASSERTED (--expected-public-key); no genesis oracle set is \
         compiled yet — see #2464 S4\n\
         custody_rule        {}\n\
         signer_mode         Hsm\n\
         module_path         {}\n\
         token_label         {}\n\
         private_key_label   {}\n\
         public_key_label    {}\n\
         provenance_module   {}\n\
         provenance_token    {}\n\
         provenance_key      {}\n\
         challenge_domain    {}\n\
         challenge           {}\n\
         signature           {}\n\
         verification        STRICT ed25519 verify PASSED\n\
         \n\
         The private half of {} is present on the token and reachable through the production \
         SignerEnv::oracle() path.",
        target.oracle_id,
        loaded_public_key.to_hex(),
        if target.custody_inventory_waived {
            "WARNING — custody inventory waived; distinct only from supplied keys"
        } else {
            "PASS — distinct from every compiled genesis producer key and supplied role key"
        },
        checked_env.module_path,
        checked_env.token_label,
        checked_env.private_key_label,
        checked_env.public_key_label,
        provenance.module_path,
        provenance.token_label,
        provenance.key_label,
        String::from_utf8_lossy(CHALLENGE_DOMAIN),
        hex::encode(challenge),
        signature.to_hex(),
        loaded_public_key.to_hex(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_from(seed: &str) -> PublicKey {
        use wrfcoin_shared_protocol::crypto::signature::PrivateKey;
        PublicKey::from_private_key(&PrivateKey::from_hex(seed).expect("valid test key"))
    }

    const KEY_A: &str = "4242424242424242424242424242424242424242424242424242424242424242"; // pragma: allowlist secret -- deterministic test-only ed25519 private key; exists solely to make these unit tests reproducible and controls nothing
    const KEY_B: &str = "1313131313131313131313131313131313131313131313131313131313131313"; // pragma: allowlist secret -- deterministic test-only ed25519 private key; exists solely to make these unit tests reproducible and controls nothing

    #[test]
    fn parsing_requires_both_arguments() {
        assert!(parse_target(["--oracle-id".to_string(), "o1".to_string()]).is_err());
        assert!(
            parse_target([
                "--expected-public-key".to_string(),
                key_from(KEY_A).to_hex()
            ])
            .is_err()
        );
        let ok = parse_target([
            "--oracle-id".to_string(),
            "o1".to_string(),
            "--expected-public-key".to_string(),
            key_from(KEY_A).to_hex(),
        ])
        .expect("both arguments present");
        assert_eq!(ok.oracle_id, "o1");
        assert_eq!(ok.expected_public_key, key_from(KEY_A));
        assert!(!ok.custody_inventory_waived);
    }

    #[test]
    fn parsing_accepts_explicit_custody_inventory_waiver_3048() {
        let parsed = parse_target([
            "--oracle-id".to_string(),
            "o1".to_string(),
            "--expected-public-key".to_string(),
            key_from(KEY_A).to_hex(),
            "--custody-inventory-waived".to_string(),
        ])
        .expect("an explicit waiver must parse");
        assert!(parsed.custody_inventory_waived);
        assert!(usage().contains("--custody-inventory-waived"));
    }

    /// `--forbid-key` is how a role whose key lives on a token (and therefore
    /// cannot be read without a PIN) enters the inventory. Without it the
    /// ceremony refuses, so the parse has to be exact.
    #[test]
    fn parsing_accepts_repeated_forbid_key_pairs_2464() {
        let parsed = parse_target([
            "--oracle-id".to_string(),
            "o1".to_string(),
            "--expected-public-key".to_string(),
            key_from(KEY_A).to_hex(),
            "--forbid-key".to_string(),
            format!("snapshot={}", key_from(KEY_B).to_hex()),
            "--forbid-key".to_string(),
            format!("producer={}", key_from(KEY_A).to_hex()),
        ])
        .expect("two forbid-key pairs must parse");

        assert_eq!(parsed.forbid.len(), 2);
        assert_eq!(parsed.forbid[0].0, "snapshot");
        assert_eq!(parsed.forbid[0].1, key_from(KEY_B));
        assert_eq!(parsed.forbid[1].0, "producer");
    }

    #[test]
    fn parsing_rejects_a_malformed_forbid_key_2464() {
        for bad in ["snapshot", "snapshot=not-hex", "=deadbeef"] {
            assert!(
                parse_target([
                    "--oracle-id".to_string(),
                    "o1".to_string(),
                    "--expected-public-key".to_string(),
                    key_from(KEY_A).to_hex(),
                    "--forbid-key".to_string(),
                    bad.to_string(),
                ])
                .is_err(),
                "--forbid-key {bad:?} must be refused rather than silently dropped, which \
                 would leave the inventory quietly incomplete"
            );
        }
    }

    #[test]
    fn parsing_rejects_a_malformed_public_key() {
        assert!(
            parse_target([
                "--oracle-id".to_string(),
                "o1".to_string(),
                "--expected-public-key".to_string(),
                "not-hex".to_string(),
            ])
            .is_err()
        );
    }

    /// The #2464 custody rule: an oracle key equal to a producer key is refused.
    #[test]
    fn an_oracle_key_matching_a_producer_key_is_refused_2464() {
        let producer = key_from(KEY_A);
        let forbidden = ForbiddenRoleKeys::default().with_key("producer", producer.clone());

        let err = reject_other_role_key_material(&producer, &forbidden, "resolved")
            .expect_err("a producer key must never certify as an oracle key");
        let rendered = format!("{err:#}");
        assert!(rendered.contains("CUSTODY VIOLATION"), "{rendered}");
        assert!(rendered.contains("producer"), "{rendered}");
    }

    #[test]
    fn a_distinct_oracle_key_passes_the_custody_rule_2464() {
        let forbidden = ForbiddenRoleKeys::default().with_key("producer", key_from(KEY_A));
        reject_other_role_key_material(&key_from(KEY_B), &forbidden, "resolved")
            .expect("a distinct key must pass");
    }

    /// The challenge must bind the oracle id, so a signature certified for one
    /// oracle cannot be replayed as proof for another.
    #[test]
    fn the_challenge_binds_the_oracle_id_and_the_key() {
        let a = Target {
            oracle_id: "oracle-a".to_string(),
            expected_public_key: key_from(KEY_A),
            forbid: Vec::new(),
            custody_inventory_waived: false,
        };
        let b = Target {
            oracle_id: "oracle-b".to_string(),
            expected_public_key: key_from(KEY_A),
            forbid: Vec::new(),
            custody_inventory_waived: false,
        };
        let c = Target {
            oracle_id: "oracle-a".to_string(),
            expected_public_key: key_from(KEY_B),
            forbid: Vec::new(),
            custody_inventory_waived: false,
        };

        assert_ne!(
            challenge_bytes(&a),
            challenge_bytes(&b),
            "oracle id unbound"
        );
        assert_ne!(
            challenge_bytes(&a),
            challenge_bytes(&c),
            "public key unbound"
        );
        assert!(challenge_bytes(&a).starts_with(CHALLENGE_DOMAIN));
    }

    /// The oracle domain must differ from the producer's, or a producer
    /// proof-of-possession transcript could be replayed as an oracle one.
    #[test]
    fn the_challenge_domain_is_not_the_producer_domain() {
        assert_ne!(
            CHALLENGE_DOMAIN,
            b"wrfcoin/core4/producer-key-proof-of-possession/v1"
        );
    }

    #[test]
    fn the_compiled_producer_pins_parse_and_are_usable_as_the_forbidden_set() {
        let keys = pinned_producer_keys().expect("compiled producer pins must parse");
        assert!(
            !keys.is_empty(),
            "an empty forbidden set would make the custody rule vacuous"
        );
    }
}
