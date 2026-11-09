//! Offline bond authorization with production BlockSigner; public packets only.
use anyhow::{Result, ensure};
use clap::{Subcommand, ValueEnum};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
use wrfcoin_cli::{
    ApiClient, ApiConfig,
    node_operator::{OperatorKey, OperatorRole},
};
use wrfcoin_shared_protocol::{
    blockchain::transaction::Transaction,
    common::types::Address,
    accounts::passkey::PasskeyAuthorization,
    consensus::{
        node_bond_registry::{NodeBondLifecycle, verify_payout_passkey},
        node_share_contracts::*,
    },
    crypto::{
        chain_binding::{ChainIdentity, SigningProtocolVersion},
        domain_signing::DomainSeparated,
        signer::{BlockSigner, SharedKeyFallback, SignerEnv, load_block_signer_for},
    },
    governance::schedule_extension::bond_admission_keys,
};

#[derive(Clone, Copy, ValueEnum)]
pub enum BondSignerRole {
    Owner,
    Server,
    Consumer,
    Governance,
    /// The pinned bond admission key co-signing a Deposit or Update.
    Admission,
    Payout,
}

#[derive(Subcommand)]
pub enum BondCommands {
    /// Build a dual-signed faucet request; only --submit performs a network write.
    /// Seed-file nodes may also use infra/testnet/services/faucet/bond-grant-request.js.
    GrantRequest {
        #[clap(long)]
        owner_address: String,
        /// Node API base URL (GET /network/info and /chain/block/<tip-2>).
        #[clap(long)]
        api_url: String,
        /// Write the public JSON request to a new file; never overwrite.
        #[clap(long)]
        out: Option<PathBuf>,
        /// Backend base URL; POST once to /api/faucet/bond-grant, without retries.
        #[clap(long)]
        submit: Option<String>,
        /// Owner-only file (Unix mode 0600; Windows: only your account) holding the node's
        /// 32-byte ed25519 server seed as hex. Testnet only. Otherwise the dedicated bond HSM signer.
        #[clap(long)]
        key_file: Option<PathBuf>,
        /// Existing protobuf ed25519 identity; default $WRFCOIN_DATA_DIR/peer_identity.key (./data if unset).
        #[clap(long)]
        p2p_key_file: Option<PathBuf>,
    },
    /// Assemble a keyless Deposit plan, then authorize and submit the approved packet once.
    Deposit(super::node_deposit::DepositArgs),
    /// Assemble a keyless Update moving a registered bond's payout account
    /// (amount 0, other fields from the node's bond), then authorize and
    /// submit the approved packet once.
    Update(super::node_bond_update::UpdateArgs),
    /// Read the canonical public owner address/key without signing or an API request.
    KeyInfo {
        #[clap(long)]
        key_file: PathBuf,
    },
    /// Sign one role of an unsigned/partially signed public transaction offline.
    /// Explicit operator key files are confined to Node owner/server/consumer roles; other roles use HSM.
    Sign {
        #[clap(long)]
        transaction_file: PathBuf,
        /// Current-user mode0600 wallet JSON or raw hex key, for Node operator roles only.
        #[clap(long)]
        key_file: Option<PathBuf>,
        #[clap(long, value_enum)]
        role: BondSignerRole,
        #[clap(long)]
        chain_id: String,
        #[clap(long)]
        network_id: String,
        #[clap(long)]
        out: PathBuf,
    },
    /// Print the 32-byte WebAuthn challenge a passkey payout wallet signs for
    /// an unsigned Deposit/Update packet (`passkey_bond_payout`). Offline.
    PayoutChallenge {
        #[clap(long)]
        transaction_file: PathBuf,
        #[clap(long)]
        chain_id: String,
        #[clap(long)]
        network_id: String,
    },
    /// Attach a passkey payout wallet's assertion to a Deposit/Update packet
    /// whose payout_key is empty, after verifying it locally against the
    /// packet's challenge and payout account. Offline; writes a new file.
    AttachPayoutPasskey {
        #[clap(long)]
        transaction_file: PathBuf,
        /// The `payout_passkey` object (or a lifecycle/transaction carrying it).
        #[clap(long)]
        payout_passkey_assertion: PathBuf,
        #[clap(long)]
        chain_id: String,
        #[clap(long)]
        network_id: String,
        #[clap(long)]
        out: PathBuf,
    },
    /// Admission service: check a Deposit/Update that names a pinned bond
    /// admission key as co-signer (`bond_admission_key`), and with --key-file
    /// co-sign it. A software key file is accepted on testnet only. Offline.
    Admit(super::node_bond_admit::AdmitArgs),
    /// Node side: bind this node's peer id and public P2P address to a packet
    /// for the admission service (peer-identity signature). Offline.
    AdmissionRequest(super::node_bond_admit::AdmissionRequestArgs),
    /// Dial a P2P address ending in /p2p/<peer id> and ask the keyless
    /// bonded-fetch Info: proves the node answers there. Sends no key or token.
    Probe {
        #[clap(long)]
        addr: String,
    },
    /// Submit a fully signed registration and 250 WRF bond deposit.
    RegisterBond {
        #[clap(long)]
        transaction_file: PathBuf,
    },
    /// Submit a fully signed Update (e.g. a new payout account).
    SubmitUpdate {
        #[clap(long)]
        transaction_file: PathBuf,
    },
    /// Submit an off-host owner-authorized exit (no immediate withdrawal).
    Exit {
        #[clap(long)]
        transaction_file: PathBuf,
    },
    /// Submit an off-host owner-authorized principal withdrawal after liability.
    Withdraw {
        #[clap(long)]
        transaction_file: PathBuf,
    },
    /// Inspect a deployment's funded bond, bindings and exit state.
    Inspect {
        #[clap(long)]
        deployment_id: String,
    },
    /// Inspect an epoch's provisional entitlements and matured payouts.
    Epoch {
        #[clap(long)]
        epoch: u64,
    },
    /// Inspect canonical receipt inclusion, distinct from local acknowledgement.
    Receipt {
        #[clap(long)]
        receipt_id: String,
    },
}

