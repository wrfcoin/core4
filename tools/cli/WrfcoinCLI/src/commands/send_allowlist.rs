//! `wrfcoin send-allowlist`: build, submit and inspect chain-enforced send
//! allowlist actions (`shared-protocol/src/accounts/send_allowlist.rs`).
//!
//! The signing commands are offline, like `station owner`: each reads the
//! account's ed25519 key file, signs the exact preimage core4 verifies,
//! self-verifies it, and writes the `Transaction` JSON that is the body of
//! `POST /accounts/send-allowlist/submit`. `--submitted-at` is pinned so the
//! artifact and its txid are reproducible. `--nonce` is the account's
//! `next_action_nonce` from `send-allowlist show`; the CLI never guesses it.
//!
//! `submit` and `show` talk to the node (`WRFCOIN_API_URL`, token from
//! `WRFCOIN_OPERATOR_TOKEN` / `WRFCOIN_READER_TOKEN`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, ensure};
use clap::{Args, Subcommand};
use serde_json::{Value, json};

use wrfcoin_shared_protocol::accounts::send_allowlist::{
    DEFAULT_ADD_DELAY_BLOCKS, SendAllowlistAction, SignedSendAllowlistAction,
};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::Address;

use wrfcoin_cli::{ApiClient, ApiConfig};

