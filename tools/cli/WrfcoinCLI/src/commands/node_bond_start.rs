//! `node bond start`: the manual testnet bonding steps as one resumable pass.
//!
//! Each run reads `<state-dir>/bond-start.json`, does every step it can, and
//! stops at the first one that has to wait:
//!
//! 1. load the owner, server and consumer key files (public keys only leave
//!    this process; no seed is printed);
//! 2. owner funds: if the owner holds less than the bond plus the fee, print
//!    the `grant-request` command to run by hand (it asks a captcha question,
//!    so it is never run from here) and wait;
//! 3. keyless Deposit plan (`deposit --dry-run --admission-cosign`) with the
//!    generated alias `node-<first 8 hex of the server key>`;
//! 4. server and consumer signatures;
//! 5. `admission-request` for the node's public address, POSTed once to the
//!    admission URL (no credential is sent);
//! 6. passkey payout approval: print the approval page link and challenge,
//!    then wait for the approved file;
//! 7. attach it, sign as owner, keep the packet and submit it to the local
//!    node; later runs re-post the same packet until the bond is REGISTERED.
//!
//! Every packet is written once to `<state-dir>/attempt-<n>/` and never
//! signed again. If the Deposit's effective epoch passes without inclusion,
//! the run says so and the next run starts a new attempt from step 3 (the
//! admission service counts that as a renewal of the same deployment).
//!
//! Exit 0: registered. Exit 2 (`RETRY_LATER`): waiting; run again later.
//! Testnet only, like every software-key bond path.
use anyhow::{Result, ensure};
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use wrfcoin_cli::{ApiClient, node_operator::OperatorKey};
use wrfcoin_shared_protocol::{
    blockchain::transaction::Transaction,
    common::types::Address,
    consensus::{
        node_bond_registry::{NODE_BOND_UNITS, NodeBondLifecycle},
        node_share_contracts::NodeBondAction,
    },
    crypto::{
        chain_binding::{ChainIdentity, SigningProtocolVersion},
        domain_signing::verify_domain_for,
        signature::{PublicKey, Signature},
    },
    governance::schedule_extension::bond_admission_keys,
};

use super::node_bond::{BondSignerRole, read_transaction, sign_with_operator_key};
use super::node_deposit::{self, DepositArgs};
use super::node_receipt::{bounded_read, durable_packet, owner_only_dir, parse_u64, retry_later};

/// The only network this command runs on.
pub const BOND_START_NETWORK: &str = "testnet";
/// File name of the state kept in `--state-dir`.
pub const STATE_FILE: &str = "bond-start.json";
/// The node drops a node transaction whose (unsigned) wrapper timestamp is
/// more than an hour old. Past this age the same signed packet is rewrapped
/// with a fresh timestamp before it is posted again; nothing is re-signed.
pub const REWRAP_AFTER_SECS: i64 = 45 * 60;
const DEFAULT_APPROVAL_PAGE: &str = "https://wrfcoin.com/wallet/passkey/bond-payout";
const ADMISSION_ROUTE: &str = "/api/bond-admission";
const MAX_ADMISSION_RESPONSE: usize = 128 * 1024;

