//! Testnet-scope proof (#2161, owner ruling 2026-10-07).
//! Mainnet genesis/key comparisons and pre-authentication rejection are deferred.
//! These tests compile the actual P2P comparison and genesis producer pins.
use std::collections::BTreeSet;
use wrfcoin_shared_protocol::{
    blockchain::{block::Block, transaction::Transaction},
    consensus::{
        block_producer_auth::{block_producer_signing_digest, verify_block_producer_on},
        producer_authority::{
            GovernanceProducers, ProducerAuthorityError, check_block_producer_authority,
        },
        producer_set::ProducerMember,
    },
    crypto::{
        chain_binding::{
            ChainIdentity, LEGACY_CHAIN_BOOTSTRAP, SigningProtocolVersion,
            assert_chain_identity_matches_genesis,
        },
        domain_signing::{sign_domain_for, verify_domain_for},
        hash::Hash,
        signature::{PrivateKey, PublicKey},
    },
};

#[path = "../../p2p/src/genesis_handshake.rs"]
mod genesis_handshake;
#[path = "../../native-chain/src/bin/launch_api/genesis_producer_pins.rs"]
mod genesis_producer_pins;

fn identity() -> ChainIdentity {
    let rows = LEGACY_CHAIN_BOOTSTRAP;
    assert!(!rows.is_empty(), "testnet identity must be pinned");
    let row = &rows[0];
    assert!(rows.iter().all(|r| r.chain_id == row.chain_id
        && r.network_id == row.network_id
        && r.genesis_hash == row.genesis_hash));
    ChainIdentity::new(row.chain_id, row.network_id)
}

fn handshake() -> genesis_handshake::GenesisHandshakeMessage {
    let id = identity();
    genesis_handshake::GenesisHandshakeMessage::new(
        id.network_id,
        id.chain_id,
        LEGACY_CHAIN_BOOTSTRAP[0].genesis_hash.into(),
        2,
    )
}

#[test]
fn chain_id_mismatch_is_refused() {
    let pinned = identity();
    assert!(assert_chain_identity_matches_genesis(&pinned, Some(&pinned)).is_ok());
    let mut changed = pinned.clone();
    changed.chain_id.push_str("-isolation-probe");
    assert!(
        assert_chain_identity_matches_genesis(&changed, Some(&pinned)).is_err(),
        "chain_id guard"
    );
}

#[test]
fn network_id_mismatch_is_refused() {
    let pinned = identity();
    let mut changed = pinned.clone();
    changed.network_id.push_str("-isolation-probe");
    assert!(
        assert_chain_identity_matches_genesis(&changed, Some(&pinned)).is_err(),
        "network_id guard"
    );
}

#[test]
fn genesis_hash_mismatch_is_refused_after_gossip_authentication() {
    // This is the production comparison reached AFTER Noise and signed gossip
    // verification. It does not assert an earlier authentication boundary.
    let local = handshake();
    assert!(!local.has_placeholder_identity());
    assert!(local.verify_match(&local).is_ok());
    let mut remote = local.clone();
    remote.genesis_hash.replace_range(
        ..1,
        if local.genesis_hash.starts_with('0') {
            "1"
        } else {
            "0"
        },
    );
    assert!(local.verify_match(&remote).is_err(), "genesis_hash guard");
}

fn other_network() -> ChainIdentity {
    let mut id = identity();
    // Only the network ID changes: a chain-ID-only binding must fail this test.
    id.network_id.push_str("-isolation-probe");
    id
}

#[test]
fn v2_transaction_signatures_bind_network_id_in_both_directions() {
    let key = PrivateKey::from_bytes(&[0x61; 32]).unwrap(); // synthetic test key
    let public = PublicKey::from_private_key(&key);
    let transaction = Transaction::new(vec![], vec![]);
    let (testnet, other) = (identity(), other_network());
    for (signing, verifying) in [(&testnet, &other), (&other, &testnet)] {
        let signature = sign_domain_for(&key, &transaction, signing, SigningProtocolVersion::V2);
        assert!(verify_domain_for(
            &public,
            &transaction,
            &signature,
            signing,
            SigningProtocolVersion::V2
        ));
        assert!(
            !verify_domain_for(
                &public,
                &transaction,
                &signature,
                verifying,
                SigningProtocolVersion::V2
            ),
            "V2 signature network binding"
        );
    }
}

