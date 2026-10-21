//! #3093: the minimum-protocol-version registry and the "can this binary
//! follow the schedule?" question, over public API only.

use wrfcoin_shared_protocol::crypto::chain_binding::GENESIS_ACTIVATION_SCHEDULE_KEY;
use wrfcoin_shared_protocol::crypto::protocol_support::{
    boot_refused, is_known_vulnerable, is_known_vulnerable_in, rollback_decision,
    rollback_decision_in, rollback_refused_message, upgrade_required_at,
    upgrade_required_in_schedule, upgrade_required_message, ProtocolVersion, RollbackDecision,
    UpgradeRequired, VulnerableVersion, CURRENT, KNOWN_FEATURES, KNOWN_VULNERABLE,
    MIN_SUPPORTED, PROTOCOL_VERSION,
};

#[test]
fn the_registry_is_consistent() {
    assert_eq!(CURRENT, ProtocolVersion(PROTOCOL_VERSION));
    assert!(MIN_SUPPORTED <= CURRENT);
    assert!(!is_known_vulnerable(CURRENT), "the shipping version is never vulnerable");
    assert!(KNOWN_FEATURES.contains(&"signing_v2"));
    for entry in KNOWN_VULNERABLE {
        assert!(entry.version < CURRENT, "a vulnerable version must be older than the shipping one");
        assert!(!entry.advisory.is_empty());
    }
}

#[test]
fn a_schedule_with_only_known_features_requires_no_upgrade() {
    assert_eq!(upgrade_required_in_schedule("0:v1,100:v2"), None);
    assert_eq!(
        upgrade_required_in_schedule("0:v1,100:v2,500:domain_separated_preimages"),
        None
    );
    let none: [(&str, &str); 0] = [];
    assert_eq!(upgrade_required_at(none), None, "no schedule key");
    assert_eq!(upgrade_required_at([("chain_id", "777")]), None);
}

#[test]
fn the_lowest_height_unknown_feature_is_the_deadline() {
    let required = upgrade_required_at([(
        GENESIS_ACTIVATION_SCHEDULE_KEY,
        "0:v1,100:v2,900:later_feature,10:earliest_feature,500:domain_separated_preimages",
    )])
    .expect("two unknown features");
    assert_eq!(
        required,
        UpgradeRequired {
            feature: "earliest_feature".into(),
            height: 10
        }
    );
    // Ties on height break on name, deterministically.
    let tie = upgrade_required_in_schedule("0:v1,7:zeta,7:alpha").unwrap();
    assert_eq!(tie.feature, "alpha");
}

#[test]
fn boot_is_refused_only_when_the_tip_already_reached_the_height() {
    let required = UpgradeRequired {
        feature: "future".into(),
        height: 10,
    };
    assert!(!boot_refused(&required, 0));
    assert!(!boot_refused(&required, 9));
    assert!(boot_refused(&required, 10));
    assert!(boot_refused(&required, u64::MAX));
}

#[test]
fn the_halt_message_names_feature_height_version_and_action() {
    let required = UpgradeRequired {
        feature: "future".into(),
        height: 10,
    };
    let message = upgrade_required_message(&required, 10);
    assert!(message.starts_with("EXIT_UPGRADE_REQUIRED"));
    assert!(message.contains("\"future\""));
    assert!(message.contains("height 10"));
    assert!(message.contains(&format!("protocol v{PROTOCOL_VERSION}")));
    assert!(message.contains("Install the release announced for height 10"));
    assert!(message.contains("needs no reset"));
}

#[test]
fn rollback_to_a_known_vulnerable_version_is_refused_and_a_plain_downgrade_is_noted() {
    let table = [VulnerableVersion {
        version: ProtocolVersion(1),
        advisory: "CVE-test: v1 signs unbound preimages",
    }];
    assert!(is_known_vulnerable_in(&table, ProtocolVersion(1)));
    assert!(!is_known_vulnerable_in(&table, ProtocolVersion(2)));

    // First boot with the check, or same/newer version: proceed.
    assert_eq!(rollback_decision_in(&table, ProtocolVersion(1), None), RollbackDecision::Proceed);
    assert_eq!(rollback_decision_in(&table, ProtocolVersion(2), Some(2)), RollbackDecision::Proceed);
    assert_eq!(rollback_decision_in(&table, ProtocolVersion(2), Some(1)), RollbackDecision::Proceed);
    // Older AND vulnerable: refused.
    let refused = rollback_decision_in(&table, ProtocolVersion(1), Some(2));
    assert_eq!(
        refused,
        RollbackDecision::Refuse {
            recorded: 2,
            running: 1,
            advisory: "CVE-test: v1 signs unbound preimages"
        }
    );
    let message = rollback_refused_message(2, 1, "CVE-test: v1 signs unbound preimages");
    assert!(message.starts_with("EXIT_ROLLBACK_REFUSED"));
    assert!(message.contains("last run by protocol v2"));
    assert!(message.contains("CVE-test"));
    // Older but not vulnerable: allowed and noted.
    assert_eq!(
        rollback_decision_in(&[], ProtocolVersion(1), Some(2)),
        RollbackDecision::DowngradeNoted { recorded: 2 }
    );
    // The committed registry is empty today, so the shipping binary only ever
    // notes a downgrade.
    assert_eq!(
        rollback_decision(Some(PROTOCOL_VERSION + 1)),
        RollbackDecision::DowngradeNoted {
            recorded: PROTOCOL_VERSION + 1
        }
    );
}
