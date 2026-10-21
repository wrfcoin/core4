//! Non-destructive proof-of-possession harness for the genesis producer keys.
//!
//! This is deliberately a binary, not an ignored hardware test. Tests may skip
//! when a token is unavailable; a ceremony gate may not. Every invocation
//! either prints a complete PASS transcript and exits 0, or prints a loud
//! failure and exits non-zero.
//!
//! The signer is resolved through the same production role path as
//! `launch_api`, with the PIN injected from a hidden interactive prompt:
//! `load_block_signer_for_with_pin(SignerEnv::producer(), pin)`. Constructing
//! a `Pkcs11Signer` directly would prove only a helper and would leave the
//! producer-specific label/fallback seam untested.

#[path = "launch_api/genesis_producer_pins.rs"]
mod genesis_producer_pins;

use anyhow::{Context, Result, anyhow, bail};
use dialoguer::Password;
use serde::Deserialize;
use std::ffi::CString;
use std::process::{Command, ExitCode, Output};
use wrfcoin_shared_protocol::crypto::signature::{PublicKey, Signature};
use wrfcoin_shared_protocol::crypto::signer::{
    self as protocol_signer, SignerEnv, SignerMode, SignerProvenance,
};
use zeroize::Zeroizing;

const PRODUCER_KEY_LABEL_9A: &str = "Private key for PIV Authentication";
const PRODUCER_PUBKEY_LABEL_9A: &str = "Public key for PIV Authentication";
const YUBICO_MODULE_PATH: &str = "/usr/lib/x86_64-linux-gnu/libykcs11.so";
const CHALLENGE_DOMAIN: &[u8] = b"wrfcoin/core4/producer-key-proof-of-possession/v1";
const GENESIS_BLOCK_HEIGHT: u64 = 1;
const BUILD_REVISION: &str = match option_env!("WRFCOIN_BUILD_REVISION") {
    Some(revision) => revision,
    None => "UNSTAMPED",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)] // secret-leak-ok: token serial is a public hardware identifier, not secret material
struct Target {
    producer_id: &'static str,
    role: &'static str,
    token_serial: &'static str,
}

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
    payout_address: String,
    public_key: String,
}

const PRIMARY: Target = Target {
    producer_id: "n5-node-a",
    role: "PRIMARY",
    token_serial: "35278470",
};
const STANDBY: Target = Target {
    producer_id: "producer-standby",
    role: "STANDBY",
    token_serial: "35278471",
};

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

#[derive(Debug)] // secret-leak-ok: pin_retry_status is a public retry-budget classification, not a PIN
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
        // secret-leak-ok: pin_retry_status renders only a fixed public retry-budget banner
        "PRODUCER KEY PROOF-OF-POSSESSION: FAIL\n{}\n{:#}\n\
         No proof-of-possession transcript was certified.",
        error.pin_retry_status.banner(),
        error.source
    )
}

fn run(args: impl IntoIterator<Item = String>) -> std::result::Result<String, RunFailure> {
    // Keep every operation that cannot authenticate to the token inside this
    // closure. Its single error boundary is the structural backstop that makes
    // future pre-login failures report an explicit zero-retry classification.
    let (target, member, pinned_public_key, checked_env, preflight) = (|| -> Result<_> {
        let target = parse_target(args)?;
        if BUILD_REVISION == "UNSTAMPED" {
            bail!(
                // secret-leak-ok: fixed pre-login retry-budget text; no PIN has been read
                "UNSTAMPED BUILD — ZERO PIN RETRIES CONSUMED. Rebuild with \
                     WRFCOIN_BUILD_REVISION set to the frozen git SHA; refusing to certify a stale \
                     or unidentified binary"
            );
        }
        let member = pinned_testnet_member(target.producer_id)?;
        let pinned_public_key = PublicKey::from_hex(&member.public_key)
            .context("compiled genesis producer key is not a valid ed25519 public key")?;
        let checked_env = validate_hardware_environment(target)?;
        let preflight = run_read_only_preflight(target, &checked_env)?;
        Ok((target, member, pinned_public_key, checked_env, preflight))
    })()
    .map_err(RunFailure::before_login)?;

    // Prompt failure is also before PKCS#11 login. Only after a PIN has been
    // returned do we cross into the conservative "one retry may be spent"
    // classification.
    let pin = prompt_pin_from_tty(target).map_err(RunFailure::before_login)?;
    run_after_prompt(
        target,
        member,
        pinned_public_key,
        checked_env,
        preflight,
        pin,
    )
    .map_err(RunFailure::after_prompt)
}

