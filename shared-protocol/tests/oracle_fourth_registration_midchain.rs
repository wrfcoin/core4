//! Pre-live gate: a FOURTH oracle registered mid-chain on a chain whose
//! genesis declared N=3/K=2 (#2055 Option A; Mango third-attester plan,
//! docs/plans/2026-09-24-mango-third-attester-plan.md in the workspace).
//!
//! The live testnet (chain 777) was cut with `oracle_set = {O1, O2, O3}`,
//! `oracle_quorum = 2`, epoch-1 registrations in block 1. O1 is cold, so the
//! chain halts whenever one of the two online attesters drops. The proposed
//! fix is one governance-signed batch of `OracleRegistration` transactions at
//! epoch 2 that names the absolute set `{O1, O2, O3, M}` with quorum **2**,
//! where `M` is a new SoftHSM key on a third host. That path — a registration
//! ABOVE height 1 — has producer/applier/WAL parity tests but has never run
//! against a chain whose block 1 declared a different set, and never live.
//! This target rehearses exactly that batch on a real block pipeline:
//! `create_next_block` on a producer, `verify_block_chained` +
//! `replay_received_block_for_test_support` on a follower that accepts the
//! producer's exact block bytes, and a second node that re-syncs every block
//! from genesis.
//!
//! What it proves:
//!
//! * (a) the epoch-2 batch is mined above height 1, every peer accepts it, and
//!   producer / live follower / from-genesis replay converge on one state root
//!   with `oracle_set.len() == 4`, `oracle_quorum == 2`;
//! * (b) after registration, attestations from 2 of the 4 identities (one of
//!   them the new key) pay a real contribution, and 1 of 4 does not — and the
//!   new key's attestation is refused at admission BEFORE the registration;
//! * (c) replaying the registration (exact bytes, and a re-signed batch at the
//!   same epoch) changes nothing;
//! * (d) the producer keeps producing blocks the follower accepts while the
//!   replayed registrations sit in its mempool;
//! * (e) the dynamic low-quality override quorum becomes
//!   `compute_oracle_quorum(4) == 3` — the same value N=3 already had — while
//!   the reward quorum stays at the declared 2;
//!
//! plus a control: the SAME batch declaring quorum 3 leaves two live
//! attesters unable to pay anything, which is the "never declare quorum 3"
//! rule of the plan, executed.
//!
//! Run:
//!   cargo test -p wrfcoin-shared-protocol --test
//! oracle_fourth_registration_midchain -- --nocapture

use std::collections::BTreeMap;
use std::sync::atomic::{
    AtomicI64,
    Ordering,
};
use std::sync::{
    Arc,
    Mutex,
    MutexGuard,
};

use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::RwLock;
use wrfcoin_reputation_engine::SCORE_SCALE;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CommitPayload,
    OracleAttestationPayload,
    RevealPayload,
    Transaction,
    TransactionInput,
    TransactionOutput,
    TransactionType,
};
use wrfcoin_shared_protocol::common::constants::compute_oracle_quorum;
use wrfcoin_shared_protocol::common::types::{
    Address,
    Balance,
};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
    Clock,
    ProofOfPublishing,
    elapsed_seconds_at_height,
};
use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;
use wrfcoin_shared_protocol::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV;
use wrfcoin_shared_protocol::crypto::genesis_oracle::GenesisOracleDeclaration;
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{
    PrivateKey,
    PublicKey,
};
use wrfcoin_shared_protocol::mempool::pool::Mempool;
use wrfcoin_shared_protocol::oracle::ceremony::GenesisOracleCeremony;
use wrfcoin_shared_protocol::security::key_registry::SigningRole;
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass,
    DataQualityTier,
    StationLocation,
    WeatherStation,
};

const CHAIN_ID: &str = "777";
const NETWORK_ID: &str = "testnet";
const MINER: &str = "0xdeadbeef000000000000000000000000000000e7";
const STATION: &str = "PWS-MIDCHAIN-ORACLE-01";

const GENESIS_EPOCH: u64 = 1;
const GENESIS_ACTIVATED_AT: i64 = 1_789_106_718; // live epoch-1 activated_at
const MIDCHAIN_EPOCH: u64 = 2; // first free epoch on the live registry

// Seeds: O1..O3 stand in for the live genesis oracles, M for the Mango key.
const SEED_GOVERNANCE: u64 = 33_000_001;
const SEED_O1: u64 = 33_000_011;
const SEED_O2: u64 = 33_000_012;
const SEED_O3: u64 = 33_000_013;
const SEED_MANGO: u64 = 33_000_014;
const SEED_CONTRIBUTOR: u64 = 33_000_021;

/// Every test mutates process-wide env (`setenv` races at the libc level) and
/// the process-wide signing-protocol version, so they are serialised.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug)]
struct ManualClock {
    seconds: AtomicI64,
}

impl Clock for ManualClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(self.seconds.load(Ordering::SeqCst), 0)
            .expect("manual clock timestamp must be representable")
    }
}

