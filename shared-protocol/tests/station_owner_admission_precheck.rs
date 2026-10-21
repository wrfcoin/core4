//! #3345 / #3347 admission: `State::precheck_station_owner_tx` (the node
//! submit route's check) must agree with what the consensus extractor plus the
//! real stateful step would actually do, for every owner-tx kind and every
//! refusal reason. A tx the precheck admits must take effect when mined; a tx
//! it refuses must be an inert no-op (and carry the documented stable code).
//!
//! One test: the feature schedule is process-wide (first-install-wins
//! `OnceLock`), so this file owns its process.
//!
//! Run from core4:
//!   cargo test -p wrfcoin-shared-protocol --test station_owner_admission_precheck

use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::blockchain::station_owner::{
    STATION_OWNER_RECOVERY_NOTICE_BLOCKS, StationOwnerActionPayload,
    StationOwnerRecoveryCancelPayload, StationOwnerRecoveryExecutePayload,
    StationOwnerRecoveryFilingPayload, StationOwnerTransferPayload, StationOwnerTxRejection,
};
use wrfcoin_shared_protocol::blockchain::transaction::{
    StationOwnerActionKind, StationRegistrationPayload, Transaction, TransactionInput,
    TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion,
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

const GOVERNANCE_ENV: &str = "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS";
const H: u64 = 2;

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).expect("32-byte ed25519 seed")
}
fn addr_of(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_address()
}
fn pk_of(k: &PrivateKey) -> String {
    PublicKey::from_private_key(k).to_hex()
}
fn chain() -> (String, String) {
    let id = ChainIdentity::active();
    (id.chain_id.clone(), id.network_id.clone())
}

fn tx_of(tag: &str, tx_type: TransactionType, data: String) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("input-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 0,
            address: format!("wrf-{tag}"),
            script: String::new(),
            nft_data: None,
        }],
    );
    tx.tx_type = tx_type;
    tx.data = Some(data);
    tx.set_id();
    tx
}

fn state_with_station(station: &str, owner: &PrivateKey) -> State {
    let mut payload = StationRegistrationPayload::new(
        station,
        addr_of(owner),
        46.7867,
        -92.1005,
        "test-real-station",
        DataQualityTier::Gold,
        ContributionProvenanceClass::Real,
    );
    payload.sign_ownership(owner);
    payload.sign(&key(0x60));
    let mut state = State::new();
    state
        .apply_station_registration(Transaction::station_from_registration(&payload))
        .unwrap();
    state
}

/// What a mined copy would actually do: extract through the shared constructor
/// and apply to a clone; report whether the owner or the ledger changed.
fn takes_effect(state: &State, tx: &Transaction, height: u64, station: &str) -> bool {
    let Some(transition) = tx.station_owner_ledger_transition(height) else {
        return false;
    };
    let mut clone = state.clone();
    let owner_before = clone
        .station_registry
        .get(station)
        .map(|s| s.owner_address.clone());
    let ledger_before = clone.station_owner_ledger.get(station).cloned();
    if clone.apply_transition(transition).is_err() {
        return false;
    }
    owner_before
        != clone
            .station_registry
            .get(station)
            .map(|s| s.owner_address.clone())
        || ledger_before != clone.station_owner_ledger.get(station).cloned()
}

/// Precheck vs reality, plus the expected stable code (`None` = admitted).
fn check(state: &State, label: &str, data: String, height: u64, station: &str, want: Option<&str>) {
    let (c, n) = chain();
    let tx = tx_of(label, TransactionType::StationRegistration, data);
    let got = state.precheck_station_owner_tx(&tx, height, &c, &n);
    let effect = takes_effect(state, &tx, height, station);
    match (&got, want) {
        (Ok(_), None) => assert!(effect, "[{label}] admitted but a mined copy is inert"),
        (Err(r), Some(code)) => {
            assert_eq!(r.code(), code, "[{label}] wrong refusal code ({r:?})");
            assert!(!effect, "[{label}] refused but a mined copy takes effect");
        },
        (got, want) => panic!("[{label}] precheck {got:?} but expected {want:?}"),
    }
}

