//! What this binary implements, and the minimum-version / known-vulnerable
//! registry #2157 assumed (#3093).
//!
//! Three questions a node answers from here, all pure and data-driven:
//!
//! 1. **Which consensus protocol version is this binary?** [`PROTOCOL_VERSION`].
//!    Bumped by any story that adds a consensus feature this binary
//!    implements; recorded on disk at every boot (`META` table, see
//!    `native-chain/src/storage.rs`) so a later boot can see what ran before.
//! 2. **Can this binary follow the chain's genesis schedule?**
//!    [`upgrade_required_at`] reads the `protocol_activation_schedule` genesis
//!    string and reports the lowest-height feature it does not implement —
//!    the activation at which an old binary must stop cleanly instead of
//!    forking silently (#1972 class).
//! 3. **Is a rollback safe?** [`rollback_decision`] refuses a boot on a version
//!    older than the one that last ran the volume when that older version is
//!    in [`KNOWN_VULNERABLE`] — a committed, reviewable table.
//!
//! The feature-name list ([`KNOWN_FEATURES`]) is by name so this module has no
//! dependency on the feature enum #3092 introduces; the two must agree, and
//! `known_features_match_the_feature_enum` in `chain_binding` pins that once
//! both are on `main`.

use serde::{Deserialize, Serialize};

use super::chain_binding::GENESIS_ACTIVATION_SCHEDULE_KEY;

/// This binary's consensus protocol version (#3093). `2` was the release of
/// the 2026-09-11/13 cut (the binary constant, not the chain's signing
/// version, which is v1 on 777/testnet). `3` = B3, the first release that
/// implements live schedule extensions (#3277 S5): the rollout's hard gate
/// is all three nodes reporting `local_protocol_version: 3` before
/// `H_ENABLE`, and the S5 upgrade notice names `min_protocol_version = 3`.
pub const PROTOCOL_VERSION: u32 = 3;

/// A consensus protocol version, comparable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProtocolVersion(pub u32);

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "protocol v{}", self.0)
    }
}

/// [`PROTOCOL_VERSION`] as a [`ProtocolVersion`].
pub const CURRENT: ProtocolVersion = ProtocolVersion(PROTOCOL_VERSION);

/// The oldest protocol version whose volumes this binary still boots.
pub const MIN_SUPPORTED: ProtocolVersion = ProtocolVersion(1);

/// Every scheduled consensus feature this binary implements, by the stable
/// name the genesis schedule carries. Must equal the feature enum's names
/// (#3092 `ConsensusFeature::ALL`).
pub const KNOWN_FEATURES: &[&str] = &[
    "signing_v2",
    "domain_separated_preimages",
    "account_authorization_dispatch",
    "social_recovery",
    "session_keys",
    "verify_unit_pricing",
    "passkey_p256_auth",
    "paymaster",
    "reward_gross_outlier_guard",
    "reference_sea_level_pressure",
    "station_owner_slash_identity",
    "faucet_sink_binding",
    "weather_skew_block_time_allowance",
    "schedule_extension",
    "forecast_skill_baseline",
    "station_owner_consent",
    "station_active_reward_gate",
    "station_owner_actions",
    "station_registration_nonce",
    "stake_submission_nonce",
    "slash_evidence_reports",
    "node_share_settlement",
    "forecaster_key_binding",
    "device_submission_limit",
    "streak_replay_node_service",
    "station_coordinates_fixed_point",
    "transfer_fee_burn",
    "security_key_spend",
    "send_allowlist",
];

/// One entry of the known-vulnerable registry: a version that must never be
/// booted on a volume a newer version has run, plus the advisory that says why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VulnerableVersion {
    pub version: ProtocolVersion,
    pub advisory: &'static str,
}

/// Committed registry of protocol versions with a known consensus- or
/// safety-relevant defect. **Reviewable data, not code**: adding a row is a
/// one-line diff that names the advisory. Empty today; the rollback check is
/// exercised through [`is_known_vulnerable_in`].
pub const KNOWN_VULNERABLE: &[VulnerableVersion] = &[];

/// Whether `version` is in the committed registry.
pub fn is_known_vulnerable(version: ProtocolVersion) -> bool {
    is_known_vulnerable_in(KNOWN_VULNERABLE, version)
}

/// [`is_known_vulnerable`] over an explicit table.
pub fn is_known_vulnerable_in(table: &[VulnerableVersion], version: ProtocolVersion) -> bool {
    table.iter().any(|v| v.version == version)
}

/// The first scheduled activation this binary cannot follow.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpgradeRequired {
    /// The scheduled feature name this binary has no implementation for.
    pub feature: String,
    /// The activation height. The node must not produce, apply or relay a
    /// block at or beyond it; it stops cleanly at `height - 1`.
    pub height: u64,
}

