//! Genesis oracle registration ceremony (#2530).
//!
//! Block 1 must register the oracle set, and until this module existed there
//! was no supported way to produce the transactions that do it: every
//! `new_oracle_registration` call site was `cfg(test)`. The #2496 block-1
//! agreement rule validated an input that had no producer.
//!
//! `build-registrations` is deliberately offline. It reads the genesis config
//! and a governance key, and writes a `genesis_oracle_ceremony_v1` artifact —
//! no node contact, no wall-clock reads that reach the signed bytes. Two
//! operators running it on the same inputs get byte-identical transactions,
//! which is what lets two nodes cut the same block 1.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wrfcoin_cli::{ApiClient, ApiConfig};
use wrfcoin_shared_protocol::blockchain::transaction::oracle_registration_authorization_message_with_domain;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey, Signature};
use wrfcoin_shared_protocol::oracle::ceremony::{
    CEREMONY_ARTIFACT_VERSION, GenesisOracleCeremony, GenesisOracleCeremonyArtifact,
};

/// Genesis oracle ceremony commands.
#[derive(Subcommand)]
pub enum OracleCommands {
    /// Build the block-1 oracle registration transactions offline.
    ///
    /// Deterministic: identical inputs produce byte-identical output,
    /// including transaction ids. Run it once and distribute the artifact —
    /// running it independently per node also works, but only if every input
    /// below is identical, `--submitted-at` included.
    BuildRegistrations {
        /// Genesis bootstrap config written by `setup genesis-config`.
        /// Supplies the oracle set and quorum.
        #[clap(long)]
        genesis: PathBuf,

        /// Chain id the registrations are signed for. Required, never
        /// inferred: it is folded into the signature preimage but not stored
        /// in the payload, so a wrong value produces registrations that fail
        /// on every peer with no diagnostic.
        #[clap(long)]
        chain_id: String,

        /// Network id. Defaults to the genesis config's `network_id`.
        #[clap(long)]
        network_id: Option<String>,

        /// Optional identity -> signing-pubkey-hex map (JSON object). Omit it
        /// when the oracle identities are themselves ed25519 public key hex,
        /// which is what `setup genesis-config` produces.
        #[clap(long)]
        signing_keys: Option<PathBuf>,

        /// Signing-key epoch. 0 is reserved for legacy identity keys.
        #[clap(long, default_value_t = 1)]
        epoch: u64,

        /// Activation timestamp (unix seconds) for the signing-key epoch.
        /// Must be identical across operators — it is consensus content.
        #[clap(long)]
        activated_at: i64,

        /// Transaction timestamp (unix seconds). Pinned rather than taken
        /// from the clock so the transaction ids are reproducible. Must sit
        /// inside the node's submission window relative to block 1.
        #[clap(long)]
        submitted_at: i64,

        /// File holding the governance private key hex.
        #[clap(long)]
        governance_key: PathBuf,

        /// Artifact output path.
        #[clap(long)]
        out: PathBuf,
    },

    /// Submit a built artifact's registrations to the CUTTING node's mempool.
    ///
    /// Target the node that will produce block 1: only that node needs the
    /// batch in its mempool. The route is operator-gated, so a token is
    /// required on every node that sets WRFCOIN_OPERATOR_TOKEN.
    SubmitRegistrations {
        /// Artifact produced by `build-registrations`.
        #[clap(long)]
        artifact: PathBuf,

        /// Operator bearer token for the cutting node. Falls back to
        /// WRFCOIN_OPERATOR_TOKEN. Without either the node answers 401.
        #[clap(long)]
        token: Option<String>,

        /// Launch API base URL of the CUTTING node (the one that will produce
        /// block 1), e.g. http://127.0.0.1:8556. Falls back to WRFCOIN_API_URL,
        /// then to the local default.
        #[clap(long)]
        node_url: Option<String>,
    },

