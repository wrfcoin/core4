//! #3565: `device_submission_limit` is schedulable only at least
//! `DEVICE_SUBMISSION_LIMIT_SUBMITTER_KEY_LEAD_BLOCKS` (2,000) blocks after
//! `station_submitter_key`, the window in which owners approve their
//! collector and phone keys. ADMISSION POLICY (`check_admission_policy`), the
//! same shape as `forecast_late_score`'s key-binding lead. `late_data_pay`
//! stays held.
//!
//! Own test binary: nothing here installs a schedule, so the node's effective
//! schedule names neither feature.

use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, DEVICE_SUBMISSION_LIMIT_SUBMITTER_KEY_LEAD_BLOCKS,
    device_submission_limit_active, device_submission_limit_active_with,
    device_submission_limit_schedule_allowed,
};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    FeatureScheduleExtension, ScheduleEntry, ScheduleExtensionError, check_admission_policy,
    check_device_submission_limit_dependency,
};

const LEAD: u64 = DEVICE_SUBMISSION_LIMIT_SUBMITTER_KEY_LEAD_BLOCKS;

fn extension(entries: &[(&str, u64)]) -> FeatureScheduleExtension {
    let mut entries: Vec<ScheduleEntry> =
        entries.iter().map(|(f, h)| ScheduleEntry::new(*f, *h)).collect();
    entries.sort_by(|a, b| (a.activation_height, &a.feature).cmp(&(b.activation_height, &b.feature)));
    FeatureScheduleExtension {
        chain_id: "888".into(),
        network_id: "testnet".into(),
        genesis_hash: "ab".repeat(32),
        sequence: 0,
        entries,
        version_steps: Vec::new(),
        min_protocol_version: 1,
        release_commit: "cd".repeat(20),
    }
}

fn too_early(result: Result<(), ScheduleExtensionError>) -> Option<(u64, Option<u64>)> {
    match result {
        Err(ScheduleExtensionError::DeviceSubmissionLimitTooEarly { activation_height, submitter_key_activation }) => {
            Some((activation_height, submitter_key_activation))
        },
        _ => None,
    }
}

#[test]
fn the_lead_rule() {
    assert_eq!(LEAD, 2_000);
    assert!(!device_submission_limit_schedule_allowed(50_000, None), "the keys must be scheduled");
    assert!(!device_submission_limit_schedule_allowed(29_999, Some(28_000)));
    assert!(device_submission_limit_schedule_allowed(30_000, Some(28_000)));
    assert!(!device_submission_limit_schedule_allowed(30_000, Some(u64::MAX)), "no overflow");
    assert!(!device_submission_limit_schedule_allowed(10, Some(10)), "not in the same block");
}

#[test]
fn schedulable_flags() {
    for node in [None, Some("ab".repeat(32))] {
        assert!(ConsensusFeature::DeviceSubmissionLimit.schedulable(node.as_deref()));
        assert!(ConsensusFeature::StationSubmitterKey.schedulable(node.as_deref()));
        assert!(!ConsensusFeature::LateDataPay.schedulable(node.as_deref()), "late-data pay stays held");
    }
}

#[test]
fn the_dependency_check() {
    // Nothing relevant named: admitted.
    assert!(check_device_submission_limit_dependency(&extension(&[("transfer_fee_burn", 9)]), None, None).is_ok());
    // Both in one extension, far enough apart: admitted; too close: refused.
    assert!(check_device_submission_limit_dependency(
        &extension(&[("station_submitter_key", 28_000), ("device_submission_limit", 30_000)]),
        None,
        None
    )
    .is_ok());
    assert_eq!(
        too_early(check_device_submission_limit_dependency(
            &extension(&[("station_submitter_key", 28_001), ("device_submission_limit", 30_000)]),
            None,
            None
        )),
        Some((30_000, Some(28_001)))
    );
    // The limit alone, keys never scheduled: refused.
    assert_eq!(
        too_early(check_device_submission_limit_dependency(&extension(&[("device_submission_limit", 30_000)]), None, None)),
        Some((30_000, None))
    );
    // The limit alone, keys already scheduled on the node: the node's height counts.
    assert!(check_device_submission_limit_dependency(
        &extension(&[("device_submission_limit", 30_000)]),
        Some(27_000),
        None
    )
    .is_ok());
    assert!(too_early(check_device_submission_limit_dependency(
        &extension(&[("device_submission_limit", 30_000)]),
        Some(29_000),
        None
    ))
    .is_some());
    // The keys alone: fine with no limit scheduled; checked against a scheduled limit.
    assert!(check_device_submission_limit_dependency(&extension(&[("station_submitter_key", 40_000)]), None, None).is_ok());
    assert!(check_device_submission_limit_dependency(
        &extension(&[("station_submitter_key", 40_000)]),
        None,
        Some(42_000)
    )
    .is_ok());
    assert_eq!(
        too_early(check_device_submission_limit_dependency(
            &extension(&[("station_submitter_key", 40_001)]),
            None,
            Some(42_000)
        )),
        Some((42_000, Some(40_001)))
    );
}

