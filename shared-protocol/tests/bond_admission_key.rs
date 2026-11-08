//! `bond_admission_key`: a pinned key may co-sign a node-bond Deposit or Update
//! from activation, and can do nothing else.
//!
//! This binary installs its own process-wide schedule (node-share settlement at
//! `A` = 3, `bond_admission_key` at `K` = 5) and pins two synthetic keys for the
//! fixture's genesis, so heights 3 and 4 are pre-activation and 5 onward active.
//! The chain with no pinned key is `bond_admission_key_unpinned.rs`.

#[path = "common/node_bond_fixture.rs"]
#[allow(dead_code)]
mod fixture;

use fixture::{A, address, advance, apply, key, pk};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::common::types::Balance;
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::node_bond_registry::{
    NODE_BOND_UNITS, NodeBondLifecycle, bond_admission_key_active,
};
use wrfcoin_shared_protocol::consensus::node_share_contracts::{
    NodeBondAction, NodeBondAuthorization, node_epoch_blocks,
};
use wrfcoin_shared_protocol::consensus::verify_cost::VerifyCost;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, FeatureSchedule, SigningProtocolVersion, feature_active,
    install_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::domain_signing::{
    is_authorized_oracle_governance, sign_domain_for,
};
use wrfcoin_shared_protocol::crypto::protocol_support::upgrade_required_in_feature_schedule;
use wrfcoin_shared_protocol::governance::schedule_extension::{
    AppliedExtension, FeatureScheduleExtension, MIN_RUNUP_BLOCKS, ScheduleEntry,
    ScheduleExtensionError, SignedFeatureScheduleExtension, bond_admission_keys,
    check_admission_policy, install_bond_admission_pin_for_tests,
    install_schedule_extension_authority_for_tests, schedule_extension_authority,
    sign_feature_schedule_extension_with_key, validate_extension_against_state,
    verify_feature_schedule_extension,
};
use wrfcoin_shared_protocol::governance::upgrade_notice::{
    SignedUpgradeNotice, UpgradeNotice, UpgradeNoticeError, sign_upgrade_notice_with_key,
    verify_upgrade_notice,
};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

const K: u64 = A + 2;
const GOVERNANCE: u8 = 99;
const ADMISSION: u8 = 60;
const SPARE: u8 = 61;
const WRONG: u8 = 62;

fn genesis_hex() -> String {
    hex::encode([7u8; 32])
}

fn identity() -> ChainIdentity {
    ChainIdentity::new("777", "testnet")
}

fn governance_keys() -> Vec<String> {
    vec![hex::encode(pk(GOVERNANCE))]
}

fn install() {
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("node_share_settlement".into(), A),
            ("account_authorization_dispatch".into(), A),
            ("bond_admission_key".into(), K),
        ])
        .unwrap(),
    );
    install_schedule_extension_authority_for_tests(
        "777",
        "testnet",
        &genesis_hex(),
        governance_keys(),
    );
    install_bond_admission_pin_for_tests(
        &genesis_hex(),
        vec![hex::encode(pk(ADMISSION)), hex::encode(pk(SPARE))],
    );
}

fn state() -> State {
    install();
    let mut s = State::new();
    s.genesis_emission_config = Some(
        wrfcoin_shared_protocol::crypto::chain_binding::GenesisEmissionConfig {
            chain_id: "777".into(),
            faucet_address: None,
            ..Default::default()
        },
    );
    s.feature_schedule_extensions.push(AppliedExtension {
        sequence: 0,
        carried_at_height: 0,
        entries: vec![ScheduleEntry::new("node_share_settlement", A)],
        version_steps: vec![],
        min_protocol_version: 2,
        release_commit: "test-only-explicit-schedule".into(),
        signers: governance_keys(),
    });
    for owner in [10, 20, 58, 59, 60] {
        s.create_account(
            address(owner),
            Balance::new(u128::from(NODE_BOND_UNITS) * 3),
        );
    }
    s.apply_transition(StateTransition::BeginNodeServiceBlock {
        block_height: A,
        parent_hash: [2; 32],
    })
    .unwrap();
    s.apply_block_finalization(A);
    s
}

fn sign(a: &NodeBondAuthorization, signer: u8) -> Vec<u8> {
    fixture::signature(a, signer)
}

