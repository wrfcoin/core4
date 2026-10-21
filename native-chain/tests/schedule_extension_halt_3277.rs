//! #3277 S3, T2: an applied schedule extension naming a feature this binary
//! does not implement installs the #3093 clean-halt deadline at its
//! activation height, the same deadline genesis or a signed notice would —
//! and only ever TIGHTENS it.
//!
//! The deadline is what `/blocks/mine` and the P2P apply path consult
//! (`production_refused_at`); both refuse the activation height and request
//! the exit-70 halt, so the node produces and applies through `H - 1` and
//! stops there with its volume intact. On restart with the tip at or beyond
//! `H`, `boot_refused` refuses to start.
//!
//! Own process: the deadline, the base schedule and the extension overlay are
//! process-wide.

use std::sync::Mutex;

use wrfcoin_native_chain::upgrade::{
    install_upgrade_deadline, production_refused_at, refresh_schedule_from_canonical_state,
    reset_for_tests, tighten_upgrade_deadline, upgrade_deadline, UpgradeDeadline,
};
use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    feature_active, refresh_feature_schedule_extensions, ConsensusFeature,
};
use wrfcoin_shared_protocol::crypto::protocol_support::{boot_refused, PROTOCOL_VERSION};
use wrfcoin_shared_protocol::governance::schedule_extension::{AppliedExtension, ScheduleEntry};

static LOCK: Mutex<()> = Mutex::new(());

fn guard() -> std::sync::MutexGuard<'static, ()> {
    let g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    reset_for_tests();
    refresh_feature_schedule_extensions(Vec::new());
    g
}

/// A canonical state holding one applied extension with `entries`.
fn state_with(entries: &[(&str, u64)]) -> State {
    let mut state = State::default();
    state.feature_schedule_extensions.push(AppliedExtension {
        sequence: 0,
        carried_at_height: 1_000,
        entries: entries.iter().map(|(n, h)| ScheduleEntry::new(*n, *h)).collect(),
        version_steps: Vec::new(),
        min_protocol_version: PROTOCOL_VERSION + 1,
        release_commit: "b4".into(),
        signers: Vec::new(),
    });
    state
}

const H: u64 = 5_000;

#[test]
fn an_unknown_extension_feature_halts_the_node_cleanly_at_its_activation() {
    let _g = guard();
    let state = state_with(&[("a_b4_feature_this_binary_lacks", H)]);
    let installed = refresh_schedule_from_canonical_state(&state, "test").expect("deadline");
    assert_eq!(installed.height, H);
    assert_eq!(installed.feature, "a_b4_feature_this_binary_lacks");
    assert_eq!(upgrade_deadline().map(|d| d.height), Some(H));

    // Produces and applies through H-1; refuses H and beyond.
    assert!(production_refused_at(H - 1).is_none());
    assert!(production_refused_at(H).is_some());
    assert!(production_refused_at(H + 7).is_some());

    // A restart whose tip already reached H refuses to boot; one below it runs.
    let required = wrfcoin_shared_protocol::crypto::protocol_support::UpgradeRequired {
        feature: installed.feature.clone(),
        height: installed.height,
    };
    assert!(!boot_refused(&required, H - 1));
    assert!(boot_refused(&required, H));
}

#[test]
fn a_known_extension_feature_installs_no_deadline_and_activates() {
    let _g = guard();
    let state = state_with(&[("session_keys", H)]);
    assert!(refresh_schedule_from_canonical_state(&state, "test").is_none());
    assert_eq!(upgrade_deadline(), None);
    assert!(!feature_active(ConsensusFeature::SessionKeys, H - 1));
    assert!(feature_active(ConsensusFeature::SessionKeys, H));
}

#[test]
fn the_deadline_only_ever_tightens() {
    let _g = guard();
    // A reorg that drops the extension does not relax the deadline it set:
    // stopping early costs a restart, stopping late is the #1972 fork.
    refresh_schedule_from_canonical_state(&state_with(&[("unknown_later", H)]), "test");
    refresh_schedule_from_canonical_state(&State::default(), "reorg away");
    assert_eq!(upgrade_deadline().map(|d| d.height), Some(H));

    // A later unknown activation cannot loosen it either.
    refresh_schedule_from_canonical_state(&state_with(&[("unknown_even_later", H + 500)]), "test");
    assert_eq!(upgrade_deadline().map(|d| d.height), Some(H));

    // An earlier one tightens it.
    refresh_schedule_from_canonical_state(&state_with(&[("unknown_sooner", H - 500)]), "test");
    assert_eq!(upgrade_deadline().map(|d| d.height), Some(H - 500));

    // And an already-installed earlier deadline from another source (genesis,
    // a signed notice) is never replaced by a later extension deadline.
    install_upgrade_deadline(Some(UpgradeDeadline {
        feature: "from_genesis".into(),
        height: 100,
        running_protocol_version: PROTOCOL_VERSION,
    }));
    assert!(!tighten_upgrade_deadline(
        UpgradeDeadline {
            feature: "from_extension".into(),
            height: H,
            running_protocol_version: PROTOCOL_VERSION,
        },
        "test"
    ));
    assert_eq!(upgrade_deadline().map(|d| d.feature), Some("from_genesis".to_string()));
}
