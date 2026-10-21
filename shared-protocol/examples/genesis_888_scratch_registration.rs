//! Local rehearsal helper. Input is a newly generated scratch-only signing key.
use wrfcoin_shared_protocol::{blockchain::transaction::Transaction, crypto::signature::{PrivateKey, PublicKey}};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 3, "synthetic key file and synthetic oracle public key required");
    let key = PrivateKey::from_hex(std::fs::read_to_string(&args[1]).unwrap().trim()).unwrap();
    let _public = PublicKey::from_private_key(&key);
    let oracle = &args[2];
    let tx = Transaction::new_oracle_registration_with_domain("888", "testnet", vec![oracle.clone()], 1,
        oracle, oracle, 1, chrono::Utc::now().timestamp(), &key);
    println!("{}", serde_json::json!({"chain_id":"888","network_id":"testnet","transactions":[tx]}));
}
