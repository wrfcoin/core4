use wrfcoin_shared_protocol::blockchain::snapshot_sync::{
    SignedSnapshotSyncPayload, SnapshotSyncPayload,
};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::consensus::node_service_receipt::{
    NodeService, NodeServiceReceipt, SignedNodeServiceReceipt,
};
use wrfcoin_shared_protocol::consensus::node_share_contracts::*;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, FeatureSchedule, GENESIS_ACTIVATION_SCHEDULE_KEY,
    LEGACY_CHAIN_BOOTSTRAP, SigningProtocolVersion,
};
use wrfcoin_shared_protocol::crypto::domain_signing::{DomainSeparated, sign_domain_for};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

mod common;

fn identity() -> ChainIdentity {
    ChainIdentity::new("777", "testnet")
}
fn key(seed: u64) -> PrivateKey {
    common::keypair(seed).0
}
fn pk(key: &PrivateKey) -> [u8; 32] {
    PublicKey::from_private_key(key)
        .as_bytes()
        .try_into()
        .unwrap()
}
fn sign<T: DomainSeparated>(subject: &T, key: &PrivateKey) -> Vec<u8> {
    sign_domain_for(key, subject, &identity(), SigningProtocolVersion::V2)
        .as_bytes()
        .to_vec()
}

fn claim() -> BondedServiceClaim {
    let consumer = key(284_801);
    let receipt = SignedNodeServiceReceipt::sign(
        NodeServiceReceipt {
            service: NodeService::SnapshotJoin,
            server_id: "synthetic.example".into(),
            consumer_pubkey: pk(&consumer),
            chain_height: 77,
            subject_hash: [3; 32],
            units: 4,
            timestamp: 5,
            nonce: [6; 32],
        },
        &consumer,
        &identity(),
        SigningProtocolVersion::V1,
    );
    let manifest = SignedSnapshotSyncPayload {
        signer_id: hex::encode(pk(&key(284_802))),
        timestamp: 5,
        nonce: "synthetic-nonce".into(),
        payload: SnapshotSyncPayload {
            height: 77,
            state_root_hex: "01".repeat(32),
            snapshot_root_hex: "02".repeat(32),
            entry_count: 4,
            tip_hash_hex: "03".repeat(32),
            finalized_height: 76,
        },
        // S1 verifies signature contracts, not snapshot export trust (S3).
        signature: "00".repeat(64),
    };
    let ticket = NodeServiceTicket {
        context: NodeSettlementContext {
            genesis_hash: [1; 32],
            chain_id: "777".into(),
            network_id: "testnet".into(),
            epoch: 0,
            roster_sequence: 1,
            roster_hash: [2; 32],
            epoch_start_parent: [3; 32],
        },
        server_id: [4; 32],
        consumer_id: [5; 32],
        ticket_id: [6; 32],
        signed_receipt_hash: BondedServiceClaim::receipt_hash(&receipt).unwrap(),
        signed_manifest_hash: BondedServiceClaim::manifest_hash(&manifest).unwrap(),
    };
    let server_signature = sign(&ticket, &key(284_802));
    let consumer_signature = sign(
        &NodeSettlementEndorsement {
            ticket_preimage_hash: node_contract_digest(&ticket, &identity()),
        },
        &consumer,
    );
    BondedServiceClaim {
        receipt,
        manifest,
        ticket,
        server_signature,
        consumer_signature,
    }
}

fn endorse(c: &mut BondedServiceClaim) {
    c.server_signature = sign(&c.ticket, &key(284_802));
    c.consumer_signature = sign(
        &NodeSettlementEndorsement {
            ticket_preimage_hash: node_contract_digest(&c.ticket, &identity()),
        },
        &key(284_801),
    );
}

#[test]
fn validly_signed_self_identity_and_replaced_receipt_still_fail() {
    let mut c = claim();
    c.ticket.consumer_id = c.ticket.server_id;
    endorse(&mut c);
    assert_eq!(verify(&c), Err(NodeContractError::Binding));
    let mut c = claim();
    c.receipt.receipt.nonce[0] ^= 1;
    c.receipt = SignedNodeServiceReceipt::sign(
        c.receipt.receipt,
        &key(284_801),
        &identity(),
        SigningProtocolVersion::V1,
    );
    // An independently valid replacement inner signature must not be swapped
    // under the old, still-valid outer endorsements.
    assert_eq!(verify(&c), Err(NodeContractError::Binding));
}

