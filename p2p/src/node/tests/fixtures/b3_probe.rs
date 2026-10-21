use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;
use wrfcoin_p2p::{P2pCommand, P2pConfig, P2pEvent, P2pNode};
use wrfcoin_shared_protocol::{
    blockchain::{
        chain::Blockchain,
        transaction::{
            RewardClaimPayload, Transaction, TransactionInput, TransactionOutput, TransactionType,
        },
    },
    common::types::{Address, Balance},
    config::settings::Config,
    consensus::proof_of_publishing::{Clock, ProofOfPublishing},
    crypto::hash::hash_string,
    mempool::pool::Mempool,
    station_registry::{
        ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
    },
};

struct PinnedClock;
impl Clock for PinnedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(1_800_000_060, 0).unwrap()
    }
}

fn confidential() -> Transaction {
    let mut tx = transaction(TransactionType::ConfidentialTransfer);
    tx.data = Some(
        serde_json::json!({
            "output_commitments": ["ef".repeat(40)], "range_proof": "12".repeat(20),
            "stealth_data": null, "ring_signature": null, "key_images": [],
            "encrypted_payload": null, "privacy_level": "Standard",
            "selective_disclosures": [], "signed_message": null
        })
        .to_string(),
    );
    tx.set_id();
    tx
}

fn transaction(kind: TransactionType) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("privacy-local-{kind}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 0,
            address: format!("0x{}", hex::encode([0x56; 20])),
            script: "OP_TRUE".into(),
            nft_data: None,
        }],
    );
    tx.timestamp = chrono::DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    tx.tx_type = kind;
    tx.sender_address = Address([0x56; 20]);
    tx.nonce = 0;
    tx
}

fn reward() -> (Transaction, Vec<WeatherStation>) {
    let keys: Vec<_> = (1..=8)
        .map(|n| {
            let sk = wrfcoin_privacy::Scalar::from_bytes_mod_order([n; 32]);
            (sk.to_bytes(), wrfcoin_privacy::scalar_to_public_key(&sk))
        })
        .collect();
    let stations = keys
        .iter()
        .enumerate()
        .map(|(i, (_, pk))| WeatherStation {
            station_id: format!("local-ring-{i}"),
            owner_address: format!("local-owner-{i}"),
            location: StationLocation {
                latitude: 0.0,
                longitude: 0.0,
                elevation_meters: None,
            },
            hardware_model: "synthetic".into(),
            calibration_date: None,
            is_active: true,
            reputation_score: 50,
            data_quality_tier: DataQualityTier::default(),
            provenance_class: ContributionProvenanceClass::Real,
            ring_public_key: Some(hex::encode(pk)),
        })
        .collect();
    let mut payload = RewardClaimPayload {
        epoch: 0,
        ring_signature: String::new(),
        key_image: hex::encode(
            wrfcoin_privacy::KeyImage::generate(&keys[0].0)
                .unwrap()
                .to_bytes(),
        ),
        amount_commitment: "ab".repeat(32),
        range_proof: "ff".repeat(32),
        destination: "local-destination".into(),
    };
    let decoys = keys[1..]
        .iter()
        .map(|(_, pk)| wrfcoin_privacy::RingMember { public_key: *pk })
        .collect::<Vec<_>>();
    let sig = wrfcoin_privacy::MLSAGSignature::sign(
        &payload.authorization_message(),
        &keys[0].0,
        &decoys,
    )
    .unwrap();
    payload.ring_signature = hex::encode(serde_json::to_vec(&sig).unwrap());
    let mut tx = transaction(TransactionType::RewardClaim);
    tx.data = Some(serde_json::to_string(&payload).unwrap());
    tx.set_id();
    (tx, stations)
}