pub(crate) fn read_transaction(path: &Path) -> Result<Transaction> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(40 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 40 * 1024, "Node transaction exceeds bound");
    serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("public Node transaction format refused"))
}
/// The passkey payout challenge for `a`, as the approval page consumes it.
pub(crate) fn payout_challenge_json(a: &NodeBondAuthorization, identity: &ChainIdentity) -> Value {
    let challenge = node_bond_payout_passkey_challenge(a, identity);
    json!({
        "challenge_hex": hex::encode(challenge),
        "challenge_base64url": wrfcoin_shared_protocol::accounts::passkey::base64url_nopad(&challenge),
        "payout_account": a.payout_account.to_hex(),
        "deployment_id": hex::encode(a.deployment_id),
        "action": format!("{:?}", a.action),
        "expected_nonce": a.expected_nonce,
        "effective_epoch": a.effective_epoch,
        "chain_id": identity.chain_id,
        "network_id": identity.network_id,
    })
}

/// Read a passkey payout approval: a bare `payout_passkey` object, or a
/// lifecycle / transaction carrying one. Verified against `a` before use.
pub(crate) fn read_payout_passkey(
    path: &Path,
    a: &NodeBondAuthorization,
    identity: &ChainIdentity,
) -> Result<PasskeyAuthorization> {
    let bytes = super::node_receipt::bounded_read(path, 40 * 1024)?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("passkey payout assertion file refused"))?;
    let lifecycle = value
        .get("data")
        .and_then(Value::as_str)
        .and_then(|data| serde_json::from_str::<Value>(data).ok())
        .unwrap_or(value);
    let candidate = lifecycle.get("payout_passkey").cloned().unwrap_or(lifecycle);
    let approval: PasskeyAuthorization = serde_json::from_value(candidate)
        .map_err(|_| anyhow::anyhow!("passkey payout assertion format refused"))?;
    verify_payout_passkey(a, &approval, identity).map_err(|_| {
        anyhow::anyhow!(
            "passkey payout assertion does not verify for this packet's payout account and challenge"
        )
    })?;
    Ok(approval)
}