/// The fixture's lifecycle with the co-sign replaced by `cosigner`'s.
fn cosigned(
    owner: u8,
    id: u8,
    nonce: u64,
    action: NodeBondAction,
    epoch: u64,
    cosigner: u8,
) -> NodeBondLifecycle {
    let mut p = fixture::auth(owner, id, nonce, action, epoch);
    p.governance_key = Some(pk(cosigner));
    p.governance_signature = sign(&p.signed.authorization, cosigner);
    p
}

fn verify_at(p: &NodeBondLifecycle, height: u64) -> bool {
    p.verify(&identity(), &[7; 32], &governance_keys(), height)
        .is_ok()
}

fn root(s: &mut State) -> wrfcoin_shared_protocol::common::types::Hash {
    s.recalculate_state_root();
    s.state_root
}

#[test]
fn feature_is_registered_appended_and_schedulable_only_where_a_key_is_pinned() {
    install();
    let f = ConsensusFeature::BondAdmissionKey;
    assert_eq!(f.name(), "bond_admission_key");
    assert_eq!(ConsensusFeature::parse("bond_admission_key"), Some(f));
    // Ordinal 30; later features append after it (31 is reserved, 32 is
    // `station_registrar_key`).
    assert_eq!(ConsensusFeature::ALL[30], f);
    assert_eq!(bincode::serialize(&f).unwrap(), 30u32.to_le_bytes());
    assert!(f.has_behaviour());
    assert!(
        wrfcoin_shared_protocol::crypto::protocol_support::KNOWN_FEATURES
            .contains(&"bond_admission_key")
    );
    assert!(!FeatureSchedule::default().active(f, u64::MAX));
    assert!(f.schedulable(Some(&genesis_hex())));
    assert!(!f.schedulable(Some(&"cd".repeat(32))));
    assert!(!f.schedulable(None));
    for h in [0, A, K - 1] {
        assert!(
            !feature_active(f, h) && !bond_admission_key_active(h),
            "height {h}"
        );
    }
    for h in [K, K + 1, u64::MAX] {
        assert!(
            feature_active(f, h) && bond_admission_key_active(h),
            "height {h}"
        );
    }
}

#[test]
fn before_activation_an_admission_co_sign_is_refused_exactly_like_any_unknown_key() {
    let mut s = state();
    let admission = cosigned(10, 1, 0, NodeBondAction::Deposit, 1, ADMISSION);
    let unknown = cosigned(10, 1, 0, NodeBondAction::Deposit, 1, WRONG);
    for height in [A, K - 1] {
        assert!(!verify_at(&admission, height), "height {height}");
        assert_eq!(
            format!(
                "{:?}",
                admission.verify(&identity(), &[7; 32], &governance_keys(), height)
            ),
            format!(
                "{:?}",
                unknown.verify(&identity(), &[7; 32], &governance_keys(), height)
            ),
            "same refusal as an unknown key at height {height}"
        );
    }
    let before = root(&mut s);
    let refused = apply(&mut s, admission, A).unwrap_err();
    assert_eq!(
        format!("{refused:?}"),
        format!("{:?}", apply(&mut s, unknown, A).unwrap_err())
    );
    assert_eq!(root(&mut s), before, "refusal leaves state untouched");
    assert!(
        s.node_bond_registry
            .as_ref()
            .unwrap()
            .registrations
            .is_empty()
    );
}

#[test]
fn the_boundary_is_exact_and_both_pinned_keys_work() {
    let mut s = state();
    let p = cosigned(10, 1, 0, NodeBondAction::Deposit, 1, ADMISSION);
    assert!(!verify_at(&p, K - 1));
    assert!(verify_at(&p, K));
    advance(&mut s, K - 1);
    assert!(apply(&mut s, p.clone(), K - 1).is_err());
    advance(&mut s, K);
    apply(&mut s, p, K).unwrap();
    // The cold spare, on another bond.
    apply(
        &mut s,
        cosigned(20, 2, 0, NodeBondAction::Deposit, 1, SPARE),
        K,
    )
    .unwrap();
    assert_eq!(
        s.node_bond_registry.as_ref().unwrap().registrations.len(),
        2
    );
}

