// Integration tests for operator lifecycle (#932) and supply cap enforcement (#936).
//
// Run from $HOME/wrfcoin/core4:
//   cargo test --test operator_lifecycle_and_supply_cap -- --nocapture

use std::sync::Arc;

use rand::rngs::StdRng;
use rand::{SeedableRng, random};
use tokio::sync::RwLock;
use wrfcoin_reputation_engine::SCORE_SCALE;

use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::StateTransition;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload, OracleAttestationPayload, RevealPayload, StationRegistrationPayload,
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::hyper_local::compose_reward_with_multipliers;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    EmissionPartition, EmissionRole, INITIAL_BLOCK_REWARD, MAX_SUPPLY, ProofOfPublishing,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::station_registry::{ContributionProvenanceClass, DataQualityTier};

// ---------------------------------------------------------------------------
// Helpers (copied from reward_integration.rs — test crates can't cross-import)
// ---------------------------------------------------------------------------

fn addr(byte: u8) -> Address {
    Address([byte; 20])
}

fn oracle_identity(seed: u64) -> (PrivateKey, String) {
    let mut rng = StdRng::seed_from_u64(seed);
    let private_key = PrivateKey::generate(&mut rng);
    let oracle_id = PublicKey::from_private_key(&private_key).to_string();
    (private_key, oracle_id)
}

fn build_tx(tx_type: TransactionType, tag: &str) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("input-{tag}")),
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
    tx.tx_type = tx_type;
    tx.set_id();
    tx
}

fn registered_station_tx(
    station_id: &str,
    owner_address: &str,
    owner_key: &PrivateKey,
    governance_key: &PrivateKey,
    tag: &str,
) -> Transaction {
    let mut payload = StationRegistrationPayload::new(
        station_id,
        owner_address,
        46.7867,
        -92.1005,
        "emission-budget-fixture",
        DataQualityTier::Gold,
        ContributionProvenanceClass::Real,
    );
    payload.sign_ownership(owner_key);
    payload.sign(governance_key);
    let mut tx = build_tx(TransactionType::StationRegistration, &format!("station-{tag}"));
    tx.data = Some(serde_json::to_string(&payload).expect("serialize registration"));
    tx.set_id();
    tx
}

fn build_commit_and_reveal(
    station_id: &str,
    timestamp: i64,
    quality: f64,
    tag: &str,
) -> (Transaction, Transaction, String) {
    let payload = serde_json::json!({"quality": quality, "volume": 1.0, "uniqueness": 1.0});
    let nonce = format!("nonce-{tag}");
    let commit_id =
        Transaction::compute_commit_id(&payload, &nonce, station_id, timestamp, "contribution");

    // Commit authorization is self-certifying; reward eligibility remains
    // gated by oracle quorum. Use a deterministic contributor key so this
    // copied fixture reaches the production path that it is meant to test.
    let signer_seed = tag
        .bytes()
        .fold(51_000u64, |seed, byte| seed.saturating_add(byte as u64));
    let mut signer_rng = StdRng::seed_from_u64(signer_seed);
    let signer = PrivateKey::generate(&mut signer_rng);
    let mut commit_payload = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: "contribution".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit_payload.sign_authorization(&signer);

    let reveal_payload = RevealPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.to_string(),
        timestamp,
        contribution_type: "contribution".to_string(),
        data: payload,
        nonce,
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        manifest: None,
        verification_receipt: None,
        compute_receipt: None,
        quality_tier: None,
    };

    let mut commit_tx = build_tx(TransactionType::ContributionCommit, &format!("commit-{tag}"));
    commit_tx.data =
        Some(serde_json::to_string(&commit_payload).expect("serialize commit payload"));
    commit_tx.set_id();

    let mut reveal_tx = build_tx(TransactionType::ContributionReveal, &format!("reveal-{tag}"));
    reveal_tx.data =
        Some(serde_json::to_string(&reveal_payload).expect("serialize reveal payload"));
    reveal_tx.set_id();

    (commit_tx, reveal_tx, commit_id)
}

fn signed_attestation_tx(
    commit_id: &str,
    private_key: &PrivateKey,
    oracle_id: &str,
    timestamp: i64,
    tag: &str,
) -> Transaction {
    let mut payload = OracleAttestationPayload {
        commit_id: commit_id.to_string(),
        oracle_id: oracle_id.to_string(),
        approved: true,
        signature: None,
        confidence: Some(1.0),
        timestamp,
        nonce: Some(format!("nonce-{tag}")),
    };

    let signature = private_key
        .sign(payload.signing_message().as_bytes())
        .to_string();
    payload.signature = Some(signature);

    let mut tx = build_tx(
        TransactionType::OracleAttestation,
        &format!("att-{tag}"),
    );
    tx.data = Some(serde_json::to_string(&payload).expect("serialize attestation payload"));
    tx.set_id();
    tx
}

