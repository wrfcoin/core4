//! Public CLI subprocess proof: real loader, synthetic off-host owner file.
#[path = "../../../../shared-protocol/tests/common/node_bond_fixture.rs"]
mod fixture;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};
use wrfcoin_shared_protocol::{
    blockchain::transaction::{Transaction, TransactionInput, TransactionOutput, TransactionType},
    consensus::{node_bond_registry::NodeBondLifecycle, node_share_contracts::NodeBondAction},
    crypto::chain_binding::ChainIdentity,
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("node-s6-cli-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn packet() -> Transaction {
    let mut p = fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1);
    p.signed.owner_signature.clear();
    let a = &p.signed.authorization;
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: wrfcoin_shared_protocol::crypto::hash::hash_bytes(b"synthetic-cli-process-input"),
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
    tx.data = Some(serde_json::to_string(&p).unwrap());
    tx.set_id();
    tx
}
fn run(home: &Path, key: Option<&Path>, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wrfcoin"));
    cmd.env_clear()
        .env("HOME", home)
        .env("WRFCOIN_CHAIN_ID", "777")
        .env("WRFCOIN_NETWORK_ID", "testnet")
        .args(args);
    if let Some(path) = key {
        cmd.env("WRFCOIN_NODE_BOND_SIGNER_KEY_FILE", path);
    }
    cmd.output().unwrap()
}
#[test]
fn public_cli_signs_with_real_key_file_and_preserves_existing_outputs() {
    let dir = Scratch::new();
    let input = dir.0.join("packet.json");
    let key = dir.0.join("synthetic-owner.key");
    let out = dir.0.join("signed.json");
    std::fs::write(&input, serde_json::to_vec(&packet()).unwrap()).unwrap();
    std::fs::write(&key, hex::encode(fixture::key(10).as_bytes())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let args = [
        "node",
        "bond",
        "sign",
        "--transaction-file",
        input.to_str().unwrap(),
        "--role",
        "owner",
        "--chain-id",
        "777",
        "--network-id",
        "testnet",
        "--out",
        out.to_str().unwrap(),
    ];
    let unavailable = run(&dir.0, None, &args);
    assert!(!unavailable.status.success());
    assert!(!out.exists());
    let result = run(&dir.0, Some(&key), &args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let signed = std::fs::read(&out).unwrap();
    let tx: Transaction = serde_json::from_slice(&signed).unwrap();
    NodeBondLifecycle::parse(&tx)
        .unwrap()
        .verify(
            &ChainIdentity::new("777", "testnet"),
            &[7; 32],
            &[hex::encode(fixture::pk(99))],
            0,
        )
        .unwrap();
    let again = run(&dir.0, Some(&key), &args);
    assert!(!again.status.success());
    assert_eq!(std::fs::read(&out).unwrap(), signed);
    // Synthetic secret bytes must not be emitted even by errors.
    let synthetic = hex::encode(fixture::key(10).as_bytes());
    for output in [unavailable, result, again] {
        assert!(!String::from_utf8_lossy(&output.stdout).contains(&synthetic));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(&synthetic));
    }
}
#[test]
fn public_cli_exposes_lifecycle_and_receipt_inspection() {
    let dir = Scratch::new();
    let result = run(&dir.0, None, &["node", "bond", "--help"]);
    assert!(result.status.success());
    let help = String::from_utf8_lossy(&result.stdout);
    for command in [
        "register-bond",
        "exit",
        "withdraw",
        "inspect",
        "epoch",
        "receipt",
        "sign",
    ] {
        assert!(help.contains(command), "missing {command}");
    }
}