#[test]
fn the_governance_key_keeps_working_before_and_after_activation() {
    for height in [A, K - 1, K, K + 1] {
        assert!(
            verify_at(&fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1), height),
            "deposit at {height}"
        );
        assert!(
            verify_at(&fixture::auth(10, 1, 1, NodeBondAction::Update, 2), height),
            "update at {height}"
        );
    }
    let mut s = state();
    apply(
        &mut s,
        fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1),
        A,
    )
    .unwrap();
    advance(&mut s, K);
    apply(
        &mut s,
        fixture::auth(20, 2, 0, NodeBondAction::Deposit, 1),
        K,
    )
    .unwrap();
}

#[test]
fn an_admission_co_sign_moves_no_state_compared_with_a_governance_one() {
    let mut governed = state();
    let mut admitted = state();
    advance(&mut governed, K);
    advance(&mut admitted, K);
    apply(
        &mut governed,
        fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1),
        K,
    )
    .unwrap();
    apply(
        &mut admitted,
        cosigned(10, 1, 0, NodeBondAction::Deposit, 1, ADMISSION),
        K,
    )
    .unwrap();
    assert_eq!(governed.node_bond_registry, admitted.node_bond_registry);
    assert_eq!(root(&mut governed), root(&mut admitted));

    // Neither the wire shape nor the price differs either.
    let g = fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1);
    let a = cosigned(10, 1, 0, NodeBondAction::Deposit, 1, ADMISSION);
    let names = |p: &NodeBondLifecycle| -> Vec<String> {
        serde_json::to_value(p)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    };
    assert_eq!(names(&g), names(&a));
    assert_eq!(
        fixture::transaction(&g, 1_000_000).verify_work_at_height(K),
        fixture::transaction(&a, 1_000_000).verify_work_at_height(K),
    );
}

#[test]
fn an_update_can_be_admitted_after_activation_but_not_before() {
    let mut s = state();
    apply(
        &mut s,
        fixture::auth(10, 1, 0, NodeBondAction::Deposit, 1),
        A,
    )
    .unwrap();
    let e1 = A + node_epoch_blocks();
    let update = cosigned(10, 1, 1, NodeBondAction::Update, 2, ADMISSION);
    assert!(!verify_at(&update, K - 1));
    advance(&mut s, e1);
    apply(&mut s, update, e1).unwrap();
    let r = &s.node_bond_registry.as_ref().unwrap().registrations[&[1; 32]];
    assert!(r.pending_binding.is_some());
}

#[test]
fn a_wrong_key_or_a_mismatched_signature_is_refused_once_active() {
    let good = cosigned(10, 1, 0, NodeBondAction::Deposit, 1, ADMISSION);
    assert!(verify_at(&good, K));
    // A key nobody pinned, correctly signed.
    assert!(!verify_at(
        &cosigned(10, 1, 0, NodeBondAction::Deposit, 1, WRONG),
        K
    ));
    // The pinned key named, another key's signature.
    let mut p = good.clone();
    p.governance_signature = sign(&p.signed.authorization, WRONG);
    assert!(!verify_at(&p, K));
    // The spare's signature under the online key's name.
    let mut p = good.clone();
    p.governance_signature = sign(&p.signed.authorization, SPARE);
    assert!(!verify_at(&p, K));
    // No signature, no key, malformed signature.
    let mut p = good.clone();
    p.governance_signature.clear();
    assert!(!verify_at(&p, K));
    let mut p = good.clone();
    p.governance_key = None;
    assert!(!verify_at(&p, K));
    let mut p = good.clone();
    p.governance_signature = vec![1; 64];
    assert!(!verify_at(&p, K));
}