// ---------------------------------------------------------------------------
// #932 — Operator lifecycle: keygen → genesis → mine → earn rewards
// ---------------------------------------------------------------------------

#[tokio::test]
async fn operator_lifecycle_keypair_through_block_reward() {
    // 1. Generate operator keypair
    let mut rng = StdRng::seed_from_u64(93200 + random::<u16>() as u64);
    let operator_sk = PrivateKey::generate(&mut rng);
    let operator_pk = PublicKey::from_private_key(&operator_sk);
    let operator_address = Address::from_public_key(&operator_pk);
    let miner_hex = format!("0x{}", hex::encode(operator_address.0));

    // 2. Initialize genesis blockchain state
    let mut chain = Blockchain::default();
    chain.state.create_account(operator_address, Balance::new(0));

    // 3. Configure oracle quorum
    let (oracle_a_sk, oracle_a_id) = oracle_identity(93210 + random::<u16>() as u64);
    let (oracle_b_sk, oracle_b_id) = oracle_identity(93220 + random::<u16>() as u64);

    chain
        .apply_state_transition(StateTransition::SetOracleConfig {
            oracle_set: vec![oracle_a_id.clone(), oracle_b_id.clone()],
            oracle_quorum: 2,
        })
        .expect("set oracle quorum");

    let station_id = format!("lifecycle-station-{}", random::<u16>());
    chain
        .state
        .governance
        .params
        .contributor_tier_policy
        .trusted_provider_station_ids
        .insert(station_id.clone());

    // Seed reputation so the reward path is exercised.
    let rep_score = SCORE_SCALE * 80 / 100;
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&hex::encode(operator_address.0), rep_score);
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&oracle_a_id, rep_score);
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&oracle_b_id, rep_score);

    let blockchain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 0, 16);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    // 4. Build commit/reveal + attestation transactions
    let ts = 1_700_330_000i64;
    let (commit_tx, mut reveal_tx, commit_id) =
        build_commit_and_reveal(&station_id, ts, 1.0, "lifecycle");
    reveal_tx.sender_address = operator_address;

    let att_1 = signed_attestation_tx(
        &commit_id,
        &oracle_a_sk,
        &oracle_a_id,
        ts + 1,
        "lifecycle-a",
    );
    let att_2 = signed_attestation_tx(
        &commit_id,
        &oracle_b_sk,
        &oracle_b_id,
        ts + 2,
        "lifecycle-b",
    );

    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        mp.add_transaction(commit_tx).expect("enqueue commit");
        mp.add_transaction(reveal_tx).expect("enqueue reveal");
        mp.add_transaction_with_signing_keys(att_1, Some(&signing_keys)).expect("enqueue attestation 1");
        mp.add_transaction_with_signing_keys(att_2, Some(&signing_keys)).expect("enqueue attestation 2");
    }

    // 5. Mine a block
    let block = pop
        .create_next_block(blockchain.clone(), mempool.clone(), &miner_hex)
        .await
        .expect("create PoP block");

    assert_eq!(block.height, 1, "first block must be height 1");

    // 6. Verify operator earned a reward (coinbase at minimum)
    let chain_guard = blockchain.read().await;
    let account = chain_guard
        .state
        .get_account(&operator_address)
        .expect("operator account must exist after block");
    assert!(
        account.balance > Balance::new(0),
        "operator must earn block reward; balance = {}",
        account.balance
    );

    // 7. Verify state root is deterministic
    drop(chain_guard);
    let mut chain_mut = blockchain.write().await;
    let root_before = chain_mut.state.state_root;
    chain_mut.state.recalculate_state_root();
    assert_eq!(
        root_before, chain_mut.state.state_root,
        "state root must be stable after recalculation"
    );

    // 8. Verify supply accounting
    let total_supply = chain_mut.state.total_minted_supply();
    assert!(
        total_supply > 0,
        "total minted supply must be positive after block production"
    );

    println!(
        "[#932 PASS] operator lifecycle validated: balance={} supply={}",
        chain_mut.state.get_account(&operator_address).unwrap().balance,
        total_supply
    );
}

// ---------------------------------------------------------------------------
// #936 — Supply cap: emission stops near MAX_SUPPLY
// ---------------------------------------------------------------------------

