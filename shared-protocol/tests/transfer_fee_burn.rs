//! Burned transfer fee under `ConsensusFeature::TransferFeeBurn`, scheduled
//! here at H together with `paymaster` (the release shape the owner signs).
//!
//! - Unscheduled, and below H, a transfer's declared `tx.fee` is never read:
//!   any declared fee gives the same balances and the same state root as a
//!   zero fee, and nothing is burned. Admission is unchanged.
//! - From H, a transparent transfer must declare `tx.fee >=` the fee; the
//!   PROTOCOL fee (not the declared one) is debited from the sender and burned
//!   (`supply_authorized_burning`, INV-SUPPLY-1 holds, circulating supply drops
//!   by exactly the fee). With a valid sponsorship the sponsor's signed fee is
//!   burned instead and the sender pays only the amount. Every refusal leaves
//!   state untouched.
//! - Reward, receipt and other non-transfer paths are not charged.
//!
//! ONE test: the feature schedule is installed process-wide, so this file
//! owns its process (same discipline as `sponsored_first_send`).

use wrfcoin_shared_protocol::accounts::paymaster::{SponsorAuthorization, sponsorship_active};
use wrfcoin_shared_protocol::accounts::session_keys::{
    MethodId, SessionKeyAction, SessionKeyGrant, SessionScope, session_key_register_message,
};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType, TransparentTransferPayload,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::transfer_fee::{
    TRANSFER_FEE_V1_BASE_UNITS, transfer_fee_at, transfer_fee_scheduled, transfer_fee_to_declare,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion,
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule, try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

use proptest::prelude::*;
use rand::{SeedableRng, rngs::StdRng};

const H: u64 = 7;
const FEE: u64 = TRANSFER_FEE_V1_BASE_UNITS;
const AMOUNT: u64 = 1_000;
const USER_FUNDS: u128 = 100_000;
const SPONSOR_FUNDS: u128 = 1_000_000;
const SPONSOR_FEE: u64 = 7_000;

struct Keys {
    user: PrivateKey,
    user_addr: Address,
    sponsor: PrivateKey,
    sponsor_addr: Address,
    to: Address,
}

fn keys() -> Keys {
    let mut rng = StdRng::seed_from_u64(1101_5000);
    let user = PrivateKey::generate(&mut rng);
    let sponsor = PrivateKey::generate(&mut rng);
    Keys {
        user_addr: Address::from_public_key(&PublicKey::from_private_key(&user)),
        sponsor_addr: Address::from_public_key(&PublicKey::from_private_key(&sponsor)),
        user,
        sponsor,
        to: Address([0x77; 20]),
    }
}

fn sponsorship(k: &Keys, nonce: u64, amount: u64, fee: u64) -> SponsorAuthorization {
    let mut auth = SponsorAuthorization {
        sponsor: k.sponsor_addr.to_hex(),
        fee,
        expires_at_height: H + 20,
        balance_floor: 0,
        first_send_only: false,
        sponsor_public_key: String::new(),
        signature: String::new(),
    };
    auth.sign(
        &k.sponsor,
        &k.user_addr.to_hex(),
        &k.to.to_hex(),
        amount,
        nonce,
    );
    auth
}

fn transfer_of(
    k: &Keys,
    nonce: u64,
    amount: u64,
    declared_fee: u64,
    sponsorship: Option<SponsorAuthorization>,
    tag: &str,
) -> Transaction {
    let mut payload = TransparentTransferPayload {
        from: k.user_addr.to_hex(),
        to: k.to.to_hex(),
        amount,
        transparent_opt_out: true,
        opt_out_audit_tag: None,
        auth_pubkey: None,
        auth_signature: None,
        authority: None,
        authority_signature: None,
        passkey_auth: None,
        session_key_auth: None,
        sponsorship,
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
    tx.fee = declared_fee;
    tx.data = Some(serde_json::to_string(&payload).unwrap());
    tx.set_id();
    tx
}

fn transfer(k: &Keys, nonce: u64, declared_fee: u64, tag: &str) -> Transaction {
    transfer_of(k, nonce, AMOUNT, declared_fee, None, tag)
}

fn fresh_state(k: &Keys, user_funds: u128) -> State {
    let mut state = State::new();
    state.create_account(k.user_addr, Balance::new(user_funds));
    state.create_account(k.sponsor_addr, Balance::new(SPONSOR_FUNDS));
    state.initialize_supply_baseline().unwrap();
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

fn spendable(state: &State, who: &Address) -> u128 {
    state
        .get_account(who)
        .map(|a| a.spendable_balance.0)
        .unwrap_or(0)
}

fn assert_refused_untouched(state: &State, tx: &Transaction, height: u64, needle: &str) {
    let mut copy = state.clone();
    let err = apply_at(&mut copy, tx, height).expect_err("must be refused");
    assert!(err.contains(needle), "expected {needle:?} in {err:?}");
    assert_eq!(copy.state_root, state.state_root, "a refusal moves no root");
    assert_eq!(
        bal(&copy, &tx.sender_address),
        bal(state, &tx.sender_address)
    );
    assert_eq!(
        copy.supply_authorized_burning,
        state.supply_authorized_burning
    );
    assert_eq!(copy.paymasters, state.paymasters);
}

/// Circulating supply identity: what is held now equals the baseline plus
/// mints minus authorized burns, and INV-SUPPLY-1 holds.
fn assert_supply_conserved(state: &mut State) {
    state
        .check_supply_conservation()
        .expect("INV-SUPPLY-1 holds after a fee burn");
    let held = state.supply_ledger().unwrap().held().unwrap();
    assert_eq!(
        held,
        state.supply_genesis + state.supply_authorized_minting - state.supply_authorized_burning
    );
}

#[test]
fn transfer_fee_is_inert_until_scheduled_then_burned_from_sender_or_sponsor() {
    let k = keys();

    // ── Phase 1: unscheduled — the declared fee is never read ───────────────
    assert!(try_feature_schedule().is_none());
    assert!(!transfer_fee_scheduled());
    assert_eq!(transfer_fee_at(u64::MAX), None);
    assert_eq!(transfer_fee_to_declare(1), 0, "envelopes unchanged");
    let mut reference = fresh_state(&k, USER_FUNDS);
    apply_at(&mut reference, &transfer(&k, 0, 0, "zero"), 100).expect("zero-fee transfer");
    for declared in [1, FEE - 1, FEE, 10 * FEE, u64::MAX] {
        let mut state = fresh_state(&k, USER_FUNDS);
        apply_at(&mut state, &transfer(&k, 0, declared, "declared"), 100).expect("fee ignored");
        assert_eq!(
            state.state_root, reference.state_root,
            "declared {declared}: byte-identical"
        );
        assert_eq!(state.supply_authorized_burning, 0);
        assert_eq!(bal(&state, &k.user_addr), USER_FUNDS - AMOUNT as u128);
    }
    // A sender with exactly the amount can still send everything.
    let mut exact = fresh_state(&k, AMOUNT as u128);
    apply_at(&mut exact, &transfer(&k, 0, 0, "exact"), 100).expect("no fee: whole balance");
    // Admission: a zero-fee transfer is admitted at any height.
    let mut mempool = Mempool::new(&Config::default());
    mempool
        .add_transaction_with_signing_keys_at_height(transfer(&k, 0, 0, "adm0"), None, Some(1_000))
        .expect("unscheduled: zero fee admitted");

    // ── Phase 2: schedule paymaster and transfer_fee_burn at the SAME H ────
    let raw = format!(
        "0:v1,{H}:{},{H}:{},{H}:{}",
        ConsensusFeature::Paymaster.name(),
        ConsensusFeature::SessionKeys.name(),
        ConsensusFeature::TransferFeeBurn.name()
    );
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    assert!(transfer_fee_scheduled());
    assert_eq!(
        transfer_fee_at(H - 1),
        None,
        "activation boundary: H-1 free"
    );
    assert_eq!(
        transfer_fee_at(H),
        Some(FEE),
        "activation boundary: H charged"
    );
    assert!(sponsorship_active(H));
    assert_eq!(transfer_fee_to_declare(H), FEE);
    assert_eq!(
        transfer_fee_to_declare(H - 3),
        FEE,
        "declared ahead of activation"
    );

    // Admission at tip+1.
    let mut mempool = Mempool::new(&Config::default());
    let under = transfer(&k, 0, FEE - 1, "under");
    let err = mempool
        .add_transaction_with_signing_keys_at_height(under.clone(), None, Some(H - 1))
        .expect_err("tip H-1 → next block H charges: under-fee refused");
    assert!(
        err.to_string().contains("SP_TRANSFER_FEE_BELOW_MINIMUM"),
        "{err}"
    );
    mempool
        .add_transaction_with_signing_keys_at_height(under.clone(), None, Some(H - 2))
        .expect("tip H-2 → next block H-1 is free: admitted");
    let mut mempool = Mempool::new(&Config::default());
    mempool
        .add_transaction_with_signing_keys_at_height(transfer(&k, 0, FEE, "ok"), None, Some(H - 1))
        .expect("declared == fee admitted");

    // Below H: still byte-identical, whatever is declared.
    let mut below = fresh_state(&k, USER_FUNDS);
    let mut below_ref = fresh_state(&k, USER_FUNDS);
    apply_at(&mut below, &transfer(&k, 0, 0, "b0"), H - 1).unwrap();
    apply_at(&mut below_ref, &transfer(&k, 0, FEE, "b1"), H - 1).unwrap();
    assert_eq!(below.state_root, below_ref.state_root);
    assert_eq!(below.supply_authorized_burning, 0);

    // At H, sender pays.
    let state0 = fresh_state(&k, USER_FUNDS);
    assert_refused_untouched(
        &state0,
        &transfer(&k, 0, 0, "z"),
        H,
        "SP_TRANSFER_FEE_BELOW_MINIMUM",
    );
    assert_refused_untouched(&state0, &under, H, "SP_TRANSFER_FEE_BELOW_MINIMUM");
    // Amount alone fits, amount + fee does not: refused, nothing moves.
    let tight = fresh_state(&k, AMOUNT as u128 + FEE as u128 - 1);
    assert_refused_untouched(&tight, &transfer(&k, 0, FEE, "tight"), H, "");
    let mut fits = fresh_state(&k, AMOUNT as u128 + FEE as u128);
    apply_at(&mut fits, &transfer(&k, 0, FEE, "fits"), H).expect("amount + fee exactly");
    assert_eq!(bal(&fits, &k.user_addr), 0);
    assert_eq!(spendable(&fits, &k.user_addr), 0);

    let mut state = fresh_state(&k, USER_FUNDS);
    let supply_before = state.issuance_committed_supply();
    // Over-declaring does not raise the burn: only the protocol fee is debited.
    apply_at(&mut state, &transfer(&k, 0, 100 * FEE, "over"), H).expect("fee-paying transfer");
    assert_eq!(
        bal(&state, &k.user_addr),
        USER_FUNDS - AMOUNT as u128 - FEE as u128
    );
    assert_eq!(
        spendable(&state, &k.user_addr),
        USER_FUNDS - AMOUNT as u128 - FEE as u128
    );
    assert_eq!(
        bal(&state, &k.to),
        AMOUNT as u128,
        "recipient gets exactly the amount"
    );
    assert_eq!(
        bal(&state, &k.sponsor_addr),
        SPONSOR_FUNDS,
        "nobody is credited the fee"
    );
    assert_eq!(state.supply_authorized_burning, FEE as u128);
    assert_eq!(
        state.issuance_committed_supply(),
        supply_before - FEE as u128,
        "burn frees headroom"
    );
    assert_eq!(state.get_account(&k.user_addr).unwrap().nonce, 1);
    assert_supply_conserved(&mut state);

    // Sponsor pays: the sender pays only the amount, the sponsor's signed fee
    // is burned, and the declared fee is still required.
    let sponsored = transfer_of(
        &k,
        1,
        AMOUNT,
        FEE,
        Some(sponsorship(&k, 1, AMOUNT, SPONSOR_FEE)),
        "sp",
    );
    let user_before = bal(&state, &k.user_addr);
    let burned_before = state.supply_authorized_burning;
    let low_sponsor = transfer_of(
        &k,
        1,
        AMOUNT,
        FEE,
        Some(sponsorship(&k, 1, AMOUNT, FEE - 1)),
        "sp-low",
    );
    assert_refused_untouched(
        &state,
        &low_sponsor,
        H + 1,
        "SP_TRANSFER_FEE_SPONSOR_BELOW_MINIMUM",
    );
    let undeclared = transfer_of(
        &k,
        1,
        AMOUNT,
        0,
        Some(sponsorship(&k, 1, AMOUNT, SPONSOR_FEE)),
        "sp-0",
    );
    assert_refused_untouched(&state, &undeclared, H + 1, "SP_TRANSFER_FEE_BELOW_MINIMUM");
    apply_at(&mut state, &sponsored, H + 1).expect("sponsored fee-paying transfer");
    assert_eq!(
        bal(&state, &k.user_addr),
        user_before - AMOUNT as u128,
        "sender pays no fee"
    );
    assert_eq!(
        bal(&state, &k.sponsor_addr),
        SPONSOR_FUNDS - SPONSOR_FEE as u128
    );
    assert_eq!(
        state.supply_authorized_burning,
        burned_before + SPONSOR_FEE as u128
    );
    assert_supply_conserved(&mut state);

    // A sponsored newcomer with exactly the amount can send all of it.
    let mut newcomer = fresh_state(&k, AMOUNT as u128);
    let first = transfer_of(
        &k,
        0,
        AMOUNT,
        FEE,
        Some(sponsorship(&k, 0, AMOUNT, FEE)),
        "first",
    );
    apply_at(&mut newcomer, &first, H).expect("no fee wall for a sponsored first send");
    assert_eq!(bal(&newcomer, &k.user_addr), 0);
    assert_eq!(newcomer.supply_authorized_burning, FEE as u128);

    // Session-key transfers (#97): the sender's balance pays the fee; the
    // key's limits count the AMOUNT only, never the burned fee.
    {
        let mut rng = StdRng::seed_from_u64(0x5e55_fee0);
        let owner_sk = PrivateKey::generate(&mut rng);
        let owner = Address::from_public_key(&PublicKey::from_private_key(&owner_sk));
        let session_sk = PrivateKey::generate(&mut rng);
        let session_key = Address::from_public_key(&PublicKey::from_private_key(&session_sk));
        let grant = SessionKeyGrant {
            public_key: PublicKey::from_private_key(&session_sk),
            scope: SessionScope {
                allowed_methods: vec![MethodId("transfer".into())],
                allowed_targets: vec![k.to],
                max_value_per_use: u128::from(AMOUNT),
                max_total_value: 2 * u128::from(AMOUNT),
                window: None,
            },
            valid_from: H,
            valid_until: H + 1_000,
            max_uses: None,
        };
        let chain_id = ChainIdentity::active().chain_id.clone();
        let message = session_key_register_message(&chain_id, &owner, 0, &grant);
        let register = SessionKeyAction::Register {
            owner,
            registration_nonce: 0,
            grant,
            owner_public_key: PublicKey::from_private_key(&owner_sk).to_hex(),
            owner_signature: owner_sk.sign(&message).to_hex(),
            session_signature: session_sk.sign(&message).to_hex(),
        };
        let session_send = |nonce: u64, tag: &str| {
            let mut payload = TransparentTransferPayload {
                from: owner.to_hex(),
                to: k.to.to_hex(),
                amount: AMOUNT,
                transparent_opt_out: true,
                opt_out_audit_tag: None,
                auth_pubkey: None,
                auth_signature: None,
                authority: None,
                authority_signature: None,
                passkey_auth: None,
                session_key_auth: None,
                sponsorship: None,
            };
            payload.sign_session_key_authorization(&session_sk, nonce);
            let mut tx = transfer(&k, nonce, FEE, tag);
            tx.sender_address = owner;
            tx.data = Some(serde_json::to_string(&payload).unwrap());
            tx.set_id();
            tx
        };
        let record = |s: &State| {
            let r = &s.session_keys[&owner].records[&session_key];
            (r.uses_so_far, r.cumulative_value_used)
        };
        let setup = |funds: u128| {
            let mut s = fresh_state(&k, USER_FUNDS);
            s.create_account(owner, Balance::new(funds));
            s.supply_baseline_initialized = false;
            s.initialize_supply_baseline().unwrap();
            s.apply_transition(StateTransition::ApplySessionKeyAction {
                action: register.clone(),
                block_height: H,
                security_key_auth: None,
            })
            .expect("register the session key at H");
            s.recalculate_state_root();
            s
        };

        // Amount fits, amount + fee does not: refused, the key spends nothing.
        let short = setup(u128::from(AMOUNT) + u128::from(FEE) - 1);
        assert_eq!(record(&short), (0, 0));
        let mut copy = short.clone();
        apply_at(&mut copy, &session_send(0, "sk-short"), H + 1).expect_err("cannot cover the fee");
        assert_eq!(
            record(&copy),
            (0, 0),
            "a refused send spends no use or allowance"
        );
        assert_eq!(bal(&copy, &owner), bal(&short, &owner));
        assert_eq!(
            copy.supply_authorized_burning,
            short.supply_authorized_burning
        );

        // Under-declared fee: refused before the key is charged.
        let mut copy = short.clone();
        let mut under = session_send(0, "sk-under");
        under.fee = 0;
        under.set_id();
        let err = apply_at(&mut copy, &under, H + 1).expect_err("under-fee");
        assert!(err.contains("SP_TRANSFER_FEE_BELOW_MINIMUM"), "{err}");
        assert_eq!(record(&copy), (0, 0));

        // Two sends exactly exhaust the 2*AMOUNT lifetime limit even though
        // the owner also paid two fees: the limit counts the amount only.
        let funds = 100 * u128::from(AMOUNT);
        let mut s = setup(funds);
        let burned = s.supply_authorized_burning;
        apply_at(&mut s, &session_send(0, "sk-1"), H + 1).expect("session send 1");
        apply_at(&mut s, &session_send(1, "sk-2"), H + 2).expect("session send 2");
        assert_eq!(
            record(&s),
            (2, 2 * u128::from(AMOUNT)),
            "limits count amount only"
        );
        assert_eq!(
            bal(&s, &owner),
            funds - 2 * u128::from(AMOUNT) - 2 * u128::from(FEE)
        );
        assert_eq!(s.supply_authorized_burning, burned + 2 * u128::from(FEE));
        assert_supply_conserved(&mut s);
        let err =
            apply_at(&mut s.clone(), &session_send(2, "sk-3"), H + 3).expect_err("lifetime spent");
        assert!(err.contains("SP_SESSION_KEY_TRANSFER_REFUSED"), "{err}");
    }

    // Rewards and internal balance moves are not charged: the fee lives only
    // in the transparent-transfer arm.
    let mut rewarded = fresh_state(&k, USER_FUNDS);
    let burned = rewarded.supply_authorized_burning;
    let earned_before = rewarded
        .get_account(&k.user_addr)
        .unwrap()
        .earned_balance_total
        .0;
    rewarded
        .apply_transition(StateTransition::ApplyReward {
            address: k.user_addr,
            amount: Balance::new(70),
            reason: "contribution".to_string(),
        })
        .expect("a reward at an active height");
    assert_eq!(
        rewarded
            .get_account(&k.user_addr)
            .unwrap()
            .earned_balance_total
            .0,
        earned_before + 70
    );
    rewarded
        .apply_transition(StateTransition::BalanceTransfer {
            from: k.sponsor_addr,
            to: k.user_addr,
            amount: Balance::new(10),
            nonce: Some(0),
            current_block: H + 1,
        })
        .expect("internal balance transfers carry no fee");
    assert_eq!(
        rewarded.supply_authorized_burning, burned,
        "no reward path burns"
    );
    assert_eq!(bal(&rewarded, &k.sponsor_addr), SPONSOR_FUNDS - 10);

    // Property: across random amounts/declared fees above the minimum, the
    // sender loses exactly amount + FEE, the recipient gains amount, and the
    // supply identity holds.
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    });
    runner
        .run(
            &(1u64..50_000, FEE..FEE * 1_000, 0u128..10_000),
            |(amount, declared, extra)| {
                let funds = u128::from(amount) + u128::from(FEE) + extra;
                let mut s = fresh_state(&k, funds);
                let before = s.issuance_committed_supply();
                apply_at(
                    &mut s,
                    &transfer_of(&k, 0, amount, declared, None, "prop"),
                    H + 2,
                )
                .map_err(TestCaseError::fail)?;
                prop_assert_eq!(bal(&s, &k.user_addr), extra);
                prop_assert_eq!(bal(&s, &k.to), u128::from(amount));
                prop_assert_eq!(s.supply_authorized_burning, u128::from(FEE));
                prop_assert_eq!(s.issuance_committed_supply(), before - u128::from(FEE));
                s.check_supply_conservation()
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                Ok(())
            },
        )
        .unwrap();
}
