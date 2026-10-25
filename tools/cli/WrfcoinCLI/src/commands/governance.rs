use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;
use serde::Serialize;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use wrfcoin_cli::{ApiClient, ApiConfig};
use wrfcoin_shared_protocol::crypto::domain_signing::oracle_governance_pubkeys;
use wrfcoin_shared_protocol::crypto::signature::PrivateKey;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::chain_binding::LegacyFaucetBinding;
use wrfcoin_shared_protocol::governance::faucet_sink_pop::{
    FaucetSinkPossession, FaucetSinkPossessionProof,
};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    FeatureScheduleExtension, ScheduleExtensionContext, SignedFeatureScheduleExtension,
    check_faucet_sink_binding_signable_against, sign_faucet_sink_binding_extension_with_key_against,
    verify_feature_schedule_extension,
};
use wrfcoin_shared_protocol::governance::upgrade_notice::{
    SignedUpgradeNotice, UpgradeNotice, sign_upgrade_notice_with_key, verify_upgrade_notice,
};

/// Commands for governance participation
#[derive(Subcommand)]
pub enum GovernanceCommands {
    /// List active governance proposals
    ListProposals {
        /// Status filter (active, passed, rejected, all)
        #[clap(long, default_value = "active")]
        status: String,
    },

    /// View details of a specific proposal
    ViewProposal {
        /// Proposal ID
        #[clap(long)]
        id: u64,
    },

    /// Create a new governance proposal
    CreateProposal {
        /// Proposal title
        #[clap(long)]
        title: String,

        /// Proposal description
        #[clap(long)]
        description: String,

        /// Proposal type (parameter, upgrade, funding)
        #[clap(long)]
        proposal_type: String,

        /// Changes in JSON format
        #[clap(long)]
        changes: String,

        /// Voting period in days
        #[clap(long, default_value = "14")]
        voting_period: u32,
    },

    /// Vote on a governance proposal
    Vote {
        /// Proposal ID
        #[clap(long)]
        id: u64,

        /// Vote (yes, no, abstain)
        #[clap(long)]
        vote: String,

        /// Optional vote justification
        #[clap(long)]
        justification: Option<String>,
    },

    /// Delegate voting power to another address
    Delegate {
        /// Address to delegate to
        #[clap(long)]
        to_address: String,

        /// Amount of voting power to delegate
        #[clap(long)]
        amount: Option<f64>,
    },

    /// View personal voting history
    #[clap(hide = true)]
    VotingHistory,

    /// Governance-signed upgrade notices (#3094): sign or verify, fully offline
    #[command(subcommand)]
    UpgradeNotice(UpgradeNoticeCommands),

    /// Governance-signed feature-schedule extensions (#3277): sign or verify,
    /// fully offline
    #[command(subcommand)]
    ScheduleExtension(ScheduleExtensionCommands),

    /// Faucet-key proof of possession for a faucet-sink binding (#3361 F3),
    /// fully offline
    #[command(subcommand)]
    FaucetPop(FaucetPopCommands),
}

/// `wrfcoin governance faucet-pop sign` — run by whoever holds the faucet
/// wallet's key, offline, once per binding.
#[derive(Subcommand)]
pub enum FaucetPopCommands {
    /// Sign a proof that the faucet key controls its address and consents to
    /// it being the faucet sink of one chain instance. The address is
    /// DERIVED from the key; the tool refuses unless a faucet-binding row in
    /// this binary names that address for that chain instance.
    Sign {
        /// The faucet account's key: a `wrfcoin wallet` keystore JSON, or a
        /// file holding the 32-byte ed25519 private key as hex
        #[clap(long)]
        faucet_key: PathBuf,

        /// Chain id the binding is for (the extension's `chain_id`)
        #[clap(long)]
        chain_id: String,

        /// Network id the binding is for (the extension's `network_id`)
        #[clap(long)]
        network_id: String,

        /// Lowercase hex block-1 hash of the chain instance
        #[clap(long)]
        genesis_hash: String,

        /// Proof output path (hand it to the governance signer)
        #[clap(long)]
        out: PathBuf,
    },
}

/// `wrfcoin governance schedule-extension sign|verify` — both usable offline.
#[derive(Subcommand)]
pub enum ScheduleExtensionCommands {
    /// Sign an extension with the governance key file (same key-file contract
    /// as `upgrade-notice sign`). The input may be an unsigned
    /// `FeatureScheduleExtension` or an already-signed one, in which case this
    /// signature is appended (prepares #2156 threshold signing). Refuses a
    /// malformed extension and any feature this binary does not implement.
    Sign {
        /// Extension JSON (unsigned, or a `SignedFeatureScheduleExtension`)
        #[clap(long)]
        extension: PathBuf,

        /// File holding the governance private key hex
        #[clap(long)]
        governance_key: PathBuf,

        /// Signed artifact output path
        #[clap(long)]
        out: PathBuf,

        /// Also check the activation window as if carried at this height
        #[clap(long)]
        carrying_height: Option<u64>,

        /// Faucet-key proof of possession (`wrfcoin governance faucet-pop
        /// sign`). Required to sign a `faucet_sink_binding*` extension
        /// (#3361 F3); refused for any other.
        #[clap(long)]
        faucet_pop: Option<PathBuf>,
    },

    /// Verify a signed extension against this chain instance and the
    /// governance allowlist, and print it with the verdict. Refusal is a
    /// non-zero exit.
    Verify {
        /// Signed artifact produced by `sign`
        #[clap(long)]
        signed: PathBuf,

        /// Lowercase hex block-1 hash of the chain the extension must be for
        /// (`GET /chain/block/1`). Required: an extension is valid for one
        /// chain instance only.
        #[clap(long)]
        genesis_hash: String,

        /// Chain id the extension must be for; defaults to the extension's own
        #[clap(long)]
        chain_id: Option<String>,

        /// Network id the extension must be for; defaults to the extension's own
        #[clap(long)]
        network_id: Option<String>,

        /// Comma-separated governance public keys (hex); defaults to
        /// WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS
        #[clap(long)]
        allowlist: Option<String>,

        /// Also check the activation window as if carried at this height
        #[clap(long)]
        carrying_height: Option<u64>,
    },
}

/// `wrfcoin governance upgrade-notice sign|verify` — both usable offline.
#[derive(Subcommand)]
pub enum UpgradeNoticeCommands {
    /// Sign a notice with the governance key file (same key-file contract as
    /// `oracle build-registrations --governance-key`). The input may be an
    /// unsigned notice or an already-signed one, in which case this signature
    /// is appended (prepares #2156 threshold signing).
    Sign {
        /// Notice JSON (unsigned `UpgradeNotice`, or a `SignedUpgradeNotice`)
        #[clap(long)]
        notice: PathBuf,

        /// File holding the governance private key hex
        #[clap(long)]
        governance_key: PathBuf,

        /// Signed artifact output path
        #[clap(long)]
        out: PathBuf,
    },