    /// Re-verify an artifact without submitting or contacting a node.
    VerifyRegistrations {
        /// Artifact produced by `build-registrations`.
        #[clap(long)]
        artifact: PathBuf,
    },
}

#[derive(Debug, Deserialize)]
struct GenesisOracleConfig {
    authorized_oracles: Vec<String>,
    quorum: usize,
}

#[derive(Debug, Deserialize)]
struct GenesisConfigFile {
    network_id: String,
    oracle_config: GenesisOracleConfig,
}

/// Wire shape of `POST /transactions/oracle/registrations`.
#[derive(Debug, Serialize)]
struct RegistrationSubmitBody<'a> {
    chain_id: &'a str,
    network_id: &'a str,
    transactions: &'a [wrfcoin_shared_protocol::blockchain::transaction::Transaction],
}

pub async fn handle_oracle_command(command: &OracleCommands) -> Result<Value> {
    match command {
        OracleCommands::BuildRegistrations {
            genesis,
            chain_id,
            network_id,
            signing_keys,
            epoch,
            activated_at,
            submitted_at,
            governance_key,
            out,
        } => {
            let genesis_config = read_genesis_config(genesis)?;
            let network_id = network_id
                .clone()
                .unwrap_or_else(|| genesis_config.network_id.clone());
            let oracle_set = genesis_config.oracle_config.authorized_oracles.clone();
            let signing_pubkeys = match signing_keys {
                Some(path) => read_signing_keys(path)?,
                // The identities produced by `setup genesis-config` are
                // already ed25519 pubkey hex, so identity-as-signing-key is
                // the correct default rather than a placeholder.
                None => oracle_set
                    .iter()
                    .map(|id| (id.clone(), id.clone()))
                    .collect(),
            };

            let ceremony = GenesisOracleCeremony::new(
                chain_id.clone(),
                network_id.clone(),
                oracle_set,
                genesis_config.oracle_config.quorum,
                signing_pubkeys,
                *epoch,
                *activated_at,
                *submitted_at,
            )
            .map_err(|err| anyhow!("ceremony inputs rejected: {err}"))?;

            let governance = read_private_key(governance_key)?;
            let artifact = ceremony.build_artifact(&governance);
            write_pretty_json(out, &artifact)?;

            Ok(json!({
                "summary": format!(
                    "Built {} genesis oracle registration(s) for {}/{}",
                    artifact.transactions.len(),
                    artifact.chain_id,
                    artifact.network_id
                ),
                "output": out.display().to_string(),
                "version": artifact.version,
                "chainId": artifact.chain_id,
                "networkId": artifact.network_id,
                "oracleSet": artifact.oracle_set,
                "oracleQuorum": artifact.oracle_quorum,
                "epoch": artifact.epoch,
                "activatedAt": artifact.activated_at,
                "submittedAt": artifact.submitted_at,
                "governancePubkey": artifact.governance_pubkey,
                "transactionIds": transaction_ids(&artifact),
            }))
        },

        OracleCommands::VerifyRegistrations { artifact } => {
            let artifact = read_artifact(artifact)?;
            verify_artifact(&artifact)?;
            Ok(json!({
                // Not "Artifact verified". An operator running this during a
                // cut reads that as "the node will take it", and this check
                // cannot know that — see `verify_artifact` for the three things
                // only a node can decide.
                "summary": format!(
                    "Artifact is self-consistent and correctly signed: {} registration(s), \
                     quorum {} of {}. This does NOT establish that a node will accept it.",
                    artifact.transactions.len(),
                    artifact.oracle_quorum,
                    artifact.oracle_set.len()
                ),
                "notCheckedOffline": [
                    "governance allowlist — the node's WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS",
                    "the node's own chain/network identity",
                    "submitted_at freshness at admission and at block assembly",
                ],
                "chainId": artifact.chain_id,
                "networkId": artifact.network_id,
                "oracleSet": artifact.oracle_set,
                "oracleQuorum": artifact.oracle_quorum,
                "transactionIds": transaction_ids(&artifact),
            }))
        },

        OracleCommands::SubmitRegistrations {
            artifact,
            token,
            node_url,
        } => {
            submit_registrations(artifact, token.clone(), node_url.clone(), |var| {
                std::env::var(var).ok()
            })
            .await
        },
    }
}