fn keypair(seed: u64) -> (PrivateKey, String) {
    let sk = PrivateKey::generate(&mut StdRng::seed_from_u64(seed));
    let hex = PublicKey::from_private_key(&sk).to_hex();
    (sk, hex)
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

/// Commit + reveal for one reading. `quality` < 0.5 puts it in the Low band,
/// which the dynamic (BFT) override quorum governs; 1.0 passes the quality
/// gate on its own, so only the static reward quorum decides its payment.
fn commit_and_reveal(
    timestamp: i64,
    quality: f64,
    sender: Address,
    tag: &str,
) -> (Transaction, Transaction, String) {
    let payload = serde_json::json!({
        "quality": quality,
        "volume": 1.0,
        "uniqueness": 1.0,
        "temperature_c": 21.5,
        "humidity_percent": 60.0,
        "pressure_hpa": 1014.0,
        "wind_speed_ms": 2.8,
        "precipitation_mm": 0.0,
        "dewpoint_c": 13.5
    });
    let nonce = format!("nonce-{tag}");
    let commit_id =
        Transaction::compute_commit_id(&payload, &nonce, STATION, timestamp, "contribution");

    let seed = tag
        .bytes()
        .fold(77u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    let commit_key = PrivateKey::generate(&mut StdRng::seed_from_u64(seed));
    let mut commit_payload = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: STATION.to_string(),
        timestamp,
        contribution_type: "contribution".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit_payload.sign_authorization(&commit_key);
    let reveal_payload = RevealPayload {
        commit_id: commit_id.clone(),
        station_id: STATION.to_string(),
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

    let mut commit = build_tx(
        TransactionType::ContributionCommit,
        &format!("commit-{tag}"),
    );
    commit.data = Some(serde_json::to_string(&commit_payload).expect("commit payload"));
    commit.set_id();

    let mut reveal = build_tx(
        TransactionType::ContributionReveal,
        &format!("reveal-{tag}"),
    );
    reveal.data = Some(serde_json::to_string(&reveal_payload).expect("reveal payload"));
    reveal.sender_address = sender;
    // Non-zero fee: keeps the reveal out of the fee-less compute-receipt gate,
    // which is orthogonal to the oracle quorum under test.
    reveal.fee = 1;
    reveal.set_id();

    (commit, reveal, commit_id)
}

fn attestation(
    commit_id: &str,
    oracle: &(PrivateKey, String),
    timestamp: i64,
    tag: &str,
) -> Transaction {
    let mut payload = OracleAttestationPayload {
        commit_id: commit_id.to_string(),
        oracle_id: oracle.1.clone(),
        approved: true,
        signature: None,
        confidence: Some(1.0),
        timestamp,
        nonce: Some(format!("nonce-{tag}")),
    };
    payload.signature = Some(
        oracle
            .0
            .sign(payload.signing_message().as_bytes())
            .to_string(),
    );
    let mut tx = build_tx(TransactionType::OracleAttestation, &format!("att-{tag}"));
    tx.data = Some(serde_json::to_string(&payload).expect("attestation payload"));
    tx.set_id();
    tx
}

/// Transactions carry a wall-clock `timestamp` that block assembly measures
/// against the block's own time (the submission window). This harness runs
/// the chain on the genesis block-time schedule, so stamp each transaction
/// with the time of the block it is meant for.
fn stamped(mut tx: Transaction, at: i64) -> Transaction {
    tx.timestamp = chrono::DateTime::from_timestamp(at, 0).expect("representable timestamp");
    tx.set_id();
    tx
}

fn metadata_u64(block: &Block, key: &str) -> u64 {
    block
        .metadata
        .get(key)
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or_else(|| panic!("block {} metadata must carry numeric {key}", block.height))
}

fn count(block: &Block, tx_type: TransactionType) -> usize {
    block
        .transactions
        .iter()
        .filter(|tx| tx.tx_type == tx_type)
        .count()
}

/// A registration batch naming `set` / `quorum` at `epoch`, one transaction
/// per identity, built by the same ceremony code the operator CLI runs.
fn registration_batch(
    governance: &PrivateKey,
    set: &[String],
    quorum: usize,
    epoch: u64,
    activated_at: i64,
    submitted_at: i64,
) -> Vec<Transaction> {
    let signing: BTreeMap<String, String> = set.iter().map(|id| (id.clone(), id.clone())).collect();
    GenesisOracleCeremony::new(
        CHAIN_ID,
        NETWORK_ID,
        set.to_vec(),
        quorum,
        signing,
        epoch,
        activated_at,
        submitted_at,
    )
    .expect("ceremony inputs must be valid")
    .build(governance)
}

/// One producer, one live follower, and the history needed to re-sync a
/// third node from genesis.
struct Net {
    genesis: Blockchain,
    clock: Arc<ManualClock>,
    genesis_time: i64,
    producer: Arc<RwLock<Blockchain>>,
    mempool: Arc<RwLock<Mempool>>,
    producer_pop: ProofOfPublishing,
    follower: Blockchain,
    follower_pop: ProofOfPublishing,
    history: Vec<Block>,
    governance: PrivateKey,
    genesis_oracles: Vec<(PrivateKey, String)>,
    mango: (PrivateKey, String),
    contributor: Address,
    /// `None`: stamp blocks on the genesis block-time schedule. `Some(secs)`:
    /// stamp each block `secs` after the previous one — the live chain mines
    /// on the collectors' cadence, not on the schedule.
    cadence: Option<i64>,
}

impl Net {
    /// Genesis with the live shape: N=3/K=2 declared, the three epoch-1
    /// registrations mined in block 1 by the producer and accepted by the
    /// follower.
    async fn cut() -> Self {
        let active = SigningProtocolVersion::install_active(SigningProtocolVersion::V2);
        assert_eq!(active, SigningProtocolVersion::V2);

        let (governance, governance_pub) = keypair(SEED_GOVERNANCE);
        unsafe {
            std::env::set_var("WRFCOIN_CHAIN_ID", CHAIN_ID);
            std::env::set_var("WRFCOIN_NETWORK_ID", NETWORK_ID);
            std::env::set_var(ORACLE_GOVERNANCE_PUBKEYS_ENV, &governance_pub);
        }

        let genesis_oracles = vec![keypair(SEED_O1), keypair(SEED_O2), keypair(SEED_O3)];
        let mango = keypair(SEED_MANGO);
        let genesis_ids: Vec<String> = genesis_oracles.iter().map(|o| o.1.clone()).collect();

        let contributor_sk = PrivateKey::generate(&mut StdRng::seed_from_u64(SEED_CONTRIBUTOR));
        let contributor = Address::from_public_key(&PublicKey::from_private_key(&contributor_sk));

        let mut genesis = Blockchain::default();
        genesis.state.create_account(contributor, Balance::new(0));
        genesis
            .state
            .register_weather_station(WeatherStation {
                station_id: STATION.to_string(),
                owner_address: contributor.to_string(),
                location: StationLocation {
                    latitude: 46.7867,
                    longitude: -92.1005,
                    elevation_meters: Some(215.0),
                },
                hardware_model: "midchain-oracle-rehearsal".to_string(),
                calibration_date: None,
                is_active: true,
                reputation_score: 50,
                data_quality_tier: DataQualityTier::Gold,
                provenance_class: ContributionProvenanceClass::Real,
                ring_public_key: None,
            })
            .expect("register a real station");
        genesis
            .state
            .governance
            .params
            .contributor_tier_policy
            .trusted_provider_station_ids
            .insert(STATION.to_string());
        genesis
            .state
            .oracle_registry_mut()
            .set_reputation_score(&hex::encode(contributor.0), SCORE_SCALE * 85 / 100);
        genesis.state.recalculate_state_root();

        let genesis_time = chrono::Utc::now().timestamp();
        let clock = Arc::new(ManualClock {
            seconds: AtomicI64::new(genesis_time),
        });

        let declaration =
            GenesisOracleDeclaration::new(genesis_ids.clone(), 2).expect("N=3/K=2 declaration");
        let mut producer_pop = ProofOfPublishing::new(1, 60, 0, 32);
        producer_pop.set_clock(clock.clone());
        producer_pop.set_genesis_oracle_declaration(
            declaration.encode_oracle_set(),
            declaration.oracle_quorum().to_string(),
        );
        // #3361: chain 777 funds a faucet slice, so its block 1 must name one.
        producer_pop.set_genesis_faucet_address(Address([0xFA; 20]));
        let mut follower_pop = ProofOfPublishing::new(1, 60, 0, 32);
        follower_pop.set_clock(clock.clone());

        let mut net = Net {
            producer: Arc::new(RwLock::new(genesis.clone())),
            follower: genesis.clone(),
            genesis,
            clock,
            genesis_time,
            mempool: Arc::new(RwLock::new(Mempool::new(&Config::default()))),
            producer_pop,
            follower_pop,
            history: Vec::new(),
            governance,
            genesis_oracles,
            mango,
            contributor,
            cadence: None,
        };

        let block_1_regs = registration_batch(
            &net.governance,
            &genesis_ids,
            2,
            GENESIS_EPOCH,
            GENESIS_ACTIVATED_AT,
            genesis_time,
        );
        net.admit(&block_1_regs).await;
        let block_1 = net.mine().await;
        assert_eq!(block_1.height, 1);
        assert_eq!(
            count(&block_1, TransactionType::OracleRegistration),
            3,
            "anti-vacuity: block 1 must seat the three genesis oracles"
        );
        {
            let chain = net.producer.read().await;
            assert_eq!(chain.state.oracle_set.len(), 3);
            assert_eq!(chain.state.oracle_quorum, 2);
        }
        net
    }

    fn ids(&self) -> Vec<String> {
        self.genesis_oracles.iter().map(|o| o.1.clone()).collect()
    }

    fn four_ids(&self) -> Vec<String> {
        let mut ids = self.ids();
        ids.push(self.mango.1.clone());
        ids
    }

    /// Wall-clock time the producer will stamp on the NEXT block.
    async fn next_block_time(&self) -> i64 {
        if let Some(secs) = self.cadence {
            let chain = self.producer.read().await;
            let last = chain
                .blocks()
                .values()
                .next_back()
                .map(|b| b.timestamp.timestamp())
                .unwrap_or(self.genesis_time);
            return last + secs;
        }
        let next = self.producer.read().await.height + 1;
        let since_block_1 =
            elapsed_seconds_at_height(next).saturating_sub(elapsed_seconds_at_height(1));
        self.genesis_time + since_block_1 as i64
    }

    async fn admit(&self, txs: &[Transaction]) {
        let mut mempool = self.mempool.write().await;
        for tx in txs {
            mempool
                .add_transaction(tx.clone())
                .unwrap_or_else(|e| panic!("mempool must admit {:?}: {e}", tx.tx_type));
        }
    }

    /// Admission the way the attestation route does it: against the chain's
    /// signing-key registry (fail-closed on unregistered identities).
    async fn admit_attestation(&self, tx: Transaction) -> anyhow::Result<()> {
        let keys = self.producer.read().await.state.signing_keys.clone();
        self.mempool
            .write()
            .await
            .add_transaction_with_signing_keys(tx, Some(&keys))
    }

    /// Produce one block, have the follower validate and apply the exact
    /// bytes, and require the two state roots to agree.
    async fn mine(&mut self) -> Block {
        let at = self.next_block_time().await;
        self.clock.seconds.store(at, Ordering::SeqCst);
        let block = self
            .producer_pop
            .create_next_block(self.producer.clone(), self.mempool.clone(), MINER)
            .await
            .expect("the producer must not wedge: create_next_block must succeed");

        let mut received = block.clone();
        let recent: Vec<i64> = self
            .follower
            .blocks()
            .values()
            .rev()
            .take(11)
            .map(|b| b.timestamp.timestamp())
            .collect();
        // Mirror the production validator (`PopConsensusAdapter::validate_block`
        // in consensus-engines): the verifier learns the oracle signing keys
        // and the oracle set from the follower's OWN state, never from the
        // producer.
        self.follower_pop
            .set_oracle_signing_keys(self.follower.state.signing_keys.clone());
        let accepted = self
            .follower_pop
            .verify_block_chained(
                &received,
                &self.follower.tip,
                self.follower.state.oracle_registry().oracle_set(),
                self.follower.state.oracle_quorum,
                &recent,
            )
            .await
            .expect("follower verification must not error");
        assert!(
            accepted,
            "follower must accept producer block {}",
            block.height
        );
        self.follower_pop
            .replay_received_block_for_test_support(&mut self.follower, &mut received)
            .unwrap_or_else(|e| panic!("follower must apply block {}: {e}", block.height));

        let producer_root = self.producer.read().await.state.state_root;
        assert_eq!(
            self.follower.state.state_root, producer_root,
            "producer and follower state roots diverged at height {}",
            block.height
        );
        self.history.push(block.clone());
        block
    }

    /// A third node that was offline the whole time syncs every block from
    /// genesis and must land on the producer's root.
    async fn resync_from_genesis(&self) {
        let mut node = self.genesis.clone();
        let mut pop = ProofOfPublishing::new(1, 60, 0, 32);
        pop.set_clock(self.clock.clone());
        for block in &self.history {
            let mut received = block.clone();
            pop.replay_received_block_for_test_support(&mut node, &mut received)
                .unwrap_or_else(|e| {
                    panic!("genesis re-sync must apply block {}: {e}", block.height)
                });
        }
        let producer = self.producer.read().await;
        assert_eq!(node.height, producer.height);
        assert_eq!(
            node.state.state_root, producer.state.state_root,
            "a node re-syncing from genesis must reach the producer's root"
        );
        assert_eq!(node.state.oracle_set, producer.state.oracle_set);
        assert_eq!(node.state.oracle_quorum, producer.state.oracle_quorum);
    }

    /// Mine the epoch-2 batch that seats `M` with the given quorum.
    async fn register_mango(&mut self, quorum: usize) -> (Block, Vec<Transaction>) {
        let at = self.next_block_time().await;
        let batch = registration_batch(
            &self.governance,
            &self.four_ids(),
            quorum,
            MIDCHAIN_EPOCH,
            at,
            at,
        );
        assert_eq!(
            batch.len(),
            4,
            "one registration per identity in the final set"
        );
        self.admit(&batch).await;
        let block = self.mine().await;
        assert!(
            block.height > 1,
            "this rehearsal is about registrations ABOVE height 1"
        );
        assert_eq!(
            count(&block, TransactionType::OracleRegistration),
            4,
            "anti-vacuity: the whole epoch-2 batch must survive producer screening"
        );
        (block, batch)
    }

    /// Commit in one block, then reveal plus `attesters`' attestations in the
    /// next. Returns the reveal block and the commit id.
    async fn contribute(
        &mut self,
        quality: f64,
        attesters: &[(PrivateKey, String)],
        tag: &str,
    ) -> (Block, String) {
        let commit_at = self.next_block_time().await;
        let reading_at = commit_at - 30;
        let (commit, reveal, commit_id) =
            commit_and_reveal(reading_at, quality, self.contributor, tag);
        self.admit(&[stamped(commit, commit_at)]).await;
        let commit_block = self.mine().await;
        assert_eq!(count(&commit_block, TransactionType::ContributionCommit), 1);

        let reveal_at = self.next_block_time().await;
        self.admit(&[stamped(reveal, reveal_at)]).await;
        let attest_at = reveal_at - 5;
        for (i, oracle) in attesters.iter().enumerate() {
            self.admit_attestation(stamped(
                attestation(
                    &commit_id,
                    oracle,
                    attest_at + i as i64,
                    &format!("{tag}-{i}"),
                ),
                reveal_at,
            ))
            .await
            .unwrap_or_else(|e| panic!("attestation {i} for {tag} must be admitted: {e}"));
        }
        let reveal_block = self.mine().await;
        (reveal_block, commit_id)
    }

    /// Mine empty blocks until the producer's tip is `height`.
    async fn advance_to(&mut self, height: u64) {
        while self.producer.read().await.height < height {
            self.mine().await;
        }
    }

    async fn earned(&self) -> u128 {
        self.producer
            .read()
            .await
            .state
            .get_account(&self.contributor)
            .map(|a| a.earned_balance_total.into_inner())
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// (a) peer acceptance and state-root parity
// ---------------------------------------------------------------------------

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "current_thread")]
async fn a_midchain_fourth_oracle_is_accepted_by_peers_and_replays_to_one_root() {
    let _env = lock_env();
    let mut net = Net::cut().await;
    net.mine().await; // an ordinary block between genesis and the registration

    let (block, _) = net.register_mango(2).await;
    assert!(block.height >= 3);

    let chain = net.producer.read().await;
    assert_eq!(chain.state.oracle_set.len(), 4, "M must be seated");
    assert!(chain.state.oracle_set.contains(&net.mango.1));
    assert_eq!(
        chain.state.oracle_quorum, 2,
        "the declared quorum must stay 2"
    );
    for id in net.four_ids() {
        let epochs: Vec<u64> = chain
            .state
            .signing_keys
            .active_keys_at(
                &id,
                SigningRole::Oracle,
                chrono::Utc::now().timestamp() + 1_000_000,
            )
            .into_iter()
            .map(|(epoch, key)| {
                assert_eq!(
                    key, id,
                    "identity-as-signing-key: every epoch key is the identity"
                );
                epoch
            })
            .collect();
        assert!(
            epochs.contains(&MIDCHAIN_EPOCH),
            "{id} must carry an epoch-{MIDCHAIN_EPOCH} key, got {epochs:?}"
        );
    }
    assert!(
        chain
            .state
            .signing_keys
            .has_identity_role(&net.mango.1, SigningRole::Oracle),
        "M must be an explicitly registered oracle operator"
    );
    drop(chain);

    // Blocks after the registration keep converging, and a node that slept
    // through all of it re-syncs to the same root.
    net.mine().await;
    net.resync_from_genesis().await;
}

// ---------------------------------------------------------------------------
// (b) 2-of-4 pays, 1-of-4 does not
// ---------------------------------------------------------------------------

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "current_thread")]
async fn two_of_four_attestations_pay_and_one_of_four_does_not() {
    let _env = lock_env();
    let mut net = Net::cut().await;

    // Before the registration the new key is not an oracle: its attestation is
    // refused at admission. This is what the registration changes.
    let probe = attestation(
        &"ab".repeat(32),
        &net.mango,
        net.genesis_time + 1,
        "pre-reg",
    );
    let refused = net.admit_attestation(probe).await;
    assert!(
        refused.is_err(),
        "an unregistered identity's attestation must be refused before the registration"
    );

    net.register_mango(2).await;

    // One of four (the new key alone): recorded, never paid.
    let before = net.earned().await;
    let (lone, lone_id) = net
        .contribute(1.0, std::slice::from_ref(&net.mango.clone()), "one-of-four")
        .await;
    assert_eq!(count(&lone, TransactionType::ContributionReveal), 1);
    assert_eq!(count(&lone, TransactionType::OracleAttestation), 1);
    assert_eq!(metadata_u64(&lone, "oracle_reward_required_quorum"), 2);
    assert_eq!(metadata_u64(&lone, "oracle_reward_quorum_commit_count"), 0);
    assert_eq!(metadata_u64(&lone, "oracle_reward_missing_quorum_count"), 1);
    assert_eq!(metadata_u64(&lone, "rewards_minted_count"), 0);
    assert_eq!(net.earned().await, before, "1-of-4 must pay nothing");
    {
        let chain = net.producer.read().await;
        let set = chain
            .state
            .attestations
            .get(&lone_id)
            .expect("attestation recorded");
        assert!(!set.has_quorum(chain.state.oracle_quorum));
    }

    // Two of four — the new key plus ONE genesis oracle, i.e. the posture
    // after the workstation drops: paid.
    let pair = [net.mango.clone(), net.genesis_oracles[1].clone()];
    let (paid, _) = net.contribute(1.0, &pair, "two-of-four").await;
    assert_eq!(count(&paid, TransactionType::OracleAttestation), 2);
    assert_eq!(metadata_u64(&paid, "oracle_reward_required_quorum"), 2);
    assert_eq!(metadata_u64(&paid, "oracle_reward_quorum_commit_count"), 1);
    assert_eq!(metadata_u64(&paid, "rewards_minted_count"), 1);
    assert!(
        net.earned().await > before,
        "2-of-4 including the new key must pay the contributor"
    );

    // Two genesis oracles without M still pay too: M is additive, not required.
    let old_pair = [
        net.genesis_oracles[1].clone(),
        net.genesis_oracles[2].clone(),
    ];
    let (old_paid, _) = net.contribute(1.0, &old_pair, "old-pair").await;
    assert_eq!(metadata_u64(&old_paid, "rewards_minted_count"), 1);

    net.resync_from_genesis().await;
}

// ---------------------------------------------------------------------------
// (c) replay is refused, (d) the producer does not wedge
// ---------------------------------------------------------------------------

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "current_thread")]
async fn replayed_registrations_change_nothing_and_the_producer_keeps_producing() {
    let _env = lock_env();
    let mut net = Net::cut().await;
    let (_, batch) = net.register_mango(2).await;

    let snapshot = |chain: &Blockchain| {
        let mut set: Vec<String> = chain.state.oracle_set.iter().cloned().collect();
        set.sort();
        let keys: Vec<(String, Vec<(u64, String)>)> = set
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    chain
                        .state
                        .signing_keys
                        .active_keys_at(id, SigningRole::Oracle, i64::MAX / 2),
                )
            })
            .collect();
        (set, chain.state.oracle_quorum, keys)
    };
    let before = snapshot(&*net.producer.read().await);

    // Replay 1: the exact mined bytes. Replay 2: the same epoch-2 content
    // re-signed with a later submitted_at (fresh tx ids, so tx-id dedup cannot
    // be what stops it). Replay 3: block 1's genesis registrations verbatim —
    // they name the 3-set and would evict M if they ever re-applied.
    let at = net.next_block_time().await;
    let resigned = registration_batch(&net.governance, &net.four_ids(), 2, MIDCHAIN_EPOCH, at, at);
    let genesis_regs: Vec<Transaction> = net.history[0]
        .transactions
        .iter()
        .filter(|tx| tx.tx_type == TransactionType::OracleRegistration)
        .cloned()
        .collect();
    assert_eq!(genesis_regs.len(), 3);

    let replays: Vec<&Transaction> = batch
        .iter()
        .chain(resigned.iter())
        .chain(genesis_regs.iter())
        .collect();
    assert_eq!(replays.len(), 11);
    let mut offered = 0usize;
    for tx in &replays {
        // Admission may refuse outright; that is the strongest outcome. What
        // must never happen is that an admitted replay reaches state.
        if net
            .mempool
            .write()
            .await
            .add_transaction((*tx).clone())
            .is_ok()
        {
            offered += 1;
        }
    }
    eprintln!(
        "replay: {offered} of {} replayed registrations were admitted to the mempool",
        replays.len()
    );

    // (d) keep producing across several blocks with the replays pending; each
    // block must be accepted by the follower with matching roots (`mine`).
    for round in 0..4 {
        let block = net.mine().await;
        assert_eq!(
            count(&block, TransactionType::OracleRegistration),
            0,
            "a replayed registration must not be mined (height {})",
            block.height
        );
        if round == 0 {
            // Not merely skipped: the producer evicts every replay from its
            // mempool in the first block after admission, so nothing lingers
            // to be retried against each later block.
            let mempool = net.mempool.read().await;
            for tx in &replays {
                let id = tx.id.expect("registration tx carries an id");
                assert!(
                    mempool.get_transaction(&id).is_none(),
                    "replayed registration {id} must be evicted, not left pending"
                );
            }
        }
    }

    // The rule that refuses them is the state transition itself, which every
    // applier runs: the epoch is already taken, so the replay cannot reach
    // state even in a block some other producer chose to build. The fresh
    // re-signed batch is inside the submission window, so this — not
    // staleness — is what stops it.
    {
        let chain = net.producer.read().await;
        for tx in resigned.iter().chain(genesis_regs.iter()) {
            let p = tx
                .parse_oracle_registration()
                .expect("registration payload");
            let mut probe = chain.state.clone();
            let refused = probe.apply_transitions_batch(vec![
                wrfcoin_shared_protocol::blockchain::state::StateTransition::ApplyOracleRegistration {
                    oracle_set: p.oracle_set,
                    oracle_quorum: p.oracle_quorum,
                    identity: p.identity.clone(),
                    public_key: p.public_key,
                    epoch: p.epoch,
                    activated_at: p.activated_at,
                },
            ]);
            assert!(
                refused.is_err(),
                "re-applying epoch {} for {} must be refused by the state transition",
                p.epoch,
                p.identity
            );
        }
    }
    assert_eq!(
        snapshot(&*net.producer.read().await),
        before,
        "replayed registrations must not change the oracle set, quorum or keys"
    );

    // And the chain still pays after the replay attempt.
    let pair = [net.mango.clone(), net.genesis_oracles[0].clone()];
    let (paid, _) = net.contribute(1.0, &pair, "after-replay").await;
    assert_eq!(metadata_u64(&paid, "rewards_minted_count"), 1);
    net.resync_from_genesis().await;
}

