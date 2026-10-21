//! Remote ceremony kit (#3096): `wrfcoin ceremony sign|assemble|custody`.
//!
//! What the 2026-09-10 cut did by hand across node container and the workstation,
//! as three commands an operator runs on their own box:
//!
//! - `sign` — produce this node's key-possession proof for a ceremony bundle
//!   (oracle or producer role) with the production signer loader; the PIN,
//!   when there is one, comes from the hidden `/dev/tty` prompt and nowhere
//!   else — the command refuses to run if any variable that could carry a PIN
//!   is set (see [`pin_env_detected`]).
//! - `assemble` — coordinator side: fold returned proofs into the bundle,
//!   refusing a duplicate identity, an identity the artifact does not
//!   register, or a proof signed over a different artifact; `--verify` runs
//!   the fail-closed possession verifier on the result.
//! - `custody` — validate the checked-in key-custody manifest and, with
//!   `--check`, compare it against every token a PKCS#11 module presents
//!   WITHOUT a PIN (`--features hsm`), or against an observed inventory file.
//!
//! Every command is offline: no launch API, no operator token.

use anyhow::{Context, Result, anyhow, bail};
use clap::{Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use wrfcoin_shared_protocol::blockchain::transaction::OracleAttestationPayload;
use wrfcoin_shared_protocol::crypto::signer::{
    ResolvedSigner, SignerEnv, env_vars, load_block_signer_for,
};
use wrfcoin_shared_protocol::oracle::ceremony::{
    GenesisOraclePossessionBundle, ProducerPossessionProof, assemble_possession_bundle,
    sign_oracle_possession_proof, sign_producer_possession_proof, verify_oracle_possession,
    verify_producer_possession_proof,
};
use wrfcoin_shared_protocol::security::key_custody::{
    CustodyManifest, CustodyReport, ObservedKey,
};

/// Default Yubico PKCS#11 module on the ceremony hosts.
const YUBICO_MODULE_PATH: &str = "/usr/lib/x86_64-linux-gnu/libykcs11.so";

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum CeremonyRole {
    /// Oracle attestation key (`SignerEnv::oracle`).
    Oracle,
    /// Block-producer key (`SignerEnv::producer`).
    Producer,
}

/// `wrfcoin ceremony …`
#[derive(Subcommand)]
pub enum CeremonyCommands {
    /// Produce this node's key-possession proof for a ceremony bundle. The
    /// signer is the production loader for the role (key file, hex env, or
    /// PKCS#11 with the PIN read from /dev/tty). Refuses if any environment
    /// variable that could carry a PIN is set.
    Sign {
        /// `ceremony-bundle.json` from the coordinator
        #[clap(long)]
        bundle: PathBuf,

        /// Which key to prove
        #[clap(long, value_enum)]
        role: CeremonyRole,

        /// Proof output path (`possession-proof-<id>.json`)
        #[clap(long)]
        out: PathBuf,

        /// Proof timestamp (unix seconds); defaults to now
        #[clap(long)]
        timestamp: Option<i64>,
    },

    /// Coordinator: fold returned proofs into the bundle. Refuses duplicate
    /// identities, identities the artifact does not register, and proofs
    /// signed over a different artifact. `--verify` then runs the fail-closed
    /// possession verifier on the result.
    Assemble {
        /// `ceremony-bundle.json` (may already hold some proofs)
        #[clap(long)]
        bundle: PathBuf,

        /// Proof files returned by operators (repeatable)
        #[clap(long = "proof", value_name = "FILE", required = true)]
        proofs: Vec<PathBuf>,

        /// Assembled bundle output path
        #[clap(long)]
        out: PathBuf,

        /// Run `verify_oracle_possession` on the assembled bundle
        #[clap(long)]
        verify: bool,

        /// Comma-separated producer public keys (hex) a producer proof must
        /// belong to; producer proofs are always signature-checked
        #[clap(long)]
        producers: Option<String>,
    },

    /// Validate the key-custody manifest; with `--check`, compare it against
    /// the tokens a PKCS#11 module presents (no PIN) or an observed inventory.
    Custody {
        /// `docs/security/KEY_CUSTODY_MANIFEST.json`
        #[clap(long)]
        manifest: PathBuf,

        /// Compare against live tokens (or `--observed`)
        #[clap(long)]
        check: bool,

        /// PKCS#11 module path; defaults to WRFCOIN_HSM_MODULE_PATH or the
        /// Yubico module
        #[clap(long)]
        module: Option<PathBuf>,

        /// JSON array of observed keys (the output of a previous `--check`
        /// run, or `enumerate_public_keys`) to compare instead of live tokens
        #[clap(long)]
        observed: Option<PathBuf>,
    },
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, what: &str) -> Result<T> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read {what} {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("failed to parse {what} {}", path.display()))
}