#[test]
fn admission_policy_runs_the_dependency_check() {
    // This process schedules nothing, so the node's schedule names neither.
    assert!(too_early(check_admission_policy(&extension(&[("device_submission_limit", 30_000)]), None)).is_some());
    assert!(check_admission_policy(
        &extension(&[("station_submitter_key", 28_000), ("device_submission_limit", 30_000)]),
        None
    )
    .is_ok());
    assert!(check_admission_policy(&extension(&[("station_submitter_key", 28_000)]), None).is_ok());
    // late_data_pay is refused by name (held).
    assert!(matches!(
        check_admission_policy(&extension(&[("late_data_pay", 30_000)]), None),
        Err(ScheduleExtensionError::FeatureNotSchedulable { .. })
    ));
    // The message names both features.
    let err = check_admission_policy(&extension(&[("device_submission_limit", 30_000)]), None).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("device_submission_limit") && text.contains("station_submitter_key"), "{text}");
}

/// The consensus half: whatever schedule a block carries (even one a custom
/// producer included past admission), the limit is in force only once the
/// key flag has been active for the lead.
#[test]
fn the_limit_is_dormant_until_the_keys_have_led_by_2000_blocks() {
    let schedule = |keys: Option<u64>, limit: u64| {
        move |f: ConsensusFeature, h: u64| match f {
            ConsensusFeature::DeviceSubmissionLimit => h >= limit,
            ConsensusFeature::StationSubmitterKey => keys.is_some_and(|k| h >= k),
            _ => false,
        }
    };
    // No key flag: never in force.
    for h in [0, 1, 5_000, u64::MAX] {
        assert!(!device_submission_limit_active_with(schedule(None, 1), h));
    }
    // Scheduled too early (keys at 10, limit at 1_000): dormant until 2_010.
    assert!(!device_submission_limit_active_with(schedule(Some(10), 1_000), 1_000));
    assert!(!device_submission_limit_active_with(schedule(Some(10), 1_000), 2_009));
    assert!(device_submission_limit_active_with(schedule(Some(10), 1_000), 2_010));
    // Properly led (keys at 28_000, limit at 30_000): in force from 30_000.
    assert!(!device_submission_limit_active_with(schedule(Some(28_000), 30_000), 29_999));
    assert!(device_submission_limit_active_with(schedule(Some(28_000), 30_000), 30_000));
    // Key flag from genesis: no exemption, the full lead still applies.
    for h in [0, 1, 5, 1_999] {
        assert!(!device_submission_limit_active_with(schedule(Some(0), 5), h), "keys at 0, limit at 5, h {h}");
    }
    assert!(device_submission_limit_active_with(schedule(Some(0), 5), 2_000));
    assert!(device_submission_limit_active_with(schedule(Some(0), 0), 2_000));
    assert!(!device_submission_limit_active_with(schedule(Some(0), 0), 1_999));
    // Keys at 1: in force from 2_001, not 2_000.
    assert!(!device_submission_limit_active_with(schedule(Some(1), 1), 2_000));
    assert!(device_submission_limit_active_with(schedule(Some(1), 1), 2_001));
    // Nothing scheduled in this process: dormant.
    assert!(!device_submission_limit_active(100));
}
