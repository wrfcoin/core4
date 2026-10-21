//! #3131 activation (#3277 test T7): `ConsensusFeature::RewardGrossOutlierGuard` scheduled at H
//! switches the attester's reward-tier rule at exactly H.
//!
//! The fold runs in the oracle attester, not in block application: nodes
//! store the confidence the attester signed. So the activation is carried to
//! the attester by the node (`GET /transactions/oracle/pending` →
//! `active_features`, computed with `feature_active(_, height + 1)`), and the
//! attester evaluates with `MechanicalPolicy::gross_outlier_guard` set from
//! it. This file drives that exact decision through the real process-wide
//! schedule:
//!   - below H the verdict is the #3098 one, byte-for-byte (replay of
//!     pre-activation heights reproduces the old confidences);
//!   - at H a lone gross Outlier collapses to Outlier;
//!   - inputs the guard does not touch give identical verdicts on both sides;
//!   - the live testnet genesis (2026-09-13 cut, v1, NO schedule key) never
//!     activates it and never halts on it: it can only be scheduled by a
//!     #3277 live schedule extension (planned: E0, with #3270).
//!
//! ONE test: the feature schedule is installed process-wide (first-install-wins
//! `OnceLock`), so this file owns its process, the same discipline as
//! `paymaster_activation_2171` and `passkey_activation_2169`.

use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, FeatureSchedule, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion,
    feature_active, install_activation_table, install_feature_schedule,
    resolve_activation_schedule, resolve_feature_schedule, try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::protocol_support::{
    KNOWN_FEATURES, upgrade_required_at, upgrade_required_in_schedule,
    upgrade_required_in_schedule_with,
};
use wrfcoin_shared_protocol::oracle::attester::PendingObservation;
use wrfcoin_shared_protocol::oracle::mechanical::{MechanicalPolicy, MechanicalVerdict};
use wrfcoin_shared_protocol::weather::quality_tier::ReadingQualityTier;
use wrfcoin_shared_protocol::weather::reference::{ReferenceComparison, ReferenceResidual};

const ACTIVATION_HEIGHT: u64 = 7;
const NOW: i64 = 1_700_000_000;

fn nws(variable: &str, residual: f64) -> ReferenceResidual {
    ReferenceResidual { source: "nws_station".into(), variable: variable.into(), residual }
}

fn observation() -> PendingObservation {
    PendingObservation {
        commit_id: "c".into(),
        station_id: "WX-3131".into(),
        nonce: "n".into(),
        timestamp: NOW - 60,
        data: serde_json::json!({
            "temperature_c": 19.6,
            "humidity_percent": 52.0,
            "pressure_hpa": 1013.0,
            "wind_speed_ms": 4.0
        }),
        contribution_type: "contribution".into(),
    }
}

/// What the attester signs for a reading whose attestation lands at `height`:
/// the policy's guard bit comes from the chain's schedule, nothing else.
fn verdict_at(height: u64, comparison: &ReferenceComparison) -> MechanicalVerdict {
    let policy = MechanicalPolicy {
        gross_outlier_guard: feature_active(ConsensusFeature::RewardGrossOutlierGuard, height),
        ..MechanicalPolicy::default()
    };
    policy.evaluate(&observation(), Some(comparison), NOW)
}