    /// Verify a signed notice against the governance allowlist and print the
    /// notice with the verdict. Refusal is a non-zero exit.
    Verify {
        /// Signed artifact produced by `sign`
        #[clap(long)]
        signed: PathBuf,

        /// Verification time (unix seconds); defaults to now
        #[clap(long)]
        now: Option<i64>,

        /// Chain id the notice must be for; defaults to the notice's own
        #[clap(long)]
        chain_id: Option<String>,

        /// Network id the notice must be for; defaults to the notice's own
        #[clap(long)]
        network_id: Option<String>,

        /// Comma-separated governance public keys (hex); defaults to
        /// WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS
        #[clap(long)]
        allowlist: Option<String>,
    },
}

pub async fn handle_governance_command(command: &GovernanceCommands) -> Result<Value> {
    if matches!(command, GovernanceCommands::VotingHistory) {
        return Err(wrfcoin_cli::errors::CliError::new(
            wrfcoin_cli::CliErrorCode::NotFound,
            "governance voting-history: not wired to a served node API (see wrfcoin/core4#3247)",
        )
        .into());
    }
    // Offline commands first: they must never need (or touch) the launch API.
    if let GovernanceCommands::UpgradeNotice(action) = command {
        return handle_upgrade_notice(action);
    }
    if let GovernanceCommands::ScheduleExtension(action) = command {
        return handle_schedule_extension(action);
    }
    if let GovernanceCommands::FaucetPop(action) = command {
        return handle_faucet_pop(action);
    }
    let client = ApiClient::new(ApiConfig::default())?;
    match command {
        GovernanceCommands::ListProposals { status } => {
            let result: Value = client
                .get(&format!("/governance/proposals?status={}", status), None)
                .await?;
            Ok(json!({ "summary": "Governance proposals listed", "proposals": result }))
        },
        GovernanceCommands::ViewProposal { id } => {
            let result: Value = client
                .get(&format!("/governance/proposals/{}", id), None)
                .await?;
            Ok(json!({ "summary": "Proposal details retrieved", "proposal": result }))
        },
        GovernanceCommands::CreateProposal {
            title,
            description,
            proposal_type,
            changes,
            voting_period,
        } => {
            let result: Value = client
                .post(
                    "/governance/proposals",
                    Some(json!({
                        "title": title,
                        "description": description,
                        "proposalType": proposal_type,
                        "changes": changes,
                        "votingPeriod": voting_period
                    })),
                )
                .await?;
            Ok(json!({ "summary": "Proposal created", "result": result }))
        },
        GovernanceCommands::Vote {
            id,
            vote,
            justification,
        } => {
            let result: Value = client
                .post(
                    "/governance/vote",
                    Some(json!({
                        "proposalId": id,
                        "vote": vote,
                        "justification": justification
                    })),
                )
                .await?;
            Ok(json!({ "summary": "Vote cast", "result": result }))
        },
        GovernanceCommands::Delegate { to_address, amount } => {
            let result: Value = client
                .post(
                    "/governance/delegate",
                    Some(json!({
                        "toAddress": to_address,
                        "amount": amount
                    })),
                )
                .await?;
            Ok(json!({ "summary": "Voting power delegated", "result": result }))
        },
        GovernanceCommands::VotingHistory => {
            let result: Value = client.get("/governance/voting-history", None).await?;
            Ok(json!({ "summary": "Voting history retrieved", "history": result }))
        },
        GovernanceCommands::UpgradeNotice(_)
        | GovernanceCommands::ScheduleExtension(_)
        | GovernanceCommands::FaucetPop(_) => {
            unreachable!("handled before the API client")
        },
    }
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

/// Same contract as `oracle build-registrations --governance-key`: a file
/// holding the governance private key hex.
fn read_governance_key(path: &Path) -> Result<PrivateKey> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read governance key {}", path.display()))?;
    PrivateKey::from_hex(raw.trim())
        .map_err(|err| anyhow!("governance key at {} is not valid: {err}", path.display()))
}

/// Accept an unsigned notice or an already-signed artifact.
fn read_notice_input(path: &Path) -> Result<SignedUpgradeNotice> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read notice {}", path.display()))?;
    if let Ok(signed) = serde_json::from_str::<SignedUpgradeNotice>(&raw) {
        return Ok(signed);
    }
    let notice: UpgradeNotice = serde_json::from_str(&raw).with_context(|| {
        format!(
            "{} is neither an UpgradeNotice nor a SignedUpgradeNotice",
            path.display()
        )
    })?;
    Ok(SignedUpgradeNotice {
        notice,
        signatures: Vec::new(),
    })
}

/// Accept an unsigned extension or an already-signed artifact.
fn read_extension_input(path: &Path) -> Result<SignedFeatureScheduleExtension> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read extension {}", path.display()))?;
    if let Ok(signed) = serde_json::from_str::<SignedFeatureScheduleExtension>(&raw) {
        return Ok(signed);
    }
    let extension: FeatureScheduleExtension = serde_json::from_str(&raw).with_context(|| {
        format!(
            "{} is neither a FeatureScheduleExtension nor a SignedFeatureScheduleExtension",
            path.display()
        )
    })?;
    Ok(SignedFeatureScheduleExtension {
        extension,
        signatures: Vec::new(),
    })
}

fn parse_allowlist(allowlist: Option<&String>) -> Result<Vec<String>> {
    let allow: Vec<String> = match allowlist {
        Some(raw) => raw
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect(),
        None => oracle_governance_pubkeys(),
    };
    if allow.is_empty() {
        bail!(
            "no governance allowlist: pass --allowlist or set WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS \
             (an empty allowlist authorizes nobody)"
        );
    }
    Ok(allow)
}

/// The faucet key, from a `wrfcoin wallet` keystore JSON or a raw hex
/// ed25519 private key file. Yields `(public key hex, signer)`.
fn read_faucet_key(path: &Path) -> Result<(String, Box<dyn Fn(&[u8]) -> Result<String>>)> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read faucet key {}", path.display()))?;
    if raw.trim_start().starts_with('{') {
        let keystore = wrfcoin_cli::crypto::WrfKeyPair::load_from_file(path)?;
        let public_key = keystore.public_key_hex()?;
        return Ok((public_key, Box::new(move |message| keystore.sign_hex(message))));
    }
    let key = PrivateKey::from_hex(raw.trim())
        .map_err(|err| anyhow!("faucet key at {} is not valid: {err}", path.display()))?;
    let public_key = wrfcoin_shared_protocol::crypto::signature::PublicKey::from_private_key(&key)
        .to_hex()
        .to_ascii_lowercase();
    Ok((public_key, Box::new(move |message| Ok(key.sign(message).to_hex()))))
}

fn canonical_genesis_hash(raw: &str) -> String {
    let lowered = raw.trim().to_ascii_lowercase();
    lowered.strip_prefix("0x").unwrap_or(&lowered).to_string()
}

