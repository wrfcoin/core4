//! Generate a WRFCoin oracle-operator keypair.
//!
//! Each oracle operator runs this once to obtain their identity. The public
//! key hex IS the on-chain `oracle_id`. On a LIVE chain it is registered via
//! a governance-authorized `OracleRegistration` transaction (#2055 Option A —
//! see `docs/launch/oracle-bootstrap-concrete.md` Phase 3); the admin route
//! `POST /transactions/oracle/operators` works only at height 0 (genesis
//! provisioning) and is rejected on a live chain (guard #1856). The private
//! key stays on the operator's machine and signs attestations (see
//! `oracle_attest_sign`).
//!
//! Usage:
//!   cargo run -q -p wrfcoin-shared-protocol --example oracle_keygen
//!
//! ORACLE_ID / ORACLE_ADDRESS go to stdout; keep ORACLE_PRIVKEY secret.

use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

fn main() {
    let sk = PrivateKey::generate(&mut rand::thread_rng());
    let pk = PublicKey::from_private_key(&sk);
    eprintln!("# WRFCoin oracle keypair — ORACLE_PRIVKEY is a SECRET, do not share or commit it.");
    println!("ORACLE_ID={}", pk.to_hex()); // register this as oracle_id + public_key
    println!("ORACLE_ADDRESS={}", pk.to_address()); // wrf-prefixed (informational)
    println!("ORACLE_PRIVKEY={}", sk.to_hex()); // feed to oracle_attest_sign
}