// ---------------------------------------------------------------------------
// (e) only the low-quality override quorum moves, and only to 3
// ---------------------------------------------------------------------------

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "current_thread")]
async fn low_quality_override_quorum_is_three_before_and_after_and_rewards_stay_at_two() {
    let _env = lock_env();
    assert_eq!(compute_oracle_quorum(3), 3, "N=3 override quorum (today)");
    assert_eq!(compute_oracle_quorum(4), 3, "N=4 override quorum (after M)");

    let mut net = Net::cut().await;

    // BEFORE (N=3): a low-quality reveal needs 3 approvals to be overridden in.
    let two = [
        net.genesis_oracles[0].clone(),
        net.genesis_oracles[1].clone(),
    ];
    let (b, _) = net.contribute(0.1, &two, "lq-n3-two").await;
    assert_eq!(
        count(&b, TransactionType::ContributionReveal),
        0,
        "N=3: 2 approvals must not override a low-quality reveal in"
    );
    let three = net.genesis_oracles.clone();
    let (b, _) = net.contribute(0.1, &three, "lq-n3-three").await;
    assert_eq!(
        count(&b, TransactionType::ContributionReveal),
        1,
        "N=3: 3 approvals override a low-quality reveal in"
    );

    net.register_mango(2).await;
    assert_eq!(net.producer.read().await.state.oracle_set.len(), 4);

    // AFTER (N=4): the same thresholds — 2 is not enough, 3 is.
    let two = [net.mango.clone(), net.genesis_oracles[2].clone()];
    let (b, _) = net.contribute(0.1, &two, "lq-n4-two").await;
    assert_eq!(
        count(&b, TransactionType::ContributionReveal),
        0,
        "N=4: 2 approvals must not override a low-quality reveal in"
    );
    let three = [
        net.mango.clone(),
        net.genesis_oracles[0].clone(),
        net.genesis_oracles[2].clone(),
    ];
    let (b, _) = net.contribute(0.1, &three, "lq-n4-three").await;
    assert_eq!(
        count(&b, TransactionType::ContributionReveal),
        1,
        "N=4: 3 approvals override a low-quality reveal in, as at N=3"
    );

    // Nothing else tightened: the reward quorum is still 2.
    let pair = [net.mango.clone(), net.genesis_oracles[2].clone()];
    let (paid, _) = net.contribute(1.0, &pair, "hq-n4-two").await;
    assert_eq!(metadata_u64(&paid, "oracle_reward_required_quorum"), 2);
    assert_eq!(metadata_u64(&paid, "rewards_minted_count"), 1);
    net.resync_from_genesis().await;
}