fn run_after_prompt(
    target: Target,
    member: PinnedMember,
    pinned_public_key: PublicKey,
    checked_env: CheckedEnvironment,
    preflight: PreflightEvidence,
    pin: Zeroizing<String>,
) -> Result<String> {
    // This exact call is the production loading seam used by launch_api #2399.
    // Do not replace it with Pkcs11Config::from_env or Pkcs11Signer::new: those
    // bypass SignerEnv::producer and would recreate #2435 while reading green.
    let resolved = protocol_signer::load_block_signer_for_with_pin(SignerEnv::producer(), pin)
        .context("production producer-signer loading path failed")?;

    if resolved.mode != SignerMode::Hsm {
        bail!(
            // secret-leak-ok: interpolates public keys and producer id only; private material is never loaded into this process
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
    if loaded_public_key != pinned_public_key {
        bail!(
            // secret-leak-ok: interpolates public keys and producer id only; private material is never loaded into this process
            "KEY PRESENT BUT NOT THE PINNED PAIR: production loader returned public key {}, but \
             genesis_producers.rs pins {} for {}. Check the inserted token, serial, slot 9A \
             labels, and producer/public label pairing",
            loaded_public_key.to_hex(),
            pinned_public_key.to_hex(),
            target.producer_id,
        );
    }

    let challenge = challenge_bytes(target, &pinned_public_key);
    let signature = resolved
        .signer
        .sign_at_height(&challenge, GENESIS_BLOCK_HEIGHT)
        .context("token was found, but signing through the production signer failed")?;

    verify_proof(&pinned_public_key, &challenge, &signature)?;

    Ok(render_transcript(
        target,
        &member,
        &checked_env,
        &preflight,
        &provenance,
        &loaded_public_key,
        &challenge,
        &signature,
    ))
}

fn parse_target(args: impl IntoIterator<Item = String>) -> Result<Target> {
    let mut args = args.into_iter();
    let first = args.next().ok_or_else(|| anyhow!(usage()))?;
    if first == "--help" || first == "-h" {
        bail!(usage());
    }
    if first != "--producer-id" {
        bail!("unknown argument {first:?}\n{}", usage());
    }
    let producer_id = args
        .next()
        .ok_or_else(|| anyhow!("--producer-id requires a value\n{}", usage()))?;
    if let Some(extra) = args.next() {
        bail!("unexpected extra argument {extra:?}\n{}", usage());
    }
    match producer_id.as_str() {
        "n5-node-a" => Ok(PRIMARY),
        "producer-standby" => Ok(STANDBY),
        // A seated genesis member with no provisioned token is NOT an unknown
        // id, and saying "unknown" sends the operator to look for a typo
        // instead of at the missing hardware (#2971 review). Distinguish the
        // two by asking the pinned set, so this stays correct as the roster
        // changes rather than tracking a second hand-maintained list.
        //
        // No `Target` is fabricated for such a host. `token_serial` is compared
        // against the attached token (`validate_slot_inventory`), so inventing
        // one would turn a missing prerequisite into a hardware mismatch
        // reported several steps later — the same class of defect as the
        // placeholder payout address this roster already guards against.
        other if pinned_testnet_member(other).is_ok() => bail!(
            // secret-leak-ok: fixed public text naming a seated producer_id and the ABSENCE of a
            // provisioned token; "token serial" here is a public hardware identifier (see the
            // `Target` derive above) and no PIN, private key or serial value is in scope
            "producer_id {producer_id:?} IS seated in the compiled genesis set, but no YubiKey \
             token serial is registered for it here, so this tool has nothing to certify \
             against.\n\n\
             This is not a bad argument. If the host's producer key is SoftHSM-backed \
             (mango-mini, 2026-09-10: PKCS#11 object `wrfcoin-producer-mango`), its proof of \
             possession is the token-side EDDSA verify recorded in \
             docs/security/PRODUCER_KEY_CEREMONY.md and this YubiKey tool does not apply. If \
             the host is meant to hold a YubiKey, run the key ceremony for it, then add its \
             `Target` — producer_id, role and the real token serial — beside PRIMARY and \
             STANDBY in this file and list it in `usage()`.\n{}",
            usage()
        ),
        _ => bail!(
            "unknown producer_id {producer_id:?}; it is not seated in the compiled genesis \
             producer set\n{}",
            usage()
        ),
    }
}

fn usage() -> &'static str {
    "usage: producer_key_pop --producer-id <n5-node-a|producer-standby>\n\
     seated on SoftHSM, not certifiable by this YubiKey tool: mango-mini\n\
     identity check: producer_key_pop --build-revision"
}

struct CheckedEnvironment {
    module_path: String,
    token_label: String,
    private_key_label: String,
    public_key_label: String,
}

fn validate_hardware_environment(target: Target) -> Result<CheckedEnvironment> {
    use protocol_signer::env_vars;

    let module_path = required_env(env_vars::HSM_MODULE_PATH)?;
    let token_label = required_env(env_vars::HSM_TOKEN_LABEL)?;
    let private_key_label = required_env(env_vars::HSM_PRODUCER_KEY_LABEL)?;
    let public_key_label = required_env(env_vars::HSM_PRODUCER_PUBKEY_LABEL)?;
    validate_hardware_selection(
        target,
        &module_path,
        &token_label,
        &private_key_label,
        &public_key_label,
    )?;

    // determinism-ok: ceremony-startup credential exclusion only; no value is read and the harness performs no consensus transition
    if let Ok(pin_env_name) = std::env::var(env_vars::HSM_PIN_ENV) {
        if env_name_is_set(&pin_env_name)? {
            bail!(
                // secret-leak-ok: interpolates the public environment-variable name, never its PIN value
                "PIN VALUE ENVIRONMENT DETECTED in {pin_env_name}. ZERO PIN RETRIES CONSUMED. \
                 Unset it before continuing; this ceremony accepts the PIN only from its hidden \
                 /dev/tty prompt"
            );
        }
    }
    // The documented production default must also be absent even when the
    // indirection variable itself was removed.
    if env_name_is_set("WRFCOIN_HSM_PIN")? {
        bail!(
            // secret-leak-ok: fixed public environment-variable name; the PIN value is never read
            "PIN VALUE ENVIRONMENT DETECTED in WRFCOIN_HSM_PIN. ZERO PIN RETRIES CONSUMED. Unset \
             it before continuing; this ceremony accepts the PIN only from its hidden /dev/tty \
             prompt"
        );
    }

    for forbidden in [
        env_vars::PRODUCER_SIGNER_PRIVATE_KEY,
        env_vars::PRODUCER_SIGNER_KEY_FILE,
    ] {
        // determinism-ok: ceremony-startup fallback exclusion only; it cannot affect consensus or state because the harness exits after one proof
        if std::env::var_os(forbidden).is_some() {
            bail!(
                "{forbidden} is set; remove debug/key-file fallbacks before the hardware proof so \
                 the transcript certifies only PKCS#11 production loading"
            );
        }
    }

    for rotation_var in [
        env_vars::ROTATION_OLD_PUBKEY,
        env_vars::ROTATION_NEW_PUBKEY,
        env_vars::ROTATION_EFFECTIVE_HEIGHT,
        env_vars::ROTATION_OVERLAP_BLOCKS,
        env_vars::ROTATION_POP,
    ] {
        // determinism-ok: ceremony-startup rotation exclusion only; it cannot affect consensus or state because the harness exits after one proof
        if std::env::var_os(rotation_var).is_some() {
            bail!(
                "{rotation_var} is set; producer-key rotation is outside this genesis possession \
                 proof and must not alter the fixed height-1 challenge"
            );
        }
    }

    Ok(CheckedEnvironment {
        module_path,
        token_label,
        private_key_label,
        public_key_label,
    })
}

struct PreflightEvidence {
    token_serial: String,
    token_label: String,
    public_key_label: String,
    public_key_id: String,
}

/// A token selected from the read-only inventory by its recorded serial.
///
/// The slot is only used immediately after the inventory has proved that this
/// exact label/serial pair occurs once. Passing both the label and this slot to
/// `pkcs11-tool` prevents a second, same-label token from being selected.
#[derive(Debug, PartialEq, Eq)]
struct TokenSelection {
    slot: String,
    label: String,
    serial: String,
}

fn env_name_is_set(name: &str) -> Result<bool> {
    let name = CString::new(name).context("PIN env-var name contains an embedded NUL")?;
    // SAFETY: getenv only checks whether the named entry exists. The pointed-to
    // value is never dereferenced or copied, so a prohibited PIN value does not
    // enter Rust memory while the harness refuses the environment.
    Ok(unsafe { !libc::getenv(name.as_ptr()).is_null() })
}

fn run_read_only_preflight(
    target: Target,
    environment: &CheckedEnvironment,
) -> Result<PreflightEvidence> {
    let slots = run_pkcs11_tool(&["--module", &environment.module_path, "--list-token-slots"])?;
    let slots_text = output_text(&slots);
    if !slots.status.success() {
        bail!(
            // secret-leak-ok: pre-login tool output contains only public token inventory and diagnostics, never the PIN
            "PRE-FLIGHT TOOL FAILURE — ZERO PIN RETRIES CONSUMED: pkcs11-tool could not list token \
             slots without login.\n{slots_text}"
        );
    }
    let selected = validate_slot_inventory(target, &environment.token_label, &slots_text)?;

    let objects = run_pkcs11_tool(&[
        "--module",
        &environment.module_path,
        "--token-label",
        &environment.token_label,
        "--slot",
        &selected.slot,
        "--list-objects",
        "--type",
        "pubkey",
    ])?;
    let objects_text = output_text(&objects);
    if !objects.status.success() {
        bail!(
            // secret-leak-ok: pre-login public-object inventory contains no credential because none was supplied
            "PRE-FLIGHT WRONG TOKEN OR LABEL — ZERO PIN RETRIES CONSUMED: pkcs11-tool could not \
             enumerate public objects without login for token {:?}.\n{objects_text}",
            environment.token_label,
        );
    }
    let (public_key_label, public_key_id) =
        validate_public_object_inventory(&environment.public_key_label, &objects_text)?;

    Ok(PreflightEvidence {
        token_serial: selected.serial,
        token_label: selected.label,
        public_key_label,
        public_key_id,
    })
}

fn validate_slot_inventory(
    target: Target,
    expected_label: &str,
    inventory: &str,
) -> Result<TokenSelection> {
    let records = token_inventory_records(inventory);
    let expected: Vec<_> = records
        .iter()
        .filter(|record| record.label == expected_label && record.serial == target.token_serial)
        .collect();
    let serial_under_other_label = records
        .iter()
        .any(|record| record.serial == target.token_serial && record.label != expected_label);
    if expected.len() != 1 || serial_under_other_label {
        bail!(
            // secret-leak-ok: no-login inventory contains public token labels and serial numbers only
            "PRE-FLIGHT WRONG TOKEN — ZERO PIN RETRIES CONSUMED: expected exactly one token record, \
             {} serial {} with token label {:?}, and no other label may carry that serial; observed \
             {:?}. Identify each token with --list-token-slots and do not enter a PIN.\n\
             {inventory}",
            target.role,
            target.token_serial,
            expected_label,
            records,
        );
    }
    let record = expected
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("internal error: validated token inventory was empty"))?;
    Ok(TokenSelection {
        slot: record.slot.clone(),
        label: record.label.clone(),
        serial: record.serial.clone(),
    })
}