/// O5: verify the artifact locally, then POST it to the cutting node.
///
/// `env` is the environment lookup used for the `WRFCOIN_API_URL` /
/// `WRFCOIN_OPERATOR_TOKEN` fallbacks; the CLI passes `std::env::var`, tests
/// pass a map so the wire contract can be pinned without touching process
/// environment.
async fn submit_registrations(
    artifact_path: &Path,
    token: Option<String>,
    node_url: Option<String>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<Value> {
    let artifact = read_artifact(artifact_path)?;
    // Verify before sending. The node rejects a bad batch anyway, but
    // failing locally names the offending transaction instead of
    // returning one error for the batch.
    verify_artifact(&artifact)?;

    let client = ApiClient::new(submit_api_config(node_url, token, env))?;
    let body = RegistrationSubmitBody {
        chain_id: &artifact.chain_id,
        network_id: &artifact.network_id,
        transactions: &artifact.transactions,
    };
    let result: Value = client
        .post(
            "/transactions/oracle/registrations",
            Some(serde_json::to_value(&body)?),
        )
        .await?;

    Ok(json!({
        "summary": format!(
            "Submitted {} genesis oracle registration(s)",
            artifact.transactions.len()
        ),
        "chainId": artifact.chain_id,
        "networkId": artifact.network_id,
        "response": result,
    }))
}

/// Resolve the node URL and operator token for `submit-registrations`.
///
/// Precedence for each, independently: the flag, then the environment
/// (`WRFCOIN_API_URL` / `WRFCOIN_OPERATOR_TOKEN`), then the default
/// (`ApiConfig::DEFAULT_BASE_URL` / no token). Blank values count as absent at
/// every level, so an empty `--token ""` or an exported-but-empty variable
/// falls through rather than sending `Authorization: Bearer ` (#2707).
///
/// Only the operator token is consulted. `ApiConfig::default()` also accepts
/// `WRFCOIN_READER_TOKEN`, but `POST /transactions/oracle/registrations` is an
/// operator route and a reader token would only produce a slower 403.
///
/// `env` is injected so the precedence is testable without mutating process
/// environment under a parallel test runner.
fn submit_api_config(
    node_url: Option<String>,
    token: Option<String>,
    env: impl Fn(&str) -> Option<String>,
) -> ApiConfig {
    fn present(value: Option<String>) -> Option<String> {
        value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    let base_url = present(node_url)
        .or_else(|| present(env("WRFCOIN_API_URL")))
        .map(|url| url.trim_end_matches('/').to_string())
        .unwrap_or_else(|| ApiConfig::DEFAULT_BASE_URL.to_string());
    let bearer_token = present(token).or_else(|| present(env("WRFCOIN_OPERATOR_TOKEN")));

    ApiConfig {
        base_url,
        bearer_token,
        ..ApiConfig::default()
    }
}

fn transaction_ids(artifact: &GenesisOracleCeremonyArtifact) -> Vec<String> {
    artifact
        .transactions
        .iter()
        .map(|tx| {
            tx.id
                .as_ref()
                .map(|id| id.to_hex())
                .unwrap_or_else(|| "<unset>".to_string())
        })
        .collect()
}

/// Check that the artifact is internally consistent and correctly signed.
///
/// **Not a prediction that a node will accept it.** The four checks below cover
/// what can happen to a file in transit between hosts: a forged or altered
/// governance signature, an unstable transaction id, a signature naming a key
/// that is not the artifact's, and a payload disagreeing with the artifact
/// header. Passing means this is the artifact that was signed, by the key it
/// names, for the chain it declares — nothing more.
///
/// Three things a node checks that this cannot. They are out of reach offline,
/// not oversights, but silence about them is what makes a green result read as
/// a go-ahead:
///
/// 1. **The governance allowlist.** A node accepts a registration only when
///    `auth_pubkey` appears in its `WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS`. This
///    verifies the signature against the key the *artifact* names; whether the
///    network trusts that key is deployment config no offline tool can see.
/// 2. **The node's own chain identity.** Signatures are verified against the
///    domain the artifact declares. A node resolves its own from the
///    environment and refuses a mismatch, so an artifact that verifies here can
///    still be built for the wrong chain.
/// 3. **Freshness.** `submitted_at` ages. An artifact that verifies today is
///    rejected at submission once it drifts past the admission margin, and
///    dropped at assembly past the submission window.
///
/// Nor can anything offline establish that the set is the FINAL one. The
/// agreement check here compares each payload against the artifact header,
/// which is self-consistency; #2496's cross-payload rule is what makes it
/// binding, and that runs at the node.
///
/// An earlier version of this comment claimed the fourth check caught "a
/// registration carrying a set that is not the final one", and claimed to
/// re-check "everything the artifact asserts about itself". Both overstated it.
/// That mattered: the previous revision of this function also claimed to catch
/// "an unauthorized signature" while no line verified one, and it shipped.
fn verify_artifact(artifact: &GenesisOracleCeremonyArtifact) -> Result<()> {
    if artifact.version != CEREMONY_ARTIFACT_VERSION {
        return Err(anyhow!(
            "artifact version is {:?}, expected {:?}",
            artifact.version,
            CEREMONY_ARTIFACT_VERSION
        ));
    }
    if artifact.transactions.is_empty() {
        return Err(anyhow!("artifact carries no transactions"));
    }

    for (index, tx) in artifact.transactions.iter().enumerate() {
        let payload = tx
            .parse_oracle_registration()
            .ok_or_else(|| anyhow!("transactions[{index}] is not an oracle registration"))?;

        if payload.oracle_set != artifact.oracle_set || payload.oracle_quorum != artifact.oracle_quorum
        {
            return Err(anyhow!(
                "transactions[{index}] (identity {}) declares a different oracle \
                 set/quorum than the artifact header",
                payload.identity
            ));
        }

        match tx.id.as_ref() {
            Some(id) if *id == tx.calculate_hash() => {},
            Some(_) => {
                return Err(anyhow!( // secret-leak-ok: interpolates an ed25519 PUBLIC key
                    "transactions[{index}] (identity {}) has an id that does not match \
                     its contents; the artifact was modified after signing",
                    payload.identity
                ));
            },
            None => {
                return Err(anyhow!(
                    "transactions[{index}] (identity {}) carries no transaction id",
                    payload.identity
                ));
            },
        }

        if payload.auth_pubkey.as_deref() != Some(artifact.governance_pubkey.as_str()) {
            return Err(anyhow!(
                "transactions[{index}] (identity {}) is signed by a different key than \
                 the artifact's governance_pubkey",
                payload.identity
            ));
        }

        // Verify the governance signature itself.
        //
        // Every check above is self-referential. `auth_pubkey` is an UNSIGNED
        // field in the same payload, so naming the governance key costs an
        // attacker nothing, and recomputing the transaction id after tampering
        // satisfies the id check. Found by an independent verifier who replaced
        // `auth_signature` with 64 zero bytes, re-derived the id, and watched
        // this function report success on an artifact every node rejects. The
        // doc comment claimed to catch "an unauthorized signature" while no
        // line here ever looked at one.
        //
        // Checked against the artifact's OWN declared chain identity rather
        // than the local environment's. `signing_chain_id()` falls back to a
        // silent default, so a verifier reading the domain from the ambient
        // env would bless an artifact signed for the wrong chain whenever the
        // operator's shell happened to match — precisely the case offline
        // verification exists to catch.
        let Some(sig_hex) = payload.auth_signature.as_deref() else {
            return Err(anyhow!(
                "transactions[{index}] (identity {}) carries no governance signature",
                payload.identity
            ));
        };
        let governance_key = PublicKey::from_hex(&artifact.governance_pubkey).map_err(|_| {
            anyhow!( // secret-leak-ok: interpolates an ed25519 PUBLIC key
                "artifact governance_pubkey {} is not a public key",
                artifact.governance_pubkey
            )
        })?;
        let Ok(signature) = Signature::from_hex(sig_hex) else {
            return Err(anyhow!(
                "transactions[{index}] (identity {}) has an unparseable governance \
                 signature",
                payload.identity
            ));
        };
        let mut unsigned = payload.clone();
        unsigned.auth_pubkey = None;
        unsigned.auth_signature = None;
        let message = oracle_registration_authorization_message_with_domain(
            &artifact.chain_id,
            &artifact.network_id,
            &unsigned,
        );
        if !governance_key.verify(&message, &signature) {
            return Err(anyhow!(
                "transactions[{index}] (identity {}) has a governance signature that does \
                 not verify under the artifact's governance_pubkey for chain {}/{}; the \
                 artifact was modified after it was signed",
                payload.identity,
                artifact.chain_id,
                artifact.network_id
            ));
        }
    }

    Ok(())
}

fn read_genesis_config(path: &Path) -> Result<GenesisConfigFile> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read genesis config {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse genesis config {}", path.display()))
}

fn read_signing_keys(path: &Path) -> Result<BTreeMap<String, String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read signing keys {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse signing keys {}", path.display()))
}

fn read_artifact(path: &Path) -> Result<GenesisOracleCeremonyArtifact> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read artifact {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse artifact {}", path.display()))
}

fn read_private_key(path: &Path) -> Result<PrivateKey> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read governance key {}", path.display()))?;
    PrivateKey::from_hex(raw.trim())
        .map_err(|err| anyhow!("governance key at {} is not valid: {err}", path.display()))
}

fn write_pretty_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(value).context("failed to serialize artifact")?
    );
    fs::write(path, body).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::blockchain::transaction::OracleRegistrationPayload;

    const CHAIN: &str = "wrfcoin-cut";
    const NETWORK: &str = "testnet";

    fn keypair(seed: u8) -> (PrivateKey, String) {
        let key = PrivateKey::from_bytes(&[seed; 32]).expect("32 bytes is a key"); // panic-ok: test helper, literal input
        let pubkey = PublicKey::from_private_key(&key).to_hex();
        (key, pubkey)
    }

    pub(super) fn artifact() -> (GenesisOracleCeremonyArtifact, PrivateKey) {
        let (gov_sk, _) = keypair(7);
        let identities: Vec<String> = (0..3).map(|i| keypair(20 + i).1).collect();
        let signing_pubkeys: BTreeMap<String, String> =
            identities.iter().map(|id| (id.clone(), id.clone())).collect();
        let ceremony = GenesisOracleCeremony::new(
            CHAIN,
            NETWORK,
            identities,
            2,
            signing_pubkeys,
            1,
            1_700_000_000,
            1_700_000_000,
        )
        .expect("ceremony inputs valid"); // panic-ok: test helper, literal inputs
        let built = ceremony.build_artifact(&gov_sk);
        (built, gov_sk)
    }

    /// A well-formed artifact verifies.
    #[test]
    fn verify_artifact_accepts_a_ceremony_output() {
        let (built, _) = artifact();
        verify_artifact(&built).expect("a freshly built artifact must verify"); // panic-ok: test assertion
    }

    /// RED control for the offline false-green an independent verifier found.
    ///
    /// Corrupting `auth_signature` and then RECOMPUTING the transaction id is
    /// the whole attack: it satisfies the id check, and `auth_pubkey` still
    /// names the artifact's governance key because that field is unsigned. If
    /// this test fails, `verify-registrations` is again reporting success on
    /// an artifact every node rejects — the worst possible outcome for a tool
    /// operators run precisely to avoid a failed cut.
    #[test]
    fn verify_artifact_rejects_a_corrupted_signature() {
        let (mut built, _) = artifact();

        let mut payload: OracleRegistrationPayload = built.transactions[0]
            .parse_oracle_registration()
            .expect("built transaction is a registration"); // panic-ok: test assertion
        payload.auth_signature = Some("00".repeat(64));
        built.transactions[0].data = Some(
            serde_json::to_string(&payload).expect("payload serializes"), // panic-ok: test assertion
        );
        // Re-derive the id so the id check cannot be what rejects this.
        built.transactions[0].id = Some(built.transactions[0].calculate_hash());

        let err = verify_artifact(&built)
            .expect_err("a corrupted governance signature must be rejected") // panic-ok: test assertion
            .to_string();
        assert!(
            err.contains("does not verify"),
            "the error must name the signature as the failure, got: {err}"
        );
    }

    /// The domain is the artifact's, not the ambient environment's.
    ///
    /// Signatures are chain-bound. An artifact signed for one chain must not
    /// verify when its header declares another, regardless of what
    /// `WRFCOIN_CHAIN_ID` happens to say on the operator's box.
    ///
    /// NOTE: this test alone does NOT prove the domain came from the artifact.
    /// It passes against an implementation that reads the ambient environment
    /// too, because the ambient default differs from this fixture's chain
    /// either way. `verify_artifact_binds_the_artifacts_own_domain` below is
    /// the one that discriminates. Keeping both is deliberate: this one is the
    /// readable statement of intent, that one is the proof.
    #[test]
    fn verify_artifact_rejects_a_signature_from_another_chain() {
        let (mut built, _) = artifact();
        built.chain_id = "wrfcoin-other".to_string();

        let err = verify_artifact(&built)
            .expect_err("a signature bound to a different chain must be rejected") // panic-ok: test assertion
            .to_string();
        assert!(
            err.contains("does not verify"),
            "the error must name the signature as the failure, got: {err}"
        );
    }

    /// The signature is checked against the ARTIFACT's declared domain.
    ///
    /// Built so the two candidate implementations disagree. The ceremony signs
    /// under the ambient chain identity, then the artifact header is changed
    /// without re-signing:
    ///
    /// - reading the domain from the artifact  -> verifies "wrfcoin-…-not"
    ///   against a signature over the ambient domain, fails, `Err`. Correct.
    /// - reading it from `signing_chain_id()`  -> verifies the ambient domain
    ///   against a signature over the ambient domain, succeeds, `Ok`. Wrong.
    ///
    /// Without this, "verified against the artifact's own domain" is a claim no
    /// test can falsify, and an implementation that quietly consults the
    /// environment passes the whole suite. `signing_chain_id()` falls back to a
    /// silent default, so that implementation would bless a wrong-chain
    /// artifact on any box whose shell happened to match — the exact failure
    /// offline verification exists to catch.
    #[test]
    fn verify_artifact_binds_the_artifacts_own_domain() {
        use wrfcoin_shared_protocol::crypto::domain_signing::{
            signing_chain_id, signing_network_id,
        };

        let ambient_chain = signing_chain_id();
        let ambient_network = signing_network_id();

        let (gov_sk, gov_pk) = keypair(7);
        let identities: Vec<String> = (0..3).map(|i| keypair(20 + i).1).collect();
        let signing_pubkeys: BTreeMap<String, String> =
            identities.iter().map(|id| (id.clone(), id.clone())).collect();
        let ceremony = GenesisOracleCeremony::new(
            ambient_chain.clone(),
            ambient_network,
            identities,
            2,
            signing_pubkeys,
            1,
            1_700_000_000,
            1_700_000_000,
        )
        .expect("ceremony inputs valid"); // panic-ok: test helper, literal inputs
        let mut built = ceremony.build_artifact(&gov_sk);
        assert_eq!(built.governance_pubkey, gov_pk);

        // Derived from the ambient value so the two can never collide.
        built.chain_id = format!("{ambient_chain}-not");

        let err = verify_artifact(&built)
            .expect_err("the artifact's declared domain must be what is verified") // panic-ok: test assertion
            .to_string();
        assert!(
            err.contains("does not verify"),
            "the error must name the signature as the failure, got: {err}"
        );
    }
}

