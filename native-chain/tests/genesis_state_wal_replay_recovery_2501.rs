//! #2501 — the RECOVERY half: genesis-seeded state must survive pure WAL replay.
//!
//! # What was already settled, and by whom
//!
//! `producer_signed_wal_parity_2226.rs` (PR #2496) established the defect and its
//! severity: the genesis producer set is written into genesis `State` by
//! `State::seed_genesis_producer_set` and is carried by **no** `StateTransition`,
//! so a node recovered by checkpoint-less WAL replay comes up without it. That
//! work then closed the *detection* side — a boot that holds signed blocks but no
//! producer set now refuses to start, which covers V1, where the #2219 tip
//! state-root commitment check is `Ok` by design.
//!
//! It deliberately left one thing open, and named it: *"the residual is
//! availability: a node whose only recovery route is a checkpoint-less WAL cannot
//! restart and must resync from a peer."*
//!
//! # The condition, corrected after review
//!
//! An earlier revision of this file argued the residual was a launch blocker
//! because *"no checkpoint exists at any height in 1..=99"* and a fresh cut chain
//! runs *"on a producer set of one"*, so there would be no peer to resync from.
//! **Both of those were false, and the review that caught them was right:**
//!
//! - `DEFAULT_CHECKPOINT_INTERVAL = 100` governs only the **periodic** writer.
//!   There is a second one: on clean exit the node calls
//!   `Storage::force_checkpoint` at **any** height > 0 (`main.rs`, phase 2 of
//!   shutdown). A cleanly stopped node at height 7 has a checkpoint at height 7.
//!   Pinned by `a_cleanly_shut_down_node_recovers_without_the_anchor` below,
//!   which passes **without** the anchor.
//! - The pinned testnet genesis set has **two** members, not one —
//!   `genesis_producers.rs` asserts `members.len() == 2` (the ceremony authorized
//!   a producer and a standby). "No peer to resync from" was never established.
//!
//! That generalisation — from the periodic predicate to *all* checkpointing —
//! is recorded here rather than quietly deleted, because an overstated rationale
//! in a consensus-adjacent PR is itself a defect: the next reader reasons from it.
//!
//! # What is actually true, and it is enough
//!
//! The exposed condition is an **abrupt** stop below height 100 — crash, OOM,
//! SIGKILL, container kill, power loss — where neither the periodic nor the
//! shutdown checkpoint ran. Then WAL replay is the only recovery route, and it
//! loses genesis-only state by construction.
//!
//! Severity, measured rather than argued: at **V2** the #2219 tip commitment
//! turns the loss into a refusal to start, so such a node cannot restart from its
//! own WAL and must be resynced from a peer — a **resilience/ops** failure, not a
//! chain halt. At **V1** the same node starts *silently unenforced*, which is the
//! more serious of the two and is the case #2496's detection half covers.
//!
//! # The structural cause, stated as a replay invariant
//!
//! Replay is `anchor + log`. `Storage::load_state` anchors on `State::new()`
//! whenever no checkpoint is recorded, but the chain's real genesis state is
//! **not** `State::new()` — it is the seeded state `main.rs` builds on its
//! fresh-genesis branch. The log is faithful; the anchor is wrong. Nothing that
//! changes transition *content* can fix an anchor that was never written, which
//! is why this needs code rather than configuration.
//!
//! So the fix is to persist the genesis anchor: `Storage::persist_genesis_state`
//! writes the seeded genesis `State` as a height-0 checkpoint, and `load_state`
//! anchors replay on it. That generalises past the producer set — it makes **all**
//! genesis-only state recoverable, which the per-field alternatives do not.
//!
//! # What each test here pins
//!
//! 1. `genesis_producer_set_survives_pure_wal_replay` — the set is back.
//! 2. `wal_replayed_state_root_matches_the_producers` — producer==replayer parity
//!    on `state_root`, the thing #2219 actually checks.
//! 3. `wal_recovered_node_starts_at_v2` — the startup refusal is lifted.
//! 4. `wal_recovered_node_still_rejects_an_unauthorized_producer` — the security
//!    property, not just the availability one. This is the test that would catch a
//!    "fix" that restored the root while leaving the gate unenforced.
//! 5. `unseeded_chain_is_unaffected` — the anchor is not a new failure mode for a
//!    chain that never had genesis-only state.
//! 6. `checkpoint_pruning_preserves_the_genesis_anchor` — the anchor must survive
//!    the two prune loops that delete `h < height`, or the fix decays back into
//!    the defect at height 100.
//! 7. `a_cleanly_shut_down_node_recovers_without_the_anchor` — the boundary of
//!    the claim: the clean path never needed this fix.
//!
//! Non-degenerate by construction: block 1 carries a **non-empty** transition
//! vector, so replay has real work to do and a passing `state_root` comparison is
//! not the all-zero-default coincidence that made the earlier fixtures unable to
//! isolate the set.
//!
//! Run: cargo test -p wrfcoin-native-chain --test genesis_state_wal_replay_recovery_2501

