//! Build, sign and submit a `NodeFraudReport` (core4-archive#3589). Non-forking: the reporter signs the
//! existing `NodeReceiptSubmissionAuthorization` whose `batch_hash` is the fraud `evidence_hash`.
//! No seed-file signer exists here: keys come from `OperatorKey` (wallet key file) or stdin mnemonic.
use super::node_receipt::{bounded_read, durable_packet, load_key, secret_file};
use anyhow::{Result, ensure};
use clap::Subcommand;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use wrfcoin_cli::{ApiClient, ApiConfig, node_operator::OperatorKey};
use wrfcoin_shared_protocol::{
    blockchain::transaction::{Transaction, TransactionInput, TransactionOutput, TransactionType},
    common::types::Address,
    consensus::{
        node_fraud::{NodeFraudReport, evidence_hash, verify_authorization},
        node_receipt_registry::NodeReceiptSubmissionAuthorization,
        node_share_contracts::{BondedServiceClaim, NodeServiceBatch},
    },
    crypto::{chain_binding::ChainIdentity, hash::hash_bytes},
};

const CLAIM_CAP: usize = 40 * 1024;
const SUBMIT_CAP: usize = 40 * 1024;

#[derive(Subcommand)]
pub enum FraudCommands {
    /// Sign a fraud report from one (forged consumer receipt) or two (equivocation) claims and submit it.
    Report {
        /// `BondedServiceClaim` wire file, or an archive JSON with `claim_hex`. Repeat for two claims.
        #[clap(long = "claim", required = true)]
        claims: Vec<PathBuf>,
        #[clap(long)]
        sender: String,
        #[clap(long)]
        genesis_hash: String,
        #[clap(long)]
        chain_id: String,
        #[clap(long, default_value = "testnet")]
        network_id: String,
        #[clap(long)]
        api_url: String,
        /// Mode0600 wallet key file.
        #[clap(long, conflicts_with = "mnemonic_stdin")]
        key_file: Option<PathBuf>,
        /// BIP-39 phrase on stdin.
        #[clap(long, conflicts_with = "key_file")]
        mnemonic_stdin: bool,
        /// Owner-only file containing the Operator bearer, never argv.
        #[clap(long)]
        operator_token_file: Option<PathBuf>,
        #[clap(long, default_value = "1000000")]
        fee: u64,
        /// New durable signed packet, written before any network call.
        #[clap(long)]
        out: Option<PathBuf>,
        /// Print the evidence hash and sizes only; load no secrets, make no network call.
        #[clap(long)]
        dry_run: bool,
    },
}

fn hash32(text: &str) -> Result<[u8; 32]> {
    ensure!(text.len() == 64, "expected 32-byte hex identifier");
    hex::decode(text)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid identifier"))
}

fn read_claim(path: &Path) -> Result<BondedServiceClaim> {
    let bytes = bounded_read(path, CLAIM_CAP)?;
    let wire = match serde_json::from_slice::<Value>(&bytes) {
        Ok(body) => hex::decode(
            body["claim_hex"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("archive claim missing"))?,
        )?,
        Err(_) => bytes,
    };
    let claim = BondedServiceClaim::from_wire(&wire)?;
    claim.validate_wire_bounds()?;
    Ok(claim)
}

pub(crate) fn evidence(claims: Vec<BondedServiceClaim>) -> Result<NodeServiceBatch> {
    ensure!((1..=2).contains(&claims.len()), "a fraud report carries one or two claims");
    Ok(NodeServiceBatch { claims })
}