#[cfg(test)]
mod submit_target_resolution_tests {
    use super::submit_api_config;
    use std::collections::HashMap;
    use wrfcoin_cli::ApiConfig;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn flag_beats_env_for_both_url_and_token() {
        let env = env_of(&[
            ("WRFCOIN_API_URL", "http://env-node:8556"),
            ("WRFCOIN_OPERATOR_TOKEN", "env-token"),
        ]);
        let cfg = submit_api_config(
            Some("http://flag-node:8556".into()),
            Some("flag-token".into()),
            env,
        );
        assert_eq!(cfg.base_url, "http://flag-node:8556");
        let flag_token_won = cfg.bearer_token.as_deref() == Some("flag-token");
        assert!(flag_token_won, "the --token flag must beat WRFCOIN_OPERATOR_TOKEN"); // secret-leak-ok: bool local; fixture literal, no value reaches the message
    }

    #[test]
    fn env_is_used_when_flags_absent() {
        let env = env_of(&[
            ("WRFCOIN_API_URL", "http://env-node:8556"),
            ("WRFCOIN_OPERATOR_TOKEN", "env-token"),
        ]);
        let cfg = submit_api_config(None, None, env);
        assert_eq!(cfg.base_url, "http://env-node:8556");
        let env_token_used = cfg.bearer_token.as_deref() == Some("env-token");
        assert!(env_token_used, "WRFCOIN_OPERATOR_TOKEN must be used when --token is absent"); // secret-leak-ok: bool local; fixture literal, no value reaches the message
    }

