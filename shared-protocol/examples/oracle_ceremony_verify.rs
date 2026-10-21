//! Fail-closed verifier for the final block-1 oracle ceremony bundle.
//!
//! N, K, and canonical transaction IDs come from the embedded #2530 artifact.
//! Every one of N operator nodes must prove possession of its distinct
//! registered signing key even when the runtime reward quorum K is smaller.
//!
//! Usage:
//!   WRFCOIN_CHAIN_ID=777 \
//!   WRFCOIN_NETWORK_ID=testnet \
//!   WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=<comma-separated-public-keys> \
//!   cargo run -q -p wrfcoin-shared-protocol --example oracle_ceremony_verify \
//!     -- ceremony-bundle.json

use std::{
    env,
    fs,
};

use wrfcoin_shared_protocol::crypto::domain_signing::{
    oracle_governance_pubkeys,
    signing_chain_id,
    signing_network_id,
};
use wrfcoin_shared_protocol::oracle::ceremony::{
    GenesisOraclePossessionBundle,
    verify_oracle_possession,
};

fn main() {
    let args = env::args().collect::<Vec<_>>();
    // #2721: BEFORE the argument-count check. `--version` previously satisfied
    // `args.len() == 2` and reached the read below, so the binary PANICKED
    // (exit 101) trying to open a file literally named `--version` — measured
    // on df1c6fabd. An operator asking a ceremony binary which commit it came
    // from got a stack trace.
    if wrfcoin_build_provenance::print_version_if_requested(
        "oracle_ceremony_verify",
        env!("CARGO_PKG_VERSION"),
        &args,
    ) {
        return;
    }
    if args.len() != 2 {
        eprintln!("usage: oracle_ceremony_verify <ceremony-bundle.json>");
        eprintln!("       oracle_ceremony_verify --version");
        std::process::exit(2);
    }

    let bytes = fs::read(&args[1]).expect("read ceremony bundle");
    let bundle = serde_json::from_slice::<GenesisOraclePossessionBundle>(&bytes)
        .expect("parse ceremony bundle");
    let report = verify_oracle_possession(
        &bundle,
        &signing_chain_id(),
        &signing_network_id(),
        &oracle_governance_pubkeys(),
    )
    .expect("ORACLE CEREMONY REFUSED");

    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("serialize ceremony verification report")
    );
}