pub(crate) fn assemble(
    batch: NodeServiceBatch,
    sender: Address,
    nonce: u64,
    genesis: [u8; 32],
    identity: &ChainIdentity,
    key: &OperatorKey,
    fee: u64,
) -> Result<Transaction> {
    let public = key.public_key();
    ensure!(Address::from_public_key(&public) == sender, "selected key does not match sender");
    let mut report = NodeFraudReport {
        batch,
        authorization: NodeReceiptSubmissionAuthorization {
            genesis_hash: genesis,
            sender,
            nonce,
            batch_hash: [0; 32],
        },
        owner_key: public.as_bytes().try_into()?,
        owner_signature: Vec::new(),
    };
    report.authorization.batch_hash = evidence_hash(&report)?;
    report.owner_signature = key
        .sign_receipt(&report.authorization, identity)?
        .as_bytes()
        .to_vec();
    // The exact consensus verifier, so a packet that would be refused is never written.
    verify_authorization(&report, identity, &genesis)?;
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_bytes(b"wrfcoin.node_fraud_report.input.v1"),
            vout: 0,
            signature: None,
            public_key: Some(public.to_hex()),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            address: sender.to_string(),
            amount: 0,
            script: "OP_TRUE".into(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::NodeFraudReport;
    tx.sender_address = sender;
    tx.nonce = nonce;
    tx.fee = fee;
    tx.data = Some(hex::encode(report.to_wire()?));
    tx.set_id();
    ensure!(serde_json::to_vec(&tx)?.len() <= SUBMIT_CAP, "transaction exceeds fraud route cap");
    Ok(tx)
}

pub async fn handle(command: &FraudCommands) -> Result<Value> {
    let FraudCommands::Report {
        claims,
        sender,
        genesis_hash,
        chain_id,
        network_id,
        api_url,
        key_file,
        mnemonic_stdin,
        operator_token_file,
        fee,
        out,
        dry_run,
    } = command;
    let identity = ChainIdentity::new(chain_id, network_id);
    let genesis = hash32(genesis_hash)?;
    let sender: Address = sender.parse()?;
    let batch = evidence(claims.iter().map(|p| read_claim(p)).collect::<Result<_>>()?)?;
    let kind = if batch.claims.len() == 1 { "forged-consumer-receipt" } else { "equivocation" };
    if *dry_run {
        let probe = NodeFraudReport {
            batch,
            authorization: NodeReceiptSubmissionAuthorization { genesis_hash: genesis, sender, nonce: 0, batch_hash: [0; 32] },
            owner_key: [0; 32],
            owner_signature: Vec::new(),
        };
        return Ok(json!({"dry_run":true,"kind":kind,"evidence_hash":hex::encode(evidence_hash(&probe)?),"submitted":false}));
    }
    let out = out.as_ref().ok_or_else(|| anyhow::anyhow!("--out required for durable submission"))?;
    ensure!(!out.exists(), "output packet already exists; inspect inclusion and retry that exact packet");
    let key = load_key(key_file.as_deref(), *mnemonic_stdin)?;
    ensure!(key.address() == sender, "reporter signer mismatch");
    let url = url::Url::parse(api_url)?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "invalid public API URL"
    );
    let token = secret_file(
        operator_token_file.as_deref().ok_or_else(|| anyhow::anyhow!("--operator-token-file required"))?,
        16 * 1024,
    )?;
    let token = std::str::from_utf8(&token).map_err(|_| anyhow::anyhow!("invalid operator credential"))?.trim();
    ensure!(!token.is_empty(), "operator credential unavailable"); // secret-leak-ok: constant error, no credential contents.
    let api = ApiClient::new_without_redirects(ApiConfig {
        base_url: api_url.trim_end_matches('/').to_owned(),
        bearer_token: Some(token.to_owned()),
        ..ApiConfig::default()
    })?;
    let network: Value = api.receipt_request("GET", "/network/info", None).await?;
    ensure!(
        network["chain_id"].as_str() == Some(chain_id)
            && network["network_id"].as_str() == Some(network_id)
            && network["genesis_hash"].as_str().and_then(super::node_deposit::api_hash) == Some(genesis),
        "API chain/genesis mismatch"
    );
    let account = api.receipt_request("GET", &format!("/addresses/{sender}"), None).await?;
    let nonce = account["nonce"].as_str().ok_or_else(|| anyhow::anyhow!("sender nonce unavailable"))?.parse::<u64>()?;
    let tx = assemble(batch, sender, nonce, genesis, &identity, &key, *fee)?;
    durable_packet(out, &tx)?;
    let ack = api.receipt_request("POST", "/node/fraud/submit", Some(serde_json::to_value(&tx)?)).await;
    match ack {
        Ok(a) if a["admitted"] == true && a["tx_id"].as_str() == tx.id.map(|h| h.to_hex()).as_deref() => Ok(json!({
            "kind": kind,
            "tx_id": tx.id.map(|h| h.to_hex()),
            "admitted": true,
            "included": false,
            "responsible": a["responsible"],
            "deadline_height": a["deadline_height"],
            "packet": out.display().to_string(),
        })),
        other => anyhow::bail!(
            "UNCERTAIN {}; retained packet requires canonical inspection and explicit operator retry",
            other.err().map(|e| e.to_string()).unwrap_or_else(|| "FRAUD_ACK".into())
        ),
    }
}

#[cfg(test)]
#[path = "../../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::consensus::node_fraud;
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn batch() -> NodeServiceBatch {
        let state = fixture::state();
        NodeServiceBatch { claims: vec![fixture::claim(&state, 1)] }
    }

    #[test]
    fn report_is_signed_for_fraud_purpose_and_parses_as_fraud() {
        let _g = LOCK.lock().unwrap();
        let identity = fixture::identity();
        let key = OperatorKey::from_seed(fixture::key(10).as_bytes()).unwrap();
        let tx = assemble(batch(), fixture::bonds::address(10), 1, [7; 32], &identity, &key, 1_000_000).unwrap();
        assert_eq!(tx.tx_type, TransactionType::NodeFraudReport);
        let p = node_fraud::parse(&tx).expect("parses as fraud report");
        node_fraud::verify_authorization(&p, &identity, &[7; 32]).unwrap();
        // Deterministic: ed25519 signatures over the same body are identical (packet reuse on retry is safe).
        let again = assemble(batch(), fixture::bonds::address(10), 1, [7; 32], &identity, &key, 1_000_000).unwrap();
        assert_eq!(tx.data, again.data);
    }

    #[test]
    fn red_control_receipt_purpose_signature_is_not_a_fraud_report() {
        let _g = LOCK.lock().unwrap();
        let identity = fixture::identity();
        let key = OperatorKey::from_seed(fixture::key(10).as_bytes()).unwrap();
        let tx = assemble(batch(), fixture::bonds::address(10), 1, [7; 32], &identity, &key, 1_000_000).unwrap();
        let mut p = node_fraud::parse(&tx).unwrap();
        // Re-point the authorization at the receipt-submission purpose hash: consensus must refuse.
        p.authorization.batch_hash = wrfcoin_shared_protocol::consensus::node_receipt_registry::batch_hash(&p.batch).unwrap();
        assert!(node_fraud::verify_authorization(&p, &identity, &[7; 32]).is_err());
        // Wrong signer for the sender is refused before any packet exists.
        let other = OperatorKey::from_seed(fixture::key(20).as_bytes()).unwrap();
        assert!(assemble(batch(), fixture::bonds::address(10), 1, [7; 32], &identity, &other, 1).is_err());
        // Zero or three claims are refused.
        assert!(evidence(vec![]).is_err());
    }
}