use std::path::PathBuf;

use rand::SeedableRng;
use rand::rngs::StdRng;

use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
use wrfcoin_native_chain::storage::verify_tip_state_root_commitment_at;
use wrfcoin_shared_protocol::blockchain::block::{Block, STATE_ROOT_METADATA_KEY};
use wrfcoin_shared_protocol::blockchain::state::{State, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{Transaction, TransactionOutput};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::consensus::block_producer_auth::sign_block_producer;
use wrfcoin_shared_protocol::consensus::producer_authority::verify_block_producer_authorized;
use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;
use wrfcoin_shared_protocol::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

/// Large enough that `needs_checkpoint` is never true for any height these
/// tests reach, so recovery is PURE WAL replay from the genesis anchor — the
/// 1..=99 window a fresh cut chain lives in.
const NEVER_CHECKPOINT: u64 = u64::MAX / 2;

fn tmp_db() -> PathBuf {
    std::env::temp_dir().join(format!("wrfcoin-2501-genesis-anchor-{}.redb", fastrand::u64(..)))
}

fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
    let mut rng = StdRng::seed_from_u64(seed);
    let sk = PrivateKey::generate(&mut rng);
    let pk = PublicKey::from_private_key(&sk);
    (sk, pk)
}

/// Canonical payout `Address` (0x-hex) — the coinbase form the mining route
/// accepts (#2290 §R2), not `pk.to_address()`'s wrf form.
fn payout_of(pk: &PublicKey) -> String {
    let key: [u8; 32] = pk.as_bytes().try_into().expect("32-byte pubkey");
    Address(key[0..20].try_into().expect("20-byte prefix")).to_string()
}

fn member_of(pk: &PublicKey, id: &str) -> ProducerMember {
    ProducerMember {
        producer_id: id.to_string(),
        payout_address: payout_of(pk),
        public_key: pk.as_bytes().try_into().expect("32-byte pubkey"),
    }
}

/// The genesis `State` `main.rs` builds on its fresh-genesis branch: seeded
/// producer set, root recalculated. This is the anchor replay has to start from.
fn seeded_genesis(member: &ProducerMember) -> State {
    let mut state = State::new();
    state
        .seed_genesis_producer_set(vec![member.clone()])
        .expect("genesis producer set must seed");
    state.recalculate_state_root();
    state
}

/// A non-empty transition vector, so replay reconstructs something real and a
/// `state_root` match cannot be the empty-replay coincidence.
fn block_1_transitions(member: &ProducerMember) -> Vec<StateTransition> {
    vec![StateTransition::ApplyReward {
        address: Address::from_hex(&member.payout_address).expect("payout address parses"),
        amount: Balance(5_000),
        reason: "#2501 fixture: block-1 producer reward".to_string(),
    }]
}

/// Apply block 1 exactly as the producer does: batch transitions, then the
/// shared finalization sink. Returns the post-block state.
fn producer_state_after_block_1(genesis: &State, member: &ProducerMember) -> State {
    let mut state = genesis.clone();
    state
        .apply_transitions_batch(block_1_transitions(member))
        .expect("block-1 transitions apply");
    state.apply_block_finalization(1);
    state
}

fn identity() -> ChainIdentity {
    ChainIdentity::new("wrfcoin-testnet", "testnet")
}

/// Block 1 signed by `signer`, paying `payout`, committing `committed_root`
/// under [`STATE_ROOT_METADATA_KEY`]. Metadata is inside `canonical_body`, so it
/// is final before the signature is taken.
fn signed_block_1(signer: &PrivateKey, payout: &str, committed_root: &str) -> Block {
    let mut coinbase = Transaction::new(vec![], vec![TransactionOutput {
        amount: 5_000,
        address: payout.to_string(),
        script: "OP_TRUE".to_string(),
        nft_data: None,
    }]);
    coinbase.set_id();

    let mut block = Block::new(3, Hash::default(), vec![coinbase], 1, 1);
    block
        .metadata
        .insert(STATE_ROOT_METADATA_KEY.to_string(), committed_root.to_string());

    let att = sign_block_producer(&block, signer, &identity(), SigningProtocolVersion::V2);
    block.producer_pubkey = att.producer_pubkey;
    block.producer_sig = att.producer_sig.to_vec();

    let hash = block.calculate_hash();
    block.set_hash(hash);
    block
}

