#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::routes::snapshot::snapshot_test_support::snapshot_of;
    use crate::routes::snapshot::{
        BootstrapTrust, GenesisExpectation, SnapshotApplyRefusal, TestGenesisPin, bootstrap_trust,
        install_verified_snapshot, verify_and_apply_snapshot_at,
    };
    use wrfcoin_native_chain::blockchain::state::{Account, State};
    use wrfcoin_shared_protocol::blockchain::snapshot_sync::SnapshotSyncPayload;
    use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion::V2;
    use wrfcoin_shared_protocol::crypto::signature::Signature;
    use wrfcoin_shared_protocol::crypto::signer::SignerProvenance;

    #[derive(Debug)]
    struct UnavailableSnapshotSigner {
        inner: Arc<dyn BlockSigner>,
        attempts: std::sync::atomic::AtomicUsize,
    }

    impl BlockSigner for UnavailableSnapshotSigner {
        fn sign(&self, _: &[u8]) -> anyhow::Result<Signature> {
            self.attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            anyhow::bail!("synthetic snapshot signer unavailable")
        }

        fn public_key(&self) -> PublicKey {
            self.inner.public_key()
        }

        fn provenance(&self) -> &SignerProvenance {
            self.inner.provenance()
        }
    }

    async fn assert_unfinalized_unsigned_export(signer: Option<Arc<dyn BlockSigner>>) {
        let mut inner = test_state(None).into_inner();
        Arc::get_mut(&mut inner).unwrap().snapshot_signing_key = signer;
        let state = web::Data::from(inner);
        {
            let mut chain = state.blockchain.write().await;
            *chain =
                crate::routes::snapshot::snapshot_test_support::with_chain_committing_to_its_state(
                    Blockchain::default(),
                    4,
                );
            chain.set_finalized_height(3);
        }
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;
        let request = test::TestRequest::get()
            .uri("/state/snapshot/sync-payload")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: synthetic test fixture bearer
            .to_request();
        let response = test::call_service(&app, request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: SnapshotSyncPayloadResponse = test::read_body_json(response).await;
        assert_eq!(body.payload.height, 4);
        assert_eq!(body.payload.finalized_height, 3);
        assert!(body.signed_payload.is_none());
        assert_eq!(state.blockchain.read().await.height, 4);
        assert_eq!(state.blockchain.read().await.finalized_height(), 3);
        assert!(state.snapshot_sync_seen.read().await.is_empty());
    }

    #[actix_web::test]
    async fn unsigned_export_without_signer_remains_available_3331() {
        assert_unfinalized_unsigned_export(None).await;
    }

    #[actix_web::test]
    async fn unsigned_export_with_failed_signer_remains_available_3331() {
        let (key, _) = keypair(3333);
        let signer = Arc::new(UnavailableSnapshotSigner {
            inner: signer_from_test_private_key(key),
            attempts: std::sync::atomic::AtomicUsize::new(0),
        });
        assert_unfinalized_unsigned_export(Some(signer.clone())).await;
        assert_eq!(signer.attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[actix_web::test]
    async fn finality_inert_source_still_serves_signed_export_3331() {
        let (key, _) = keypair(3334);
        let state = test_state(Some(key.clone()));
        {
            let mut chain = state.blockchain.write().await;
            *chain =
                crate::routes::snapshot::snapshot_test_support::with_chain_committing_to_its_state(
                    Blockchain::default(),
                    4,
                );
            assert_eq!(chain.finalized_height(), 0);
        }
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;
        let request = test::TestRequest::get()
            .uri("/state/snapshot/sync-payload")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: synthetic test fixture bearer
            .to_request();
        let response = test::call_service(&app, request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: SnapshotSyncPayloadResponse = test::read_body_json(response).await;
        let signed = body.signed_payload.unwrap();
        assert!(signed.has_valid_signature());
        assert_eq!(signed.payload.height, 4);
        assert_eq!(signed.payload.finalized_height, 0);
        assert_eq!(state.blockchain.read().await.finalized_height(), 0);
        // The original shared fixture must model the actual PoP export,
        // including its inert finality claim, without finalizing the tip.
        let (fixture, entries, blocks) = {
            let chain = state.blockchain.read().await;
            snapshot_of(&chain.state, chain.height)
        };
        assert_eq!(signed.payload, fixture);
        let _pin = TestGenesisPin::set(blocks[0].hash.unwrap().to_hex());
        let recipient = test_state(Some(key));
        let ingest_app = test::init_service(
            App::new()
                .app_data(recipient.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;
        let body_entries: Vec<_> = entries
            .iter()
            .map(|(k, v)| json!({"key_hex":hex::encode(k), "value_hex":hex::encode(v)}))
            .collect();
        let ingest = test::TestRequest::post()
            .uri("/network/snapshot-sync/ingest")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: synthetic test fixture bearer
            .set_json(json!({"signed_payload":signed,"entries":body_entries}))
            .to_request();
        assert_eq!(
            test::call_service(&ingest_app, ingest).await.status(),
            StatusCode::OK
        );
        assert_eq!(
            recipient.blockchain.read().await.tip,
            state.blockchain.read().await.tip
        );
        assert_eq!(
            hex::encode(recipient.blockchain.read().await.state.state_root.0),
            fixture.state_root_hex
        );
        assert_eq!(recipient.blockchain.read().await.finalized_height(), 4);
    }

    #[actix_web::test]
    async fn finality_inert_snapshot_still_installs_and_reloads_3331() {
        let (key, _) = keypair(3335);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inert-recipient.redb");
        let mut inner = test_state(Some(key.clone())).into_inner();
        Arc::get_mut(&mut inner).unwrap().storage =
            Arc::new(RwLock::new(Storage::open(&path, 100).unwrap()));
        let state = web::Data::from(inner);
        let mut source = State::default();
        source.recalculate_state_root();
        let (payload, entries, blocks) = snapshot_of(&source, 4);
        let _pin = TestGenesisPin::set(blocks[0].hash.unwrap().to_hex());
        assert_eq!(payload.finalized_height, 0);
        let signed = wrfcoin_shared_protocol::blockchain::snapshot_sync::sign_snapshot_sync_payload(
            payload,
            &key,
            chrono::Utc::now().timestamp(),
            "inert-finality-3331",
        );
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;
        let body_entries: Vec<_> = entries
            .iter()
            .map(|(k, v)| json!({"key_hex":hex::encode(k), "value_hex":hex::encode(v)}))
            .collect();
        let request = || {
            test::TestRequest::post()
                .uri("/network/snapshot-sync/ingest")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: synthetic test fixture bearer
                .set_json(json!({"signed_payload":signed,"entries":body_entries}))
                .to_request()
        };
        let response = test::call_service(&app, request()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let accepted: serde_json::Value = test::read_body_json(response).await;
        // SOL-R story 8: the owner-permitted inert-finality bootstrap says so.
        assert_eq!(accepted["bootstrap_trust"], "signer_trusted_bootstrap");
        assert_eq!(state.blockchain.read().await.height, 4);
        assert_eq!(state.blockchain.read().await.finalized_height(), 4);
        assert_eq!(state.blockchain.read().await.tip, blocks[3].hash.unwrap());
        assert_eq!(
            state.blockchain.read().await.state.state_root,
            source.state_root
        );
        assert_eq!(
            test::call_service(&app, request()).await.status(),
            StatusCode::CONFLICT
        );
        drop(app);
        drop(state);
        let reopened = Storage::open(&path, 100).unwrap();
        let restarted = reopened.load_state().unwrap().unwrap();
        assert_eq!(restarted.height, 4);
        assert_eq!(restarted.finalized_height(), 4);
        assert_eq!(restarted.tip, blocks[3].hash.unwrap());
        assert_eq!(restarted.state.state_root, source.state_root);
    }

    /// A local source can reorg its unfinalized tip, while an installed recipient
    /// has no earlier state/WAL and therefore cannot. Exercise real persistence,
    /// restart loading and fork planning; these fixture blocks test the planner,
    /// not consensus acceptance of a competing branch.
    #[tokio::test]
    async fn unfinalized_source_reorg_cannot_install_a_locked_recipient_3331() {
        let mut source_state = State::default();
        source_state.recalculate_state_root();
        let (mut payload, entries, blocks) = snapshot_of(&source_state, 3);
        payload.finalized_height = 1;
        let source = Blockchain::from_persisted(
            3,
            blocks[2].hash.unwrap(),
            source_state.clone(),
            blocks.iter().cloned().map(|b| (b.height, b)).collect(),
            3,
            1,
        );
        let mut competing = blocks[2].clone();
        competing.nonce ^= 3331;
        competing.hash = None;
        competing.set_hash(competing.calculate_hash());
        let mut next = competing.clone();
        next.height = 4;
        next.prev_block_hash = competing.hash.unwrap();
        next.hash = None;
        next.set_hash(next.calculate_hash());
        let candidate = [competing, next];
        assert!(
            source.plan_reorg(&candidate).unwrap().is_some(),
            "source adopts a fork at 2 above finality 1"
        );
        let genesis = GenesisExpectation {
            pinned_hash: Some(blocks[0].hash.unwrap().to_hex()),
            ..GenesisExpectation::of_this_node()
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recipient.redb");
        let recipient = RwLock::new(Blockchain::default());
        let storage = RwLock::new(Storage::open(&path, 100).unwrap());
        let verified =
            verify_and_apply_snapshot_at(V2, &genesis, State::default(), &payload, &entries);
        let refused = verified.is_err();
        if let Ok(verified) = verified {
            install_verified_snapshot(&recipient, &storage, verified, 3)
                .await
                .unwrap();
            assert_eq!(recipient.read().await.finalized_height(), 3);
            assert!(
                recipient
                    .read()
                    .await
                    .plan_reorg(&candidate)
                    .unwrap()
                    .is_none()
            );
            drop(storage);
            let reopened = Storage::open(&path, 100).unwrap();
            let restarted = reopened.load_state().unwrap().unwrap();
            assert_eq!(restarted.finalized_height(), 3);
            assert!(restarted.plan_reorg(&candidate).unwrap().is_none());
            eprintln!(
                "REPRODUCED #3331: source accepts fork at 2, recipient rejects before/after restart at finalized 3"
            );
        } else {
            assert_eq!(recipient.read().await.height, 0);
            assert!(!storage.read().await.has_chain_history().unwrap());
            // Same recipient can instead install the state from a finalized tip.
            payload.finalized_height = payload.height;
            let verified =
                verify_and_apply_snapshot_at(V2, &genesis, State::default(), &payload, &entries)
                    .unwrap();
            install_verified_snapshot(&recipient, &storage, verified, 3)
                .await
                .unwrap();
            assert_eq!(
                recipient.read().await.state.state_root,
                source_state.state_root
            );
            assert_eq!(recipient.read().await.tip, blocks[2].hash.unwrap());
            let mut at_anchor = blocks[2].clone();
            at_anchor.height = 4;
            at_anchor.prev_block_hash = blocks[2].hash.unwrap();
            at_anchor.hash = None;
            at_anchor.set_hash(at_anchor.calculate_hash());
            assert!(
                recipient
                    .read()
                    .await
                    .plan_reorg(&[at_anchor])
                    .unwrap()
                    .is_some(),
                "extensions at H remain possible"
            );
        }
        assert!(
            refused,
            "an unfinalized source tip must be refused before it permanently anchors a fresh node"
        );
    }

    #[actix_web::test]
    async fn snapshot_finality_http_refusal_preserves_retry_and_export_3331() {
        let (signer, _) = keypair(3331);
        let state = test_state(Some(signer.clone()));
        let mut source = State::default();
        source
            .accounts
            .insert(Address::new([0x33; 20]), Account::new(Balance(3331)));
        source.recalculate_state_root();
        let (mut payload, entries, blocks) = snapshot_of(&source, 3);
        let _pin = TestGenesisPin::set(blocks[0].hash.unwrap().to_hex());
        payload.finalized_height = 2;
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;
        let body_entries: Vec<_> = entries
            .iter()
            .map(|(k, v)| json!({"key_hex":hex::encode(k), "value_hex":hex::encode(v)}))
            .collect();
        let sign = |p| {
            wrfcoin_shared_protocol::blockchain::snapshot_sync::sign_snapshot_sync_payload(
                p,
                &signer,
                chrono::Utc::now().timestamp(),
                "snapshot-finality-3331",
            )
        };
        let request = |signed| {
            test::TestRequest::post()
                .uri("/network/snapshot-sync/ingest")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: synthetic test fixture bearer
                .set_json(json!({"signed_payload":signed,"entries":body_entries}))
                .to_request()
        };
        let old_root = state.blockchain.read().await.state.state_root;
        let response = test::call_service(&app, request(sign(payload.clone()))).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body: serde_json::Value = test::read_body_json(response).await;
        assert!(
            body.to_string().contains("SNAPSHOT_TIP_NOT_FINALIZED"),
            "{body}"
        );
        assert_eq!(state.blockchain.read().await.height, 0);
        assert_eq!(state.blockchain.read().await.state.state_root, old_root);
        assert!(!state.storage.read().await.has_chain_history().unwrap());
        assert!(state.snapshot_sync_seen.read().await.is_empty());
        // Altering the signed claim without signing again cannot pass trust.
        let mut altered = sign(payload.clone());
        altered.payload.finalized_height = 3;
        let tampered = test::call_service(&app, request(altered)).await;
        assert_eq!(tampered.status(), StatusCode::UNPROCESSABLE_ENTITY);
        payload.finalized_height = 3;
        let good = sign(payload.clone());
        assert_eq!(
            test::call_service(&app, request(good.clone()))
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(state.blockchain.read().await.height, 3);
        assert_eq!(state.blockchain.read().await.finalized_height(), 3);
        assert_eq!(
            state.blockchain.read().await.state.state_root,
            source.state_root
        );
        assert_eq!(
            test::call_service(&app, request(good)).await.status(),
            StatusCode::CONFLICT
        );
    }

    #[actix_web::test]
    async fn unfinalized_tip_is_not_signed_for_export_3331() {
        let (signer, _) = keypair(3332);
        let state = test_state(Some(signer));
        {
            let mut chain = state.blockchain.write().await;
            *chain =
                crate::routes::snapshot::snapshot_test_support::with_chain_committing_to_its_state(
                    Blockchain::default(),
                    4,
                );
            chain.set_finalized_height(3);
        }
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;
        let export = || {
            test::TestRequest::get()
                .uri("/state/snapshot/sync-payload")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: synthetic test fixture bearer
                .to_request()
        };
        let unavailable = test::call_service(&app, export()).await;
        assert_eq!(unavailable.status(), StatusCode::CONFLICT);
        let body: serde_json::Value = test::read_body_json(unavailable).await;
        assert!(
            body.to_string().contains("SNAPSHOT_TIP_NOT_FINALIZED"),
            "{body}"
        );
        state.blockchain.write().await.set_finalized_height(4);
        let available = test::call_service(&app, export()).await;
        assert_eq!(available.status(), StatusCode::OK);
        let body: SnapshotSyncPayloadResponse = test::read_body_json(available).await;
        let signed = body.signed_payload.unwrap();
        assert_eq!(signed.payload.height, 4);
        assert_eq!(signed.payload.finalized_height, 4);
        assert!(signed.has_valid_signature());
    }

    // ---- SOL-R story 8: one explicit bootstrap-trust disposition (Refs #3331) -----------------------------------

    fn state_with_balance(seed: u8, balance: u128) -> State {
        let mut state = State::default();
        state
            .accounts
            .insert(Address::new([seed; 20]), Account::new(Balance(balance)));
        state.recalculate_state_root();
        state
    }

    fn genesis_of(blocks: &[Block]) -> GenesisExpectation {
        GenesisExpectation {
            pinned_hash: Some(blocks[0].hash.unwrap().to_hex()),
            ..GenesisExpectation::of_this_node()
        }
    }

    #[actix_web::test]
    async fn the_disposition_is_a_function_of_the_signed_pair_only_3331() {
        assert_eq!(
            bootstrap_trust(3, 3),
            BootstrapTrust::SignerAttestedFinalTip
        );
        assert_eq!(
            bootstrap_trust(0, 3),
            BootstrapTrust::SignerTrustedBootstrap
        );
        assert_eq!(
            BootstrapTrust::SignerAttestedFinalTip.label(),
            "signer_attested_final_tip"
        );
        assert_eq!(
            BootstrapTrust::SignerTrustedBootstrap.label(),
            "signer_trusted_bootstrap"
        );
        // Neither label claims a fork-final certificate.
        for trust in [
            BootstrapTrust::SignerAttestedFinalTip,
            BootstrapTrust::SignerTrustedBootstrap,
        ] {
            assert!(
                trust.label().starts_with("signer_"),
                "{trust:?} must name the signer as the authority"
            );
            assert!(!trust.label().contains("certif"), "{trust:?}");
        }
    }

    #[actix_web::test]
    async fn a_final_tip_admits_and_a_non_final_signed_tip_refuses_3331() {
        let source = state_with_balance(0x31, 3331);
        let (mut payload, entries, blocks) = snapshot_of(&source, 3);
        let genesis = genesis_of(&blocks);

        payload.finalized_height = 3;
        let admitted =
            verify_and_apply_snapshot_at(V2, &genesis, State::default(), &payload, &entries)
                .expect("a final signed tip is admitted");
        assert_eq!(admitted.trust, BootstrapTrust::SignerAttestedFinalTip);

        for finalized in [1, 2] {
            payload.finalized_height = finalized;
            let refused =
                verify_and_apply_snapshot_at(V2, &genesis, State::default(), &payload, &entries)
                    .expect_err("a non-final signed tip with finalized > 0 must refuse");
            assert_eq!(
                refused,
                SnapshotApplyRefusal::TipNotFinalized {
                    finalized,
                    height: 3
                },
                "finalized {finalized} of 3"
            );
        }
    }

    #[actix_web::test]
    async fn inert_finality_is_the_signer_trusted_bootstrap_and_never_fork_final_3331() {
        let source = state_with_balance(0x32, 3332);
        let (payload, entries, blocks) = snapshot_of(&source, 3);
        assert_eq!(payload.finalized_height, 0);
        let admitted = verify_and_apply_snapshot_at(
            V2,
            &genesis_of(&blocks),
            State::default(),
            &payload,
            &entries,
        )
        .expect("the owner-permitted inert bootstrap path stays available");
        assert_eq!(admitted.trust, BootstrapTrust::SignerTrustedBootstrap);
        assert_ne!(admitted.trust, BootstrapTrust::SignerAttestedFinalTip);
    }

    /// Two sources at the SAME height with different state (an alternate fork)
    /// each get exactly the disposition their own signed fields declare, and a
    /// fork cannot borrow the other's.
    #[actix_web::test]
    async fn same_height_alternate_forks_keep_their_own_disposition_3331() {
        let fork_a = state_with_balance(0x41, 100);
        let fork_b = state_with_balance(0x42, 200);
        let (mut a, entries_a, blocks_a) = snapshot_of(&fork_a, 3);
        let (mut b, entries_b, blocks_b) = snapshot_of(&fork_b, 3);
        assert_eq!(a.height, b.height);
        assert_ne!(
            a.tip_hash_hex, b.tip_hash_hex,
            "the forks really differ at the same height"
        );
        assert_ne!(a.state_root_hex, b.state_root_hex);
        let verify = |p: &SnapshotSyncPayload, e: &[(Vec<u8>, Vec<u8>)], g: &[Block]| {
            verify_and_apply_snapshot_at(V2, &genesis_of(g), State::default(), p, e)
        };

        // A final, B inert.
        a.finalized_height = 3;
        b.finalized_height = 0;
        assert_eq!(
            verify(&a, &entries_a, &blocks_a).unwrap().trust,
            BootstrapTrust::SignerAttestedFinalTip
        );
        assert_eq!(
            verify(&b, &entries_b, &blocks_b).unwrap().trust,
            BootstrapTrust::SignerTrustedBootstrap
        );

        // A final, B non-final: the non-final fork refuses while the final one admits.
        b.finalized_height = 2;
        assert_eq!(
            verify(&a, &entries_a, &blocks_a).unwrap().trust,
            BootstrapTrust::SignerAttestedFinalTip
        );
        assert_eq!(
            verify(&b, &entries_b, &blocks_b).unwrap_err(),
            SnapshotApplyRefusal::TipNotFinalized {
                finalized: 2,
                height: 3
            }
        );

        // Swapping the signed finality fields swaps the dispositions: nothing else decides it.
        a.finalized_height = 0;
        b.finalized_height = 3;
        assert_eq!(
            verify(&a, &entries_a, &blocks_a).unwrap().trust,
            BootstrapTrust::SignerTrustedBootstrap
        );
        assert_eq!(
            verify(&b, &entries_b, &blocks_b).unwrap().trust,
            BootstrapTrust::SignerAttestedFinalTip
        );
    }

    /// Restart control: both admitted dispositions install a LOCAL anchor at the
    /// snapshot height that survives a reopen, but the label is not persisted, so
    /// a restarted node cannot claim a finality certificate it never had; the same
    /// signed snapshot verified again after the restart declares the same disposition.
    #[tokio::test]
    async fn restart_keeps_the_local_anchor_and_the_disposition_is_rederived_3331() {
        for (finalized, expected) in [
            (0u64, BootstrapTrust::SignerTrustedBootstrap),
            (3u64, BootstrapTrust::SignerAttestedFinalTip),
        ] {
            let source = state_with_balance(0x51, 5000 + u128::from(finalized));
            let (mut payload, entries, blocks) = snapshot_of(&source, 3);
            payload.finalized_height = finalized;
            let genesis = genesis_of(&blocks);
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("restart.redb");
            let recipient = RwLock::new(Blockchain::default());
            let storage = RwLock::new(Storage::open(&path, 100).unwrap());
            let verified =
                verify_and_apply_snapshot_at(V2, &genesis, State::default(), &payload, &entries)
                    .unwrap();
            assert_eq!(verified.trust, expected);
            install_verified_snapshot(&recipient, &storage, verified, 3)
                .await
                .unwrap();
            assert_eq!(recipient.read().await.finalized_height(), 3);
            drop(storage);

            let reopened = Storage::open(&path, 100).unwrap();
            let restarted = reopened.load_state().unwrap().unwrap();
            assert_eq!(restarted.height, 3);
            assert_eq!(
                restarted.finalized_height(),
                3,
                "the anchor is local and survives a restart"
            );
            assert_eq!(restarted.state.state_root, source.state_root);

            let again =
                verify_and_apply_snapshot_at(V2, &genesis, State::default(), &payload, &entries)
                    .unwrap();
            assert_eq!(
                again.trust, expected,
                "the same signed snapshot declares the same disposition"
            );
        }
    }
}
