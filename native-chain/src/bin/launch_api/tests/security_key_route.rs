//! Security-key wallet routes (`security_key_spend`): status read, action
//! admission, and the `/transactions/submit` State-backed refusal of a
//! phrase-only send from a protected account.
//!
//! Each test runs in its own child process (`isolated_chain_domain`), so the
//! process-wide feature overlay it installs cannot leak into other tests.
use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator;
    use wrfcoin_shared_protocol::accounts::security_key::{
        ESCAPE_DELAY_MIN_BLOCKS, SecurityKeyActionBody, SecurityKeyActionPayload,
        SecurityKeyAssertionWire, SecurityKeyWire, action_challenge, action_message, apply_action,
        spend_challenge,
    };
    use wrfcoin_shared_protocol::blockchain::transaction::transparent_transfer_authorization_message;
    use wrfcoin_shared_protocol::crypto::chain_binding::{
        ChainIdentity, ConsensusFeature, refresh_feature_schedule_extensions,
    };

    const TO: &str = "0x2222222222222222222222222222222222222222";

    fn owner_key() -> PrivateKey {
        PrivateKey::from_bytes(&[0x3C; 32]).unwrap()
    }
    fn owner() -> wrfcoin_shared_protocol::common::types::Address {
        wrfcoin_shared_protocol::common::types::Address::from_public_key(&PublicKey::from_private_key(&owner_key()))
    }
    fn yubikey() -> SoftwareAuthenticator {
        SoftwareAuthenticator::new(&[0x4D; 32], "wrfcoin.com")
    }
    fn assertion(challenge: &[u8; 32]) -> SecurityKeyAssertionWire {
        let yk = yubikey();
        let raw = yk.assert(challenge);
        SecurityKeyAssertionWire {
            public_key_p256: hex::encode(yk.key.public_key().to_raw_xy()),
            authenticator_data: hex::encode(raw.authenticator_data),
            client_data_json: hex::encode(raw.client_data_json),
            signature: hex::encode(raw.signature),
        }
    }
    fn enable_payload(corrupt: bool) -> SecurityKeyActionPayload {
        let yk = yubikey();
        let body = SecurityKeyActionBody::Enable {
            key: SecurityKeyWire {
                public_key_p256: hex::encode(yk.key.public_key().to_raw_xy()),
                rp_id_hash: hex::encode(yk.rp_id_hash),
                credential_id: "c0de".into(),
            },
            escape_delay_blocks: ESCAPE_DELAY_MIN_BLOCKS,
        };
        let id = ChainIdentity::active();
        let message = action_message(&id.chain_id, &id.network_id, &owner(), 0, &body).unwrap();
        let mut signature = owner_key().sign(&message).to_hex();
        if corrupt {
            signature = signature.chars().rev().collect();
        }
        SecurityKeyActionPayload {
            account: owner().to_string(),
            nonce: 0,
            action: body,
            owner_public_key: PublicKey::from_private_key(&owner_key()).to_hex(),
            owner_signature: signature,
            key_assertions: vec![assertion(&action_challenge(&message))],
        }
    }
    fn bearer() -> (&'static str, String) {
        ("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")) // secret-leak-ok: synthetic test bearer only.
    }
    fn activate() {
        refresh_feature_schedule_extensions(vec![
            (ConsensusFeature::AccountAuthorizationDispatch.name().to_string(), 0),
            (ConsensusFeature::SecurityKeySpend.name().to_string(), 1),
        ]);
    }

    #[actix_web::test]
    async fn inactive_feature_reports_status_and_refuses_actions() {
        const NAME: &str = "tests::security_key_route::tests::inactive_feature_reports_status_and_refuses_actions";
        if crate::helpers::test_env::isolated_chain_domain(NAME, "777", "testnet", "0x2727272727272727272727272727272727272727", "none") {
            return;
        }
        let server = test_state(None);
        let app = test::init_service(test_app(server.clone())).await;
        let status = test::call_service(
            &app,
            test::TestRequest::get().uri(&format!("/accounts/{}/security-key", owner())).to_request(),
        )
        .await;
        assert_eq!(status.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(status).await;
        assert_eq!(body["data"]["feature_active_at_next_height"], false, "{body}");
        assert_eq!(body["data"]["required_to_spend"], false);
        assert_eq!(body["data"]["next_nonce"], 0);

        let unauth = test::call_service(
            &app,
            test::TestRequest::post().uri("/accounts/security-key/actions").set_json(enable_payload(false)).to_request(),
        )
        .await;
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);
        let inactive = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/accounts/security-key/actions")
                .insert_header(bearer())
                .set_json(enable_payload(false))
                .to_request(),
        )
        .await;
        assert_eq!(inactive.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(server.mempool.read().await.get_all_transactions().is_empty());
    }

    #[actix_web::test]
    async fn active_feature_admits_a_valid_action_and_refuses_a_forged_one() {
        const NAME: &str = "tests::security_key_route::tests::active_feature_admits_a_valid_action_and_refuses_a_forged_one";
        if crate::helpers::test_env::isolated_chain_domain(NAME, "777", "testnet", "0x2727272727272727272727272727272727272727", "none") {
            return;
        }
        activate();
        let server = test_state(None);
        let app = test::init_service(test_app(server.clone())).await;
        let forged = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/accounts/security-key/actions")
                .insert_header(bearer())
                .set_json(enable_payload(true))
                .to_request(),
        )
        .await;
        assert_eq!(forged.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value = test::read_body_json(forged).await;
        assert!(body.to_string().contains("SECURITY_KEY_ACTION_REJECTED"), "{body}");
        assert!(server.mempool.read().await.get_all_transactions().is_empty());

        let ok = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/accounts/security-key/actions")
                .insert_header(bearer())
                .set_json(enable_payload(false))
                .to_request(),
        )
        .await;
        let status = ok.status();
        let body: serde_json::Value = test::read_body_json(ok).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let pending = server.mempool.read().await.get_all_transactions();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].tx_type, TransactionType::SecurityKeyAction);
    }

    #[actix_web::test]
    async fn submit_refuses_a_phrase_only_send_from_a_protected_account() {
        const NAME: &str = "tests::security_key_route::tests::submit_refuses_a_phrase_only_send_from_a_protected_account";
        if crate::helpers::test_env::isolated_chain_domain(NAME, "777", "testnet", "0x2727272727272727272727272727272727272727", "none") {
            return;
        }
        activate();
        let server = test_state(None);
        {
            let mut chain = server.blockchain.write().await;
            chain.state.create_account(owner(), wrfcoin_shared_protocol::common::types::Balance::new(1_000));
            let id = ChainIdentity::active();
            apply_action(
                &mut chain.state.authorization_declarations,
                &enable_payload(false),
                1,
                &id.chain_id,
                &id.network_id,
            )
            .unwrap();
        }
        let app = test::init_service(test_app(server.clone())).await;
        let from = owner().to_string();
        let message = transparent_transfer_authorization_message(&from, TO, 10, 0);
        let signature = owner_key().sign(&message).to_hex();
        let base = json!({
            "from": from, "to": TO, "amount": 10, "nonce": "0",
            "transparent_opt_out": true,
            "auth_pubkey": PublicKey::from_private_key(&owner_key()).to_hex(),
            "auth_signature": signature,
        });
        let phrase_only = test::call_service(
            &app,
            test::TestRequest::post().uri("/transactions/submit").insert_header(bearer()).set_json(base.clone()).to_request(),
        )
        .await;
        assert_eq!(phrase_only.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value = test::read_body_json(phrase_only).await;
        assert!(body.to_string().contains("SECURITY_KEY_REQUIRED"), "{body}");
        assert!(server.mempool.read().await.get_all_transactions().is_empty());

        let mut with_key = base.clone();
        with_key["security_key_auth"] = serde_json::to_value(assertion(&spend_challenge(&owner(), &message))).unwrap();
        let ok = test::call_service(
            &app,
            test::TestRequest::post().uri("/transactions/submit").insert_header(bearer()).set_json(with_key).to_request(),
        )
        .await;
        let status = ok.status();
        let body: serde_json::Value = test::read_body_json(ok).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let pending = server.mempool.read().await.get_all_transactions();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].security_key_spend_proof().is_some(), "the assertion rides in the transaction");

        let status = test::call_service(
            &app,
            test::TestRequest::get().uri(&format!("/accounts/{}/security-key", owner())).to_request(),
        )
        .await;
        let body: serde_json::Value = test::read_body_json(status).await;
        assert_eq!(body["data"]["required_to_spend"], true, "{body}");
        assert_eq!(body["data"]["next_nonce"], 1);
        assert_eq!(body["data"]["keys"].as_array().unwrap().len(), 1);
    }
}
