//! Build and sign a WRFCoin oracle attestation, emitting the exact JSON body
//! for `POST /transactions/oracle/attestation`.
//!
//! Signing uses the canonical domain-separated message
//! (`wrfcoin-sign-v1|wrfcoin.oracle_attestation.v1|…`) via the protocol's own
//! `OracleAttestationPayload`, so the signature is guaranteed to match what the
//! node verifies. The tool self-verifies with `has_valid_signature()` before
//! printing and aborts if the recipe ever drifts.
//!
//! HTTP is intentionally left to the caller (no client dep): pipe the body to
//! curl with an Oracle-role bearer token, e.g.
//!
//!   BODY=$(cargo run -q -p wrfcoin-shared-protocol --example oracle_attest_sign -- \
//!            "$ORACLE_PRIVKEY" "$COMMIT_ID" true 0.9 "$(date +%s)" "$NONCE")
//!   curl -sf -X POST "$NODE/transactions/oracle/attestation" \
//!        -H "Authorization: Bearer $ORACLE_TOKEN" -H 'Content-Type: application/json' \
//!        -d "$BODY"
//!
//! Args: <privkey_hex> <commit_id> <approved:true|false> <confidence:0..1|null> <timestamp_unix> <nonce>
//!
//! The nonce is REQUIRED and must be non-empty: the node's
//! `submit_oracle_attestation` route rejects a missing nonce with
//! 422 "nonce is required" (`routes/oracle.rs`), so an attestation signed
//! without one can never be submitted.

use std::env;

use wrfcoin_shared_protocol::blockchain::transaction::OracleAttestationPayload;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 7 {
        eprintln!(
            "usage: oracle_attest_sign <privkey_hex> <commit_id> <approved:true|false> \
             <confidence:0..1|null> <timestamp_unix> <nonce>"
        );
        eprintln!("note: nonce is required — the node rejects attestations without one (422)");
        std::process::exit(2);
    }

    let sk = PrivateKey::from_hex(&args[1]).expect("arg1: invalid oracle privkey hex");
    let oracle_id = PublicKey::from_private_key(&sk).to_hex();
    let commit_id = args[2].clone();
    let approved: bool = args[3].parse().expect("arg3: approved must be true|false");
    let confidence: Option<f64> = if args[4].eq_ignore_ascii_case("null") {
        None
    } else {
        let c: f64 = args[4].parse().expect("arg4: confidence must be a float in [0,1] or null");
        assert!((0.0..=1.0).contains(&c), "confidence must be within [0, 1]");
        Some(c)
    };
    let timestamp: i64 = args[5].parse().expect("arg5: timestamp must be unix seconds");
    let nonce = args[6].trim().to_string();
    assert!(
        !nonce.is_empty(),
        "arg6: nonce must be non-empty — submit_oracle_attestation rejects a missing nonce (422)"
    );
    let nonce: Option<String> = Some(nonce);

    let mut payload = OracleAttestationPayload {
        commit_id: commit_id.clone(),
        oracle_id: oracle_id.clone(),
        approved,
        signature: None,
        confidence,
        timestamp,
        nonce: nonce.clone(),
    };
    let message = payload.canonical_signing_message();
    payload.signature = Some(sk.sign(message.as_bytes()).to_hex());

    // Self-verify against the protocol's own verifier before emitting. If this
    // ever fails, the signing recipe has drifted from the node — fail loud.
    assert!(
        payload.has_valid_signature(),
        "self-verification failed — attestation signing recipe drifted from the protocol"
    );

    // Node route (`submit_oracle_attestation`) reads `commit_hash`, not `commit_id`.
    let body = serde_json::json!({
        "oracle_id": oracle_id,
        "commit_hash": commit_id,
        "approved": approved,
        "confidence": confidence,
        "timestamp": timestamp,
        "nonce": nonce,
        "signature": payload.signature,
    });
    println!("{}", serde_json::to_string(&body).expect("serialize attestation body"));
}
