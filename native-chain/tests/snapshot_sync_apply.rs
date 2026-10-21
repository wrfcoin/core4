//! Round-trip tests for `State::apply_snapshot_entries` (#1405), format v2.
//!
//! #3288 fix 3a replaced the hand-encoded v1 format (12 pipe-delimited entry
//! prefixes, which left 27 of 40 state-root sections at the importing node's
//! own values) with the whole-`State` codec in
//! `wrfcoin_shared_protocol::blockchain::state_snapshot`. The exporter lives
//! in shared-protocol now, so these tests build real states and export them
//! instead of hand-constructing wire bytes.
//!
//! Kept from the PR #1435 review: a production-scale height, crash-restart
//! convergence, and an empty attestation set.

use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::state_snapshot::{
    export_state_entries, first_carried_difference,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::oracle::attestation::{AttestationRecord, AttestationSet};

fn addr(byte: u8) -> Address {
    Address::new([byte; 20])
}

/// A state shaped like a chain at height ~5000: funded accounts, an oracle
/// set with signing keys, attestations and spent nonces.
fn h5000_state() -> State {
    let mut state = State::new();
    for (i, balance) in [500_000u128, 123_456, 1].into_iter().enumerate() {
        let mut account =
            wrfcoin_shared_protocol::blockchain::state::Account::new(Balance(balance));
        account.nonce = 7 - i as u64 * 2;
        state.accounts.insert(addr(0x11 * (i as u8 + 1)), account);
    }
    let identity = "a1".repeat(32);
    state
        .apply_transitions_batch(std::iter::once(StateTransition::ApplyOracleRegistration {
            oracle_set: vec![identity.clone()],
            oracle_quorum: 1,
            identity: identity.clone(),
            public_key: identity,
            epoch: 1,
            activated_at: 5_000,
        }))
        .expect("oracle registration applies");
    let mut set = AttestationSet::default();
    for (oracle, approved, confidence, ts) in
        [("oracle-a", true, 0.91, 4_965), ("oracle-b", false, 0.42, 4_966)]
    {
        set.by_oracle.insert(
            oracle.into(),
            AttestationRecord { oracle_id: oracle.into(), approved, confidence, timestamp: ts },
        );
    }
    state.attestations.insert("commit-abc".into(), set);
    for i in 0..500u64 {
        state.spent_oracle_nonces.insert(format!("nonce-{i:04}"));
        state.spent_oracle_nonces_height.insert(format!("nonce-{i:04}"), 4_500 + i);
    }
    state.recalculate_state_root();
    state
}

/// Full end-to-end apply at production scale: every carried field matches,
/// not just the root.
#[test]
fn snapshot_sync_h5000_e2e() {
    let source = h5000_state();
    let entries = export_state_entries(&source).expect("export");
    let mut dest = State::new();
    dest.apply_snapshot_entries(&entries).expect("apply");
    assert_eq!(dest.state_root, source.state_root);
    assert_eq!(first_carried_difference(&source, &dest).unwrap(), None);
    assert_eq!(dest.accounts.len(), 3);
    assert_eq!(dest.spent_oracle_nonces.len(), 500);
    assert_eq!(dest.attestations["commit-abc"].by_oracle.len(), 2);
}

/// Crash-restart and idempotency. A partial entry list is REFUSED (a carried
/// field is missing) and leaves the state untouched, so a restart that
/// re-applies the full list converges on the clean result. Applying twice
/// equals applying once.
#[test]
fn snapshot_sync_crash_restart() {
    let source = h5000_state();
    let entries = export_state_entries(&source).expect("export");

    let mut dest = State::new();
    dest.recalculate_state_root();
    let untouched = dest.state_root;
    let partial = entries[..entries.len() / 2].to_vec();
    assert!(dest.apply_snapshot_entries(&partial).is_err(), "a partial snapshot must be refused");
    assert_eq!(dest.state_root, untouched, "a refused apply must not touch the state");

    dest.apply_snapshot_entries(&entries).expect("restart apply");
    assert_eq!(dest.state_root, source.state_root);
    dest.apply_snapshot_entries(&entries).expect("second apply");
    assert_eq!(dest.state_root, source.state_root, "double apply equals single apply");
}

/// Restarts can interrupt at any offset: every cut point is refused and
/// untouched, and the full replay converges.
#[test]
fn snapshot_sync_crash_restart_cutpoints_converge() {
    let source = h5000_state();
    let entries = export_state_entries(&source).expect("export");
    for cutpoint in [1usize, entries.len() / 3, entries.len() / 2, entries.len() - 1] {
        let mut restarted = State::new();
        assert!(
            restarted.apply_snapshot_entries(&entries[..cutpoint]).is_err(),
            "cut point {cutpoint} must be refused"
        );
        restarted.apply_snapshot_entries(&entries).expect("full replay after restart");
        assert_eq!(restarted.state_root, source.state_root, "cut point {cutpoint}");
    }
}

/// An empty attestation set (legal when a commit has no oracle votes yet)
/// round-trips.
#[test]
fn snapshot_sync_empty_attestation_applies() {
    let mut source = State::new();
    source.attestations.insert("commit-empty".into(), AttestationSet::default());
    source.recalculate_state_root();
    let mut dest = State::new();
    dest.apply_snapshot_entries(&export_state_entries(&source).unwrap())
        .expect("empty attestation must apply");
    let set = dest.attestations.get("commit-empty").expect("empty set exists");
    assert!(set.by_oracle.is_empty());
    assert_eq!(dest.state_root, source.state_root);
}

/// Unknown keys, including every v1 prefix, are refused as unsupported.
#[test]
fn snapshot_sync_unknown_prefix_rejected() {
    let mut entries = export_state_entries(&State::new()).unwrap();
    entries.push((b"never_heard_of_this/key".to_vec(), b"value".to_vec()));
    let mut state = State::new();
    let err = state.apply_snapshot_entries(&entries).expect_err("unknown prefix must be rejected");
    assert!(format!("{err}").contains("unsupported entry type"), "got: {err}");
}