fn transfer(
    s: &str,
    from: &PrivateKey,
    to: &PrivateKey,
    nonce: u64,
    exp: u64,
    counter: bool,
) -> String {
    let (c, n) = chain();
    let mut p = StationOwnerTransferPayload::new(
        s,
        addr_of(from),
        pk_of(from),
        addr_of(to),
        pk_of(to),
        nonce,
        exp,
    );
    p.sign_as_owner_with_domain(&c, &n, from);
    if counter {
        p.countersign_with_domain(&c, &n, to);
    }
    serde_json::to_string(&p).unwrap()
}
fn action(s: &str, o: &PrivateKey, k: StationOwnerActionKind, nonce: u64, exp: u64) -> String {
    let (c, n) = chain();
    let mut p = StationOwnerActionPayload::new(s, addr_of(o), pk_of(o), k, nonce, exp);
    p.sign_with_domain(&c, &n, o);
    serde_json::to_string(&p).unwrap()
}
fn filing(
    s: &str,
    from: &PrivateKey,
    to: &PrivateKey,
    nonce: u64,
    gov: &PrivateKey,
    accept: bool,
) -> String {
    let (c, n) = chain();
    let mut p =
        StationOwnerRecoveryFilingPayload::new(s, addr_of(from), addr_of(to), pk_of(to), nonce);
    p.sign_as_governance_with_domain(&c, &n, gov);
    if accept {
        p.accept_with_domain(&c, &n, to);
    }
    serde_json::to_string(&p).unwrap()
}
fn cancel(s: &str, o: &PrivateKey, nonce: u64) -> String {
    let (c, n) = chain();
    let mut p = StationOwnerRecoveryCancelPayload::new(s, addr_of(o), pk_of(o), nonce);
    p.sign_with_domain(&c, &n, o);
    serde_json::to_string(&p).unwrap()
}
fn execute(s: &str, nonce: u64) -> String {
    serde_json::to_string(&StationOwnerRecoveryExecutePayload::new(s, nonce)).unwrap()
}

/// Apply a tx for real (extract + apply) to advance a scenario's state.
fn mine(state: &mut State, tag: &str, data: String, height: u64) {
    let tx = tx_of(tag, TransactionType::StationRegistration, data);
    let t = tx
        .station_owner_ledger_transition(height)
        .unwrap_or_else(|| panic!("[{tag}] setup step must extract"));
    state.apply_transition(t).unwrap();
}

