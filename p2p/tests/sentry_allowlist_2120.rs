//! #2120 — a validator keeps inbound consensus connections only from its sentries.
//!
//! The decision (`config::inbound_consensus_allowed`) and the settings parser are
//! unit-tested in `config.rs`; the refusal counter in `metrics.rs`. These tests
//! prove the crate-level wiring: the parser refuses unknown roles and
//! malformed peer ids rather than guessing, `apply_role_env` leaves a config
//! untouched when nothing is set, and — at source level — the swarm applies the
//! decision on `ConnectionEstablished` before the peer holds a slot.

use libp2p::PeerId;
use wrfcoin_p2p::config::{NodeRole, P2pConfig, inbound_consensus_allowed, parse_role_settings};

fn pid() -> PeerId {
    libp2p::identity::Keypair::generate_ed25519().public().to_peer_id()
}

#[test]
fn role_settings_are_parsed_or_refused_never_guessed() {
    let (a, b) = (pid(), pid());
    let raw = format!(" {a}, {b} ,{a} ");
    let (role, list) = parse_role_settings(Some("validator"), Some(&raw)).unwrap();
    assert_eq!(role, NodeRole::Validator);
    assert_eq!(list.as_deref(), Some(&[a, b][..]), "deduplicated, order kept");
    assert!(inbound_consensus_allowed(role, list.as_deref(), false, &b));
    assert!(!inbound_consensus_allowed(role, list.as_deref(), false, &pid()));

    let (role, list) = parse_role_settings(None, None).unwrap();
    assert_eq!(role, NodeRole::Public);
    assert!(list.is_none());
    let (_, list) = parse_role_settings(Some("sentry"), Some("")).unwrap();
    assert_eq!(list, Some(Vec::new()), "configured-but-empty is distinct from absent");

    let err = parse_role_settings(Some("producer"), None).unwrap_err();
    assert!(err.contains("WRFCOIN_P2P_NODE_ROLE"), "{err}");
    let err = parse_role_settings(Some("validator"), Some("not-a-peer-id")).unwrap_err();
    assert!(err.contains("WRFCOIN_P2P_SENTRY_ALLOWLIST"), "{err}");
}

#[test]
fn apply_role_env_is_a_no_op_when_nothing_is_set() {
    // These variables are not set in the test environment; the helper must leave
    // the config exactly as constructed and a validator config must still fail
    // `validate` without an allowlist (the startup half of the rule).
    let mut cfg = P2pConfig::default();
    cfg.apply_role_env().unwrap();
    assert_eq!(cfg.role, NodeRole::Public);
    assert!(cfg.sentry_allowlist.is_none());
    let mut validator = P2pConfig::default();
    validator.role = NodeRole::Validator;
    assert!(validator.validate().is_err(), "a validator with no allowlist refuses to start");
}

/// Source pin: the decision runs on every `ConnectionEstablished`, before the
/// subnet check and before `peer_connected` grants a slot, and a refusal closes
/// the connection and records the metric.
#[test]
fn the_swarm_applies_the_decision_before_granting_a_slot() {
    let node = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/node.rs")).unwrap();
    let arm = node.find("SwarmEvent::ConnectionEstablished {").expect("arm");
    let body = &node[arm..arm + 6000];
    let decision = body.find("crate::config::inbound_consensus_allowed(").expect("decision in the arm");
    let subnet = body.find("subnet_allows(").expect("subnet check");
    let slot = body.find("peer_manager.peer_connected(").expect("slot grant");
    assert!(decision < subnet && decision < slot, "sentry decision must come first");
    let refusal = &body[decision..subnet];
    assert!(refusal.contains("DropReason::NotAllowlistedSentry"), "refusal must be counted");
    assert!(refusal.contains("close_connection(connection_id)"), "refusal must close the connection");
    assert!(body.contains("endpoint.is_dialer()"), "outbound dials are exempt by the endpoint, not by guess");
}
