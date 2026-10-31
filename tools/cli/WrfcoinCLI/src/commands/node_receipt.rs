//! Owner-authorized receipt assembly; payout keys never participate.
use anyhow::{Result, ensure};
use clap::Subcommand;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};
use wrfcoin_cli::{ApiClient, ApiConfig, node_operator::OperatorKey};
#[cfg(test)]
use wrfcoin_shared_protocol::crypto::{domain_signing::DomainSeparated, signature::PublicKey};
use wrfcoin_shared_protocol::{
    blockchain::transaction::{Transaction, TransactionInput, TransactionOutput, TransactionType},
    common::types::Address,
    consensus::{
        node_receipt_registry::{
            NodeReceiptSubmission, NodeReceiptSubmissionAuthorization, batch_hash, claim_hash,
        },
        node_share_contracts::{BondedServiceClaim, MAX_NODE_CLAIMS, NodeServiceBatch},
    },
    crypto::{
        chain_binding::{ChainIdentity, SigningProtocolVersion},
        hash::hash_bytes,
        signature::PrivateKey,
    },
};
use zeroize::Zeroizing;

const INGEST_CAP: usize = 2_097_152;
// The receipt endpoint is stricter than bonded snapshot ingestion.
const SUBMIT_CAP: usize = 40 * 1024;
const ARTIFACT_CAP: usize = 40 * 1024;

#[derive(Subcommand)]
pub enum ReceiptCommands {
    Submit {
        #[clap(long)]
        archive: PathBuf,
        #[clap(long)]
        epoch: u64,
        /// Registered server deployment, not consumer deployment.
        #[clap(long)]
        deployment_id: String,
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
        /// Mode0600, current-user wallet PKCS#8 JSON or legacy raw hex seed.
        #[clap(long, conflicts_with = "mnemonic_stdin")]
        key_file: Option<PathBuf>,
        /// BIP-39 phrase on stdin (SLIP-0010 master, empty passphrase).
        #[clap(long, conflicts_with = "key_file")]
        mnemonic_stdin: bool,
        /// Owner-only file containing Operator bearer, never argv.
        #[clap(long)]
        operator_token_file: Option<PathBuf>,
        #[clap(long, default_value = "1000000")]
        fee: u64,
        /// New durable public signed packet; retained for uncertain retries.
        #[clap(long)]
        out: Option<PathBuf>,
        /// Hash and sizes only, without loading secrets or calling the API.
        #[clap(long)]
        dry_run: bool,
    },
}

fn id(text: &str) -> Result<[u8; 32]> {
    ensure!(text.len() == 64, "expected 32-byte hex identifier");
    hex::decode(text)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid identifier"))
}
pub(crate) fn bounded_read(path: &Path, cap: usize) -> Result<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "regular file required"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= cap, "file exceeds bound");
    Ok(bytes)
}
pub(crate) fn secret_file(path: &Path, cap: usize) -> Result<Zeroizing<Vec<u8>>> {
    wrfcoin_shared_protocol::security::key_file::assert_key_file_permissions(path)?;
    // Windows has no mode bits: require an owner-only DACL instead (the
    // launcher's operator.token passes), reading the handle that was checked.
    #[cfg(windows)]
    {
        let file = wrfcoin_shared_protocol::security::key_file::open_owner_only_key_file(path)?;
        let mut bytes = Zeroizing::new(Vec::new());
        file.take(cap as u64 + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= cap, "file exceeds bound");
        Ok(bytes)
    }
    #[cfg(not(windows))]
    Ok(Zeroizing::new(bounded_read(path, cap)?))
}

pub(crate) fn load_key(path: Option<&Path>, mnemonic_stdin: bool) -> Result<OperatorKey> {
    ensure!(
        // secret-leak-ok: boolean key-source selection only; the error contains no input value.
        path.is_some() ^ mnemonic_stdin,
        "select exactly one owner key source"
    );
    if let Some(path) = path {
        return OperatorKey::load(path);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    std::io::stdin().take(4097).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 4096, "mnemonic input exceeds bound");
    let phrase =
        std::str::from_utf8(&bytes).map_err(|_| anyhow::anyhow!("invalid mnemonic input"))?;
    let key = key_from_mnemonic(phrase)?;
    OperatorKey::from_seed(key.as_bytes())
}