#[derive(Args)]
pub struct StartArgs {
    /// Owner-only folder for this command's state (`bond-start.json`) and
    /// every packet it writes. Use one folder per bond.
    #[clap(long)]
    pub state_dir: PathBuf,
    /// Owner key file (mode 0600 wallet JSON or 64-hex seed). The bond's 250
    /// WRF and the fee are paid from this account.
    #[clap(long)]
    pub owner_key_file: PathBuf,
    /// The node's own server key file (64-hex seed, mode 0600).
    #[clap(long)]
    pub server_key_file: PathBuf,
    /// Consumer key file (64-hex seed, mode 0600), distinct from the others.
    #[clap(long)]
    pub consumer_key_file: PathBuf,
    /// Passkey wallet address that receives the bond's rewards.
    #[clap(long)]
    pub payout_address: String,
    /// Where you save the approved file the payout approval page gives back.
    /// Until it exists, the run stops with exit 2.
    #[clap(long)]
    pub payout_approval_file: PathBuf,
    /// This node's public P2P address, ending in its own peer id
    /// (e.g. /ip4/203.0.113.7/tcp/9000/p2p/12D3Koo…).
    #[clap(long)]
    pub multiaddr: String,
    /// Existing protobuf ed25519 identity; default $WRFCOIN_DATA_DIR/peer_identity.key (./data if unset).
    #[clap(long)]
    pub p2p_key_file: Option<PathBuf>,
    /// Bond admission endpoint, e.g. https://<backend>/api/bond-admission.
    /// https only (http only for a loopback host); no credential is sent.
    #[clap(long)]
    pub admission_url: String,
    /// Payout approval page shown to the passkey wallet holder.
    #[clap(long, default_value = DEFAULT_APPROVAL_PAGE)]
    pub approval_page_url: String,
    /// This machine's own node.
    #[clap(long, default_value = "http://127.0.0.1:8556")]
    pub api_url: String,
    /// Owner-only file containing the local node's Operator bearer.
    #[clap(long)]
    pub operator_token_file: PathBuf,
    #[clap(long, default_value = "888")]
    pub chain_id: String,
    #[clap(long, default_value = "1000000")]
    pub fee: u64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Step {
    /// Nothing written for this attempt yet.
    Start,
    /// Keyless Deposit plan written.
    Planned,
    /// Server and consumer signatures written.
    Signed,
    /// Admission request (peer proof) written; the POST may be retried.
    Requested,
    /// Admission co-sign received; waiting for the payout approval.
    Admitted,
    /// Final packet written and posted; re-posted until registered.
    Submitted,
    Registered,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Files {
    plan: Option<PathBuf>,
    signed: Option<PathBuf>,
    request: Option<PathBuf>,
    admitted: Option<PathBuf>,
    submitted: Option<PathBuf>,
}

/// Public values only: keys, addresses, ids and file paths.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct State {
    version: u32,
    chain_id: String,
    network_id: String,
    genesis_hash: String,
    deployment_id: String,
    owner_key: String,
    server_key: String,
    consumer_key: String,
    payout_address: String,
    multiaddr: String,
    service_alias: String,
    admission_key: String,
    attempt: u32,
    step: Step,
    effective_epoch: Option<u64>,
    expected_nonce: Option<u64>,
    #[serde(default)]
    grant_instructed: bool,
    #[serde(default)]
    files: Files,
}

impl State {
    fn bindings(&self) -> [&str; 10] {
        [
            &self.chain_id,
            &self.network_id,
            &self.genesis_hash,
            &self.owner_key,
            &self.server_key,
            &self.consumer_key,
            &self.payout_address,
            &self.multiaddr,
            &self.service_alias,
            &self.admission_key,
        ]
    }
    fn attempt_dir(&self, state_dir: &Path) -> PathBuf {
        state_dir.join(format!("attempt-{}", self.attempt))
    }
    fn file(&self, path: &Option<PathBuf>, what: &str) -> Result<PathBuf> {
        path.clone()
            .ok_or_else(|| anyhow::anyhow!("state names no {what} packet; inspect {STATE_FILE}"))
    }
}

fn load_state(path: &Path) -> Result<Option<State>> {
    if !path.exists() {
        return Ok(None);
    }
    let state: State = serde_json::from_slice(&bounded_read(path, 64 * 1024)?)
        .map_err(|_| anyhow::anyhow!("{STATE_FILE} unreadable; inspect it before running again"))?;
    ensure!(state.version == 1, "{STATE_FILE} version not supported");
    Ok(Some(state))
}

/// Write the state to a temporary file and rename it over the old one.
fn save_state(path: &Path, state: &State) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    file.write_all(&serde_json::to_vec_pretty(state)?)?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn hex_key(key: &OperatorKey) -> String {
    hex::encode(key.public_key().as_bytes())
}

/// https anywhere, plain http only to a loopback host; never a credential.
fn admission_endpoint(text: &str) -> Result<url::Url> {
    let url = url::Url::parse(text).map_err(|_| anyhow::anyhow!("--admission-url refused"))?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    ensure!(
        (url.scheme() == "https" || (url.scheme() == "http" && loopback))
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "--admission-url must be https (http only to a loopback host), without credentials, query or fragment"
    );
    Ok(url)
}

/// The backend the admission URL belongs to, for the printed grant command.
fn grant_backend(admission: &url::Url) -> String {
    let path = admission.path().trim_end_matches('/');
    match path.strip_suffix(ADMISSION_ROUTE) {
        Some(prefix) => format!("{}{prefix}", admission.origin().ascii_serialization()),
        None => "<backend url>".into(),
    }
}

fn shell_path(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

fn registered_for(view: &Value, owner: &Address, server_key: &[u8; 32]) -> Result<bool> {
    if view["status"] != "REGISTERED" {
        return Ok(false);
    }
    let binding = &view["registration"]["binding"];
    ensure!(
        binding["owner_account"] == serde_json::to_value(owner)?
            && binding["server_key"] == serde_json::to_value(server_key)?,
        "this deployment is registered to another owner or server key"
    );
    Ok(true)
}

/// Check the admission service's answer against the packet that was sent:
/// same transaction and authorization, same signatures, plus a co-sign that
/// verifies under the admission key named in the packet.
fn check_admitted(
    data: &Value,
    sent: &Transaction,
    state: &State,
    identity: &ChainIdentity,
) -> Result<Transaction> {
    ensure!(
        data["admitted"] == true
            && data["action"] == "Deposit"
            && data["deploymentId"].as_str() == Some(state.deployment_id.as_str())
            && data["effectiveEpoch"].as_u64() == state.effective_epoch,
        "admission answer does not match this Deposit"
    );
    let tx: Transaction = serde_json::from_value(data["transaction"].clone())
        .map_err(|_| anyhow::anyhow!("admission answer carries no transaction"))?;
    let got = NodeBondLifecycle::parse(&tx)
        .ok_or_else(|| anyhow::anyhow!("admission answer is not a Node lifecycle transaction"))?;
    let want = NodeBondLifecycle::parse(sent)
        .ok_or_else(|| anyhow::anyhow!("invalid Node lifecycle transaction"))?;
    ensure!(
        got.signed == want.signed
            && got.owner_key == want.owner_key
            && got.governance_key == want.governance_key
            && got.payout_key == want.payout_key
            && got.payout_signature.is_empty()
            && got.payout_passkey.is_none()
            && node_deposit::normalized(&tx)? == node_deposit::normalized(sent)?,
        "admission answer changed the packet"
    );
    let key = PublicKey::from_bytes(&node_deposit::id(&state.admission_key)?)?;
    let sig = Signature::from_bytes(&got.governance_signature)
        .map_err(|_| anyhow::anyhow!("admission answer carries no co-sign"))?;
    ensure!(
        verify_domain_for(
            &key,
            &got.signed.authorization,
            &sig,
            identity,
            SigningProtocolVersion::V2
        ),
        "admission co-sign does not verify"
    );
    Ok(tx)
}

/// POST the request bytes once. `Ok(Err(detail))`: the service said wait or
/// could not be reached; `Err`: a refusal no later run can change.
async fn post_admission(
    url: &url::Url,
    body: Vec<u8>,
) -> Result<std::result::Result<Value, String>> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        // The service may dial the node and run its checks: up to ~90 s.
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|_| anyhow::anyhow!("HTTP client unavailable"))?;
    let response = match client
        .post(url.clone())
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return Ok(Err("admission service unreachable".into())),
    };
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let mut response = response;
    let mut bytes = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        ensure!(
            bytes.len() + chunk.len() <= MAX_ADMISSION_RESPONSE,
            "admission answer exceeds bound"
        );
        bytes.extend_from_slice(&chunk);
    }
    let payload: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(Ok(payload));
    }
    let code = payload["error"]["code"]
        .as_str()
        .filter(|c| c.len() <= 64 && c.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'))
        .unwrap_or("ADMISSION_UNAVAILABLE")
        .to_string();
    let message = payload["error"]["message"]
        .as_str()
        .filter(|m| m.len() <= 200 && !m.chars().any(char::is_control))
        .unwrap_or("");
    let wait = retry_after
        .map(|s| format!(" (retry after {s} s)"))
        .unwrap_or_default();
    let detail = format!("admission {status} {code}: {message}{wait}");
    // Refusals that a later run cannot change are errors; everything else waits.
    let final_refusal = matches!(status, 400 | 413 | 422)
        || matches!(
            code.as_str(),
            "SERVER_KEY_ALREADY_ADMITTED" | "ADDRESS_ALREADY_ADMITTED" | "NODE_WRONG_CHAIN"
        );
    if final_refusal {
        anyhow::bail!("{detail}");
    }
    Ok(Err(detail))
}