fn verify(c: &BondedServiceClaim) -> Result<(), NodeContractError> {
    c.verify_contract(
        &identity(),
        &[1; 32],
        &pk(&key(284_802)),
        &pk(&key(284_801)),
        SigningProtocolVersion::V1,
    )
}

#[test]
fn exact_three_service_signatures_and_bounded_wire() {
    let c = claim();
    assert_eq!(verify(&c), Ok(()));
    let wire = c.to_wire().unwrap();
    assert!(wire.len() <= MAX_NODE_CLAIM_BYTES);
    assert_eq!(BondedServiceClaim::from_wire(&wire).unwrap(), c);
    let mut trailing = wire.clone();
    trailing.push(0);
    assert_eq!(
        BondedServiceClaim::from_wire(&trailing),
        Err(NodeContractError::Bounds)
    );
    assert_eq!(
        BondedServiceClaim::from_wire(&vec![0; MAX_NODE_CLAIM_BYTES + 1]),
        Err(NodeContractError::Bounds)
    );
    for index in 0..3 {
        for length in [0, 63, 65] {
            let mut broken = c.clone();
            let sig = match index {
                0 => &mut broken.receipt.signature,
                1 => &mut broken.server_signature,
                _ => &mut broken.consumer_signature,
            };
            *sig = vec![0; length];
            assert!(
                verify(&broken).is_err(),
                "signature {index}, length {length}"
            );
        }
    }
}

#[test]
fn tampered_signed_bytes_genesis_domains_and_aliases_are_rejected() {
    let c = claim();
    let mut variants = Vec::new();
    let mut t = c.clone();
    t.ticket.context.genesis_hash[0] ^= 1;
    variants.push(t);
    let mut t = c.clone();
    t.ticket.context.epoch += 1;
    variants.push(t);
    let mut t = c.clone();
    t.ticket.context.roster_sequence += 1;
    variants.push(t);
    let mut t = c.clone();
    t.ticket.context.roster_hash[0] ^= 1;
    variants.push(t);
    let mut t = c.clone();
    t.ticket.context.epoch_start_parent[0] ^= 1;
    variants.push(t);
    let mut t = c.clone();
    t.ticket.ticket_id[0] ^= 1;
    variants.push(t);
    let mut t = c.clone();
    t.ticket.consumer_id = t.ticket.server_id;
    variants.push(t);
    let mut t = c.clone();
    t.receipt.receipt.units += 1;
    variants.push(t);
    let mut t = c.clone();
    t.receipt.signature[0] ^= 1;
    variants.push(t);
    let mut t = c.clone();
    t.manifest.payload.height += 1;
    variants.push(t);
    let mut t = c.clone();
    t.manifest.signature.push('0');
    variants.push(t);
    let mut t = c.clone();
    t.consumer_signature = t.server_signature.clone();
    variants.push(t);
    for t in variants {
        assert!(verify(&t).is_err());
    }
    assert!(
        c.verify_contract(
            &ChainIdentity::new("778", "testnet"),
            &[1; 32],
            &pk(&key(284_802)),
            &pk(&key(284_801)),
            SigningProtocolVersion::V1
        )
        .is_err()
    );
    assert!(
        c.verify_contract(
            &identity(),
            &[9; 32],
            &pk(&key(284_802)),
            &pk(&key(284_801)),
            SigningProtocolVersion::V1
        )
        .is_err()
    );
    assert!(
        c.verify_contract(
            &identity(),
            &[1; 32],
            &pk(&key(284_801)),
            &pk(&key(284_801)),
            SigningProtocolVersion::V1
        )
        .is_err()
    );
    assert!(
        c.verify_contract(
            &identity(),
            &[1; 32],
            &pk(&key(284_802)),
            &pk(&key(284_801)),
            SigningProtocolVersion::V2
        )
        .is_err()
    );
}

fn bond() -> SignedNodeBondAuthorization {
    let authorization = NodeBondAuthorization {
        genesis_hash: [1; 32],
        deployment_id: [2; 32],
        owner_account: Address::new([3; 20]),
        escrow_account: Address::new([4; 20]),
        payout_account: Address::new([5; 20]),
        server_key: pk(&key(284_802)),
        consumer_key: pk(&key(284_801)),
        service_alias: "synthetic.example".into(),
        action: NodeBondAction::Deposit,
        amount_units: 250_000_000_000,
        expected_nonce: 6,
        effective_epoch: 7,
    };
    let owner_signature = sign(&authorization, &key(284_803));
    let digest = node_contract_digest(&authorization, &identity());
    let server_signature = sign(
        &NodeBondPossession {
            authorization_preimage_hash: digest,
            role: NodeBondKeyRole::Server,
        },
        &key(284_802),
    );
    let consumer_signature = sign(
        &NodeBondPossession {
            authorization_preimage_hash: digest,
            role: NodeBondKeyRole::Consumer,
        },
        &key(284_801),
    );
    SignedNodeBondAuthorization {
        authorization,
        owner_signature,
        server_signature,
        consumer_signature,
    }
}