#[test]
fn a_replayed_or_transplanted_co_sign_is_refused() {
    let mut s = state();
    advance(&mut s, K);
    let p = cosigned(10, 1, 0, NodeBondAction::Deposit, 1, ADMISSION);
    apply(&mut s, p.clone(), K).unwrap();
    // The same packet again: the nonce moved, and the deployment is taken.
    let before = root(&mut s);
    assert!(apply(&mut s, p.clone(), K).is_err());
    assert_eq!(root(&mut s), before);

    // A signature made for another bond.
    let other = cosigned(20, 2, 0, NodeBondAction::Deposit, 1, ADMISSION);
    let mut stolen = other.clone();
    stolen.governance_signature = p.governance_signature.clone();
    assert!(verify_at(&other, K));
    assert!(!verify_at(&stolen, K));

    // A signature for this bond at another nonce.
    let mut later = fixture::auth(10, 1, 1, NodeBondAction::Deposit, 1);
    later.governance_key = Some(pk(ADMISSION));
    later.governance_signature = p.governance_signature.clone();
    assert!(!verify_at(&later, K));

    // A Deposit's signature does not carry over to an Update of the same bond.
    let mut update = fixture::auth(10, 1, 0, NodeBondAction::Update, 1);
    update.governance_key = Some(pk(ADMISSION));
    update.governance_signature = p.governance_signature.clone();
    assert!(!verify_at(&update, K));
}

#[test]
fn a_signature_for_another_chain_is_refused() {
    let _ = state();
    // The same authorization signed by the pinned key for chain 888.
    let mut p = cosigned(10, 1, 0, NodeBondAction::Deposit, 1, ADMISSION);
    p.governance_signature = sign_domain_for(
        &key(ADMISSION),
        &p.signed.authorization,
        &ChainIdentity::new("888", "testnet"),
        SigningProtocolVersion::V2,
    )
    .as_bytes()
    .to_vec();
    assert!(!verify_at(&p, K));

    // A packet honestly bound to another genesis: the key is pinned per chain
    // instance, so it means nothing there, while governance still works.
    let other_genesis = [8u8; 32];
    let mut q = fixture::auth_on(10, 1, 0, NodeBondAction::Deposit, 1, other_genesis);
    assert!(
        q.verify(&identity(), &other_genesis, &governance_keys(), K)
            .is_ok()
    );
    q.governance_key = Some(pk(ADMISSION));
    q.governance_signature = sign(&q.signed.authorization, ADMISSION);
    assert!(
        q.verify(&identity(), &other_genesis, &governance_keys(), K)
            .is_err()
    );
}

#[test]
fn exit_and_withdraw_refuse_an_admission_co_sign_at_every_height() {
    for action in [NodeBondAction::Exit, NodeBondAction::Withdraw] {
        let plain = fixture::auth(10, 1, 1, action, 2);
        assert!(verify_at(&plain, K), "{action:?}: owner alone still works");

        let with_key_and_signature = cosigned(10, 1, 1, action, 2, ADMISSION);
        let mut key_only = plain.clone();
        key_only.governance_key = Some(pk(ADMISSION));
        let mut signature_only = plain.clone();
        signature_only.governance_signature = sign(&plain.signed.authorization, ADMISSION);
        for height in [A, K, K + 1] {
            assert!(
                !verify_at(&with_key_and_signature, height),
                "{action:?} at {height}"
            );
            assert!(
                !verify_at(&key_only, height),
                "{action:?} key only at {height}"
            );
            assert!(
                !verify_at(&signature_only, height),
                "{action:?} signature only at {height}"
            );
        }
    }

    // An owner cannot be replaced by the admission key either: a lifecycle
    // whose owner signature is the admission key's is refused.
    let mut p = fixture::auth(10, 1, 1, NodeBondAction::Exit, 2);
    p.signed.owner_signature = sign(&p.signed.authorization, ADMISSION);
    assert!(!verify_at(&p, K));
    let mut p = fixture::auth(10, 1, 1, NodeBondAction::Withdraw, 2);
    p.owner_key = pk(ADMISSION);
    p.signed.owner_signature = sign(&p.signed.authorization, ADMISSION);
    assert!(!verify_at(&p, K));
}

#[test]
fn a_deposit_co_sign_cannot_stand_in_for_the_other_approvals() {
    // Each other role is still required on an admitted Deposit.
    let good = cosigned(10, 1, 0, NodeBondAction::Deposit, 1, ADMISSION);
    let a = good.signed.authorization.clone();
    let mut owner = good.clone();
    owner.signed.owner_signature = sign(&a, ADMISSION);
    assert!(!verify_at(&owner, K));
    let mut payout = good.clone();
    payout.payout_signature = sign(&a, ADMISSION);
    assert!(!verify_at(&payout, K));
    let mut server = good.clone();
    server.signed.server_signature.clear();
    assert!(!verify_at(&server, K));
    let mut consumer = good;
    consumer.signed.consumer_signature.clear();
    assert!(!verify_at(&consumer, K));
}

