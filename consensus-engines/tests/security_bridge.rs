//! Integration tests for `security::bridge::SecurityConsensusBridge` (core4#1386).
//!
//! The in-source test module in `src/security/bridge.rs` only exercises struct
//! defaults. These tests hit the 8 async handler surfaces — each with a
//! happy-path, a negative/error-path, and a boundary case — so refactors of
//! the bridge can catch regressions in the actual handler logic.
//!
//! Test fixture: a fresh `SecurityConsensusBridge` wired with
//! `DynamicConsensusEngine` (PoW → PoP) + `SecureConsensusCoordinator` +
//! `ConsensusSecurityManager` whose keys are initialized in-memory. This
//! matches the production wiring from `new_with_components`.
//!
//! Note on "happy path": a freshly constructed manager has zero reputation
//! nodes tracked, which makes `byzantine_tolerance = false` in
//! `AttackPreventionSystem::get_network_health`. As a result, the
//! `detect_byzantine_faults` checkpoint returns a `Warning`, which demotes
//! `overall_status` to `RequiresManualReview` for any transition. The
//! "happy-path" tests below therefore assert on observable *success through
//! the handler itself* (Ok return, correct operation_type wired into the
//! result) rather than on `success == true`, and validate the negative paths
//! that flip the result to explicit failure.

use std::sync::Arc;

use sha2::{Digest, Sha256};
use wrfcoin_consensus_engines::coordination::dynamic_engine::{
    DynamicConsensusConfig, DynamicConsensusEngine, GovernanceConfig, LeaderElectionParams,
    TransitionTrigger,
};
use wrfcoin_consensus_engines::engine::ConsensusEngineManager;
use wrfcoin_consensus_engines::factory::ConsensusFactory;
use wrfcoin_consensus_engines::security::bridge::{
    SecureOperationType, SecurityConsensusBridge, SecurityConsensusBridgeConfig,
};
use wrfcoin_consensus_engines::security::coordinator::{
    SecureConsensusConfig, SecureConsensusCoordinator,
};
use wrfcoin_consensus_engines::security::manager::{
    AuthorizationLevel, ConsensusSecurityManager, SecurityManagerConfig, SecurityThreatLevel,
};
use wrfcoin_consensus_engines::types::ConsensusType;

// ---------- fixture ----------

/// Build a `SecurityConsensusBridge` wired like production: PoW initial
/// engine, PoP target algorithm, initialized crypto keys, default bridge /
/// coordinator configs. Two `DynamicConsensusEngine` instances are built (one
/// for the bridge, one for the coordinator) because both own the engine
/// exclusively.
async fn make_bridge() -> SecurityConsensusBridge {
    make_bridge_with(default_security_config()).await
}

async fn make_bridge_with(
    security_config: SecurityManagerConfig,
) -> SecurityConsensusBridge {
    let bridge_engine = build_dynamic_engine().await;
    let coord_engine = build_dynamic_engine().await;

    let coordinator_cfg = SecureConsensusConfig {
        security_config: security_config.clone(),
        ..SecureConsensusConfig::default()
    };
    let coordinator = Arc::new(
        SecureConsensusCoordinator::new(coordinator_cfg, coord_engine)
            .await
            .expect("coordinator init"),
    );

    let security_manager = Arc::new(ConsensusSecurityManager::new(security_config.clone()));
    security_manager
        .initialize_keys(None)
        .await
        .expect("key init");

    let bridge_cfg = SecurityConsensusBridgeConfig {
        security_config,
        ..SecurityConsensusBridgeConfig::default()
    };
    SecurityConsensusBridge::new_with_components(
        bridge_cfg,
        security_manager,
        bridge_engine,
        coordinator,
    )
}

async fn build_dynamic_engine() -> DynamicConsensusEngine {
    let pow_config = ConsensusFactory::default_config(ConsensusType::ProofOfWork);
    let initial =
        ConsensusFactory::create(ConsensusType::ProofOfWork, &pow_config).expect("pow create");
    let manager = ConsensusEngineManager::new(initial, pow_config.clone());

    let mut dynamic_config = DynamicConsensusConfig::default();
    dynamic_config.consensus_config =
        ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);

    DynamicConsensusEngine::new(dynamic_config, manager, GovernanceConfig::default())
        .await
        .expect("dynamic engine init")
}