fn write_pretty_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    let body = format!("{}\n", serde_json::to_string_pretty(value)?);
    fs::write(path, body).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// The name of any environment variable that could carry a PIN value. `None`
/// when the environment is clean.
pub fn pin_env_detected() -> Option<String> {
    pin_env_in(std::env::vars_os())
}

/// Environment names that contain `PIN` but are known not to carry a
/// credential. The match is exact and case-sensitive: a lookalike such as
/// `COREPACK_ENABLE_AUTO_PIN2` or `corepack_enable_auto_pin` still refuses.
/// Add a name only with a comment saying who sets it and why it is not a PIN.
const NON_CREDENTIAL_PIN_NAMES: &[&str] = &[
    // Node.js corepack. Developer shells and Node tooling set it to switch off
    // writing the package-manager version into package.json ("pin"). It is a
    // boolean feature switch, never a PIN, and is set on the workstations where
    // ceremonies are run.
    "COREPACK_ENABLE_AUTO_PIN",
];

/// [`pin_env_detected`] over an explicit environment. Value contents are never
/// interpreted: PKCS#11 receives a PIN's raw bytes, so `true`, ` 1 ` or a
/// two-character value is as much a credential as `123456`.
///
/// - `WRFCOIN_HSM_PIN_ENV` (the indirection) trips on any value, even empty.
/// - The variable it names trips on any non-empty value, whatever its name.
/// - Any other name containing `PIN` (ASCII case-insensitive) trips on any
///   non-empty value, unless it is exactly in [`NON_CREDENTIAL_PIN_NAMES`].
///
/// An empty value trips nothing except the indirection: the PKCS#11 reader
/// refuses an empty PIN, so it can carry no credential.
fn pin_env_in<I>(vars: I) -> Option<String>
where
    I: IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
{
    let vars: Vec<(String, std::ffi::OsString)> = vars
        .into_iter()
        .map(|(key, value)| (key.to_string_lossy().into_owned(), value))
        .collect();
    if vars.iter().any(|(name, _)| name == env_vars::HSM_PIN_ENV) {
        return Some(env_vars::HSM_PIN_ENV.to_string());
    }
    vars.into_iter().find_map(|(name, value)| {
        if value.is_empty() || NON_CREDENTIAL_PIN_NAMES.contains(&name.as_str()) {
            return None;
        }
        name.to_ascii_uppercase().contains("PIN").then_some(name)
    })
}

/// Resolve the role's production signer. With `--features hsm` and a
/// configured module, the PIN is read from the hidden /dev/tty prompt; the
/// key-file / hex-env path needs no PIN.
pub(super) fn resolve_signer(role: CeremonyRole) -> Result<ResolvedSigner> {
    let (env, name) = match role {
        CeremonyRole::Oracle => (SignerEnv::oracle(), "oracle"),
        CeremonyRole::Producer => (SignerEnv::producer(), "producer"),
    };
    if let Some(var) = pin_env_detected() {
        // secret-leak-ok: interpolates the public environment-variable NAME, never its value
        bail!(
            // secret-leak-ok: interpolates the env-var NAME only; PIN hit is prose across a line continuation
            "PIN VALUE ENVIRONMENT DETECTED in {var}. Unset it before continuing; this ceremony \
             accepts the PIN only from its hidden /dev/tty prompt (zero PIN retries consumed)"
        );
    }
    #[cfg(feature = "hsm")]
    {
        if std::env::var_os(env_vars::HSM_MODULE_PATH).is_some() {
            let pin = dialoguer::Password::new()
                .with_prompt(format!("{name} token PIN (hidden, /dev/tty)"))
                .interact()
                .context("PIN prompt failed before any PKCS#11 login (zero retries consumed)")?;
            return wrfcoin_shared_protocol::crypto::signer::load_block_signer_for_with_pin(
                env,
                zeroize::Zeroizing::new(pin),
            );
        }
    }
    load_block_signer_for(env)?.ok_or_else(|| {
        anyhow!(
            // secret-leak-ok: interpolates signer/env-var NAMES only; token hit is prose across a line continuation
            "no {name} signer configured: set {} (key file) or {} (hex){}",
            env.key_file_var.unwrap_or("-"),
            env.hex_key_var,
            if cfg!(feature = "hsm") {
                format!(", or {} for a PKCS#11 token", env_vars::HSM_MODULE_PATH)
            } else {
                "; this build has no --features hsm, so a PKCS#11 token needs the in-image \
                 build"
                    .to_string()
            }
        )
    })
}