/// Arguments every signed action shares.
#[derive(Args, Debug)]
pub struct SignArgs {
    /// The account's ed25519 private key file (hex). Never leaves this machine.
    #[clap(long)]
    pub key: PathBuf,
    /// Chain id the signature binds (use the value the node reports, e.g. 888).
    #[clap(long)]
    pub chain_id: String,
    /// The account's `next_action_nonce` (from `send-allowlist show`).
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
pub enum SendAllowlistCommands {
    /// Turn the allowlist on now, with an initial list. Only while it is off.
    Enable {
        #[clap(flatten)]
        sign: SignArgs,
        /// Waiting period for later additions and for turning it off, in blocks
        /// (default ~48 h at 77 s/block; minimum ~24 h).
        #[clap(long, default_value_t = DEFAULT_ADD_DELAY_BLOCKS)]
        delay_blocks: u64,
        /// Address allowed from the start (repeatable).
        #[clap(long = "allow")]
        allow: Vec<String>,
    },
    /// Ask for an address to be added. It takes effect after the delay.
    Add {
        #[clap(flatten)]
        sign: SignArgs,
        #[clap(long)]
        address: String,
    },
    /// Cancel a pending addition (starts a cooldown on new requests).
    CancelAdd {
        #[clap(flatten)]
        sign: SignArgs,
        #[clap(long)]
        address: String,
    },
    /// Remove an address now (allowed or pending).
    Remove {
        #[clap(flatten)]
        sign: SignArgs,
        #[clap(long)]
        address: String,
    },
    /// Raise the waiting period now (it can never be lowered while on).
    RaiseDelay {
        #[clap(flatten)]
        sign: SignArgs,
        #[clap(long)]
        delay_blocks: u64,
    },
    /// Ask for the allowlist to be turned off. Takes effect after the delay.
    Disable {
        #[clap(flatten)]
        sign: SignArgs,
    },
    /// Cancel a pending turn-off (starts a cooldown on new requests).
    CancelDisable {
        #[clap(flatten)]
        sign: SignArgs,
    },
    /// Submit an artifact written by one of the commands above.
    Submit {
        #[clap(long)]
        artifact: PathBuf,
    },
    /// Show an account's allowlist, pending additions and next nonce.
    Show {
        #[clap(long)]
        address: String,
    },
}

fn parse_address(text: &str) -> Result<Address> {
    text.trim()
        .parse::<Address>()
        .map_err(|e| anyhow!("{text} is not a 40-hex-character address: {e:?}"))
}

/// Build the signed transaction for `action`. Pure apart from the key.
pub fn build_transaction(
    key: &wrfcoin_shared_protocol::crypto::signature::PrivateKey,
    chain_id: &str,
    action: SendAllowlistAction,
    nonce: u64,
    submitted_at: i64,
) -> Result<Transaction> {
    let signed = SignedSendAllowlistAction::sign(chain_id, key, action, nonce);
    signed
        .verify(chain_id)
        .map_err(|e| anyhow!("self-verification failed: {e}"))?;
    let mut outpoint = b"wrfcoin.send_allowlist.outpoint.v1".to_vec();
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
    tx.tx_type = TransactionType::SendAllowlist;
    tx.sender_address = signed.account;
    tx.nonce = nonce;
    tx.timestamp = chrono::DateTime::from_timestamp(submitted_at, 0)
        .ok_or_else(|| anyhow!("--submitted-at {submitted_at} is not a valid unix time"))?;
    tx.data = Some(serde_json::to_string(&signed)?);
    tx.set_id();
    ensure!(
        SignedSendAllowlistAction::parse(&tx).as_ref() == Some(&signed),
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

fn sign_and_write(sign: &SignArgs, action: SendAllowlistAction) -> Result<Value> {
    let key = super::station::read_key(&sign.key)?;
    let tx = build_transaction(&key, &sign.chain_id, action.clone(), sign.nonce, sign.submitted_at)?;
    write_artifact(&sign.out, &tx)?;
    Ok(json!({
        "account": tx.sender_address.to_string(),
        "action": action,
        "action_nonce": sign.nonce,
        "tx_id": tx.id.map(|id| id.to_hex()),
        "artifact": sign.out.display().to_string(),
        "next": "wrfcoin send-allowlist submit --artifact <artifact>",
    }))
}

pub async fn handle_send_allowlist_command(command: &SendAllowlistCommands) -> Result<Value> {
    use SendAllowlistCommands as C;
    match command {
        C::Enable { sign, delay_blocks, allow } => {
            let allowed = allow.iter().map(|a| parse_address(a)).collect::<Result<Vec<_>>>()?;
            sign_and_write(sign, SendAllowlistAction::Enable { add_delay_blocks: *delay_blocks, allowed })
        },
        C::Add { sign, address } => {
            sign_and_write(sign, SendAllowlistAction::RequestAdd { address: parse_address(address)? })
        },
        C::CancelAdd { sign, address } => {
            sign_and_write(sign, SendAllowlistAction::CancelAdd { address: parse_address(address)? })
        },
        C::Remove { sign, address } => {
            sign_and_write(sign, SendAllowlistAction::Remove { address: parse_address(address)? })
        },
        C::RaiseDelay { sign, delay_blocks } => {
            sign_and_write(sign, SendAllowlistAction::RaiseDelay { add_delay_blocks: *delay_blocks })
        },
        C::Disable { sign } => sign_and_write(sign, SendAllowlistAction::RequestDisable),
        C::CancelDisable { sign } => sign_and_write(sign, SendAllowlistAction::CancelDisable),
        C::Submit { artifact } => {
            let raw = std::fs::read(artifact)
                .with_context(|| format!("cannot read {}", artifact.display()))?;
            let tx: Transaction = serde_json::from_slice(&raw)
                .with_context(|| format!("{} is not a transaction artifact", artifact.display()))?;
            ensure!(
                SignedSendAllowlistAction::parse(&tx).is_some(),
                "{} is not a send-allowlist transaction",
                artifact.display()
            );
            let api = ApiClient::new(ApiConfig::default())?;
            let ack = api
                .receipt_request("POST", "/accounts/send-allowlist/submit", Some(serde_json::to_value(&tx)?))
                .await
                .map_err(|e| anyhow!("node refused the action ({e}); run `send-allowlist show` to check the nonce and pending state"))?;
            Ok(json!({"tx_id": tx.id.map(|id| id.to_hex()), "node": ack}))
        },
        C::Show { address } => {
            let account = parse_address(address)?;
            let api = ApiClient::new(ApiConfig::default())?;
            api.receipt_request("GET", &format!("/accounts/{account}/send-allowlist"), None).await
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};
    use wrfcoin_shared_protocol::crypto::signature::PrivateKey;

    #[test]
    fn built_transaction_is_reproducible_and_verifies() {
        let key = PrivateKey::generate(&mut StdRng::seed_from_u64(7));
        let action = SendAllowlistAction::RequestAdd { address: Address([3; 20]) };
        let a = build_transaction(&key, "888", action.clone(), 4, 1_800_000_000).unwrap();
        let b = build_transaction(&key, "888", action, 4, 1_800_000_000).unwrap();
        assert_eq!(a.id, b.id, "pinned timestamp makes the txid reproducible");
        let signed = SignedSendAllowlistAction::parse(&a).unwrap();
        assert!(signed.verify("888").is_ok());
        assert!(signed.verify("777").is_err(), "bound to the chain id");
        assert_eq!(a.tx_type, TransactionType::SendAllowlist);
        assert!(a.validate_structure());
    }
}
