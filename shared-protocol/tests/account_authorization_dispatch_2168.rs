//! #2168 — the per-account authorization dispatch, its scheduling, and the
//! user-operation mempool lane.
//!
//! Three properties this target exists to pin, none of which the unit tests
//! beside the modules can reach:
//!
//! 1. **Dormancy is scheduled, not accidental.** A real `FeatureSchedule` is
//!    installed here, so the dispatch is genuinely inactive below its
//!    activation height and genuinely active at and above it. Without an
//!    installed schedule every feature is inactive, which would make a
//!    dormancy test pass for the wrong reason.
//! 2. **The two mempool lanes cannot starve each other.** Filling one must
//!    leave the other's capacity untouched, in both directions.
//! 3. **A binary without this rule halts rather than forks**, via
//!    `KNOWN_FEATURES`.

use wrfcoin_shared_protocol::accounts::abstraction::{
    gas_costs, GasMeter, UserOperation, ValidationContext, ValidationPipeline,
};
use wrfcoin_shared_protocol::accounts::authorization::{
    apply_declaration, authorize_debit, authorization_for, check_declaration,
    declare_authorization_message, dispatch_active, AccountAuthorization,
    AccountAuthorizationDeclaration, AuthorizationError, AuthorizationRule,
    DeclareAuthorization, DeclarationError, SpendAuthorization,
};
use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput,
};
use wrfcoin_shared_protocol::common::types::{
    Address, BlockNumber, ChainId, Nonce, Timestamp,
};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    install_feature_schedule, ConsensusFeature, FeatureSchedule,
};
use wrfcoin_shared_protocol::crypto::protocol_support::KNOWN_FEATURES;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::{Mempool, DEFAULT_USER_OP_LANE_CAPACITY};
use std::collections::BTreeMap;
use rand::{rngs::StdRng, SeedableRng};

const ACTIVATION: u64 = 20_000;
const PROGRAM: [u8; 32] = [3u8; 32];

fn install_once() {
    let schedule = FeatureSchedule::from_entries(vec![(
        ConsensusFeature::AccountAuthorizationDispatch
            .name()
            .to_string(),
        ACTIVATION,
    )])
    .expect("single-feature schedule is valid");
    install_feature_schedule(schedule);
}

fn smart_rule() -> AuthorizationRule {
    AuthorizationRule::SmartAccount { program: PROGRAM }
}

fn signed_declaration(rule: AuthorizationRule, nonce: u64, generation: u64) -> DeclareAuthorization {
    let mut rng = StdRng::seed_from_u64(2168 + nonce);
    let private_key = PrivateKey::generate(&mut rng);
    let public_key = PublicKey::from_private_key(&private_key);
    let account = Address::from_public_key(&public_key);
    let authorization = Some(AccountAuthorization::declaring(rule));
    let message = declare_authorization_message(
        &wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active().chain_id,
        &account,
        &authorization,
        nonce,
        generation,
    );
    DeclareAuthorization {
        account,
        authorization,
        nonce,
        generation,
        signer_public_key: public_key.to_hex(),
        signature: private_key.sign(&message).to_hex(),
    }
}

#[test]
fn declaration_apply_refuses_unsupported_rules_without_consuming_the_wrapper() {
    install_once();
    let cases = [
        smart_rule(),
        AuthorizationRule::Delegated { program: PROGRAM, valid_until: None },
        AuthorizationRule::Delegated { program: PROGRAM, valid_until: Some(Timestamp::new(u64::MAX)) },
    ];
    for rule in cases {
        let declaration = signed_declaration(rule.clone(), 0, 0);
        let account = declaration.account;
        let mut states = BTreeMap::new();
        let before = AccountAuthorizationDeclaration::default();
        let result = apply_declaration(
            &mut states,
            declaration,
            ACTIVATION,
            Timestamp::new(7),
            &wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active().chain_id,
        );
        assert_eq!(result, Err(DeclarationError::Authorization(AuthorizationError::RuleNotImplemented)));
        assert_eq!(states.get(&account), None, "refusal must not create/advance wrapper: {rule:?}");
        assert_eq!(AccountAuthorizationDeclaration::default(), before);
    }
}