#[tokio::test]
async fn admission_precheck_agrees_with_what_a_mined_tx_does() {
    std::env::set_var(GOVERNANCE_ENV, pk_of(&key(0x60)));
    let raw = format!(
        "0:v1,{H}:{},{H}:{}",
        ConsensusFeature::StationOwnerConsent.name(),
        ConsensusFeature::StationOwnerActions.name()
    );
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);

    let (a, b, mallory, gov, bad_gov) = (key(0xA1), key(0xB2), key(0xE5), key(0x60), key(0x61));
    let s = "ws-admission";
    let h = 1_000u64;
    let exp = h + 500;
    let fresh = || state_with_station(s, &a);

    // Not an owner tx, and the feature gate.
    let (c, n) = chain();
    let state = fresh();
    let plain = tx_of("plain", TransactionType::StationRegistration, "{\"x\":1}".into());
    assert_eq!(
        state.precheck_station_owner_tx(&plain, h, &c, &n),
        Err(StationOwnerTxRejection::NotOwnerTx)
    );
    check(
        &state,
        "below-activation",
        transfer(s, &a, &b, 0, 1, true),
        H - 1,
        s,
        Some("OWNER_TX_FEATURE_NOT_ACTIVE"),
    );

    // Transfer.
    check(&state, "transfer-ok", transfer(s, &a, &b, 0, exp, true), h, s, None);
    check(&state, "transfer-nonce", transfer(s, &a, &b, 1, exp, true), h, s, Some("OWNER_TX_NONCE_MISMATCH"));
    check(&state, "transfer-nocounter", transfer(s, &a, &b, 0, exp, false), h, s, Some("OWNER_TX_BAD_COUNTERSIGNATURE"));
    check(&state, "transfer-expired", transfer(s, &a, &b, 0, h - 1, true), h, s, Some("OWNER_TX_EXPIRED"));
    check(&state, "transfer-toolong", transfer(s, &a, &b, 0, h + 10_001, true), h, s, Some("OWNER_TX_VALIDITY_TOO_LONG"));
    check(&state, "transfer-wrong-owner", transfer(s, &mallory, &b, 0, exp, true), h, s, Some("OWNER_TX_NOT_REGISTERED_OWNER"));
    check(&state, "transfer-same", transfer(s, &a, &a, 0, exp, true), h, s, Some("OWNER_TX_SAME_OWNER"));
    check(&state, "transfer-unknown", transfer("nope", &a, &b, 0, exp, true), h, "nope", Some("OWNER_TX_UNKNOWN_STATION"));

    // Actions.
    use StationOwnerActionKind as K;
    check(&state, "stolen-ok", action(s, &a, K::ReportStolen, 0, exp), h, s, None);
    check(&state, "unfreeze-notfrozen", action(s, &a, K::Unfreeze, 0, exp), h, s, Some("OWNER_TX_NOT_FROZEN"));
    check(&state, "deactivate-ok", action(s, &a, K::Deactivate, 0, exp), h, s, None);
    check(&state, "reactivate-not-deactivated", action(s, &a, K::Reactivate, 0, exp), h, s, Some("OWNER_TX_NOT_DEACTIVATED"));
    check(&state, "action-wrong-owner", action(s, &mallory, K::ReportStolen, 0, exp), h, s, Some("OWNER_TX_NOT_REGISTERED_OWNER"));

    let mut frozen = fresh();
    mine(&mut frozen, "freeze", action(s, &a, K::ReportStolen, 0, exp), h);
    check(&frozen, "transfer-frozen", transfer(s, &a, &b, 1, exp, true), h, s, Some("OWNER_TX_STATION_FROZEN"));
    check(&frozen, "stolen-twice", action(s, &a, K::ReportStolen, 1, exp), h, s, Some("OWNER_TX_STATION_FROZEN"));
    check(&frozen, "unfreeze-ok", action(s, &a, K::Unfreeze, 1, exp), h, s, None);
    // A frozen station can neither self-deactivate nor reactivate: the real
    // step refuses both, and the precheck says so with the same code.
    check(&frozen, "deactivate-frozen", action(s, &a, K::Deactivate, 1, exp), h, s, Some("OWNER_TX_STATION_FROZEN"));
    check(&frozen, "reactivate-frozen", action(s, &a, K::Reactivate, 1, exp), h, s, Some("OWNER_TX_STATION_FROZEN"));
    let mut frozen_and_deactivated = fresh();
    mine(&mut frozen_and_deactivated, "deact-first", action(s, &a, K::Deactivate, 0, exp), h);
    mine(&mut frozen_and_deactivated, "then-freeze", action(s, &a, K::ReportStolen, 1, exp), h);
    check(&frozen_and_deactivated, "reactivate-frozen-and-deactivated", action(s, &a, K::Reactivate, 2, exp), h, s, Some("OWNER_TX_STATION_FROZEN"));
    check(&frozen_and_deactivated, "deactivate-frozen-and-deactivated", action(s, &a, K::Deactivate, 2, exp), h, s, Some("OWNER_TX_STATION_FROZEN"));

    let mut deact = fresh();
    mine(&mut deact, "deact", action(s, &a, K::Deactivate, 0, exp), h);
    check(&deact, "deactivate-twice", action(s, &a, K::Deactivate, 1, exp), h, s, Some("OWNER_TX_ALREADY_DEACTIVATED"));
    check(&deact, "reactivate-ok", action(s, &a, K::Reactivate, 1, exp), h, s, None);

    // Recovery: file, cancel, execute.
    check(&state, "file-ok", filing(s, &a, &b, 0, &gov, true), h, s, None);
    check(&state, "file-noaccept", filing(s, &a, &b, 0, &gov, false), h, s, Some("OWNER_TX_BAD_COUNTERSIGNATURE"));
    check(&state, "file-nongov", filing(s, &a, &b, 0, &bad_gov, true), h, s, Some("OWNER_TX_NOT_GOVERNANCE"));
    check(&state, "file-nonce", filing(s, &a, &b, 3, &gov, true), h, s, Some("OWNER_TX_NONCE_MISMATCH"));
    check(&state, "cancel-nothing", cancel(s, &a, 0), h, s, Some("OWNER_TX_NO_PENDING_RECOVERY"));
    check(&state, "execute-nothing", execute(s, 0), h, s, Some("OWNER_TX_NO_PENDING_RECOVERY"));

    let mut pending = fresh();
    mine(&mut pending, "file", filing(s, &a, &b, 0, &gov, true), h);
    let ready = h + STATION_OWNER_RECOVERY_NOTICE_BLOCKS;
    check(&pending, "file-twice", filing(s, &a, &b, 0, &gov, true), h + 1, s, Some("OWNER_TX_RECOVERY_ALREADY_PENDING"));
    check(&pending, "cancel-ok", cancel(s, &a, 0), h + 1, s, None);
    check(&pending, "cancel-late", cancel(s, &a, 0), ready, s, Some("OWNER_TX_NOTICE_WINDOW_ELAPSED"));
    check(&pending, "cancel-wrong-owner", cancel(s, &mallory, 0), h + 1, s, Some("OWNER_TX_NOT_REGISTERED_OWNER"));
    // A cancel signed for a stale or future recovery nonce is a mismatch, and so is
    // a cancel for the right recovery after the owner moved the nonce on.
    check(&pending, "cancel-stale-nonce", cancel(s, &a, 1), h + 1, s, Some("OWNER_TX_NONCE_MISMATCH"));
    check(&pending, "cancel-future-nonce", cancel(s, &a, 9), h + 1, s, Some("OWNER_TX_NONCE_MISMATCH"));
    check(&pending, "execute-early", execute(s, 0), ready - 1, s, Some("OWNER_TX_NOTICE_WINDOW_NOT_ELAPSED"));
    check(&pending, "execute-ok", execute(s, 0), ready, s, None);
    check(&pending, "execute-wrong-nonce", execute(s, 5), ready, s, Some("OWNER_TX_NONCE_MISMATCH"));

    // Cancel at a stale nonce after the ledger moved on (the owner acted first).
    let mut moved_on = pending.clone();
    mine(&mut moved_on, "owner-acts", action(s, &a, K::Deactivate, 0, h + 600), h + 2);
    check(&moved_on, "cancel-after-owner-acted", cancel(s, &a, 0), h + 3, s, Some("OWNER_TX_NO_PENDING_RECOVERY"));
    // A live owner acting voids the pending recovery (nonce moved on).
    let mut voided = pending.clone();
    mine(&mut voided, "act", action(s, &a, K::Deactivate, 0, h + 600), h + 2);
    check(&voided, "execute-voided", execute(s, 0), ready, s, Some("OWNER_TX_NO_PENDING_RECOVERY"));

    // Read side.
    let view = pending.station_owner_ledger_view(s);
    assert_eq!(view.owner_nonce, 0);
    assert!(view.frozen.is_none() && !view.owner_deactivated);
    let p = view.pending_recovery.expect("pending recovery is reported");
    assert_eq!((p.recovery_nonce, p.filed_at_height), (0, h));
    assert_eq!(p.executable_at_height, ready);
    let empty = fresh().station_owner_ledger_view(s);
    assert_eq!(empty.owner_nonce, 0);
    assert!(empty.pending_recovery.is_none());
}