#[test]
fn the_admission_key_is_no_governance_key_for_extensions_or_notices() {
    install();
    let authority = schedule_extension_authority().unwrap();
    let context = authority.context();
    let pinned = hex::encode(pk(ADMISSION));
    let spare = hex::encode(pk(SPARE));
    assert_eq!(
        bond_admission_keys(&genesis_hex()),
        vec![pinned.clone(), spare.clone()]
    );
    assert!(!context.allowlist.contains(&pinned) && !context.allowlist.contains(&spare));

    // Schedule extension signed by each pinned key: not authorized.
    let tip = 21_200;
    let extension = FeatureScheduleExtension {
        chain_id: "777".into(),
        network_id: "testnet".into(),
        genesis_hash: genesis_hex(),
        sequence: 0,
        entries: vec![ScheduleEntry::new(
            "reward_gross_outlier_guard",
            tip + MIN_RUNUP_BLOCKS,
        )],
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "ab1b1a4930000000000000000000000000000000".into(),
    };
    for signer in [ADMISSION, SPARE] {
        let signed = SignedFeatureScheduleExtension {
            extension: extension.clone(),
            signatures: vec![
                sign_feature_schedule_extension_with_key(&extension, &key(signer)).unwrap(),
            ],
        };
        let expected = hex::encode(pk(signer));
        assert_eq!(
            verify_feature_schedule_extension(&signed, &context),
            Err(ScheduleExtensionError::SignerNotAuthorized {
                signer_pubkey: expected.clone()
            })
        );
        assert_eq!(
            validate_extension_against_state(&signed, tip, &context, &[], &[]),
            Err(ScheduleExtensionError::SignerNotAuthorized {
                signer_pubkey: expected
            })
        );
    }
    // The governance key signs the same extension.
    let good = SignedFeatureScheduleExtension {
        extension: extension.clone(),
        signatures: vec![
            sign_feature_schedule_extension_with_key(&extension, &key(GOVERNANCE)).unwrap(),
        ],
    };
    assert_eq!(verify_feature_schedule_extension(&good, &context), Ok(()));

    // Upgrade notice signed by the admission key: not authorized.
    let notice = UpgradeNotice {
        notice_id: UpgradeNotice::notice_id_for(14_000, "ab1b1a4930000000000000000000000000000000"),
        chain_id: "777".into(),
        network_id: "testnet".into(),
        activation_height: 14_000,
        min_protocol_version: 3,
        features: vec![],
        release_commit: "ab1b1a4930000000000000000000000000000000".into(),
        image_digests: Default::default(),
        issued_at: 1_000,
        expires_at: None,
    };
    let by = |signer: u8| SignedUpgradeNotice {
        notice: notice.clone(),
        signatures: vec![sign_upgrade_notice_with_key(&notice, &key(signer))],
    };
    assert_eq!(
        verify_upgrade_notice(&by(ADMISSION), "777", "testnet", context.allowlist, 1_001),
        Err(UpgradeNoticeError::SignerNotAuthorized {
            signer_pubkey: pinned.clone()
        })
    );
    assert_eq!(
        verify_upgrade_notice(&by(GOVERNANCE), "777", "testnet", context.allowlist, 1_001),
        Ok(())
    );

    // Oracle-set governance reads its own allowlist, never the pins.
    assert!(!is_authorized_oracle_governance(&pinned));
}