/// Attach a verified passkey payout approval to an online lifecycle packet.
pub(crate) fn attach_payout_passkey(
    tx: &mut Transaction,
    path: &Path,
    identity: &ChainIdentity,
) -> Result<()> {
    let mut p = NodeBondLifecycle::parse(tx)
        .ok_or_else(|| anyhow::anyhow!("invalid Node lifecycle transaction"))?;
    ensure!(
        matches!(p.signed.authorization.action, NodeBondAction::Deposit | NodeBondAction::Update),
        "only Deposit/Update carry a payout approval"
    );
    ensure!(
        p.payout_key.is_none() && p.payout_signature.is_empty(),
        "packet names an ed25519 payout key; reassemble without --payout-key for a passkey payout"
    );
    p.payout_passkey = Some(read_payout_passkey(path, &p.signed.authorization, identity)?);
    tx.data = Some(serde_json::to_string(&p)?);
    tx.set_id();
    Ok(())
}

fn public_id(value: &str) -> Result<String> {
    ensure!(value.len() == 64, "expected a 32-byte hex identifier");
    Ok(hex::encode(hex::decode(value)?))
}

/// Subject bytes and signer-key binding are checked before any signature.
pub(crate) fn sign_packet(
    tx: &mut Transaction,
    role: BondSignerRole,
    identity: &ChainIdentity,
    signer: &dyn BlockSigner,
) -> Result<()> {
    let mut p = NodeBondLifecycle::parse(tx)
        .ok_or_else(|| anyhow::anyhow!("invalid Node lifecycle transaction"))?;
    let a = &p.signed.authorization;
    let key: [u8; 32] = signer.public_key().as_bytes().try_into()?;
    let online = matches!(a.action, NodeBondAction::Deposit | NodeBondAction::Update);
    let preimage = match role {
        BondSignerRole::Owner => {
            ensure!(
                key == p.owner_key
                    && Address::from_public_key(&signer.public_key()) == a.owner_account,
                "owner signer mismatch"
            );
            a.signing_preimage_for(identity, SigningProtocolVersion::V2)
        },
        BondSignerRole::Server | BondSignerRole::Consumer => {
            ensure!(online, "online keys cannot authorize exit or withdrawal");
            let server = matches!(role, BondSignerRole::Server);
            ensure!(
                key == if server { a.server_key } else { a.consumer_key },
                "service signer mismatch"
            );
            NodeBondPossession {
                authorization_preimage_hash: node_contract_digest(a, identity),
                role: if server {
                    NodeBondKeyRole::Server
                } else {
                    NodeBondKeyRole::Consumer
                },
            }
            .signing_preimage_for(identity, SigningProtocolVersion::V2)
        },
        BondSignerRole::Governance => {
            ensure!(
                online && p.governance_key == Some(key),
                "governance signer mismatch"
            );
            a.signing_preimage_for(identity, SigningProtocolVersion::V2)
        },
        BondSignerRole::Admission => {
            // Deposit and Update only, and only a key this binary pins for
            // the packet's chain: never the role a governance key signs under.
            ensure!(
                online
                    && p.governance_key == Some(key)
                    && bond_admission_keys(&hex::encode(a.genesis_hash))
                        .contains(&hex::encode(key)),
                "admission signer mismatch"
            );
            a.signing_preimage_for(identity, SigningProtocolVersion::V2)
        },
        BondSignerRole::Payout => {
            ensure!(
                online
                    && p.payout_key == Some(key)
                    && Address::from_public_key(&signer.public_key()) == a.payout_account,
                "payout signer mismatch"
            );
            a.signing_preimage_for(identity, SigningProtocolVersion::V2)
        },
    };
    let signature = signer.sign(&preimage)?.as_bytes().to_vec();
    match role {
        BondSignerRole::Owner => p.signed.owner_signature = signature,
        BondSignerRole::Server => p.signed.server_signature = signature,
        BondSignerRole::Consumer => p.signed.consumer_signature = signature,
        BondSignerRole::Governance | BondSignerRole::Admission => {
            p.governance_signature = signature
        },
        BondSignerRole::Payout => p.payout_signature = signature,
    }
    tx.data = Some(serde_json::to_string(&p)?);
    tx.set_id();
    Ok(())
}