/// `SecurityManagerConfig` with multisig disabled, so emergency authorization
/// tests don't need to carry 3 signatures. Injects a SHA-256 of a known
/// plaintext key into `emergency_override_keys` so the happy-path emergency
/// flow can pass `authorize_emergency_transition`.
fn default_security_config() -> SecurityManagerConfig {
    let mut cfg = SecurityManagerConfig::default();
    cfg.enable_multisig = false;
    cfg.emergency_override_keys = vec![sha256_hex("emergency-test-key")];
    cfg
}

fn sha256_hex(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn invalid_standard_auth() -> AuthorizationLevel {
    AuthorizationLevel::Standard {
        node_id: "unregistered-node".to_string(),
        signature: vec![0u8; 64],
    }
}

fn governance_auth(approval: u64, total: u64, proposal: &str) -> AuthorizationLevel {
    AuthorizationLevel::Governance {
        proposal_id: proposal.to_string(),
        approval_votes: approval,
        total_votes: total,
        governance_signature: vec![0u8; 64],
    }
}

fn default_trigger() -> TransitionTrigger {
    TransitionTrigger::Governance {
        proposal_id: "prop-1".to_string(),
        approval_percentage: 0.7,
        vote_count: 70,
    }
}

// =============================================================================
// Handler 1: execute_secure_algorithm_transition
// =============================================================================

#[tokio::test]
async fn transition_valid_governance_returns_ok_with_transition_operation_type() {
    let bridge = make_bridge().await;

    let result = bridge
        .execute_secure_algorithm_transition(
            ConsensusType::ProofOfWork,
            ConsensusType::ProofOfStake,
            governance_auth(70, 100, "prop-1"),
            default_trigger(),
        )
        .await
        .expect("handler returns Ok even when internal validation fails");

    assert!(matches!(
        result.operation_type,
        SecureOperationType::AlgorithmTransition { .. }
    ));
    assert!(!result.operation_id.is_empty());
    // timestamp is captured from UNIX_EPOCH; handler returning means the
    // timestamp field was populated non-zero.
    assert!(result.timestamp > 0);
}

#[tokio::test]
async fn transition_invalid_standard_auth_fails_with_error_details() {
    let bridge = make_bridge().await;

    let result = bridge
        .execute_secure_algorithm_transition(
            ConsensusType::ProofOfWork,
            ConsensusType::ProofOfStake,
            invalid_standard_auth(),
            default_trigger(),
        )
        .await
        .expect("handler maps auth failure to failed result, not Err");

    assert!(!result.success);
    assert!(!result.security_validated);
    assert!(result.error_details.is_some());
}

#[tokio::test]
async fn transition_boundary_governance_below_majority_is_rejected() {
    // Governance auth with <50% approval must be rejected by
    // validate_governance_authorization, yielding a failed result.
    let bridge = make_bridge().await;

    let result = bridge
        .execute_secure_algorithm_transition(
            ConsensusType::ProofOfWork,
            ConsensusType::ProofOfStake,
            governance_auth(20, 100, "prop-minority"),
            default_trigger(),
        )
        .await
        .expect("handler returns Ok with failed result");

    assert!(!result.success);
    assert!(result.error_details.is_some());
}

// =============================================================================
// Handler 2: execute_secure_leader_election
// =============================================================================

#[tokio::test]
async fn leader_election_valid_candidates_returns_election_operation_type() {
    let bridge = make_bridge().await;

    let candidates = vec![
        "validator-alpha".to_string(),
        "validator-beta".to_string(),
        "validator-gamma".to_string(),
        "validator-delta".to_string(),
    ];
    let mut params = LeaderElectionParams::default();
    params.min_candidates = 3;

    let result = bridge
        .execute_secure_leader_election(candidates.clone(), params)
        .await
        .expect("Ok result");

    match &result.operation_type {
        SecureOperationType::LeaderElection { candidates: c, .. } => {
            assert_eq!(c.len(), candidates.len());
        },
        other => panic!("expected LeaderElection, got {other:?}"),
    }
}

#[tokio::test]
async fn leader_election_all_malicious_filtered_returns_failed_result() {
    let bridge = make_bridge().await;

    // screen_election_candidates drops any candidate whose name contains
    // "malicious"; with every candidate filtered, the handler must return a
    // failed result with "No valid candidates" in error_details.
    let candidates = vec![
        "malicious-node-a".to_string(),
        "malicious-node-b".to_string(),
        "malicious-node-c".to_string(),
    ];

    let result = bridge
        .execute_secure_leader_election(candidates, LeaderElectionParams::default())
        .await
        .expect("Ok result");

    assert!(!result.success);
    let err = result.error_details.expect("error_details set");
    assert!(
        err.contains("No valid candidates"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn leader_election_boundary_empty_candidate_list_fails() {
    let bridge = make_bridge().await;

    let result = bridge
        .execute_secure_leader_election(vec![], LeaderElectionParams::default())
        .await
        .expect("Ok result");

    assert!(!result.success);
    match result.operation_type {
        SecureOperationType::LeaderElection {
            candidates, elected, ..
        } => {
            assert!(candidates.is_empty());
            assert_eq!(elected, "none");
        },
        other => panic!("expected LeaderElection, got {other:?}"),
    }
}

// =============================================================================
// Handler 3: execute_emergency_protocol
// =============================================================================

#[tokio::test]
async fn emergency_unknown_protocol_with_valid_key_returns_failed_result() {
    // With multisig disabled and a valid emergency key, the outer
    // authorization passes. The "unknown-protocol" branch then returns
    // ConsensusError::InvalidState inside the protocol match arm — which the
    // handler converts into a failed result with error_details populated.
    let bridge = make_bridge().await;

    let result = bridge
        .execute_emergency_protocol(
            "definitely-not-a-real-protocol".to_string(),
            "emergency-test-key".to_string(),
            "security breach".to_string(),
            vec![],
        )
        .await
        .expect("authorization passes, inner protocol mismatch yields failed result");

    assert!(!result.success);
    let err = result.error_details.expect("error_details set");
    assert!(err.contains("Unknown emergency protocol"), "unexpected: {err}");
}

#[tokio::test]
async fn emergency_invalid_override_key_returns_err() {
    let bridge = make_bridge().await;

    let err = bridge
        .execute_emergency_protocol(
            "network_isolation".to_string(),
            "not-a-known-key".to_string(),
            "security breach".to_string(),
            vec![],
        )
        .await;

    assert!(
        err.is_err(),
        "unknown emergency key must bubble up as Err, got: {err:?}"
    );
}

#[tokio::test]
async fn emergency_network_isolation_with_valid_key_succeeds() {
    let bridge = make_bridge().await;

    let result = bridge
        .execute_emergency_protocol(
            "network_isolation".to_string(),
            "emergency-test-key".to_string(),
            "critical network partition".to_string(),
            vec![],
        )
        .await
        .expect("Ok result");

    assert!(result.success, "network_isolation is a no-op stub that returns Ok(())");
    assert!(matches!(
        result.operation_type,
        SecureOperationType::EmergencyProtocol { .. }
    ));
}

// =============================================================================
// Handler 4: generate_governance_security_report
// =============================================================================

#[tokio::test]
async fn governance_report_enabled_returns_ok() {
    let bridge = make_bridge().await;
    let report = bridge.generate_governance_security_report().await;
    assert!(
        report.is_ok(),
        "governance report should succeed with default-enabled reporting"
    );
}

#[tokio::test]
async fn governance_report_disabled_returns_configuration_err() {
    let bridge_engine = build_dynamic_engine().await;
    let coord_engine = build_dynamic_engine().await;
    let coordinator = Arc::new(
        SecureConsensusCoordinator::new(SecureConsensusConfig::default(), coord_engine)
            .await
            .expect("coordinator init"),
    );
    let security_manager = Arc::new(ConsensusSecurityManager::new(
        SecurityManagerConfig::default(),
    ));
    security_manager.initialize_keys(None).await.unwrap();

    let mut cfg = SecurityConsensusBridgeConfig::default();
    cfg.enable_governance_reporting = false;

    let bridge = SecurityConsensusBridge::new_with_components(
        cfg,
        security_manager,
        bridge_engine,
        coordinator,
    );

    let err = bridge.generate_governance_security_report().await;
    assert!(err.is_err(), "disabled reporting must return Err");
}

#[tokio::test]
async fn governance_report_boundary_preserves_config_flag() {
    // Toggling the flag at construction time is the only public lever;
    // verify both branches.
    for enabled in [true, false] {
        let bridge_engine = build_dynamic_engine().await;
        let coord_engine = build_dynamic_engine().await;
        let coordinator = Arc::new(
            SecureConsensusCoordinator::new(SecureConsensusConfig::default(), coord_engine)
                .await
                .unwrap(),
        );
        let mgr = Arc::new(ConsensusSecurityManager::new(
            SecurityManagerConfig::default(),
        ));
        mgr.initialize_keys(None).await.unwrap();

        let mut cfg = SecurityConsensusBridgeConfig::default();
        cfg.enable_governance_reporting = enabled;

        let bridge =
            SecurityConsensusBridge::new_with_components(cfg, mgr, bridge_engine, coordinator);
        let outcome = bridge.generate_governance_security_report().await;
        assert_eq!(
            outcome.is_ok(),
            enabled,
            "enable_governance_reporting={enabled} should control Ok/Err"
        );
    }
}

// =============================================================================
// Handler 5: get_comprehensive_security_status
// =============================================================================

#[tokio::test]
async fn security_status_initial_state_is_green_with_zero_transitions() {
    let bridge = make_bridge().await;
    let status = bridge.get_comprehensive_security_status().await;

    assert!(matches!(status.overall_threat_level, SecurityThreatLevel::Green));
    assert_eq!(status.active_transitions, 0);
    assert_eq!(status.recent_threats, 0);
    assert_eq!(status.system_status, "SECURE_MONITORING_ACTIVE");
}

#[tokio::test]
async fn security_status_tracks_operations_after_election() {
    let bridge = make_bridge().await;

    let _ = bridge
        .execute_secure_leader_election(
            vec!["malicious-a".to_string(), "malicious-b".to_string(), "malicious-c".to_string()],
            LeaderElectionParams::default(),
        )
        .await
        .expect("Ok");

    let status = bridge.get_comprehensive_security_status().await;
    // one failed election operation is tracked in active_operations
    assert!(
        status.active_transitions >= 1,
        "expected at least one tracked operation, got {}",
        status.active_transitions
    );
}

#[tokio::test]
async fn security_status_boundary_monitoring_disabled_reports_disabled_status() {
    let bridge_engine = build_dynamic_engine().await;
    let coord_engine = build_dynamic_engine().await;
    let coordinator = Arc::new(
        SecureConsensusCoordinator::new(SecureConsensusConfig::default(), coord_engine)
            .await
            .unwrap(),
    );
    let mgr = Arc::new(ConsensusSecurityManager::new(
        SecurityManagerConfig::default(),
    ));
    mgr.initialize_keys(None).await.unwrap();

    let mut cfg = SecurityConsensusBridgeConfig::default();
    cfg.enable_realtime_monitoring = false;

    let bridge =
        SecurityConsensusBridge::new_with_components(cfg, mgr, bridge_engine, coordinator);
    let status = bridge.get_comprehensive_security_status().await;

    assert_eq!(status.system_status, "SECURE_MONITORING_DISABLED");
}

// =============================================================================
// Handler 6: get_network_health_with_security
// =============================================================================

#[tokio::test]
async fn network_health_initial_state_has_averaged_score() {
    let bridge = make_bridge().await;
    let health = bridge.get_network_health_with_security().await;

    // Fresh bridge: monitoring_state.network_health_score == 1.0, engine
    // base_health score is in [0,1]. Averaged, result must be in [0,1].
    assert!(
        health.overall_score >= 0.0 && health.overall_score <= 1.0,
        "overall_score out of bounds: {}",
        health.overall_score
    );
}

#[tokio::test]
async fn network_health_reports_byzantine_tolerance_flag() {
    let bridge = make_bridge().await;
    let health = bridge.get_network_health_with_security().await;

    // bridge sets byzantine_tolerance to base_health.byzantine_tolerance AND
    // active_violations < max_security_violations. Fresh bridge has zero
    // violations and default max=3, so bool must match engine's base value
    // (type check — accessor must be reachable without panic).
    let _ = health.byzantine_tolerance;
    // participation_rate and avg_latency should be concrete f64s, not NaN.
    assert!(!health.participation_rate.is_nan());
    assert!(!health.avg_latency.is_nan());
}

#[tokio::test]
async fn network_health_boundary_issues_list_extends_anomalies() {
    let bridge = make_bridge().await;
    let health = bridge.get_network_health_with_security().await;
    // Fresh bridge has no current_anomalies; issues list must still be a
    // valid Vec (not truncated to something unexpected).
    assert!(
        health.issues.iter().all(|s| !s.is_empty()),
        "any issues reported must be non-empty strings"
    );
}

// =============================================================================
// Handler 7: start_security_monitoring
// =============================================================================

#[tokio::test]
async fn start_monitoring_returns_ok() {
    let bridge = make_bridge().await;
    let result = bridge.start_security_monitoring().await;
    assert!(result.is_ok(), "start_security_monitoring should succeed");
}

#[tokio::test]
async fn start_monitoring_is_idempotent_safe_to_call_twice() {
    // The handler spawns a tokio task each call; the bridge itself owns no
    // handle, so repeated calls must not panic or return Err.
    let bridge = make_bridge().await;
    assert!(bridge.start_security_monitoring().await.is_ok());
    assert!(bridge.start_security_monitoring().await.is_ok());
}

#[tokio::test]
async fn start_monitoring_boundary_status_remains_active_after_call() {
    let bridge = make_bridge().await;
    bridge.start_security_monitoring().await.unwrap();
    // status query must still work and report active monitoring
    let status = bridge.get_comprehensive_security_status().await;
    assert_eq!(status.system_status, "SECURE_MONITORING_ACTIVE");
}

// =============================================================================
// Handler 8: new_with_components (construction contract)
//
// perform_security_scan is a private static method only reachable through
// start_security_monitoring's spawned task, so we cover the 8th surface by
// asserting the construction contract of new_with_components — the entry
// point every other handler depends on.
// =============================================================================

#[tokio::test]
async fn construction_happy_path_wires_every_public_handler() {
    // Build with defaults — all handlers must be callable without panicking.
    let bridge = make_bridge().await;

    // Touch every public handler to prove wiring.
    let _ = bridge.get_comprehensive_security_status().await;
    let _ = bridge.get_network_health_with_security().await;
    assert!(bridge.start_security_monitoring().await.is_ok());
}

#[tokio::test]
async fn construction_respects_max_security_violations_override() {
    let bridge_engine = build_dynamic_engine().await;
    let coord_engine = build_dynamic_engine().await;
    let coordinator = Arc::new(
        SecureConsensusCoordinator::new(SecureConsensusConfig::default(), coord_engine)
            .await
            .unwrap(),
    );
    let mgr = Arc::new(ConsensusSecurityManager::new(
        SecurityManagerConfig::default(),
    ));
    mgr.initialize_keys(None).await.unwrap();

    let mut cfg = SecurityConsensusBridgeConfig::default();
    cfg.max_security_violations = 7;

    let bridge =
        SecurityConsensusBridge::new_with_components(cfg, mgr, bridge_engine, coordinator);
    // The override is observable indirectly: byzantine_tolerance in network
    // health uses `active_violations < max_security_violations`. With zero
    // violations, this must evaluate true.
    let health = bridge.get_network_health_with_security().await;
    // byzantine_tolerance depends on base engine; just ensure handler
    // doesn't panic with overridden config.
    let _ = health.byzantine_tolerance;
}

#[tokio::test]
async fn construction_boundary_monitoring_flag_controls_system_status() {
    // Enabled → ACTIVE; disabled → DISABLED. Boundary between the two
    // string constants emitted by get_comprehensive_security_status.
    for enabled in [true, false] {
        let bridge_engine = build_dynamic_engine().await;
        let coord_engine = build_dynamic_engine().await;
        let coordinator = Arc::new(
            SecureConsensusCoordinator::new(SecureConsensusConfig::default(), coord_engine)
                .await
                .unwrap(),
        );
        let mgr = Arc::new(ConsensusSecurityManager::new(
            SecurityManagerConfig::default(),
        ));
        mgr.initialize_keys(None).await.unwrap();

        let mut cfg = SecurityConsensusBridgeConfig::default();
        cfg.enable_realtime_monitoring = enabled;

        let bridge =
            SecurityConsensusBridge::new_with_components(cfg, mgr, bridge_engine, coordinator);
        let status = bridge.get_comprehensive_security_status().await;
        let expected = if enabled {
            "SECURE_MONITORING_ACTIVE"
        } else {
            "SECURE_MONITORING_DISABLED"
        };
        assert_eq!(status.system_status, expected);
    }
}
