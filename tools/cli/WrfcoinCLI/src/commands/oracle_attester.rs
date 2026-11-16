//! Attester tools: `wrfcoin oracle statement` and `wrfcoin oracle sweep`.
//!
//! `statement` is read-only. It reads public block views from a node and
//! recomputes, for one oracle id, what the chain recorded: attestations
//! included, decisions paid (from `attester_settlements` metadata) and the
//! amount paid. The counting lives in
//! [`wrfcoin_shared_protocol::oracle::statement`], shared with the node's own
//! attester route, so any two readers of the same blocks agree.
//!
//! `sweep` moves the oracle address's balance to a wallet. Attester pay lands
//! on the oracle key's own address, and the wallet keystore cannot sign for
//! it, so this command signs one ordinary transparent transfer with the oracle
//! key file. It is testnet only, requires `--to`, shows the amount and asks
//! before sending (`--yes` for scripts).

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail, ensure};
use clap::Args;
use serde_json::{Value, json};
use wrfcoin_cli::{ApiClient, ApiConfig};
use wrfcoin_shared_protocol::blockchain::transaction::transparent_transfer_authorization_message_with_domain;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::domain_signing::DEFAULT_CHAIN_ID;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::oracle::statement::{
    EpochWindows, IncludedVote, OracleStatement, StatementBlock, compute_statement, format_units,
    parse_settlements,
};

/// The local node's launch API.
pub const DEFAULT_NODE_URL: &str = "http://127.0.0.1:8556";
/// The only network a sweep is allowed on.
const TESTNET_NETWORK_ID: &str = "testnet";
/// Largest range one statement scans, in blocks (16 epochs of 512).
pub const MAX_STATEMENT_BLOCKS: u64 = 8_192;
/// Default pause between block reads: keeps an unauthenticated reader under
/// a node's 100-per-minute per-address read limit.
const DEFAULT_DELAY_MS: u64 = 700;

#[derive(Args, Debug, Clone)]
pub struct StatementArgs {
    /// Oracle id (the oracle's ed25519 public key, 64 hex characters).
    #[clap(
        long,
        required_unless_present = "key_file",
        conflicts_with = "key_file"
    )]
    pub oracle: Option<String>,

    /// Derive the oracle id from this oracle key file instead of `--oracle`.
    /// Only the public half is used.
    #[clap(long)]
    pub key_file: Option<PathBuf>,

    /// First height to scan (inclusive).
    #[clap(long, conflicts_with = "epoch")]
    pub from_height: Option<u64>,

    /// Last height to scan (inclusive). Defaults to the node's tip.
    #[clap(long, conflicts_with = "epoch")]
    pub to_height: Option<u64>,

    /// Scan one epoch: heights `epoch-start-height + E * epoch-blocks` onward
    /// for `epoch-blocks` blocks, clipped to the node's tip.
    #[clap(long)]
    pub epoch: Option<u64>,

    /// Epoch length in blocks, for `--epoch` and the per-epoch table.
    #[clap(long, default_value_t = 512)]
    pub epoch_blocks: u64,

    /// Height epoch 0 starts at.
    #[clap(long, default_value_t = 0)]
    pub epoch_start_height: u64,

    /// Node launch API base URL. Block reads are public.
    #[clap(long, default_value = DEFAULT_NODE_URL)]
    pub node_url: String,

    /// Pause between block reads, in milliseconds.
    #[clap(long, default_value_t = DEFAULT_DELAY_MS)]
    pub delay_ms: u64,

    /// Also list every payment (height, decision, amount).
    #[clap(long)]
    pub payments: bool,
}

/// No `Debug`/`Clone`: it names the operator token file.
#[derive(Args)]
pub struct SweepArgs {
    /// Oracle key file (64 hex characters, owner-only). Its address is the
    /// one attester pay lands on.
    #[clap(long)]
    pub key_file: PathBuf,

    /// Wallet address to move the balance to. Required: there is no default.
    #[clap(long)]
    pub to: Option<String>,