#[test]
fn bond_owner_and_both_role_bound_possessions_are_required() {
    let b = bond();
    let check = |b: &SignedNodeBondAuthorization| {
        b.verify_contract(&identity(), &[1; 32], &pk(&key(284_803)))
    };
    assert_eq!(check(&b), Ok(()));
    let mut variants = Vec::new();
    let mut t = b.clone();
    t.authorization.action = NodeBondAction::Withdraw;
    variants.push(t);
    let mut t = b.clone();
    t.authorization.amount_units += 1;
    variants.push(t);
    let mut t = b.clone();
    t.authorization.expected_nonce += 1;
    variants.push(t);
    let mut t = b.clone();
    t.authorization.effective_epoch += 1;
    variants.push(t);
    let mut t = b.clone();
    t.authorization.genesis_hash[0] ^= 1;
    variants.push(t);
    let mut t = b.clone();
    t.authorization.consumer_key = t.authorization.server_key;
    variants.push(t);
    let mut t = b.clone();
    t.authorization.owner_account = t.authorization.escrow_account;
    variants.push(t);
    let mut t = b.clone();
    t.authorization.payout_account = t.authorization.escrow_account;
    variants.push(t);
    let mut t = b.clone();
    t.authorization.service_alias = "a".repeat(MAX_NODE_ALIAS_BYTES + 1);
    variants.push(t);
    let mut t = b.clone();
    t.owner_signature.clear();
    variants.push(t);
    let mut t = b.clone();
    t.server_signature.clear();
    variants.push(t);
    let mut t = b.clone();
    t.consumer_signature = t.server_signature.clone();
    variants.push(t);
    for t in variants {
        assert!(check(&t).is_err());
    }
    assert!(
        b.verify_contract(&identity(), &[1; 32], &pk(&key(284_802)))
            .is_err()
    );
    for (ordinal, action) in [
        NodeBondAction::Deposit,
        NodeBondAction::Update,
        NodeBondAction::Exit,
        NodeBondAction::Withdraw,
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            bincode::serialize(action).unwrap(),
            (ordinal as u32).to_le_bytes()
        );
    }
}

#[test]
fn node_gate_is_default_off_but_signed_extensions_are_admissible() {
    let f = ConsensusFeature::NodeShareSettlement;
    assert_eq!(ConsensusFeature::parse("node_share_settlement"), Some(f));
    assert!(f.has_behaviour());
    for genesis in [
        None,
        Some("synthetic"),
        Some(
            wrfcoin_shared_protocol::crypto::chain_binding::LEGACY_FAUCET_BINDINGS[0].genesis_hash,
        ),
    ] {
        assert!(f.schedulable(genesis));
    }
    assert!(LEGACY_CHAIN_BOOTSTRAP.iter().all(|r| r.feature != f));
    for height in [0, 1, 1023, 1024, 4096, u64::MAX] {
        assert!(!FeatureSchedule::default().active(f, height));
    }
    for raw in [
        "0:v1,0:node_share_settlement",
        "0:v1,1024:node_share_settlement",
    ] {
        assert!(
            FeatureSchedule::from_genesis_metadata([(GENESIS_ACTIVATION_SCHEDULE_KEY, raw)])
                .is_ok()
        );
    }
    assert_eq!(bincode::serialize(&f).unwrap(), 21u32.to_le_bytes());
    use wrfcoin_shared_protocol::governance::schedule_extension::{
        FeatureScheduleExtension, ScheduleEntry, check_admission_policy,
    };
    let extension = FeatureScheduleExtension {
        genesis_hash: "01".repeat(32),
        chain_id: "777".into(),
        network_id: "testnet".into(),
        sequence: 1,
        entries: vec![ScheduleEntry::new(f.name(), 10_000)],
        version_steps: vec![],
        min_protocol_version: 3,
        release_commit: "synthetic".into(),
    };
    assert!(check_admission_policy(&extension, None).is_ok());
    assert_eq!(NODE_BOND_WRF, 250);
    assert_eq!(MIN_DISTINCT_CONSUMERS, 1);
    assert_eq!(
        (TESTNET_CHALLENGE_BLOCKS, MAINNET_CHALLENGE_BLOCKS),
        (1024, 4096)
    );
}

