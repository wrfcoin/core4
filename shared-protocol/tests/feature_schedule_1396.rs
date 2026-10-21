//! #3092 (finishes #1396): consensus feature activation schedule as genesis
//! content. Pure-API tests — nothing here installs a process-wide global, so
//! every test is independent and order-free.

use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ActivationScheduleError, ActivationTableError, ChainIdentity, ConsensusFeature,
    FeatureSchedule, GENESIS_ACTIVATION_SCHEDULE_KEY, GENESIS_PROTOCOL_VERSION_KEY,
    SigningProtocolVersion, parse_activation_schedule, parse_genesis_activation_schedule,
    resolve_activation_schedule, resolve_feature_schedule,
    resolve_uncut_chain_protocol_with_schedule,
};

use ConsensusFeature::{DomainSeparatedPreimages as DSP, SigningV2};
use SigningProtocolVersion::{V1, V2};

const KEY: &str = GENESIS_ACTIVATION_SCHEDULE_KEY;

fn schedule(raw: &str, genesis_version: SigningProtocolVersion) -> FeatureSchedule {
    let metadata = [(KEY, raw)];
    let table = resolve_activation_schedule(metadata, genesis_version).expect("table");
    resolve_feature_schedule(metadata, &table).expect("features")
}

// ---------------------------------------------------------------------------
// Parsing: versions and named features share one genesis string
// ---------------------------------------------------------------------------

#[test]
fn a_mixed_schedule_parses_into_version_steps_and_named_features() {
    let parsed = parse_genesis_activation_schedule(
        "0:v1, 1000:v2 ,5000:domain_separated_preimages,7000:future_feature_9",
    )
    .expect("parses");
    assert_eq!(parsed.versions, vec![(0, V1), (1000, V2)]);
    assert_eq!(
        parsed.features,
        vec![
            ("domain_separated_preimages".to_string(), 5000),
            ("future_feature_9".to_string(), 7000)
        ]
    );
}

#[test]
fn a_version_looking_token_must_be_a_version_this_binary_knows() {
    for (raw, token) in [("0:v1,10:v9", "v9"), ("0:v1,10:3", "3"), ("0:V7", "V7")] {
        assert_eq!(
            parse_genesis_activation_schedule(raw),
            Err(ActivationScheduleError::UnknownVersion {
                raw: token.to_string()
            }),
            "{raw}"
        );
    }
}

#[test]
fn the_signing_version_is_not_schedulable_as_a_feature_name() {
    match parse_genesis_activation_schedule("0:v1,10:signing_v2") {
        Err(ActivationScheduleError::Malformed { reason, .. }) => {
            assert!(reason.contains("scheduled as v2"), "{reason}")
        },
        other => panic!("expected Malformed, got {other:?}"),
    }
}

#[test]
fn a_token_that_is_neither_version_nor_identifier_is_malformed() {
    for raw in ["0:v1,10:Bad-Name", "0:v1,10:9lives", "0:v1,10:has space", "0:v1,10:UPPER"] {
        match parse_genesis_activation_schedule(raw) {
            Err(ActivationScheduleError::Malformed { reason, .. }) => {
                assert!(reason.contains("neither a version"), "{raw}: {reason}")
            },
            other => panic!("{raw}: expected Malformed, got {other:?}"),
        }
    }
}

#[test]
fn the_version_table_parser_ignores_feature_entries() {
    let table = parse_activation_schedule("0:v1,100:v2,50:domain_separated_preimages").unwrap();
    assert_eq!(table.version_at(99), V1);
    assert_eq!(table.version_at(100), V2);
    assert_eq!(table.entries(), &[(0, V1), (100, V2)]);
}

#[test]
fn a_features_only_schedule_leaves_the_scalar_at_every_height() {
    for genesis in [V1, V2] {
        let table = resolve_activation_schedule([(KEY, "50:domain_separated_preimages")], genesis)
            .expect("features-only is valid");
        for h in [0u64, 49, 50, 51, u64::MAX] {
            assert_eq!(table.version_at(h), genesis, "genesis {genesis} at {h}");
        }
    }
}

// ---------------------------------------------------------------------------
// Validation reuses ActivationTableError; features add DuplicateFeature
// ---------------------------------------------------------------------------

#[test]
fn validation_errors_reuse_the_activation_table_errors() {
    assert_eq!(
        parse_activation_schedule("5:v1"),
        Err(ActivationScheduleError::Invalid(ActivationTableError::MissingGenesisEntry {
            first_height: 5
        }))
    );
    assert_eq!(
        parse_activation_schedule("0:v1,10:v2,10:v2"),
        Err(ActivationScheduleError::Invalid(ActivationTableError::NonIncreasingHeight {
            previous: 10,
            next: 10
        }))
    );
    assert_eq!(
        parse_activation_schedule("0:v2,10:v1"),
        Err(ActivationScheduleError::Invalid(ActivationTableError::VersionRegression {
            height: 10,
            previous: V2,
            next: V1
        }))
    );
}

