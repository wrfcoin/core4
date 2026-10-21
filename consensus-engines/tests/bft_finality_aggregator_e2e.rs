//! End-to-end integration test for the BFT finality aggregator (#1388).
//!
//! Exercises the path the plan calls out: a 3-validator
//! [`BftFinalityGadget`] receives one signed vote per validator via the
//! cryptographically-verified entry point, the supermajority threshold
//! is crossed, and a [`FinalitySignal`] arrives on the channel that
//! `launch_api/finality_events.rs::run_finality_event_loop` consumes.
//!
//! Together with the unit tests in
//! `consensus-engines/src/selection/bft_finality.rs` and the wiring
//! tests in `shared-protocol`/`native-chain`, this completes the
//! "BFT finality channel emits at least one cert" acceptance criterion
//! for issue #1388 without requiring a full multi-process testnet.

use ed25519_dalek::{Signer, SigningKey};
use wrfcoin_consensus_engines::{
    BftFinalityGadget, FinalityEvent, FinalityValidator, FinalityVote,
};
use wrfcoin_shared_protocol::consensus::finality::finality_signal_channel;

/// Mint a deterministic ed25519 signing key from a single seed byte.
fn signing_key(seed: u8) -> SigningKey {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    SigningKey::from_bytes(&bytes)
}

/// Mint a signed vote for the given block on behalf of the validator
/// identified by `(id, sk)`. The signing message is computed via the
/// gadget's canonical helper so signers and verifiers stay in lock-step.
fn make_signed_vote(
    gadget: &BftFinalityGadget,
    sk: &SigningKey,
    validator_id: &str,
    block_height: u64,
    block_hash: [u8; 32],
) -> FinalityVote {
    // #2256: scope the vote to the committee the gadget serves.
    let epoch = gadget.epoch();
    let validator_set_hash = gadget.validator_set_hash();
    let msg = BftFinalityGadget::signing_message_for(
        block_hash,
        block_height,
        epoch,
        validator_set_hash,
        validator_id,
    );
    let sig = sk.sign(&msg);
    FinalityVote {
        block_hash,
        block_height,
        epoch,
        validator_set_hash,
        validator_id: validator_id.to_string(),
        signature: sig.to_bytes().to_vec(),
    }
}

#[tokio::test]
async fn three_validator_supermajority_emits_finality_signal_on_channel() {
    // Three validators with equal stake — three votes are required to
    // cross the strict 2/3+ threshold (2/3 of total stake = exactly 2/3,
    // which is NOT a supermajority).
    let sk0 = signing_key(11);
    let sk1 = signing_key(22);
    let sk2 = signing_key(33);

    let validators = vec![
        FinalityValidator {
            id: "v0".into(),
            stake_weight: 100,
            public_key: Some(sk0.verifying_key().to_bytes()),
        },
        FinalityValidator {
            id: "v1".into(),
            stake_weight: 100,
            public_key: Some(sk1.verifying_key().to_bytes()),
        },
        FinalityValidator {
            id: "v2".into(),
            stake_weight: 100,
            public_key: Some(sk2.verifying_key().to_bytes()),
        },
    ];

    let (sink_tx, mut sink_rx) = finality_signal_channel();
    let mut gadget = BftFinalityGadget::with_finality_sink(validators, sink_tx);

    let block_hash = [0xABu8; 32];
    let block_height: u64 = 42;

    // First two votes: under the supermajority. No signal must arrive.
    gadget
        .submit_vote_verified(make_signed_vote(
            &gadget,
            &sk0,
            "v0",
            block_height,
            block_hash,
        ))
        .expect("v0 vote must be accepted");
    gadget
        .submit_vote_verified(make_signed_vote(
            &gadget,
            &sk1,
            "v1",
            block_height,
            block_hash,
        ))
        .expect("v1 vote must be accepted");
    assert!(
        sink_rx.try_recv().is_err(),
        "no signal must be emitted before supermajority is crossed"
    );

    // Third vote: 300/300 stake → strict 2/3+ → finality.
    let event = gadget
        .submit_vote_verified(make_signed_vote(
            &gadget,
            &sk2,
            "v2",
            block_height,
            block_hash,
        ))
        .expect("v2 vote must be accepted");
    assert!(
        matches!(event, FinalityEvent::BlockFinalized { .. }),
        "third vote must produce BlockFinalized, got {event:?}"
    );

    // The acceptance test: the signal must arrive on the channel that
    // `run_finality_event_loop` consumes.
    let signal = sink_rx
        .recv()
        .await
        .expect("supermajority must emit a FinalitySignal on the sink");
    assert_eq!(signal.height, block_height, "signal height mismatch");
    assert_eq!(signal.block_hash, block_hash, "signal block_hash mismatch");
    assert_eq!(
        signal.source, "bft-gadget",
        "signal source must identify the BFT gadget so operators can trace its origin"
    );

    // The certificate snapshot is reachable for downstream gossiping
    // (this is what `p2p_events.rs::run_p2p_event_loop` rebroadcasts).
    let cert = gadget
        .last_certificate()
        .expect("certificate must be populated alongside the signal");
    assert_eq!(cert.block_height, block_height);
    assert_eq!(cert.block_hash, block_hash);
    assert_eq!(
        cert.votes.len(),
        3,
        "cert must carry all 3 supermajority votes"
    );
    assert_eq!(cert.total_vote_weight, 300);
    assert_eq!(cert.total_validator_weight, 300);

    // Deterministic ordering — required so cert bytes hash identically
    // on every honest node and gossiped certs deduplicate.
    let ids: Vec<&str> = cert.votes.iter().map(|v| v.validator_id.as_str()).collect();
    assert_eq!(ids, vec!["v0", "v1", "v2"]);
}

#[tokio::test]
async fn bft_aggregator_does_not_emit_for_minority_vote_set() {
    // Negative test: with only 2 of 3 votes, the channel must stay quiet.
    let sk0 = signing_key(7);
    let sk1 = signing_key(8);
    let sk2 = signing_key(9);

    let validators = vec![
        FinalityValidator {
            id: "v0".into(),
            stake_weight: 100,
            public_key: Some(sk0.verifying_key().to_bytes()),
        },
        FinalityValidator {
            id: "v1".into(),
            stake_weight: 100,
            public_key: Some(sk1.verifying_key().to_bytes()),
        },
        FinalityValidator {
            id: "v2".into(),
            stake_weight: 100,
            public_key: Some(sk2.verifying_key().to_bytes()),
        },
    ];

    let (sink_tx, mut sink_rx) = finality_signal_channel();
    let mut gadget = BftFinalityGadget::with_finality_sink(validators, sink_tx);
    let block_hash = [0x01u8; 32];

    gadget
        .submit_vote_verified(make_signed_vote(&gadget, &sk0, "v0", 1, block_hash))
        .unwrap();
    gadget
        .submit_vote_verified(make_signed_vote(&gadget, &sk1, "v1", 1, block_hash))
        .unwrap();

    // 2/3 of stake = exactly 2/3, which is NOT > 2/3 (strict).
    assert!(
        !gadget.is_finalized(1),
        "exactly 2/3 stake must not finalize; the rule is strict > 2/3"
    );
    assert!(
        sink_rx.try_recv().is_err(),
        "no signal must be emitted until the supermajority is strictly exceeded"
    );
    assert!(
        gadget.last_certificate().is_none(),
        "no certificate must be produced below the supermajority threshold"
    );
}