fn resolve_signer() -> Result<wrfcoin_shared_protocol::crypto::signer::ResolvedSigner> {
    ensure!(
        // secret-leak-ok: presence check over variable names only; no credential value is read or formatted
        super::ceremony::pin_env_detected().is_none(),
        "PIN environment refused; use the hidden prompt"
    );
    let env = SignerEnv {
        hex_key_var: "WRFCOIN_NODE_BOND_SIGNER_PRIVATE_KEY",
        key_file_var: Some("WRFCOIN_NODE_BOND_SIGNER_KEY_FILE"),
        hsm_key_label_var: Some("WRFCOIN_HSM_NODE_BOND_KEY_LABEL"),
        hsm_pubkey_label_var: Some("WRFCOIN_HSM_NODE_BOND_PUBKEY_LABEL"),
        hsm_shared_fallback: SharedKeyFallback::Forbidden,
    };
    #[cfg(feature = "hsm")]
    if std::env::var_os(wrfcoin_shared_protocol::crypto::signer::env_vars::HSM_MODULE_PATH)
        .is_some()
    {
        let pin = dialoguer::Password::new()
            .with_prompt("Dedicated node bond signer PIN")
            .interact()?;
        return wrfcoin_shared_protocol::crypto::signer::load_block_signer_for_with_pin(
            env,
            zeroize::Zeroizing::new(pin),
        );
    }
    load_block_signer_for(env)?
        .ok_or_else(|| anyhow::anyhow!("dedicated Node bond signer unavailable"))
}

/// The packet a submit command sends, refused unless its lifecycle action is
/// the one the command names (Deposit, Update, Exit or Withdraw).
pub(crate) fn submittable(command: &BondCommands, transaction_file: &Path) -> Result<Transaction> {
    let tx = read_transaction(transaction_file)?;
    let p = NodeBondLifecycle::parse(&tx)
        .ok_or_else(|| anyhow::anyhow!("invalid Node lifecycle transaction"))?;
    let expected = match command {
        BondCommands::RegisterBond { .. } => NodeBondAction::Deposit,
        BondCommands::SubmitUpdate { .. } => NodeBondAction::Update,
        BondCommands::Exit { .. } => NodeBondAction::Exit,
        BondCommands::Withdraw { .. } => NodeBondAction::Withdraw,
        _ => anyhow::bail!("not a submit command"),
    };
    ensure!(
        p.signed.authorization.action == expected,
        "lifecycle action mismatch"
    );
    Ok(tx)
}

