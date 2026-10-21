//! #2290 — the APPEND BOUNDARY itself refuses an unauthorized locally built block.
//!
//! `/blocks/mine` gates via the #2418 seam, gossip gates in
//! `pop_adapter::validate_block`, and IBD gates in `p2p::sync::validate_block`.
//! Those prove today's *callers* are correct. They do not prove the *boundary*
//! is guarded: a future builder, or a new caller that does not go through the
//! route, would append an unauthorized block with nothing to stop it, and the
//! failure is silent — the node advances its own chain while every peer rejects
//! the block.
//!
//! So `ProofOfPublishing::create_next_block` now applies the SAME
//! `verify_block_producer_authorized`, resolved from the SAME
//! `producer_set_history`, after signature attachment and before `add_block` /
//! staged-state installation. One predicate at several boundaries cannot drift;
//! a second implementation of it would be the producer/applier divergence this
//! epic exists to close.
//!
//! The assertion bar here is deliberately the one #2418 set. A regression that
//! checks only the durable block count PASSES on the broken code — Bob's first
//! version did exactly that and shipped a self-fork, because the in-memory chain
//! had already advanced while storage had not. So all six dimensions are
//! snapshotted: height, tip, state root, in-memory blocks, mempool, durable
//! blocks.
//!
//! Run: cargo test -p wrfcoin-native-chain --test producer_auth_append_boundary_2290

use std::path::PathBuf;
use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::RwLock;

use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::State;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;
use wrfcoin_shared_protocol::mempool::pool::Mempool;

/// Unique temp dir per test, mirroring `producer_auth_persisted_boot_2290.rs`
/// (this crate has no `tempfile` dev-dependency).
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wrfcoin-2290-append-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
    let mut rng = StdRng::seed_from_u64(seed);
    let sk = PrivateKey::generate(&mut rng);
    let pk = PublicKey::from_private_key(&sk);
    (sk, pk)
}

/// Everything an unauthorized append would move. Compared as a whole so a fix
/// that protects one dimension and leaks another cannot pass.
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    height: u64,
    tip: String,
    state_root: String,
    in_memory_blocks: usize,
    mempool_txs: usize,
    durable_blocks: usize,
}

async fn snapshot(
    chain: &Arc<RwLock<Blockchain>>,
    mempool: &Arc<RwLock<Mempool>>,
    storage: &Storage,
) -> Snapshot {
    let c = chain.read().await;
    Snapshot {
        height: c.height,
        tip: c.tip.to_string(),
        state_root: c.state.state_root.to_string(),
        in_memory_blocks: c.blocks().len(),
        mempool_txs: mempool.read().await.transaction_ids().len(),
        durable_blocks: storage
            .scan_persisted_blocks(|chunk| {
                let _ = chunk;
                std::ops::ControlFlow::Continue(())
            })
            .expect("scan durable archive"),
    }
}

/// **The observable persistence seam.**
///
/// `create_next_block` has no storage handle and never calls `persist_block`, so
/// a test that opens a `Storage` and calls only the builder sees a durable count
/// of zero before AND after, whatever the builder does. The storage dimension of
/// the snapshot is then decoration: it can never be the assertion that fails.
///
/// This helper closes that hole by doing what the `/blocks/mine` route does once
/// its gate passes — persist the accepted block. A build that should have been
/// refused therefore becomes visible in the durable archive, and the six-field
/// comparison can genuinely go RED on it.
async fn build_then_persist(
    pop: &mut ProofOfPublishing,
    chain: &Arc<RwLock<Blockchain>>,
    mempool: &Arc<RwLock<Mempool>>,
    miner: &str,
    storage: &mut Storage,
) -> anyhow::Result<()> {
    let block = pop
        .create_next_block(chain.clone(), mempool.clone(), miner)
        .await?;
    // Only reached when the builder ACCEPTED the block. Mirrors the route's
    // post-gate persistence.
    storage
        .persist_block(block, vec![], None)
        .await
        .map_err(|e| anyhow::anyhow!("persist failed: {e}"))?;
    Ok(())
}

/// A chain whose genesis seeds an authorized producer set containing `member`.
fn seeded_chain(member: &PublicKey) -> Blockchain {
    let mut state = State::default();
    state
        .seed_genesis_producer_set(vec![ProducerMember {
            producer_id: "authorized-producer".to_string(),
            payout_address: Address([0x5au8; 20]).to_hex(),
            public_key: <[u8; 32]>::try_from(member.as_bytes()).expect("32-byte key"),
        }])
        .expect("genesis producer set is valid");
    Blockchain::from_persisted(0, Hash::default(), state, Default::default(), 0, 0)
}