// ---------------------------------------------------------------------------
// CONTROL: the same batch declaring quorum 3 starves two live attesters
// ---------------------------------------------------------------------------

/// The plan's hard rule, executed: with one genesis oracle cold, declaring
/// K=3 for N=4 means the two remaining attesters can never pay anything —
/// the collectors would withhold and mining would stall. Everything else here
/// is identical to the K=2 tests, so this pins that the quorum VALUE in the
/// batch is what the positive results depend on.
#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "current_thread")]
async fn control_declaring_quorum_three_leaves_two_live_attesters_unable_to_pay() {
    let _env = lock_env();
    let mut net = Net::cut().await;
    net.register_mango(3).await;
    assert_eq!(net.producer.read().await.state.oracle_quorum, 3);

    let before = net.earned().await;
    let live_pair = [net.mango.clone(), net.genesis_oracles[1].clone()];
    let (b, _) = net.contribute(1.0, &live_pair, "k3-two").await;
    assert_eq!(metadata_u64(&b, "oracle_reward_required_quorum"), 3);
    assert_eq!(metadata_u64(&b, "oracle_reward_quorum_commit_count"), 0);
    assert_eq!(metadata_u64(&b, "rewards_minted_count"), 0);
    assert_eq!(
        net.earned().await,
        before,
        "K=3 with two live attesters must pay nothing — the halt the plan forbids"
    );
}

