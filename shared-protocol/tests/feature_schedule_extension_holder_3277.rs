//! #3277 S3: the effective feature schedule = base (genesis ∪ bootstrap) ∪
//! applied extensions, refreshed from the canonical `State`.
//!
//! Own process: the base is a first-install-wins `OnceLock` and the overlay
//! is process-global, so these tests serialize on one lock and never share a
//! process with tests that assume no schedule is installed.

use std::sync::Mutex;

use rand::rngs::StdRng;
use rand::SeedableRng;
use wrfcoin_shared_protocol::blockchain::chain::MAX_REORG_DEPTH;
use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    effective_feature_schedule, feature_active, feature_schedule_extension_entries,
    install_feature_schedule, refresh_feature_schedule_extensions, try_feature_schedule,
    ConsensusFeature, FeatureSchedule,
};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    sign_feature_schedule_extension_with_key, FeatureScheduleExtension, ScheduleEntry,
    ScheduleExtensionContext, SignedFeatureScheduleExtension, MIN_RUNUP_BLOCKS,
    SCHEDULE_EXTENSION_FEATURE,
};

static LOCK: Mutex<()> = Mutex::new(());

const H_ENABLE: u64 = 100;
const GENESIS: &str = "abababababababababababababababababababababababababababababababab";

/// Base: the extension gate at H_ENABLE and `session_keys` at 50.
fn install_base() {
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            (SCHEDULE_EXTENSION_FEATURE.to_string(), H_ENABLE),
            ("session_keys".to_string(), 50),
        ])
        .unwrap(),
    );
    assert_eq!(
        try_feature_schedule().unwrap().activation_height("session_keys"),
        Some(50),
        "this file owns its process's base schedule"
    );
}

fn guard() -> std::sync::MutexGuard<'static, ()> {
    let g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    install_base();
    refresh_feature_schedule_extensions(Vec::new());
    g
}

fn governance() -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(3277))
}

fn signed(sequence: u64, entries: &[(&str, u64)]) -> SignedFeatureScheduleExtension {
    let extension = FeatureScheduleExtension {
        chain_id: "777".into(),
        network_id: "testnet".into(),
        genesis_hash: GENESIS.into(),
        sequence,
        entries: entries.iter().map(|(n, h)| ScheduleEntry::new(*n, *h)).collect(),
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "s3".into(),
    };
    let signatures = vec![sign_feature_schedule_extension_with_key(&extension, &governance())
            .expect("governance signs a non-held extension")];
    SignedFeatureScheduleExtension { extension, signatures }
}

/// Apply through the real S2 arm with an explicit authority and the base.
fn apply(state: &mut State, ext: &SignedFeatureScheduleExtension, carried_at: u64) {
    let allow = vec![PublicKey::from_private_key(&governance()).to_hex().to_ascii_lowercase()];
    let ctx = ScheduleExtensionContext {
        chain_id: "777",
        network_id: "testnet",
        genesis_hash: GENESIS,
        allowlist: &allow,
    };
    let base = State::base_feature_schedule();
    state
        .apply_feature_schedule_extension_with(ext, carried_at, &ctx, &base)
        .expect("extension applies");
}

#[test]
fn an_applied_extension_activates_only_after_a_refresh_from_state() {
    let _g = guard();
    let mut state = State::default();
    let carried = 1_000;
    let at = carried + MIN_RUNUP_BLOCKS;
    apply(&mut state, &signed(0, &[("paymaster", at)]), carried);

    // Applying to a State alone never touches the process-wide answer.
    assert!(!feature_active(ConsensusFeature::Paymaster, at + 10));
    assert!(feature_schedule_extension_entries().is_empty());

    state.refresh_effective_feature_schedule();
    assert!(!feature_active(ConsensusFeature::Paymaster, at - 1));
    assert!(feature_active(ConsensusFeature::Paymaster, at));
    assert_eq!(
        effective_feature_schedule().activation_height("paymaster"),
        Some(at)
    );
    // The base is untouched by the overlay.
    assert_eq!(try_feature_schedule().unwrap().activation_height("paymaster"), None);

    // A refresh from a state WITHOUT the extension (a reorg away from it)
    // removes it — the overlay is replaced, never merged.
    State::default().refresh_effective_feature_schedule();
    assert!(!feature_active(ConsensusFeature::Paymaster, at + 10));
    assert_eq!(effective_feature_schedule().activation_height("paymaster"), None);
}

#[test]
fn the_overlay_can_never_move_a_base_feature() {
    let _g = guard();
    // Even if an overlay entry named a base feature (V6 forbids it in state),
    // the base answer stands: session_keys stays at 50, not 10.
    refresh_feature_schedule_extensions(vec![("session_keys".into(), 10)]);
    assert!(!feature_active(ConsensusFeature::SessionKeys, 20));
    assert!(feature_active(ConsensusFeature::SessionKeys, 50));
    assert_eq!(effective_feature_schedule().activation_height("session_keys"), Some(50));
    refresh_feature_schedule_extensions(Vec::new());
}

#[test]
fn unknown_extension_features_are_reported_by_first_unknown() {
    let _g = guard();
    // `schedule_extension` itself becomes a ConsensusFeature in S5; until
    // then the base's gate is the lowest unknown, so skip past it by height.
    let lowest_unknown = |schedule: &FeatureSchedule| {
        schedule
            .entries()
            .iter()
            .find(|(name, _)| {
                name != SCHEDULE_EXTENSION_FEATURE
                    && !ConsensusFeature::ALL.iter().any(|k| k.name() == name)
            })
            .cloned()
    };
    refresh_feature_schedule_extensions(vec![("not_a_feature_this_binary_has".into(), 9_000)]);
    assert_eq!(
        lowest_unknown(&effective_feature_schedule()),
        Some(("not_a_feature_this_binary_has".to_string(), 9_000))
    );
    refresh_feature_schedule_extensions(Vec::new());
    assert_eq!(lowest_unknown(&effective_feature_schedule()), None);
}

/// T3 at the schedule level (design §1.5): an extension carried on ONE branch
/// only. At every height a node can evaluate while the branches are within
/// reorg reach of each other, both branches' effective schedules give the
/// SAME `feature_active` answers, so a reorg can never flip a decision
/// already taken. The property holds only because MIN_RUNUP_BLOCKS >
/// MAX_REORG_DEPTH: with a run-up of 100 or less, the branch-only entry
/// activates inside reorg reach and this test fails.
#[test]
fn branch_only_extension_is_invisible_within_reorg_reach() {
    let _g = guard();
    let fork_point = 2_000;
    let carried = fork_point + 1;
    let activation = carried + MIN_RUNUP_BLOCKS;

    let common = State::default();
    let mut branch_a = common.clone();
    apply(&mut branch_a, &signed(0, &[("paymaster", activation)]), carried);
    let branch_b = common;

    // Every height evaluable while A and B are within reorg reach: from the
    // fork point until the fork point sinks MAX_REORG_DEPTH below the tip.
    for height in fork_point..=carried + MAX_REORG_DEPTH {
        branch_a.refresh_effective_feature_schedule();
        let on_a = feature_active(ConsensusFeature::Paymaster, height);
        branch_b.refresh_effective_feature_schedule();
        let on_b = feature_active(ConsensusFeature::Paymaster, height);
        assert_eq!(
            on_a, on_b,
            "height {height}: branch-only extension changed a decision within reorg reach"
        );
    }
    refresh_feature_schedule_extensions(Vec::new());
}