/// Nothing but the node-bond co-sign, the scheduling guards and offline tooling
/// reads the admission table, so no other action can come to trust it.
#[test]
fn only_the_bond_path_and_the_scheduling_guards_read_the_pins() {
    fn scan(dir: &std::path::Path, hits: &mut Vec<String>, root: &std::path::Path) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                scan(&path, hits, root);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                if source.contains("bond_admission_keys(")
                    || source.contains("BOND_ADMISSION_KEY_PINS")
                    || source.contains("admission_keys_at(")
                {
                    hits.push(path.strip_prefix(root).unwrap().display().to_string());
                }
            }
        }
    }
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().unwrap();
    let mut hits = Vec::new();
    for crate_src in [
        "shared-protocol/src",
        "native-chain/src",
        "tools/cli/WrfcoinCLI/src",
    ] {
        scan(&root.join(crate_src), &mut hits, root);
    }
    hits.sort();
    assert_eq!(
        hits,
        [
            "shared-protocol/src/consensus/node_bond_registry.rs",
            "shared-protocol/src/crypto/chain_binding.rs",
            "shared-protocol/src/governance/schedule_extension.rs",
            "tools/cli/WrfcoinCLI/src/commands/node_bond.rs",
            "tools/cli/WrfcoinCLI/src/commands/node_bond_admit.rs",
            "tools/cli/WrfcoinCLI/src/commands/node_bond_update.rs",
            "tools/cli/WrfcoinCLI/src/commands/node_deposit.rs",
        ]
    );
}

#[test]
fn the_admission_key_cannot_hold_a_bond_role_once_active() {
    // The fixture's server key is owner + 1 and its consumer key owner + 2.
    let as_server = fixture::auth(59, 1, 0, NodeBondAction::Deposit, 1);
    assert_eq!(as_server.signed.authorization.server_key, pk(ADMISSION));
    let as_consumer = fixture::auth(58, 1, 0, NodeBondAction::Deposit, 1);
    assert_eq!(as_consumer.signed.authorization.consumer_key, pk(ADMISSION));
    // Owner and payout are both address(60).
    let as_payout = fixture::auth(60, 2, 0, NodeBondAction::Deposit, 1);
    assert_eq!(
        as_payout.signed.authorization.payout_account,
        address(ADMISSION)
    );

    // Before activation, as without this feature.
    for p in [&as_server, &as_consumer, &as_payout] {
        let mut s = state();
        apply(&mut s, p.clone(), A).unwrap();
    }
    // Active: refused.
    for p in [&as_server, &as_consumer, &as_payout] {
        let mut s = state();
        advance(&mut s, K);
        assert!(apply(&mut s, p.clone(), K).is_err());
    }
}

#[test]
fn mempool_admission_follows_the_carrying_height() {
    let _ = state();
    let tx = fixture::transaction(
        &cosigned(10, 1, 0, NodeBondAction::Deposit, 1, ADMISSION),
        1_000_000,
    );
    // Tip K - 2 carries at K - 1: inactive. Tip K - 1 carries at K: active.
    let mut pool = Mempool::new(&Config::default());
    let refused = pool
        .add_transaction_with_signing_keys_at_height(tx.clone(), None, Some(K - 2))
        .unwrap_err();
    assert!(
        refused.to_string().contains("SP_NODE_BOND_AUTHORIZATION"),
        "{refused}"
    );
    let mut pool = Mempool::new(&Config::default());
    pool.add_transaction_with_signing_keys_at_height(tx, None, Some(K - 1))
        .unwrap();
}

#[test]
fn a_binary_with_the_key_does_not_halt_at_activation() {
    install();
    let schedule = FeatureSchedule::from_entries(vec![("bond_admission_key".into(), K)]).unwrap();
    assert_eq!(upgrade_required_in_feature_schedule(&schedule), None);
}

#[test]
fn scheduling_guards_admit_only_the_pinned_chain() {
    install();
    let extension = |genesis: String| FeatureScheduleExtension {
        chain_id: "777".into(),
        network_id: "testnet".into(),
        genesis_hash: genesis,
        sequence: 0,
        entries: vec![ScheduleEntry::new(
            "bond_admission_key",
            21_200 + MIN_RUNUP_BLOCKS,
        )],
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "ab1b1a4930000000000000000000000000000000".into(),
    };
    let here = extension(genesis_hex());
    assert_eq!(check_admission_policy(&here, Some(&genesis_hex())), Ok(()));
    sign_feature_schedule_extension_with_key(&here, &key(GOVERNANCE)).unwrap();

    let other = "cd".repeat(32);
    let elsewhere = extension(other.clone());
    assert!(check_admission_policy(&elsewhere, Some(&other)).is_err());
    assert!(check_admission_policy(&here, None).is_err());
    assert!(sign_feature_schedule_extension_with_key(&elsewhere, &key(GOVERNANCE)).is_err());
}
