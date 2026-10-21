//! Produce one live operator-key possession proof for the final genesis oracle
//! ceremony bundle.
//!
//! The local key comes from the production oracle-signer loader, so a ceremony
//! uses the same key-file/HSM custody and signing implementation as runtime
//! attestations. No secret is accepted in argv or written to output.
//!
//! Debug/key-file usage:
//!   WRFCOIN_CHAIN_ID=777 \
//!   WRFCOIN_NETWORK_ID=testnet \
//!   WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS=<governance-public-key> \
//!   WRFCOIN_ORACLE_SIGNER_KEY_FILE=/secure/oracle-key.hex \
//!   cargo run -q -p wrfcoin-shared-protocol --example oracle_ceremony_sign \
//!     -- ceremony-bundle.json <timestamp>
//!
//! Production/HSM usage adds `--features hsm` and the oracle-specific PKCS#11
//! variables documented by `SignerEnv::oracle`.
//!
//! The input bundle embeds the final #2530 genesis artifact and may contain
//! zero or more possession proofs. The tool derives the local oracle identity
//! by matching the resolved signer's public key to exactly one artifact
//! registration, then emits one proof JSON object for the coordinator to append
//! to `possession_proofs`.

use std::{
    env,
    fs,
};

use wrfcoin_shared_protocol::crypto::domain_signing::{
    oracle_governance_pubkeys,
    signing_chain_id,
    signing_network_id,
};
use wrfcoin_shared_protocol::crypto::signer::load_oracle_signer;
use wrfcoin_shared_protocol::oracle::ceremony::{
    GenesisOraclePossessionBundle,
    sign_oracle_possession_proof,
};

fn main() {
    let args = env::args().collect::<Vec<_>>();
    // #2721: BEFORE the argument-count check. Previously `--version` fell
    // through to it and exited 2 with a usage line, so an operator holding a
    // ceremony binary had no way to ask which commit it came from — the
    // verification step the cut card's A1 abort assumes exists.
    if wrfcoin_build_provenance::print_version_if_requested(
        "oracle_ceremony_sign",
        env!("CARGO_PKG_VERSION"),
        &args,
    ) {
        return;
    }
    if args.len() != 3 {
        eprintln!("usage: oracle_ceremony_sign <ceremony-bundle.json> <timestamp>");
        eprintln!("       oracle_ceremony_sign --version");
        std::process::exit(2);
    }

    let bytes = fs::read(&args[1]).expect("arg1: read ceremony bundle");
    let bundle = serde_json::from_slice::<GenesisOraclePossessionBundle>(&bytes)
        .expect("arg1: parse ceremony bundle");
    let timestamp = args[2]
        .parse::<i64>()
        .expect("arg2: timestamp must be unix seconds");
    let resolved = load_oracle_signer()
        .expect("load local oracle signer")
        .expect("ORACLE CEREMONY REFUSED: no oracle signer configured");
    let signing_public = resolved.signer.public_key();
    let chain_id = signing_chain_id();
    let network_id = signing_network_id();
    let proof = sign_oracle_possession_proof(
        &bundle,
        &chain_id,
        &network_id,
        &oracle_governance_pubkeys(),
        resolved.signer.as_ref(),
        timestamp,
    )
    .expect("ORACLE CEREMONY REFUSED: local signer is not in the final registration set");

    eprintln!(
        "# oracle possession verified locally: identity={} public_key={} signer={}",
        proof.oracle_id,
        signing_public.to_hex(),
        resolved.signer.provenance()
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&proof).expect("serialize possession proof")
    );
}
