use std::time::{Duration, Instant};

use libp2p::futures::StreamExt;
use libp2p::gossipsub;
use libp2p::swarm::SwarmEvent;
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;

use super::BehaviourHarness;
use crate::P2pEvent;
use crate::behaviour::WrfcoinBehaviourEvent;
use crate::message::WrfcoinMessage;

fn privacy_tx(kind: TransactionType) -> Vec<u8> {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string("synthetic-privacy-input"),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 0,
            address: "local-only".into(),
            script: "OP_TRUE".into(),
            nft_data: None,
        }],
    );
    tx.tx_type = kind;
    tx.data = Some("{}".into());
    if matches!(kind, TransactionType::Stake | TransactionType::Unstake) {
        use rand::{SeedableRng, rngs::StdRng};
        use wrfcoin_shared_protocol::blockchain::transaction::StakeAuthPayload;
        use wrfcoin_shared_protocol::common::types::Address;
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
        let key = PrivateKey::generate(&mut StdRng::seed_from_u64(3430));
        let account = Address::from_public_key(&PublicKey::from_private_key(&key));
        let mut payload = StakeAuthPayload {
            account: account.to_hex(),
            amount: 1_000,
            auth_pubkey: None,
            auth_signature: None,
        };
        payload.sign_authorization(&key, kind == TransactionType::Unstake, 7);
        tx.sender_address = account;
        tx.nonce = 7;
        tx.inputs[0].public_key = Some(account.to_string());
        tx.outputs[0].address = account.to_string();
        tx.data = Some(serde_json::to_string(&payload).unwrap());
        assert!(
            tx.verify_stake_authorization(),
            "synthetic fixture is owner-authorized"
        );
    }
    tx.set_id();
    bincode::serialize(&tx).unwrap()
}

#[tokio::test]
async fn privacy_admission_outbound_does_not_publish_or_queue() {
    let (mut h, _) = BehaviourHarness::new();
    for kind in [
        TransactionType::RewardClaim,
        TransactionType::ConfidentialTransfer,
        TransactionType::Stake,
        TransactionType::Unstake,
    ] {
        super::super::publish_message(
            &mut h.swarm,
            &h.tm,
            &h.metrics,
            &mut h.pending_broadcasts,
            WrfcoinMessage::NewTransactions {
                tx_bytes: vec![privacy_tx(kind)],
            },
            1_048_576,
            Instant::now(),
        );
    }
    assert_eq!(
        h.pending_broadcasts.len(),
        0,
        "disabled gossip must not await a mesh"
    );
    assert_eq!(h.metrics.snapshot(0).messages_rejected, 4);
}

/// A and C have only B as a neighbour. A bypasses local outbound admission,
/// emulating an unpatched peer. Capture C's wire before its application
/// handler: filtering only C's event queue cannot hide an unintended B
/// rebroadcast.
#[tokio::test]
async fn privacy_admission_three_node_relay_and_block_bytes() {
    let (mut a, _) = BehaviourHarness::new();
    let (mut b, mut b_rx) = BehaviourHarness::new();
    let (mut c, _) = BehaviourHarness::new();
    b.swarm.listen_on("/memory/0".parse().unwrap()).unwrap();
    let addr = loop {
        if let SwarmEvent::NewListenAddr { address, .. } = b.swarm.select_next_some().await {
            break address;
        }
    };
    a.swarm.dial(addr.clone()).unwrap();
    c.swarm.dial(addr).unwrap();
    let mut b_wire = Vec::new();
    let mut c_wire = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        step(&mut a, &mut b, &mut c, &mut b_wire, &mut c_wire).await;
        if b.swarm
            .behaviour()
            .gossipsub
            .mesh_peers(&b.tm.transactions_hash())
            .count()
            == 2
            && a.swarm
                .behaviour()
                .gossipsub
                .mesh_peers(&a.tm.transactions_hash())
                .count()
                == 1
            && c.swarm
                .behaviour()
                .gossipsub
                .mesh_peers(&c.tm.transactions_hash())
                .count()
                == 1
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "three-node mesh must actually form"
        );
    }
    assert_eq!(a.swarm.connected_peers().count(), 1);
    assert_eq!(c.swarm.connected_peers().count(), 1);
    for kind in [
        TransactionType::RewardClaim,
        TransactionType::ConfidentialTransfer,
        TransactionType::Stake,
        TransactionType::Unstake,
    ] {
        let disabled = privacy_tx(kind);
        let message = WrfcoinMessage::NewTransactions {
            tx_bytes: vec![disabled.clone()],
        };
        let raw = message.to_bytes().unwrap();
        a.swarm
            .behaviour_mut()
            .gossipsub
            .publish(message.topic(&a.tm), raw.clone())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !b_wire.contains(&raw) {
            step(&mut a, &mut b, &mut c, &mut b_wire, &mut c_wire).await;
            assert!(
                Instant::now() < deadline,
                "B must receive attacker bytes on the wire"
            );
        }
        // Block messages may contain these transactions. Admission policy must
        // leave historical consensus bytes intact at the gossip boundary.
        let block = WrfcoinMessage::NewBlock {
            block_bytes: disabled,
        };
        let block_raw = block.to_bytes().unwrap();
        a.swarm
            .behaviour_mut()
            .gossipsub
            .publish(block.topic(&a.tm), block_raw.clone())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            step(&mut a, &mut b, &mut c, &mut b_wire, &mut c_wire).await;
        }
        assert!(
            c_wire.contains(&block_raw),
            "B must still relay exact block bytes"
        );
        assert!(
            !c_wire.contains(&raw),
            "B must not relay disabled transaction gossip to C"
        );
        while let Ok(event) = b_rx.try_recv() {
            assert!(
                !matches!(event, P2pEvent::NewTransactions(_)),
                "disabled batch must not reach the mempool consumer"
            );
        }
    }
    // A batch mixing an ordinary transaction with a disabled one is refused
    // whole; the same ordinary transaction in its own envelope must still flow.
    let mut ordinary: Transaction =
        bincode::deserialize(&privacy_tx(TransactionType::RewardClaim)).unwrap();
    ordinary.tx_type = TransactionType::Reward;
    ordinary.set_id();
    let good = bincode::serialize(&ordinary).unwrap();
    let mut mixed_raws = Vec::new();
    for kind in [
        TransactionType::RewardClaim,
        TransactionType::ConfidentialTransfer,
        TransactionType::Stake,
        TransactionType::Unstake,
    ] {
        let mixed = WrfcoinMessage::NewTransactions {
            tx_bytes: vec![good.clone(), privacy_tx(kind)],
        };
        let raw = mixed.to_bytes().unwrap();
        a.swarm
            .behaviour_mut()
            .gossipsub
            .publish(mixed.topic(&a.tm), raw.clone())
            .unwrap();
        mixed_raws.push(raw);
    }
    let control = WrfcoinMessage::NewTransactions {
        tx_bytes: vec![good],
    };
    let control_raw = control.to_bytes().unwrap();
    a.swarm
        .behaviour_mut()
        .gossipsub
        .publish(control.topic(&a.tm), control_raw.clone())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        step(&mut a, &mut b, &mut c, &mut b_wire, &mut c_wire).await;
    }
    for raw in mixed_raws {
        assert!(b_wire.contains(&raw));
        assert!(!c_wire.contains(&raw));
    }
    assert!(c_wire.contains(&control_raw));
}

