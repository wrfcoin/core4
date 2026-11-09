//! #3336: fixed-point station coordinates (`station_coordinates_fixed_point`).
//!
//! Station-registration governance signatures covered raw f64 bits that every
//! verifier recovers through serde_json's float parse, which is not correctly
//! rounded. From the feature's activation H the preimage and the stored record
//! use integers (`latitude_e7`, `longitude_e7`, `elevation_mm`).
//!
//! Two child processes (the feature schedule is process-wide):
//! - **gate off**: a legacy registration applies exactly as on main, also when
//!   its JSON carries the new integer fields (ignored); a registration signed
//!   over the fixed-point preimage does not verify.
//! - **gate on (H = 3)**: a legacy registration in a block below H applies as on
//!   main; from H a legacy registration yields nothing and a fixed-point one
//!   applies with coordinates from the integers, even when its f64 fields are
//!   wrong (the #3333 pair: `-92.04194742607375`). Producer (`create_next_block`),
//!   applier (`extract_transitions` → `apply_transitions_batch_at_height`) and
//!   WAL replay agree on `state_root` and on every station record at every
//!   height.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;

mod helpers {
    pub(crate) mod test_env {
        // Mount the canonical test-only lock and fixture pins (#3521).
        include!("../src/bin/launch_api/test_env.rs");
    }
}

use std::sync::Arc;

use tokio::sync::RwLock;
use transition_extractor::extract_transitions;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::blockchain::transaction::{
    StationRegistrationPayload, Transaction, TransactionInput, TransactionOutput, TransactionType,
    format_degrees_e7, parse_degrees_e7,
};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, GENESIS_CHAIN_ID_KEY, feature_active,
    refresh_feature_schedule_extensions,
};
use wrfcoin_shared_protocol::crypto::hash::{Hash, hash_string};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

const H: u64 = 3;
const FEATURE: &str = "station_coordinates_fixed_point";
const CHILD_ENV: &str = "WRFCOIN_STATION_FIXED_POINT_CHILD_3336";
const TEST_NAME: &str = "station_coordinates_fixed_point_producer_applier_wal_3336";
/// #3333: serde_json parses this back one ulp off the correctly rounded value.
const LON_3333: &str = "-92.04194742607375";

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}
fn gov() -> PrivateKey {
    key(0x60)
}
fn chain_ids() -> (String, String) {
    let i = ChainIdentity::active();
    (i.chain_id.clone(), i.network_id.clone())
}

fn tx_of(tag: &str, payload: &StationRegistrationPayload) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("fixed-3336-input-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1,
            address: format!("wrf-{tag}"),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::StationRegistration;
    tx.data = Some(serde_json::to_string(payload).unwrap());
    tx.set_id();
    tx
}

fn unsigned(station: &str, owner: &PrivateKey, lat: f64, lon: f64) -> StationRegistrationPayload {
    StationRegistrationPayload::new(
        station,
        PublicKey::from_private_key(owner).to_address(),
        lat,
        lon,
        "fixed-point-fixture",
        DataQualityTier::Gold,
        ContributionProvenanceClass::Real,
    )
}

/// Signed over the legacy f64-bit preimage.
fn legacy(station: &str, owner: &PrivateKey, lat: f64, lon: f64) -> StationRegistrationPayload {
    let (c, n) = chain_ids();
    let mut p = unsigned(station, owner, lat, lon);
    p.elevation_meters = Some(220.5);
    p.sign_ownership_with_domain(&c, &n, owner);
    p.sign_with_domain(&c, &n, &gov());
    p
}

/// Signed over the fixed-point preimage.
fn fixed(station: &str, owner: &PrivateKey, lat: &str, lon: &str, mm: i64) -> StationRegistrationPayload {
    let (c, n) = chain_ids();
    let (lat_e7, lon_e7) = (parse_degrees_e7(lat).unwrap(), parse_degrees_e7(lon).unwrap());
    // The f64 fields are whatever serde_json makes of the text: NOT signed.
    let mut p = unsigned(station, owner, lat.parse().unwrap(), lon.parse().unwrap())
        .with_fixed_coordinates(lat_e7, lon_e7, Some(mm));
    p.sign_ownership_with_domain(&c, &n, owner);
    assert!(p.sign_fixed_with_domain(&c, &n, &gov()));
    p
}

fn record(state: &State, station: &str) -> Option<(u64, u64, Option<u64>)> {
    state.station_registry.get(station).map(|s| {
        (
            s.location.latitude.to_bits(),
            s.location.longitude.to_bits(),
            s.location.elevation_meters.map(f64::to_bits),
        )
    })
}

#[test]
fn station_coordinates_fixed_point_producer_applier_wal_3336() {
    let Some(mode) = std::env::var_os(CHILD_ENV) else {
        for mode in ["off", "on"] {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST_NAME, "--nocapture"])
                .env(CHILD_ENV, mode)
                .env("WRFCOIN_CHAIN_ID", "777")
                .env("WRFCOIN_NETWORK_ID", "testnet")
                .env("WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS", PublicKey::from_private_key(&gov()).to_hex())
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success() && stdout.contains("1 passed"),
                "child ({mode}) failed:\n{stdout}\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        return;
    };
    let feature_on = mode == "on";
    if feature_on {
        refresh_feature_schedule_extensions(vec![(FEATURE.to_string(), H)]);
    }
    assert_eq!(feature_active(ConsensusFeature::StationCoordinatesFixedPoint, H), feature_on);
    assert!(!feature_active(ConsensusFeature::StationCoordinatesFixedPoint, H - 1));
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(run(feature_on));
}

