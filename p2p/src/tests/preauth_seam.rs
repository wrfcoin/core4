//! Coverage for the pre-authentication verification seam (#2117).
//!
//! `claim_verify_budget` is the decision that stands between an attacker-chosen
//! public key and a full ed25519 verification on the P2P event loop. These
//! tests cover the decision and the counter.
//!
//! What they do **not** cover is the `return` at each of the two call sites in
//! `handle_behaviour_event`, which is what actually prevents the work. Removing
//! it compiles and passes — a gap found by a RED control on this very change,
//! not discovered later. Closing it needs a real `handle_behaviour_event`
//! invocation, which is what the harness in #2533 provides; that is a follow-up
//! once it merges, and the same one already owed to #2526.

use std::sync::Arc;
use std::time::Duration;

use libp2p::PeerId;

use crate::metrics::MetricsCollector;
use crate::node::claim_verify_budget;
use crate::preauth_budget::{PreAuthLimits, VerifyBudget};

fn limits(max: u32) -> PreAuthLimits {
    PreAuthLimits {
        max_verify_ops_per_window: max,
        verify_window: Duration::from_secs(1),
        ..PreAuthLimits::default()
    }
}

#[test]
fn the_seam_permits_verification_up_to_the_ceiling_then_refuses() {
    let mut budget = VerifyBudget::new(limits(3));
    let metrics = Arc::new(MetricsCollector::new());
    let peer = PeerId::random();

    for i in 0..3 {
        assert!(
            claim_verify_budget(&mut budget, &metrics, peer),
            "verification {i} is within the ceiling"
        );
    }
    assert!(
        !claim_verify_budget(&mut budget, &metrics, peer),
        "the fourth ed25519 verification this second must be refused"
    );
}

#[test]
fn a_refusal_increments_the_skip_counter_and_a_permit_does_not() {
    let mut budget = VerifyBudget::new(limits(1));
    let metrics = Arc::new(MetricsCollector::new());
    let peer = PeerId::random();

    assert!(claim_verify_budget(&mut budget, &metrics, peer));
    assert_eq!(
        metrics.snapshot(0).preauth_verify_skipped,
        0,
        "a permitted verification must not look like a skipped one"
    );

    assert!(!claim_verify_budget(&mut budget, &metrics, peer));
    assert_eq!(
        metrics.snapshot(0).preauth_verify_skipped,
        1,
        "a refusal must be observable, or an operator cannot tell the ceiling \
         is biting"
    );
}

#[test]
fn one_peer_exhausting_the_seam_does_not_refuse_another() {
    // The per-peer keying is what stops the defence becoming the attack: a
    // global budget would let one flooder deny verification to every honest
    // peer, which is worse than the burn it prevents.
    let mut budget = VerifyBudget::new(limits(1));
    let metrics = Arc::new(MetricsCollector::new());
    let attacker = PeerId::random();
    let honest = PeerId::random();

    assert!(claim_verify_budget(&mut budget, &metrics, attacker));
    assert!(!claim_verify_budget(&mut budget, &metrics, attacker));

    assert!(
        claim_verify_budget(&mut budget, &metrics, honest),
        "an honest peer must still get its verification"
    );
}

#[test]
fn a_disabled_budget_permits_everything() {
    let mut budget = VerifyBudget::new(limits(0));
    let metrics = Arc::new(MetricsCollector::new());
    let peer = PeerId::random();

    for _ in 0..1_000 {
        assert!(claim_verify_budget(&mut budget, &metrics, peer));
    }
    assert_eq!(metrics.snapshot(0).preauth_verify_skipped, 0);
}