#[tokio::test]
async fn supply_cap_emission_stops_near_max_supply() {
    let headroom: u128 = 500;
    let near_cap_seed = MAX_SUPPLY.saturating_sub(headroom);

    let mut chain = Blockchain::default();
    chain.state.create_account(addr(1), Balance::new(near_cap_seed));

    let blockchain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let mut pop = ProofOfPublishing::new(1, 60, 1_000_000, 10);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    let initial_supply = near_cap_seed;

    for i in 0..3 {
        let _block = pop
            .create_next_block(
                blockchain.clone(),
                mempool.clone(),
                "0x1111111111111111111111111111111111111111",
            )
            .await
            .expect("block creation must succeed");

        let total = blockchain.read().await.state.total_minted_supply();
        assert!(
            total <= MAX_SUPPLY,
            "total supply must stay <= MAX_SUPPLY after block {}: {} > {}",
            i + 1,
            total,
            MAX_SUPPLY
        );
    }

    let final_supply = blockchain.read().await.state.total_minted_supply();
    let minted_delta = final_supply.saturating_sub(initial_supply);
    assert!(
        minted_delta <= headroom,
        "total minted delta must not exceed remaining headroom: {} > {}",
        minted_delta,
        headroom
    );

    println!(
        "[#936 PASS] supply cap near-max: initial={} final={} delta={} headroom={}",
        initial_supply, final_supply, minted_delta, headroom
    );
}

// ---------------------------------------------------------------------------
// #936 — Emission budget limiting via constrained remaining supply
// ---------------------------------------------------------------------------