fn is_version_token(token: &str) -> bool {
    let digits = token
        .strip_prefix('v')
        .or_else(|| token.strip_prefix('V'))
        .unwrap_or(token);
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

/// The lowest-height feature named in a genesis schedule string that is not
/// in [`KNOWN_FEATURES`]. Version tokens (`vN`) are the signing version and
/// are not features; entries that do not parse as `height:token` are skipped
/// (a malformed schedule never reaches this — the boot gate refused it).
pub fn upgrade_required_in_schedule(raw: &str) -> Option<UpgradeRequired> {
    upgrade_required_in_schedule_with(raw, KNOWN_FEATURES)
}

/// [`upgrade_required_in_schedule`] over an explicit known-feature list.
pub fn upgrade_required_in_schedule_with(raw: &str, known: &[&str]) -> Option<UpgradeRequired> {
    let mut lowest: Option<UpgradeRequired> = None;
    for field in raw.split(',') {
        let Some((height_raw, token_raw)) = field.trim().split_once(':') else {
            continue;
        };
        let Ok(height) = height_raw.trim().parse::<u64>() else {
            continue;
        };
        let token = token_raw.trim();
        if token.is_empty() || is_version_token(token) || known.contains(&token) {
            continue;
        }
        let candidate = UpgradeRequired {
            feature: token.to_string(),
            height,
        };
        let replace = match &lowest {
            None => true,
            Some(current) => {
                (height, token) < (current.height, current.feature.as_str())
            },
        };
        if replace {
            lowest = Some(candidate);
        }
    }
    lowest
}

/// The first activation in an already-resolved schedule — typically the
/// EFFECTIVE schedule, genesis ∪ applied extensions (#3277 S3) — whose feature
/// is not in [`KNOWN_FEATURES`]. Entries are sorted by height then name, so
/// the first unknown one is the lowest.
pub fn upgrade_required_in_feature_schedule(
    schedule: &crate::crypto::chain_binding::FeatureSchedule,
) -> Option<UpgradeRequired> {
    upgrade_required_in_feature_schedule_with(schedule, KNOWN_FEATURES)
}

/// [`upgrade_required_in_feature_schedule`] over an explicit known list.
pub fn upgrade_required_in_feature_schedule_with(
    schedule: &crate::crypto::chain_binding::FeatureSchedule,
    known: &[&str],
) -> Option<UpgradeRequired> {
    schedule
        .entries()
        .iter()
        .find(|(name, _)| !known.contains(&name.as_str()))
        .map(|(name, height)| UpgradeRequired {
            feature: name.clone(),
            height: *height,
        })
}

/// [`upgrade_required_in_schedule`] over flattened genesis metadata. Absent
/// key → `None`: a genesis that schedules nothing cannot require an upgrade.
pub fn upgrade_required_at<'a, I>(genesis_metadata: I) -> Option<UpgradeRequired>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    genesis_metadata
        .into_iter()
        .find(|(key, _)| *key == GENESIS_ACTIVATION_SCHEDULE_KEY)
        .and_then(|(_, raw)| upgrade_required_in_schedule(raw))
}

/// Whether a node at `tip_height` must refuse to START given `required`: it
/// already holds a block at or beyond the activation, so it cannot even
/// replay its own tip on the old rules.
pub fn boot_refused(required: &UpgradeRequired, tip_height: u64) -> bool {
    tip_height >= required.height
}

/// The operator-facing message for the clean halt. One line an operator can
/// act on: the feature, the height, this binary's version, and what to do.
pub fn upgrade_required_message(required: &UpgradeRequired, at_height: u64) -> String {
    format!(
        "EXIT_UPGRADE_REQUIRED: the genesis schedule activates feature {:?} at height {}, \
         which this binary ({}) does not implement; refusing to produce, apply or relay block \
         {} (#3093). Install the release announced for height {} — see /network/upgrade or the \
         signed upgrade notice — and restart; the data volume is intact and needs no reset.",
        required.feature, required.height, CURRENT, at_height, required.height
    )
}

/// The outcome of comparing this binary's version with the version recorded
/// by the binary that last ran the volume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RollbackDecision {
    /// Same or newer version, or no record (first boot with this check).
    Proceed,
    /// Older than what last ran, but not in the vulnerable registry: allowed
    /// and logged, because an operator rolling an image back on purpose is
    /// the #3091 recovery path.
    DowngradeNoted { recorded: u32 },
    /// Older than what last ran AND known vulnerable: refuse
    /// (`EXIT_ROLLBACK_REFUSED`).
    Refuse { recorded: u32, running: u32, advisory: &'static str },
}

/// Decide whether to boot given the recorded version, using the committed
/// registry.
pub fn rollback_decision(recorded: Option<u32>) -> RollbackDecision {
    rollback_decision_in(KNOWN_VULNERABLE, CURRENT, recorded)
}