pub async fn handle(command: &BondCommands) -> Result<Value> {
    if let BondCommands::GrantRequest { owner_address, api_url, out, submit, key_file, p2p_key_file } = command {
        use wrfcoin_cli::bond_grant;
        if let Some(path) = out {
            ensure!(!path.exists(), "output request already exists; inspect before retry");
        }
        let peer = bond_grant::peer_key(&p2p_key_file.clone().unwrap_or_else(bond_grant::default_peer_key_path))?;
        // A seed file (testnet only, enforced in prepare against the node's
        // reported network) or the dedicated HSM signer.
        let signer: std::sync::Arc<dyn BlockSigner> = if let Some(path) = key_file {
            std::sync::Arc::new(bond_grant::server_key(path)?)
        } else {
            // Exactly the dedicated role/PIN resolution used by bond sign --role server.
            resolve_signer().map_err(|_| anyhow::anyhow!("dedicated Node bond signer unavailable"))?.signer
        };
        let client = bond_grant::client()?;
        let request = bond_grant::prepare(&client, api_url, owner_address, signer.as_ref(), &peer, key_file.is_some()).await?;
        if let Some(path) = out {
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)
                .map_err(|_| anyhow::anyhow!("output request file unavailable"))?;
            file.write_all(&serde_json::to_vec_pretty(&request)?)?;
            file.sync_all()?;
        }
        if let Some(backend) = submit {
            let answer = bond_grant::submit(&client, backend, &request).await?;
            return Ok(json!({"request": request, "faucet": answer}));
        }
        return Ok(serde_json::to_value(request)?);
    }
    if let BondCommands::KeyInfo { key_file } = command {
        let key = OperatorKey::load(key_file)?;
        return Ok(
            json!({"address":key.address().to_string(), "public_key":key.public_key().to_hex()}),
        );
    }
    if let BondCommands::Deposit(args) = command {
        return super::node_deposit::handle(args).await;
    }
    if let BondCommands::Update(args) = command {
        return super::node_bond_update::handle(args).await;
    }
    if let BondCommands::Admit(args) = command {
        return super::node_bond_admit::handle_admit(args).await;
    }
    if let BondCommands::AdmissionRequest(args) = command {
        return super::node_bond_admit::handle_request(args).await;
    }
    if let BondCommands::Probe { addr } = command {
        return super::snapshot::probe_info(addr).await;
    }
    if let BondCommands::PayoutChallenge { transaction_file, chain_id, network_id } = command {
        let tx = read_transaction(transaction_file)?;
        let p = NodeBondLifecycle::parse(&tx)
            .ok_or_else(|| anyhow::anyhow!("invalid Node lifecycle transaction"))?;
        let a = &p.signed.authorization;
        ensure!(
            matches!(a.action, NodeBondAction::Deposit | NodeBondAction::Update),
            "only Deposit/Update carry a payout approval"
        );
        return Ok(payout_challenge_json(a, &ChainIdentity::new(chain_id, network_id)));
    }
    if let BondCommands::AttachPayoutPasskey {
        transaction_file,
        payout_passkey_assertion,
        chain_id,
        network_id,
        out,
    } = command
    {
        let mut tx = read_transaction(transaction_file)?;
        ensure!(!out.exists(), "output packet already exists; inspect before retry");
        attach_payout_passkey(&mut tx, payout_passkey_assertion, &ChainIdentity::new(chain_id, network_id))?;
        let bytes = serde_json::to_vec_pretty(&tx)?;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(out)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        return Ok(json!({"attached":true,"included":false,"output":out.display().to_string()}));
    }
    if let BondCommands::Sign {
        transaction_file,
        key_file,
        role,
        chain_id,
        network_id,
        out,
    } = command
    {
        let mut tx = read_transaction(transaction_file)?;
        ensure!(
            !out.exists(),
            "output packet already exists; inspect before retry"
        );
        let identity = ChainIdentity::new(chain_id, network_id);
        if let Some(path) = key_file {
            let operator_role = match role {
                BondSignerRole::Owner => OperatorRole::Owner,
                BondSignerRole::Server => OperatorRole::Server,
                BondSignerRole::Consumer => OperatorRole::Consumer,
                _ => anyhow::bail!(
                    "software operator signer role refused; use dedicated HSM for governance/payout"
                ),
            };
            let key = OperatorKey::load(path)?;
            let mut payload = NodeBondLifecycle::parse(&tx)
                .ok_or_else(|| anyhow::anyhow!("invalid Node lifecycle transaction"))?;
            let public: [u8; 32] = key.public_key().as_bytes().try_into()?;
            ensure!(
                payload.governance_key != Some(public),
                "software operator key cannot double as governance key"
            );
            if matches!(role, BondSignerRole::Owner) {
                ensure!(
                    key.public_key().as_bytes() == payload.owner_key,
                    "owner public key mismatch"
                );
            }
            let signature = key
                .sign_lifecycle(operator_role, &payload.signed.authorization, &identity)?
                .as_bytes()
                .to_vec();
            match role {
                BondSignerRole::Owner => {
                    payload.signed.owner_signature = signature;
                },
                BondSignerRole::Server => payload.signed.server_signature = signature,
                BondSignerRole::Consumer => payload.signed.consumer_signature = signature,
                _ => unreachable!(),
            }
            tx.data = Some(serde_json::to_string(&payload)?);
            tx.set_id();
        } else {
            let signer = resolve_signer()?;
            sign_packet(&mut tx, *role, &identity, signer.signer.as_ref())?;
        }
        let bytes = serde_json::to_vec_pretty(&tx)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        return Ok(json!({"signed":true,"included":false,"output":out.display().to_string()}));
    }
    if let BondCommands::RegisterBond { transaction_file }
    | BondCommands::SubmitUpdate { transaction_file }
    | BondCommands::Exit { transaction_file }
    | BondCommands::Withdraw { transaction_file } = command
    {
        // Refuse a packet of the wrong action before any client is built.
        submittable(command, transaction_file)?;
    }
    let api = ApiClient::new(ApiConfig::default())?;
    match command {
        BondCommands::RegisterBond { transaction_file }
        | BondCommands::SubmitUpdate { transaction_file }
        | BondCommands::Exit { transaction_file }
        | BondCommands::Withdraw { transaction_file } => {
            let tx = submittable(command, transaction_file)?;
            Ok(api
                .post("/node/bonds/submit", Some(serde_json::to_value(tx)?))
                .await?)
        },
        BondCommands::Inspect { deployment_id } => Ok(api
            .get(&format!("/node/bonds/{}", public_id(deployment_id)?), None)
            .await?),
        BondCommands::Epoch { epoch } => {
            Ok(api.get(&format!("/node/settlement/{epoch}"), None).await?)
        },
        BondCommands::Receipt { receipt_id } => Ok(api
            .get(&format!("/node/receipts/{}", public_id(receipt_id)?), None)
            .await?),
        BondCommands::Sign { .. }
        | BondCommands::Deposit(_)
        | BondCommands::Update(_)
        | BondCommands::Admit(_)
        | BondCommands::AdmissionRequest(_)
        | BondCommands::Probe { .. }
        | BondCommands::KeyInfo { .. }
        | BondCommands::GrantRequest { .. }
        | BondCommands::PayoutChallenge { .. }
        | BondCommands::AttachPayoutPasskey { .. } => {
            unreachable!()
        },
    }
}