#[test]
fn v2_identical_block_hashes_bind_network_id_and_magic() {
    let block = Block::new(1, Hash::default(), vec![], 1, 1);
    let (testnet, other) = (identity(), other_network());
    assert_ne!(testnet.network_magic(), other.network_magic());
    assert_ne!(
        block.calculate_hash_for(SigningProtocolVersion::V2, Some(&testnet)),
        block.calculate_hash_for(SigningProtocolVersion::V2, Some(&other)),
        "V2 block network binding"
    );
}

#[test]
fn v1_identical_block_hashes_retain_known_gap_2066() {
    let block = Block::new(1, Hash::default(), vec![], 1, 1);
    assert_eq!(
        block.calculate_hash_for(SigningProtocolVersion::V1, Some(&identity())),
        block.calculate_hash_for(SigningProtocolVersion::V1, Some(&other_network())),
        "V1 gap #2066 changed; review the documented scope"
    );
}

fn members() -> Vec<ProducerMember> {
    let json: serde_json::Value = serde_json::from_str(
        genesis_producer_pins::TESTNET_GENESIS_PRODUCER_SET.expect("pinned genesis producers"),
    )
    .unwrap();
    json["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| ProducerMember {
            producer_id: m["producer_id"].as_str().unwrap().into(),
            payout_address: m["payout_address"].as_str().unwrap().into(),
            public_key: hex::decode(m["public_key"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap(),
        })
        .collect()
}

#[test]
fn three_producer_keys_are_pinned_to_the_public_regression_fixture() {
    let members = members();
    assert_eq!(members.len(), 3, "three pinned producers");
    let pinned: BTreeSet<_> = members
        .iter()
        .map(|m| (m.producer_id.clone(), hex::encode(m.public_key)))
        .collect();
    assert_eq!(pinned.len(), 3);
    assert_eq!(
        members
            .iter()
            .map(|m| m.public_key)
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );
    let custody: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/public_producer_pins.json"
    ))
    .unwrap();
    let recorded: BTreeSet<_> = custody["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["role"] == "producer")
        .map(|e| {
            (
                e["identity"].as_str().unwrap().to_string(),
                e["public_key"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(pinned, recorded, "genesis/public fixture producer pins");
}

#[test]
fn authenticated_non_pinned_producer_is_refused_by_live_authority_gate() {
    let members = members();
    let key = PrivateKey::from_bytes(&[0x62; 32]).unwrap(); // synthetic, never a ceremony key
    let public = PublicKey::from_private_key(&key);
    assert!(
        !members
            .iter()
            .any(|m| m.public_key.as_slice() == public.as_bytes())
    );
    let policy = GovernanceProducers::try_from_history(&[GovernanceProducers::genesis_change(
        members.clone(),
    )])
    .unwrap();
    let mut block = Block::new(
        1,
        Hash::default(),
        vec![Transaction::new_coinbase(&members[0].payout_address, 1, 1)],
        1,
        1,
    );
    block.producer_pubkey = public.as_bytes().try_into().unwrap();
    block.producer_sig = key
        .sign(&block_producer_signing_digest(
            &block,
            &block.producer_pubkey,
            &identity(),
            SigningProtocolVersion::V2,
        ))
        .as_bytes()
        .to_vec();
    assert!(
        verify_block_producer_on(&block, &identity(), SigningProtocolVersion::V2).is_ok(),
        "probe signature must authenticate"
    );
    assert_eq!(
        check_block_producer_authority(&block, &policy, &identity(), SigningProtocolVersion::V2),
        Err(ProducerAuthorityError::NotAuthorized),
        "non-pinned producer authority guard"
    );
    // Positive control: seat only the synthetic probe key in a test history.
    // This proves the refusal above is membership, not a blanket rejection or
    // an invalid signature/payout. It uses no real producer private material.
    let mut probe_members = members;
    probe_members[0].public_key = block.producer_pubkey;
    let probe_policy =
        GovernanceProducers::try_from_history(&[GovernanceProducers::genesis_change(
            probe_members,
        )])
        .unwrap();
    assert_eq!(
        check_block_producer_authority(
            &block,
            &probe_policy,
            &identity(),
            SigningProtocolVersion::V2
        ),
        Ok(())
    );
}