/// Build the exact on-disk shape a fresh cut node reaches at height 1 with no
/// checkpoint: genesis anchor persisted, then block 1 with its transitions.
///
/// Returns `(db_path, producer_state, member)`.
async fn persist_fresh_cut_chain_at_height_1() -> (PathBuf, State, ProducerMember) {
    let (member_sk, member_pk) = keypair(1);
    let member = member_of(&member_pk, "n5-a");

    let genesis = seeded_genesis(&member);
    let producer_state = producer_state_after_block_1(&genesis, &member);

    let db_path = tmp_db();
    let mut storage = Storage::open(&db_path, NEVER_CHECKPOINT).expect("open storage");

    // THE FIX, at its single production call site's equivalent: `main.rs` writes
    // the genesis anchor on the fresh-genesis branch, before any block exists.
    storage
        .persist_genesis_state(&genesis)
        .await
        .expect("genesis anchor must persist");

    let block = signed_block_1(
        &member_sk,
        &member.payout_address,
        &producer_state.state_root.to_string(),
    );
    storage
        .persist_block(block, block_1_transitions(&member), None)
        .await
        .expect("persist block 1");

    (db_path, producer_state, member)
}

// ---------------------------------------------------------------------------
// The reachability premise, asserted rather than asserted-in-prose
// ---------------------------------------------------------------------------

/// Half of the condition: the PERIODIC writer never fires below height 100.
///
/// Named for exactly what it measures. An earlier revision called this
/// `no_checkpoint_exists_below_height_100_by_default`, which was wrong — it
/// asserts only `needs_checkpoint`, the periodic predicate, and there is a
/// second writer (see the test below). Reviewing that name against what the
/// body proves is what caught the overstatement.
#[test]
fn periodic_checkpointing_never_fires_below_height_100() {
    let storage_interval = DEFAULT_CHECKPOINT_INTERVAL;
    assert_eq!(
        storage_interval, 100,
        "the periodic-writer gap is derived from this constant"
    );
    let db_path = tmp_db();
    let storage = Storage::open(&db_path, storage_interval).expect("open storage");
    for height in 1..storage_interval {
        assert!(
            !storage.needs_checkpoint(height),
            "the periodic writer must not fire at height {height}"
        );
    }
    let _ = std::fs::remove_file(&db_path);
}