// ---------------------------------------------------------------------------
// (b) + (d) at live heights: deferral instead of an unpaid mine
// ---------------------------------------------------------------------------

/// The live chain is above `RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT`,
/// where a real reveal WITHOUT quorum is not mined unpaid but held back in the
/// mempool until quorum arrives (#2777). So on the live chain "1-of-4 does not
/// pay" means "is deferred", and the risk to rule out is a deferred reveal
/// wedging the producer. This registers M above that height, shows a 1-of-4
/// reveal is deferred while blocks keep flowing, then shows a second
/// attestation from the NEW key releases and pays it.
#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "current_thread")]
async fn at_live_heights_one_of_four_defers_without_wedging_and_two_of_four_releases() {
    use wrfcoin_shared_protocol::consensus::proof_of_publishing::RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT;

    let _env = lock_env();
    let mut net = Net::cut().await;
    net.advance_to(RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT + 5)
        .await;
    // From here on, blocks arrive every five minutes, like the live chain's
    // collector-driven cadence — far inside the 3600 s submission window, so a
    // deferred reveal stays eligible across several blocks.
    net.cadence = Some(300);
    let (reg_block, _) = net.register_mango(2).await;
    assert!(reg_block.height > RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT);

    let before = net.earned().await;
    let commit_at = net.next_block_time().await;
    let (commit, reveal, commit_id) =
        commit_and_reveal(commit_at - 30, 1.0, net.contributor, "live-height");
    net.admit(&[stamped(commit, commit_at)]).await;
    let commit_block = net.mine().await;
    assert_eq!(count(&commit_block, TransactionType::ContributionCommit), 1);

    // One of four: the genesis oracle that stays online.
    let reveal_at = net.next_block_time().await;
    net.admit(&[stamped(reveal.clone(), reveal_at)]).await;
    net.admit_attestation(stamped(
        attestation(&commit_id, &net.genesis_oracles[1], reveal_at - 5, "live-1"),
        reveal_at,
    ))
    .await
    .expect("first attestation admitted");

    // Blocks keep coming, the reveal is held back, nothing is paid. The reveal
    // window at this height is the 3-block floor (1440 / 3600 s rounds to 0),
    // so the second approval has to land within two deferred blocks — the
    // attesters poll every 5 s, so on the live cadence that is minutes.
    let height_now = net.producer.read().await.height;
    assert_eq!(
        wrfcoin_shared_protocol::consensus::proof_of_publishing::reveal_window_blocks(height_now),
        3,
        "the deferral budget below is sized to the 3-block reveal window"
    );
    let reveal_id = stamped(reveal, reveal_at).id.expect("reveal id");
    for _ in 0..2 {
        let block = net.mine().await;
        assert_eq!(
            count(&block, TransactionType::ContributionReveal),
            0,
            "1-of-4: the real reveal must be deferred at height {}",
            block.height
        );
        assert_eq!(metadata_u64(&block, "rewards_minted_count"), 0);
    }
    assert!(
        net.mempool
            .read()
            .await
            .get_transaction(&reveal_id)
            .is_some(),
        "the deferred reveal must still be pending, not dropped"
    );
    assert_eq!(net.earned().await, before);

    // The NEW key supplies the second approval: the reveal is released and paid.
    let release_at = net.next_block_time().await;
    net.admit_attestation(stamped(
        attestation(&commit_id, &net.mango, release_at - 5, "live-m"),
        release_at,
    ))
    .await
    .expect("M's attestation admitted after registration");
    let released = net.mine().await;
    assert_eq!(
        count(&released, TransactionType::ContributionReveal),
        1,
        "2-of-4 (one genesis oracle + M) must release the deferred reveal"
    );
    assert!(net.earned().await > before, "and pay the contributor");

    net.resync_from_genesis().await;
}

