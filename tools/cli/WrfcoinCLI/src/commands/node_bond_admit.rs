//! Bond admission co-sign (`bond_admission_key`), split between the two ends.
//!
//! - **Service end:** `node bond admit` checks a Deposit/Update packet that
//!   names a pinned admission key as its co-signer and already carries valid
//!   server and consumer possession signatures, and prints a public summary
//!   for the admission service's policy. With `--key-file` and `--out` it then
//!   co-signs. A software key file is accepted on `testnet` only; elsewhere the
//!   dedicated HSM path (`node bond sign --role admission`) stays the only one.
//! - **Node end:** `node bond admission-request` binds the node's peer id and
//!   announced P2P address to the packet with a peer-identity signature, the
//!   same possession proof the faucet bond grant asks for.
//! - `node bond probe` dials a P2P address and asks the keyless bonded-fetch
//!   `Info`, so the service can prove the node is reachable from outside.
//!
//! None of these submit anything or read a feature schedule. The chain still
//! refuses an admission co-sign until `bond_admission_key` is active.
use anyhow::{Result, ensure};
use clap::Args;
use serde_json::{Value, json};
use std::{io::Write, path::PathBuf};
use wrfcoin_shared_protocol::{
    blockchain::transaction::Transaction,
    common::types::Address,
    consensus::{node_bond_registry::NodeBondLifecycle, node_share_contracts::*},
    crypto::{
        chain_binding::{ChainIdentity, SigningProtocolVersion},
        domain_signing::verify_domain_for,
        signature::{PublicKey, Signature},
        signer::BlockSigner,
    },
    governance::schedule_extension::bond_admission_keys,
};

use super::node_bond::{BondSignerRole, read_transaction, sign_packet};

/// Domain of the node's peer-identity signature in an admission request.
pub const ADMISSION_PEER_DOMAIN: &[u8] = b"wrfcoin_bond_admission_peer_v1";
/// The only network on which `admit` accepts a software admission key.
pub const SOFTWARE_ADMISSION_NETWORK: &str = "testnet";

#[derive(Args)]
pub struct AdmitArgs {
    /// Deposit or Update packet assembled with `--admission-cosign`, carrying
    /// the server and consumer signatures.
    #[clap(long)]
    pub transaction_file: PathBuf,
    #[clap(long)]
    pub chain_id: String,
    #[clap(long)]
    pub network_id: String,
    /// Owner-only file (Unix mode 0600; Windows: only your account) holding the
    /// admission key's 32-byte ed25519 seed as hex. Testnet only. Without it,
    /// only the check runs.
    #[clap(long, requires = "out")]
    pub key_file: Option<PathBuf>,
    /// Write the co-signed packet to this new file; never overwrite.
    #[clap(long, requires = "key_file")]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct AdmissionRequestArgs {
    /// The packet to send for admission (server and consumer signed).
    #[clap(long)]
    pub transaction_file: PathBuf,
    /// This node's public P2P address, ending in its own peer id
    /// (e.g. /ip4/203.0.113.7/tcp/9000/p2p/12D3Koo…).
    #[clap(long)]
    pub multiaddr: String,
    #[clap(long)]
    pub chain_id: String,
    #[clap(long)]
    pub network_id: String,
    /// Existing protobuf ed25519 identity; default $WRFCOIN_DATA_DIR/peer_identity.key (./data if unset).
    #[clap(long)]
    pub p2p_key_file: Option<PathBuf>,
    /// Write the public request JSON to this new file; never overwrite.
    #[clap(long)]
    pub out: Option<PathBuf>,
}

fn possession_ok(
    a: &NodeBondAuthorization,
    role: NodeBondKeyRole,
    key: &[u8; 32],
    sig: &[u8],
    identity: &ChainIdentity,
) -> bool {
    let (Ok(key), Ok(sig)) = (PublicKey::from_bytes(key), Signature::from_bytes(sig)) else {
        return false;
    };
    let subject = NodeBondPossession {
        authorization_preimage_hash: node_contract_digest(a, identity),
        role,
    };
    verify_domain_for(&key, &subject, &sig, identity, SigningProtocolVersion::V2)
}

