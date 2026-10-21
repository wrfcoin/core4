//! CI gate integration test for consensus invariants.
//!
//! This test:
//! 1. Runs the bounded model checker against all registered invariants.
//! 2. Outputs a JSON report to `target/invariant-check-results.json`.
//! 3. Fails the test (and CI) if any invariant is violated.

use std::fs;
use std::path::Path;
use wrfcoin_shared_protocol::invariants::{
    all_invariants,
    model_checker::{self, ModelCheckConfig, ModelCheckReport},
};

fn write_report(report: &ModelCheckReport) {
    let json = serde_json::to_string_pretty(report).expect("serialize report");
    let target_dir = Path::new("target");
    if target_dir.exists() {
        let path = target_dir.join("invariant-check-results.json");
        fs::write(&path, &json).unwrap_or_else(|e| {
            eprintln!(
                "Warning: could not write report to {}: {}",
                path.display(),
                e
            );
        });
        eprintln!("Report written to {}", path.display());
    }
    eprintln!("{json}");
}

#[test]
fn model_check_gate_default_config() {
    let config = ModelCheckConfig::default();
    let invariants = all_invariants();
    let report = model_checker::run(&config, &invariants);

    eprintln!(
        "Model check: {} states, {} transitions, {} invariant checks, {} violations",
        report.states_explored,
        report.transitions_applied,
        report.invariant_checks,
        report.violations.len(),
    );

    write_report(&report);

    assert!(
        report.all_passed,
        "INVARIANT VIOLATIONS DETECTED ({}):\n{}",
        report.violations.len(),
        report
            .violations
            .iter()
            .map(|v| format!(
                "  [depth={}] {}: {} (transition: {})",
                v.depth, v.invariant, v.detail, v.transition
            ))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

#[test]
fn model_check_gate_with_staking() {
    let config = ModelCheckConfig {
        num_accounts: 2,
        initial_balances: vec![200, 100],
        amounts: vec![1, 25, 50, 100],
        max_depth: 4,
        stake_required: vec![50, 30],
    };
    let invariants = all_invariants();
    let report = model_checker::run(&config, &invariants);

    eprintln!(
        "Staking model check: {} states, {} transitions, {} checks",
        report.states_explored, report.transitions_applied, report.invariant_checks,
    );

    assert!(
        report.all_passed,
        "INVARIANT VIOLATIONS: {:?}",
        report.violations,
    );
}

#[test]
fn model_check_gate_three_accounts() {
    let config = ModelCheckConfig {
        num_accounts: 3,
        initial_balances: vec![100, 50, 25],
        amounts: vec![1, 25],
        max_depth: 4,
        stake_required: vec![0, 0, 10],
    };
    let invariants = all_invariants();
    let report = model_checker::run(&config, &invariants);

    assert!(
        report.all_passed,
        "INVARIANT VIOLATIONS: {:?}",
        report.violations,
    );
}