#[test]
fn bounded_batch_and_bond_wire_pins() {
    use wrfcoin_shared_protocol::crypto::hash::hash_bytes;
    let c = claim();
    let b = bond();
    let batch = NodeServiceBatch {
        claims: vec![c.clone(); MAX_NODE_CLAIMS],
    };
    let wire = batch.to_wire().unwrap();
    assert_eq!(NodeServiceBatch::from_wire(&wire).unwrap(), batch);
    assert_eq!(
        NodeServiceBatch { claims: vec![] }.to_wire(),
        Err(NodeContractError::Bounds)
    );
    assert_eq!(
        NodeServiceBatch {
            claims: vec![c.clone(); MAX_NODE_CLAIMS + 1]
        }
        .to_wire(),
        Err(NodeContractError::Bounds)
    );
    assert_eq!(
        NodeServiceBatch::from_wire(&vec![0; MAX_NODE_BATCH_BYTES + 1]),
        Err(NodeContractError::Bounds)
    );
    assert_eq!(
        NodeServiceBatch::from_wire(&u64::MAX.to_le_bytes()),
        Err(NodeContractError::Bounds)
    );
    let mut oversized = c.clone();
    oversized.manifest.nonce = "x".repeat(MAX_NODE_CLAIM_BYTES);
    assert_eq!(oversized.to_wire(), Err(NodeContractError::Bounds));
    let wire = b.to_wire().unwrap();
    assert_eq!(SignedNodeBondAuthorization::from_wire(&wire).unwrap(), b);
    let mut trailing = wire.clone();
    trailing.push(0);
    assert_eq!(
        SignedNodeBondAuthorization::from_wire(&trailing),
        Err(NodeContractError::Bounds)
    );
    assert_eq!(
        SignedNodeBondAuthorization::from_wire(&vec![0; MAX_NODE_REGISTRY_BYTES + 1]),
        Err(NodeContractError::Bounds)
    );
    // Public synthetic fixture digests pin fixed-int wire, signatures and field
    // order independently of serializer round trips. V1 is also executed by
    // the unchanged NodeServiceReceipt golden in the signed-type inventory.
    let digests = [
        hex::encode(hash_bytes(&c.to_wire().unwrap()).as_bytes()),
        hex::encode(hash_bytes(&wire).as_bytes()),
        hex::encode(hash_bytes(&bincode::serialize(&c.receipt).unwrap()).as_bytes()),
    ];
    assert_eq!(
        digests,
        [
            "0fd0b4c99dcf970434e5b58be45e86f17a31c121466971296330fd454c00ef05", // pragma: allowlist secret - public synthetic S1 wire digest
            "aa68c535a2f8b3a6620fd5d5106ae094d49b4c7c1cc63f9d69484870f19369cb", // pragma: allowlist secret - public synthetic S1 wire digest
            "d695c71915d05fe3ff9c4fe6c2706cb1b5098865e9af5a158b7febcb19852a55", // pragma: allowlist secret - public synthetic S1 wire digest
        ]
    );
}

#[path = "common/node_share_vectors.rs"]
mod node_share_vectors;

#[test]
fn signed_contract_goldens_execute() {
    node_share_vectors::assert_pinned_vectors();
}

#[test]
fn regenerate_node_contract_vectors_when_requested() {
    if std::env::var("WRFCOIN_REGEN_NODE_CONTRACT_VECTORS").as_deref() != Ok("1") {
        return;
    }
    std::fs::write(
        node_share_vectors::FIXTURE,
        node_share_vectors::fixture_text(),
    )
    .unwrap();
}

#[test]
fn lifecycle_exit_and_withdraw_need_only_offline_owner() {
    for action in [NodeBondAction::Exit, NodeBondAction::Withdraw] {
        let mut b = bond();
        b.authorization.action = action;
        b.owner_signature = sign(&b.authorization, &key(284_803));
        b.server_signature.clear();
        b.consumer_signature.clear();
        assert_eq!(
            b.verify_contract(&identity(), &[1; 32], &pk(&key(284_803))),
            Ok(())
        );
        let mut injected = b.clone();
        injected.server_signature = vec![0; 64];
        assert!(
            injected
                .verify_contract(&identity(), &[1; 32], &pk(&key(284_803)))
                .is_err()
        );
        b.owner_signature = sign(&b.authorization, &key(284_802));
        assert!(
            b.verify_contract(&identity(), &[1; 32], &pk(&key(284_803)))
                .is_err()
        );
    }
}