async fn step(
    a: &mut BehaviourHarness,
    b: &mut BehaviourHarness,
    c: &mut BehaviourHarness,
    b_wire: &mut Vec<Vec<u8>>,
    c_wire: &mut Vec<Vec<u8>>,
) {
    let (side, event) = tokio::select! {
        e = a.swarm.select_next_some() => (0, e),
        e = b.swarm.select_next_some() => (1, e),
        e = c.swarm.select_next_some() => (2, e),
        _ = tokio::time::sleep(Duration::from_millis(25)) => return,
    };
    if let SwarmEvent::Behaviour(event) = event {
        if let WrfcoinBehaviourEvent::Gossipsub(gossipsub::Event::Message { message, .. }) = &event
        {
            if side == 1 {
                b_wire.push(message.data.clone());
            }
            if side == 2 {
                c_wire.push(message.data.clone());
            }
        }
        match side {
            0 => a.dispatch(event).await,
            1 => b.dispatch(event).await,
            _ => c.dispatch(event).await,
        }
    }
}

/// Golden bytes produced by unmodified B3 (65134a64), over a real local TCP
/// gossip connection, with full-mining,p2p,hsm,upgrade-notice-fetch and no zk.
/// Node admission policy must never reinterpret these historical blocks.
#[tokio::test]
async fn privacy_admission_feature_off_consensus_golden_bytes() {
    use std::sync::Arc;
    use wrfcoin_shared_protocol::{
        blockchain::{block::Block, chain::Blockchain},
        common::types::{Address, Balance},
        consensus::proof_of_publishing::{Clock, ProofOfPublishing},
        crypto::{
            chain_binding::{ChainIdentity, SigningProtocolVersion},
            domain_signing::DomainSeparated,
        },
        station_registry::WeatherStation,
    };
    struct PinnedClock;
    impl Clock for PinnedClock {
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
            chrono::DateTime::from_timestamp(1_800_000_060, 0).unwrap()
        }
    }
    for fixture in [
        include_str!("fixtures/b3-ConfidentialTransfer.json"),
        include_str!("fixtures/b3-RewardClaim.json"),
    ] {
        let fixture: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let raw = hex::decode(fixture["block_bytes"].as_str().unwrap()).unwrap();
        let mut block: Block = bincode::deserialize(&raw).unwrap();
        let consensus_bytes = hex::decode(fixture["consensus_bytes"].as_str().unwrap()).unwrap();
        let identity = ChainIdentity::new("", "");
        assert_eq!(
            // secret-leak-ok: public synthetic block signing preimage and golden consensus bytes; no private key material
            block.signing_preimage_for(&identity, SigningProtocolVersion::V1),
            consensus_bytes
        );
        assert_eq!(block.hash, Some(block.calculate_hash()));
        let mut peer = ProofOfPublishing::new(1, 60, 0, 10);
        peer.set_clock(Arc::new(PinnedClock));
        assert_eq!(
            peer.verify_block(&block, &Default::default(), 1, &[])
                .await
                .unwrap(),
            fixture["accepted"].as_bool().unwrap()
        );
        let mut historical = Blockchain::default();
        historical
            .state
            .create_account(Address([0x56; 20]), Balance::new(1_000_000));
        let stations: Vec<WeatherStation> =
            serde_json::from_value(fixture["stations"].clone()).unwrap();
        for station in stations {
            historical.state.station_registry.register(station).unwrap();
        }
        historical.state.recalculate_state_root();
        // Replay is independent of admission. This does not claim a B3 peer
        // accepts the confidential block: the verdict above must remain false.
        peer.replay_received_block_for_test_support(&mut historical, &mut block)
            .unwrap();
        assert_eq!(
            historical.state.state_root.to_string(),
            fixture["state_root"].as_str().unwrap()
        );
        assert_eq!(
            // secret-leak-ok: public synthetic block signing preimage and golden consensus bytes; no private key material
            block.signing_preimage_for(&identity, SigningProtocolVersion::V1),
            consensus_bytes
        );
        assert_eq!(historical.height, 1);
    }
}