#[test]
fn refused_delegation_cannot_lock_out_the_owners_normal_debit() {
    install_once();
    let declaration = signed_declaration(
        AuthorizationRule::Delegated { program: PROGRAM, valid_until: Some(Timestamp::new(u64::MAX)) },
        0,
        0,
    );
    let account = declaration.account;
    let mut state = State::new();
    assert_eq!(
        apply_declaration(
            &mut state.authorization_declarations,
            declaration,
            ACTIVATION,
            Timestamp::new(1),
            &wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active().chain_id,
        ),
        Err(DeclarationError::Authorization(AuthorizationError::RuleNotImplemented))
    );
    // `signed_declaration(..., 0, 0)` derives this same owner key.
    let mut rng = StdRng::seed_from_u64(2168);
    let key = PrivateKey::generate(&mut rng);
    let owner_key = PublicKey::from_private_key(&key);
    let owner_key_hex = owner_key.to_hex();
    let owner = Address::from_public_key(&owner_key);
    let message = b"owner debit";
    let signature = key.sign(message).to_hex();
    assert!(authorize_debit(
        &state,
        &owner,
        message,
        SpendAuthorization { signer_public_key: &owner_key_hex, signature: &signature },
        ACTIVATION,
        Timestamp::new(u64::MAX),
    ).is_ok());
    assert!(state.authorization_declarations.get(&account).is_none());
}

#[test]
fn every_storable_declaration_is_time_inert() {
    install_once();
    let mut rng = StdRng::seed_from_u64(900_2169);
    let key = PrivateKey::generate(&mut rng);
    let public_key = PublicKey::from_private_key(&key);
    let public_key_hex = public_key.to_hex();
    let account = Address::from_public_key(&public_key);
    let message = b"time-inert debit";
    let signature = key.sign(message).to_hex();
    for declaration in [AccountAuthorization::none(), AccountAuthorization::declaring(AuthorizationRule::SignerIsSender)] {
        struct View(AccountAuthorization);
        impl wrfcoin_shared_protocol::accounts::authorization::AuthorizationView for View {
            fn declared(&self, _: &Address) -> AccountAuthorization { self.0.clone() }
        }
        let view = View(declaration);
        let at_zero = authorize_debit(&view, &account, message, SpendAuthorization { signer_public_key: &public_key_hex, signature: &signature }, ACTIVATION, Timestamp::new(0));
        let at_max = authorize_debit(&view, &account, message, SpendAuthorization { signer_public_key: &public_key_hex, signature: &signature }, ACTIVATION, Timestamp::new(u64::MAX));
        assert_eq!(at_zero, at_max);
    }
}

fn user_op(nonce: u64) -> UserOperation {
    UserOperation {
        sender: Address([4u8; 20]),
        nonce: Nonce::new(nonce),
        call_data_hash: [1u8; 32],
        max_fee_per_gas: 1,
        signer_public_key: vec![1, 2, 3],
        signature: vec![4, 5, 6],
        valid_after: None,
        valid_until: None,
        validation_gas_limit: 1_000_000,
        call_gas_limit: 0,
        paymaster: None,
    }
}

/// The dormancy property, against a REAL installed schedule.
#[test]
fn the_dispatch_is_dormant_below_its_activation_and_live_at_and_above_it() {
    install_once();

    assert!(!dispatch_active(0));
    assert!(!dispatch_active(ACTIVATION - 1));
    assert!(dispatch_active(ACTIVATION));
    assert!(dispatch_active(ACTIVATION + 1_000_000));

    let declared = AccountAuthorization::declaring(smart_rule());
    let now = Timestamp::new(0);

    // Below the activation an account's declaration is not consulted: the
    // chain behaves exactly as it does today. This is what makes the module
    // safe to merge before a genesis cut.
    assert_eq!(
        authorization_for(&declared, ACTIVATION - 1, now),
        AuthorizationRule::SignerIsSender
    );
    // At and above, it is honoured.
    assert_eq!(authorization_for(&declared, ACTIVATION, now), smart_rule());

    // Declaring is refused below activation and unsupported validators remain
    // undeclarable even once the dispatch seam is scheduled.
    assert_eq!(
        check_declaration(&smart_rule(), ACTIVATION - 1, now),
        Err(AuthorizationError::DispatchNotActive {
            height: ACTIVATION - 1
        })
    );
    assert_eq!(
        check_declaration(&smart_rule(), ACTIVATION, now),
        Err(AuthorizationError::RuleNotImplemented)
    );

    for delegated in [
        AuthorizationRule::Delegated { program: PROGRAM, valid_until: None },
        AuthorizationRule::Delegated { program: PROGRAM, valid_until: Some(Timestamp::new(u64::MAX)) },
    ] {
        assert_eq!(
            check_declaration(&delegated, ACTIVATION, now),
            Err(AuthorizationError::RuleNotImplemented),
            "unsupported delegated policy must never be stored"
        );
    }

    // Unsupported validators are refused before any rule-specific validation.
    assert_eq!(
        check_declaration(
            &AuthorizationRule::SmartAccount { program: [0u8; 32] },
            ACTIVATION,
            now
        ),
        Err(AuthorizationError::RuleNotImplemented)
    );
}

