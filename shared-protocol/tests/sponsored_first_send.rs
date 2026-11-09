//! Sponsored first sends: a sponsor pays a transfer's fee under
//! `ConsensusFeature::Paymaster`, scheduled here at H = 5.
//!
//! - Unscheduled, and below H, a transfer carrying a sponsorship applies
//!   exactly as the same transfer without it: same balances, same state root,
//!   `paymasters` stays empty (byte-absent from the root).
//! - From H, a valid sponsorship debits the fee from the sponsor, burns it,
//!   and records the sponsorship; every refusal leaves state untouched.
//!
//! ONE test: the feature schedule is installed process-wide, so this file
//! owns its process (same discipline as `passkey_activation_2169`).

use wrfcoin_shared_protocol::accounts::paymaster::{
    MAX_SPONSORSHIP_VALIDITY_BLOCKS, SponsorAuthorization, sponsorship_active,
    sponsorship_scheduled,
};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType, TransparentTransferPayload,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion,
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule, try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

use rand::{SeedableRng, rngs::StdRng};

const H: u64 = 5;
const AMOUNT: u64 = 1_000;
const FEE: u64 = 5_000;
const USER_FUNDS: u128 = 10_000;
const SPONSOR_FUNDS: u128 = 1_000_000;

struct Keys {
    user: PrivateKey,
    user_addr: Address,
    sponsor: PrivateKey,
    sponsor_addr: Address,
    to: Address,
}

fn keys() -> Keys {
    let mut rng = StdRng::seed_from_u64(1101);
    let user = PrivateKey::generate(&mut rng);
    let sponsor = PrivateKey::generate(&mut rng);
    Keys {
        user_addr: Address::from_public_key(&PublicKey::from_private_key(&user)),
        sponsor_addr: Address::from_public_key(&PublicKey::from_private_key(&sponsor)),
        user,
        sponsor,
        to: Address([0x66; 20]),
    }
}

fn sponsorship(k: &Keys, nonce: u64, expires: u64, floor: u64, first_send_only: bool) -> SponsorAuthorization {
    let mut auth = SponsorAuthorization {
        sponsor: k.sponsor_addr.to_hex(),
        fee: FEE,
        expires_at_height: expires,
        balance_floor: floor,
        first_send_only,
        sponsor_public_key: String::new(),
        signature: String::new(),
    };
    auth.sign(&k.sponsor, &k.user_addr.to_hex(), &k.to.to_hex(), AMOUNT, nonce);
    auth
}

fn transfer(k: &Keys, nonce: u64, sponsorship: Option<SponsorAuthorization>, tag: &str) -> Transaction {
    let mut payload = TransparentTransferPayload {
        from: k.user_addr.to_hex(),
        to: k.to.to_hex(),
        amount: AMOUNT,
        transparent_opt_out: true,
        opt_out_audit_tag: None,
        auth_pubkey: None,
        auth_signature: None,
        authority: None,
        authority_signature: None,
        passkey_auth: None,
        sponsorship,
        session_key_auth: None,
    };
    payload.sign_authorization(&k.user, nonce);
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(tag),
            vout: 0,
            signature: None,
            public_key: Some(payload.from.clone()),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 0,
            address: payload.from.clone(),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = TransactionType::Transfer;
    tx.sender_address = k.user_addr;
    tx.nonce = nonce;
    tx.fee = 0;
    tx.data = Some(serde_json::to_string(&payload).unwrap());
    tx.set_id();
    tx
}

fn fresh_state(k: &Keys) -> State {
    let mut state = State::new();
    state.create_account(k.user_addr, Balance::new(USER_FUNDS));
    state.create_account(k.sponsor_addr, Balance::new(SPONSOR_FUNDS));
    state.recalculate_state_root();
    state
}

fn apply_at(state: &mut State, tx: &Transaction, height: u64) -> Result<(), String> {
    let result = state
        .apply_transition(StateTransition::ProcessGovernanceTransaction {
            tx: tx.clone(),
            current_block: height,
        })
        .map_err(|e| e.to_string());
    state.recalculate_state_root();
    result
}

fn bal(state: &State, who: &Address) -> u128 {
    state.get_account(who).map(|a| a.balance.0).unwrap_or(0)
}

/// Apply `tx` at `height` to a copy of `state` and require a refusal that
/// leaves the state root, balances and sponsor records unchanged.
fn assert_refused_untouched(state: &State, tx: &Transaction, height: u64, needle: &str) {
    let mut copy = state.clone();
    let err = apply_at(&mut copy, tx, height).expect_err("must be refused");
    assert!(err.contains(needle), "expected {needle:?} in {err:?}");
    assert_eq!(copy.state_root, state.state_root, "a refusal moves no root");
    assert_eq!(copy.paymasters, state.paymasters);
    assert_eq!(copy.supply_authorized_burning, state.supply_authorized_burning);
}