/// Tests that the emission cap enforcement (lines 1829-1891 of proof_of_publishing.rs)
/// correctly engages proportional reward scaling when remaining supply is tight.
///
/// Strategy: pre-fund to near MAX_SUPPLY so `remaining_supply` is small (~3000 base units),
/// then submit enough contributions that nominal rewards exceed the remaining budget.
/// This triggers `budget_limited = true` and proportional scaling.
#[tokio::test]
async fn emission_budget_limits_rewards_when_supply_constrained() {
    // MUST NOT collide with the contributor range addr(10..=129) — see the
    // disjointness assertion below (#2484).
    const MINER_HEX: &str = "0xf0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0";

    let (oracle_a_sk, oracle_a_id) = oracle_identity(93610 + random::<u16>() as u64);
    let (oracle_b_sk, oracle_b_id) = oracle_identity(93620 + random::<u16>() as u64);

    let mut chain = Blockchain::default();
    chain
        .apply_state_transition(StateTransition::SetOracleConfig {
            oracle_set: vec![oracle_a_id.clone(), oracle_b_id.clone()],
            oracle_quorum: 2,
        })
        .expect("set oracle quorum");

    // Leave the live coinbases plus only 3_000 base units for contributors.
    // A bare 3_000 was silently exhausted by the coinbase after denomination
    // rescaling, so the proportional branch paid 0/120 and this test looked at
    // no contributor reward at all. Derive from the live schedule anchor: a
    // larger denomination change cannot recreate that vacuous fixture.
    //
    // ADR-003 (#2846) moves that anchor. The two blocks this test produces no
    // longer mint `INITIAL_BLOCK_REWARD` each — the coinbase draws the PRODUCER
    // SLICE of `E(t)`. Leaving `2 * INITIAL_BLOCK_REWARD` of headroom would
    // therefore leave ~144e9 base units unspendable by a chain that can only
    // mint `E(t)` per block, `remaining_supply` would stop binding, and this
    // test would go vacuous in exactly the direction #2484 exists to prevent.
    // Re-anchor on the slice the coinbase actually draws, keeping the 3_000
    // contributor allowance — and therefore the binding constraint — intact.
    let coinbase_per_block = chain.state.emission_for_tests().coinbase_allowance(1);
    assert_eq!(
        coinbase_per_block,
        chain.state.emission_for_tests().coinbase_allowance(2),
        "both blocks must sit in one block-time tier for a single-coinbase \
         headroom to be exact (#2846)"
    );
    assert!(
        coinbase_per_block < INITIAL_BLOCK_REWARD as u128,
        "the coinbase must be a strict fraction of E(t), or the partition is not \
         in force and this fixture is measuring the old ceiling"
    );
    assert!(
        coinbase_per_block
            < EmissionPartition::at_height(1, None).slice(EmissionRole::Contribution),
        "the coinbase must leave room in the contribution slice for the \
         contribution rewards this test exists to measure (#2846)"
    );
    let headroom: u128 = 2 * coinbase_per_block + 3_000;
    chain
        .state
        .create_account(addr(1), Balance::new(MAX_SUPPLY.saturating_sub(headroom)));

    let rep_score = SCORE_SCALE * 90 / 100;
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&oracle_a_id, rep_score);
    chain
        .state
        .oracle_registry_mut()
        .set_reputation_score(&oracle_b_id, rep_score);

    let station_count = 120;
    let ts = 1_700_400_000i64;
    let mut governance_rng = StdRng::seed_from_u64(2542);
    let governance_key = PrivateKey::generate(&mut governance_rng);
    std::env::set_var(
        "WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS",
        PublicKey::from_private_key(&governance_key).to_hex(),
    );
    let mut contributor_addresses = Vec::new();
    let mut all_txs: Vec<Transaction> = Vec::new();
    let mut station_txs: Vec<Transaction> = Vec::new();

    for i in 0..station_count {
        let contributor = addr((i + 10) as u8);
        contributor_addresses.push(contributor);
        chain.state.create_account(contributor, Balance::new(0));

        let station_id = format!("cap-station-{}-{}", i, random::<u16>());
        let mut owner_rng = StdRng::seed_from_u64(25_420 + i as u64);
        let owner_key = PrivateKey::generate(&mut owner_rng);

        chain
            .state
            .oracle_registry_mut()
            .set_reputation_score(&hex::encode(contributor.0), rep_score);

        let tag = format!("cap-{i}");
        let (commit_tx, mut reveal_tx, commit_id) =
            build_commit_and_reveal(&station_id, ts + i as i64, 1.0, &tag);
        reveal_tx.sender_address = contributor;
        // #2484: without a fee (or a valid compute receipt) the producer
        // self-screen (#1730) drops EVERY reveal as
        // "zero-fee weather tx without valid compute receipt", so no
        // contribution reward is ever computed and the budget-limiting branch
        // this test exists for is unreachable. Measured: 120 reveals in, 0 in
        // the block, `oracle_reward_revealed_commit_count = 0`.
        reveal_tx.fee = 1;

        let att_1 = signed_attestation_tx(
            &commit_id,
            &oracle_a_sk,
            &oracle_a_id,
            ts + i as i64 + 1,
            &format!("{tag}-a"),
        );
        let att_2 = signed_attestation_tx(
            &commit_id,
            &oracle_b_sk,
            &oracle_b_id,
            ts + i as i64 + 2,
            &format!("{tag}-b"),
        );

        station_txs.push(registered_station_tx(
            &station_id,
            &contributor.to_string(),
            &owner_key,
            &governance_key,
            &tag,
        ));
        all_txs.push(commit_tx);
        all_txs.push(reveal_tx);
        all_txs.push(att_1);
        all_txs.push(att_2);
    }

    let blockchain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

    // Mine the production registration transition first. The reward processor
    // reads pre-block state, so co-locating registrations with reveals would
    // reproduce the old manufactured-eligibility fixture rather than proving
    // a registered station was already canonical.
    let mut pop = ProofOfPublishing::new(1, 60, 0, (station_count * 5 + 1) as usize);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        for tx in station_txs {
            mp.add_transaction_with_signing_keys(tx, Some(&signing_keys))
                .expect("enqueue registration");
        }
    }
    pop.create_next_block(blockchain.clone(), mempool.clone(), MINER_HEX)
        .await
        .expect("mine station registrations before contribution rewards");
    {
        let mut mp = mempool.write().await;
        let signing_keys = { blockchain.read().await.state.signing_keys.clone() };
        for tx in all_txs {
            mp.add_transaction_with_signing_keys(tx, Some(&signing_keys))
                .expect("enqueue contribution tx");
        }
    }

    let block = pop
        .create_next_block(
            blockchain.clone(),
            mempool.clone(),
            MINER_HEX,
        )
        .await
        .expect("create PoP block with many contributors near supply cap");

    let chain_guard = blockchain.read().await;
    let total_supply = chain_guard.state.total_minted_supply();

    // Critical invariant: total supply must never exceed MAX_SUPPLY.
    assert!(
        total_supply <= MAX_SUPPLY,
        "total supply must stay <= MAX_SUPPLY: {} > {}",
        total_supply,
        MAX_SUPPLY
    );

    // Check if budget limiting engaged (block metadata).
    let budget_limited = block
        .metadata
        .get("rewards_budget_limited")
        .map(|v| v == "true")
        .unwrap_or(false);

    // Count how many contributors actually received rewards.
    let mut rewarded_count = 0u32;
    let mut total_contributor_rewards: u128 = 0;
    for contributor in &contributor_addresses {
        if let Some(account) = chain_guard.state.get_account(contributor) {
            if account.balance > Balance::new(0) {
                rewarded_count += 1;
                total_contributor_rewards += account.balance.into_inner();
            }
        }
    }

    // The total contributor rewards + coinbase must not exceed the headroom.
    let minted_delta = total_supply.saturating_sub(MAX_SUPPLY.saturating_sub(headroom));
    assert!(
        minted_delta <= headroom,
        "minted delta must not exceed headroom: {} > {}",
        minted_delta,
        headroom
    );

    // Fixture invariant: the miner must be disjoint from the contributor set,
    // or its coinbase is counted as a contributor payout. That collision is
    // what produced the phantom "rewarded=1/120" — the miner was 0x2222…22,
    // every byte 0x22 = 34 = addr(34) = contributor i=24.
    let miner_address = Address(
        <[u8; 20]>::try_from(hex::decode(MINER_HEX.trim_start_matches("0x")).expect("miner hex"))
            .expect("miner address is 20 bytes"),
    );
    assert!(
        !contributor_addresses.contains(&miner_address),
        "miner {miner_address:?} is inside the contributor address range; its \
         coinbase would be double-counted as a contribution reward (#2484)"
    );

    // ── #2484 acceptance bar ────────────────────────────────────────────
    // This test asserted only `total_supply <= MAX_SUPPLY` and
    // `minted_delta <= headroom`, both of which held trivially while the
    // proportional-scaling branch was never entered: 119 of 120 reveals were
    // dropped by the producer self-screen, the surviving "reward" was the
    // coinbase landing on a contributor-range address, and `budget_limited`
    // was false. The assertions below are what make the test evidence.
    assert!(
        budget_limited,
        "the proportional-scaling branch was not entered — nominal rewards          ({rewarded_count} rewarded) never exceeded the remaining budget, so          this test proves nothing about budget limiting (#2484)"
    );
    assert_eq!(
        rewarded_count as usize, station_count,
        "every contributor with a quorum-attested reveal must be rewarded;          {rewarded_count} of {station_count} were. A collapse to 1 is the          original #2484 shape (self-screen dropping zero-fee reveals)"
    );
    assert!(
        total_contributor_rewards > 0,
        "contributors must actually receive the scaled rewards"
    );
    // The cap binds EXACTLY: scaling consumes the headroom without exceeding
    // it. This is the property the whole budget-limiter exists to provide.
    assert_eq!(
        total_supply, MAX_SUPPLY,
        "budget-limited emission must land exactly on MAX_SUPPLY, not under          (leaving headroom unused) and never over"
    );

    println!(
        "[#936 PASS] emission budget limiting: total_supply={} budget_limited={} rewarded={}/{} contributor_rewards={} minted_delta={} headroom={}",
        total_supply, budget_limited, rewarded_count, station_count, total_contributor_rewards, minted_delta, headroom
    );
}