#[derive(Debug, PartialEq, Eq)]
struct TokenInventoryRecord {
    slot: String,
    label: String,
    serial: String,
}

fn token_inventory_records(inventory: &str) -> Vec<TokenInventoryRecord> {
    inventory
        .split("Slot ")
        .filter_map(|record| {
            let slot = record
                .lines()
                .next()?
                .trim()
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>();
            if slot.is_empty() {
                return None;
            }
            let mut label = None;
            let mut serial = None;
            for line in record.lines() {
                let Some((field, value)) = line.split_once(':') else {
                    continue;
                };
                match field.trim().to_ascii_lowercase().as_str() {
                    "token label" => label = Some(value.trim().to_string()),
                    "serial num" => serial = Some(value.trim().to_string()),
                    _ => {},
                }
            }
            label
                .zip(serial)
                .map(|(label, serial)| TokenInventoryRecord {
                    slot,
                    label,
                    serial,
                })
        })
        .collect()
}

fn validate_public_object_inventory(
    expected_label: &str,
    inventory: &str,
) -> Result<(String, String)> {
    public_object_with_label_and_id(inventory, expected_label, "01").ok_or_else(|| {
        anyhow!(
            // secret-leak-ok: pre-login inventory contains only public token labels and object IDs
            "PRE-FLIGHT WRONG LABEL/SLOT — ZERO PIN RETRIES CONSUMED: expected public object label \
             {:?} with CKA_ID 01 (PIV 9A), but the read-only inventory did not contain that exact \
             pair. Slot 9C is CKA_ID 02 and must not be selected.\n{inventory}",
            expected_label,
        )
    })
}