#[test]
fn sponsored_first_send_is_inert_until_paymaster_activates_then_charges_the_sponsor() {
    let k = keys();
    let plain = transfer(&k, 0, None, "plain");
    let sponsored = transfer(&k, 0, Some(sponsorship(&k, 0, H + 10, 0, true)), "sponsored");
    let mut bad_sig_auth = sponsorship(&k, 0, H + 10, 0, true);
    bad_sig_auth.fee += 1; // no longer what the sponsor signed
    let bad_sig = transfer(&k, 0, Some(bad_sig_auth), "bad-sig");

    // ── Phase 1: unscheduled — the sponsorship is ignored everywhere ────────
    assert!(try_feature_schedule().is_none());
    assert!(!sponsorship_scheduled());
    let mut mempool = Mempool::new(&Config::default());
    mempool.add_transaction(bad_sig.clone()).expect("unscheduled: field ignored at admission");

    let mut reference = fresh_state(&k);
    apply_at(&mut reference, &plain, 100).expect("plain transfer");
    for tx in [&sponsored, &bad_sig] {
        let mut state = fresh_state(&k);
        apply_at(&mut state, tx, 100).expect("applies as a plain transfer");
        assert_eq!(state.state_root, reference.state_root, "byte-identical to unsponsored");
        assert!(state.paymasters.is_empty(), "state stays byte-absent");
        assert_eq!(bal(&state, &k.sponsor_addr), SPONSOR_FUNDS);
        assert_eq!(state.supply_authorized_burning, 0);
    }

    // ── Phase 2: schedule paymaster at H ────────────────────────────────────
    let raw = format!("0:v1,{H}:{}", ConsensusFeature::Paymaster.name());
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    assert!(sponsorship_scheduled());
    assert!(!sponsorship_active(H - 1));
    assert!(sponsorship_active(H));

    // Admission now screens the sponsor signature.
    let mut mempool = Mempool::new(&Config::default());
    assert!(mempool.add_transaction(bad_sig.clone()).is_err(), "scheduled: bad sponsor signature refused");
    mempool.add_transaction(sponsored.clone()).expect("scheduled: valid sponsorship admitted");

    // Below H: still ignored, byte-identical to the plain transfer.
    let mut below = fresh_state(&k);
    let mut below_ref = fresh_state(&k);
    apply_at(&mut below, &transfer(&k, 0, Some(sponsorship(&k, 0, H + 10, 0, true)), "b"), H - 1).unwrap();
    apply_at(&mut below_ref, &plain, H - 1).unwrap();
    assert_eq!(below.state_root, below_ref.state_root);
    assert!(below.paymasters.is_empty());

    // At H: the sponsor pays the fee, the fee is burned, the user pays only
    // the amount, and the sponsorship is recorded.
    let mut state = fresh_state(&k);
    // Refusals first, each leaving state untouched.
    assert_refused_untouched(&state, &bad_sig, H, "SP_SPONSORED_SEND_REFUSED");
    let expired = transfer(&k, 0, Some(sponsorship(&k, 0, H, 0, true)), "expired");
    assert_refused_untouched(&state, &expired, H + 1, "expired");
    let too_long = transfer(
        &k,
        0,
        Some(sponsorship(&k, 0, H + MAX_SPONSORSHIP_VALIDITY_BLOCKS + 1, 0, true)),
        "too-long",
    );
    assert_refused_untouched(&state, &too_long, H, "valid for more than");
    let floor = transfer(
        &k,
        0,
        Some(sponsorship(&k, 0, H + 10, (SPONSOR_FUNDS - FEE as u128 + 1) as u64, true)),
        "floor",
    );
    assert_refused_untouched(&state, &floor, H, "keep its floor");
    // The user cannot cover the amount: the sponsor is not charged either.
    let mut poor = state.clone();
    poor.get_account_mut(&k.user_addr).unwrap().balance = Balance::new(0);
    poor.get_account_mut(&k.user_addr).unwrap().spendable_balance = Balance::new(0);
    poor.recalculate_state_root();
    assert_refused_untouched(&poor, &sponsored, H, "");
    assert_eq!(bal(&poor, &k.sponsor_addr), SPONSOR_FUNDS);

    apply_at(&mut state, &sponsored, H).expect("sponsored first send applies at H");
    assert_eq!(bal(&state, &k.user_addr), USER_FUNDS - AMOUNT as u128);
    assert_eq!(bal(&state, &k.to), AMOUNT as u128);
    assert_eq!(bal(&state, &k.sponsor_addr), SPONSOR_FUNDS - FEE as u128);
    assert_eq!(state.supply_authorized_burning, FEE as u128);
    let record = &state.paymasters[&k.sponsor_addr];
    assert_eq!(record.deposit, 0);
    assert_eq!(record.sponsorships_this_epoch, 1);
    assert_eq!(record.per_sender[&k.user_addr].count, 1);
    let mut unsponsored = fresh_state(&k);
    apply_at(&mut unsponsored, &plain, H).unwrap();
    assert_ne!(state.state_root, unsponsored.state_root, "the sponsor record is in the root");

    // Replay of the same transfer: refused by the account nonce.
    assert_refused_untouched(&state, &sponsored, H + 1, "");

    // First-send-only: the user's second send (nonce 1) is not paid for.
    let second = transfer(&k, 1, Some(sponsorship(&k, 1, H + 10, 0, true)), "second");
    assert_refused_untouched(&state, &second, H + 1, "first send");

    // A sponsor that did not ask for first-send-only may pay a later send.
    let later = transfer(&k, 1, Some(sponsorship(&k, 1, H + 10, 0, false)), "later");
    apply_at(&mut state, &later, H + 1).expect("any-send sponsorship");
    assert_eq!(bal(&state, &k.sponsor_addr), SPONSOR_FUNDS - 2 * FEE as u128);
    assert_eq!(state.paymasters[&k.sponsor_addr].per_sender[&k.user_addr].count, 2);
}