async fn run(feature_on: bool) {
    let owner = key(0xA1);
    let mut genesis = Block::new(1, Hash::default(), Vec::new(), 1, 1);
    genesis.metadata.insert(GENESIS_CHAIN_ID_KEY.into(), "777".into());
    let genesis_transitions = extract_transitions(&State::new(), &genesis, 1, 0);
    let mut applier = State::new();
    applier.apply_transitions_batch_at_height(1, genesis_transitions.clone()).unwrap();
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    {
        let mut c = chain.write().await;
        c.height = 1;
        c.state = applier.clone();
    }
    let db = std::env::temp_dir().join(format!(
        "station-fixed-3336-{}-{}.redb",
        std::process::id(),
        feature_on
    ));
    let _ = std::fs::remove_file(&db);
    let mut storage = Storage::open(&db, DEFAULT_CHECKPOINT_INTERVAL).unwrap();
    storage.persist_block(genesis, genesis_transitions, None).await.unwrap();
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let miner = Address([0x41; 20]).to_string();

    // The #3333 pair as a fixed-point payload whose f64 longitude is one ulp off.
    let mut ulp_off = fixed("ws-3333", &owner, "46.7867", LON_3333, 200_000);
    ulp_off.longitude = f64::from_bits(ulp_off.longitude.to_bits() + 1);
    // A legacy-signed payload that also carries the new fields (ignored while off).
    let mut legacy_with_fields = legacy("ws-extra", &owner, 46.7867, -92.1005);
    legacy_with_fields.latitude_e7 = Some(467_867_000);
    legacy_with_fields.longitude_e7 = Some(-921_005_000);
    let plan: Vec<(u64, Vec<(&str, StationRegistrationPayload)>)> = vec![
        (2, vec![
            ("legacy-below-h", legacy("ws-legacy", &owner, 46.7867, -92.1005)),
            ("legacy-extra-fields", legacy_with_fields.clone()),
        ]),
        (3, vec![
            ("legacy-at-h", legacy("ws-late", &owner, 10.0, 20.0)),
            ("fixed-at-h", fixed("ws-fixed", &owner, "-89.9999999", "179.9999999", -12_345)),
        ]),
        (4, vec![("fixed-3333", ulp_off.clone())]),
    ];

    for (height, txs) in plan {
        for (tag, payload) in txs {
            let tx = tx_of(tag, &payload);
            let keys = chain.read().await.state.signing_keys.clone();
            mempool
                .write()
                .await
                .add_transaction_with_signing_keys_at_height(tx, Some(&keys), Some(height - 1))
                .unwrap_or_else(|e| {
                    // Admission refusing what consensus refuses is fine; record it.
                    eprintln!("admission refused {tag} at {height}: {e}");
                });
        }
        let before = applier.clone();
        let block = pop
            .create_next_block(chain.clone(), mempool.clone(), &miner)
            .await
            .expect("the real producer builds the block");
        assert_eq!(block.height, height);
        let producer = chain.read().await.state.clone();
        let transitions = extract_transitions(&before, &block, block.height, block.timestamp.timestamp());
        applier.apply_transitions_batch_at_height(block.height, transitions.clone()).unwrap();
        storage.persist_block(block, transitions, None).await.unwrap();
        assert_eq!(applier.state_root, producer.state_root, "applier root at {height}");
        for station in ["ws-legacy", "ws-extra", "ws-late", "ws-fixed", "ws-3333"] {
            assert_eq!(record(&applier, station), record(&producer, station), "{station} at {height}");
        }
    }
    let producer = chain.read().await.state.clone();

    let rows = storage.load_wal_transitions_range(1, 4).unwrap();
    let mut wal = State::new();
    for height in 1..=4 {
        wal.apply_transitions_batch_at_height(
            height,
            rows.iter().filter(|(h, _)| *h == height).map(|(_, t)| t.clone()),
        )
        .unwrap();
    }
    assert_eq!(wal.state_root, producer.state_root, "WAL replay root");

    // Below H, on and off alike: legacy registrations apply as on main.
    let legacy_bits = (46.7867f64.to_bits(), (-92.1005f64).to_bits(), Some(220.5f64.to_bits()));
    assert_eq!(record(&producer, "ws-legacy"), Some(legacy_bits));
    assert_eq!(record(&producer, "ws-extra"), Some(legacy_bits), "the integer fields are ignored below H");
    if feature_on {
        assert_eq!(record(&producer, "ws-late"), None, "from H a legacy registration yields nothing");
        let fixed_record = record(&producer, "ws-fixed").expect("fixed-point registration applied at H");
        assert_eq!(fixed_record.0, (-899_999_999f64 / 1e7).to_bits());
        assert_eq!(fixed_record.1, (1_799_999_999f64 / 1e7).to_bits());
        assert_eq!(fixed_record.2, Some((-12_345f64 / 1e3).to_bits()));
        let r = record(&producer, "ws-3333").expect("the #3333 pair verifies under the fixed preimage");
        assert_eq!(r.1, (parse_degrees_e7(LON_3333).unwrap() as f64 / 1e7).to_bits());
        assert_ne!(r.1, ulp_off.longitude.to_bits(), "stored from the integer, not the f64 field");
        assert_eq!(format_degrees_e7(parse_degrees_e7(LON_3333).unwrap()), "-92.0419474");
    } else {
        assert_eq!(
            record(&producer, "ws-late"),
            Some((10f64.to_bits(), 20f64.to_bits(), Some(220.5f64.to_bits()))),
            "gate off: legacy registrations apply at every height, as on main"
        );
        assert_eq!(record(&producer, "ws-fixed"), None, "gate off: a fixed-point signature does not verify");
        assert_eq!(record(&producer, "ws-3333"), None);
    }

    drop(storage);
    std::fs::remove_file(db).unwrap();
}