/// The same control at live heights: under K=3 the two live attesters'
/// approvals never release the reveal. It is deferred until its reveal window
/// closes and then dropped, unpaid — on the live chain the collectors would
/// withhold `/blocks/mine` behind it, which is the halt shape.
#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "current_thread")]
async fn control_at_live_heights_quorum_three_defers_until_the_reveal_expires() {
    use wrfcoin_shared_protocol::consensus::proof_of_publishing::RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT;

    let _env = lock_env();
    let mut net = Net::cut().await;
    net.advance_to(RICH_AMBIENT_REWARD_ACTIVATION_HEIGHT + 5)
        .await;
    net.cadence = Some(300);
    net.register_mango(3).await;

    let before = net.earned().await;
    let commit_at = net.next_block_time().await;
    let (commit, reveal, commit_id) =
        commit_and_reveal(commit_at - 30, 1.0, net.contributor, "live-k3");
    net.admit(&[stamped(commit, commit_at)]).await;
    net.mine().await;

    let reveal_at = net.next_block_time().await;
    net.admit(&[stamped(reveal, reveal_at)]).await;
    for (i, oracle) in [net.genesis_oracles[1].clone(), net.mango.clone()]
        .iter()
        .enumerate()
    {
        net.admit_attestation(stamped(
            attestation(
                &commit_id,
                oracle,
                reveal_at - 5 + i as i64,
                &format!("live-k3-{i}"),
            ),
            reveal_at,
        ))
        .await
        .expect("attestation admitted");
    }
    // Well past the 3-block reveal window: never mined, never paid.
    for _ in 0..6 {
        let block = net.mine().await;
        assert_eq!(
            count(&block, TransactionType::ContributionReveal),
            0,
            "K=3 with two live attesters must never release the reveal (height {})",
            block.height
        );
    }
    assert_eq!(net.earned().await, before);
}
