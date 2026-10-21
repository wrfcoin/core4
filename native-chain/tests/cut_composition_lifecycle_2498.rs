// An empty test binary reports "0 passed; 0 failed; 0 ignored" — identical
// to a passing target and invisible behind sibling suites (the vacuity shape
// scripts/cargo_test_nonvacuous.py rejects as EMPTY_TEST_BINARY). Without the
// feature this target now declares its absence as a countable `ignored`
// instead of compiling to silence — #2748's principle, extended to the cfg
// dimension.
#[cfg(not(feature = "cut-composition-harness"))]
#[test]
#[ignore = "this target's tests require --features cut-composition-harness; without it the binary would be empty and indistinguishable from a passing one. Run: cargo test -p wrfcoin-native-chain --features cut-composition-harness --test cut_composition_lifecycle_2498"]
fn tests_require_the_cut_composition_harness_feature() {}

#[cfg(feature = "cut-composition-harness")]
mod gated {
    //! Connected, state-carrying genesis-cut composition rehearsal (#2498 / #2557).
    //!
    //! One producer chain and one follower chain carry the same state through:
    //!
    //! 1. seeded genesis producer authority;
    //! 2. the final N=3/K=2 oracle-registration batch in block 1;
    //! 3. checkpoint-less WAL restart at height 1;
    //! 4. consecutive rewards through the first wall-clock reduction boundary;
    //! 5. follower validation/application and final state-root convergence.
    //!
    //! The test is feature-gated because the carrier PR is intentionally based on
    //! `main`, while two required cut gates are still open. The composition script
    //! merges every required gate first, builds the resulting tree, and only then
    //! enables this target. A missing or conflicting gate therefore cannot turn
    //! into a skipped green test.

    mod transition_extractor {
        // build.rs copies only the production portion of the exact composed
        // transition extractor. Its own unit-test module depends on launch_api
        // test helpers that do not exist in an integration-test crate.
        include!(concat!(
            env!("OUT_DIR"),
            "/cut_composition_transition_extractor.rs"
        ));
    }