#[test]
fn gross_outlier_guard_activates_at_its_scheduled_height_and_not_before() {
    // ── The live testnet (777/testnet, 2026-09-13 cut) has NO schedule. ────
    // Block 1 carries `protocol_version = "v1"` and no
    // `protocol_activation_schedule` key (design doc 2026-09-24 §0). With
    // that genesis the feature is inactive at every height, and nothing asks
    // this binary to halt: an unscheduled feature is invisible to #3093.
    let live_genesis = [("protocol_version", "v1")];
    let live = FeatureSchedule::from_genesis_metadata(live_genesis).unwrap();
    assert!(live.entries().is_empty(), "the live chain schedules nothing");
    for feature in ConsensusFeature::ALL {
        if *feature != ConsensusFeature::SigningV2 {
            assert!(!live.active(*feature, u64::MAX), "{feature} must be inactive on the live chain");
        }
    }
    assert_eq!(upgrade_required_at(live_genesis), None, "an unscheduled feature never halts a node");

    // ── This binary knows the feature; a pre-#3131 binary would halt on it. ─
    let name = ConsensusFeature::RewardGrossOutlierGuard.name();
    assert_eq!(name, "reward_gross_outlier_guard", "#3277 story S8 fixes this name");
    assert_eq!(ConsensusFeature::parse(name), Some(ConsensusFeature::RewardGrossOutlierGuard));
    assert!(KNOWN_FEATURES.contains(&name));
    let raw = format!("0:v1,{ACTIVATION_HEIGHT}:{name}");
    assert_eq!(upgrade_required_in_schedule(&raw), None);
    let pre_3131_known: Vec<&str> =
        KNOWN_FEATURES.iter().copied().filter(|known| *known != name).collect();
    let required = upgrade_required_in_schedule_with(&raw, &pre_3131_known)
        .expect("a pre-#3131 binary must report the upgrade (#3093 clean halt)");
    assert_eq!((required.feature.as_str(), required.height), (name, ACTIVATION_HEIGHT));

    // ── Schedule the feature at H, as the boot gate would. ──────────────────
    assert!(try_feature_schedule().is_none(), "this file owns its process; no schedule may pre-exist");
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);

    assert!(!feature_active(ConsensusFeature::RewardGrossOutlierGuard, ACTIVATION_HEIGHT - 1));
    assert!(feature_active(ConsensusFeature::RewardGrossOutlierGuard, ACTIVATION_HEIGHT));

    // ── The 2464 reading: 10.4 °C off, every other variable good. ───────────
    let gross = ReferenceComparison {
        residuals: vec![
            nws("temperature_c", -10.4),
            nws("pressure_hpa", -0.2),
            nws("humidity_pct", 2.0),
            nws("wind_speed_ms", -0.2),
        ],
    };
    let before = verdict_at(ACTIVATION_HEIGHT - 1, &gross);
    assert_eq!(before.tier, ReadingQualityTier::Degraded);
    assert_eq!(before.confidence, 0.4958, "pre-activation replay reproduces the #3098 confidence");
    assert_eq!(before, MechanicalPolicy::default().evaluate(&observation(), Some(&gross), NOW));
    let after = verdict_at(ACTIVATION_HEIGHT, &gross);
    assert_eq!(after.tier, ReadingQualityTier::Outlier);
    assert!(after.confidence <= 0.1, "{after:?}");

    // ── Inputs the guard does not touch: identical on both sides of H. ──────
    let untouched = [
        // Near-miss: past marginal_max (6), not past gross_max (9).
        vec![nws("temperature_c", 8.5), nws("humidity_pct", 2.0)],
        // Exactly at the limit: strict >, not gross.
        vec![nws("temperature_c", 9.0), nws("humidity_pct", 2.0)],
        // The live Duluth WS-5000 shape: uncorrected station pressure ~27 hPa
        // below KDLH sea level; pressure has no gross limit.
        vec![
            nws("temperature_c", 1.2),
            nws("pressure_hpa", -28.2),
            nws("humidity_pct", 3.8),
            nws("wind_speed_ms", -3.9),
        ],
        // T7 barometer (#3098's motivating case): 7–8 hPa low against a
        // gridded reference. hrrr pressure marginal_max is 6, so it is the
        // lone Outlier, and hrrr has no gross limit: Degraded on both sides.
        vec![
            ReferenceResidual { source: "hrrr".into(), variable: "temperature_c".into(), residual: 0.4 },
            ReferenceResidual { source: "hrrr".into(), variable: "pressure_hpa".into(), residual: -8.0 },
        ],
        // T7 barometer against KDLH (Steven's live -7.7 hPa): inside the
        // nws_station good band, with his wind offset Degraded.
        vec![
            nws("temperature_c", 1.6),
            nws("pressure_hpa", -7.7),
            nws("humidity_pct", -4.0),
            nws("wind_speed_ms", -5.0),
        ],
        // Two outliers: already Outlier under #3098.
        vec![nws("temperature_c", 12.0), nws("wind_speed_ms", 9.5), nws("humidity_pct", 1.0)],
        // All good.
        vec![nws("temperature_c", 0.4), nws("pressure_hpa", 1.0), nws("humidity_pct", 4.0)],
    ];
    for residuals in untouched {
        let comparison = ReferenceComparison { residuals };
        if comparison.residuals.iter().any(|r| r.variable == "pressure_hpa" && r.residual.abs() >= 7.0) {
            // Every barometer case stays Degraded (or better) after activation.
            assert_ne!(verdict_at(ACTIVATION_HEIGHT, &comparison).tier, ReadingQualityTier::Outlier);
        }
        assert_eq!(
            verdict_at(ACTIVATION_HEIGHT - 1, &comparison),
            verdict_at(ACTIVATION_HEIGHT, &comparison),
            "the guard must not change {comparison:?}"
        );
    }
}