#[tokio::test]
async fn local_b3_gossip_producer_and_peer_verdicts() {
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Warn)
        .try_init();
    wrfcoin_shared_protocol::crypto::chain_binding::install_activation_table(
        wrfcoin_shared_protocol::crypto::chain_binding::ActivationTable::default(),
    );
    wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion::install_active(
        wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion::V1,
    );
    assert!(!cfg!(feature = "zk"));
    assert!(
        cfg!(feature = "full-mining")
            && cfg!(feature = "p2p")
            && cfg!(feature = "hsm")
            && cfg!(feature = "upgrade-notice-fetch")
    );
    let config = P2pConfig {
        listen_addresses: vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()],
        max_peers_per_subnet: 0,
        genesis_hash: "ab".repeat(32),
        network_id: "testnet".into(),
        chain_id: "777".into(),
        ..P2pConfig::default()
    };
    let mut b = P2pNode::start(config.clone()).await.unwrap();
    let address = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(P2pEvent::ListeningAddress(addr)) = b.event_rx.recv().await {
                break addr;
            }
        }
    })
    .await
    .unwrap();
    let mut a = P2pNode::start(P2pConfig {
        bootstrap_peers: vec![address.with(libp2p::multiaddr::Protocol::P2p(b.local_peer_id))],
        ..config
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Some(P2pEvent::SubscriptionStateChanged {
                subscribed: true, ..
            }) = a.event_rx.recv().await
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    let (reward, stations) = reward();
    for tx in [confidential(), reward] {
        let raw = bincode::serialize(&tx).unwrap();
        a.command_tx
            .send(P2pCommand::BroadcastTransactions(vec![raw.clone()]))
            .await
            .unwrap();
        let received = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if let Some(P2pEvent::NewTransactions(bytes)) = b.event_rx.recv().await {
                    break bytes;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(received, vec![raw]);
        let decoded: Transaction = bincode::deserialize(&received[0]).unwrap();
        let mut initial = Blockchain::default();
        initial
            .state
            .create_account(Address([0x56; 20]), Balance::new(1_000_000));
        for station in &stations {
            initial
                .state
                .station_registry
                .register(station.clone())
                .unwrap();
        }
        initial.state.recalculate_state_root();
        let chain = Arc::new(RwLock::new(initial));
        let pool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        pool.write()
            .await
            .add_transaction_with_signing_keys(
                decoded.clone(),
                Some(&chain.read().await.state.signing_keys),
            )
            .unwrap();
        let mut producer = ProofOfPublishing::new(1, 60, 0, 10);
        producer.declare_no_genesis_oracles();
        producer.set_clock(Arc::new(PinnedClock));
        let block = producer
            .create_next_block(chain.clone(), pool.clone(), &hex::encode([0x42; 20]))
            .await
            .unwrap();
        assert!(block.transactions.iter().any(|t| t.id == decoded.id));
        assert_eq!(chain.read().await.height, 1);
        let block_wire = bincode::serialize(&block).unwrap();
        b.command_tx
            .send(P2pCommand::BroadcastBlock(block_wire.clone()))
            .await
            .unwrap();
        let peer_wire = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if let Some(P2pEvent::NewBlock(bytes)) = a.event_rx.recv().await {
                    break bytes;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(peer_wire, block_wire);
        let block = bincode::deserialize(&peer_wire).unwrap();
        let mut peer = ProofOfPublishing::new(1, 60, 0, 10);
        peer.set_clock(Arc::new(PinnedClock));
        let accepted = peer
            .verify_block(&block, &Default::default(), 1, &[])
            .await
            .unwrap();
        assert_eq!(accepted, decoded.tx_type == TransactionType::RewardClaim);
        use wrfcoin_shared_protocol::crypto::domain_signing::DomainSeparated;
        let consensus_bytes = block.signing_preimage_for(
            &wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::new("", ""),
            wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion::V1,
        );
        let fixture = serde_json::json!({"block": block, "block_bytes": hex::encode(peer_wire), "consensus_bytes": hex::encode(consensus_bytes), "stations": stations, "accepted": accepted,
            "state_root": chain.read().await.state.state_root.to_string()});
        std::fs::write(
            std::env::temp_dir().join(format!("b3-{}.json", decoded.tx_type)),
            serde_json::to_vec_pretty(&fixture).unwrap(),
        )
        .unwrap();
        eprintln!(
            "B3 LOCAL: {:?}, gossip/admission/inclusion=true, producer height=1, peer accepted={accepted}",
            decoded.tx_type
        );
    }
    a.command_tx.send(P2pCommand::Shutdown).await.unwrap();
    b.command_tx.send(P2pCommand::Shutdown).await.unwrap();
}