/// Check `tx` as an admission request and return its lifecycle and a public
/// summary. Refuses anything the admission key must never co-sign.
pub(crate) fn check(
    tx: &Transaction,
    identity: &ChainIdentity,
) -> Result<(NodeBondLifecycle, Value)> {
    let p = NodeBondLifecycle::parse(tx)
        .ok_or_else(|| anyhow::anyhow!("invalid Node lifecycle transaction"))?;
    let a = &p.signed.authorization;
    ensure!(
        matches!(a.action, NodeBondAction::Deposit | NodeBondAction::Update),
        "an admission key co-signs only a Deposit or an Update"
    );
    let key = p.governance_key.ok_or_else(|| {
        anyhow::anyhow!("packet names no co-signer; assemble with --admission-cosign")
    })?;
    let genesis = hex::encode(a.genesis_hash);
    ensure!(
        bond_admission_keys(&genesis).contains(&hex::encode(key)),
        "co-signer is not a bond admission key this binary pins for the packet's genesis"
    );
    ensure!(
        p.governance_signature.is_empty(),
        "packet already carries a co-sign; inspect it before asking again"
    );
    ensure!(
        PublicKey::from_bytes(&p.owner_key)
            .is_ok_and(|owner| Address::from_public_key(&owner) == a.owner_account),
        "owner key does not match the owner account"
    );
    ensure!(
        !a.service_alias.is_empty() && a.service_alias.len() <= MAX_NODE_ALIAS_BYTES,
        "service alias empty or too long"
    );
    ensure!(
        a.server_key != a.consumer_key
            && p.owner_key != a.server_key
            && p.owner_key != a.consumer_key
            && a.owner_account != a.escrow_account
            && a.payout_account != a.escrow_account,
        "packet key or account bindings refused"
    );
    ensure!(
        key != a.server_key && key != a.consumer_key && key != p.owner_key,
        "admission key cannot double as a bond role key"
    );
    ensure!(
        possession_ok(
            a,
            NodeBondKeyRole::Server,
            &a.server_key,
            &p.signed.server_signature,
            identity
        ),
        "server possession signature missing or invalid; sign --role server first"
    );
    ensure!(
        possession_ok(
            a,
            NodeBondKeyRole::Consumer,
            &a.consumer_key,
            &p.signed.consumer_signature,
            identity
        ),
        "consumer possession signature missing or invalid; sign --role consumer first"
    );
    let summary = json!({
        "action": format!("{:?}", a.action),
        "chain_id": identity.chain_id,
        "network_id": identity.network_id,
        "genesis_hash": genesis,
        "deployment_id": hex::encode(a.deployment_id),
        "owner_account": a.owner_account.to_hex(),
        "escrow_account": a.escrow_account.to_hex(),
        "payout_account": a.payout_account.to_hex(),
        "server_key": hex::encode(a.server_key),
        "consumer_key": hex::encode(a.consumer_key),
        "service_alias": a.service_alias,
        "amount_units": a.amount_units,
        "expected_nonce": a.expected_nonce,
        "effective_epoch": a.effective_epoch,
        "admission_key": hex::encode(key),
        "authorization_hash": hex::encode(node_contract_digest(a, identity)),
    });
    Ok((p, summary))
}