#[cfg(test)]
#[path = "../../../../../shared-protocol/tests/common/node_bond_fixture.rs"]
pub(crate) mod fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::{
        blockchain::transaction::{TransactionInput, TransactionOutput, TransactionType},
        crypto::signer::EnvKeySigner,
    };

    fn packet(p: &NodeBondLifecycle) -> Transaction {
        let a = &p.signed.authorization;
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: wrfcoin_shared_protocol::crypto::hash::hash_bytes(b"synthetic-s6-cli-input"),
                vout: 0,
                signature: None,
                public_key: Some(hex::encode(p.owner_key)),
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                address: a.owner_account.to_string(),
                amount: 0,
                script: "OP_TRUE".into(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::NodeBondLifecycle;
        tx.sender_address = a.owner_account;
        tx.nonce = a.expected_nonce;
        tx.fee = 1_000_000;
        tx.data = Some(serde_json::to_string(p).unwrap());
        tx.set_id();
        tx
    }
    #[test]
    fn node_bond_real_signer_authorizes_each_role_and_refuses_wrong_keys() {
        let identity = ChainIdentity::new("777", "testnet");
        let mut p = fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1);
        p.signed.owner_signature.clear();
        p.signed.server_signature.clear();
        p.signed.consumer_signature.clear();
        p.governance_signature.clear();
        p.payout_signature.clear();
        let mut tx = packet(&p);
        for (role, byte) in [
            (BondSignerRole::Owner, 10),
            (BondSignerRole::Server, 11),
            (BondSignerRole::Consumer, 12),
            (BondSignerRole::Governance, 99),
            (BondSignerRole::Payout, 10),
        ] {
            let wrong = EnvKeySigner::from_private_key(fixture::key(80), "synthetic-wrong");
            let unchanged = tx.clone();
            assert!(sign_packet(&mut tx, role, &identity, &wrong).is_err());
            assert_eq!(
                serde_json::to_vec(&tx).unwrap(),
                serde_json::to_vec(&unchanged).unwrap()
            );
            let signer = EnvKeySigner::from_private_key(fixture::key(byte), "synthetic-cli");
            sign_packet(&mut tx, role, &identity, &signer).unwrap();
        }
        let signed = NodeBondLifecycle::parse(&tx).unwrap();
        signed
            .verify(&identity, &[7; 32], &[hex::encode(fixture::pk(99))], 0)
            .unwrap();
        assert!(
            signed
                .verify(
                    &ChainIdentity::new("1", "mainnet"),
                    &[7; 32],
                    &[hex::encode(fixture::pk(99))],
                    0
                )
                .is_err()
        );
        assert!(
            signed
                .verify(&identity, &[8; 32], &[hex::encode(fixture::pk(99))], 0)
                .is_err()
        );
    }
    #[test]
    fn node_bond_online_signer_cannot_sign_exit_or_withdrawal() {
        for action in [NodeBondAction::Exit, NodeBondAction::Withdraw] {
            let mut tx = packet(&fixture::auth(10, 1, 1, action, 2));
            let signer = EnvKeySigner::from_private_key(fixture::key(11), "synthetic-service");
            assert!(
                sign_packet(
                    &mut tx,
                    BondSignerRole::Server,
                    &ChainIdentity::new("777", "testnet"),
                    &signer
                )
                .is_err()
            );
            let owner = EnvKeySigner::from_private_key(fixture::key(10), "synthetic-owner");
            sign_packet(
                &mut tx,
                BondSignerRole::Owner,
                &ChainIdentity::new("777", "testnet"),
                &owner,
            )
            .unwrap();
            NodeBondLifecycle::parse(&tx)
                .unwrap()
                .verify(&ChainIdentity::new("777", "testnet"), &[7; 32], &[], 0)
                .unwrap();
        }
    }
    /// `--role admission` signs a Deposit or Update for a key pinned on the
    /// packet's chain, and nothing else. The packet verifies as an admission
    /// co-sign, not as a governance one.
    #[test]
    fn admission_role_signs_only_online_actions_for_a_pinned_key() {
        use wrfcoin_shared_protocol::governance::schedule_extension::install_bond_admission_pin_for_tests;
        install_bond_admission_pin_for_tests(&hex::encode([7u8; 32]), vec![hex::encode(fixture::pk(60))]);
        let identity = ChainIdentity::new("777", "testnet");
        let admission = EnvKeySigner::from_private_key(fixture::key(60), "synthetic-admission");

        let mut p = fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1);
        p.governance_key = Some(fixture::pk(60));
        p.governance_signature.clear();
        let mut tx = packet(&p);
        sign_packet(&mut tx, BondSignerRole::Admission, &identity, &admission).unwrap();
        let signed = NodeBondLifecycle::parse(&tx).unwrap();
        let cosigner = [hex::encode(fixture::pk(60))];
        signed.verify_with_gates(&identity, &[7; 32], &[], &cosigner, false).unwrap();
        assert!(signed.verify_with_gates(&identity, &[7; 32], &[], &[], false).is_err());

        // A key the packet names but this binary does not pin for the chain.
        let unpinned = EnvKeySigner::from_private_key(fixture::key(61), "synthetic-unpinned");
        let mut p = fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1);
        p.governance_key = Some(fixture::pk(61));
        let mut tx = packet(&p);
        assert!(sign_packet(&mut tx, BondSignerRole::Admission, &identity, &unpinned).is_err());

        // The right key on another chain's packet.
        let mut p = fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1);
        p.signed.authorization.genesis_hash = [8; 32];
        p.governance_key = Some(fixture::pk(60));
        let mut tx = packet(&p);
        assert!(sign_packet(&mut tx, BondSignerRole::Admission, &identity, &admission).is_err());

        // Exit and Withdraw take no co-sign of any kind.
        for action in [NodeBondAction::Exit, NodeBondAction::Withdraw] {
            let mut p = fixture::auth(10, 1, 1, action, 2);
            p.governance_key = Some(fixture::pk(60));
            let mut tx = packet(&p);
            assert!(
                sign_packet(&mut tx, BondSignerRole::Admission, &identity, &admission).is_err(),
                "{action:?}"
            );
        }
    }

    #[test]
    fn passkey_payout_challenge_attach_and_offline_verify() {
        use wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator;
        let identity = ChainIdentity::new("777", "testnet");
        let wallet = SoftwareAuthenticator::new(&[0x3c; 32], "wrfcoin.com");
        let mut p = fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1);
        p.signed.authorization.payout_account = wallet.account();
        fixture::resign(&mut p, 10);
        p.payout_key = None;
        p.payout_signature.clear();
        let mut tx = packet(&p);

        let shown = payout_challenge_json(&p.signed.authorization, &identity);
        let challenge = node_bond_payout_passkey_challenge(&p.signed.authorization, &identity);
        assert_eq!(shown["challenge_hex"], hex::encode(challenge));
        assert_eq!(shown["payout_account"], wallet.account().to_hex());

        let dir = std::env::temp_dir().join(format!("wrfcoin-passkey-payout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A wrong wallet's assertion is refused before anything is written.
        let other = SoftwareAuthenticator::new(&[0x4d; 32], "wrfcoin.com");
        let bad = dir.join("bad.json");
        std::fs::write(&bad, serde_json::to_vec(&other.authorization_for(&other.assert(&challenge))).unwrap()).unwrap();
        let before = tx.clone();
        assert!(attach_payout_passkey(&mut tx, &bad, &identity).is_err());
        assert_eq!(tx.data, before.data);
        // The right wallet, wrapped the way the approval page may emit it.
        let good = dir.join("good.json");
        let approval = wallet.authorization_for(&wallet.assert(&challenge));
        std::fs::write(&good, serde_json::to_vec(&json!({"payout_passkey": approval})).unwrap()).unwrap();
        attach_payout_passkey(&mut tx, &good, &identity).unwrap();
        let attached = NodeBondLifecycle::parse(&tx).unwrap();
        assert_eq!(attached.payout_passkey.as_ref(), Some(&approval));
        let keys = [hex::encode(fixture::pk(99))];
        attached.verify_with_passkey_payout(&identity, &[7; 32], &keys, true).unwrap();
        // A binary without the feature (or below activation) refuses it.
        assert!(attached.verify_with_passkey_payout(&identity, &[7; 32], &keys, false).is_err());
        // An ed25519-payout packet cannot also take a passkey approval.
        let mut ed = packet(&fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1));
        assert!(attach_payout_passkey(&mut ed, &good, &identity).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn public_identifier_cannot_inject_a_route() {
        assert!(public_id("../receipts/submit").is_err());
        assert!(public_id(&"g".repeat(64)).is_err());
        assert_eq!(public_id(&"AB".repeat(32)).unwrap(), "ab".repeat(32));
    }
}