fn key_from_mnemonic(phrase: &str) -> Result<PrivateKey> {
    let mnemonic: bip39::Mnemonic = phrase
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid mnemonic input"))?;
    let seed = Zeroizing::new(mnemonic.to_seed(""));
    let tag = ring::hmac::sign(
        &ring::hmac::Key::new(ring::hmac::HMAC_SHA512, b"ed25519 seed"),
        &*seed,
    );
    PrivateKey::from_bytes(&tag.as_ref()[..32])
        .map_err(|_| anyhow::anyhow!("mnemonic derivation failed"))
}

fn read_batch(
    archive: &Path,
    epoch: u64,
    deployment: [u8; 32],
    genesis: [u8; 32],
    identity: &ChainIdentity,
) -> Result<NodeServiceBatch> {
    let mut claims = BTreeMap::new();
    let mut scanned = 0;
    for entry in std::fs::read_dir(archive)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("bonded-") || !name.ends_with(".json") {
            continue;
        }
        scanned += 1;
        ensure!(
            scanned <= 256,
            "archive scan exceeds bound; split the epoch archive"
        );
        let body: Value = serde_json::from_slice(&bounded_read(&entry.path(), ARTIFACT_CAP)?)?;
        let encoded = body["claim_hex"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("archive claim missing"))?;
        ensure!(encoded.len() <= ARTIFACT_CAP, "claim exceeds bound");
        let claim = BondedServiceClaim::from_wire(&hex::decode(encoded)?)?;
        let c = &claim.ticket.context;
        ensure!(
            c.epoch == epoch
                && c.genesis_hash == genesis
                && c.chain_id == identity.chain_id
                && c.network_id == identity.network_id
                && claim.ticket.server_id == deployment,
            "archive epoch, chain, genesis or server mismatch"
        );
        // Use the exact contract functions called by consensus verify_claim.
        // Outer contracts require V2. Inner receipts may precede V2 activation;
        // consensus alone resolves the genesis schedule at their carrying height.
        // Registry binding is checked separately before owner signing.
        let server_key = id(&claim.manifest.signer_id)?;
        ensure!(
            claim.manifest.has_valid_signature_for(identity),
            "archive manifest signature refused"
        );
        claim
            .verify_contract(
                identity,
                &genesis,
                &server_key,
                &claim.receipt.receipt.consumer_pubkey,
                SigningProtocolVersion::V2,
            )
            .or_else(|_| {
                claim.verify_contract(
                    identity,
                    &genesis,
                    &server_key,
                    &claim.receipt.receipt.consumer_pubkey,
                    SigningProtocolVersion::V1,
                )
            })
            .map_err(|_| anyhow::anyhow!("archive contract signature refused"))?;
        let digest = claim_hash(&claim)?;
        ensure!(
            body["receipt_id"].as_str() == Some(hex::encode(digest).as_str()),
            "archive receipt identifier mismatch"
        );
        claims.insert(digest, claim);
        ensure!(
            claims.len() <= MAX_NODE_CLAIMS,
            "too many claims for one batch"
        );
    }
    let batch = NodeServiceBatch {
        claims: claims.into_values().collect(),
    };
    batch.to_wire()?;
    Ok(batch)
}