// ---------------------------------------------------------------------------
// #936 — MAX_COMBINED_MULTIPLIER_BPS caps compose_reward_with_multipliers()
// ---------------------------------------------------------------------------

#[test]
fn max_combined_multiplier_bps_caps_compose_reward() {
    let base: u128 = 1_000_000_000; // 10 WRF

    // Case 1: Exceeds cap — 18000 * 15000 / 100 = 2_700_000 > 20_000 → capped to 20_000
    let capped = compose_reward_with_multipliers(base, 18_000, 15_000);
    assert_eq!(
        capped,
        base * 20_000 / 10_000,
        "combined multiplier must be capped at 2.0x: got {}",
        capped
    );

    // Case 2: Below cap — 150 * 12_000 / 100 = 18_000 < 20_000
    let uncapped = compose_reward_with_multipliers(base, 150, 12_000);
    assert_eq!(
        uncapped,
        base * 18_000 / 10_000,
        "below-cap multiplier must apply directly: got {}",
        uncapped
    );

    // Case 3: Exactly at cap — 200 * 10_000 / 100 = 20_000
    let at_cap = compose_reward_with_multipliers(base, 200, 10_000);
    assert_eq!(
        at_cap,
        base * 20_000 / 10_000,
        "exactly-at-cap must yield 2.0x: got {}",
        at_cap
    );

    // Case 4: Baseline (no bonus) — 100 * 10_000 / 100 = 10_000
    let baseline = compose_reward_with_multipliers(base, 100, 10_000);
    assert_eq!(
        baseline, base,
        "baseline (1.0x) must return base reward unchanged: got {}",
        baseline
    );

    // Case 5: Zero base reward
    let zero = compose_reward_with_multipliers(0, 18_000, 15_000);
    assert_eq!(zero, 0, "zero base reward must yield zero: got {}", zero);

    println!("[#936 PASS] combined multiplier cap enforcement validated");
}
