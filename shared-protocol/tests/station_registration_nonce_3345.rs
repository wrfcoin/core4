//! `ConsensusFeature::StationRegistrationNonce` (#3345): a governance station
//! registration binds the station's owner-ledger nonce.
//!
//! Scheduled at H = 3 (test only; unscheduled it changes nothing). Covers:
//! - below H: v1 behaves as on main and a v2 payload is inert;
//! - from H: the v1 format is refused ENTIRELY (an already-mined v1 payload does
//!   not apply again), and only v2 with the matching `expected_nonce` applies;
//! - applying a v2 registration ADVANCES the per-station nonce, so the same v2
//!   payload does not apply twice;
//! - an owner action since signing invalidates an in-flight registration;
//! - a v2 registration cannot change the owner once `station_owner_consent` is on;
//! - the v2 preimage binds the nonce and a v1 signature never verifies as v2;
//! - the production path (`create_next_block`) agrees.
//!
//! One test: the schedule is process-wide (first-install-wins), so this file
//! owns its process.
//!
//! Run from core4:
//!   cargo test -p wrfcoin-shared-protocol --test station_registration_nonce_3345

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::RwLock;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::blockchain::station_owner::{
    StationOwnerActionPayload, StationRegistrationRefusal,
};
use wrfcoin_shared_protocol::blockchain::transaction::{
    StationOwnerActionKind, StationRegistrationPayload, Transaction, TransactionInput,
    TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion,
    feature_active, install_activation_table, install_feature_schedule,
    resolve_activation_schedule, resolve_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";
const H: u64 = 3;

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}
fn addr(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_address()
}
fn chain() -> (String, String) {
    let i = ChainIdentity::active();
    (i.chain_id.clone(), i.network_id.clone())
}
fn gov() -> PrivateKey {
    key(0x60)
}

fn tx_of(tag: &str, data: String) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("nonce-input-{tag}")),
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
    tx.data = Some(data);
    tx.set_id();
    tx
}

/// An unsigned payload for `station`, owner `owner`, with `elevation` as a
/// harmless way to make two payloads for one station differ.
fn base(station: &str, owner: &PrivateKey, elevation: f64) -> StationRegistrationPayload {
    let mut p = StationRegistrationPayload::new(
        station,
        addr(owner),
        46.7867,
        -92.1005,
        "test-real-station",
        DataQualityTier::Gold,
        ContributionProvenanceClass::Real,
    );
    p.elevation_meters = Some(elevation);
    p
}
fn v1(station: &str, owner: &PrivateKey, elevation: f64) -> Transaction {
    let mut p = base(station, owner, elevation);
    p.sign_ownership(owner);
    p.sign(&gov());
    tx_of(
        &format!("{station}-v1-{elevation}"),
        serde_json::to_string(&p).unwrap(),
    )
}
fn v2(station: &str, owner: &PrivateKey, elevation: f64, nonce: u64) -> Transaction {
    let mut p = base(station, owner, elevation).with_expected_nonce(nonce);
    p.sign_ownership(owner);
    p.sign(&gov());
    tx_of(
        &format!("{station}-v2-{elevation}-{nonce}"),
        serde_json::to_string(&p).unwrap(),
    )
}

/// Extract through the ONE shared step, then apply. `true` if a transition was
/// extracted.
fn run(state: &mut State, tx: &Transaction, height: u64) -> bool {
    match tx.station_registration_step(height) {
        Some(t) => {
            state.apply_transition(t).expect("an extracted step never fails the block");
            true
        },
        None => false,
    }
}
fn nonce(state: &State, station: &str) -> u64 {
    state.station_owner_ledger_view(station).owner_nonce
}
fn elevation(state: &State, station: &str) -> Option<f64> {
    state
        .station_registry
        .get(station)
        .and_then(|s| s.location.elevation_meters)
}
fn owner_of(state: &State, station: &str) -> Option<String> {
    state.station_registry.get(station).map(|s| s.owner_address.clone())
}

