//! `attester_pay_v2` changes nothing while it is not scheduled.
//!
//! Approve votes, reject votes that reach the reject threshold, mixed votes, a
//! repeated vote, a flipped vote, a straggler, three oracle registrations whose
//! JSON carries a `payout_bond` member (a 64-hex string, a number, and the
//! member twice) and two attester settlements run through the real `State`
//! apply path with the feature unscheduled. The state-root material and the
//! snapshot export must be byte-identical to the ones `origin/main` efc9cc5
//! produced for the same transitions.
//!
//! How the pins were taken: this file was first written against an unmodified
//! checkout of efc9cc5 (before any source change of this feature), without
//! the block marked "this binary only" in the test (it names items efc9cc5
//! does not have) and with `registration_with_payout_bond` returning only the
//! transition; `run()`, its transitions and both digest functions are
//! unchanged since. The two digests that run printed are the constants below.
//!
//! Own test binary: the feature schedule is process-wide, and this file must
//! never schedule the feature.

use sha3::{Digest, Sha3_256};
use wrfcoin_shared_protocol::blockchain::state::{ATTESTER_PAYOUT_BONDS_ROOT_TAG, State, StateTransition};
use wrfcoin_shared_protocol::blockchain::state_snapshot::export_state_entries;
use wrfcoin_shared_protocol::blockchain::transaction::{OracleAttestationPayload, Transaction};
use wrfcoin_shared_protocol::consensus::proof_of_publishing::EmissionRole;
use wrfcoin_shared_protocol::crypto::chain_binding::{ConsensusFeature, FeatureSchedule, feature_active};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

const T0: i64 = 1_700_000_000;
const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";

/// SHA3-256 of `State::state_root_material()` after `run()`, on origin/main efc9cc5.
const ROOT_MATERIAL_SHA3: &str = "73c2252118c1dfc12fcc35e2e054b7f978a5c3230f73e7812dd3643ef8bdda07"; // pragma: allowlist secret -- state digest, not a secret
/// SHA3-256 over the snapshot export (each key and value, length-prefixed),
/// on origin/main efc9cc5.
const SNAPSHOT_SHA3: &str = "abc29ca4261aa743b614e6fb0b3ec13112014989bb974472fec69418e06e0735"; // pragma: allowlist secret -- state digest, not a secret

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).unwrap()
}

fn identity(seed: u8) -> String {
    PublicKey::from_private_key(&key(seed)).to_hex()
}

fn vote(decision: &str, seed: u8, approved: bool, tag: &str) -> StateTransition {
    let mut payload = OracleAttestationPayload {
        commit_id: decision.to_string(),
        oracle_id: identity(seed),
        approved,
        signature: None,
        confidence: Some(1.0),
        timestamp: T0,
        nonce: Some(format!("nonce-{decision}-{seed}-{tag}")),
    };
    payload.signature = Some(key(seed).sign(payload.signing_message().as_bytes()).to_string());
    StateTransition::RecordOracleAttestation { payload, current_block: 0 }
}

fn at(height: u64, transition: StateTransition) -> StateTransition {
    match transition {
        StateTransition::RecordOracleAttestation { payload, .. } => {
            StateTransition::RecordOracleAttestation { payload, current_block: height }
        },
        other => other,
    }
}

/// Settle the whole outstanding debt at `height`, exactly as a producer stages it.
fn settle(state: &mut State, height: u64) -> usize {
    let due: Vec<String> = state.attester_settlement_due.keys().cloned().collect();
    let slice = state.emission_ledger.remaining(EmissionRole::Attester);
    let records = state.derive_attester_settlements(&due, height, slice).expect("the debt is settleable");
    let settled = records.len();
    state
        .apply_transition(StateTransition::ApplyAttesterSettlements { records })
        .expect("settlement applies");
    settled
}

/// A governance-signed registration (signed over the fields every binary
/// knows) whose JSON then gains a `payout_bond` member in the given shape.
fn registration_with_payout_bond(set: &[String], seed: u8, epoch: u64, shape: u8) -> (Transaction, StateTransition) {
    let governance = key(0x77);
    let mut tx = Transaction::new_oracle_registration(
        set.to_vec(),
        2,
        &identity(seed),
        &PublicKey::from_private_key(&key(seed.wrapping_add(0x40))).to_hex(),
        epoch,
        T0 + 1_000,
        &governance,
    );
    let data = tx.data.clone().expect("registration data");
    let member = match shape {
        0 => format!("\"payout_bond\":\"{}\",", "ab".repeat(32)),
        1 => "\"payout_bond\":7,".to_string(),
        _ => format!("\"payout_bond\":{{\"a\":1}},\"payout_bond\":\"{}\",", "cd".repeat(32)),
    };
    tx.data = Some(data.replacen('{', &format!("{{{member}"), 1));
    let payload = tx.parse_oracle_registration().expect("an unknown member does not stop the parse");
    assert!(tx.verify_oracle_registration_authorization(), "the signature covers the known fields only");
    let transition = StateTransition::ApplyOracleRegistration {
        oracle_set: payload.oracle_set,
        oracle_quorum: payload.oracle_quorum,
        identity: payload.identity,
        public_key: payload.public_key,
        epoch: payload.epoch,
        activated_at: payload.activated_at,
    };
    (tx, transition)
}