#[test]
fn legacy_feature_wire_ordinals_are_pinned() {
    // Independently pinned from f6ea8bbd7, not derived from the current ALL.
    let names = [
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
    ];
    for (ordinal, name) in names.iter().enumerate() {
        let feature = ConsensusFeature::parse(name).unwrap();
        assert_eq!(
            bincode::serialize(&feature).unwrap(),
            (ordinal as u32).to_le_bytes()
        );
    }
    assert!(FeatureSchedule::default().entries().is_empty());
}

#[path = "common/node_share_reference.rs"]
mod reference;

#[test]
fn signed_commitments_match_independent_reference_and_bind_every_field() {
    let receipt = reference::sample_receipt();
    let expected = reference::receipt_hash(&receipt);
    assert_eq!(
        BondedServiceClaim::receipt_hash(&receipt).unwrap(),
        expected
    );
    let original = serde_json::to_value(&receipt).unwrap();
    for (field, replacement) in [
        ("service", serde_json::json!("sync_range")),
        ("server_id", serde_json::json!("other.example")),
        ("consumer_pubkey", serde_json::json!([8; 32].to_vec())),
        ("chain_height", serde_json::json!(78)),
        ("subject_hash", serde_json::json!([8; 32].to_vec())),
        ("units", serde_json::json!(6)),
        ("timestamp", serde_json::json!(-6)),
        ("nonce", serde_json::json!([8; 32].to_vec())),
    ] {
        let mut changed = original.clone();
        changed["receipt"][field] = replacement;
        let changed: SignedNodeServiceReceipt = serde_json::from_value(changed).unwrap();
        let actual = BondedServiceClaim::receipt_hash(&changed).unwrap();
        assert_eq!(actual, reference::receipt_hash(&changed), "{field}");
        assert_ne!(actual, expected, "{field} omitted");
    }
    let mut changed = receipt.clone();
    changed.signature[0] ^= 1;
    assert_eq!(
        BondedServiceClaim::receipt_hash(&changed).unwrap(),
        reference::receipt_hash(&changed)
    );
    assert_ne!(reference::receipt_hash(&changed), expected);

    let manifest = reference::sample_manifest();
    let expected = reference::manifest_hash(&manifest);
    assert_eq!(
        BondedServiceClaim::manifest_hash(&manifest).unwrap(),
        expected
    );
    let original = serde_json::to_value(&manifest).unwrap();
    for (field, replacement, payload) in [
        ("signer_id", serde_json::json!("08".repeat(32)), false),
        ("timestamp", serde_json::json!(-6), false),
        ("nonce", serde_json::json!("other-nonce"), false),
        ("signature", serde_json::json!("08".repeat(64)), false),
        ("height", serde_json::json!(78), true),
        ("state_root_hex", serde_json::json!("08".repeat(32)), true),
        (
            "snapshot_root_hex",
            serde_json::json!("08".repeat(32)),
            true,
        ),
        ("entry_count", serde_json::json!(6), true),
        ("tip_hash_hex", serde_json::json!("08".repeat(32)), true),
        ("finalized_height", serde_json::json!(75), true),
    ] {
        let mut changed = original.clone();
        if payload {
            changed["payload"][field] = replacement;
        } else {
            changed[field] = replacement;
        }
        let changed: SignedSnapshotSyncPayload = serde_json::from_value(changed).unwrap();
        let actual = BondedServiceClaim::manifest_hash(&changed).unwrap();
        assert_eq!(actual, reference::manifest_hash(&changed), "{field}");
        assert_ne!(actual, expected, "{field} omitted");
    }
}


#[path = "common/node_receipt_slice3.rs"]
mod node_receipt_slice3;

#[path = "common/node_receipt_vectors.rs"]
mod node_receipt_vectors;
#[test]
fn node_receipt_submission_has_executed_golden() { node_receipt_vectors::check_pins(); }

#[path = "common/node_bond_slice2.rs"]
mod node_bond_slice2;

#[test]
fn s6e_admission_ready_without_installing_an_activation() {
    use wrfcoin_shared_protocol::crypto::chain_binding::{ConsensusFeature, NODE_SHARE_3527_AND_S6_COMPLETE};
    assert!(NODE_SHARE_3527_AND_S6_COMPLETE, "S6e admits signed extensions after the prerequisite merge gate");
    for genesis in [None, Some(""), Some("not-a-genesis"), Some("0707070707070707070707070707070707070707070707070707070707070707")] {
        assert!(ConsensusFeature::NodeShareSettlement.schedulable(genesis));
        assert!(!FeatureSchedule::default().active(ConsensusFeature::NodeShareSettlement, 48_000));
    }
}