/// [`rollback_decision`] over an explicit table and running version.
pub fn rollback_decision_in(
    table: &[VulnerableVersion],
    running: ProtocolVersion,
    recorded: Option<u32>,
) -> RollbackDecision {
    let Some(recorded) = recorded else {
        return RollbackDecision::Proceed;
    };
    if running.0 >= recorded {
        return RollbackDecision::Proceed;
    }
    match table.iter().find(|v| v.version == running) {
        Some(v) => RollbackDecision::Refuse {
            recorded,
            running: running.0,
            advisory: v.advisory,
        },
        None => RollbackDecision::DowngradeNoted { recorded },
    }
}

/// The operator-facing message for a refused rollback.
pub fn rollback_refused_message(recorded: u32, running: u32, advisory: &str) -> String {
    format!(
        "EXIT_ROLLBACK_REFUSED: this volume was last run by protocol v{recorded}; this binary is \
         protocol v{running}, which is in the known-vulnerable registry ({advisory}). Refusing to \
         boot an older, vulnerable version on a newer volume (#3093, #2157). Install a release \
         at protocol v{recorded} or later."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_features_are_the_feature_enum_names() {
        // Pinned here by name so the two lists cannot drift once #3092's
        // `ConsensusFeature::ALL` is on main: update both together.
        assert_eq!(
            KNOWN_FEATURES,
            &[
                "signing_v2",
                "domain_separated_preimages",
                "account_authorization_dispatch",
                "social_recovery",
                "session_keys",
                "verify_unit_pricing",
                "passkey_p256_auth",
                "paymaster",
                "reward_gross_outlier_guard",
                "reference_sea_level_pressure",
                "station_owner_slash_identity",
                "faucet_sink_binding",
                "weather_skew_block_time_allowance",
                "schedule_extension",
                "forecast_skill_baseline",
                "station_owner_consent",
                "station_active_reward_gate",
                "station_owner_actions",
                "station_registration_nonce",
                "stake_submission_nonce",
                "slash_evidence_reports",
                "node_share_settlement",
                "forecaster_key_binding",
                "device_submission_limit",
                "streak_replay_node_service",
                "station_coordinates_fixed_point",
                "transfer_fee_burn",
                "security_key_spend",
                "send_allowlist"
            ]
        );
    }

    #[test]
    fn version_tokens_are_never_upgrade_requirements() {
        assert_eq!(upgrade_required_in_schedule("0:v1,100:v2,5:V2,7:2"), None);
    }

    #[test]
    fn the_lowest_height_unknown_feature_wins() {
        let r = upgrade_required_in_schedule(
            "0:v1,900:zz_late,100:domain_separated_preimages,300:bb_mid,300:aa_mid",
        )
        .expect("two unknowns");
        assert_eq!(r, UpgradeRequired { feature: "aa_mid".into(), height: 300 });
        assert_eq!(
            upgrade_required_in_schedule_with("0:v1,10:x", &["x"]),
            None,
            "a known feature never requires an upgrade"
        );
    }

    #[test]
    fn rollback_decisions() {
        let table = [VulnerableVersion {
            version: ProtocolVersion(1),
            advisory: "test advisory",
        }];
        assert_eq!(rollback_decision_in(&table, ProtocolVersion(2), None), RollbackDecision::Proceed);
        assert_eq!(rollback_decision_in(&table, ProtocolVersion(2), Some(2)), RollbackDecision::Proceed);
        assert_eq!(rollback_decision_in(&table, ProtocolVersion(3), Some(2)), RollbackDecision::Proceed);
        assert_eq!(
            rollback_decision_in(&table, ProtocolVersion(1), Some(2)),
            RollbackDecision::Refuse { recorded: 2, running: 1, advisory: "test advisory" }
        );
        assert_eq!(
            rollback_decision_in(&[], ProtocolVersion(1), Some(2)),
            RollbackDecision::DowngradeNoted { recorded: 2 }
        );
        assert_eq!(rollback_decision(Some(PROTOCOL_VERSION + 5)), RollbackDecision::DowngradeNoted {
            recorded: PROTOCOL_VERSION + 5
        });
        assert!(!is_known_vulnerable(CURRENT), "the running version must never be in the registry");
        assert!(MIN_SUPPORTED <= CURRENT);
    }

    /// #3277 S5: B3 is protocol version 3 — the release the rollout's hard
    /// gate checks (`/network/upgrade` `local_protocol_version: 3` on all
    /// three nodes before `H_ENABLE`) and the S5 upgrade notice names.
    #[test]
    fn b3_is_protocol_version_3_3277() {
        assert_eq!(PROTOCOL_VERSION, 3);
        assert_eq!(CURRENT, ProtocolVersion(3));
    }
}