/// The admission request's peer message:
/// `ADMISSION_PEER_DOMAIN || str(chain_id) || str(network_id) ||
/// str(authorization_hash hex) || str(multiaddr)`, where
/// `str(s) = u32LE(byte length) || UTF-8 bytes`. Not hashed before signing.
pub fn admission_peer_message(
    chain_id: &str,
    network_id: &str,
    authorization_hash: &str,
    multiaddr: &str,
) -> Result<Vec<u8>> {
    let mut bytes = ADMISSION_PEER_DOMAIN.to_vec();
    for value in [chain_id, network_id, authorization_hash, multiaddr] {
        let len = u32::try_from(value.len())
            .map_err(|_| anyhow::anyhow!("admission request field exceeds bound"))?;
        bytes.extend_from_slice(&len.to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    Ok(bytes)
}

fn write_new(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| {
            anyhow::anyhow!("output file unavailable or already exists; inspect before retry")
        })?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub async fn handle_admit(args: &AdmitArgs) -> Result<Value> {
    let mut tx = read_transaction(&args.transaction_file)?;
    let identity = ChainIdentity::new(&args.chain_id, &args.network_id);
    let (p, summary) = check(&tx, &identity)?;
    let (Some(key_file), Some(out)) = (&args.key_file, &args.out) else {
        return Ok(json!({"admissible": true, "signed": false, "summary": summary}));
    };
    ensure!(
        identity.network_id == SOFTWARE_ADMISSION_NETWORK,
        "a software admission key is accepted on testnet only; use the dedicated HSM signer"
    );
    ensure!(
        !out.exists(),
        "output packet already exists; inspect before retry"
    );
    let signer = wrfcoin_cli::bond_grant::server_key(key_file)
        .map_err(|_| anyhow::anyhow!("admission key file refused (owner-only 64-hex seed file)"))?;
    let public: [u8; 32] = signer.public_key().as_bytes().try_into()?;
    ensure!(
        p.governance_key == Some(public),
        "admission key file does not match the packet's co-signer"
    );
    sign_packet(&mut tx, BondSignerRole::Admission, &identity, &signer)?;
    // Check the co-sign as the chain will, before anything leaves this host.
    let signed = NodeBondLifecycle::parse(&tx)
        .ok_or_else(|| anyhow::anyhow!("invalid Node lifecycle transaction"))?;
    let sig = Signature::from_bytes(&signed.governance_signature)?;
    let admission_public = signer.public_key();
    ensure!( // secret-leak-ok: verifies a public signature with a public key; the message is a fixed string
        verify_domain_for(
            &admission_public,
            &signed.signed.authorization,
            &sig,
            &identity,
            SigningProtocolVersion::V2
        ),
        "admission co-sign did not verify"
    );
    write_new(out, &serde_json::to_vec_pretty(&tx)?)?;
    Ok(json!({
        "admissible": true,
        "signed": true,
        "included": false,
        "output": out.display().to_string(),
        "summary": summary,
    }))
}

pub async fn handle_request(args: &AdmissionRequestArgs) -> Result<Value> {
    let tx = read_transaction(&args.transaction_file)?;
    let identity = ChainIdentity::new(&args.chain_id, &args.network_id);
    let (_, summary) = check(&tx, &identity)?;
    if let Some(path) = &args.out {
        ensure!(
            !path.exists(),
            "output request already exists; inspect before retry"
        );
    }
    let peer = wrfcoin_cli::bond_grant::peer_key(
        &args
            .p2p_key_file
            .clone()
            .unwrap_or_else(wrfcoin_cli::bond_grant::default_peer_key_path),
    )?;
    let addr: libp2p::Multiaddr = args
        .multiaddr
        .parse()
        .map_err(|_| anyhow::anyhow!("--multiaddr is not a valid P2P multiaddr"))?;
    let peer_id = peer.public().to_peer_id();
    ensure!(
        matches!(addr.iter().last(), Some(libp2p::multiaddr::Protocol::P2p(id)) if id == peer_id),
        "--multiaddr must end in /p2p/ and this node's own peer id"
    );
    let authorization_hash = summary["authorization_hash"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("summary missing authorization hash"))?
        .to_string();
    let multiaddr = addr.to_string();
    let message = admission_peer_message(
        &identity.chain_id,
        &identity.network_id,
        &authorization_hash,
        &multiaddr,
    )?;
    let signature = peer
        .sign(&message)
        .map_err(|_| anyhow::anyhow!("peer signing failed"))?;
    ensure!(
        peer.public().verify(&message, &signature),
        "peer signature verification failed"
    );
    let request = json!({
        "transaction": tx,
        "peerId": peer_id.to_string(),
        "multiaddr": multiaddr,
        "authorizationHash": authorization_hash,
        "peerSignature": hex::encode(signature),
    });
    if let Some(path) = &args.out {
        write_new(path, &serde_json::to_vec_pretty(&request)?)?;
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::super::node_bond::fixture;
    use super::*;
    use wrfcoin_shared_protocol::{
        blockchain::transaction::{TransactionInput, TransactionOutput, TransactionType},
        governance::schedule_extension::install_bond_admission_pin_for_tests,
    };

    // Every test in this binary shares one pin (first install wins), the same
    // one node_bond.rs uses: genesis [7; 32], key [60; 32]. Synthetic only.
    fn pin() {
        install_bond_admission_pin_for_tests(
            &hex::encode([7u8; 32]),
            vec![hex::encode(fixture::pk(60))],
        );
    }
    fn identity() -> ChainIdentity {
        ChainIdentity::new("777", "testnet")
    }
    fn packet(p: &NodeBondLifecycle) -> Transaction {
        let a = &p.signed.authorization;
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: wrfcoin_shared_protocol::crypto::hash::hash_bytes(
                    b"synthetic-admission-input",
                ),
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
    /// A Deposit (or Update) the admission key is asked to co-sign.
    fn request(action: NodeBondAction) -> NodeBondLifecycle {
        let nonce = if action == NodeBondAction::Deposit {
            0
        } else {
            1
        };
        let mut p = fixture::auth(10, 1, nonce, action, 1);
        p.governance_key = Some(fixture::pk(60));
        p.governance_signature.clear();
        p
    }
    fn dir(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("wrfcoin-bond-admit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn secret(path: PathBuf, bytes: &[u8], mode: u32) -> PathBuf {
        std::fs::write(&path, bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        #[cfg(not(unix))]
        let _ = mode;
        path
    }
    fn write_packet(dir: &std::path::Path, name: &str, p: &NodeBondLifecycle) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, serde_json::to_vec(&packet(p)).unwrap()).unwrap();
        path
    }

    #[test]
    fn check_reports_a_pinned_deposit_and_update() {
        pin();
        for action in [NodeBondAction::Deposit, NodeBondAction::Update] {
            let p = request(action);
            let (_, summary) = check(&packet(&p), &identity()).unwrap();
            assert_eq!(summary["action"], format!("{action:?}"));
            assert_eq!(summary["admission_key"], hex::encode(fixture::pk(60)));
            assert_eq!(summary["server_key"], hex::encode(fixture::pk(11)));
            assert_eq!(
                summary["authorization_hash"],
                hex::encode(node_contract_digest(&p.signed.authorization, &identity()))
            );
        }
    }

    #[test]
    fn check_refuses_what_the_admission_key_must_never_cosign() {
        pin();
        let id = identity();
        let refused = |p: &NodeBondLifecycle, needle: &str| {
            let error = check(&packet(p), &id).unwrap_err().to_string();
            assert!(error.contains(needle), "{needle}: {error}");
        };
        for action in [NodeBondAction::Exit, NodeBondAction::Withdraw] {
            let mut p = fixture::auth(10, 1, 1, action, 2);
            p.governance_key = Some(fixture::pk(60));
            refused(&p, "only a Deposit or an Update");
        }
        let mut p = request(NodeBondAction::Deposit);
        p.governance_key = None;
        refused(&p, "names no co-signer");

        let mut p = request(NodeBondAction::Deposit);
        p.governance_key = Some(fixture::pk(61));
        refused(&p, "not a bond admission key");

        // The genesis governance key is not an admission key.
        let mut p = request(NodeBondAction::Deposit);
        p.governance_key = Some(fixture::pk(99));
        refused(&p, "not a bond admission key");

        // Another chain's packet (no pin for that genesis).
        let mut p = request(NodeBondAction::Deposit);
        p.signed.authorization.genesis_hash = [8; 32];
        refused(&p, "not a bond admission key");

        let mut p = request(NodeBondAction::Deposit);
        p.governance_signature = vec![1; 64];
        refused(&p, "already carries a co-sign");

        let mut p = request(NodeBondAction::Deposit);
        p.signed.server_signature.clear();
        refused(&p, "server possession");

        let mut p = request(NodeBondAction::Deposit);
        p.signed.consumer_signature[0] ^= 1;
        refused(&p, "consumer possession");

        // Possession signed for another chain identity.
        let p = request(NodeBondAction::Deposit);
        assert!(check(&packet(&p), &ChainIdentity::new("888", "testnet")).is_err());

        // The admission key named as the bond's own server key.
        let mut p = request(NodeBondAction::Deposit);
        p.signed.authorization.server_key = fixture::pk(60);
        refused(&p, "");
    }

    #[tokio::test]
    async fn admit_signs_with_a_testnet_key_file_and_the_chain_accepts_the_cosign() {
        pin();
        let d = dir("sign");
        let key = secret(
            d.join("admission.seed"),
            hex::encode([60u8; 32]).as_bytes(),
            0o600,
        );
        let input = write_packet(&d, "unsigned.json", &request(NodeBondAction::Deposit));
        let out = d.join("admitted.json");
        let args = AdmitArgs {
            transaction_file: input.clone(),
            chain_id: "777".into(),
            network_id: "testnet".into(),
            key_file: Some(key.clone()),
            out: Some(out.clone()),
        };
        let answer = handle_admit(&args).await.unwrap();
        assert_eq!(answer["signed"], true);
        let tx: Transaction = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        let signed = NodeBondLifecycle::parse(&tx).unwrap();
        let cosigner = [hex::encode(fixture::pk(60))];
        // The fixture already carries owner and payout approvals, so the
        // whole packet verifies once the admission key is in force, and not
        // as a governance co-sign.
        signed
            .verify_with_gates(&identity(), &[7; 32], &[], &cosigner, false)
            .unwrap();
        assert!(
            signed
                .verify_with_gates(&identity(), &[7; 32], &[], &[], false)
                .is_err()
        );

        // Never overwrites.
        let error = handle_admit(&args).await.unwrap_err().to_string();
        assert!(error.contains("already exists"), "{error}");

        // The check alone needs no key.
        let dry = AdmitArgs {
            key_file: None,
            out: None,
            ..args
        };
        assert_eq!(handle_admit(&dry).await.unwrap()["signed"], false);
    }

    #[tokio::test]
    async fn admit_refuses_a_software_key_off_testnet_a_wrong_key_and_a_loose_file() {
        pin();
        let d = dir("refuse");
        let input = write_packet(&d, "unsigned.json", &request(NodeBondAction::Update));
        let good = secret(
            d.join("good.seed"),
            hex::encode([60u8; 32]).as_bytes(),
            0o600,
        );
        let wrong = secret(
            d.join("wrong.seed"),
            hex::encode([61u8; 32]).as_bytes(),
            0o600,
        );
        let args = |key: &PathBuf, network: &str, out: &str| AdmitArgs {
            transaction_file: input.clone(),
            chain_id: "777".into(),
            network_id: network.into(),
            key_file: Some(key.clone()),
            out: Some(d.join(out)),
        };
        // This fixture's possession signatures are for testnet, so a mainnet
        // label fails the check; the network rule alone is the next test.
        assert!(
            handle_admit(&args(&good, "mainnet", "a.json"))
                .await
                .is_err()
        );
        assert!(!d.join("a.json").exists());

        let error = handle_admit(&args(&wrong, "testnet", "b.json"))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("does not match"), "{error}");
        assert!(!d.join("b.json").exists());

        #[cfg(unix)]
        {
            let loose = secret(
                d.join("loose.seed"),
                hex::encode([60u8; 32]).as_bytes(),
                0o644,
            );
            let error = handle_admit(&args(&loose, "testnet", "c.json"))
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains("admission key file refused"), "{error}");
            assert!(!d.join("c.json").exists());
        }
    }

    /// The network rule on its own: a packet whose possession signatures are
    /// valid for a non-testnet identity is still refused a software co-sign.
    #[tokio::test]
    async fn admit_software_key_network_rule() {
        pin();
        let d = dir("network");
        let mainnet = ChainIdentity::new("1", "mainnet");
        let mut p = request(NodeBondAction::Deposit);
        let digest = node_contract_digest(&p.signed.authorization, &mainnet);
        for (role, byte) in [
            (NodeBondKeyRole::Server, 11u8),
            (NodeBondKeyRole::Consumer, 12u8),
        ] {
            let sig = wrfcoin_shared_protocol::crypto::domain_signing::sign_domain_for(
                &fixture::key(byte),
                &NodeBondPossession {
                    authorization_preimage_hash: digest,
                    role,
                },
                &mainnet,
                SigningProtocolVersion::V2,
            )
            .as_bytes()
            .to_vec();
            match role {
                NodeBondKeyRole::Server => p.signed.server_signature = sig,
                NodeBondKeyRole::Consumer => p.signed.consumer_signature = sig,
            }
        }
        check(&packet(&p), &mainnet).unwrap();
        let input = write_packet(&d, "mainnet.json", &p);
        let key = secret(d.join("k.seed"), hex::encode([60u8; 32]).as_bytes(), 0o600);
        let error = handle_admit(&AdmitArgs {
            transaction_file: input,
            chain_id: "1".into(),
            network_id: "mainnet".into(),
            key_file: Some(key),
            out: Some(d.join("o.json")),
        })
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("testnet only"), "{error}");
        assert!(!d.join("o.json").exists());
    }

    #[test]
    fn admission_peer_message_layout_is_frozen() {
        // infra testnet/services/bond-admission tests carry the same vector.
        let message = admission_peer_message(
            "888",
            "testnet",
            &"ab".repeat(32),
            "/ip4/203.0.113.7/tcp/9000",
        )
        .unwrap();
        let mut expected = b"wrfcoin_bond_admission_peer_v1".to_vec();
        for value in [
            "888",
            "testnet",
            &"ab".repeat(32),
            "/ip4/203.0.113.7/tcp/9000",
        ] {
            expected.extend_from_slice(&(value.len() as u32).to_le_bytes());
            expected.extend_from_slice(value.as_bytes());
        }
        assert_eq!(message, expected);

        // Cross-implementation vector: the libp2p identity from seed [2; 32]
        // signing the message for its own address. infra's bond-admission
        // tests verify the same bytes with node:crypto.
        let peer = libp2p_identity::Keypair::ed25519_from_bytes([2u8; 32]).unwrap();
        let peer_id = peer.public().to_peer_id().to_string();
        assert_eq!(
            peer_id,
            "12D3KooWJWoaqZhDaoEFshF7Rh1bpY9ohihFhzcW6d69Lr2NASuq" // pragma: allowlist secret -- public peer id of a synthetic test seed
        );
        let addr = format!("/ip4/203.0.113.7/tcp/9000/p2p/{peer_id}");
        let message = admission_peer_message("888", "testnet", &"ab".repeat(32), &addr).unwrap();
        assert_eq!(
            hex::encode(peer.sign(&message).unwrap()),
            "07fb6838d507d91edb0a9ab6f4b5df81b9be5f26d67d32bbda883cb1ac4f49ebbc1290f4ef89e34017ae85d6cdc26a566ccb406a6461af8684d560bab994f308" // pragma: allowlist secret -- public signature, synthetic test vector
        );
    }

    #[tokio::test]
    async fn admission_request_binds_this_peer_and_its_address() {
        pin();
        let d = dir("request");
        let input = write_packet(&d, "unsigned.json", &request(NodeBondAction::Deposit));
        let peer = libp2p_identity::Keypair::ed25519_from_bytes([2u8; 32]).unwrap();
        let peer_file = secret(
            d.join("peer_identity.key"),
            &peer.to_protobuf_encoding().unwrap(),
            0o600,
        );
        let peer_id = peer.public().to_peer_id();
        let base = |multiaddr: String, out: Option<PathBuf>| AdmissionRequestArgs {
            transaction_file: input.clone(),
            multiaddr,
            chain_id: "777".into(),
            network_id: "testnet".into(),
            p2p_key_file: Some(peer_file.clone()),
            out,
        };
        let addr = format!("/ip4/203.0.113.7/tcp/9000/p2p/{peer_id}");
        let out = d.join("request.json");
        let answer = handle_request(&base(addr.clone(), Some(out.clone())))
            .await
            .unwrap();
        assert_eq!(answer["peerId"], peer_id.to_string());
        assert_eq!(answer["multiaddr"], addr);
        let hash = answer["authorizationHash"].as_str().unwrap();
        let message = admission_peer_message("777", "testnet", hash, &addr).unwrap();
        let sig = hex::decode(answer["peerSignature"].as_str().unwrap()).unwrap();
        assert!(peer.public().verify(&message, &sig));
        assert!(out.exists());

        // Another node's peer id in the address, or none at all, is refused.
        let other = libp2p_identity::Keypair::ed25519_from_bytes([3u8; 32])
            .unwrap()
            .public()
            .to_peer_id();
        let error = handle_request(&base(
            format!("/ip4/203.0.113.7/tcp/9000/p2p/{other}"),
            None,
        ))
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("own peer id"), "{error}");
        assert!(
            handle_request(&base("/ip4/203.0.113.7/tcp/9000".into(), None))
                .await
                .is_err()
        );
        // A packet the service would refuse is refused here first.
        let mut p = request(NodeBondAction::Deposit);
        p.governance_key = Some(fixture::pk(61));
        let bad = write_packet(&d, "unpinned.json", &p);
        let args = AdmissionRequestArgs {
            transaction_file: bad,
            ..base(addr, None)
        };
        assert!(handle_request(&args).await.is_err());
    }
}
