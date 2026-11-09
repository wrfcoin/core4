//! `wrfcoin governance advisory`: build, submit and inspect signed, text-only
//! advisory proposals and votes (`shared-protocol/src/governance/advisory.rs`).
//!
//! `propose` and `vote` are offline, like `send-allowlist`: each reads the
//! wallet's ed25519 key file, signs the exact preimage core4 verifies (chain
//! id, network id, account, action, nonce), self-verifies it, and writes the
//! `Transaction` JSON that is the body of `POST /governance/advisory/submit`.
//! `--nonce` is the wallet's current account nonce; the CLI never guesses it
//! (`advisory eligibility` prints it). `--submitted-at` pins the txid.
//!
//! `submit`, `list`, `show` and `eligibility` talk to the node
//! (`WRFCOIN_API_URL`, token from `WRFCOIN_OPERATOR_TOKEN` / `WRFCOIN_READER_TOKEN`).
//!
//! A proposal is advisory: if it passes, the chain records "passed" and
//! nothing else happens. The text is public and permanent.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, ensure};
use clap::{Args, Subcommand};
use serde_json::{Value, json};

use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::signature::PrivateKey;
use wrfcoin_shared_protocol::governance::advisory::{
    AdvisoryAction, SignedAdvisoryAction, proposal_id, validate_summary, validate_title,
};

use wrfcoin_cli::{ApiClient, ApiConfig};

/// Arguments every signed action shares.
#[derive(Args, Debug)]
pub struct AdvisorySignArgs {
    /// The wallet's ed25519 private key file (hex). Never leaves this machine.
    #[clap(long)]
    pub key: PathBuf,
    /// Chain id the signature binds (`chain_id` in `advisory list`, e.g. 888).
    #[clap(long)]
    pub chain_id: String,
    /// Network id the signature binds (`network_id` in `advisory list`).
    #[clap(long)]
    pub network_id: String,
    /// The wallet's current account nonce (`next_nonce` in `advisory eligibility`).
    #[clap(long)]
    pub nonce: u64,
    /// Transaction timestamp (unix seconds), pinned for reproducibility.
    #[clap(long)]
    pub submitted_at: i64,
    /// Output artifact path (the submit body).
    #[clap(long)]
    pub out: PathBuf,
}

#[derive(Subcommand, Debug)]
pub enum AdvisoryCommands {
    /// Sign a text-only proposal (title + summary). Costs a 1 WRF deposit,
    /// returned unless it fails quorum. Nothing is ever executed.
    Propose {
        #[clap(flatten)]
        sign: AdvisorySignArgs,
        /// One line, up to 160 bytes.
        #[clap(long)]
        title: String,
        /// Up to 1,024 bytes; newlines allowed.
        #[clap(long, conflicts_with = "summary_file")]
        summary: Option<String>,
        /// Read the summary from a UTF-8 file instead.
        #[clap(long)]
        summary_file: Option<PathBuf>,
    },
    /// Sign a vote on an active proposal.
    Vote {
        #[clap(flatten)]
        sign: AdvisorySignArgs,
        /// The proposal id (`adv-` + 16 hex).
        #[clap(long)]
        proposal_id: String,
        /// Vote for (default) or, with --against, against.
        #[clap(long)]
        against: bool,
    },
    /// Submit an artifact written by `propose` or `vote`.
    Submit {
        #[clap(long)]
        artifact: PathBuf,
    },
    /// List proposals (newest first).
    List {
        /// active, passed, rejected, failed_quorum or all
        #[clap(long, default_value = "all")]
        status: String,
    },
    /// Show one proposal.
    Show {
        #[clap(long)]
        id: String,
    },
    /// Whether a wallet may vote on a proposal, and its current nonce.
    Eligibility {
        #[clap(long)]
        id: String,
        #[clap(long)]
        address: String,
    },
}