/// A seeded chain with NO producer signer builds an UNSIGNED block.
///
/// The scenario is chosen so that only the append-boundary gate can catch it.
/// The obvious case — a signer whose key is not a member — is caught earlier by
/// the §R2 payout resolver, which refuses to produce for a non-member. A test
/// using that case passes without the gate and proves nothing about it; the
/// first draft of this file did exactly that and had to be re-targeted.
///
/// With no signer configured the payout resolver early-returns (nothing to
/// resolve), the payout binding is inert, stamping and signing are no-ops, and
/// the block reaches `add_block` unsigned. `verify_block_producer_authorized`
/// rejects it as `Unauthenticated(Unsigned)` — and nothing else in the builder
/// does.
///
/// RED without the gate: `create_next_block` returns `Ok`, and the snapshot
/// moves on height, tip, state root, in-memory blocks and mempool while the
/// durable archive stays put — the exact self-fork shape.
#[tokio::test]
async fn unauthorized_producer_cannot_append_locally_2290() {
    let dir = temp_dir("unauthorized");
    let mut storage = Storage::open(&dir.join("wrfcoin.redb"), 1_000_000).expect("open storage");

    // The set authorizes `member_pk`. This node has NO producer signer, so the
    // block it builds is unsigned — invisible to every check except the gate.
    let (_member_sk, member_pk) = keypair(2_290_001);

    let mut pop = ProofOfPublishing::new(1, 60, 1_000_000, 10);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();

    let chain = Arc::new(RwLock::new(seeded_chain(&member_pk)));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    {
        // A couple of mempool entries so a drain would be observable.
        let mut m = mempool.write().await;
        for i in 0..2u8 {
            let tx = Transaction::new_coinbase(&Address([i; 20]).to_hex(), 1, 1);
            let _ = m.add_transaction(tx);
        }
    }

    let before = snapshot(&chain, &mempool, &storage).await;

    let result = build_then_persist(
        &mut pop,
        &chain,
        &mempool,
        &Address([0x5au8; 20]).to_hex(),
        &mut storage,
    )
    .await;

    // Snapshot BEFORE asserting on the result, for the same reason as the
    // non-member test: panicking on the result first would make the state
    // comparison unreachable on exactly the path it exists to check.
    let after = snapshot(&chain, &mempool, &storage).await;

    assert_eq!(
        before, after,
        "a refused append must leave height, tip, state root, in-memory blocks, mempool \
         AND durable blocks untouched. Asserting only the durable count PASSES on the \
         broken code, because the in-memory chain advances while storage does not — that \
         is how the first version of this guard shipped a self-fork (#2418)."
    );

    let err = match result {
        Err(e) => format!("{e:#}"),
        Ok(()) => panic!(
            "an unsigned block must not be appended. The append-boundary gate is the only \
             guard that can see this case — the payout resolver early-returns without a \
             signer (#2290)."
        ),
    };
    assert!(
        err.contains("producer-authority gate"),
        "the refusal must come from the append-boundary gate, got: {err}"
    );
}