pub fn handle_ceremony_command(command: &CeremonyCommands) -> Result<Value> {
    match command {
        CeremonyCommands::Sign {
            bundle,
            role,
            out,
            timestamp,
        } => {
            let bundle: GenesisOraclePossessionBundle = read_json(bundle, "ceremony bundle")?;
            let resolved = resolve_signer(*role)?;
            let ts = timestamp.unwrap_or_else(|| chrono::Utc::now().timestamp());
            let public_key = resolved.signer.public_key().to_hex();
            match role {
                CeremonyRole::Oracle => {
                    let proof = sign_oracle_possession_proof(
                        &bundle,
                        &bundle.artifact.chain_id,
                        &bundle.artifact.network_id,
                        &[bundle.artifact.governance_pubkey.clone()],
                        resolved.signer.as_ref(),
                        ts,
                    )
                    .map_err(|e| {
                        anyhow!("ORACLE CEREMONY REFUSED: {e} (local signer {public_key})")
                    })?;
                    write_pretty_json(out, &proof)?;
                    Ok(json!({
                        "summary": format!(
                            "Oracle possession proof for identity {} written (ceremony {})",
                            proof.oracle_id, bundle.ceremony_id
                        ),
                        "role": "oracle",
                        "identity": proof.oracle_id,
                        "publicKey": public_key,
                        "signer": resolved.signer.provenance().to_string(),
                        "ceremonyId": bundle.ceremony_id,
                        "timestamp": ts,
                        "output": out.display().to_string(),
                    }))
                },
                CeremonyRole::Producer => {
                    let proof = sign_producer_possession_proof(&bundle, resolved.signer.as_ref(), ts)
                        .map_err(|e| anyhow!("PRODUCER CEREMONY REFUSED: {e}"))?;
                    write_pretty_json(out, &proof)?;
                    Ok(json!({
                        "summary": format!(
                            "Producer possession proof for {} written (ceremony {})",
                            proof.producer_pubkey, bundle.ceremony_id
                        ),
                        "role": "producer",
                        "publicKey": proof.producer_pubkey,
                        "signer": resolved.signer.provenance().to_string(),
                        "ceremonyId": bundle.ceremony_id,
                        "registrationCommitment": proof.registration_commitment,
                        "timestamp": ts,
                        "output": out.display().to_string(),
                    }))
                },
            }
        },
        CeremonyCommands::Assemble {
            bundle,
            proofs,
            out,
            verify,
            producers,
        } => {
            let bundle: GenesisOraclePossessionBundle = read_json(bundle, "ceremony bundle")?;
            let expected_producers: Vec<String> = producers
                .as_deref()
                .map(|raw| {
                    raw.split(',')
                        .map(|s| s.trim().to_ascii_lowercase())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let mut oracle_proofs: Vec<OracleAttestationPayload> = Vec::new();
            let mut producer_proofs: Vec<ProducerPossessionProof> = Vec::new();
            for path in proofs {
                let raw = fs::read_to_string(path)
                    .with_context(|| format!("failed to read proof {}", path.display()))?;
                if let Ok(proof) = serde_json::from_str::<OracleAttestationPayload>(&raw) {
                    oracle_proofs.push(proof);
                } else if let Ok(proof) = serde_json::from_str::<ProducerPossessionProof>(&raw) {
                    verify_producer_possession_proof(&bundle, &proof, &expected_producers)
                        .map_err(|e| anyhow!("ASSEMBLE REFUSED {}: {e}", path.display()))?;
                    producer_proofs.push(proof);
                } else {
                    bail!(
                        "{} is neither an oracle possession proof nor a producer possession proof",
                        path.display()
                    );
                }
            }
            let assembled = assemble_possession_bundle(&bundle, &oracle_proofs)
                .map_err(|e| anyhow!("ASSEMBLE REFUSED: {e}"))?;
            let verified = if *verify {
                let report = verify_oracle_possession(
                    &assembled,
                    &assembled.artifact.chain_id,
                    &assembled.artifact.network_id,
                    &[assembled.artifact.governance_pubkey.clone()],
                )
                .map_err(|e| anyhow!("ASSEMBLE --verify REFUSED: {e}"))?;
                Some(report)
            } else {
                None
            };
            write_pretty_json(out, &assembled)?;
            Ok(json!({
                "summary": format!(
                    "Assembled ceremony {}: {} oracle proof(s) of {} identities, {} producer proof(s){}",
                    assembled.ceremony_id,
                    assembled.possession_proofs.len(),
                    assembled.artifact.oracle_set.len(),
                    producer_proofs.len(),
                    match &verified {
                        Some(v) => format!(", verified {} operator(s)", v.verified_operator_count),
                        None => String::new(),
                    }
                ),
                "output": out.display().to_string(),
                "ceremonyId": assembled.ceremony_id,
                "oracleProofCount": assembled.possession_proofs.len(),
                "oracleSetSize": assembled.artifact.oracle_set.len(),
                "producerProofs": producer_proofs.iter().map(|p| p.producer_pubkey.clone()).collect::<Vec<_>>(),
                "verified": verified,
            }))
        },
        CeremonyCommands::Custody {
            manifest,
            check,
            module,
            observed,
        } => {
            let manifest_value: CustodyManifest = read_json(manifest, "custody manifest")?;
            manifest_value
                .validate()
                .map_err(|e| anyhow!("custody manifest {} is invalid: {e}", manifest.display()))?;
            if !*check {
                return Ok(json!({
                    "summary": format!(
                        "Custody manifest valid: {} entries ({} with a recorded public key)",
                        manifest_value.entries.len(),
                        manifest_value.entries.iter().filter(|e| !e.public_key.is_empty()).count()
                    ),
                    "entries": manifest_value.entries,
                }));
            }
            let observed_keys: Vec<ObservedKey> = match observed {
                Some(path) => read_json(path, "observed inventory")?,
                None => enumerate_live_tokens(module.as_deref())?,
            };
            let report = CustodyReport::check(&manifest_value, &observed_keys);
            let summary = format!(
                "Custody check: {} match, {} unfilled, {} finding(s) total",
                report.matches(),
                report.unfilled(),
                report.findings.len()
            );
            let body = json!({
                "summary": summary,
                "consistent": report.is_consistent(),
                "observed": observed_keys,
                "report": report,
            });
            if report.is_consistent() {
                Ok(body)
            } else {
                Err(anyhow!(
                    "CUSTODY MISMATCH: the manifest does not match the tokens:\n{}",
                    serde_json::to_string_pretty(&body)?
                ))
            }
        },
    }
}

#[cfg(feature = "hsm")]
fn enumerate_live_tokens(module: Option<&Path>) -> Result<Vec<ObservedKey>> {
    let module: PathBuf = match module {
        Some(m) => m.to_path_buf(),
        None => std::env::var_os(env_vars::HSM_MODULE_PATH)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(YUBICO_MODULE_PATH)),
    };
    let keys = wrfcoin_shared_protocol::crypto::signer::enumerate_public_keys(&module)
        .with_context(|| format!("custody check could not enumerate {}", module.display()))?;
    Ok(keys
        .into_iter()
        .map(|k| ObservedKey {
            token_serial: k.token_serial,
            token_label: k.token_label,
            object_label: k.object_label,
            cka_id: k.cka_id,
            public_key: k.public_key,
        })
        .collect())
}

#[cfg(not(feature = "hsm"))]
fn enumerate_live_tokens(_module: Option<&Path>) -> Result<Vec<ObservedKey>> {
    bail!(
        "this build has no --features hsm, so it cannot read tokens; pass --observed <json> \
         (from an hsm-enabled `custody --check`) or use the in-image build ({YUBICO_MODULE_PATH})"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};
    use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
    use wrfcoin_shared_protocol::oracle::ceremony::GenesisOracleCeremony;
    use wrfcoin_shared_protocol::security::key_custody::{CustodyEntry, CustodyFinding};

    /// Signer env vars are process-global.
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("wrfcoin-ceremony-tests-{name}-{nonce}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn key(seed: u64) -> PrivateKey {
        PrivateKey::generate(&mut StdRng::seed_from_u64(seed))
    }

    fn write_key_file(dir: &Path, name: &str, key: &PrivateKey) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("{}\n", key.to_hex())).expect("write key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod 600");
        }
        path
    }

    /// A two-oracle bundle whose signing keys are `key(11)` and `key(12)`.
    fn bundle(dir: &Path) -> (PathBuf, GenesisOraclePossessionBundle, Vec<PrivateKey>) {
        let mut identities: Vec<String> = (1..=2)
            .map(|seed| PublicKey::from_private_key(&key(seed)).to_hex())
            .collect();
        identities.sort();
        let signing_keys = vec![key(11), key(12)];
        let signing_pubkeys: BTreeMap<String, String> = identities
            .iter()
            .zip(&signing_keys)
            .map(|(id, k)| (id.clone(), PublicKey::from_private_key(k).to_hex()))
            .collect();
        let ceremony = GenesisOracleCeremony::new(
            "777",
            "testnet",
            identities,
            2,
            signing_pubkeys,
            1,
            1_800_000_000,
            1_800_000_100,
        )
        .expect("valid ceremony");
        let bundle = GenesisOraclePossessionBundle {
            ceremony_id: "cut-test-3096".into(),
            artifact: ceremony.build_artifact(&key(90)),
            possession_proofs: Vec::new(),
        };
        let path = dir.join("ceremony-bundle.json");
        write_pretty_json(&path, &bundle).expect("write bundle");
        (path, bundle, signing_keys)
    }

    fn sign_as(dir: &Path, bundle_path: &Path, role: CeremonyRole, k: &PrivateKey, out: &str) -> Value {
        let key_path = write_key_file(dir, &format!("{out}.key"), k);
        let (var, other) = match role {
            CeremonyRole::Oracle => (SignerEnv::oracle().key_file_var.unwrap(), SignerEnv::producer().key_file_var.unwrap()),
            CeremonyRole::Producer => (SignerEnv::producer().key_file_var.unwrap(), SignerEnv::oracle().key_file_var.unwrap()),
        };
        std::env::remove_var(other);
        std::env::set_var(var, &key_path);
        let result = handle_ceremony_command(&CeremonyCommands::Sign {
            bundle: bundle_path.to_path_buf(),
            role,
            out: dir.join(out),
            timestamp: Some(1_800_000_200),
        });
        std::env::remove_var(var);
        result.expect("sign")
    }

    /// Operator token unset (#3051): every command here is offline.
    #[test]
    fn two_software_signers_sign_assemble_and_verify_count_two_3096() {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir("e2e");
        let (bundle_path, bundle, signing_keys) = bundle(&dir);
        let a = sign_as(&dir, &bundle_path, CeremonyRole::Oracle, &signing_keys[0], "proof-a.json");
        let b = sign_as(&dir, &bundle_path, CeremonyRole::Oracle, &signing_keys[1], "proof-b.json");
        assert_eq!(a["role"], json!("oracle"));
        assert_ne!(a["identity"], b["identity"]);

        // A producer proof rides along and is signature-checked.
        let producer = key(21);
        let p = sign_as(&dir, &bundle_path, CeremonyRole::Producer, &producer, "producer-proof.json");
        assert_eq!(p["ceremonyId"], json!(bundle.ceremony_id));
        let producer_hex = PublicKey::from_private_key(&producer).to_hex().to_ascii_lowercase();

        let out = dir.join("ceremony-bundle.final.json");
        let result = handle_ceremony_command(&CeremonyCommands::Assemble {
            bundle: bundle_path.clone(),
            proofs: vec![dir.join("proof-a.json"), dir.join("proof-b.json"), dir.join("producer-proof.json")],
            out: out.clone(),
            verify: true,
            producers: Some(producer_hex.clone()),
        })
        .expect("assemble --verify");
        assert_eq!(result["oracleProofCount"], json!(2));
        assert_eq!(result["verified"]["verified_operator_count"], json!(2));
        assert_eq!(result["producerProofs"], json!([producer_hex]));

        // The wrong producer set refuses the producer proof.
        let err = handle_ceremony_command(&CeremonyCommands::Assemble {
            bundle: bundle_path.clone(),
            proofs: vec![dir.join("proof-a.json"), dir.join("proof-b.json"), dir.join("producer-proof.json")],
            out: out.clone(),
            verify: false,
            producers: Some("deadbeef".into()),
        })
        .expect_err("unexpected producer");
        assert!(err.to_string().contains("not in the expected producer set"), "{err}");

        // A duplicate proof file is refused by identity.
        let err = handle_ceremony_command(&CeremonyCommands::Assemble {
            bundle: bundle_path.clone(),
            proofs: vec![dir.join("proof-a.json"), dir.join("proof-a.json")],
            out: out.clone(),
            verify: false,
            producers: None,
        })
        .expect_err("duplicate identity");
        assert!(err.to_string().contains("duplicate proof"), "{err}");

        // --verify on an incomplete bundle refuses (coverage).
        let err = handle_ceremony_command(&CeremonyCommands::Assemble {
            bundle: bundle_path,
            proofs: vec![dir.join("proof-a.json")],
            out,
            verify: true,
            producers: None,
        })
        .expect_err("one of two");
        assert!(err.to_string().contains("verify REFUSED"), "{err}");
    }

    fn vars(pairs: &[(&str, &str)]) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    }

    /// Synthetic values a value heuristic would wave through (core4#3393
    /// review), plus plain digits. None is a real PIN.
    const PIN_LIKE_VALUES: &[&str] = &[
        "true",
        "false",
        "TRUE",
        " 1  ",
        "\u{e9}\u{e9}",
        "\u{1f600}",
        "\u{e9}\u{e9}\u{e9}",
        "0000",
        "fake-pin-0000",
    ];

    #[test]
    fn pin_guard_exempts_only_the_exact_non_credential_names() {
        for value in ["0", "1", "true", ""] {
            assert_eq!(
                // secret-leak-ok: test asserts env-var NAMES over obviously fake literal values; no PIN is read
                pin_env_in(vars(&[("COREPACK_ENABLE_AUTO_PIN", value)])),
                None,
                "COREPACK_ENABLE_AUTO_PIN={value:?} must not trip the guard"
            );
        }
        for lookalike in [
            "COREPACK_ENABLE_AUTO_PIN2",
            "corepack_enable_auto_pin",
            "Corepack_Enable_Auto_Pin",
            "COREPACK_ENABLE_AUTO_PIN_",
            "XCOREPACK_ENABLE_AUTO_PIN",
        ] {
            assert_eq!(
                // secret-leak-ok: test asserts env-var NAMES over obviously fake literal values; no PIN is read
                pin_env_in(vars(&[(lookalike, "0")])),
                Some(lookalike.to_string()),
                "{lookalike} is not the exempt name and must trip"
            );
        }
        assert_eq!(
            // secret-leak-ok: test asserts env-var NAMES over obviously fake literal values; no PIN is read
            pin_env_in(vars(&[("PATH", "/usr/bin"), ("HOME", "/h")])),
            None
        );
    }

    #[test]
    fn pin_guard_trips_on_any_non_empty_value_of_a_pin_name() {
        for name in [
            "WRFCOIN_HSM_PIN",
            "WRFCOIN_HSM_SO_PIN",
            "WRFCOIN_ATTESTER_HSM_PIN",
            "WRFCOIN_ATTESTER_HSM_SO_PIN",
            "WRFCOIN_SEED_HSM_PIN",
            "WRFCOIN_HSM_PROOF_PIN",
            "PKCS11_PIN",
            "USER_PIN",
            "SO_PIN",
            "HSM_PIN",
            "wrfcoin_hsm_pin",
            "PIN",
            "TOKEN_PIN_VALUE",
        ] {
            for value in PIN_LIKE_VALUES {
                assert_eq!(
                    // secret-leak-ok: test asserts env-var NAMES over obviously fake literal values; no PIN is read
                    pin_env_in(vars(&[("COREPACK_ENABLE_AUTO_PIN", "0"), (name, value)])),
                    Some(name.to_string()),
                    "{name} with a non-empty value must trip the guard"
                );
            }
            // An empty value carries no credential (the PKCS#11 reader refuses it).
            assert_eq!(
                // secret-leak-ok: test asserts env-var NAMES over obviously fake literal values; no PIN is read
                pin_env_in(vars(&[(name, "")])),
                None,
                "{name} empty"
            );
        }
    }

    #[test]
    fn pin_guard_trips_on_the_indirection_and_its_target() {
        // The indirection trips whatever its value, even empty.
        for value in ["", "MY_TOKEN_SECRET"] {
            assert_eq!(
                // secret-leak-ok: test asserts env-var NAMES over obviously fake literal values; no PIN is read
                pin_env_in(vars(&[("WRFCOIN_HSM_PIN_ENV", value)])),
                Some("WRFCOIN_HSM_PIN_ENV".to_string())
            );
        }
        // A target named without PIN is still covered: the indirection that
        // names it is present, and it is reported first.
        assert_eq!(
            // secret-leak-ok: test asserts env-var NAMES over obviously fake literal values; no PIN is read
            pin_env_in(vars(&[
                ("MY_TOKEN_SECRET", "true"),
                ("WRFCOIN_HSM_PIN_ENV", "MY_TOKEN_SECRET")
            ])),
            Some("WRFCOIN_HSM_PIN_ENV".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn pin_guard_trips_on_a_non_utf8_value_or_name() {
        use std::os::unix::ffi::OsStringExt;
        let value = std::ffi::OsString::from_vec(vec![0xff, 0xfe]);
        assert_eq!(
            // secret-leak-ok: test asserts env-var NAMES over obviously fake literal values; no PIN is read
            pin_env_in(vec![("WRFCOIN_HSM_PIN".into(), value.clone())]),
            Some("WRFCOIN_HSM_PIN".to_string())
        );
        let name = std::ffi::OsString::from_vec(b"HSM_PIN_\xff".to_vec());
        assert!(
            // secret-leak-ok: test asserts env-var NAMES over obviously fake literal values; no PIN is read
            pin_env_in(vec![(name, value)]).is_some()
        );
    }

    #[test]
    fn sign_refuses_when_a_pin_variable_is_set_3096() {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir("pin-env");
        let (bundle_path, _, _) = bundle(&dir);
        std::env::set_var("WRFCOIN_TEST_TOKEN_PIN", "1234");
        let err = handle_ceremony_command(&CeremonyCommands::Sign {
            bundle: bundle_path,
            role: CeremonyRole::Oracle,
            out: dir.join("never.json"),
            timestamp: Some(1),
        })
        .expect_err("PIN in env must refuse");
        std::env::remove_var("WRFCOIN_TEST_TOKEN_PIN");
        let text = err.to_string();
        assert!(text.contains("PIN VALUE ENVIRONMENT DETECTED in WRFCOIN_TEST_TOKEN_PIN"), "{text}");
        assert!(!text.contains("1234"), "the PIN value must never be echoed");
        assert!(!dir.join("never.json").exists());
    }

    /// core4#3393 review: through the real command handler, for both roles,
    /// a PIN-like value in a real PIN variable refuses before any signer is
    /// loaded and is never echoed.
    #[test]
    fn sign_refuses_pin_like_values_for_both_roles_3393() {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir("pin-like");
        let (bundle_path, _, _) = bundle(&dir);
        for role in [CeremonyRole::Oracle, CeremonyRole::Producer] {
            for value in PIN_LIKE_VALUES {
                std::env::set_var("WRFCOIN_HSM_PIN", value);
                let err = handle_ceremony_command(&CeremonyCommands::Sign {
                    bundle: bundle_path.clone(),
                    role,
                    out: dir.join("never.json"),
                    timestamp: Some(1),
                })
                .expect_err("a PIN-like value must refuse");
                std::env::remove_var("WRFCOIN_HSM_PIN");
                let text = err.to_string();
                assert!(
                    // secret-leak-ok: test asserts the env-var NAME in the refusal over obviously fake literal values; no PIN is read
                    text.contains("PIN VALUE ENVIRONMENT DETECTED in WRFCOIN_HSM_PIN"),
                    "{role:?} {value:?}: {text}"
                );
                assert!(!dir.join("never.json").exists());
            }
        }
    }

    #[test]
    fn custody_check_reports_the_one_wrong_slot_and_exits_nonzero_3096() {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir("custody");
        let good = PublicKey::from_private_key(&key(31)).to_hex().to_ascii_lowercase();
        let other = PublicKey::from_private_key(&key(32)).to_hex().to_ascii_lowercase();
        let entry = |identity: &str, serial: &str, slot: &str, cka: &str, pk: &str| CustodyEntry {
            identity: identity.into(),
            role: "producer".into(),
            token_serial: serial.into(),
            piv_slot: slot.into(),
            public_label: "Public key for PIV Authentication".into(),
            cka_id: cka.into(),
            host: "test".into(),
            public_key: pk.into(),
        };
        let manifest = CustodyManifest {
            version: 1,
            entries: vec![
                entry("n5-node-a", "35278470", "9A", "01", &good),
                // Wrong slot: the manifest says 9C/02 but the key lives at 9A/01.
                entry("producer-standby", "35278471", "9C", "02", &other),
                entry("oracle-1", "35278471", "9D", "03", ""),
            ],
        };
        let observed = vec![
            ObservedKey { token_serial: "35278470".into(), token_label: "YubiKey PIV #35278470".into(), object_label: "Public key for PIV Authentication".into(), cka_id: Some("01".into()), public_key: good.clone() },
            ObservedKey { token_serial: "35278471".into(), token_label: "YubiKey PIV #35278471".into(), object_label: "Public key for PIV Authentication".into(), cka_id: Some("01".into()), public_key: other.clone() },
            ObservedKey { token_serial: "35278471".into(), token_label: "YubiKey PIV #35278471".into(), object_label: "Public key for Key Management".into(), cka_id: Some("03".into()), public_key: PublicKey::from_private_key(&key(33)).to_hex() },
            // No manifest entry claims this object: reported as unexpected, never fatal.
            ObservedKey { token_serial: "35278470".into(), token_label: "YubiKey PIV #35278470".into(), object_label: "Public key for Digital Signature".into(), cka_id: Some("02".into()), public_key: PublicKey::from_private_key(&key(34)).to_hex() },
        ];
        let manifest_path = dir.join("manifest.json");
        let observed_path = dir.join("observed.json");
        write_pretty_json(&manifest_path, &manifest).unwrap();
        write_pretty_json(&observed_path, &observed).unwrap();

        // Validate-only passes.
        let ok = handle_ceremony_command(&CeremonyCommands::Custody {
            manifest: manifest_path.clone(),
            check: false,
            module: None,
            observed: None,
        })
        .expect("valid manifest");
        assert!(ok["summary"].as_str().unwrap().contains("3 entries"));

        // --check against the observed inventory: the wrong slot is a mismatch.
        let err = handle_ceremony_command(&CeremonyCommands::Custody {
            manifest: manifest_path.clone(),
            check: true,
            module: None,
            observed: Some(observed_path.clone()),
        })
        .expect_err("one wrong slot must fail");
        let text = err.to_string();
        assert!(text.starts_with("CUSTODY MISMATCH"), "{text}");
        assert!(text.contains("object_absent") && text.contains("producer-standby"), "{text}");
        assert!(text.contains("\"unfilled\"") && text.contains("oracle-1"), "{text}");

        // Fix the slot: consistent, with oracle-1 still unfilled and the
        // unclaimed 9C object on the first token reported as unexpected.
        let mut fixed = manifest.clone();
        fixed.entries[1] = entry("producer-standby", "35278471", "9A", "01", &other);
        write_pretty_json(&manifest_path, &fixed).unwrap();
        let ok = handle_ceremony_command(&CeremonyCommands::Custody {
            manifest: manifest_path,
            check: true,
            module: None,
            observed: Some(observed_path),
        })
        .expect("consistent");
        assert_eq!(ok["consistent"], json!(true));
        let report: CustodyReport = serde_json::from_value(ok["report"].clone()).unwrap();
        assert_eq!(report.matches(), 2);
        assert_eq!(report.unfilled(), 1);
        assert!(report.findings.iter().any(|f| matches!(f, CustodyFinding::Unexpected { .. })));
    }
}