/// An account that declared nothing is unaffected at every height — the
/// state of every account on the chain today.
#[test]
fn an_undeclared_account_is_unaffected_on_both_sides_of_the_activation() {
    install_once();
    let none = AccountAuthorization::none();
    for height in [0, ACTIVATION - 1, ACTIVATION, ACTIVATION + 10_000] {
        assert_eq!(
            authorization_for(&none, height, Timestamp::new(0)),
            AuthorizationRule::SignerIsSender,
            "height {height}"
        );
    }
}

/// The lane requirement: neither pool can starve the other.
#[test]
fn the_user_operation_lane_is_bounded_independently_of_the_transaction_lane() {
    let mut mempool = Mempool::new(&Config::default());
    assert_eq!(mempool.user_operation_count(), 0);
    assert_eq!(mempool.user_op_capacity(), DEFAULT_USER_OP_LANE_CAPACITY);

    // Fill the user-op lane past its bound.
    mempool.set_user_op_capacity(8);
    for i in 0..32u64 {
        let mut hash = [0u8; 32];
        hash[..8].copy_from_slice(&i.to_be_bytes());
        assert!(mempool.add_user_operation(hash, user_op(i)));
    }
    assert_eq!(
        mempool.user_operation_count(),
        8,
        "the lane must hold at its own bound, not grow without limit"
    );

    // A flooded user-op lane leaves the transaction lane completely free.
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: wrfcoin_shared_protocol::crypto::hash::hash_bytes(b"2168"),
            vout: 0,
            signature: Some("00".into()),
            public_key: Some("00".into()),
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1,
            address: "0x1111111111111111111111111111111111111111".into(),
            script: String::new(),
            nft_data: None,
        }],
    );
    tx.set_id();
    assert!(
        mempool.add_transaction(tx).is_ok(),
        "a full user-op lane must not close the transaction lane"
    );
    assert_eq!(mempool.user_operation_count(), 8, "and vice versa");

    // Duplicate admission is a no-op, not an error: duplicate gossip arrivals
    // are ordinary.
    let mut hash = [0u8; 32];
    hash[..8].copy_from_slice(&31u64.to_be_bytes());
    assert!(!mempool.add_user_operation(hash, user_op(31)));

    // Removal is exact and idempotent.
    assert!(mempool.contains_user_operation(&hash));
    assert!(mempool.remove_user_operation(&hash).is_some());
    assert!(!mempool.contains_user_operation(&hash));
    assert!(mempool.remove_user_operation(&hash).is_none());
    assert_eq!(mempool.user_operation_count(), 7);

    // Capacity zero disables the lane outright.
    mempool.set_user_op_capacity(0);
    assert_eq!(mempool.user_operation_count(), 0);
}

/// Validation work is charged even when the operation is rejected.
#[test]
fn a_rejected_operation_is_still_charged_for_the_work_it_forced() {
    let mut pipeline = ValidationPipeline::default();
    let context = ValidationContext {
        chain_id: ChainId::new(777),
        block_number: BlockNumber::new(1),
        timestamp: Timestamp::new(1_000),
    };
    let mut op = user_op(0);
    op.signature = vec![0u8; 2048];

    let mut meter = GasMeter::new(1_000_000);
    let err = pipeline
        .validate_metered(&op, context, None, &mut meter)
        .expect_err("an unsigned-for-this-hash operation must not validate");
    let _ = err;
    assert!(
        meter.spent() >= gas_costs::BASE + gas_costs::SIGNATURE_VERIFY
            + 2048 * gas_costs::PER_SIGNATURE_BYTE,
        "a rejected operation must be charged base + verify + its signature bytes: {}",
        meter.spent()
    );

    // A budget smaller than the work required rejects for exhaustion rather
    // than admitting the operation for free.
    let mut tiny = GasMeter::new(gas_costs::BASE);
    let err = pipeline
        .validate_metered(&op, context, None, &mut tiny)
        .expect_err("budget cannot cover the signature check");
    assert!(err.to_string().contains("validation gas exhausted"), "{err}");
}

/// #3093 interaction: a scheduled consensus rule must be a known feature.
#[test]
fn the_feature_name_is_registered_so_an_old_binary_halts_rather_than_forks() {
    let name = ConsensusFeature::AccountAuthorizationDispatch.name();
    assert_eq!(name, "account_authorization_dispatch");
    assert!(KNOWN_FEATURES.contains(&name));
    for feature in ConsensusFeature::ALL {
        assert_eq!(ConsensusFeature::parse(feature.name()), Some(*feature));
        assert!(KNOWN_FEATURES.contains(&feature.name()), "{}", feature.name());
    }
    assert_eq!(KNOWN_FEATURES.len(), ConsensusFeature::ALL.len());
}