#[tokio::test]
async fn registrations_bind_the_station_nonce() {
    std::env::set_var(GOVERNANCE_ENV, PublicKey::from_private_key(&gov()).to_hex());
    let (a, b) = (key(0xA1), key(0xB2));

    // ── Unscheduled: a v2 payload is inert at every height; v1 is as on main. ──
    let mut state = State::new();
    assert!(!run(&mut state, &v2("ws-pre", &a, 1.0, 0), 5), "v2 is inert while unscheduled");
    assert!(run(&mut state, &v1("ws-pre", &a, 1.0), 5), "v1 applies as on main");
    assert_eq!(nonce(&state, "ws-pre"), 0, "no ledger entry, nonce untouched");

    // ── Schedule the feature (and the two owner features) at H. ───────────────
    let raw = format!(
        "0:v1,{H}:{},{H}:{},{H}:{}",
        ConsensusFeature::StationRegistrationNonce.name(),
        ConsensusFeature::StationOwnerConsent.name(),
        ConsensusFeature::StationOwnerActions.name(),
    );
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    assert!(feature_active(ConsensusFeature::StationRegistrationNonce, H));
    assert!(!feature_active(ConsensusFeature::StationRegistrationNonce, H - 1));

    // ── Below H: v1 applies, v2 is still inert. ───────────────────────────────
    let mut state = State::new();
    let mined_v1 = v1("ws-a", &a, 10.0);
    assert!(run(&mut state, &mined_v1, H - 1), "below H: v1 applies");
    assert!(!run(&mut state, &v2("ws-a", &a, 11.0, 0), H - 1), "below H: v2 is inert");
    assert_eq!(elevation(&state, "ws-a"), Some(10.0));

    // ── From H: the v1 format is refused ENTIRELY. ────────────────────────────
    assert!(
        !run(&mut state, &mined_v1, H),
        "an already-mined v1 payload does not apply again once the feature is active"
    );
    assert!(
        !run(&mut state, &v1("ws-new", &a, 1.0), H),
        "and no NEW v1 registration applies either"
    );
    assert_eq!(owner_of(&state, "ws-new"), None);

    // ── From H: v2 with the matching nonce applies and advances it. ───────────
    // The station already exists (v1 era) with no ledger entry: nonce 0.
    let first = v2("ws-a", &a, 20.0, 0);
    assert!(run(&mut state, &first, H));
    assert_eq!(elevation(&state, "ws-a"), Some(20.0), "the v2 registration applied");
    assert_eq!(nonce(&state, "ws-a"), 1, "applying advances owner_nonce");

    // The SAME v2 payload does not apply twice (its signature is still valid).
    let before_root = state.state_root;
    assert!(run(&mut state, &first, H + 1), "it still extracts (the signature is valid)...");
    assert_eq!(elevation(&state, "ws-a"), Some(20.0));
    assert_eq!(nonce(&state, "ws-a"), 1, "...but is an inert no-op: nonce unchanged");
    assert_eq!(state.state_root, before_root, "state is byte-identical after the repeat");
    let stale = v2("ws-a", &a, 99.0, 0);
    assert!(run(&mut state, &stale, H + 1));
    assert_eq!(elevation(&state, "ws-a"), Some(20.0), "a stale expected_nonce never applies");

    // A brand-new station's first v2 registration uses nonce 0 (no ledger entry).
    assert!(run(&mut state, &v2("ws-b", &a, 1.0, 0), H));
    assert_eq!(owner_of(&state, "ws-b"), Some(addr(&a)));
    assert_eq!(nonce(&state, "ws-b"), 1);

    // ── Admission precheck agrees with apply. ─────────────────────────────────
    let st = state.station_registry.get("ws-a").unwrap().clone();
    assert_eq!(
        state.precheck_station_registration_v2(&st, 0, H).map(|_| ()),
        Err(StationRegistrationRefusal::NonceMismatch)
    );
    assert!(state.precheck_station_registration_v2(&st, 1, H).is_ok());

    // ── An owner action since signing invalidates an in-flight registration. ──
    let in_flight = v2("ws-a", &a, 30.0, 1); // signed against nonce 1
    let (c, n) = chain();
    let mut act = StationOwnerActionPayload::new(
        "ws-a",
        addr(&a),
        PublicKey::from_private_key(&a).to_hex(),
        StationOwnerActionKind::Deactivate,
        1,
        H + 100,
    );
    act.sign_with_domain(&c, &n, &a);
    let action_tx = tx_of("owner-act", serde_json::to_string(&act).unwrap());
    state
        .apply_transition(action_tx.station_owner_ledger_transition(H).expect("action extracts"))
        .unwrap();
    assert_eq!(nonce(&state, "ws-a"), 2, "the owner action consumed the shared nonce");
    assert!(run(&mut state, &in_flight, H + 1));
    assert_eq!(elevation(&state, "ws-a"), Some(20.0), "the in-flight registration did not apply");
    // Re-signed against the new nonce, it applies.
    assert!(run(&mut state, &v2("ws-a", &a, 30.0, 2), H + 1));
    assert_eq!(elevation(&state, "ws-a"), Some(30.0));
    assert_eq!(nonce(&state, "ws-a"), 3);

    // ── Consent: a v2 registration cannot change the owner. ───────────────────
    let hijack = v2("ws-a", &b, 40.0, 3);
    assert!(run(&mut state, &hijack, H + 1));
    assert_eq!(owner_of(&state, "ws-a"), Some(addr(&a)), "owner unchanged");
    assert_eq!(nonce(&state, "ws-a"), 3, "a refused registration does not advance the nonce");
    let st = state.station_registry.get("ws-a").unwrap().clone();
    let mut moved = st.clone();
    moved.owner_address = addr(&b);
    assert_eq!(
        state.precheck_station_registration_v2(&moved, 3, H).map(|_| ()),
        Err(StationRegistrationRefusal::OwnerChangeWithoutConsent)
    );

    // ── Shape and preimage. ───────────────────────────────────────────────────
    let mut bad = base("ws-shape", &a, 1.0);
    bad.expected_nonce = Some(1); // v1 kind carrying a nonce
    assert!(bad.validate().is_err());
    let mut bad = base("ws-shape", &a, 1.0).with_expected_nonce(1);
    bad.expected_nonce = None; // v2 kind without one
    assert!(bad.validate().is_err());
    let unsigned_v1 = base("ws-shape", &a, 1.0);
    let unsigned_v2 = base("ws-shape", &a, 1.0).with_expected_nonce(7);
    let (c, n) = chain();
    use wrfcoin_shared_protocol::blockchain::transaction::station_registration_authorization_message_with_domain as msg;
    assert_ne!(msg(&c, &n, &unsigned_v1), msg(&c, &n, &unsigned_v2), "v2 has its own tag and tail");
    assert_ne!(
        msg(&c, &n, &unsigned_v2),
        msg(&c, &n, &base("ws-shape", &a, 1.0).with_expected_nonce(8)),
        "the nonce is bound into the governance preimage"
    );
    // Tampering expected_nonce after signing breaks the governance signature.
    let mut tampered: StationRegistrationPayload =
        serde_json::from_str(v2("ws-t", &a, 1.0, 0).data.as_deref().unwrap()).unwrap();
    tampered.expected_nonce = Some(5);
    let t_tx = tx_of("tampered", serde_json::to_string(&tampered).unwrap());
    assert!(t_tx.station_registration_step(H).is_none(), "a re-nonced payload fails authorization");

    // ── Production path: create_next_block agrees. ────────────────────────────
    let mut rng = StdRng::seed_from_u64(9);
    let miner = PrivateKey::generate(&mut rng);
    let miner_hex = format!(
        "0x{}",
        hex::encode(
            wrfcoin_shared_protocol::common::types::Address::from_public_key(
                &PublicKey::from_private_key(&miner)
            )
            .0
        )
    );
    let chain_arc = Arc::new(RwLock::new(Blockchain::default()));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    pop.declare_no_genesis_oracles();
    let offer = |tx: Transaction| {
        let chain_arc = chain_arc.clone();
        let mempool = mempool.clone();
        async move {
            let keys = chain_arc.read().await.state.signing_keys.clone();
            mempool
                .write()
                .await
                .add_transaction_with_signing_keys(tx, Some(&keys))
                .expect("admitted to the mempool");
        }
    };
    // Heights: block 1 and 2 are below H (v1 era); block 3 is the first at H.
    let prod_v1 = v1("ws-prod", &a, 1.0);
    offer(prod_v1.clone()).await;
    pop.create_next_block(chain_arc.clone(), mempool.clone(), &miner_hex).await.unwrap();
    offer(v1("ws-filler", &a, 2.0)).await;
    pop.create_next_block(chain_arc.clone(), mempool.clone(), &miner_hex).await.unwrap();
    assert_eq!(chain_arc.read().await.height, 2);
    assert_eq!(
        chain_arc.read().await.state.station_registry.get("ws-prod").map(|s| s.location.elevation_meters),
        Some(Some(1.0))
    );
    // At H a v1 payload is re-wrapped in a FRESH envelope (a new input txid and
    // a new timestamp, so the mempool and the producer window both take it).
    // Each case below is a block of its own, so the outcome never depends on
    // the order in which a block applies its transactions: a producer arm that
    // still applied v1 would change the state in the v1-only blocks, and could
    // not be hidden by a later v2 write.
    let rewrap = |tag: &str, elevation: f64| {
        let mut t = v1("ws-prod", &a, elevation);
        t.inputs[0].txid = hash_string(tag);
        t.timestamp = chrono::Utc::now();
        t.set_id();
        t
    };
    let state_of = |c: &Blockchain| {
        (
            c.state.station_registry.get("ws-prod").map(|s| s.location.elevation_meters),
            c.state.station_owner_ledger_view("ws-prod").owner_nonce,
        )
    };
    // Case 1 (v1 alone, before any v2): the state does not move.
    offer(rewrap("fresh-envelope-1", 9.0)).await;
    pop.create_next_block(chain_arc.clone(), mempool.clone(), &miner_hex).await.unwrap();
    {
        let c = chain_arc.read().await;
        assert_eq!(c.height, 3);
        assert_eq!(state_of(&c), (Some(Some(1.0)), 0), "at H a re-wrapped v1 changes nothing");
    }
    // Case 2: a v2 registration applies and advances the nonce.
    offer(v2("ws-prod", &a, 5.0, 0)).await;
    pop.create_next_block(chain_arc.clone(), mempool.clone(), &miner_hex).await.unwrap();
    {
        let c = chain_arc.read().await;
        assert_eq!(c.height, 4);
        assert_eq!(state_of(&c), (Some(Some(5.0)), 1), "at H the v2 registration applied");
    }
    // Case 3 (v1 alone, AFTER the v2): it still changes nothing.
    offer(rewrap("fresh-envelope-2", 9.0)).await;
    pop.create_next_block(chain_arc.clone(), mempool.clone(), &miner_hex).await.unwrap();
    {
        let c = chain_arc.read().await;
        assert_eq!(c.height, 5);
        assert_eq!(state_of(&c), (Some(Some(5.0)), 1), "a re-wrapped v1 after the v2 changes nothing");
    }
    // Case 4: both in one block, v1 offered before v2 and again after; the v2
    // value is the only one that may appear, and the nonce moves exactly once.
    offer(rewrap("fresh-envelope-3", 9.0)).await;
    offer(v2("ws-prod", &a, 7.0, 1)).await;
    offer(rewrap("fresh-envelope-4", 8.0)).await;
    pop.create_next_block(chain_arc.clone(), mempool.clone(), &miner_hex).await.unwrap();
    {
        let c = chain_arc.read().await;
        assert_eq!(c.height, 6);
        assert_eq!(state_of(&c), (Some(Some(7.0)), 2), "mixed block: only the v2 applied");
    }
}