    /// Node launch API base URL.
    #[clap(long, default_value = DEFAULT_NODE_URL)]
    pub node_url: String,

    /// File holding the node's operator token (account reads and submits
    /// need it). The token is never printed.
    #[clap(long)]
    pub operator_token_file: PathBuf,

    /// Refuse unless the node reports exactly this chain id.
    #[clap(long)]
    pub chain_id: Option<String>,

    /// Show what would be sent and stop.
    #[clap(long)]
    pub dry_run: bool,

    /// Send without asking (for scripts).
    #[clap(long)]
    pub yes: bool,
}

// ---------------------------------------------------------------------------
// statement
// ---------------------------------------------------------------------------

/// Turn a node's public block view (`GET /chain/block/{height}`) into the
/// statement facts. Unknown transaction types are ignored; a malformed
/// settlements value is an error.
pub fn statement_block_from_view(view: &Value) -> Result<StatementBlock> {
    let height = view
        .get("height")
        .and_then(|h| h.as_u64().or_else(|| h.as_str()?.parse().ok()))
        .ok_or_else(|| anyhow!("block view has no height"))?;
    let votes = view
        .get("transactions")
        .and_then(Value::as_array)
        .map(|txs| {
            txs.iter()
                .filter(|tx| tx.get("type").and_then(Value::as_str) == Some("OracleAttestation"))
                .filter_map(|tx| {
                    let payload = tx.get("payload")?;
                    Some(IncludedVote {
                        oracle_id: payload.get("oracle_id")?.as_str()?.to_string(),
                        commit_id: payload.get("commit_id")?.as_str()?.to_string(),
                        approved: payload.get("approved")?.as_bool()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let raw = view
        .get("metadata")
        .and_then(|m| m.get("attester_settlements"))
        .and_then(Value::as_str);
    let settlements = parse_settlements(raw).map_err(|err| anyhow!("block {height}: {err}"))?;
    Ok(StatementBlock {
        height,
        votes,
        settlements,
    })
}

fn normalize_oracle_id(raw: &str) -> Result<String> {
    let id = raw.trim().trim_start_matches("0x").to_ascii_lowercase();
    ensure!(
        id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "oracle id must be 64 hex characters (the oracle's public key)"
    );
    Ok(id)
}

/// Resolve the scan range against the node's tip.
fn statement_range(args: &StatementArgs, tip: u64) -> Result<(u64, u64)> {
    let (from, to) = if let Some(epoch) = args.epoch {
        let windows = EpochWindows {
            epoch_blocks: args.epoch_blocks,
            start_height: args.epoch_start_height,
        };
        ensure!(args.epoch_blocks > 0, "--epoch-blocks must be at least 1");
        let (from, to) = windows
            .range(epoch)
            .ok_or_else(|| anyhow!("epoch {epoch} is out of range"))?;
        (from, to.min(tip))
    } else {
        let to = args.to_height.unwrap_or(tip).min(tip);
        let from = args
            .from_height
            .unwrap_or_else(|| to.saturating_sub(args.epoch_blocks.max(1) - 1));
        (from, to)
    };
    ensure!(
        from <= to,
        "nothing to scan: from height {from} is after to height {to} (node tip {tip})"
    );
    let span = to - from + 1;
    ensure!(
        span <= MAX_STATEMENT_BLOCKS,
        "range of {span} blocks is over the {MAX_STATEMENT_BLOCKS}-block limit; split it"
    );
    Ok((from, to))
}

fn number(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

async fn node_tip(client: &ApiClient) -> Result<u64> {
    let info: Value = client.get("/network/info", None).await?;
    info.get("chain_height")
        .and_then(number)
        .ok_or_else(|| anyhow!("node did not report chain_height"))
}

pub async fn statement(args: &StatementArgs) -> Result<Value> {
    let oracle_id = match (&args.oracle, &args.key_file) {
        (Some(id), _) => normalize_oracle_id(id)?,
        (None, Some(path)) => super::oracle::oracle_public_key_from_file(path)?.to_hex(),
        (None, None) => bail!("pass --oracle <id> or --key-file <oracle key>"),
    };
    let client = ApiClient::new(ApiConfig {
        base_url: args.node_url.trim_end_matches('/').to_string(),
        ..ApiConfig::default()
    })?;
    let tip = node_tip(&client).await?;
    let (from, to) = statement_range(args, tip)?;

    let mut blocks = Vec::with_capacity((to - from + 1) as usize);
    for height in from..=to {
        if height > from && args.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(args.delay_ms)).await;
        }
        let view: Value = client
            .get(&format!("/chain/block/{height}"), None)
            .await
            .with_context(|| format!("reading block {height}"))?;
        blocks.push(statement_block_from_view(&view)?);
    }
    Ok(render_statement(
        &compute_statement(
            &oracle_id,
            from,
            to,
            &blocks,
            Some(EpochWindows {
                epoch_blocks: args.epoch_blocks,
                start_height: args.epoch_start_height,
            }),
        ),
        args.payments,
    ))
}

/// JSON plus a plain table in `summary`.
pub fn render_statement(s: &OracleStatement, with_payments: bool) -> Value {
    fn row(label: &str, value: impl std::fmt::Display) -> String {
        format!("  {label:<24}{value}")
    }
    let short = &s.oracle_id[..s.oracle_id.len().min(8)];
    let mut table = vec![
        format!(
            "Attester statement for {short}… (heights {}..={}, {} blocks)",
            s.from_height, s.to_height, s.blocks_scanned
        ),
        row("Attestations included", s.attestations_included),
        row(
            "  approve / reject",
            format!("{} / {}", s.approve_votes, s.reject_votes),
        ),
        row("Paid votes (on time)", s.paid_votes),
        row("Approve votes not paid", s.unpaid_approve_votes),
        row("Decisions settled", s.decisions_settled),
        row("Paid", format!("{} WRF", format_units(s.paid_units))),
        row(
            "Last attested height",
            s.last_attested_height
                .map_or_else(|| "none in range".to_string(), |h| h.to_string()),
        ),
    ];
    if let Some(epochs) = &s.epochs {
        table.push(row(
            "Active epochs",
            format!("{} of {}", s.active_epochs().unwrap_or(0), epochs.len()),
        ));
        table.push("  epoch  heights            included  paid  WRF".into());
        for e in epochs {
            table.push(format!(
                "  {:<6} {:>8}..={:<8} {:>8}  {:>4}  {}",
                e.epoch,
                e.from_height,
                e.to_height,
                e.attestations_included,
                e.paid_votes,
                format_units(e.paid_units)
            ));
        }
    }
    if with_payments {
        for p in &s.payments {
            table.push(format!(
                "  paid at {} for decision {}: {} WRF",
                p.height,
                p.decision_id,
                format_units(p.amount_units)
            ));
        }
    }
    let mut statement = serde_json::to_value(s).unwrap_or(Value::Null);
    if let Some(obj) = statement.as_object_mut() {
        // u128 amounts as strings so no JSON reader loses precision.
        obj.insert("paid_units".into(), json!(s.paid_units.to_string()));
        obj.insert("paid_wrf".into(), json!(format_units(s.paid_units)));
        obj.insert("active_epochs".into(), json!(s.active_epochs()));
        if !with_payments {
            obj.remove("payments");
        }
    }
    json!({
        "summary": table.join("\n"),
        "statement": statement,
    })
}

// ---------------------------------------------------------------------------
// sweep
// ---------------------------------------------------------------------------

/// What a sweep would send, before anything is signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepPlan {
    pub chain_id: String,
    pub network_id: String,
    pub from: String,
    pub to: String,
    pub balance_units: u64,
    pub fee_units: u64,
    pub amount_units: u64,
    pub nonce: u64,
}

impl SweepPlan {
    fn to_json(&self) -> Value {
        json!({
            "chain_id": self.chain_id,
            "network_id": self.network_id,
            "from": self.from,
            "to": self.to,
            "balance": format_units(self.balance_units as u128),
            "network_fee": format_units(self.fee_units as u128),
            "amount": format_units(self.amount_units as u128),
            "amount_units": self.amount_units.to_string(),
            "nonce": self.nonce.to_string(),
        })
    }

    fn describe(&self) -> String {
        format!(
            "Move {} WRF from the attester address {} to {} (balance {} WRF, network fee {} WRF, chain {}/{})",
            format_units(self.amount_units as u128),
            self.from,
            self.to,
            format_units(self.balance_units as u128),
            format_units(self.fee_units as u128),
            self.chain_id,
            self.network_id,
        )
    }
}

/// Load the oracle key (owner-only file, 64 hex characters).
fn oracle_private_key(path: &Path) -> Result<PrivateKey> {
    super::oracle::oracle_private_key_from_file(path)
}

/// Parse a destination address and return its canonical form.
fn destination(raw: Option<&str>) -> Result<Address> {
    let raw = raw.ok_or_else(|| {
        anyhow!("--to <wallet address> is required: a sweep never picks a destination for you")
    })?;
    raw.trim()
        .parse::<Address>()
        .map_err(|_| anyhow!("--to is not a valid address"))
}

/// Read the node, check it is a testnet, and work out what would be sent.
pub async fn plan_sweep(
    api: &ApiClient,
    from: &Address,
    to: &Address,
    pinned_chain_id: Option<&str>,
) -> Result<SweepPlan> {
    ensure!(
        from != to,
        "--to is the attester address itself; pick your wallet address"
    );
    let info = api.receipt_request("GET", "/network/info", None).await?;
    let chain_id = info
        .get("chain_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let network_id = info
        .get("network_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    ensure!(
        network_id == TESTNET_NETWORK_ID,
        "sweep is testnet only; this node reports network {network_id:?}"
    );
    ensure!(
        !chain_id.is_empty() && chain_id != DEFAULT_CHAIN_ID,
        "sweep is testnet only; this node reports chain {chain_id:?}"
    );
    if let Some(pinned) = pinned_chain_id {
        ensure!(
            pinned == chain_id,
            "node reports chain {chain_id}, not the --chain-id {pinned} you asked for"
        );
    }
    let fee_units = wrfcoin_cli::api::parse_transfer_fee(&info).unwrap_or(0);

    let account = api
        .receipt_request("GET", &format!("/addresses/{from}"), None)
        .await?;
    let balance_units = account
        .get("spendable_balance")
        .or_else(|| account.get("balance"))
        .and_then(number)
        .ok_or_else(|| anyhow!("node did not report the attester address balance"))?;
    let nonce = account
        .get("nonce")
        .and_then(number)
        .ok_or_else(|| anyhow!("node did not report the attester address nonce"))?;
    let amount_units = balance_units.saturating_sub(fee_units);
    ensure!(
        amount_units > 0,
        "nothing to move: the attester address holds {} WRF and the network fee is {} WRF",
        format_units(balance_units as u128),
        format_units(fee_units as u128)
    );
    Ok(SweepPlan {
        chain_id,
        network_id,
        from: from.to_string(),
        to: to.to_string(),
        balance_units,
        fee_units,
        amount_units,
        nonce,
    })
}

/// Sign the plan's transfer with the oracle key. The chain identity comes from
/// the plan (read from the node and checked), never from process environment.
pub fn sign_sweep(plan: &SweepPlan, key: &PrivateKey) -> Value {
    let message = transparent_transfer_authorization_message_with_domain(
        &plan.chain_id,
        &plan.network_id,
        &plan.from,
        &plan.to,
        plan.amount_units,
        plan.nonce,
    );
    let signature = key.sign(&message);
    let mut body = json!({
        "from": plan.from,
        "to": plan.to,
        "amount": plan.amount_units,
        "nonce": plan.nonce.to_string(),
        "transparent_opt_out": true,
        "transparent_opt_out_reason": "cli",
        "auth_pubkey": PublicKey::from_private_key(key).to_hex(),
        "auth_signature": signature.to_hex(),
    });
    if plan.fee_units > 0 {
        body["fee"] = json!(plan.fee_units);
    }
    body
}

/// `confirm` is asked once with the plan's description unless `--yes` or
/// `--dry-run` is given. It returns `Ok(false)` to cancel.
pub async fn sweep(args: &SweepArgs, confirm: impl FnOnce(&str) -> Result<bool>) -> Result<Value> {
    // Cheap input checks first: no key read and no network before them.
    let to = destination(args.to.as_deref())?;
    let key = oracle_private_key(&args.key_file)?;
    let public = PublicKey::from_private_key(&key);
    let from = Address::from_public_key(&public);
    let api = super::node_deposit::operator_api(&args.node_url, &args.operator_token_file)?;
    let plan = plan_sweep(&api, &from, &to, args.chain_id.as_deref()).await?;

    if args.dry_run {
        return Ok(json!({
            "summary": format!("Dry run. {}", plan.describe()),
            "dry_run": true,
            "oracle_id": public.to_hex(),
            "plan": plan.to_json(),
            "submitted": false,
        }));
    }
    if !args.yes && !confirm(&plan.describe())? {
        return Ok(json!({
            "summary": "Cancelled. Nothing was sent.",
            "cancelled": true,
            "plan": plan.to_json(),
            "submitted": false,
        }));
    }
    let body = sign_sweep(&plan, &key);
    let accepted = api
        .receipt_request("POST", "/transactions/submit", Some(body))
        .await?;
    let tx_id = accepted
        .get("tx_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(json!({
        "summary": format!(
            "Submitted: {} WRF to {} (tx_id {tx_id}), not yet confirmed",
            format_units(plan.amount_units as u128),
            plan.to
        ),
        "oracle_id": public.to_hex(),
        "plan": plan.to_json(),
        "submitted": true,
        "tx_id": tx_id,
    }))
}

/// Terminal confirmation for an interactive run. A non-interactive run must
/// pass `--yes` (or `--dry-run`); it is never confirmed implicitly.
pub fn terminal_confirm(interactive: bool) -> impl FnOnce(&str) -> Result<bool> {
    move |description: &str| {
        if !interactive {
            bail!("refusing to send without confirmation: pass --yes, or --dry-run to preview");
        }
        println!("{description}");
        Ok(dialoguer::Confirm::new()
            .with_prompt("Send this transfer?")
            .default(false)
            .interact()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::blockchain::transaction::TransparentTransferPayload;

    const SEED: [u8; 32] = [7u8; 32];
    const WALLET: &str = "0x2222222222222222222222222222222222222222";

    fn oracle() -> (PrivateKey, String, Address) {
        let key = PrivateKey::from_bytes(&SEED).unwrap();
        let public = PublicKey::from_private_key(&key);
        let address = Address::from_public_key(&public);
        (key, public.to_hex(), address)
    }

    fn secret(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        path
    }

    struct Fixture {
        root: PathBuf,
        node: mockito::ServerGuard,
    }

    fn fixture(tag: &str) -> Fixture {
        let root =
            std::env::temp_dir().join(format!("oracle-sweep-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        secret(&root, "oracle.key", hex::encode(SEED).as_bytes()); // secret-leak-ok: synthetic test seed.
        secret(&root, "operator.token", b"test-operator-token"); // secret-leak-ok: synthetic test token.
        Fixture {
            root,
            node: mockito::Server::new(),
        }
    }

    fn args(f: &Fixture, to: Option<&str>) -> SweepArgs {
        SweepArgs {
            key_file: f.root.join("oracle.key"),
            to: to.map(str::to_string),
            node_url: f.node.url(),
            operator_token_file: f.root.join("operator.token"),
            chain_id: None,
            dry_run: false,
            yes: false,
        }
    }

    fn mock_node(
        f: &mut Fixture,
        network: &str,
        chain: &str,
        fee: Option<u64>,
        balance: u64,
    ) -> Vec<mockito::Mock> {
        let (_, _, from) = oracle();
        let fee_json = match fee {
            Some(units) => json!({"active": true, "base_units": units.to_string()}),
            None => json!({"active": false, "base_units": "0"}),
        };
        vec![
            f.node
                .mock("GET", "/network/info")
                .match_header("authorization", "Bearer test-operator-token")
                .with_body(
                    json!({"chain_id": chain, "network_id": network, "chain_height": 28040,
                        "transfer_fee": fee_json})
                    .to_string(),
                )
                .create(),
            f.node
                .mock("GET", format!("/addresses/{from}").as_str())
                .with_body(
                    json!({"ok": true, "data": {"address": from.to_string(),
                        "balance": balance.to_string(), "spendable_balance": balance.to_string(),
                        "stake_locked": "0", "earned_balance_total": balance.to_string(),
                        "nonce": "3"}})
                    .to_string(),
                )
                .create(),
        ]
    }

    fn run(a: &SweepArgs, answer: Option<bool>) -> Result<Value> {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(sweep(a, |_| match answer {
                Some(yes) => Ok(yes),
                None => bail!("confirmation must not be asked"),
            }))
    }

    #[test]
    fn sweep_signs_a_chain_valid_transfer_of_the_balance_minus_fee() {
        let mut f = fixture("send");
        let _reads = mock_node(&mut f, "testnet", "888", Some(1_000), 85_000_000);
        let (_, oracle_hex, from) = oracle();
        let submit = f
            .node
            .mock("POST", "/transactions/submit")
            .with_body_from_request(move |request| {
                let body: Value = serde_json::from_slice(request.body().unwrap()).unwrap();
                assert_eq!(body["from"], from.to_string());
                assert_eq!(body["to"], WALLET);
                assert_eq!(body["amount"], 85_000_000 - 1_000);
                assert_eq!(body["fee"], 1_000);
                assert_eq!(body["nonce"], "3");
                // The signature verifies over the chain's own preimage for 888/testnet.
                let public = PublicKey::from_hex(body["auth_pubkey"].as_str().unwrap()).unwrap();
                let signature = wrfcoin_shared_protocol::crypto::signature::Signature::from_hex(
                    body["auth_signature"].as_str().unwrap(),
                )
                .unwrap();
                let message = transparent_transfer_authorization_message_with_domain(
                    "888",
                    "testnet",
                    &from.to_string(),
                    WALLET,
                    85_000_000 - 1_000,
                    3,
                );
                assert!(public.verify(&message, &signature));
                assert_eq!(Address::from_public_key(&public), from);
                json!({"ok": true, "data": {"accepted": true, "tx_id": "ab12"}})
                    .to_string()
                    .into_bytes()
            })
            .expect(1)
            .create();
        let out = run(&args(&f, Some(WALLET)), Some(true)).unwrap();
        submit.assert();
        assert_eq!(out["submitted"], true);
        assert_eq!(out["tx_id"], "ab12");
        assert_eq!(out["oracle_id"], oracle_hex);
        assert_eq!(out["plan"]["amount"], "0.84999000");
    }

    #[test]
    fn sweep_payload_passes_the_chain_transfer_authorization() {
        // Same bytes the node would rebuild: from/to/amount/nonce under the
        // pinned chain identity.
        let (key, _, from) = oracle();
        let plan = SweepPlan {
            chain_id: "888".into(),
            network_id: "testnet".into(),
            from: from.to_string(),
            to: WALLET.into(),
            balance_units: 10,
            fee_units: 0,
            amount_units: 10,
            nonce: 0,
        };
        let body = sign_sweep(&plan, &key);
        assert!(
            body.get("fee").is_none(),
            "no fee field when none is in force"
        );
        let payload = TransparentTransferPayload {
            from: plan.from.clone(),
            to: plan.to.clone(),
            amount: 10,
            transparent_opt_out: true,
            opt_out_audit_tag: None,
            auth_pubkey: body["auth_pubkey"].as_str().map(str::to_string),
            auth_signature: body["auth_signature"].as_str().map(str::to_string),
            authority: None,
            authority_signature: None,
            passkey_auth: None,
            sponsorship: None,
            session_key_auth: None,
        };
        let message = transparent_transfer_authorization_message_with_domain(
            "888",
            "testnet",
            &payload.from,
            &payload.to,
            10,
            0,
        );
        let public = PublicKey::from_hex(payload.auth_pubkey.as_deref().unwrap()).unwrap();
        let sig = wrfcoin_shared_protocol::crypto::signature::Signature::from_hex(
            payload.auth_signature.as_deref().unwrap(),
        )
        .unwrap();
        assert!(public.verify(&message, &sig));
    }

    #[test]
    fn sweep_refuses_without_to_before_reading_the_key_or_the_node() {
        let mut f = fixture("no-to");
        let info = f.node.mock("GET", "/network/info").expect(0).create();
        let err = run(&args(&f, None), None).unwrap_err().to_string();
        assert!(err.contains("--to"), "{err}");
        info.assert();
    }

    #[test]
    fn sweep_refuses_mainnet_and_non_testnet_nodes() {
        let mut f = fixture("mainnet");
        let _reads = mock_node(&mut f, "mainnet", "888", None, 5);
        let err = run(&args(&f, Some(WALLET)), None).unwrap_err().to_string();
        assert!(err.contains("testnet only"), "{err}");

        let mut f = fixture("default-chain");
        let _reads = mock_node(&mut f, "testnet", DEFAULT_CHAIN_ID, None, 5);
        let err = run(&args(&f, Some(WALLET)), None).unwrap_err().to_string();
        assert!(err.contains("testnet only"), "{err}");
    }

    #[test]
    fn sweep_refuses_a_pinned_chain_mismatch_and_a_self_send() {
        let mut f = fixture("pin");
        let _reads = mock_node(&mut f, "testnet", "888", None, 5);
        let mut a = args(&f, Some(WALLET));
        a.chain_id = Some("777".into());
        let err = run(&a, None).unwrap_err().to_string();
        assert!(err.contains("--chain-id 777"), "{err}");

        let (_, _, from) = oracle();
        let err = run(&args(&f, Some(&from.to_string())), None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("attester address itself"), "{err}");
    }

    #[test]
    fn sweep_refuses_when_the_fee_eats_the_balance() {
        let mut f = fixture("empty");
        let _reads = mock_node(&mut f, "testnet", "888", Some(500), 500);
        let err = run(&args(&f, Some(WALLET)), None).unwrap_err().to_string();
        assert!(err.contains("nothing to move"), "{err}");
    }

    #[test]
    fn sweep_dry_run_and_cancel_never_submit() {
        let mut f = fixture("cancel");
        let _reads = mock_node(&mut f, "testnet", "888", None, 42);
        let submit = f
            .node
            .mock("POST", "/transactions/submit")
            .expect(0)
            .create();

        let mut dry = args(&f, Some(WALLET));
        dry.dry_run = true;
        let out = run(&dry, None).unwrap();
        assert_eq!(out["dry_run"], true);
        assert_eq!(out["plan"]["amount_units"], "42");

        let out = run(&args(&f, Some(WALLET)), Some(false)).unwrap();
        assert_eq!(out["cancelled"], true);
        submit.assert();
    }

    #[test]
    fn non_interactive_without_yes_is_refused() {
        let err = terminal_confirm(false)("x").unwrap_err().to_string();
        assert!(err.contains("--yes"), "{err}");
    }

    #[test]
    fn block_view_parses_the_node_shape() {
        let view = json!({
            "height": 28025,
            "metadata": {"attester_settlements":
                "[{\"decision_id\":\"aa\",\"finalization_height\":28025,\"allocations\":[{\"attester_id\":\"c4db\",\"amount\":41666667}]}]"},
            "transactions": [
                {"sender": "0x00", "type": "Coinbase"},
                {"payload": {"approved": true, "commit_id": "aa", "oracle_id": "c4db"}, "type": "OracleAttestation"},
                {"payload": {"approved": false, "commit_id": "bb", "oracle_id": "781f"}, "type": "OracleAttestation"},
                {"payload": {"commit_id": "aa"}, "type": "ContributionReveal"}
            ]
        });
        let block = statement_block_from_view(&view).unwrap();
        assert_eq!(block.height, 28025);
        assert_eq!(block.votes.len(), 2);
        assert!(!block.votes[1].approved);
        assert_eq!(block.settlements[0].allocations[0].amount, 41_666_667);

        let bad = json!({"height": 1, "metadata": {"attester_settlements": "{"}});
        assert!(statement_block_from_view(&bad).is_err());
    }

    fn statement_args() -> StatementArgs {
        StatementArgs {
            oracle: Some("ab".repeat(32)),
            key_file: None,
            from_height: None,
            to_height: None,
            epoch: None,
            epoch_blocks: 512,
            epoch_start_height: 0,
            node_url: DEFAULT_NODE_URL.into(),
            delay_ms: 0,
            payments: false,
        }
    }

    #[test]
    fn statement_range_defaults_and_limits() {
        let a = statement_args();
        assert_eq!(statement_range(&a, 28_037).unwrap(), (27_526, 28_037));
        let mut e = statement_args();
        e.epoch = Some(54);
        assert_eq!(statement_range(&e, 28_037).unwrap(), (27_648, 28_037));
        e.epoch = Some(60);
        assert!(
            statement_range(&e, 28_037).is_err(),
            "future epoch has nothing to scan"
        );
        let mut wide = statement_args();
        wide.from_height = Some(0);
        assert!(
            statement_range(&wide, 28_037)
                .unwrap_err()
                .to_string()
                .contains("limit")
        );
    }

    #[test]
    fn oracle_id_is_checked() {
        assert!(normalize_oracle_id("c4db").is_err());
        assert_eq!(
            normalize_oracle_id(&format!("0x{}", "AB".repeat(32))).unwrap(),
            "ab".repeat(32)
        );
    }

    #[test]
    fn statement_against_a_fake_node() {
        let me = "c4dbd69bc84a4651db0a03ccc142a019c0856126ac3c920e361cba2269034725";
        let mut node = mockito::Server::new();
        let _info = node
            .mock("GET", "/network/info")
            .with_body(json!({"chain_height": 11}).to_string())
            .create();
        let _b10 = node
            .mock("GET", "/chain/block/10")
            .with_body(json!({"ok": true, "data": {"height": 10, "metadata": {
                "attester_settlements": format!("[{{\"decision_id\":\"aa\",\"finalization_height\":10,\"allocations\":[{{\"attester_id\":\"{me}\",\"amount\":41666667}}]}}]")},
                "transactions": [{"payload": {"approved": true, "commit_id": "aa", "oracle_id": me}, "type": "OracleAttestation"}]}}).to_string())
            .create();
        let _b11 = node
            .mock("GET", "/chain/block/11")
            .with_body(
                json!({"ok": true, "data": {"height": 11, "metadata": {}, "transactions": []}})
                    .to_string(),
            )
            .create();
        let mut a = statement_args();
        a.oracle = Some(me.into());
        a.from_height = Some(10);
        a.node_url = node.url();
        a.payments = true;
        let out = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(statement(&a))
            .unwrap();
        let s = &out["statement"];
        assert_eq!(s["attestations_included"], 1);
        assert_eq!(s["paid_votes"], 1);
        assert_eq!(s["paid_units"], "41666667");
        assert_eq!(s["paid_wrf"], "0.41666667");
        assert_eq!(s["last_attested_height"], 10);
        assert_eq!(s["payments"].as_array().unwrap().len(), 1);
        let table = out["summary"].as_str().unwrap();
        assert!(
            table.contains("  Paid                    0.41666667 WRF"),
            "{table}"
        );
    }
}