fn print_grant_instructions(
    args: &StartArgs,
    owner: &Address,
    admission: &url::Url,
    need: u128,
    have: u128,
) {
    let p2p = args
        .p2p_key_file
        .as_ref()
        .map(|p| format!(" --p2p-key-file {}", shell_path(p)))
        .unwrap_or_default();
    eprintln!(
        "The owner account {owner} holds {have} base units; the bond and fee need {need}.\n\
         Ask the testnet faucet for the bond grant yourself (it asks one captcha question), then run this command again:\n\n  \
         wrfcoin node bond grant-request --owner-address {owner} --api-url {} --key-file {}{p2p} --submit {}\n",
        args.api_url,
        shell_path(&args.server_key_file),
        grant_backend(admission),
    );
}

fn print_payout_instructions(args: &StartArgs, admitted: &Path, challenge: &Value) {
    eprintln!(
        "Payout approval needed. On the device that holds the passkey wallet {}:\n  \
         1. open {}\n  \
         2. load this file there: {}\n  \
         3. save the approved file it gives back as: {}\n  \
         4. run this command again (before the Deposit's epoch {} closes).\n\
         Challenge the passkey signs: {}",
        args.payout_address,
        args.approval_page_url,
        admitted.display(),
        args.payout_approval_file.display(),
        challenge["effective_epoch"],
        challenge["challenge_hex"].as_str().unwrap_or(""),
    );
}

/// `final-<n>.json`, the first name not yet taken in this attempt.
fn next_final(dir: &Path) -> PathBuf {
    (1u32..)
        .map(|n| dir.join(format!("final-{n}.json")))
        .find(|p| !p.exists())
        .expect("a free final packet name")
}

fn status(state: &State, status: &str) -> Value {
    json!({
        "status": status,
        "step": state.step,
        "attempt": state.attempt,
        "deployment_id": state.deployment_id,
        "service_alias": state.service_alias,
        "effective_epoch": state.effective_epoch,
        "submitted_packet": state.files.submitted,
    })
}