    #[test]
    fn defaults_when_neither_flag_nor_env() {
        let cfg = submit_api_config(None, None, env_of(&[]));
        assert_eq!(cfg.base_url, ApiConfig::DEFAULT_BASE_URL);
        assert!(cfg.bearer_token.is_none(), "no token source must mean no token"); // secret-leak-ok: is_none() yields a bool; no value reaches the message
    }

    #[test]
    fn blank_values_fall_through_at_every_level() {
        // A blank flag must not shadow a real env value, and a blank env
        // value must not produce `Authorization: Bearer ` (empty credential).
        let env = env_of(&[
            ("WRFCOIN_API_URL", "http://env-node:8556"),
            ("WRFCOIN_OPERATOR_TOKEN", "   "),
        ]);
        let cfg = submit_api_config(Some("  ".into()), Some("".into()), env);
        assert_eq!(cfg.base_url, "http://env-node:8556");
        assert!(cfg.bearer_token.is_none(), "a blank token must fall through to none"); // secret-leak-ok: is_none() yields a bool; no value reaches the message
    }

    #[test]
    fn reader_token_is_not_accepted_for_an_operator_route() {
        let env = env_of(&[("WRFCOIN_READER_TOKEN", "reader-token")]);
        let cfg = submit_api_config(None, None, env);
        assert!(cfg.bearer_token.is_none(), "WRFCOIN_READER_TOKEN must not be used for an operator route"); // secret-leak-ok: is_none() yields a bool; no value reaches the message
    }