fn run() -> State {
    std::env::set_var(GOVERNANCE_ENV, PublicKey::from_private_key(&key(0x77)).to_hex());
    let set: Vec<String> = (1u8..=4).map(identity).collect();
    let mut state = State::new();
    state
        .apply_transition(StateTransition::SetOracleConfig { oracle_set: set.clone(), oracle_quorum: 2 })
        .expect("oracle configuration applies");

    // Block 100: an approved decision, a rejected one (3 of 4 reject, so
    // approval is impossible) and a split one.
    state.open_emission_block(100);
    for transition in [
        vote("d-approve", 1, true, "a"),
        vote("d-approve", 2, true, "a"),
        vote("d-reject", 1, false, "a"),
        vote("d-reject", 2, false, "a"),
        vote("d-reject", 3, false, "a"),
        vote("d-mixed", 1, true, "a"),
        vote("d-mixed", 2, false, "a"),
        vote("d-mixed", 3, false, "a"),
        // The same vote again: refused, no slash.
        vote("d-approve", 1, true, "again"),
    ] {
        state.apply_transition(at(100, transition)).expect("vote applies");
    }
    // A flipped vote is refused.
    assert!(state.apply_transition(at(100, vote("d-reject", 1, true, "flip"))).is_err());
    assert_eq!(state.attester_settlement_due.keys().collect::<Vec<_>>(), ["d-approve"]);
    assert_eq!(settle(&mut state, 100), 1);
    assert!(state.attester_settlement_due.is_empty());

    // Block 101: the split decision reaches approve quorum, the rejected one
    // gets its fourth reject vote, and the approved one a straggler.
    state.open_emission_block(101);
    for transition in [
        vote("d-mixed", 4, true, "b"),
        vote("d-reject", 4, false, "b"),
        vote("d-approve", 3, true, "b"),
        vote("d-approve", 4, false, "b"),
    ] {
        state.apply_transition(at(101, transition)).expect("vote applies");
    }
    assert_eq!(state.attester_settlement_due.keys().collect::<Vec<_>>(), ["d-mixed"]);
    assert_eq!(settle(&mut state, 101), 1);

    // Block 102: three registrations whose JSON names `payout_bond`.
    state.open_emission_block(102);
    for (seed, epoch, shape) in [(2u8, 1u64, 0u8), (3, 1, 1), (4, 1, 2)] {
        state
            .apply_transition(registration_with_payout_bond(&set, seed, epoch, shape).1)
            .expect("registration applies");
    }
    assert_eq!(
        state.attester_settlements.keys().collect::<Vec<_>>(),
        ["d-approve", "d-mixed"],
        "the rejected decision is never owed or paid while inactive"
    );
    state
}

fn root_digest(state: &State) -> String {
    hex::encode(Sha3_256::digest(state.state_root_material()))
}

fn snapshot_digest(state: &State) -> String {
    let mut hasher = Sha3_256::new();
    for (k, v) in export_state_entries(state).expect("export") {
        hasher.update((k.len() as u64).to_le_bytes());
        hasher.update(&k);
        hasher.update((v.len() as u64).to_le_bytes());
        hasher.update(&v);
    }
    hex::encode(hasher.finalize())
}

#[test]
fn an_unscheduled_attester_pay_v2_leaves_root_and_snapshot_byte_identical() {
    let state = run();
    let root = root_digest(&state);
    let snapshot = snapshot_digest(&state);
    // Deterministic: a second run gives the same bytes.
    let again = run();
    assert_eq!(root_digest(&again), root);
    assert_eq!(snapshot_digest(&again), snapshot);
    println!("root_material_sha3={root} snapshot_sha3={snapshot}");
    assert_eq!(root, ROOT_MATERIAL_SHA3, "root material differs from origin/main");
    assert_eq!(snapshot, SNAPSHOT_SHA3, "snapshot export differs from origin/main");
    // ---- this binary only (efc9cc5 has none of the items below) ----
    for height in [0, 100, 102, u64::MAX] {
        assert!(!feature_active(ConsensusFeature::AttesterPayV2, height), "never scheduled here");
    }
    assert!(!FeatureSchedule::default().active(ConsensusFeature::AttesterPayV2, u64::MAX));
    assert!(state.attester_payout_bonds.is_empty(), "nothing is written while inactive");
    let material = state.state_root_material();
    assert!(
        !material.windows(ATTESTER_PAYOUT_BONDS_ROOT_TAG.len()).any(|w| w == ATTESTER_PAYOUT_BONDS_ROOT_TAG),
        "no payout-bond section in the root"
    );
    // The shared registration step emits exactly the transition `run()`
    // applied, at every height, for every `payout_bond` shape.
    let set: Vec<String> = (1u8..=4).map(identity).collect();
    for shape in 0..3u8 {
        let (tx, by_hand) = registration_with_payout_bond(&set, 2, 9, shape);
        for height in [0, 102, u64::MAX] {
            let step = tx.oracle_registration_step(height).expect("authorized");
            assert_eq!(format!("{step:?}"), format!("{by_hand:?}"), "shape {shape} at {height}");
            assert_eq!(tx.parse_oracle_registration_at(height), tx.parse_oracle_registration());
            assert_eq!(tx.parse_oracle_registration_at(height).unwrap().payout_bond, None);
            assert!(tx.verify_oracle_registration_authorization_at(height));
        }
    }
    // The rejected decision is refused by the settlement derivation.
    let refused = state.derive_attester_settlements(&["d-reject".to_string()], 102, 1_000).unwrap_err();
    assert!(refused.to_string().contains("no oracle quorum"), "{refused}");
}