pub fn handle_faucet_pop(action: &FaucetPopCommands) -> Result<Value> {
    handle_faucet_pop_with_bindings(
        action,
        wrfcoin_shared_protocol::crypto::chain_binding::LEGACY_FAUCET_BINDINGS,
    )
}

fn handle_faucet_pop_with_bindings(
    action: &FaucetPopCommands,
    bindings: &[LegacyFaucetBinding],
) -> Result<Value> {
    match action {
        FaucetPopCommands::Sign {
            faucet_key,
            chain_id,
            network_id,
            genesis_hash,
            out,
        } => {
            let genesis = canonical_genesis_hash(genesis_hash);
            let (public_key, sign) = read_faucet_key(faucet_key)?;
            let key = wrfcoin_shared_protocol::crypto::signature::PublicKey::from_hex(&public_key)
                .map_err(|err| anyhow!("faucet key does not yield an ed25519 public key: {err}"))?;
            let address = Address::from_public_key(&key);
            // Refuse early, before anything is signed, unless this binary
            // commits this address for this chain instance.
            let row = bindings
                .iter()
                .find(|row| {
                    row.genesis_hash == genesis
                        && row.genesis_chain_id == chain_id.as_str()
                        && row.address.and_then(|a| Address::from_hex(a).ok()) == Some(address)
                })
                .ok_or_else(|| {
                    anyhow!(
                        "refusing to sign: this key controls {address}, and no faucet-binding row \
                         in this binary binds {address} on chain {chain_id} genesis {genesis} \
                         (#3361 F3)"
                    )
                })?;
            let possession = FaucetSinkPossession::new(chain_id.clone(), network_id.clone(), genesis.clone(), address);
            let proof = FaucetSinkPossessionProof {
                signature: sign(&possession.preimage())?,
                possession,
                public_key,
            };
            write_pretty_json(out, &proof)?;
            Ok(json!({
                "summary": format!(
                    "Signed faucet proof of possession for {address} as the {} sink of {chain_id}/{network_id} genesis {genesis}",
                    row.feature
                ),
                "output": out.display().to_string(),
                "address": address.to_string(),
                "feature": row.feature.name(),
            }))
        },
    }
}

/// Features this tool refuses to schedule until a named issue lands, even
/// though the binary marks them schedulable. It covers EVERY path in this CLI
/// that emits a feature schedule: the signed extension (`governance
/// schedule-extension sign`) and the genesis artifact (`setup genesis
/// --activate`). This is a tool hold, not consensus: `check_admission_policy`
/// and `verify_feature_schedule_extension` are unchanged, so a chain still
/// accepts a valid extension signed elsewhere.
///
/// The table is empty: core4#3299's elevation bound (DEM -50/+60 m, core4#38)
/// is the trust condition the owner ruled sufficient for
/// `reference_sea_level_pressure`, so the hold on it is lifted. The mechanism
/// stays for any future tool hold.
pub(crate) const SCHEDULING_HELD: &[(&str, &str)] = &[];

/// Refuse (`refusing to <action>: scheduling of `<name>` is held (<why>)`)
/// when any of `features` is in `held`. Shared by every schedule-emitting path.
pub(crate) fn check_held_feature_names<'a>(
    features: impl IntoIterator<Item = &'a str>,
    held: &[(&str, &str)],
    action: &str,
) -> Result<()> {
    for feature in features {
        if let Some((name, why)) = held.iter().find(|(name, _)| *name == feature) {
            bail!("refusing to {action}: scheduling of `{name}` is held ({why})");
        }
    }
    Ok(())
}

fn check_scheduling_held(
    extension: &FeatureScheduleExtension,
    held: &[(&str, &str)],
) -> Result<()> {
    check_held_feature_names(extension.entries.iter().map(|e| e.feature.as_str()), held, "sign")
}

pub fn handle_schedule_extension(action: &ScheduleExtensionCommands) -> Result<Value> {
    handle_schedule_extension_with_bindings(
        action,
        wrfcoin_shared_protocol::crypto::chain_binding::LEGACY_FAUCET_BINDINGS,
    )
}

