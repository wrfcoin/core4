//! Non-vacuous transfer apply-height coverage for #2168.

use rand::{rngs::StdRng, SeedableRng};
use wrfcoin_shared_protocol::accounts::authorization::{
    authorize_debit, AccountAuthorization, AccountAuthorizationDeclaration,
    AuthorizationRule, EmptyAuthorizationView, SpendAuthorization,
};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{Transaction, TransactionType, TransparentTransferPayload};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::crypto::chain_binding::{install_feature_schedule, ConsensusFeature, FeatureSchedule};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

const H: u64 = 21_680;

#[test]
fn transfer_apply_uses_the_inclusion_height_for_the_state_backed_debit_view() {
    install_feature_schedule(FeatureSchedule::from_entries(vec![
        (ConsensusFeature::AccountAuthorizationDispatch.name().to_string(), H),
    ]).expect("schedule"));
    let mut rng = StdRng::seed_from_u64(H);
    let key = PrivateKey::generate(&mut rng);
    let sender_public_key = PublicKey::from_private_key(&key);
    let sender = Address::from_public_key(&sender_public_key);
    let recipient = Address([0xB2; 20]);
    let payload = TransparentTransferPayload {
        from: sender.to_string(), to: recipient.to_string(), amount: 10,
        transparent_opt_out: true, opt_out_audit_tag: None,
        auth_pubkey: None, auth_signature: None, authority: None, authority_signature: None,
        passkey_auth: None,
    };
    let mut tx = Transaction::new(vec![], vec![]);
    tx.tx_type = TransactionType::Transfer;
    tx.nonce = 0;
    let mut payload = payload;
    payload.sign_authorization(&key, tx.nonce);
    tx.data = Some(serde_json::to_string(&payload).expect("payload"));
    tx.set_id();

    for (height, accepted) in [(H - 1, true), (H, false)] {
        let mut state = State::new();
        state.create_account(sender, Balance::new(20));
        state.create_account(recipient, Balance::new(0));
        state.authorization_declarations.insert(sender, AccountAuthorizationDeclaration {
            next_nonce: 0,
            current: Some(AccountAuthorization::declaring(AuthorizationRule::SmartAccount { program: [7; 32] })),
            generation: 0,
        });
        let result = state.apply_transition(StateTransition::ProcessGovernanceTransaction {
            tx: tx.clone(), current_block: height,
        });
        assert_eq!(result.is_ok(), accepted, "production apply result at height {height}: {result:?}");
        assert_eq!(state.get_account(&sender).unwrap().balance, Balance::new(if accepted { 10 } else { 20 }));
        assert_eq!(state.get_account(&recipient).unwrap().balance, Balance::new(if accepted { 10 } else { 0 }));
    }
}

#[test]
fn pre_block_empty_view_agrees_with_state_view_for_every_storable_rule() {
    install_feature_schedule(FeatureSchedule::from_entries(vec![
        (ConsensusFeature::AccountAuthorizationDispatch.name().to_string(), H),
    ]).expect("schedule"));
    let mut rng = StdRng::seed_from_u64(H + 1);
    let key = PrivateKey::generate(&mut rng);
    let public_key = PublicKey::from_private_key(&key);
    let account = Address::from_public_key(&public_key);
    let public_key_hex = public_key.to_hex();
    let message = b"pre-block tripwire";
    let signature = key.sign(message).to_hex();

    let rules = vec![
        AuthorizationRule::SignerIsSender,
        AuthorizationRule::SmartAccount { program: [3; 32] },
        AuthorizationRule::Delegated { program: [4; 32], valid_until: None },
    ];
    for rule in rules {
        // Deliberately exhaustive: adding an enum variant fails compilation
        // until this tripwire's enumeration is revised.
        match &rule {
            AuthorizationRule::SignerIsSender => {},
            AuthorizationRule::SmartAccount { .. } => {},
            AuthorizationRule::Delegated { .. } => {},
        }
        let declaration = AccountAuthorization::declaring(rule.clone());
        let declaration_check = wrfcoin_shared_protocol::accounts::authorization::check_declaration(
            &rule, H, wrfcoin_shared_protocol::common::types::Timestamp::new(1),
        );
        if declaration_check.is_ok() {
            let mut state = State::new();
            state.authorization_declarations.insert(account, AccountAuthorizationDeclaration {
                next_nonce: 0, current: Some(declaration), generation: 0,
            });
            let pre_block = authorize_debit(
                &EmptyAuthorizationView, &account, message,
                SpendAuthorization { signer_public_key: &public_key_hex, signature: &signature }, H,
                wrfcoin_shared_protocol::common::types::Timestamp::new(1),
            );
            let state_backed = authorize_debit(
                &state, &account, message,
                SpendAuthorization { signer_public_key: &public_key_hex, signature: &signature }, H,
                wrfcoin_shared_protocol::common::types::Timestamp::new(1),
            );
            assert_eq!(pre_block, state_backed, "accepted rule must be safe for EmptyView pre-block validation");
        } else {
            assert!(declaration_check.is_err(), "unsupported rule must remain refused");
        }
    }
}