fn run_pkcs11_tool(args: &[&str]) -> Result<Output> {
    Command::new("pkcs11-tool").args(args).output().context(
        "PRE-FLIGHT TOOL FAILURE — ZERO PIN RETRIES CONSUMED: could not execute pkcs11-tool",
    )
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

fn public_object_with_label_and_id(
    inventory: &str,
    expected_label: &str,
    expected_id: &str,
) -> Option<(String, String)> {
    inventory
        .split("Public Key Object")
        .skip(1)
        .find_map(|record| {
            let mut label = None;
            let mut id = None;
            for line in record.lines() {
                let Some((field, value)) = line.split_once(':') else {
                    continue;
                };
                match field.trim().to_ascii_lowercase().as_str() {
                    "label" => label = Some(value.trim()),
                    "id" => id = Some(value.trim()),
                    _ => {},
                }
            }
            match (label, id) {
                (Some(label), Some(id)) if label == expected_label && id == expected_id => {
                    Some((label.to_string(), id.to_string()))
                },
                _ => None,
            }
        })
}

fn prompt_pin_from_tty(target: Target) -> Result<Zeroizing<String>> {
    eprintln!(
        // secret-leak-ok: fixed warning plus public role and serial only; the PIN is never interpolated
        "\nREAD-ONLY PRE-FLIGHT PASSED — ZERO PIN RETRIES CONSUMED.\n\
         The next prompt performs the ONE authenticated attempt for {} serial {}.\n\
         A wrong PIN can consume one of only three retries. On any failure after this prompt,\n\
         STOP and DO NOT RE-RUN.",
        target.role, target.token_serial
    );
    let pin = Password::new()
        .with_prompt("YubiKey PIV PIN (input hidden; never stored in env, argv, file, or history)")
        .allow_empty_password(false)
        .interact()
        .context(
            "interactive hidden PIN prompt failed before PKCS#11 login; ZERO PIN RETRIES CONSUMED",
        )?;
    Ok(Zeroizing::new(pin))
}

fn validate_hardware_selection(
    target: Target,
    module_path: &str,
    token_label: &str,
    private_key_label: &str,
    public_key_label: &str,
) -> Result<()> {
    use protocol_signer::env_vars;

    if module_path != YUBICO_MODULE_PATH {
        bail!(
            // secret-leak-ok: token label and serial are public hardware identifiers recorded by the ceremony
            "{} must be exactly {YUBICO_MODULE_PATH:?} for the recorded YubiKey ceremony, got \
             {module_path:?}; refusing to certify SoftHSM, OpenSC, or an unrecorded module",
            env_vars::HSM_MODULE_PATH,
        );
    }
    if !token_label.contains(target.token_serial) {
        bail!(
            // secret-leak-ok: prints only the configured PKCS#11 object label, never private-key material
            "WRONG TOKEN CONFIGURED: {}={token_label:?} does not contain the {} token serial {}",
            env_vars::HSM_TOKEN_LABEL,
            target.role,
            target.token_serial,
        );
    }
    if private_key_label != PRODUCER_KEY_LABEL_9A {
        bail!(
            // secret-leak-ok: prints only the configured PKCS#11 object label, never private-key material
            "{} must select slot 9A exactly as {PRODUCER_KEY_LABEL_9A:?}, got \
             {private_key_label:?}; slot 9C is the snapshot signer and must not be touched",
            env_vars::HSM_PRODUCER_KEY_LABEL,
        );
    }
    if public_key_label != PRODUCER_PUBKEY_LABEL_9A {
        bail!(
            "{} must select slot 9A exactly as {PRODUCER_PUBKEY_LABEL_9A:?}, got \
             {public_key_label:?}; no shared-label fallback is permitted in this proof",
            env_vars::HSM_PRODUCER_PUBKEY_LABEL,
        );
    }
    Ok(())
}

fn required_env(name: &str) -> Result<String> {
    // determinism-ok: ceremony-startup configuration read only; the short-lived proof harness performs no consensus transition or state mutation
    let value = std::env::var(name).map_err(|_| anyhow!("{name} is not set"))?;
    if value.trim().is_empty() {
        bail!("{name} is empty");
    }
    Ok(value)
}

fn pinned_testnet_member(producer_id: &str) -> Result<PinnedMember> {
    let raw = genesis_producer_pins::TESTNET_GENESIS_PRODUCER_SET.ok_or_else(|| {
        anyhow!(
            // secret-leak-ok: fixed error text about a public genesis key pin; no PIN or private key is in scope
            "no testnet genesis producer set is compiled into this binary; refusing to certify a \
             key against an absent pin"
        )
    })?;
    let set: PinnedSet = serde_json::from_str(raw)
        .context("compiled testnet genesis producer set is invalid JSON")?;
    if set.version != 1 {
        bail!(
            // secret-leak-ok: strict verification failure prints only the pinned public key
            "compiled testnet genesis producer set has unsupported version {}",
            set.version
        );
    }
    set.members
        .into_iter()
        .find(|member| member.producer_id == producer_id)
        .ok_or_else(|| {
            anyhow!(
                "producer_id {producer_id:?} is not present in the compiled testnet genesis \
                 producer set"
            )
        })
}

fn challenge_bytes(target: Target, pinned_public_key: &PublicKey) -> Vec<u8> {
    let mut challenge = Vec::with_capacity(
        CHALLENGE_DOMAIN.len()
            + 1
            + target.producer_id.len()
            + 1
            + pinned_public_key.as_bytes().len(),
    );
    challenge.extend_from_slice(CHALLENGE_DOMAIN);
    challenge.push(0);
    challenge.extend_from_slice(target.producer_id.as_bytes());
    challenge.push(0);
    challenge.extend_from_slice(pinned_public_key.as_bytes());
    challenge
}

fn verify_proof(
    pinned_public_key: &PublicKey,
    challenge: &[u8],
    signature: &Signature,
) -> Result<()> {
    if !pinned_public_key.verify(challenge, signature) {
        bail!(
            // secret-leak-ok: strict verification failure prints only the pinned public key
            "KEY PRESENT BUT PRIVATE HALF DOES NOT MATCH THE PINNED KEY: signature failed strict \
             verification under genesis public key {}",
            pinned_public_key.to_hex()
        );
    }

    let mut tampered = challenge.to_vec();
    let last = tampered
        .last_mut()
        .ok_or_else(|| anyhow!("internal error: possession challenge is empty"))?;
    *last ^= 0x01;
    if pinned_public_key.verify(&tampered, signature) {
        bail!(
            "internal verification failure: the signature also verified after the challenge was \
             tampered"
        );
    }
    Ok(())
}

struct Pkcs11Provenance {
    module_path: String,
    token_label: String,
    key_label: String,
}

fn render_transcript(
    target: Target,
    member: &PinnedMember,
    environment: &CheckedEnvironment,
    preflight: &PreflightEvidence,
    provenance: &Pkcs11Provenance,
    loaded_public_key: &PublicKey,
    challenge: &[u8],
    signature: &Signature,
) -> String {
    format!(
        // secret-leak-ok: ceremony transcript contains public identifiers, public keys, challenge, and signature; it never includes the PIN or private key
        "## Producer key proof-of-possession transcript\n\n\
         - Result: **PASS**\n\
         - Producer ID: `{producer_id}`\n\
         - Role: `{role}`\n\
         - Harness build revision: `{build_revision}`\n\
         - Pinned payout address: `{payout_address}` (member identity only; not used for signing)\n\
         - Expected YubiKey serial: `{serial}`\n\
         - Production loader: `load_block_signer_for_with_pin(SignerEnv::producer(), interactive_pin)`\n\
         - Signer mode: `Hsm`\n\
         - Signer provenance: `Pkcs11`\n\
         - PIN input: hidden interactive terminal prompt; not environment, argv, file, or shell history\n\
         - Read-only pre-flight `--list-token-slots`: **PASS** — serial `{preflight_serial}`, label `{preflight_token}`\n\
         - Read-only pre-flight `--list-objects --type pubkey`: **PASS** — label `{preflight_public_label}`, CKA_ID `{preflight_public_id}`\n\
         - PKCS#11 module (environment): `{env_module}`\n\
         - PKCS#11 module (resolved provenance): `{provenance_module}`\n\
         - Token label (environment): `{env_token}`\n\
         - Token label (resolved provenance): `{provenance_token}`\n\
         - Private-key label: `{private_label}`\n\
         - Resolved private-key label: `{provenance_key}`\n\
         - Public-key label: `{public_label}`\n\
         - Pinned genesis public key: `{pinned_key}`\n\
         - Production-loader public key: `{loaded_key}`\n\
         - Public-key equality: **PASS**\n\
         - Challenge domain: `{domain}`\n\
         - Signing height: `{height}`\n\
         - Challenge hex: `{challenge_hex}`\n\
         - Signature hex: `{signature_hex}`\n\
         - Signature verifies under pinned genesis key: **PASS**\n\
         - Software-verifier negative control (tampered challenge rejected): **PASS**\n\
         - By-construction at this build revision: no private-key export API is called\n\
         - By-construction at this build revision: no PKCS#11 write/generate/delete API is called\n\
         - Observed slot selection: **9A** — public label `{preflight_public_label}`, CKA_ID `{preflight_public_id}`; slot 9C/CKA_ID 02 was not selected\n",
        producer_id = target.producer_id,
        role = target.role,
        build_revision = BUILD_REVISION,
        payout_address = member.payout_address,
        serial = target.token_serial,
        preflight_serial = preflight.token_serial,
        preflight_token = preflight.token_label,
        preflight_public_label = preflight.public_key_label,
        preflight_public_id = preflight.public_key_id,
        env_module = environment.module_path,
        provenance_module = provenance.module_path,
        env_token = environment.token_label,
        provenance_token = provenance.token_label,
        private_label = environment.private_key_label,
        provenance_key = provenance.key_label,
        public_label = environment.public_key_label,
        pinned_key = member.public_key,
        loaded_key = loaded_public_key.to_hex(),
        domain = String::from_utf8_lossy(CHALLENGE_DOMAIN),
        height = GENESIS_BLOCK_HEIGHT,
        challenge_hex = hex::encode(challenge),
        signature_hex = signature.to_hex(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::crypto::signature::PrivateKey;

    #[test]
    fn only_provisioned_genesis_members_are_selectable() {
        assert_eq!(
            parse_target(["--producer-id".into(), "n5-node-a".into()]).unwrap(),
            PRIMARY
        );
        assert_eq!(
            parse_target(["--producer-id".into(), "producer-standby".into()]).unwrap(),
            STANDBY
        );
        assert!(parse_target(std::iter::empty()).is_err());
    }

    /// A seated member with no token and a genuinely unknown id must fail
    /// **differently**.
    ///
    /// Collapsing them is what the #2971 review caught: `mango-mini` is a
    /// compiled genesis member, so reporting it as an unknown producer_id sends
    /// the operator hunting for a typo while the real cause is that no YubiKey
    /// was ever provisioned for that host. The distinction is the whole value of
    /// the message, so it is asserted rather than left to inspection.
    #[test]
    fn a_seated_member_without_a_token_is_not_reported_as_unknown() {
        let seated_no_token = parse_target(["--producer-id".into(), "mango-mini".into()])
            .expect_err("mango-mini has no provisioned token, so it cannot be certified")
            .to_string();
        assert!(
            // secret-leak-ok: asserts on fixed public error text about a MISSING token; the
            // value under test is an error message, not credential material
            seated_no_token.contains("IS seated in the compiled genesis set"),
            "a seated-but-unprovisioned host must say so; got: {seated_no_token}"
        );
        assert!(
            // secret-leak-ok: asserts on fixed public error text about a MISSING token; no
            // serial, PIN or private key is in scope
            seated_no_token.contains("no YubiKey"),
            "the message must name the missing prerequisite; got: {seated_no_token}"
        );

        let unknown = parse_target(["--producer-id".into(), "not-a-member".into()])
            .expect_err("an unseated id must be rejected")
            .to_string();
        assert!(
            unknown.contains("not seated in the compiled genesis"),
            "an unseated id must be reported as unseated; got: {unknown}"
        );
        assert!(
            !unknown.contains("IS seated"),
            "an unseated id must not borrow the seated-host message; got: {unknown}"
        );
    }

    /// Every seated member is either certifiable or refused with the
    /// missing-token reason — never silently absent from the harness.
    ///
    /// This is the part that survives the roster changing: when mango's ceremony
    /// lands and its `Target` is added, this keeps passing; if a FOURTH seat is
    /// added and nobody touches this file, it still passes for the right reason
    /// rather than the harness quietly not covering it.
    #[test]
    fn every_seated_member_is_either_certifiable_or_explains_itself() {
        let raw = genesis_producer_pins::TESTNET_GENESIS_PRODUCER_SET.expect("pinned");
        let set: PinnedSet = serde_json::from_str(raw).expect("pinned set parses");
        for member in &set.members {
            match parse_target(["--producer-id".into(), member.producer_id.clone()]) {
                Ok(target) => assert_eq!(target.producer_id, member.producer_id),
                Err(e) => {
                    let msg = e.to_string();
                    assert!(
                        // secret-leak-ok: asserts on fixed public error text naming a seated
                        // producer_id and the absence of a token; no secret value is in scope
                        msg.contains("IS seated in the compiled genesis set"),
                        "{}: a seated member must be refused with the missing-token reason, \
                         not as unknown; got: {msg}",
                        member.producer_id
                    );
                },
            }
        }
    }

    #[test]
    fn top_level_failure_backstop_always_reports_retry_budget() {
        let before_login = RunFailure::before_login(anyhow!("representative pre-login failure"));
        let rendered = render_failure(&before_login);
        assert!(rendered.contains("ZERO PIN RETRIES CONSUMED"));
        assert!(!rendered.contains("ONE PIN RETRY MAY HAVE BEEN CONSUMED"));

        let after_prompt =
            RunFailure::after_prompt(anyhow!("representative authenticated-path failure"));
        let rendered = render_failure(&after_prompt);
        assert!(rendered.contains("ONE PIN RETRY MAY HAVE BEEN CONSUMED"));
        assert!(rendered.contains("STOP AND DO NOT RE-RUN"));
        assert!(!rendered.contains("ZERO PIN RETRIES CONSUMED"));

        let usage_failure = run(std::iter::empty()).unwrap_err();
        assert_eq!(
            // secret-leak-ok: compares a public retry-budget enum; assertion output contains no PIN
            usage_failure.pin_retry_status,
            PinRetryStatus::ZeroConsumed,
            "argument failures must remain on the structural pre-login side of the boundary"
        );
    }

    #[test]
    fn harness_reads_the_exact_keys_compiled_into_genesis() {
        let primary = pinned_testnet_member(PRIMARY.producer_id).unwrap();
        let standby = pinned_testnet_member(STANDBY.producer_id).unwrap();
        assert_eq!(
            primary.public_key,
            "1e5180e7add06bbe07cf2698ec9d4882ae1cfb54b13de67bae92a4e625ceec1a" // pragma: allowlist secret -- public genesis key
        );
        assert_eq!(
            standby.public_key,
            "cd77a1f439ec6896727f585e81228795c8008c8f8db7763bb9c42afa8d119aaa" // pragma: allowlist secret -- public genesis key
        );
    }

    #[test]
    fn strict_verifier_accepts_the_right_private_half_and_rejects_tampering() {
        let private_key = PrivateKey::generate(&mut rand::thread_rng());
        let public_key = PublicKey::from_private_key(&private_key);
        let challenge = challenge_bytes(PRIMARY, &public_key);
        let signature = private_key.sign(&challenge);
        verify_proof(&public_key, &challenge, &signature).unwrap();

        let wrong_public_key =
            PublicKey::from_private_key(&PrivateKey::generate(&mut rand::thread_rng()));
        assert!(verify_proof(&wrong_public_key, &challenge, &signature).is_err());
    }

    #[test]
    fn wrong_token_and_slot_9c_are_loud_failures() {
        let wrong_token = validate_hardware_selection(
            PRIMARY,
            YUBICO_MODULE_PATH,
            "YubiKey PIV #35278471",
            PRODUCER_KEY_LABEL_9A,
            PRODUCER_PUBKEY_LABEL_9A,
        )
        .unwrap_err();
        assert!(wrong_token.to_string().contains("WRONG TOKEN CONFIGURED")); // secret-leak-ok: asserts on a public token-status message, not a credential

        let slot_9c = validate_hardware_selection(
            PRIMARY,
            YUBICO_MODULE_PATH,
            "YubiKey PIV #35278470",
            "Private key for Digital Signature",
            "Public key for Digital Signature",
        )
        .unwrap_err();
        assert!(slot_9c.to_string().contains("slot 9C"));
    }

    #[test]
    fn read_only_preflight_distinguishes_wrong_token_and_wrong_slot_without_login() {
        let slots = "Slot 0 (0x0): Yubico YubiKey OTP+FIDO+CCID 01 00\n\
  token label        : YubiKey PIV #35278470\n\
  serial num         : 35278470\n";
        assert_eq!(
            // secret-leak-ok: test; `TokenSelection` holds public no-login slot/label/serial
            validate_slot_inventory(PRIMARY, "YubiKey PIV #35278470", slots).unwrap(),
            TokenSelection {
                slot: "0".to_string(),
                label: "YubiKey PIV #35278470".to_string(),
                serial: "35278470".to_string(),
            }
        );
        let wrong_token =
            validate_slot_inventory(STANDBY, "YubiKey PIV #35278471", slots).unwrap_err();
        assert!(
            // secret-leak-ok: fixed classification from public no-login inventory only
            wrong_token.to_string().contains("PRE-FLIGHT WRONG TOKEN")
        );
        assert!(
            // secret-leak-ok: fixed retry-budget text from public no-login inventory only
            wrong_token
                .to_string()
                .contains("ZERO PIN RETRIES CONSUMED")
        );
        let two_tokens = format!(
            // secret-leak-ok: test; fixture of public no-login PKCS#11 slot inventory
            "Slot 0 (0x0): Yubico YubiKey OTP+FIDO+CCID 01 00\n\
  token label        : YubiKey PIV #38072452\n\
  serial num         : 38072452\n\
Slot 1 (0x1): Yubico YubiKey OTP+FIDO+CCID 01 00\n\
  token label        : YubiKey PIV #35278470\n\
  serial num         : 35278470\n"
        );
        let selected = validate_slot_inventory(PRIMARY, "YubiKey PIV #35278470", &two_tokens)
            .expect("the expected token may be selected while an attester token is also attached");
        assert_eq!(
            selected.slot, "1",
            "the selected slot is anchored to the expected serial"
        );

        let duplicate = format!(
            // secret-leak-ok: test; fixture of public no-login PKCS#11 slot inventory
            "{slots}Slot 1 (0x1): Yubico YubiKey OTP+FIDO+CCID 01 00\n\
  token label        : YubiKey PIV #35278470\n\
  serial num         : 35278470\n"
        );
        assert!(validate_slot_inventory(PRIMARY, "YubiKey PIV #35278470", &duplicate).is_err());
        let serial_under_wrong_label = format!(
            // secret-leak-ok: test; fixture of public no-login PKCS#11 slot inventory
            "{slots}Slot 1 (0x1): Yubico YubiKey OTP+FIDO+CCID 01 00\n\
  token label        : wrong-label\n\
  serial num         : 35278470\n"
        );
        assert!(
            validate_slot_inventory(PRIMARY, "YubiKey PIV #35278470", &serial_under_wrong_label)
                .is_err()
        );

        let objects = "\
Public Key Object; EC_EDWARDS  EC_POINT 255 bits\n\
  label:      Public key for PIV Authentication\n\
  ID:         01\n\
Public Key Object; EC_EDWARDS  EC_POINT 255 bits\n\
  label:      Public key for Digital Signature\n\
  ID:         02\n";
        assert_eq!(
            validate_public_object_inventory(PRODUCER_PUBKEY_LABEL_9A, objects).unwrap(),
            (PRODUCER_PUBKEY_LABEL_9A.to_string(), "01".to_string())
        );
        let slot_9c = validate_public_object_inventory("Public key for Digital Signature", objects)
            .unwrap_err();
        assert!(slot_9c.to_string().contains("PRE-FLIGHT WRONG LABEL/SLOT"));
        assert!(slot_9c.to_string().contains("ZERO PIN RETRIES CONSUMED"));
    }

    #[test]
    fn two_token_inventory_selects_the_expected_serial_3089() {
        let inventory = "Slot 0 (0x0): Yubico YubiKey OTP+FIDO+CCID 01 00\n\
  token label        : YubiKey PIV #38072452\n\
  serial num         : 38072452\n\
Slot 1 (0x1): Yubico YubiKey OTP+FIDO+CCID 01 00\n\
  token label        : YubiKey PIV #35278470\n\
  serial num         : 35278470\n";
        let selected = validate_slot_inventory(PRIMARY, "YubiKey PIV #35278470", inventory)
            .expect("the producer token must remain usable beside the attester token");
        assert_eq!(selected.slot, "1");
        assert_eq!(selected.label, "YubiKey PIV #35278470");
        assert_eq!(selected.serial, "35278470");
    }

    #[test]
    fn missing_required_configuration_is_never_a_skip() {
        let error = required_env("WRFCOIN_TEST_2435_INTENTIONALLY_ABSENT")
            .expect_err("missing hardware configuration must fail");
        assert!(error.to_string().contains("is not set"));
    }
}