/// `bindings` is the faucet-binding row table the signing gate resolves
/// against: the committed `LEGACY_FAUCET_BINDINGS` in production, a
/// synthetic row in tests that must hold the sink's key.
fn handle_schedule_extension_with_bindings(
    action: &ScheduleExtensionCommands,
    bindings: &[LegacyFaucetBinding],
) -> Result<Value> {
    match action {
        ScheduleExtensionCommands::Sign {
            extension,
            governance_key,
            out,
            carrying_height,
            faucet_pop,
        } => {
            let mut signed = read_extension_input(extension)?;
            // The block-1 hash is canonical lowercase hex; accept an operator's
            // pasted spelling, but only while nothing has been signed yet —
            // rewriting a signed field would invalidate earlier signatures.
            if signed.signatures.is_empty() {
                let trimmed = signed.extension.genesis_hash.trim().to_ascii_lowercase();
                signed.extension.genesis_hash =
                    trimmed.strip_prefix("0x").unwrap_or(&trimmed).to_string();
            }
            signed
                .extension
                .check_well_formed()
                .map_err(|err| anyhow!("refusing to sign a malformed extension: {err}"))?;
            // Admission policy, not validity (design §1.3): the signing tool
            // refuses exactly what mempool and producer admission refuse — a
            // feature this binary does not implement or does not mark
            // schedulable (`ConsensusFeature::schedulable`), and any
            // signing-version step until S11.
            //
            // Offline, "this node's block 1" is the chain instance the
            // extension names: a faucet binding passes only when a row binds
            // exactly that instance (#3361 F3).
            wrfcoin_shared_protocol::governance::schedule_extension::check_admission_policy(
                &signed.extension,
                Some(&signed.extension.genesis_hash),
            )
            .map_err(|err| anyhow!("refusing to sign: {err}"))?;
            check_scheduling_held(&signed.extension, SCHEDULING_HELD)?;
            if let Some(height) = carrying_height {
                signed
                    .extension
                    .check_activation_window(*height)
                    .map_err(|err| anyhow!("refusing to sign: {err}"))?;
            }
            // #3361 F3: a faucet binding is signed only for its row's chain
            // instance and with the faucet key's proof of possession; the
            // sink it resolves to is printed for confirmation.
            let proof: Option<FaucetSinkPossessionProof> = match faucet_pop {
                Some(path) => Some(read_json(path, "faucet proof of possession")?),
                None => None,
            };
            // Check first (before the governance key is read) so a refusal
            // names the resolved sink; the library signer re-checks.
            let faucet_sink =
                check_faucet_sink_binding_signable_against(bindings, &signed.extension, proof.as_ref())
                    .map_err(|err| anyhow!("refusing to sign: {err}"))?;
            if let Some(sink) = &faucet_sink {
                eprintln!(
                    "faucet sink binding: {} from height {} on chain {} genesis {} -> {}",
                    sink.feature,
                    sink.activation_height,
                    sink.chain_id,
                    sink.genesis_hash,
                    sink.address.as_deref().unwrap_or("NO SINK (explicit unbind)")
                );
            }
            let key = read_governance_key(governance_key)?;
            let (signature, _) = sign_faucet_sink_binding_extension_with_key_against(
                bindings,
                &signed.extension,
                proof.as_ref(),
                &key,
            )
            .map_err(|err| anyhow!("refusing to sign: {err}"))?;
            if signed
                .signatures
                .iter()
                .any(|s| s.signer_pubkey.eq_ignore_ascii_case(&signature.signer_pubkey))
            {
                bail!(
                    "extension is already signed by {}; a signer signs once",
                    signature.signer_pubkey
                );
            }
            let signer_pubkey = signature.signer_pubkey.clone();
            signed.signatures.push(signature);
            write_pretty_json(out, &signed)?;
            Ok(json!({
                "summary": format!(
                    "Signed schedule extension #{} for {}/{} genesis {} ({} entr{}, {} signature(s))",
                    signed.extension.sequence,
                    signed.extension.chain_id,
                    signed.extension.network_id,
                    signed.extension.genesis_hash,
                    signed.extension.entries.len(),
                    if signed.extension.entries.len() == 1 { "y" } else { "ies" },
                    signed.signatures.len()
                ),
                "output": out.display().to_string(),
                "sequence": signed.extension.sequence,
                "signerPubkey": signer_pubkey,
                "signatureCount": signed.signatures.len(),
                "faucetSink": faucet_sink.map(|sink| json!({
                    "feature": sink.feature,
                    "activationHeight": sink.activation_height,
                    "chainId": sink.chain_id,
                    "genesisHash": sink.genesis_hash,
                    "address": sink.address,
                })),
            }))
        },
        ScheduleExtensionCommands::Verify {
            signed,
            genesis_hash,
            chain_id,
            network_id,
            allowlist,
            carrying_height,
        } => {
            let artifact: SignedFeatureScheduleExtension = read_json(signed, "signed extension")?;
            let allow = parse_allowlist(allowlist.as_ref())?;
            let chain = chain_id
                .clone()
                .unwrap_or_else(|| artifact.extension.chain_id.clone());
            let network = network_id
                .clone()
                .unwrap_or_else(|| artifact.extension.network_id.clone());
            // Canonical form is lowercase hex without a prefix; normalise the
            // operator's input BEFORE it is compared.
            let lowered = genesis_hash.trim().to_ascii_lowercase();
            let genesis = lowered.strip_prefix("0x").unwrap_or(&lowered).to_string();
            let extension_json = serde_json::to_value(&artifact.extension)?;
            let context = ScheduleExtensionContext {
                chain_id: &chain,
                network_id: &network,
                genesis_hash: &genesis,
                allowlist: &allow,
            };
            let verdict = verify_feature_schedule_extension(&artifact, &context).and_then(|()| {
                match carrying_height {
                    Some(height) => artifact.extension.check_activation_window(*height),
                    None => Ok(()),
                }
            });
            match verdict {
                Ok(()) => Ok(json!({
                    "summary": format!(
                        "VALID: schedule extension #{} for {}/{} genesis {} — {} entr{} (protocol v{}, \
                         release {}); {} governance signature(s) verified{}",
                        artifact.extension.sequence,
                        chain,
                        network,
                        genesis,
                        artifact.extension.entries.len(),
                        if artifact.extension.entries.len() == 1 { "y" } else { "ies" },
                        artifact.extension.min_protocol_version,
                        artifact.extension.release_commit,
                        artifact.signatures.len(),
                        match carrying_height {
                            Some(h) => format!("; activation window checked at carrying height {h}"),
                            None => "; activation window NOT checked (no --carrying-height)".into(),
                        }
                    ),
                    "verdict": "valid",
                    "extension": extension_json,
                    "signers": artifact.signatures.iter().map(|s| s.signer_pubkey.clone()).collect::<Vec<_>>(),
                    "carryingHeight": carrying_height,
                })),
                Err(err) => Err(anyhow!(
                    "REFUSED: {err}\nextension: {}",
                    serde_json::to_string_pretty(&extension_json)?
                )),
            }
        },
    }
}