    #[test]
    fn trailing_slash_is_trimmed_so_paths_join_cleanly() {
        let cfg = submit_api_config(Some("http://flag-node:8556/".into()), None, env_of(&[]));
        assert_eq!(cfg.base_url, "http://flag-node:8556");
    }
}

#[cfg(test)]
mod submit_registrations_wire_tests {
    //! `submit-registrations` on the wire, through the same function the CLI
    //! calls. These are the regression controls for #2707: O5 must
    //! authenticate to the cutting node, and must fail as an auth error —
    //! not a parse error or a retry storm — when it cannot.

    use super::submit_registrations;
    use mockito::{Matcher, Server};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use wrfcoin_cli::CliErrorCode;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    /// Write a freshly built, verifying artifact to a unique temp path.
    struct ArtifactFile(PathBuf);

    impl ArtifactFile {
        fn new(tag: &str) -> Self {
            let (built, _) = super::tests::artifact();
            let path = std::env::temp_dir().join(format!(
                "wrfcoin-2707-{tag}-{}.json",
                std::process::id()
            ));
            super::write_pretty_json(&path, &built).expect("write artifact"); // panic-ok: test setup
            Self(path)
        }
    }

    impl Drop for ArtifactFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    const ACCEPTED: &str = r#"{"accepted":3,"transaction_ids":[],"oracle_set":[],"oracle_quorum":2}"#;