fn assemble(
    batch: NodeServiceBatch,
    sender: Address,
    nonce: u64,
    genesis: [u8; 32],
    identity: &ChainIdentity,
    key: &OperatorKey,
    fee: u64,
) -> Result<Transaction> {
    let public = key.public_key();
    ensure!(
        Address::from_public_key(&public) == sender,
        "selected key does not match registered owner sender"
    );
    let authorization = NodeReceiptSubmissionAuthorization {
        genesis_hash: genesis,
        sender,
        nonce,
        batch_hash: batch_hash(&batch)?,
    };
    let owner_signature = key
        .sign_receipt(&authorization, identity)?
        .as_bytes()
        .to_vec();
    let submission = NodeReceiptSubmission {
        batch,
        authorization,
        owner_key: public.as_bytes().try_into()?,
        owner_signature,
    };
    submission.verify_authorization(identity, &genesis)?;
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_bytes(b"wrfcoin.node_receipt_submission.input.v1"),
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
    tx.tx_type = TransactionType::NodeServiceBatch;
    tx.sender_address = sender;
    tx.nonce = nonce;
    tx.fee = fee;
    tx.data = Some(hex::encode(submission.to_wire()?));
    tx.set_id();
    let bytes = serde_json::to_vec(&tx)?;
    ensure!(
        bytes.len() <= SUBMIT_CAP && bytes.len() <= INGEST_CAP,
        "transaction exceeds receipt ingest cap"
    );
    Ok(tx)
}

pub(crate) fn durable_packet(out: &Path, tx: &Transaction) -> Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?;
    file.write_all(&serde_json::to_vec(tx)?)?;
    file.sync_all()?;
    std::fs::File::open(
        out.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?
    .sync_all()?;
    Ok(())
}

pub async fn handle(command: &ReceiptCommands) -> Result<Value> {
    let ReceiptCommands::Submit {
        archive,
        epoch,
        deployment_id,
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
    let genesis = id(genesis_hash)?;
    let sender: Address = sender.parse()?;
    let batch = read_batch(archive, *epoch, id(deployment_id)?, genesis, &identity)?;
    let digest = hex::encode(batch_hash(&batch)?);
    let ids: Vec<String> = batch
        .claims
        .iter()
        .map(|c| claim_hash(c).map(hex::encode))
        .collect::<Result<_, _>>()?;
    if *dry_run {
        return Ok(
            json!({"dry_run":true,"batch_hash":digest,"batch_bytes":batch.to_wire()?.len(),"receipt_ids":ids,"submitted":false,"included":false}),
        );
    }
    let out = out
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("--out required for durable submission"))?;
    ensure!(
        !out.exists(),
        "output packet already exists; inspect inclusion and retry that exact packet"
    );
    let key = load_key(key_file.as_deref(), *mnemonic_stdin)?;
    let selected_sender = key.address();
    ensure!(selected_sender == sender, "owner signer mismatch");
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
        operator_token_file
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--operator-token-file required"))?,
        16 * 1024,
    )?;
    let token = std::str::from_utf8(&token)
        .map_err(|_| anyhow::anyhow!("invalid operator credential"))?
        .trim();
    let credential_available = !token.is_empty();
    ensure!(credential_available, "operator credential unavailable"); // secret-leak-ok: constant error and boolean presence check, no credential contents.
    let api = ApiClient::new_without_redirects(ApiConfig {
        base_url: api_url.trim_end_matches('/').to_owned(),
        bearer_token: Some(token.to_owned()),
        ..ApiConfig::default()
    })?;
    let network: Value = api.receipt_request("GET", "/network/info", None).await?;
    ensure!(
        network["chain_id"].as_str() == Some(chain_id)
            && network["network_id"].as_str() == Some(network_id)
            && network["genesis_hash"]
                .as_str()
                .and_then(super::node_deposit::api_hash)
                == Some(genesis),
        "API chain/genesis mismatch"
    );
    let registration: Value = api
        .receipt_request(
            "GET",
            &format!("/node/bonds/{}", hex::encode(id(deployment_id)?)),
            None,
        )
        .await?;
    ensure!(
        registration["registration"]["binding"]["owner_account"] == serde_json::to_value(sender)?,
        "registered owner mismatch"
    );
    for claim in &batch.claims {
        let server = &registration["registration"]["binding"];
        let consumer = api
            .receipt_request(
                "GET",
                &format!("/node/bonds/{}", hex::encode(claim.ticket.consumer_id)),
                None,
            )
            .await?;
        ensure!(
            server["server_key"] == serde_json::to_value(id(&claim.manifest.signer_id)?)?
                && consumer["registration"]["binding"]["consumer_key"]
                    == serde_json::to_value(claim.receipt.receipt.consumer_pubkey)?,
            "archive registered signer mismatch"
        );
    }
    let account = api
        .receipt_request("GET", &format!("/addresses/{sender}"), None)
        .await?;
    let nonce = account["nonce"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("sender nonce unavailable"))?
        .parse::<u64>()?;
    let tx = assemble(batch, sender, nonce, genesis, &identity, &key, *fee)?;
    let size = serde_json::to_vec(&tx)?.len();
    durable_packet(out, &tx)?;
    let ack = api
        .receipt_request(
            "POST",
            "/node/receipts/submit",
            Some(serde_json::to_value(&tx)?),
        )
        .await;
    let admitted = ack.as_ref().is_ok_and(|a| {
        a["admitted"] == true && a["tx_id"].as_str() == tx.id.map(|h| h.to_hex()).as_deref()
    });

    let mut views = Vec::new();
    for receipt_id in &ids {
        let view = api
            .receipt_request("GET", &format!("/node/receipts/{receipt_id}"), None)
            .await
            .unwrap_or(Value::Null);
        views.push(json!({"receipt_id":receipt_id,"included":view["receipt_id"].as_str() == Some(receipt_id) && view["status"] == "INCLUDED" && view["included"] == true}));
    }
    let included = views.iter().all(|v| v["included"] == true);
    if !admitted {
        let safe_error = ack
            .err()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "RECEIPT_ACK".into());
        anyhow::bail!(
            "UNCERTAIN {safe_error}; included={included}; retained packet requires canonical inspection and explicit operator retry"
        );
    }
    Ok(
        json!({"batch_hash":digest,"transaction_bytes":size,"tx_id":tx.id.map(|h| h.to_hex()),"admitted":true,"included":included,"receipt_ids":ids,"receipts":views,"packet":out.display().to_string()}),
    )
}