#[test]
fn a_feature_scheduled_twice_is_refused() {
    assert_eq!(
        FeatureSchedule::from_entries(vec![("x".into(), 10), ("y".into(), 5), ("x".into(), 20)]),
        Err(ActivationTableError::DuplicateFeature {
            feature: "x".into()
        })
    );
    let metadata = [(KEY, "0:v1,10:domain_separated_preimages,20:domain_separated_preimages")];
    let table = resolve_activation_schedule(metadata, V1).unwrap();
    assert_eq!(
        resolve_feature_schedule(metadata, &table),
        Err(ActivationScheduleError::Invalid(ActivationTableError::DuplicateFeature {
            feature: "domain_separated_preimages".into()
        }))
    );
}

// ---------------------------------------------------------------------------
// active() flips exactly at H; SigningV2 is derived from the version table
// ---------------------------------------------------------------------------

#[test]
fn active_flips_exactly_at_the_activation_height() {
    let s = schedule("0:v1,100:v2,300:domain_separated_preimages", V1);
    assert!(!s.active(SigningV2, 99));
    assert!(s.active(SigningV2, 100));
    assert!(s.active(SigningV2, u64::MAX));
    assert!(!s.active(DSP, 299));
    assert!(s.active(DSP, 300));
    assert_eq!(s.activation_height("signing_v2"), Some(100));
    assert_eq!(s.activation_height("domain_separated_preimages"), Some(300));
    assert_eq!(s.activation_height("never_scheduled"), None);
    assert_eq!(
        s.entries(),
        &[
            ("signing_v2".to_string(), 100),
            ("domain_separated_preimages".to_string(), 300)
        ]
    );
}

#[test]
fn genesis_time_features_are_active_from_height_zero() {
    let s = schedule("0:v2,0:domain_separated_preimages", V2);
    assert!(s.active(SigningV2, 0));
    assert!(s.active(DSP, 0));
    // Sorted by height then name.
    assert_eq!(s.entries()[0].0, "domain_separated_preimages");
    assert_eq!(s.entries()[1].0, "signing_v2");
}

#[test]
fn the_feature_view_and_the_version_table_agree_at_every_height() {
    for raw in ["0:v1,42:v2", "0:v2", "0:v1", "0:v1,7:v2,9:domain_separated_preimages"] {
        let genesis = parse_activation_schedule(raw).unwrap().version_at(0);
        let metadata = [(KEY, raw)];
        let table = resolve_activation_schedule(metadata, genesis).unwrap();
        let features = resolve_feature_schedule(metadata, &table).unwrap();
        for h in (0u64..120).chain([u64::MAX]) {
            assert_eq!(
                table.version_at(h) >= V2,
                features.active(SigningV2, h),
                "{raw} at height {h}"
            );
        }
        assert_eq!(features.activation_height("signing_v2"), table.activation_height_of(V2));
    }
}

// ---------------------------------------------------------------------------
// Unknown features parse and are reportable (the #3093 seam)
// ---------------------------------------------------------------------------

#[test]
fn a_schedule_naming_an_unknown_feature_parses_and_first_unknown_reports_it() {
    let s = schedule("0:v1,10:zz_future,5:aa_future,20:domain_separated_preimages", V1);
    assert_eq!(s.entries().len(), 3);
    // Lowest height first, regardless of input order.
    assert_eq!(
        s.first_unknown(ConsensusFeature::ALL),
        Some(("aa_future".to_string(), 5))
    );
    // Nothing known at all: the lowest-height entry of any name.
    assert_eq!(s.first_unknown(&[]), Some(("aa_future".to_string(), 5)));
    // Every scheduled feature known: nothing to report.
    let known_only = schedule("0:v1,10:v2,20:domain_separated_preimages", V1);
    assert_eq!(known_only.first_unknown(ConsensusFeature::ALL), None);
    assert_eq!(
        known_only.first_unknown(&[SigningV2]),
        Some(("domain_separated_preimages".to_string(), 20))
    );
}

#[test]
fn feature_names_round_trip_through_parse_and_display() {
    for f in ConsensusFeature::ALL {
        assert_eq!(ConsensusFeature::parse(f.name()), Some(*f));
        assert_eq!(f.to_string(), f.name());
    }
    assert_eq!(ConsensusFeature::parse("no_such_feature"), None);
    assert_eq!(serde_json::to_string(&DSP).unwrap(), "\"domain_separated_preimages\"");
}

// ---------------------------------------------------------------------------
// from_genesis: the block is the source
// ---------------------------------------------------------------------------