/// **The operationally realistic case: the local miner is NOT the seeded member.**
///
/// This is what a misprovisioned node looks like on the cut — genesis seeds the
/// authorized set, and this node's producer key is somebody else's. It must not
/// append and must not persist, and the six-dimension snapshot must be identical
/// afterwards.
///
/// Attribution, stated precisely because it matters for what a revert proves:
/// this case is refused by the **§R2 payout resolver**, which runs before the
/// append gate and declines to resolve a payout for a non-member. The append
/// gate is the backstop beneath it, exercised in isolation by
/// `unauthorized_producer_cannot_append_locally_2290` (unsigned block, which the
/// resolver cannot see). Both layers are proven; neither test is credited to the
/// other's rule.
#[tokio::test]
async fn a_non_member_local_miner_cannot_append_or_persist_2290() {
    let dir = temp_dir("non-member-miner");
    let mut storage = Storage::open(&dir.join("wrfcoin.redb"), 1_000_000).expect("open storage");

    // Genesis authorizes `member_pk`. This node holds a different key entirely.
    let (_member_sk, member_pk) = keypair(2_290_001);
    let (local_sk, local_pk) = keypair(2_290_002);
    assert_ne!(
        member_pk.as_bytes(),
        local_pk.as_bytes(),
        "test premise: the local miner must not be the seeded member"
    );

    let mut pop = ProofOfPublishing::new(1, 60, 1_000_000, 10);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();
    pop.set_producer_signer(Arc::new(EnvKeySigner::from_private_key(
        local_sk,
        "TEST_NON_MEMBER_LOCAL_MINER",
    )));

    let chain = Arc::new(RwLock::new(seeded_chain(&member_pk)));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    {
        let mut m = mempool.write().await;
        for i in 0..2u8 {
            let tx = Transaction::new_coinbase(&Address([i; 20]).to_hex(), 1, 1);
            let _ = m.add_transaction(tx);
        }
    }

    let before = snapshot(&chain, &mempool, &storage).await;

    let result = build_then_persist(
        &mut pop,
        &chain,
        &mempool,
        &Address([0x5au8; 20]).to_hex(),
        &mut storage,
    )
    .await;

    // The after-snapshot is taken BEFORE any assertion on `result`.
    //
    // Ordering matters and was wrong in the first version of this test: asserting
    // `result.is_err()` first meant that when the guards were removed — the only
    // case where the six-field comparison has anything to say — the test panicked
    // on the result assertion and never reached the comparison. The state
    // assertion was dead code on precisely the path it existed for.
    let after = snapshot(&chain, &mempool, &storage).await;

    assert_eq!(
        before, after,
        "no append and no persist: height, tip, state root, in-memory blocks, mempool \
         AND durable blocks must all be unchanged. This is the load-bearing assertion \
         and it is checked BEFORE the result, so removing the guards shows exactly which \
         dimensions moved rather than failing on `is_err` first."
    );
    assert!(
        result.is_err(),
        "a node whose producer key is not in the authorized set must not produce. \
         Producing here advances only this node's chain while every peer rejects the \
         block — the silent self-fork #2290 exists to prevent."
    );
}

/// The converse: the authorized member CAN append. A gate that cannot pass is a
/// gate that gets deleted the first time it blocks a launch.
#[tokio::test]
async fn authorized_producer_can_append_locally_2290() {
    let dir = temp_dir("authorized");
    let storage = Storage::open(&dir.join("wrfcoin.redb"), 1_000_000).expect("open storage");

    let (member_sk, member_pk) = keypair(2_290_001);
    let mut pop = ProofOfPublishing::new(1, 60, 1_000_000, 10);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();
    pop.set_producer_signer(Arc::new(EnvKeySigner::from_private_key(
        member_sk,
        "TEST_AUTHORIZED_PRODUCER",
    )));

    let chain = Arc::new(RwLock::new(seeded_chain(&member_pk)));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let before = snapshot(&chain, &mempool, &storage).await;

    pop.create_next_block(chain.clone(), mempool.clone(), &Address([0x5au8; 20]).to_hex())
        .await
        .expect("the authorized member must be able to produce");

    let after = snapshot(&chain, &mempool, &storage).await;
    assert_eq!(after.height, before.height + 1, "the authorized block appends");
    assert_ne!(after.tip, before.tip, "tip advances");
}

/// Scope guard: a chain that never seeded a producer set is unenforced, exactly
/// as the shared verifier is. Without this the gate would brick every legacy and
/// dev chain.
#[tokio::test]
async fn an_unseeded_chain_is_not_gated_2290() {
    let dir = temp_dir("unseeded");
    let storage = Storage::open(&dir.join("wrfcoin.redb"), 1_000_000).expect("open storage");

    let (rogue_sk, _) = keypair(2_290_002);
    let mut pop = ProofOfPublishing::new(1, 60, 1_000_000, 10);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();
    pop.set_producer_signer(Arc::new(EnvKeySigner::from_private_key(
        rogue_sk,
        "TEST_ANY_PRODUCER",
    )));

    // No producer set seeded.
    let chain = Arc::new(RwLock::new(Blockchain::default()));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let before = snapshot(&chain, &mempool, &storage).await;

    pop.create_next_block(chain.clone(), mempool.clone(), &Address([0x5au8; 20]).to_hex())
        .await
        .expect("an unseeded chain must remain unenforced");

    let after = snapshot(&chain, &mempool, &storage).await;
    assert_eq!(after.height, before.height + 1);
}