/// Build the signed transaction for `action`. Pure apart from the key.
pub fn build_transaction(
    key: &PrivateKey,
    chain_id: &str,
    network_id: &str,
    action: AdvisoryAction,
    nonce: u64,
    submitted_at: i64,
) -> Result<Transaction> {
    let signed = SignedAdvisoryAction::sign(chain_id, network_id, key, action, nonce);
    signed
        .verify(chain_id, network_id)
        .map_err(|e| anyhow!("refused before signing anything usable: {e}"))?;
    let mut outpoint = b"wrfcoin.advisory_governance.outpoint.v1".to_vec();
    outpoint.extend_from_slice(&signed.account.0);
    outpoint.extend_from_slice(&nonce.to_le_bytes());
    let mut tx = Transaction::new(
        vec![TransactionInput {
            // One outpoint per (account, nonce): two actions for the same
            // nonce conflict in the mempool, as only one can ever apply.
            txid: wrfcoin_shared_protocol::crypto::hash::hash_bytes(&outpoint),
            vout: 0,
            signature: None,
            public_key: Some(signed.public_key.clone()),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            address: signed.account.to_string(),
            amount: 0,
            script: "OP_TRUE".into(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::AdvisoryGovernance;
    tx.sender_address = signed.account;
    tx.nonce = nonce;
    tx.timestamp = chrono::DateTime::from_timestamp(submitted_at, 0)
        .ok_or_else(|| anyhow!("--submitted-at {submitted_at} is not a valid unix time"))?;
    tx.data = Some(serde_json::to_string(&signed)?);
    tx.set_id();
    ensure!(
        SignedAdvisoryAction::parse(&tx).as_ref() == Some(&signed),
        "built transaction does not parse back"
    );
    Ok(tx)
}

fn write_artifact(path: &Path, tx: &Transaction) -> Result<()> {
    let body = serde_json::to_vec_pretty(tx)?;
    super::station::atomic_artifact_write(path, |file| {
        use std::io::Write;
        file.write_all(&body)
    })
    .with_context(|| format!("cannot write {}", path.display()))
}

fn sign_and_write(sign: &AdvisorySignArgs, action: AdvisoryAction) -> Result<Value> {
    let key = super::station::read_key(&sign.key)?;
    let tx = build_transaction(&key, &sign.chain_id, &sign.network_id, action.clone(), sign.nonce, sign.submitted_at)?;
    write_artifact(&sign.out, &tx)?;
    let mut out = json!({
        "account": tx.sender_address.to_string(),
        "action": action,
        "nonce": sign.nonce,
        "tx_id": tx.id.map(|id| id.to_hex()),
        "artifact": sign.out.display().to_string(),
        "next": "wrfcoin governance advisory submit --artifact <artifact>",
    });
    if matches!(action, AdvisoryAction::Propose { .. }) {
        out["proposal_id"] = json!(proposal_id(&tx.sender_address, sign.nonce));
        out["warning"] = json!("This text is public and permanent. Don't include personal details.");
    }
    Ok(out)
}

fn parse_address(text: &str) -> Result<Address> {
    text.trim()
        .parse::<Address>()
        .map_err(|e| anyhow!("{text} is not a 40-hex-character address: {e:?}"))
}

pub async fn handle_advisory_command(command: &AdvisoryCommands) -> Result<Value> {
    use AdvisoryCommands as C;
    match command {
        C::Propose { sign, title, summary, summary_file } => {
            let summary = match (summary, summary_file) {
                (Some(text), None) => text.clone(),
                (None, Some(path)) => std::fs::read_to_string(path)
                    .with_context(|| format!("cannot read {}", path.display()))?,
                _ => return Err(anyhow!("give --summary or --summary-file")),
            };
            validate_title(title).map_err(|e| anyhow!("title: {e}"))?;
            validate_summary(&summary).map_err(|e| anyhow!("summary: {e}"))?;
            sign_and_write(sign, AdvisoryAction::Propose { title: title.clone(), summary })
        },
        C::Vote { sign, proposal_id, against } => sign_and_write(
            sign,
            AdvisoryAction::Vote { proposal_id: proposal_id.trim().to_string(), approve: !against },
        ),
        C::Submit { artifact } => {
            let raw = std::fs::read(artifact).with_context(|| format!("cannot read {}", artifact.display()))?;
            let tx: Transaction = serde_json::from_slice(&raw)
                .with_context(|| format!("{} is not a transaction artifact", artifact.display()))?;
            ensure!(
                SignedAdvisoryAction::parse(&tx).is_some(),
                "{} is not an advisory governance transaction",
                artifact.display()
            );
            let api = ApiClient::new(ApiConfig::default())?;
            let ack = api
                .receipt_request("POST", "/governance/advisory/submit", Some(serde_json::to_value(&tx)?))
                .await
                .map_err(|e| anyhow!("node refused the action ({e}); check the nonce with `advisory eligibility`"))?;
            Ok(json!({
                "tx_id": tx.id.map(|id| id.to_hex()),
                "node": ack,
                "note": "Submitted is not counted: check `advisory show` after a few blocks.",
            }))
        },
        C::List { status } => {
            let api = ApiClient::new(ApiConfig::default())?;
            api.receipt_request("GET", &format!("/governance/advisory/proposals?status={}", status.trim()), None).await
        },
        C::Show { id } => {
            let api = ApiClient::new(ApiConfig::default())?;
            api.receipt_request("GET", &format!("/governance/advisory/proposals/{}", id.trim()), None).await
        },
        C::Eligibility { id, address } => {
            let account = parse_address(address)?;
            let api = ApiClient::new(ApiConfig::default())?;
            api.receipt_request(
                "GET",
                &format!("/governance/advisory/proposals/{}/eligibility?address={account}", id.trim()),
                None,
            )
            .await
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn welcome_proposal_builds_reproducibly_and_verifies() {
        let key = PrivateKey::generate(&mut StdRng::seed_from_u64(11));
        let action = AdvisoryAction::Propose {
            title: "Welcome to governance".into(),
            summary: "The first advisory proposal on chain 888. It changes nothing.".into(),
        };
        let a = build_transaction(&key, "888", "testnet", action.clone(), 2, 1_800_000_000).unwrap();
        let b = build_transaction(&key, "888", "testnet", action, 2, 1_800_000_000).unwrap();
        assert_eq!(a.id, b.id, "pinned timestamp makes the txid reproducible");
        let signed = SignedAdvisoryAction::parse(&a).unwrap();
        assert!(signed.verify("888", "testnet").is_ok());
        assert!(signed.verify("777", "testnet").is_err(), "bound to the chain id");
        assert!(signed.verify("888", "devnet").is_err(), "bound to the network id");
        assert_eq!(a.tx_type, TransactionType::AdvisoryGovernance);
        assert_eq!(a.nonce, 2);
        assert!(a.validate_structure());
    }

    #[test]
    fn a_vote_builds_and_bad_text_is_refused_before_signing() {
        let key = PrivateKey::generate(&mut StdRng::seed_from_u64(12));
        let id = proposal_id(&Address([1; 20]), 0);
        let vote = AdvisoryAction::Vote { proposal_id: id, approve: false };
        assert!(build_transaction(&key, "888", "testnet", vote, 0, 1_800_000_000).is_ok());
        let long = AdvisoryAction::Propose { title: "x".repeat(161), summary: "s".into() };
        assert!(build_transaction(&key, "888", "testnet", long, 0, 1_800_000_000).is_err());
        let bad_id = AdvisoryAction::Vote { proposal_id: "LAW-001".into(), approve: true };
        assert!(build_transaction(&key, "888", "testnet", bad_id, 0, 1_800_000_000).is_err());
    }
}