#[test]
fn from_genesis_metadata_reads_the_scalar_version_when_no_schedule_is_recorded() {
    let v2 = FeatureSchedule::from_genesis_metadata([(GENESIS_PROTOCOL_VERSION_KEY, "v2")]).unwrap();
    assert!(v2.active(SigningV2, 0));
    assert!(!v2.active(DSP, 0));

    let v1 = FeatureSchedule::from_genesis_metadata([(GENESIS_PROTOCOL_VERSION_KEY, "v1")]).unwrap();
    assert!(v1.entries().is_empty());
    assert!(!v1.active(SigningV2, u64::MAX));

    let nothing: [(&str, &str); 0] = [];
    assert!(FeatureSchedule::from_genesis_metadata(nothing).unwrap().entries().is_empty());

    match FeatureSchedule::from_genesis_metadata([(GENESIS_PROTOCOL_VERSION_KEY, "v7")]) {
        Err(ActivationScheduleError::GenesisVersion(_)) => {},
        other => panic!("expected GenesisVersion, got {other:?}"),
    }
}

#[test]
fn from_genesis_reads_the_schedule_out_of_block_one() {
    let mut block = Block::new(3, Default::default(), vec![], 1, 1);
    block
        .metadata
        .insert(GENESIS_PROTOCOL_VERSION_KEY.to_string(), "v1".to_string());
    block.metadata.insert(
        KEY.to_string(),
        "0:v1,50:v2,80:domain_separated_preimages,90:not_yet_implemented".to_string(),
    );
    let s = FeatureSchedule::from_genesis(&block).expect("schedule from block");
    assert!(!s.active(SigningV2, 49) && s.active(SigningV2, 50));
    assert!(!s.active(DSP, 79) && s.active(DSP, 80));
    assert_eq!(
        s.first_unknown(ConsensusFeature::ALL),
        Some(("not_yet_implemented".to_string(), 90))
    );

    let bare = Block::new(3, Default::default(), vec![], 1, 1);
    assert!(FeatureSchedule::from_genesis(&bare).unwrap().entries().is_empty());
}

// ---------------------------------------------------------------------------
// Uncut chain: the cut-time schedule is validated like a genesis-borne one
// ---------------------------------------------------------------------------

fn identity() -> ChainIdentity {
    ChainIdentity::new("wrfcoin-test-3092", "testnet-3092")
}

#[test]
fn an_uncut_chain_resolves_version_table_and_features_together() {
    let resolved = resolve_uncut_chain_protocol_with_schedule(
        Some(V2),
        &identity(),
        Some("0:v2,50:domain_separated_preimages"),
    )
    .expect("valid cut");
    assert_eq!(resolved.version, V2);
    assert_eq!(resolved.table.version_at(0), V2);
    assert!(resolved.features.active(SigningV2, 0));
    assert!(!resolved.features.active(DSP, 49));
    assert!(resolved.features.active(DSP, 50));
    assert_eq!(
        resolved.schedule_raw.as_deref(),
        Some("0:v2,50:domain_separated_preimages")
    );
}

#[test]
fn an_uncut_chain_with_no_schedule_is_the_identity_schedule() {
    let resolved =
        resolve_uncut_chain_protocol_with_schedule(Some(V1), &identity(), None).expect("valid");
    assert_eq!(resolved.version, V1);
    assert_eq!(resolved.table.version_at(u64::MAX), V1);
    assert!(resolved.features.entries().is_empty());
    assert_eq!(resolved.schedule_raw, None);
    // An empty or whitespace declaration is "no schedule", not an error.
    let blank =
        resolve_uncut_chain_protocol_with_schedule(Some(V1), &identity(), Some("  ")).expect("blank");
    assert_eq!(blank, resolved);
}

#[test]
fn a_features_only_cut_schedule_keeps_the_configured_version_everywhere() {
    let resolved = resolve_uncut_chain_protocol_with_schedule(
        Some(V2),
        &identity(),
        Some("10:domain_separated_preimages"),
    )
    .expect("valid");
    assert_eq!(resolved.table.version_at(0), V2);
    assert!(resolved.features.active(SigningV2, 0));
    assert!(resolved.features.active(DSP, 10));
}

#[test]
fn a_cut_schedule_that_disagrees_with_the_configured_version_is_refused() {
    let err = resolve_uncut_chain_protocol_with_schedule(Some(V1), &identity(), Some("0:v2"))
        .expect_err("v1 configured, schedule starts at v2");
    assert!(err.to_string().contains("one chain cannot start at two versions"), "{err}");
    let err = resolve_uncut_chain_protocol_with_schedule(Some(V1), &identity(), Some("0:v1,5:v9"))
        .expect_err("unknown version");
    assert!(err.to_string().contains("unknown protocol version"), "{err}");
}