    use std::collections::BTreeMap;
    use std::sync::atomic::{
        AtomicI64,
        Ordering,
    };
    use std::sync::{
        Arc,
        Mutex,
    };

    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use tokio::sync::RwLock;
    use wrfcoin_native_chain::storage::Storage;
    use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
    use wrfcoin_shared_protocol::blockchain::transaction::TransactionType;
    use wrfcoin_shared_protocol::common::types::Address;
    use wrfcoin_shared_protocol::config::settings::Config;
    use wrfcoin_shared_protocol::consensus::producer_authority::verify_block_producer_authorized;
    use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;
    use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
        Clock,
        EMISSION_RETAIN_DENOMINATOR,
        EMISSION_RETAIN_NUMERATOR,
        LIVENESS_BLOCK_REWARD_FRACTION,
        ProofOfPublishing,
        adjusted_block_reward,
        elapsed_seconds_at_height,
        emission_periods_at_height,
    };
    use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;
    use wrfcoin_shared_protocol::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV;
    use wrfcoin_shared_protocol::crypto::genesis_oracle::GenesisOracleDeclaration;
    use wrfcoin_shared_protocol::crypto::signature::{
        PrivateKey,
        PublicKey,
    };
    use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;
    use wrfcoin_shared_protocol::mempool::pool::Mempool;
    use wrfcoin_shared_protocol::oracle::ceremony::GenesisOracleCeremony;

    const CHAIN_ID: &str = "2498";
    const NETWORK_ID: &str = "genesis-cut-composition";
    const NEVER_CHECKPOINT: u64 = u64::MAX / 2;
    const REDUCTION_PRE_HEIGHT: u64 = 42_999;
    const REDUCTION_HEIGHT: u64 = 43_000;
    const END_HEIGHT: u64 = 43_004;
    const FOLLOWER_PAUSE_HEIGHT: u64 = 42_995;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[derive(Debug)]
    struct ManualClock {
        seconds: AtomicI64,
    }

    impl ManualClock {
        fn new(seconds: i64) -> Self {
            Self {
                seconds: AtomicI64::new(seconds),
            }
        }

        fn set(&self, seconds: i64) {
            self.seconds.store(seconds, Ordering::SeqCst);
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
            chrono::DateTime::from_timestamp(self.seconds.load(Ordering::SeqCst), 0)
                .expect("manual clock timestamp must be representable")
        }
    }

    fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
        let mut rng = StdRng::seed_from_u64(seed);
        let private = PrivateKey::generate(&mut rng);
        let public = PublicKey::from_private_key(&private);
        (private, public)
    }

    fn payout_of(public: &PublicKey) -> Address {
        let bytes: [u8; 32] = public.as_bytes().try_into().expect("32-byte public key");
        Address(bytes[..20].try_into().expect("20-byte address prefix"))
    }

    fn producer_member(public: &PublicKey, id: &str) -> ProducerMember {
        ProducerMember {
            producer_id: id.to_string(),
            payout_address: payout_of(public).to_string(),
            public_key: public
                .as_bytes()
                .try_into()
                .expect("producer public key is 32 bytes"),
        }
    }

    fn oracle_ceremony(
        governance: &PrivateKey,
        submitted_at: i64,
    ) -> (
        Vec<String>,
        Vec<wrfcoin_shared_protocol::blockchain::transaction::Transaction>,
    ) {
        let identities: Vec<String> = (0..3)
            .map(|offset| keypair(25_570_100 + offset).1.to_hex())
            .collect();
        let signing_pubkeys: BTreeMap<String, String> = identities
            .iter()
            .map(|identity| (identity.clone(), identity.clone()))
            .collect();
        let ceremony = GenesisOracleCeremony::new(
            CHAIN_ID,
            NETWORK_ID,
            identities.clone(),
            2,
            signing_pubkeys,
            1,
            1_785_240_000,
            submitted_at,
        )
        .expect("N=3/K=2 ceremony inputs must be valid");
        (identities, ceremony.build(governance))
    }

    fn configured_pop(
        producer_private: &PrivateKey,
        payout: &Address,
        oracle_ids: &[String],
        clock: Arc<ManualClock>,
    ) -> ProofOfPublishing {
        let declaration =
            GenesisOracleDeclaration::new(oracle_ids.to_vec(), 2).expect("N=3/K=2 declaration");
        let mut pop = ProofOfPublishing::new(1, 60, 1_000_000, 32);
        pop.set_producer_signer(Arc::new(EnvKeySigner::from_private_key(
            producer_private.clone(),
            "CUT_COMPOSITION_PRODUCER",
        )));
        pop.set_producer_payout_address(payout);
        pop.set_clock(clock);
        pop.set_genesis_oracle_declaration(
            declaration.encode_oracle_set(),
            declaration.oracle_quorum().to_string(),
        );
        pop
    }

    async fn apply_to_follower(
        verifier: &mut ProofOfPublishing,
        follower: &mut Blockchain,
        block: &mut wrfcoin_shared_protocol::blockchain::block::Block,
    ) {
        let recent_timestamps: Vec<i64> = follower
            .blocks()
            .values()
            .rev()
            .take(11)
            .map(|recent| recent.timestamp.timestamp())
            .collect();
        let accepted = verifier
            .verify_block_chained(
                block,
                &follower.tip,
                &follower.state.oracle_set,
                follower.state.oracle_quorum,
                &recent_timestamps,
            )
            .await
            .expect("follower verification must not error");
        assert!(
            accepted,
            "follower must validate producer block {} before applying it",
            block.height
        );
        verify_block_producer_authorized(block, &follower.state.producer_set_history).unwrap_or_else(
            |error| {
                panic!(
                    "follower must authorize producer block {} against genesis: {error}",
                    block.height
                )
            },
        );
        verifier
            .replay_received_block_for_test_support(follower, block)
            .unwrap_or_else(|error| panic!("follower must apply block {}: {error}", block.height));
    }

    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "current_thread")]
    async fn one_chain_survives_genesis_wal_reduction_and_peer_sync_2557() {
        let _env_guard = ENV_LOCK.lock().expect("environment lock");
        let active = SigningProtocolVersion::install_active(SigningProtocolVersion::V2);
        assert_eq!(
            active,
            SigningProtocolVersion::V2,
            "the genesis cut must execute under signing protocol V2"
        );

        let (governance_private, governance_public) = keypair(25_570_001);
        let genesis_time = chrono::Utc::now().timestamp();
        let clock = Arc::new(ManualClock::new(genesis_time));
        unsafe {
            std::env::set_var("WRFCOIN_CHAIN_ID", CHAIN_ID);
            std::env::set_var("WRFCOIN_NETWORK_ID", NETWORK_ID);
            std::env::set_var(ORACLE_GOVERNANCE_PUBKEYS_ENV, governance_public.to_hex());
        }

        let (producer_private, producer_public) = keypair(25_570_010);
        let (_standby_private, standby_public) = keypair(25_570_011);
        let producer = producer_member(&producer_public, "cut-producer-a");
        let standby = producer_member(&standby_public, "cut-producer-b");
        let payout = payout_of(&producer_public);

        let mut genesis = Blockchain::default();
        genesis
            .state
            .seed_genesis_producer_set(vec![producer.clone(), standby])
            .expect("genesis producer set must seed");
        genesis.state.recalculate_state_root();
        let genesis_state = genesis.state.clone();

        let (oracle_ids, registration_txs) = oracle_ceremony(&governance_private, genesis_time);
        assert_eq!(
            registration_txs.len(),
            3,
            "anti-vacuity: the final N=3 ceremony must emit three registrations"
        );

        let producer_chain = Arc::new(RwLock::new(genesis.clone()));
        let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        {
            let mut pending = mempool.write().await;
            for transaction in &registration_txs {
                pending
                    .add_transaction(transaction.clone())
                    .expect("the production mempool must admit each registration");
            }
        }

        let mut producer_pop =
            configured_pop(&producer_private, &payout, &oracle_ids, Arc::clone(&clock));
        let mut block_1 = producer_pop
            .create_next_block(
                Arc::clone(&producer_chain),
                Arc::clone(&mempool),
                &payout.to_string(),
            )
            .await
            .expect("the authorized producer must cut block 1");
        assert_eq!(
            block_1.height, 1,
            "the registration batch belongs in block 1"
        );
        assert_eq!(
            block_1
                .transactions
                .iter()
                .filter(|tx| tx.tx_type == TransactionType::OracleRegistration)
                .count(),
            3,
            "anti-vacuity: all three registrations must survive production screening"
        );

        let producer_root_at_1 = {
            let chain = producer_chain.read().await;
            assert_eq!(chain.state.oracle_set.len(), 3, "producer must seat N=3");
            assert_eq!(chain.state.oracle_quorum, 2, "producer must seat K=2");
            assert_eq!(
                chain.state.producer_set_history[0].members.len(),
                2,
                "both genesis producer keys must remain authorized"
            );
            chain.state.state_root
        };

        // The follower starts from the same genesis and accepts the exact block
        // bytes. It stays alive through the rest of the test; no later phase creates
        // a fresh chain and calls that composition.
        let mut follower = genesis.clone();
        let mut follower_pop = ProofOfPublishing::new(1, 60, 1_000_000, 32);
        follower_pop.set_clock(clock.clone());
        apply_to_follower(&mut follower_pop, &mut follower, &mut block_1).await;
        assert_eq!(
            follower.state.state_root, producer_root_at_1,
            "producer and follower must converge after block-1 registration"
        );

        // Persist the producer's exact block through the production transition
        // extractor. With an interval above every reachable height, height 1 has no
        // periodic checkpoint: recovery is the height-0 genesis anchor plus WAL.
        let db_path = std::env::temp_dir().join(format!(
            "wrfcoin-cut-composition-{}.redb",
            fastrand::u64(..)
        ));
        {
            let mut storage = Storage::open(&db_path, NEVER_CHECKPOINT).expect("storage opens");
            storage
                .persist_genesis_state(&genesis_state)
                .await
                .expect("genesis anchor persists before block 1");
            let transitions = transition_extractor::extract_transitions(
                &genesis_state,
                &block_1,
                1,
                block_1.timestamp.timestamp(),
            );
            assert!(
                !transitions.is_empty(),
                "anti-vacuity: block 1 must produce WAL transitions"
            );
            storage
                .persist_block(block_1.clone(), transitions, None)
                .await
                .expect("block 1 persists without a periodic checkpoint");
            let (checkpoint_height, _) = storage
                .load_checkpoint_at_or_before(1)
                .expect("checkpoint lookup")
                .expect("height-0 genesis anchor exists");
            assert_eq!(
                checkpoint_height, 0,
                "WAL control: no post-genesis checkpoint may hide the replay path"
            );
        }

        let recovered = {
            let storage = Storage::open(&db_path, NEVER_CHECKPOINT).expect("storage reopens");
            storage
                .load_state()
                .expect("WAL replay must not error")
                .expect("WAL replay must recover the chain")
        };
        assert_eq!(recovered.height, 1, "restart must recover block 1");
        assert_eq!(
            recovered.state.state_root, producer_root_at_1,
            "WAL-only restart must reproduce the producer state root"
        );
        assert_eq!(
            recovered.state.oracle_set.len(),
            3,
            "restart must retain N=3"
        );
        assert_eq!(recovered.state.oracle_quorum, 2, "restart must retain K=2");
        assert_eq!(
            recovered.state.producer_set_history[0].members.len(),
            2,
            "restart must retain both genesis producer keys"
        );

        // Continue the SAME recovered producer chain. The follower carries one
        // state forward, then deliberately falls behind before the reduction so
        // the final phase proves backlog catch-up rather than lockstep application.
        let producer_chain = Arc::new(RwLock::new(recovered));
        producer_pop = configured_pop(&producer_private, &payout, &oracle_ids, Arc::clone(&clock));
        let empty_mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let mut reward_before = None;
        let mut reward_after = None;
        let mut sync_backlog = Vec::new();

        for expected_height in 2..=END_HEIGHT {
            let elapsed_since_block_1 =
                elapsed_seconds_at_height(expected_height).saturating_sub(elapsed_seconds_at_height(1));
            clock.set(
                genesis_time
                    .checked_add(elapsed_since_block_1 as i64)
                    .expect("cut rehearsal time must fit in i64"),
            );
            let mut block = producer_pop
                .create_next_block(
                    Arc::clone(&producer_chain),
                    Arc::clone(&empty_mempool),
                    &payout.to_string(),
                )
                .await
                .unwrap_or_else(|error| {
                    panic!("producer must advance to height {expected_height}: {error}")
                });
            assert_eq!(
                block.height, expected_height,
                "the recovered producer must advance consecutively"
            );

            if matches!(expected_height, REDUCTION_PRE_HEIGHT | REDUCTION_HEIGHT) {
                let coinbase = block
                    .transactions
                    .iter()
                    .find(|tx| tx.is_coinbase())
                    .and_then(|tx| tx.outputs.first())
                    .map(|output| output.amount)
                    .expect("boundary block must carry a coinbase");
                if expected_height == REDUCTION_PRE_HEIGHT {
                    reward_before = Some(coinbase);
                } else {
                    reward_after = Some(coinbase);
                }
            }

            if expected_height <= FOLLOWER_PAUSE_HEIGHT {
                apply_to_follower(&mut follower_pop, &mut follower, &mut block).await;
            } else {
                sync_backlog.push(block);
            }
        }

        assert_eq!(
            follower.height, FOLLOWER_PAUSE_HEIGHT,
            "anti-vacuity: the follower must be behind before catch-up"
        );
        assert_eq!(
            sync_backlog.len() as u64,
            END_HEIGHT - FOLLOWER_PAUSE_HEIGHT,
            "anti-vacuity: catch-up must cross the reduction boundary"
        );
        for block in &mut sync_backlog {
            apply_to_follower(&mut follower_pop, &mut follower, block).await;
        }

        let before = reward_before.expect("height 42,999 reward observed");
        let after = reward_after.expect("height 43,000 reward observed");
        assert_eq!(
            emission_periods_at_height(REDUCTION_PRE_HEIGHT),
            0,
            "height 42,999 must be before the first reduction"
        );
        assert_eq!(
            emission_periods_at_height(REDUCTION_HEIGHT),
            1,
            "height 43,000 must be after the first reduction"
        );
        // ADR-003 Phase A (#2851): the coinbase draws `coinbase_allowance` (the
        // ratified sub-split of the contribution slice), not all of E(t). Read the
        // expectation from that single source of truth rather than re-deriving the
        // formula — a re-derived copy is exactly what went stale here when the
        // pre-ADR-003 `adjusted_block_reward(h) / 10` expectation outlived the
        // 2026-08-30 coinbase re-denomination.
        assert_eq!(
            before,
            follower.state.emission_for_tests().coinbase_allowance(REDUCTION_PRE_HEIGHT) as u64 / LIVENESS_BLOCK_REWARD_FRACTION,
            "the real empty block before the boundary must pay the liveness share \
             of the ADR-003 coinbase allowance"
        );
        assert_eq!(
            after,
            follower.state.emission_for_tests().coinbase_allowance(REDUCTION_HEIGHT) as u64 / LIVENESS_BLOCK_REWARD_FRACTION,
            "the real empty block after the boundary must pay the reduced liveness \
             share of the ADR-003 coinbase allowance"
        );
        assert_eq!(
            adjusted_block_reward(REDUCTION_HEIGHT),
            adjusted_block_reward(REDUCTION_PRE_HEIGHT) * EMISSION_RETAIN_NUMERATOR
                / EMISSION_RETAIN_DENOMINATOR,
            "the first wall-clock reduction must retain exactly 2/3"
        );
        assert!(
            after > 0 && after < before,
            "anti-vacuity: the observed reward must step down, not stay flat or become 0 -> 0"
        );

        let producer = producer_chain.read().await;
        assert_eq!(
            producer.height, END_HEIGHT,
            "producer reaches the end height"
        );
        assert_eq!(
            follower.height, END_HEIGHT,
            "follower sync reaches the producer height"
        );
        assert_eq!(
            follower.tip, producer.tip,
            "follower sync must end on the producer tip"
        );
        assert_eq!(
            follower.state.state_root, producer.state.state_root,
            "one carried follower must converge with the recovered producer after the reduction"
        );
        assert_eq!(
            follower.state.total_minted_supply(),
            producer.state.total_minted_supply(),
            "producer and follower must agree on cumulative emission"
        );

        unsafe {
            std::env::remove_var("WRFCOIN_CHAIN_ID");
            std::env::remove_var("WRFCOIN_NETWORK_ID");
            std::env::remove_var(ORACLE_GOVERNANCE_PUBKEYS_ENV);
        }
        let _ = std::fs::remove_file(&db_path);
    }
}