pub async fn handle(args: &StartArgs) -> Result<Value> {
    let identity = ChainIdentity::new(&args.chain_id, BOND_START_NETWORK);
    let admission = admission_endpoint(&args.admission_url)?;
    let owner = OperatorKey::load(&args.owner_key_file)?;
    let server = OperatorKey::load(&args.server_key_file)?;
    let consumer = OperatorKey::load(&args.consumer_key_file)?;
    let (owner_key, server_key, consumer_key) =
        (hex_key(&owner), hex_key(&server), hex_key(&consumer));
    ensure!(
        owner_key != server_key && owner_key != consumer_key && server_key != consumer_key,
        "owner, server and consumer keys must all differ"
    );
    let payout: Address = args
        .payout_address
        .parse()
        .map_err(|_| anyhow::anyhow!("--payout-address refused"))?;
    ensure!(
        payout != owner.address(),
        "payout must differ from the owner account"
    );
    let multiaddr: libp2p::Multiaddr = args
        .multiaddr
        .parse()
        .map_err(|_| anyhow::anyhow!("--multiaddr is not a valid P2P multiaddr"))?;

    let api: ApiClient = node_deposit::operator_api(&args.api_url, &args.operator_token_file)?;
    let network = api.receipt_request("GET", "/network/info", None).await?;
    ensure!(
        network["chain_id"].as_str() == Some(args.chain_id.as_str())
            && network["network_id"].as_str() == Some(BOND_START_NETWORK),
        "the node at --api-url is not chain {} {BOND_START_NETWORK}",
        args.chain_id
    );
    let genesis = network["genesis_hash"]
        .as_str()
        .and_then(node_deposit::api_hash)
        .map(hex::encode)
        .ok_or_else(|| anyhow::anyhow!("node genesis unavailable"))?;
    let admission_key = bond_admission_keys(&genesis)
        .into_iter()
        .next()
        .ok_or_else(|| {
            anyhow::anyhow!("this binary pins no bond admission key for this chain; update the CLI")
        })?;

    owner_only_dir(&args.state_dir)?;
    let state_path = args.state_dir.join(STATE_FILE);
    let fresh = State {
        version: 1,
        chain_id: args.chain_id.clone(),
        network_id: BOND_START_NETWORK.into(),
        genesis_hash: genesis.clone(),
        deployment_id: hex::encode(rand::random::<[u8; 32]>()),
        owner_key: owner_key.clone(),
        server_key: server_key.clone(),
        consumer_key: consumer_key.clone(),
        payout_address: payout.to_string(),
        multiaddr: multiaddr.to_string(),
        service_alias: format!("node-{}", &server_key[..8]),
        admission_key,
        attempt: 1,
        step: Step::Start,
        effective_epoch: None,
        expected_nonce: None,
        grant_instructed: false,
        files: Files::default(),
    };
    let mut state = match load_state(&state_path)? {
        Some(kept) => {
            ensure!(
                kept.bindings() == fresh.bindings(),
                "{} belongs to another bond (keys, payout, address or chain differ); use a new --state-dir",
                state_path.display()
            );
            kept
        },
        None => {
            save_state(&state_path, &fresh)?;
            fresh
        },
    };
    let owner_address = owner.address();
    let server_bytes = node_deposit::id(&server_key)?;
    let bond_path = format!("/node/bonds/{}", state.deployment_id);

    let view = api.receipt_request("GET", &bond_path, None).await?;
    if registered_for(&view, &owner_address, &server_bytes)? {
        if state.step != Step::Registered {
            state.step = Step::Registered;
            save_state(&state_path, &state)?;
        }
        return Ok(status(&state, "REGISTERED"));
    }
    ensure!(
        state.step != Step::Registered,
        "{STATE_FILE} says registered but the node does not; inspect the bond before running again"
    );
    let account = api
        .receipt_request("GET", &format!("/addresses/{owner_address}"), None)
        .await?;
    let nonce = parse_u64(&account["nonce"])
        .or_else(|| account["nonce"].as_u64())
        .ok_or_else(|| anyhow::anyhow!("owner nonce unavailable"))?;

    // A packet whose epoch has begun, or whose nonce another transaction
    // used, can never be included: start a new attempt on the next run.
    if state.step > Step::Start {
        let node_epoch = parse_u64(&view["epoch"]);
        let epoch_passed =
            matches!((node_epoch, state.effective_epoch), (Some(now), Some(e)) if now >= e);
        let nonce_used = state.expected_nonce.is_some_and(|n| nonce > n);
        if epoch_passed || nonce_used {
            let why = if epoch_passed {
                format!(
                    "effective epoch {} began",
                    state.effective_epoch.unwrap_or_default()
                )
            } else {
                "the owner nonce was used by another transaction".to_string()
            };
            let old = state.attempt;
            state.attempt += 1;
            state.step = Step::Start;
            state.effective_epoch = None;
            state.expected_nonce = None;
            state.files = Files::default();
            save_state(&state_path, &state)?;
            return Err(retry_later(format!(
                "attempt {old} was not included ({why}); its packets stay in attempt-{old}. \
                 Run again to renew from a new Deposit plan (attempt {})",
                state.attempt
            )));
        }
    }

    loop {
        match state.step {
            Step::Start => {
                let balance = account["spendable_balance"]
                    .as_str()
                    .or(account["balance"].as_str())
                    .and_then(|b| b.parse::<u128>().ok())
                    .unwrap_or(0);
                let need = u128::from(NODE_BOND_UNITS) + u128::from(args.fee);
                if balance < need {
                    print_grant_instructions(args, &owner_address, &admission, need, balance);
                    if !state.grant_instructed {
                        state.grant_instructed = true;
                        save_state(&state_path, &state)?;
                    }
                    return Err(retry_later(
                        "waiting for owner funds: run the grant-request command above yourself, then run again"
                            .to_string(),
                    ));
                }
                let activation = parse_u64(&view["activation_height"])
                    .ok_or_else(|| anyhow::anyhow!("node bonds are not active on this node yet"))?;
                let dir = state.attempt_dir(&args.state_dir);
                owner_only_dir(&dir)?;
                let plan_path = dir.join("plan.json");
                // A keyless plan left by an interrupted run carries no signature.
                if plan_path.exists() {
                    std::fs::remove_file(&plan_path)?;
                }
                let plan = node_deposit::handle(&DepositArgs {
                    deployment_id: state.deployment_id.clone(),
                    owner_key: owner_key.clone(),
                    payout_address: state.payout_address.clone(),
                    payout_key: None,
                    payout_passkey_assertion: None,
                    server_key: server_key.clone(),
                    consumer_key: consumer_key.clone(),
                    governance_key: state.admission_key.clone(),
                    admission_cosign: true,
                    service_alias: state.service_alias.clone(),
                    genesis_hash: genesis.clone(),
                    chain_id: args.chain_id.clone(),
                    network_id: BOND_START_NETWORK.into(),
                    activation_height: activation,
                    api_url: args.api_url.clone(),
                    operator_token_file: args.operator_token_file.clone(),
                    key_file: None,
                    approvals_file: None,
                    fee: args.fee,
                    out: Some(plan_path.clone()),
                    dry_run: true,
                })
                .await
                .map_err(|e| {
                    let text = e.to_string();
                    if text.contains("too close to the end of epoch") {
                        retry_later(text)
                    } else {
                        e
                    }
                })?;
                let tx = read_transaction(&plan_path)?;
                state.effective_epoch = plan["effective_epoch"].as_u64();
                state.expected_nonce = Some(tx.nonce);
                state.files.plan = Some(plan_path);
                state.step = Step::Planned;
                save_state(&state_path, &state)?;
            },
            Step::Planned => {
                let plan = read_transaction(&state.file(&state.files.plan, "plan")?)?;
                let out = state.attempt_dir(&args.state_dir).join("signed.json");
                if out.exists() {
                    // Written by an interrupted run: keep it if it signs this plan.
                    let kept = read_transaction(&out)?;
                    ensure!(
                        NodeBondLifecycle::parse(&kept).map(|p| p.signed.authorization)
                            == NodeBondLifecycle::parse(&plan).map(|p| p.signed.authorization),
                        "{} is for another plan; inspect it",
                        out.display()
                    );
                } else {
                    let mut tx = plan;
                    sign_with_operator_key(&mut tx, BondSignerRole::Server, &server, &identity)?;
                    sign_with_operator_key(
                        &mut tx,
                        BondSignerRole::Consumer,
                        &consumer,
                        &identity,
                    )?;
                    durable_packet(&out, &tx)?;
                }
                state.files.signed = Some(out);
                state.step = Step::Signed;
                save_state(&state_path, &state)?;
            },
            Step::Signed => {
                let signed = state.file(&state.files.signed, "signed")?;
                let out = state.attempt_dir(&args.state_dir).join("request.json");
                if !out.exists() {
                    super::node_bond_admit::handle_request(
                        &super::node_bond_admit::AdmissionRequestArgs {
                            transaction_file: signed,
                            multiaddr: state.multiaddr.clone(),
                            chain_id: args.chain_id.clone(),
                            network_id: BOND_START_NETWORK.into(),
                            p2p_key_file: args.p2p_key_file.clone(),
                            out: Some(out.clone()),
                        },
                    )
                    .await?;
                }
                state.files.request = Some(out);
                state.step = Step::Requested;
                save_state(&state_path, &state)?;
            },
            Step::Requested => {
                let request_path = state.file(&state.files.request, "request")?;
                // Always the bytes on disk: a retry sends exactly what was sent before.
                let body = bounded_read(&request_path, 48 * 1024)?;
                let signed = read_transaction(&state.file(&state.files.signed, "signed")?)?;
                let answer = match post_admission(&admission, body).await? {
                    Ok(answer) => answer,
                    Err(detail) => {
                        return Err(retry_later(format!(
                            "{detail}; the same admission request is sent again on the next run"
                        )));
                    },
                };
                let tx = check_admitted(&answer["data"], &signed, &state, &identity)?;
                let out = state.attempt_dir(&args.state_dir).join("admitted.json");
                if out.exists() {
                    ensure!(
                        read_transaction(&out)?.data == tx.data,
                        "{} holds another admission answer; inspect it",
                        out.display()
                    );
                } else {
                    durable_packet(&out, &tx)?;
                }
                state.files.admitted = Some(out);
                state.step = Step::Admitted;
                save_state(&state_path, &state)?;
            },
            Step::Admitted => {
                let admitted_path = state.file(&state.files.admitted, "admitted")?;
                let mut tx = read_transaction(&admitted_path)?;
                let p = NodeBondLifecycle::parse(&tx)
                    .ok_or_else(|| anyhow::anyhow!("invalid Node lifecycle transaction"))?;
                ensure!(
                    p.signed.authorization.action == NodeBondAction::Deposit,
                    "not a Deposit"
                );
                let challenge =
                    super::node_bond::payout_challenge_json(&p.signed.authorization, &identity);
                if !args.payout_approval_file.exists() {
                    print_payout_instructions(args, &admitted_path, &challenge);
                    return Err(retry_later(format!(
                        "waiting for payout approval: save the approved file as {} and run again",
                        args.payout_approval_file.display()
                    )));
                }
                if let Err(e) = super::node_bond::attach_payout_passkey(
                    &mut tx,
                    &args.payout_approval_file,
                    &identity,
                ) {
                    print_payout_instructions(args, &admitted_path, &challenge);
                    return Err(retry_later(format!(
                        "waiting for payout approval: {} does not approve this packet ({e}); \
                         approve {} and save the result there",
                        args.payout_approval_file.display(),
                        admitted_path.display()
                    )));
                }
                sign_with_operator_key(&mut tx, BondSignerRole::Owner, &owner, &identity)?;
                tx.timestamp = chrono::Utc::now();
                tx.set_id();
                let full = NodeBondLifecycle::parse(&tx)
                    .ok_or_else(|| anyhow::anyhow!("invalid Node lifecycle transaction"))?;
                full.verify_with_gates(
                    &identity,
                    &node_deposit::id(&genesis)?,
                    &[],
                    std::slice::from_ref(&state.admission_key),
                    true,
                )?;
                ensure!(
                    serde_json::to_vec(&tx)?.len() <= 40 * 1024,
                    "Deposit packet exceeds cap"
                );
                // A final packet left by an interrupted run is kept, never overwritten.
                let out = next_final(&state.attempt_dir(&args.state_dir));
                durable_packet(&out, &tx)?;
                state.files.submitted = Some(out);
                state.step = Step::Submitted;
                save_state(&state_path, &state)?;
            },
            Step::Submitted => {
                let mut path = state.file(&state.files.submitted, "submitted")?;
                let kept = read_transaction(&path)?;
                let age = chrono::Utc::now().timestamp() - kept.timestamp.timestamp();
                if age > REWRAP_AFTER_SECS {
                    // Same signed lifecycle bytes (`data`), fresh unsigned wrapper.
                    let mut tx = kept.clone();
                    tx.timestamp = chrono::Utc::now();
                    tx.set_id();
                    let next = next_final(&state.attempt_dir(&args.state_dir));
                    durable_packet(&next, &tx)?;
                    state.files.submitted = Some(next.clone());
                    save_state(&state_path, &state)?;
                    path = next;
                }
                let body: Value = serde_json::from_slice(&bounded_read(&path, 40 * 1024)?)?;
                let ack = api
                    .receipt_request("POST", "/node/bonds/submit", Some(body))
                    .await;
                let view = api
                    .receipt_request("GET", &bond_path, None)
                    .await
                    .unwrap_or(Value::Null);
                if registered_for(&view, &owner_address, &server_bytes)? {
                    state.step = Step::Registered;
                    save_state(&state_path, &state)?;
                    return Ok(status(&state, "REGISTERED"));
                }
                let detail = match ack {
                    Ok(a) if a["admitted"] == true => "admitted, waiting for inclusion".to_string(),
                    Ok(_) => "posted, waiting for inclusion".to_string(),
                    Err(e) => e.to_string(),
                };
                return Err(retry_later(format!(
                    "epoch {}: {detail}; packet {}",
                    state.effective_epoch.unwrap_or_default(),
                    path.display()
                )));
            },
            Step::Registered => unreachable!("handled before the loop"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::node_bond::{fixture, sign_packet};
    use super::*;
    use wrfcoin_shared_protocol::{
        accounts::passkey::test_support::SoftwareAuthenticator,
        consensus::node_share_contracts::node_bond_payout_passkey_challenge,
        crypto::signer::EnvKeySigner,
        governance::schedule_extension::install_bond_admission_pin_for_tests,
    };

    // The pin every test in this binary shares (first install wins), the same
    // one node_bond.rs and node_bond_admit.rs use. Synthetic keys only.
    fn pin() {
        install_bond_admission_pin_for_tests(
            &hex::encode([7u8; 32]),
            vec![hex::encode(fixture::pk(60))],
        );
    }
    fn identity() -> ChainIdentity {
        ChainIdentity::new("777", BOND_START_NETWORK)
    }
    const BONDS: &str = r"^/node/bonds/[0-9a-f]{64}$";

    struct Fx {
        root: PathBuf,
        server: mockito::ServerGuard,
        args: StartArgs,
        wallet: SoftwareAuthenticator,
        owner: Address,
        server_key: [u8; 32],
    }

    fn secret(path: PathBuf, bytes: &[u8]) -> PathBuf {
        std::fs::write(&path, bytes).unwrap(); // secret-leak-ok: synthetic test fixture only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        path
    }

    fn fx(tag: &str) -> Fx {
        pin();
        let root = std::env::temp_dir().join(format!("bond-start-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let seed = |n: u8, name: &str| secret(root.join(name), hex::encode([n; 32]).as_bytes());
        let peer = libp2p_identity::Keypair::ed25519_from_bytes([2u8; 32]).unwrap();
        let peer_id = peer.public().to_peer_id();
        let wallet = SoftwareAuthenticator::new(&[0x3c; 32], "wrfcoin.com");
        let server = mockito::Server::new();
        let args = StartArgs {
            state_dir: root.join("state"),
            owner_key_file: seed(21, "owner.key"),
            server_key_file: seed(22, "server.key"),
            consumer_key_file: seed(23, "consumer.key"),
            payout_address: wallet.account().to_string(),
            payout_approval_file: root.join("payout-approved.json"),
            multiaddr: format!("/ip4/203.0.113.7/tcp/9000/p2p/{peer_id}"),
            p2p_key_file: Some(secret(
                root.join("peer_identity.key"),
                &peer.to_protobuf_encoding().unwrap(),
            )),
            admission_url: format!("{}/api/bond-admission", server.url()),
            approval_page_url: DEFAULT_APPROVAL_PAGE.into(),
            api_url: server.url(),
            operator_token_file: secret(root.join("token"), b"synthetic-bond-start-bearer"),
            chain_id: "777".into(),
            fee: 1_000_000,
        };
        let key = |n: u8| OperatorKey::from_seed(&[n; 32]).unwrap();
        Fx {
            owner: key(21).address(),
            server_key: key(22).public_key().as_bytes().try_into().unwrap(),
            root,
            server,
            args,
            wallet,
        }
    }

    fn envelope(data: Value) -> String {
        json!({"ok":true,"data":data,"error":null,"meta":{"request_id":"r"}}).to_string()
    }
    fn unknown(height: u64, epoch: u64) -> Value {
        json!({"height":height.to_string(),"activation_height":"2","epoch":epoch.to_string(),
            "status":"UNKNOWN","registration":null})
    }
    fn registered(f: &Fx) -> Value {
        json!({"height":"12400","activation_height":"2","epoch":"25","status":"REGISTERED",
            "registration":{"binding":{"owner_account":f.owner,"server_key":f.server_key}}})
    }
    const FUNDED: &str = "50000000000";

    /// Every read a run makes of the local node.
    fn node(f: &mut Fx, height: u64, bond: Value, nonce: u64, balance: &str) -> Vec<mockito::Mock> {
        let owner = f.owner;
        vec![
            f.server
                .mock("GET", "/network/info")
                .with_body(
                    json!({"chain_id":"777","network_id":"testnet",
                    "genesis_hash":format!("0x{}", hex::encode([7u8; 32]))})
                    .to_string(),
                )
                .create(),
            f.server
                .mock("GET", "/blockchain")
                .with_body(envelope(json!({"height":height})))
                .create(),
            f.server
                .mock("GET", format!("/addresses/{owner}").as_str())
                .with_body(envelope(
                    json!({"address":owner.to_string(),"balance":balance,
                    "spendable_balance":balance,"nonce":nonce.to_string()}),
                ))
                .create(),
            f.server
                .mock("GET", mockito::Matcher::Regex(BONDS.into()))
                .with_body(bond.to_string())
                .create(),
        ]
    }

    /// The admission service: co-signs what it is sent with the pinned key.
    fn admission(f: &mut Fx, hits: usize) -> mockito::Mock {
        f.server
            .mock("POST", "/api/bond-admission")
            // A public request: the node's bearer never goes to the admission service.
            .match_header("authorization", mockito::Matcher::Missing)
            .with_body_from_request(|request| {
                let body: Value = serde_json::from_slice(request.body().unwrap()).unwrap();
                let mut tx: Transaction =
                    serde_json::from_value(body["transaction"].clone()).unwrap();
                let key = EnvKeySigner::from_private_key(fixture::key(60), "synthetic-admission");
                sign_packet(&mut tx, BondSignerRole::Admission, &identity(), &key).unwrap();
                let a = NodeBondLifecycle::parse(&tx).unwrap().signed.authorization;
                json!({"ok":true,"data":{"admitted":true,"included":false,"action":"Deposit",
                    "deploymentId":hex::encode(a.deployment_id),"effectiveEpoch":a.effective_epoch,
                    "transaction":tx}})
                .to_string()
                .into_bytes()
            })
            .expect(hits)
            .create()
    }

    /// The local node's submit route: the packet must verify as the chain will check it.
    fn submit(f: &mut Fx, hits: usize) -> mockito::Mock {
        f.server
            .mock("POST", "/node/bonds/submit")
            .with_status(202)
            .with_body_from_request(|request| {
                let tx: Transaction = serde_json::from_slice(request.body().unwrap()).unwrap();
                NodeBondLifecycle::parse(&tx)
                    .unwrap()
                    .verify_with_gates(
                        &identity(),
                        &[7; 32],
                        &[],
                        &[hex::encode(fixture::pk(60))],
                        true,
                    )
                    .unwrap();
                json!({"tx_id":tx.id.unwrap().to_hex(),"admitted":true,"included":false})
                    .to_string()
                    .into_bytes()
            })
            .expect(hits)
            .create()
    }

    fn run(args: &StartArgs) -> Result<Value> {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(handle(args))
    }
    fn waits(result: Result<Value>, says: &str) -> String {
        let err = result.unwrap_err();
        assert_eq!(crate::commands::emit::classify_exit_code(&err), 2, "{err}");
        let text = err.to_string();
        assert!(
            text.starts_with("RETRY_LATER") && text.contains(says),
            "{text}"
        );
        text
    }
    fn state(f: &Fx) -> State {
        load_state(&f.args.state_dir.join(STATE_FILE))
            .unwrap()
            .unwrap()
    }
    fn attempt(f: &Fx, n: u32) -> PathBuf {
        f.args.state_dir.join(format!("attempt-{n}"))
    }

    /// Fresh state to the payout wait: one admission POST, nothing submitted.
    fn to_admitted(f: &mut Fx) {
        let _node = node(f, 12_321, unknown(12_321, 24), 0, FUNDED);
        let posted = admission(f, 1);
        let submitted = submit(f, 0);
        waits(run(&f.args), "waiting for payout approval");
        posted.assert();
        submitted.assert();
    }

    fn approve(f: &Fx, wallet: &SoftwareAuthenticator) {
        let tx = read_transaction(&attempt(f, state(f).attempt).join("admitted.json")).unwrap();
        let a = NodeBondLifecycle::parse(&tx).unwrap().signed.authorization;
        let challenge = node_bond_payout_passkey_challenge(&a, &identity());
        let approval = wallet.authorization_for(&wallet.assert(&challenge));
        std::fs::write(
            &f.args.payout_approval_file,
            serde_json::to_vec(&json!({"payout_passkey": approval})).unwrap(),
        )
        .unwrap();
    }

    /// Admitted, approved and submitted once: waiting for inclusion.
    fn to_submitted(f: &mut Fx) {
        to_admitted(f);
        approve(f, &f.wallet);
        let _node = node(f, 12_330, unknown(12_330, 24), 0, FUNDED);
        let submitted = submit(f, 1);
        waits(run(&f.args), "waiting for inclusion");
        submitted.assert();
    }

    #[test]
    fn fresh_start_waits_for_the_grant_then_runs_to_the_admission_post() {
        let mut f = fx("fresh");
        // Too little on the owner: print the grant command, write nothing else.
        {
            let _node = node(&mut f, 12_321, unknown(12_321, 24), 0, "1");
            let posted = admission(&mut f, 0);
            waits(run(&f.args), "waiting for owner funds");
            posted.assert();
        }
        assert!(state(&f).grant_instructed);
        assert_eq!(state(&f).step, Step::Start);
        assert!(!attempt(&f, 1).join("plan.json").exists());
        let deployment = state(&f).deployment_id;

        // Funded: plan, signatures, peer proof, one admission POST, then the payout wait.
        to_admitted(&mut f);
        let s = state(&f);
        assert_eq!(s.step, Step::Admitted);
        assert_eq!(s.deployment_id, deployment);
        assert_eq!(s.effective_epoch, Some(25));
        assert_eq!(
            s.service_alias,
            format!("node-{}", &hex::encode(f.server_key)[..8])
        );
        for name in ["plan.json", "signed.json", "request.json", "admitted.json"] {
            assert!(attempt(&f, 1).join(name).exists(), "{name}");
        }
        let request: Value =
            serde_json::from_slice(&std::fs::read(attempt(&f, 1).join("request.json")).unwrap())
                .unwrap();
        assert_eq!(request["multiaddr"], f.args.multiaddr);
        let sent = NodeBondLifecycle::parse(
            &serde_json::from_value(request["transaction"].clone()).unwrap(),
        )
        .unwrap();
        assert_eq!(sent.signed.authorization.service_alias, s.service_alias);
        assert_eq!(sent.governance_key, Some(fixture::pk(60)));
        assert!(sent.signed.owner_signature.is_empty());
        // No seed ever lands in the state.
        let text = std::fs::read_to_string(f.args.state_dir.join(STATE_FILE)).unwrap();
        for n in [21u8, 22, 23] {
            assert!(!text.contains(&hex::encode([n; 32])));
        }
        std::fs::remove_dir_all(&f.root).unwrap();
    }

    #[test]
    fn waiting_for_the_passkey_sends_and_signs_nothing_again() {
        let mut f = fx("waiting");
        to_admitted(&mut f);
        let dir = attempt(&f, 1);
        let before: Vec<Vec<u8>> = ["plan.json", "signed.json", "request.json", "admitted.json"]
            .iter()
            .map(|n| std::fs::read(dir.join(n)).unwrap())
            .collect();
        let _node = node(&mut f, 12_330, unknown(12_330, 24), 0, FUNDED);
        let posted = admission(&mut f, 0);
        let submitted = submit(&mut f, 0);
        waits(run(&f.args), "waiting for payout approval");
        // An approval from another wallet is not this packet's: still waiting.
        approve(&f, &SoftwareAuthenticator::new(&[0x4d; 32], "wrfcoin.com"));
        waits(run(&f.args), "does not approve this packet");
        posted.assert();
        submitted.assert();
        let after: Vec<Vec<u8>> = ["plan.json", "signed.json", "request.json", "admitted.json"]
            .iter()
            .map(|n| std::fs::read(dir.join(n)).unwrap())
            .collect();
        assert_eq!(before, after);
        assert!(!dir.join("final-1.json").exists());
        std::fs::remove_dir_all(&f.root).unwrap();
    }

    #[test]
    fn approval_is_attached_owner_signed_and_submitted_once() {
        let mut f = fx("attach");
        to_submitted(&mut f);
        let s = state(&f);
        assert_eq!(s.step, Step::Submitted);
        let packet = attempt(&f, 1).join("final-1.json");
        assert_eq!(s.files.submitted.as_deref(), Some(packet.as_path()));
        let p = NodeBondLifecycle::parse(&read_transaction(&packet).unwrap()).unwrap();
        assert!(p.payout_passkey.is_some() && !p.signed.owner_signature.is_empty());
        std::fs::remove_dir_all(&f.root).unwrap();
    }

    #[test]
    fn later_runs_repost_the_same_bytes_and_only_rewrap_a_stale_wrapper() {
        let mut f = fx("repost");
        to_submitted(&mut f);
        let packet = attempt(&f, 1).join("final-1.json");
        let first = std::fs::read(&packet).unwrap();
        let _node = node(&mut f, 12_340, unknown(12_340, 24), 0, FUNDED);
        {
            let same: Value = serde_json::from_slice(&first).unwrap();
            let again = f
                .server
                .mock("POST", "/node/bonds/submit")
                .match_body(mockito::Matcher::Json(same))
                .with_status(202)
                .with_body(json!({"admitted":true,"included":false}).to_string())
                .expect(1)
                .create();
            waits(run(&f.args), "waiting for inclusion");
            again.assert();
        }
        assert_eq!(std::fs::read(&packet).unwrap(), first);

        // Past the node's one-hour window, the same signed lifecycle goes out
        // in a fresh wrapper; the signed `data` is byte-identical.
        let mut old: Transaction = serde_json::from_slice(&first).unwrap();
        old.timestamp = chrono::Utc::now() - chrono::Duration::hours(2);
        old.set_id();
        std::fs::write(&packet, serde_json::to_vec(&old).unwrap()).unwrap();
        let submitted = submit(&mut f, 1);
        waits(run(&f.args), "final-2.json");
        submitted.assert();
        let fresh = read_transaction(&attempt(&f, 1).join("final-2.json")).unwrap();
        assert_eq!(fresh.data, old.data);
        assert!(chrono::Utc::now().timestamp() - fresh.timestamp.timestamp() < 60);
        assert_eq!(
            state(&f).files.submitted,
            Some(attempt(&f, 1).join("final-2.json"))
        );
        std::fs::remove_dir_all(&f.root).unwrap();
    }

    #[test]
    fn a_registered_bond_exits_zero_and_posts_nothing() {
        let mut f = fx("done");
        to_submitted(&mut f);
        let bond = registered(&f);
        let _node = node(&mut f, 12_400, bond, 1, FUNDED);
        let submitted = submit(&mut f, 0);
        let posted = admission(&mut f, 0);
        for _ in 0..2 {
            let done = run(&f.args).unwrap();
            assert_eq!(done["status"], "REGISTERED");
            assert_eq!(done["deployment_id"], state(&f).deployment_id);
        }
        submitted.assert();
        posted.assert();
        assert_eq!(state(&f).step, Step::Registered);
        std::fs::remove_dir_all(&f.root).unwrap();
    }

    #[test]
    fn a_passed_epoch_says_so_and_the_next_run_renews_from_a_new_plan() {
        let mut f = fx("renew");
        to_submitted(&mut f);
        let deployment = state(&f).deployment_id;
        let first = std::fs::read(attempt(&f, 1).join("final-1.json")).unwrap();
        // Epoch 25 has begun and the bond is not registered.
        {
            let _node = node(&mut f, 12_900, unknown(12_900, 25), 0, FUNDED);
            let submitted = submit(&mut f, 0);
            waits(run(&f.args), "was not included (effective epoch 25 began)");
            submitted.assert();
        }
        let s = state(&f);
        assert_eq!(
            (s.attempt, s.step, s.effective_epoch),
            (2, Step::Start, None)
        );
        // Rerun: same deployment, a new plan for epoch 26, a new admission POST.
        let _node = node(&mut f, 12_900, unknown(12_900, 25), 0, FUNDED);
        let posted = admission(&mut f, 1);
        waits(run(&f.args), "waiting for payout approval");
        posted.assert();
        let s = state(&f);
        assert_eq!(
            (s.attempt, s.step, s.effective_epoch),
            (2, Step::Admitted, Some(26))
        );
        assert_eq!(s.deployment_id, deployment);
        assert!(attempt(&f, 2).join("admitted.json").exists());
        // The first attempt's packets are kept as they were.
        assert_eq!(
            std::fs::read(attempt(&f, 1).join("final-1.json")).unwrap(),
            first
        );
        std::fs::remove_dir_all(&f.root).unwrap();
    }

    #[test]
    fn admission_waits_resend_the_same_request_and_final_refusals_stop() {
        let mut f = fx("refusal");
        let _node = node(&mut f, 12_321, unknown(12_321, 24), 0, FUNDED);
        {
            let busy = f
                .server
                .mock("POST", "/api/bond-admission")
                .with_status(409)
                .with_header("retry-after", "3600")
                .with_body(json!({"error":{"code":"NODE_TOO_NEW","message":"The node must stay connected for 24 hours first"}}).to_string())
                .expect(1)
                .create();
            waits(run(&f.args), "NODE_TOO_NEW");
            busy.assert();
        }
        assert_eq!(state(&f).step, Step::Requested);
        let request: Value =
            serde_json::from_slice(&std::fs::read(attempt(&f, 1).join("request.json")).unwrap())
                .unwrap();
        {
            let again = f
                .server
                .mock("POST", "/api/bond-admission")
                .match_body(mockito::Matcher::Json(request))
                .with_status(409)
                .with_body(json!({"error":{"code":"SERVER_KEY_ALREADY_ADMITTED"}}).to_string())
                .expect(1)
                .create();
            let err = run(&f.args).unwrap_err();
            assert_eq!(crate::commands::emit::classify_exit_code(&err), 1, "{err}");
            assert!(
                err.to_string().contains("SERVER_KEY_ALREADY_ADMITTED"),
                "{err}"
            );
            again.assert();
        }
        // Another bond's arguments cannot reuse this state folder.
        let mut other = StartArgs {
            payout_address: fixture::address(9).to_string(),
            ..f.args
        };
        let err = run(&other).unwrap_err().to_string();
        assert!(err.contains("belongs to another bond"), "{err}");
        // Plain http to a non-loopback host is refused before anything runs.
        other.admission_url = "http://203.0.113.9/api/bond-admission".into();
        assert!(
            run(&other)
                .unwrap_err()
                .to_string()
                .contains("--admission-url")
        );
        std::fs::remove_dir_all(&f.root).unwrap();
    }
}