    #[tokio::test(flavor = "multi_thread")]
    async fn flag_token_is_sent_as_bearer_to_the_flag_node() {
        let file = ArtifactFile::new("flag");
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/transactions/oracle/registrations")
            .match_header("authorization", "Bearer flag-token")
            .match_body(Matcher::PartialJsonString(
                r#"{"chain_id":"wrfcoin-cut","network_id":"testnet"}"#.to_string(),
            ))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(ACCEPTED)
            .expect(1)
            .create_async()
            .await;

        // Env points somewhere else with a different token; flags must win.
        let env = env_of(&[
            ("WRFCOIN_API_URL", "http://127.0.0.1:1"),
            ("WRFCOIN_OPERATOR_TOKEN", "env-token"),
        ]);
        let out = submit_registrations(
            &file.0,
            Some("flag-token".into()),
            Some(server.url()),
            env,
        )
        .await
        .expect("submission must succeed"); // panic-ok: test assertion

        assert_eq!(out["response"]["accepted"], 3);
        assert_eq!(out["chainId"], "wrfcoin-cut");
        mock.assert_async().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn env_token_is_sent_when_flag_absent() {
        let file = ArtifactFile::new("env");
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/transactions/oracle/registrations")
            .match_header("authorization", "Bearer env-token")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(ACCEPTED)
            .expect(1)
            .create_async()
            .await;

        let env = env_of(&[
            ("WRFCOIN_API_URL", server.url().as_str()),
            ("WRFCOIN_OPERATOR_TOKEN", "env-token"),
        ]);
        let out = submit_registrations(&file.0, None, None, env)
            .await
            .expect("submission must succeed"); // panic-ok: test assertion

        assert_eq!(out["response"]["accepted"], 3);
        mock.assert_async().await;
    }

    /// The #2707 failure: no token anywhere. The request must carry NO
    /// Authorization header (not an empty one), and the node's 401 must
    /// surface as `AuthError` so the operator sees the cause, not a retry
    /// storm ending in a generic failure.
    #[tokio::test(flavor = "multi_thread")]
    async fn no_token_sends_no_header_and_401_is_an_auth_error() {
        let file = ArtifactFile::new("none");
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/transactions/oracle/registrations")
            .match_header("authorization", Matcher::Missing)
            .with_status(401)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"ok":false,"error":{"code":"UNAUTHORIZED","message":"Missing or malformed Authorization header"}}"#,
            )
            // The shared client retries non-2xx responses; how many times is
            // its policy, not this test's contract. What matters is that
            // every attempt matched the no-header matcher above.
            .expect_at_least(1)
            .create_async()
            .await;

        let env = env_of(&[("WRFCOIN_API_URL", server.url().as_str())]);
        let err = submit_registrations(&file.0, None, None, env)
            .await
            .expect_err("an unauthenticated submission must fail"); // panic-ok: test assertion

        assert_eq!(CliErrorCode::from_error(&err), CliErrorCode::AuthError);
        mock.assert_async().await;
    }
}