#[cfg(test)]
#[path = "../../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod fixture;

#[cfg(test)]
mod tests {
    use super::*;
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn batch() -> NodeServiceBatch {
        // Fixture construction installs test-only global authority: serialize it.
        let state = fixture::state();
        NodeServiceBatch {
            claims: vec![fixture::claim(&state, 1)],
        }
    }
    #[test]
    fn receipt_v2_signature_and_red_controls_use_consensus_verifier() {
        let _guard = LOCK.lock().unwrap();
        let identity = fixture::identity();
        let tx = assemble(
            batch(),
            fixture::bonds::address(10),
            1,
            [7; 32],
            &identity,
            &OperatorKey::from_seed(fixture::key(10).as_bytes()).unwrap(),
            1_000_000,
        )
        .unwrap();
        let p = NodeReceiptSubmission::parse(&tx).unwrap();
        p.verify_authorization(&identity, &[7; 32]).unwrap();
        let preimage = p
            .authorization
            .signing_preimage_for(&identity, SigningProtocolVersion::V2);
        // #3535 V2 escrow bindings change the frozen roster and batch digest.
        assert_eq!(
            hash_bytes(&preimage).as_bytes().as_slice(),
            &[
                41, 34, 96, 164, 191, 198, 99, 67, 53, 60, 31, 139, 206, 18, 55, 122, 144, 31, 110,
                211, 112, 117, 6, 192, 23, 161, 2, 250, 218, 236, 214, 232
            ]
        );
        assert_eq!(
            p.owner_signature.as_slice(),
            &[
                2, 105, 142, 152, 67, 133, 62, 38, 90, 24, 131, 35, 112, 103, 250, 144, 72, 106,
                222, 25, 203, 104, 178, 116, 158, 80, 250, 95, 100, 9, 187, 111, 157, 89, 242, 13,
                132, 53, 182, 156, 86, 31, 127, 11, 60, 43, 19, 77, 206, 124, 185, 139, 13, 90, 19,
                46, 49, 185, 55, 107, 183, 22, 34, 5
            ]
        );
        assert!(
            p.verify_authorization(&ChainIdentity::new("778", "testnet"), &[7; 32])
                .is_err()
        );
        assert!(
            p.verify_authorization(&ChainIdentity::new("777", "mainnet"), &[7; 32])
                .is_err()
        );
        assert!(p.verify_authorization(&identity, &[8; 32]).is_err());
        for mutation in 0..5 {
            let mut wrong = p.clone();
            match mutation {
                0 => wrong.authorization.nonce += 1,
                1 => wrong.authorization.sender = fixture::bonds::address(20),
                2 => wrong.authorization.batch_hash[0] ^= 1,
                3 => {
                    wrong.owner_signature = fixture::key(10)
                        .sign(
                            &p.authorization
                                .signing_preimage_for(&identity, SigningProtocolVersion::V1),
                        )
                        .as_bytes()
                        .to_vec()
                },
                _ => {
                    let mut wrong_domain = preimage.clone();
                    let offset = wrong_domain
                        .windows(b"wrfcoin.node_receipt_submission.v1".len())
                        .position(|w| w == b"wrfcoin.node_receipt_submission.v1")
                        .unwrap();
                    wrong_domain[offset] ^= 1;
                    wrong.owner_signature =
                        fixture::key(10).sign(&wrong_domain).as_bytes().to_vec();
                },
            }
            assert!(
                wrong.verify_authorization(&identity, &[7; 32]).is_err(),
                "RED control {mutation}"
            );
        }
        let mut wrong_nonce = tx.clone();
        wrong_nonce.nonce += 1;
        assert!(NodeReceiptSubmission::parse(&wrong_nonce).is_none());
        let mut state = fixture::state();
        let mut stale = p.clone();
        stale.authorization.nonce = 0;
        stale.owner_signature = fixture::sign(&stale.authorization, 10);
        assert!(fixture::apply(&mut state, stale).is_err());
        fixture::apply(&mut state, p).unwrap();
    }
    #[test]
    fn receipt_archive_binding_bounds_and_deterministic_assembly() {
        let _guard = LOCK.lock().unwrap();
        let batch = batch();
        let claim = &batch.claims[0];
        let root = std::env::temp_dir().join(format!("receipt-cli-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let body = json!({"claim_hex":hex::encode(claim.to_wire().unwrap()),"receipt_id":hex::encode(claim_hash(claim).unwrap()),"included":false});
        std::fs::write(
            root.join("bonded-one.json"),
            serde_json::to_vec(&body).unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join("bonded-duplicate.json"),
            serde_json::to_vec(&body).unwrap(),
        )
        .unwrap();
        assert_eq!(
            read_batch(&root, 1, [1; 32], [7; 32], &fixture::identity()).unwrap(),
            batch
        );
        assert!(read_batch(&root, 2, [1; 32], [7; 32], &fixture::identity()).is_err());
        assert!(read_batch(&root, 1, [2; 32], [7; 32], &fixture::identity()).is_err());
        assert!(read_batch(&root, 1, [1; 32], [8; 32], &fixture::identity()).is_err());
        assert!(
            read_batch(
                &root,
                1,
                [1; 32],
                [7; 32],
                &ChainIdentity::new("778", "testnet")
            )
            .is_err()
        );
        assert!(
            // secret-leak-ok: fixed synthetic key fixture and boolean error control; no private values formatted.
            assemble(
                batch.clone(),
                fixture::bonds::address(20),
                1,
                [7; 32],
                &fixture::identity(),
                &OperatorKey::from_seed(fixture::key(10).as_bytes()).unwrap(),
                1_000_000
            )
            .is_err()
        );
        let oversized = NodeServiceBatch {
            claims: vec![claim.clone(); MAX_NODE_CLAIMS + 1],
        };
        assert!(
            // secret-leak-ok: fixed synthetic key fixture and boolean error control; no private values formatted.
            assemble(
                oversized,
                fixture::bonds::address(10),
                1,
                [7; 32],
                &fixture::identity(),
                &OperatorKey::from_seed(fixture::key(10).as_bytes()).unwrap(),
                1_000_000
            )
            .is_err()
        );
        std::fs::write(
            root.join("bonded-too-large.json"),
            vec![b' '; INGEST_CAP + 1],
        )
        .unwrap();
        assert!(read_batch(&root, 1, [1; 32], [7; 32], &fixture::identity()).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn distinct_cold_payout_accepts_hot_owner_receipt_in_consensus() {
        use wrfcoin_shared_protocol::consensus::node_share_contracts::NodeBondAction;
        let _guard = LOCK.lock().unwrap();
        let mut state = fixture::bonds::state();
        let mut registration = fixture::bonds::auth(10, 1, 0, NodeBondAction::Deposit, 1);
        registration.signed.authorization.payout_account = fixture::bonds::address(30);
        fixture::bonds::resign(&mut registration, 10);
        registration.payout_key = Some(fixture::pk(30));
        registration.payout_signature =
            fixture::bonds::signature(&registration.signed.authorization, 30);
        fixture::bonds::apply(&mut state, registration, fixture::bonds::A).unwrap();
        fixture::bonds::apply(
            &mut state,
            fixture::bonds::auth(20, 2, 0, NodeBondAction::Deposit, 1),
            fixture::bonds::A,
        )
        .unwrap();
        fixture::bonds::advance(&mut state, fixture::H);
        let binding =
            &state.node_bond_registry.as_ref().unwrap().frozen_epochs[&1].bindings[&[1; 32]];
        assert_ne!(binding.owner_account, binding.payout_account);
        let batch = NodeServiceBatch {
            claims: vec![fixture::claim(&state, 1)],
        };
        let tx = assemble(
            batch,
            fixture::bonds::address(10),
            1,
            [7; 32],
            &fixture::identity(),
            &OperatorKey::from_seed(fixture::key(10).as_bytes()).unwrap(),
            1_000_000,
        )
        .unwrap();
        fixture::apply(&mut state, NodeReceiptSubmission::parse(&tx).unwrap()).unwrap();
        assert_eq!(
            state.node_receipt_registry.as_ref().unwrap().accepted.len(),
            1
        );
    }

    #[test]
    fn mnemonic_matches_existing_wallet_master_and_sanitizes_errors() {
        use base64::Engine;
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let key = key_from_mnemonic(phrase).unwrap();
        let wallet = wrfcoin_cli::crypto::WrfKeyPair::from_mnemonic(phrase).unwrap();
        assert_eq!(
            // secret-leak-ok: compares public keys from fixed synthetic mnemonic, no private bytes.
            base64::engine::general_purpose::STANDARD
                .encode(PublicKey::from_private_key(&key).as_bytes()),
            wallet.public_key
        );
        assert_eq!(
            // secret-leak-ok: asserts constant sanitized error for fixed invalid synthetic input.
            key_from_mnemonic("synthetic-invalid-secret")
                .err()
                .unwrap()
                .to_string(),
            "invalid mnemonic input"
        );
    }

    #[test]
    fn receipt_cli_fetches_nonce_submits_and_reads_back_id() {
        let _guard = LOCK.lock().unwrap();
        let batch = batch();
        let root = std::env::temp_dir().join(format!("receipt-cli-http-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let receipt_id = hex::encode(claim_hash(&batch.claims[0]).unwrap());
        let body = json!({"claim_hex":hex::encode(batch.claims[0].to_wire().unwrap()),"receipt_id":receipt_id,"included":false});
        std::fs::write(
            root.join("bonded-one.json"),
            serde_json::to_vec(&body).unwrap(),
        )
        .unwrap();
        let key_file = root.join("key");
        let token_file = root.join("token");
        std::fs::write(&key_file, hex::encode([10u8; 32])).unwrap(); // secret-leak-ok: synthetic signing fixture only.
        std::fs::write(&token_file, "synthetic-receipt-cli-bearer").unwrap(); // secret-leak-ok: fixed synthetic bearer for mock server only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [&key_file, &token_file] {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
        }
        let sender = fixture::bonds::address(10);
        let expected = assemble(
            batch,
            sender,
            1,
            [7; 32],
            &fixture::identity(),
            &OperatorKey::from_seed(fixture::key(10).as_bytes()).unwrap(),
            1_000_000,
        )
        .unwrap();
        let network = mockito::mock("GET", "/network/info")
            .with_body(
                // Real launch_api shape: routes/network.rs get_network_info emits
                // `"genesis_hash": format!("0x{}", hash.to_hex())` (0x + 64 hex).
                json!({"chain_id":"777","network_id":"testnet","genesis_hash":format!("0x{}",hex::encode([7;32]))})
                    .to_string(),
            )
            .create();
        let registration = mockito::mock(
            "GET",
            format!("/node/bonds/{}", hex::encode([1; 32])).as_str(),
        )
        .with_body(json!({"registration":{"binding":{"owner_account":sender,"server_key":id(&fixture::claim(&fixture::state(),1).manifest.signer_id).unwrap()}}}).to_string())
        .create();
        let consumer = mockito::mock("GET", format!("/node/bonds/{}", hex::encode(fixture::claim(&fixture::state(),1).ticket.consumer_id)).as_str())
            .with_body(json!({"registration":{"binding":{"consumer_key":fixture::claim(&fixture::state(),1).receipt.receipt.consumer_pubkey}}}).to_string()).create();
        let account = mockito::mock("GET", format!("/addresses/{sender}").as_str()).with_body(json!({"address":sender.to_string(),"balance":"100000000000","spendable_balance":"100000000000","stake_locked":"0","earned_balance_total":"0","nonce":"1"}).to_string()).create();
        let post = mockito::mock("POST", "/node/receipts/submit")
            .match_body(mockito::Matcher::PartialJson(
                json!({"sender_address":sender,"nonce":1,"fee":1000000,"data":expected.data}),
            ))
            .with_status(202)
            .with_body_from_request(|request| {
                let tx: Transaction = serde_json::from_slice(request.body().unwrap()).unwrap();
                NodeReceiptSubmission::parse(&tx)
                    .unwrap()
                    .verify_authorization(&fixture::identity(), &[7; 32])
                    .unwrap();
                json!({"tx_id":tx.id.unwrap().to_hex(),"admitted":true,"included":false})
                    .to_string()
                    .into_bytes()
            })
            .create();
        let readback = mockito::mock("GET", format!("/node/receipts/{receipt_id}").as_str())
            .with_body(
                json!({"receipt_id":receipt_id,"included":null,"status":"UNKNOWN"}).to_string(),
            )
            .create();
        let command = ReceiptCommands::Submit {
            archive: root.clone(),
            epoch: 1,
            deployment_id: hex::encode([1; 32]),
            sender: sender.to_string(),
            genesis_hash: hex::encode([7; 32]),
            chain_id: "777".into(),
            network_id: "testnet".into(),
            api_url: mockito::server_url(),
            key_file: Some(key_file),
            mnemonic_stdin: false,
            operator_token_file: Some(token_file),
            fee: 1_000_000,
            out: Some(root.join("packet.json")),
            dry_run: false,
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(handle(&command)).unwrap();
        assert_eq!(result["admitted"], true);
        assert_eq!(result["included"], false);
        assert_eq!(result["receipt_ids"][0], receipt_id);
        let packet: Transaction =
            serde_json::from_slice(&std::fs::read(root.join("packet.json")).unwrap()).unwrap();
        assert_eq!(packet.data, expected.data);
        assert_eq!(packet.nonce, 1);
        assert_eq!(result["tx_id"], packet.id.unwrap().to_hex());
        assert!(runtime.block_on(handle(&command)).is_err());
        network.assert();
        registration.assert();
        account.assert();
        consumer.assert();
        post.assert();
        readback.assert();
        std::fs::remove_dir_all(root).unwrap();
    }
}