/// The other half, and the reason the claim had to be narrowed: a node that
/// exits CLEANLY writes a checkpoint at any height > 0
/// (`main.rs` → `Storage::force_checkpoint`), so it recovers with the producer
/// set intact **without the anchor**.
///
/// This test deliberately does NOT call `persist_genesis_state`. It is the
/// discriminator: if it ever needed the anchor, the anchor's justification would
/// be broader than abrupt restart, and the claim in the module docs would be
/// wrong in the other direction.
#[tokio::test]
async fn a_cleanly_shut_down_node_recovers_without_the_anchor() {
    let (member_sk, member_pk) = keypair(11);
    let member = member_of(&member_pk, "n5-a");
    let genesis = seeded_genesis(&member);
    let producer_state = producer_state_after_block_1(&genesis, &member);

    let db_path = tmp_db();
    let mut storage = Storage::open(&db_path, NEVER_CHECKPOINT).expect("open storage");
    // No `persist_genesis_state` — this is the pre-fix shape on purpose.
    let block = signed_block_1(
        &member_sk,
        &member.payout_address,
        &producer_state.state_root.to_string(),
    );
    storage
        .persist_block(block, block_1_transitions(&member), None)
        .await
        .expect("persist block 1");
    // The graceful-shutdown writer, at height 1 — below the periodic interval.
    storage
        .force_checkpoint(&producer_state, 1)
        .await
        .expect("shutdown checkpoint");
    drop(storage);

    let reopened = Storage::open(&db_path, NEVER_CHECKPOINT).expect("reopen storage");
    let recovered = reopened
        .load_state()
        .expect("load_state must succeed")
        .expect("a persisted chain must be found");

    assert_eq!(
        recovered.state.producer_set_history.len(),
        1,
        "a clean shutdown checkpoints at any height > 0, so the producer set \
         survives WITHOUT the genesis anchor — the anchor's justification is the \
         ABRUPT-stop case only"
    );
    assert!(
        verify_tip_state_root_commitment_at(SigningProtocolVersion::V2, &recovered).is_ok(),
        "and the cleanly shut down node starts normally"
    );

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// 1 — the set is back
// ---------------------------------------------------------------------------

#[tokio::test]
async fn genesis_producer_set_survives_pure_wal_replay() {
    let (db_path, _producer_state, member) = persist_fresh_cut_chain_at_height_1().await;

    let storage = Storage::open(&db_path, NEVER_CHECKPOINT).expect("reopen storage");
    let recovered = storage
        .load_state()
        .expect("load_state must succeed")
        .expect("a persisted chain must be found");

    assert_eq!(
        recovered.height, 1,
        "precondition: the recovered chain must be at height 1"
    );
    assert_eq!(
        recovered.state.producer_set_history.len(),
        1,
        "the genesis producer set must survive checkpoint-less WAL replay — an \
         empty history is read everywhere as \"this chain never configured \
         producer auth\""
    );
    assert_eq!(
        recovered.state.producer_set_history[0].members,
        vec![member],
        "the recovered set must be the seeded set, member-for-member"
    );

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// 2 — producer == replayer on state_root
// ---------------------------------------------------------------------------

#[tokio::test]
async fn wal_replayed_state_root_matches_the_producers() {
    let (db_path, producer_state, _member) = persist_fresh_cut_chain_at_height_1().await;

    let storage = Storage::open(&db_path, NEVER_CHECKPOINT).expect("reopen storage");
    let recovered = storage
        .load_state()
        .expect("load_state must succeed")
        .expect("a persisted chain must be found");

    assert_eq!(
        recovered.state.state_root.to_string(),
        producer_state.state_root.to_string(),
        "WAL replay must reach the producer's post-block-1 state_root; a mismatch \
         here is exactly what #2219 turns into a startup refusal"
    );

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// 3 — the node starts again
// ---------------------------------------------------------------------------

#[tokio::test]
async fn wal_recovered_node_starts_at_v2() {
    let (db_path, _producer_state, _member) = persist_fresh_cut_chain_at_height_1().await;

    let storage = Storage::open(&db_path, NEVER_CHECKPOINT).expect("reopen storage");
    let recovered = storage
        .load_state()
        .expect("load_state must succeed")
        .expect("a persisted chain must be found");

    // The version-explicit variant, for the same reason the production code has
    // one: a test binary installs nothing, and V2 is the version the #2034 cut
    // activates and the only one under which this check does anything.
    let verdict = verify_tip_state_root_commitment_at(SigningProtocolVersion::V2, &recovered);
    assert!(
        verdict.is_ok(),
        "a WAL-recovered node must start: {:?}. Before the genesis anchor this is \
         EXIT_STATE_ROOT_COMMITMENT_MISMATCH, reachable only by an ABRUPT stop \
         (kill -9, container kill, power loss) below the first checkpoint -- a \
         clean shutdown calls force_checkpoint at any height > 0 and writes a \
         correct anchor, so it cannot arrive here",
        verdict.err()
    );

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// 4 — enforcement, not just availability
// ---------------------------------------------------------------------------

/// The test that separates a real fix from a fix that only restores the root.
///
/// A recovered node must not merely *start* — it must still reject a block from
/// a key the genesis set does not authorise. Before the anchor, the recovered
/// history is empty and `verify_block_producer_authorized` returns `Ok(())` for
/// this same rogue block, because an empty set means "unenforced".
#[tokio::test]
async fn wal_recovered_node_still_rejects_an_unauthorized_producer() {
    let (db_path, producer_state, member) = persist_fresh_cut_chain_at_height_1().await;

    let storage = Storage::open(&db_path, NEVER_CHECKPOINT).expect("reopen storage");
    let recovered = storage
        .load_state()
        .expect("load_state must succeed")
        .expect("a persisted chain must be found");

    // A block signed by a key that is NOT in the genesis set, paying its own
    // address — rejected solely for non-membership.
    let (rogue_sk, rogue_pk) = keypair(9_999);
    let rogue_block = signed_block_1(
        &rogue_sk,
        &payout_of(&rogue_pk),
        &producer_state.state_root.to_string(),
    );

    let verdict =
        verify_block_producer_authorized(&rogue_block, &recovered.state.producer_set_history);
    assert!(
        verdict.is_err(),
        "a WAL-recovered node must still ENFORCE producer authority; got Ok(()), \
         which is the silent-downgrade reading of #2501 — the seeded member is \
         {} and the rogue key is not in the set",
        member.producer_id
    );

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// 5 — no new failure mode for a chain that never had genesis-only state
// ---------------------------------------------------------------------------

/// A chain whose genesis state is plain `State::new()` must recover exactly as
/// it does today. The anchor has to be an addition, not a behavior change, or it
/// bricks every legacy and dev chain in the name of fixing one.
#[tokio::test]
async fn unseeded_chain_is_unaffected() {
    let db_path = tmp_db();
    let mut storage = Storage::open(&db_path, NEVER_CHECKPOINT).expect("open storage");

    let (sk, pk) = keypair(7);
    let payout = payout_of(&pk);
    let unseeded = State::new();
    let mut producer_state = unseeded.clone();
    producer_state
        .apply_transitions_batch(vec![StateTransition::ApplyReward {
            address: Address::from_hex(&payout).expect("payout parses"),
            amount: Balance(5_000),
            reason: "#2501 fixture: unseeded chain".to_string(),
        }])
        .expect("transitions apply");
    producer_state.apply_block_finalization(1);

    // No `persist_genesis_state` call — this is the pre-existing shape.
    let block = signed_block_1(&sk, &payout, &producer_state.state_root.to_string());
    storage
        .persist_block(block, vec![StateTransition::ApplyReward {
            address: Address::from_hex(&payout).expect("payout parses"),
            amount: Balance(5_000),
            reason: "#2501 fixture: unseeded chain".to_string(),
        }], None)
        .await
        .expect("persist block 1");

    // redb is single-writer per file: the recovery handle cannot open until this
    // one is closed, exactly as a restart closes the process's handle.
    drop(storage);
    let reopened = Storage::open(&db_path, NEVER_CHECKPOINT).expect("reopen storage");
    let recovered = reopened
        .load_state()
        .expect("load_state must succeed")
        .expect("a persisted chain must be found");

    assert!(
        recovered.state.producer_set_history.is_empty(),
        "a chain that never seeded a producer set must still recover with none — \
         the anchor must not invent one"
    );
    assert_eq!(
        recovered.state.state_root.to_string(),
        producer_state.state_root.to_string(),
        "an unseeded chain must still replay to the producer's root"
    );

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// 6 — the anchor must outlive checkpoint pruning
// ---------------------------------------------------------------------------

/// Both checkpoint writers prune with `h < height`, which would delete a
/// height-0 anchor the first time a real checkpoint lands. That would make the
/// fix silently decay back into the defect at height 100 — and the WAL is pruned
/// at the same moment, so the anchor's loss would only surface on a much later
/// restart. Pinned here because a `h < height` prune reads entirely reasonable.
#[tokio::test]
async fn checkpoint_pruning_preserves_the_genesis_anchor() {
    let (member_sk, member_pk) = keypair(3);
    let member = member_of(&member_pk, "n5-a");
    let genesis = seeded_genesis(&member);
    let producer_state = producer_state_after_block_1(&genesis, &member);

    let db_path = tmp_db();
    // Checkpoint on EVERY block, so block 1 immediately triggers the prune path.
    let mut storage = Storage::open(&db_path, 1).expect("open storage");
    storage
        .persist_genesis_state(&genesis)
        .await
        .expect("genesis anchor must persist");

    let block = signed_block_1(
        &member_sk,
        &member.payout_address,
        &producer_state.state_root.to_string(),
    );
    storage
        .persist_block(block, block_1_transitions(&member), Some(
            bincode::serialize(&producer_state).expect("state serializes"),
        ))
        .await
        .expect("persist block 1 with checkpoint");

    // A shutdown checkpoint runs the second prune loop.
    storage
        .force_checkpoint(&producer_state, 1)
        .await
        .expect("force checkpoint");

    // See the note in `unseeded_chain_is_unaffected`: redb is single-writer.
    drop(storage);
    let reopened = Storage::open(&db_path, 1).expect("reopen storage");
    let anchor = reopened
        .load_genesis_state()
        .expect("genesis anchor read must succeed");
    assert!(
        anchor.is_some(),
        "the height-0 genesis anchor must survive both checkpoint prune loops — \
         it is the only reconstruction path for genesis-only state, and losing it \
         restores #2501 at the first checkpoint"
    );
    assert_eq!(
        anchor.expect("checked is_some above").producer_set_history.len(),
        1,
        "the surviving anchor must still carry the seeded producer set"
    );

    let _ = std::fs::remove_file(&db_path);
}