pub fn handle_upgrade_notice(action: &UpgradeNoticeCommands) -> Result<Value> {
    match action {
        UpgradeNoticeCommands::Sign {
            notice,
            governance_key,
            out,
        } => {
            let mut signed = read_notice_input(notice)?;
            if signed.notice.notice_id.trim().is_empty() {
                signed.notice.notice_id = UpgradeNotice::notice_id_for(
                    signed.notice.activation_height,
                    &signed.notice.release_commit,
                );
            }
            if signed.notice.chain_id.trim().is_empty() || signed.notice.network_id.trim().is_empty() {
                bail!("notice must name chain_id and network_id; a notice is trusted for one chain only");
            }
            let key = read_governance_key(governance_key)?;
            let signature = sign_upgrade_notice_with_key(&signed.notice, &key);
            if signed
                .signatures
                .iter()
                .any(|s| s.signer_pubkey.eq_ignore_ascii_case(&signature.signer_pubkey))
            {
                bail!(
                    "notice is already signed by {}; a signer signs once",
                    signature.signer_pubkey
                );
            }
            let signer_pubkey = signature.signer_pubkey.clone();
            signed.signatures.push(signature);
            write_pretty_json(out, &signed)?;
            Ok(json!({
                "summary": format!(
                    "Signed upgrade notice {} for {}/{} (activation height {}, {} signature(s))",
                    signed.notice.notice_id,
                    signed.notice.chain_id,
                    signed.notice.network_id,
                    signed.notice.activation_height,
                    signed.signatures.len()
                ),
                "output": out.display().to_string(),
                "noticeId": signed.notice.notice_id,
                "signerPubkey": signer_pubkey,
                "signatureCount": signed.signatures.len(),
            }))
        },
        UpgradeNoticeCommands::Verify {
            signed,
            now,
            chain_id,
            network_id,
            allowlist,
        } => {
            let artifact: SignedUpgradeNotice = read_json(signed, "signed notice")?;
            let allow: Vec<String> = match allowlist {
                Some(raw) => raw
                    .split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect(),
                None => oracle_governance_pubkeys(),
            };
            if allow.is_empty() {
                bail!(
                    "no governance allowlist: pass --allowlist or set WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS \
                     (an empty allowlist authorizes nobody)"
                );
            }
            let chain = chain_id.clone().unwrap_or_else(|| artifact.notice.chain_id.clone());
            let network = network_id
                .clone()
                .unwrap_or_else(|| artifact.notice.network_id.clone());
            let at = now.unwrap_or_else(|| chrono::Utc::now().timestamp());
            let notice_json = serde_json::to_value(&artifact.notice)?;
            match verify_upgrade_notice(&artifact, &chain, &network, &allow, at) {
                Ok(()) => Ok(json!({
                    "summary": format!(
                        "VALID: upgrade notice {} for {}/{} — install release {} before height {} \
                         (protocol v{}); {} governance signature(s) verified",
                        artifact.notice.notice_id,
                        chain,
                        network,
                        artifact.notice.release_commit,
                        artifact.notice.activation_height,
                        artifact.notice.min_protocol_version,
                        artifact.signatures.len()
                    ),
                    "verdict": "valid",
                    "notice": notice_json,
                    "signers": artifact.signatures.iter().map(|s| s.signer_pubkey.clone()).collect::<Vec<_>>(),
                    "verifiedAt": at,
                })),
                Err(err) => Err(anyhow!(
                    "REFUSED: {err}\nnotice: {}",
                    serde_json::to_string_pretty(&notice_json)?
                )),
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};
    use wrfcoin_shared_protocol::crypto::signature::PublicKey;

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("wrfcoin-governance-tests-{name}-{nonce}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write_key(dir: &Path, seed: u64) -> (PathBuf, String) {
        let key = PrivateKey::generate(&mut StdRng::seed_from_u64(seed));
        let path = dir.join(format!("governance-{seed}.key")); // secret-leak-ok: test; `seed` is the u64 test RNG seed in a temp filename
        fs::write(&path, format!("{}\n", key.to_hex())).expect("write key");
        (path, PublicKey::from_private_key(&key).to_hex().to_ascii_lowercase())
    }

    fn write_notice(dir: &Path) -> PathBuf {
        let notice = UpgradeNotice {
            notice_id: String::new(),
            chain_id: "777".into(),
            network_id: "testnet".into(),
            activation_height: 14_000,
            min_protocol_version: 3,
            features: vec!["domain_separated_preimages".into()],
            release_commit: "8d29ea85b0123456".into(),
            image_digests: BTreeMap::from([(
                "native-chain-amd64".to_string(),
                "sha256:aaaa".to_string(),
            )]),
            issued_at: 1_800_000_000,
            expires_at: Some(1_800_086_400),
        };
        let path = dir.join("notice.json");
        fs::write(&path, serde_json::to_string_pretty(&notice).unwrap()).expect("write notice");
        path
    }

    /// Operator token deliberately unset (rule from #3051): these commands
    /// never touch the launch API.
    #[test]
    fn sign_then_verify_offline_round_trip_3094() {
        let dir = temp_dir("roundtrip");
        let (key_a, pub_a) = write_key(&dir, 1);
        let (key_b, pub_b) = write_key(&dir, 2);
        let notice = write_notice(&dir);
        let signed = dir.join("signed.json");

        let result = handle_upgrade_notice(&UpgradeNoticeCommands::Sign {
            notice: notice.clone(),
            governance_key: key_a.clone(),
            out: signed.clone(),
        })
        .expect("sign");
        assert_eq!(result["noticeId"], json!("upgrade-14000-8d29ea85b012"));
        assert_eq!(result["signerPubkey"], json!(pub_a));

        // Second signer appends; the same signer twice is refused.
        let result = handle_upgrade_notice(&UpgradeNoticeCommands::Sign {
            notice: signed.clone(),
            governance_key: key_b,
            out: signed.clone(),
        })
        .expect("second signature");
        assert_eq!(result["signatureCount"], json!(2));
        let err = handle_upgrade_notice(&UpgradeNoticeCommands::Sign {
            notice: signed.clone(),
            governance_key: key_a,
            out: signed.clone(),
        })
        .expect_err("same signer twice");
        assert!(err.to_string().contains("already signed"), "{err}");

        let verdict = handle_upgrade_notice(&UpgradeNoticeCommands::Verify {
            signed: signed.clone(),
            now: Some(1_800_000_100),
            chain_id: None,
            network_id: None,
            allowlist: Some(format!("{pub_a},{PUB_B}", PUB_B = pub_b.to_ascii_uppercase())),
        })
        .expect("verify");
        assert_eq!(verdict["verdict"], json!("valid"));
        assert_eq!(verdict["notice"]["activation_height"], json!(14_000));
        assert_eq!(verdict["signers"].as_array().map(|s| s.len()), Some(2));
    }

    #[test]
    fn verify_refuses_tampering_wrong_chain_expiry_and_unlisted_signers_3094() {
        let dir = temp_dir("refusals");
        let (key_a, pub_a) = write_key(&dir, 3);
        let notice = write_notice(&dir);
        let signed = dir.join("signed.json");
        handle_upgrade_notice(&UpgradeNoticeCommands::Sign {
            notice,
            governance_key: key_a,
            out: signed.clone(),
        })
        .expect("sign");

        let refuse = |signed_path: PathBuf, now: Option<i64>, chain: Option<&str>, allow: &str, needle: &str| {
            let err = handle_upgrade_notice(&UpgradeNoticeCommands::Verify {
                signed: signed_path,
                now,
                chain_id: chain.map(str::to_string),
                network_id: None,
                allowlist: Some(allow.to_string()),
            })
            .expect_err("must refuse");
            let text = err.to_string();
            assert!(text.starts_with("REFUSED"), "{text}");
            assert!(text.contains(needle), "{needle}: {text}");
        };

        refuse(signed.clone(), Some(1_800_000_100), None, "deadbeef", "not in the governance allowlist");
        refuse(signed.clone(), Some(1_800_000_100), Some("888"), &pub_a, "this node is on");
        refuse(signed.clone(), Some(1_900_000_000), None, &pub_a, "expired");

        let mut artifact: SignedUpgradeNotice =
            serde_json::from_str(&fs::read_to_string(&signed).unwrap()).unwrap();
        artifact.notice.activation_height += 1;
        let tampered = dir.join("tampered.json");
        fs::write(&tampered, serde_json::to_string(&artifact).unwrap()).unwrap();
        refuse(tampered, Some(1_800_000_100), None, &pub_a, "does not verify");

        let err = handle_upgrade_notice(&UpgradeNoticeCommands::Verify {
            signed,
            now: Some(1_800_000_100),
            chain_id: None,
            network_id: None,
            allowlist: Some(String::new()),
        })
        .expect_err("empty allowlist");
        assert!(err.to_string().contains("no governance allowlist"), "{err}");
    }

    const S1_GENESIS: &str = "abababababababababababababababababababababababababababababababab";

    fn write_extension(dir: &Path, features: &[(&str, u64)]) -> PathBuf {
        use wrfcoin_shared_protocol::governance::schedule_extension::ScheduleEntry;
        let extension = FeatureScheduleExtension {
            chain_id: "777".into(),
            network_id: "testnet".into(),
            genesis_hash: S1_GENESIS.into(),
            sequence: 0,
            entries: features
                .iter()
                .map(|(name, height)| ScheduleEntry::new(*name, *height))
                .collect(),
            version_steps: Vec::new(),
            min_protocol_version: 3,
            release_commit: "ab1b1a4930000000000000000000000000000000".into(),
        };
        let path = dir.join("extension.json");
        fs::write(&path, serde_json::to_string_pretty(&extension).unwrap()).expect("write extension");
        path
    }

    /// #3277 S1: sign then verify offline; a second signer appends; the same
    /// signer twice is refused. Never touches the launch API.
    #[test]
    fn schedule_extension_sign_then_verify_offline_round_trip_3277() {
        let dir = temp_dir("sched-ext-roundtrip");
        let (key_a, pub_a) = write_key(&dir, 11);
        let (key_b, pub_b) = write_key(&dir, 12);
        let extension = write_extension(&dir, &[("passkey_p256_auth", 25_000), ("verify_unit_pricing", 25_000)]);
        let signed = dir.join("signed.json");

        let result = handle_schedule_extension(&ScheduleExtensionCommands::Sign {
            extension,
            governance_key: key_a.clone(),
            out: signed.clone(),
            carrying_height: Some(21_300),
            faucet_pop: None,
        })
        .expect("sign");
        assert_eq!(result["signerPubkey"], json!(pub_a));
        assert_eq!(result["sequence"], json!(0));

        let result = handle_schedule_extension(&ScheduleExtensionCommands::Sign {
            extension: signed.clone(),
            governance_key: key_b,
            out: signed.clone(),
            carrying_height: None,
            faucet_pop: None,
        })
        .expect("second signature");
        assert_eq!(result["signatureCount"], json!(2));
        let err = handle_schedule_extension(&ScheduleExtensionCommands::Sign {
            extension: signed.clone(),
            governance_key: key_a,
            out: signed.clone(),
            carrying_height: None,
            faucet_pop: None,
        })
        .expect_err("same signer twice");
        assert!(err.to_string().contains("already signed"), "{err}");

        let verdict = handle_schedule_extension(&ScheduleExtensionCommands::Verify {
            signed,
            genesis_hash: S1_GENESIS.into(),
            chain_id: None,
            network_id: None,
            allowlist: Some(format!("{pub_a},{}", pub_b.to_ascii_uppercase())),
            carrying_height: Some(21_300),
        })
        .expect("verify");
        assert_eq!(verdict["verdict"], json!("valid"));
        assert_eq!(verdict["extension"]["entries"][0]["feature"], json!("passkey_p256_auth"));
        assert_eq!(verdict["signers"].as_array().map(|s| s.len()), Some(2));
    }

    /// The signing tool refuses unknown features, malformed extensions and
    /// an out-of-window height — admission policy, before any key is read.
    #[test]
    fn schedule_extension_sign_refuses_unknown_malformed_and_out_of_window_3277() {
        let dir = temp_dir("sched-ext-sign-refusals");
        let (key, _) = write_key(&dir, 13);
        let sign = |features: &[(&str, u64)], carrying: Option<u64>| {
            handle_schedule_extension(&ScheduleExtensionCommands::Sign {
                extension: write_extension(&dir, features),
                governance_key: key.clone(),
                out: dir.join("out.json"),
                carrying_height: carrying,
                faucet_pop: None,
            })
            .expect_err("must refuse")
            .to_string()
        };
        let err = sign(&[("not_a_feature_this_binary_has", 25_000)], None);
        assert!(err.contains("not implemented by this binary"), "{err}");
        let err = sign(&[("signing_v2", 25_000)], None);
        assert!(err.contains("malformed"), "{err}");
        let err = sign(&[("verify_unit_pricing", 25_000), ("session_keys", 24_000)], None);
        assert!(err.contains("out of order"), "{err}");
        let err = sign(&[("verify_unit_pricing", 21_400)], Some(21_300));
        assert!(err.contains("minimum run-up"), "{err}");
        // #3277 S4: every feature `ConsensusFeature::schedulable` marks false.
        for feature in ["paymaster", "social_recovery", "session_keys"] {
            let err = sign(&[(feature, 25_000)], None);
            assert!(err.contains("not schedulable"), "{feature}: {err}");
        }
        // #3361 F3: `S1_GENESIS` is not the faucet row's chain instance, so
        // the bound name is refused here; a suffix with no row, everywhere.
        let err = sign(&[("faucet_sink_binding", 25_000)], None);
        assert!(err.contains("not schedulable here (#3361 F3)"), "{err}");
        let err = sign(&[("faucet_sink_binding_r2", 25_000)], None);
        assert!(err.contains("has no faucet-binding row in this binary"), "{err}");

        // A version-step extension is well-formed but not signable yet.
        let step_path = dir.join("step.json");
        let mut step: FeatureScheduleExtension =
            serde_json::from_str(&fs::read_to_string(write_extension(&dir, &[])).unwrap()).unwrap();
        step.version_steps = vec![
            wrfcoin_shared_protocol::governance::schedule_extension::VersionStep {
                activation_height: 30_000,
                version: 2,
            },
        ];
        fs::write(&step_path, serde_json::to_string(&step).unwrap()).unwrap();
        let err = handle_schedule_extension(&ScheduleExtensionCommands::Sign {
            extension: step_path,
            governance_key: key.clone(),
            out: dir.join("out.json"),
            carrying_height: None,
            faucet_pop: None,
        })
        .expect_err("version step")
        .to_string();
        assert!(err.contains("S11"), "{err}");
    }

    /// core4#3299: a schedule of unheld features still signs, and the
    /// consensus gates never see the list.
    #[test]
    fn schedule_extension_unheld_feature_signs_3299() {
        let dir = temp_dir("sched-ext-hold");
        let (key, _) = write_key(&dir, 14);
        handle_schedule_extension(&ScheduleExtensionCommands::Sign {
            extension: write_extension(&dir, &[("reward_gross_outlier_guard", 25_000)]),
            governance_key: key,
            out: dir.join("out.json"),
            carrying_height: None,
            faucet_pop: None,
        })
        .expect("an unheld schedulable feature still signs");
        assert!(
            wrfcoin_shared_protocol::crypto::chain_binding::ConsensusFeature::parse(
                "reference_sea_level_pressure"
            )
            .is_some_and(|feature| feature.schedulable(None)),
            "consensus accepts it"
        );
    }

    /// core4#3299 follow-up: the owner ruled the elevation bound (DEM -50/+60 m,
    /// core4#38) is the trust condition, so the CLI no longer holds
    /// `reference_sea_level_pressure`: it signs, alone or among others, and the
    /// hold table is empty. The mechanism still refuses whatever a table names.
    #[test]
    fn schedule_extension_sea_level_pressure_signs_with_hold_lifted_3299() {
        let dir = temp_dir("sched-ext-sea-level");
        let (key, _) = write_key(&dir, 15);
        for (i, features) in [
            vec![("reference_sea_level_pressure", 25_000)],
            vec![("reference_sea_level_pressure", 25_000), ("verify_unit_pricing", 25_000)],
        ]
        .into_iter()
        .enumerate()
        {
            let out = dir.join(format!("out-{i}.json"));
            handle_schedule_extension(&ScheduleExtensionCommands::Sign {
                extension: write_extension(&dir, &features),
                governance_key: key.clone(),
                out: out.clone(),
                carrying_height: None,
                faucet_pop: None,
            })
            .expect("reference_sea_level_pressure signs once the hold is lifted");
            assert!(out.exists());
        }
        assert!(SCHEDULING_HELD.is_empty(), "no feature is held by this tool");
        let held = [("reference_sea_level_pressure", "test hold")];
        let err = check_held_feature_names(["reference_sea_level_pressure"], &held, "sign")
            .unwrap_err()
            .to_string();
        assert!(err.contains("scheduling of `reference_sea_level_pressure` is held (test hold)"), "{err}");
    }

    #[test]
    fn schedule_extension_verify_refuses_tampering_other_genesis_and_unlisted_3277() {
        let dir = temp_dir("sched-ext-refusals");
        let (key, pubkey) = write_key(&dir, 14);
        let signed = dir.join("signed.json");
        handle_schedule_extension(&ScheduleExtensionCommands::Sign {
            extension: write_extension(&dir, &[("passkey_p256_auth", 25_000)]),
            governance_key: key,
            out: signed.clone(),
            carrying_height: None,
            faucet_pop: None,
        })
        .expect("sign");

        let refuse = |path: PathBuf, genesis: &str, allow: &str, carrying: Option<u64>, needle: &str| {
            let err = handle_schedule_extension(&ScheduleExtensionCommands::Verify {
                signed: path,
                genesis_hash: genesis.into(),
                chain_id: None,
                network_id: None,
                allowlist: Some(allow.into()),
                carrying_height: carrying,
            })
            .expect_err("must refuse")
            .to_string();
            assert!(err.starts_with("REFUSED"), "{err}");
            assert!(err.contains(needle), "{needle}: {err}");
        };
        refuse(signed.clone(), S1_GENESIS, "deadbeef", None, "not in the governance allowlist");
        // An uppercase or 0x-prefixed paste of the right hash verifies.
        for spelling in [S1_GENESIS.to_ascii_uppercase(), format!("0x{S1_GENESIS}")] {
            let verdict = handle_schedule_extension(&ScheduleExtensionCommands::Verify {
                signed: signed.clone(),
                genesis_hash: spelling,
                chain_id: None,
                network_id: None,
                allowlist: Some(pubkey.clone()),
                carrying_height: None,
            })
            .expect("case-normalised genesis hash verifies");
            assert_eq!(verdict["verdict"], json!("valid"));
        }
        let other = "f".repeat(64);
        refuse(signed.clone(), &other, &pubkey, None, "this chain's block 1 is");
        refuse(signed.clone(), S1_GENESIS, &pubkey, Some(24_000), "minimum run-up");

        let mut artifact: SignedFeatureScheduleExtension =
            serde_json::from_str(&fs::read_to_string(&signed).unwrap()).unwrap();
        artifact.extension.entries[0].activation_height += 1;
        let tampered = dir.join("tampered.json");
        fs::write(&tampered, serde_json::to_string(&artifact).unwrap()).unwrap();
        refuse(tampered, S1_GENESIS, &pubkey, None, "does not verify");
    }

    // ---------------------------------------------------------------------
    // #3361 F3: the faucet-sink binding at the signing tool
    // ---------------------------------------------------------------------

    use wrfcoin_shared_protocol::crypto::chain_binding::LEGACY_FAUCET_BINDINGS;

    /// The live 777 instance: the committed row's block-1 hash.
    fn row_genesis() -> &'static str {
        LEGACY_FAUCET_BINDINGS[0].genesis_hash
    }

    /// A row identical to the committed one except that its sink is an
    /// address whose key THIS test holds (nobody but the faucet service
    /// holds the committed row's key).
    fn synthetic_rows(sink: Address) -> Vec<LegacyFaucetBinding> {
        let hex: &'static str =
            Box::leak(sink.to_string().trim_start_matches("0x").to_string().into_boxed_str());
        vec![LegacyFaucetBinding {
            address: Some(hex),
            ..LEGACY_FAUCET_BINDINGS[0]
        }]
    }

    fn write_faucet_extension(dir: &Path, genesis: &str, feature: &str) -> PathBuf {
        use wrfcoin_shared_protocol::governance::schedule_extension::ScheduleEntry;
        let extension = FeatureScheduleExtension {
            chain_id: "777".into(),
            network_id: "testnet".into(),
            genesis_hash: genesis.into(),
            sequence: 1,
            entries: vec![ScheduleEntry::new(feature, 40_000)],
            version_steps: Vec::new(),
            min_protocol_version: 3,
            release_commit: "ab1b1a4930000000000000000000000000000000".into(),
        };
        let path = dir.join(format!("e0f-{genesis}-{feature}.json"));
        fs::write(&path, serde_json::to_string_pretty(&extension).unwrap()).unwrap();
        path
    }

    fn faucet_pop(
        dir: &Path,
        key: &Path,
        rows: &[LegacyFaucetBinding],
        network: &str,
        genesis: &str,
    ) -> Result<PathBuf> {
        let out = dir.join(format!("pop-{network}-{genesis}.json"));
        handle_faucet_pop_with_bindings(
            &FaucetPopCommands::Sign {
                faucet_key: key.to_path_buf(),
                chain_id: "777".into(),
                network_id: network.into(),
                genesis_hash: genesis.into(),
                out: out.clone(),
            },
            rows,
        )?;
        Ok(out)
    }

    fn sign_e0f(
        dir: &Path,
        rows: &[LegacyFaucetBinding],
        extension: PathBuf,
        pop: Option<PathBuf>,
    ) -> Result<Value> {
        let (governance_key, _) = write_key(dir, 3361);
        handle_schedule_extension_with_bindings(
            &ScheduleExtensionCommands::Sign {
                extension,
                governance_key,
                out: dir.join("e0f-signed.json"),
                carrying_height: Some(37_000),
                faucet_pop: pop,
            },
            rows,
        )
    }

    /// Accept path: the faucet key holder signs a proof (hex key file AND a
    /// `wrfcoin wallet` keystore), governance signs E0f with it, the resolved
    /// sink is reported, and the artifact verifies.
    #[test]
    fn faucet_pop_then_e0f_signs_on_the_rows_chain_and_prints_the_sink_3361() {
        let dir = temp_dir("faucet-e0f-accept");
        let faucet = PrivateKey::generate(&mut StdRng::seed_from_u64(0xfa));
        let sink = Address::from_public_key(&PublicKey::from_private_key(&faucet));
        let key_path = dir.join("faucet.key"); // secret-leak-ok: test; a temp file holding a seeded test key
        fs::write(&key_path, faucet.to_hex()).unwrap();
        let rows = synthetic_rows(sink);

        let pop = faucet_pop(&dir, &key_path, &rows, "testnet", row_genesis()).expect("pop");
        let result = sign_e0f(
            &dir,
            &rows,
            write_faucet_extension(&dir, row_genesis(), "faucet_sink_binding"),
            Some(pop),
        )
        .expect("E0f signs on the row's chain with the faucet key's proof");
        assert_eq!(result["faucetSink"]["address"], json!(sink.to_string()));
        assert_eq!(result["faucetSink"]["genesisHash"], json!(row_genesis()));
        assert_eq!(result["faucetSink"]["feature"], json!("faucet_sink_binding"));
        assert_eq!(result["signatureCount"], json!(1));
        let signed: SignedFeatureScheduleExtension =
            serde_json::from_str(&fs::read_to_string(dir.join("e0f-signed.json")).unwrap()).unwrap();
        assert_eq!(signed.extension.entries[0].feature, "faucet_sink_binding");

        // The same, from a `wrfcoin wallet` keystore.
        let keystore = wrfcoin_cli::crypto::WrfKeyPair::generate().unwrap();
        let keystore_path = dir.join("faucet-keystore.json");
        keystore.save_to_file(&keystore_path).unwrap();
        let keystore_sink = Address::from_hex(&keystore.canonical_address().unwrap()).unwrap();
        let rows = synthetic_rows(keystore_sink);
        let pop = faucet_pop(&dir, &keystore_path, &rows, "testnet", row_genesis()).expect("keystore pop");
        let result = sign_e0f(
            &dir,
            &rows,
            write_faucet_extension(&dir, row_genesis(), "faucet_sink_binding"),
            Some(pop),
        )
        .expect("a keystore proof is as good as a raw-key proof");
        assert_eq!(result["faucetSink"]["address"], json!(keystore_sink.to_string()));

        // A non-faucet extension reports no sink.
        let result = sign_e0f(
            &dir,
            &rows,
            write_faucet_extension(&dir, row_genesis(), "verify_unit_pricing"),
            None,
        )
        .expect("plain extension");
        assert_eq!(result["faucetSink"], Value::Null);
    }

    /// The CLI refusal matrix: mismatched chain instance, missing proof,
    /// wrong key, proof for another network / genesis / address, a proof
    /// for an extension that binds nothing, and a faucet-pop for an address
    /// no row names.
    #[test]
    fn e0f_signing_refuses_every_mismatch_3361() {
        let dir = temp_dir("faucet-e0f-refusals");
        let faucet = PrivateKey::generate(&mut StdRng::seed_from_u64(0xfa));
        let sink = Address::from_public_key(&PublicKey::from_private_key(&faucet));
        let key_path = dir.join("faucet.key"); // secret-leak-ok: test; a temp file holding a seeded test key
        fs::write(&key_path, faucet.to_hex()).unwrap();
        let rows = synthetic_rows(sink);
        let good_pop = faucet_pop(&dir, &key_path, &rows, "testnet", row_genesis()).unwrap();
        let e0f = |genesis: &str, feature: &str| write_faucet_extension(&dir, genesis, feature);
        let refused = |extension: PathBuf, pop: Option<PathBuf>, needle: &str| {
            let err = sign_e0f(&dir, &rows, extension, pop).expect_err("must refuse").to_string();
            assert!(err.contains("refusing to sign"), "{err}");
            assert!(err.contains(needle), "{needle}: {err}");
        };

        // Mismatched chain instance: refused even with a good proof.
        refused(e0f(S1_GENESIS, "faucet_sink_binding"), Some(good_pop.clone()), "not schedulable here");
        // No row for the suffix.
        refused(e0f(row_genesis(), "faucet_sink_binding_r2"), Some(good_pop.clone()), "has no faucet-binding row");
        // No proof: refused, and the refusal names the sink to confirm.
        refused(e0f(row_genesis(), "faucet_sink_binding"), None, &format!("no proof of possession from the faucet key for {sink}"));
        // A proof by another key, for the right claim.
        let stranger = PrivateKey::generate(&mut StdRng::seed_from_u64(0xbad));
        let stranger_proof = wrfcoin_shared_protocol::governance::faucet_sink_pop::sign_faucet_sink_possession(
            &FaucetSinkPossession::new("777", "testnet", row_genesis(), sink),
            &stranger,
        );
        let stranger_path = dir.join("stranger-pop.json");
        fs::write(&stranger_path, serde_json::to_string(&stranger_proof).unwrap()).unwrap();
        refused(e0f(row_genesis(), "faucet_sink_binding"), Some(stranger_path), "key controls");
        // Proofs by the right key for another network / instance / address.
        let other_network = faucet_pop(&dir, &key_path, &rows, "mainnet", row_genesis()).unwrap();
        refused(e0f(row_genesis(), "faucet_sink_binding"), Some(other_network), "is for chain");
        let mut other_genesis_rows = rows.clone();
        other_genesis_rows[0].genesis_hash = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
        let other_genesis = faucet_pop(&dir, &key_path, &other_genesis_rows, "testnet", other_genesis_rows[0].genesis_hash).unwrap();
        refused(e0f(row_genesis(), "faucet_sink_binding"), Some(other_genesis), "bound to genesis");
        let mut other_address = stranger_proof.clone();
        other_address.possession.address = Address([0x39; 20]).to_string();
        other_address.public_key = PublicKey::from_private_key(&faucet).to_hex();
        let other_address_path = dir.join("other-address-pop.json");
        fs::write(&other_address_path, serde_json::to_string(&other_address).unwrap()).unwrap();
        refused(e0f(row_genesis(), "faucet_sink_binding"), Some(other_address_path), "is for address");
        // A proof handed in for an extension that binds nothing.
        refused(e0f(row_genesis(), "verify_unit_pricing"), Some(good_pop), "binds no faucet");

        // faucet-pop itself refuses a key no row names, before signing.
        let err = faucet_pop(&dir, &key_path, LEGACY_FAUCET_BINDINGS, "testnet", row_genesis())
            .expect_err("no committed row names a test key")
            .to_string();
        assert!(err.contains(&format!("this key controls {sink}")), "{err}");
        assert!(err.contains("no faucet-binding row"), "{err}");
    }

    /// Production path (the committed row): without the faucet service's
    /// proof the tool refuses, and names the committed sink it would bind.
    #[test]
    fn the_committed_row_needs_the_faucet_services_proof_3361() {
        let dir = temp_dir("faucet-e0f-committed");
        let (governance_key, _) = write_key(&dir, 3362);
        let err = handle_schedule_extension(&ScheduleExtensionCommands::Sign {
            extension: write_faucet_extension(&dir, row_genesis(), "faucet_sink_binding"),
            governance_key,
            out: dir.join("out.json"),
            carrying_height: None,
            faucet_pop: None,
        })
        .expect_err("no proof")
        .to_string();
        let committed = LEGACY_FAUCET_BINDINGS[0].address.unwrap();
        assert!(err.contains(&format!("0x{committed}")), "{err}");
        assert!(err.contains("no proof of possession"), "{err}");
    }
}
