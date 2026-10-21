//! #2226 item 6 — WAL-replay parity across **signed** blocks.
//!
//! The producer/applier parity suites cover signed blocks over the wire, and
//! `oracle_registration_wal_replay_2055.rs` covers WAL replay of an unsigned
//! one. Nothing covered the intersection: a block carrying
//! `producer_pubkey` + `producer_sig`, through persistence and recovery.
//!
//! Two things could go wrong there, and neither would be visible to any existing
//! test:
//!
//!   1. **The signature fields are `#[serde(default)]`.** If the persisted
//!      encoding omitted them, they would come back all-zero / empty with no
//!      error — a recovered chain whose blocks are silently unauthenticated.
//!   2. **The genesis producer set is seeded into `State`, not applied as a
//!      transition** (`State::seed_genesis_producer_set`, called from
//!      `main.rs`'s fresh-genesis branch only). Pure WAL replay reconstructs
//!      state by replaying transitions from a default genesis, so if nothing
//!      carries the seeded set it would be absent after recovery — and since the
//!      enforcement gate treats an empty set as *unenforced*, producer authority
//!      would silently switch OFF on restart. `main.rs` takes the
//!      `Ok(Some(chain))` branch on restart and does not re-seed.
//!
//! # Result: (1) is fine on both routes. (2) is a REAL GAP on pure WAL replay.
//!
//! | recovery route | signature fields | seeded producer set | state_root |
//! |---|---|---|---|
//! | checkpoint | survive | survives | matches |
//! | pure WAL replay | survive | **LOST** (`history_len == 0`) | does not match |
//!
//! The cause is structural, not a serialization bug: `producer_set_history` is
//! written into genesis `State` directly and is **not** carried by any
//! `StateTransition`, so there is nothing in the WAL for replay to apply. No
//! amount of transition content changes this — it is absent from the log by
//! construction. The checkpoint route works because a checkpoint serializes
//! `State` whole.
//!
//! **Why it matters.** `main.rs` seeds the set only on its fresh-genesis branch
//! (`Ok(None)`); on restart it takes the loaded state as-is. So a node recovered
//! by WAL replay holds an empty set, and the gate reads an empty set as "this
//! chain never configured producer auth" — producer authority is *unenforced*.
//! `verify_persisted_blocks_authorized` also short-circuits on an empty history,
//! so the startup boot check returns `Ok(0)` and **passes vacuously**.
//!
//! **Reachability.** `DEFAULT_CHECKPOINT_INTERVAL` is 100, so heights 1..99 have
//! no checkpoint to recover from. At the cut's genesis block time this is not a
//! narrow window.
//!
//! ## SEVERITY: SETTLED — it is fail-closed at V2, and V2 only
//!
//! The open question was whether production actually *starts* in the degraded
//! state. It does not. `main.rs:1466` runs
//! `verify_tip_state_root_commitment(&blockchain)` immediately after the
//! protocol gate installs the signing version, and turns any `Err` into a
//! startup refusal. Executed on both routes:
//!
//! | route | startup verdict |
//! |---|---|
//! | checkpoint | `Ok` — node starts |
//! | pure WAL replay | `Err(EXIT_STATE_ROOT_COMMITMENT_MISMATCH)` — node **REFUSES TO START** |
//!
//! So #2501 is **not** a silent authentication downgrade. It is fail-closed, and
//! the residual is **availability**: a node whose only recovery route is a
//! checkpoint-less WAL cannot restart, and must resync from a peer. Loud, and a
//! much lower priority than a chain that comes up unenforced.
//!
//! **The guarantee is V2-only.** `verify_tip_state_root_commitment_at` returns
//! `Ok(())` below V2, and a V1 producer stamps no commitment at all (V1 hashes
//! are frozen). Asserted here: the same degraded chain **starts cleanly at V1**.
//! That matters because V2 activates only via the #2034 cut, while a producer set
//! is seeded today by `WRFCOIN_NETWORK_ID=testnet` — so a *pre-cut* node has the
//! loss with none of the detection. That is the one place the silent-downgrade
//! reading still holds, and it is pre-cut only.
//!
//! ## Attribution, stated precisely because a mutation caught me overclaiming
//!
//! These fixtures show the WAL-recovered chain is *refused*; they do **not**
//! show it is refused *because* the producer set is missing. They persist a
//! coinbase-only block with an EMPTY transition vector, so replay reconstructs
//! nothing and the root differs for a much larger reason. Removing
//! `producer_set_history` from `state_root_material` does not lift the WAL
//! refusal — which is the proof that these tests cannot isolate the set.
//!
//! The set's contribution to the root is therefore proven separately and
//! directly by `producer_set_is_folded_into_state_root`, which clears only that
//! field and recomputes. That test DOES fail under the same mutation. The two
//! together support the structural claim; neither does alone.
//!
//! The WAL-replay test is a **characterisation**: it asserts the loss, the
//! `Ok(())` producer-gate verdict that would make the downgrade silent, the
//! startup refusal that stops it mattering, and the V1 boundary of that refusal.
//! It flips to the positive assertion the moment the gap is closed.
//!
//! Run: cargo test -p wrfcoin-native-chain --test producer_signed_wal_parity_2226

use std::sync::Arc;

use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::RwLock;

use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::storage::Storage;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::producer_authority::verify_block_producer_authorized;
use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;
use wrfcoin_shared_protocol::mempool::pool::Mempool;

/// Never checkpoints, so recovery is PURE WAL replay from genesis.
const NEVER_CHECKPOINT: u64 = u64::MAX / 2;
/// Checkpoints every block, so recovery comes from the checkpoint.
const ALWAYS_CHECKPOINT: u64 = 1;

fn tmp_db() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("wrfcoin-2226-wal-{}.redb", fastrand::u64(..)))
}

/// V2 is the version the #2034 cut activates, and the only one under which
/// producer signatures are verified in production. Asserted, not assumed.
fn v2() {
    let installed = SigningProtocolVersion::install_active(SigningProtocolVersion::V2);
    assert_eq!(
        installed,
        SigningProtocolVersion::V2,
        "this binary must run at V2 — got {installed}"
    );
}

fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
    let mut rng = StdRng::seed_from_u64(seed);
    let sk = PrivateKey::generate(&mut rng);
    let pk = PublicKey::from_private_key(&sk);
    (sk, pk)
}

/// Canonical payout `Address` (0x-hex) — the coinbase form the mining route
/// accepts (#2290 §R2).
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

/// Produce block 1 through the production path on a chain whose genesis has the
/// producer set seeded — the shape `main.rs` builds for a fresh cut chain.
///
/// Returns the post-block chain (height 1) so the caller can persist it.
async fn produce_signed_block_1(sk: &PrivateKey, member: &ProducerMember) -> Blockchain {
    v2();
    let mut genesis = Blockchain::default();
    genesis
        .state
        .seed_genesis_producer_set(vec![member.clone()])
        .expect("genesis producer set must seed");
    genesis.state.recalculate_state_root();

    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    // #2523: a test chain with no oracle ceremony must SAY so — silence at
    // height 1 is refused, because it cannot be told from a misconfigured cut.
    pop.declare_no_genesis_oracles();
    pop.set_producer_signer(Arc::new(EnvKeySigner::from_private_key(
        sk.clone(),
        "TEST_PRODUCER_KEY",
    )));

    let chain = Arc::new(RwLock::new(genesis));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));

    // Passes `create_next_block`'s own append-boundary authority gate precisely
    // because the signer IS the seeded member and the coinbase pays its stable
    // payout address — so this is the authorized-producer path, not a bypass.
    let block = pop
        .create_next_block(Arc::clone(&chain), mempool, &member.payout_address)
        .await
        .expect("the seeded member must be able to produce block 1");

    assert_ne!(
        block.producer_pubkey, [0u8; 32],
        "precondition: the produced block must carry a stamped producer key"
    );
    assert!(
        !block.producer_sig.is_empty(),
        "precondition: the produced block must carry a producer signature"
    );

    let guard = chain.read().await;
    assert_eq!(guard.height, 1, "the producer must have appended block 1");
    guard.clone()
}

/// The shared body: persist the signed block under `interval`, recover, and
/// assert every property that has to survive.
async fn assert_signed_block_survives_recovery(
    interval: u64,
    label: &str,
    seeded_set_survives: bool,
) {
    let (sk, pk) = keypair(2_226_601);
    let member = member_of(&pk, "n5-node-a");

    let produced = produce_signed_block_1(&sk, &member).await;
    let producer_root = produced.state.state_root.0;
    let block_1 = produced
        .block_at_height(1)
        .expect("block 1 must be in the producer's window")
        .clone();
    let signed_pubkey = block_1.producer_pubkey;
    let signed_sig = block_1.producer_sig.clone();

    // Sanity: the produced block authenticates BEFORE any persistence, so a
    // failure after recovery is attributable to the round-trip.
    assert_eq!(
        verify_block_producer_authorized(&block_1, &produced.state.producer_set_history),
        Ok(()),
        "[{label}] precondition: the produced block must authenticate pre-restart"
    );

    let path = tmp_db();
    {
        let mut storage = Storage::open(&path, interval).expect("open storage");
        let checkpoint = storage
            .checkpoint_state_bytes(1, &produced.state)
            .expect("checkpoint_state_bytes");
        if interval == NEVER_CHECKPOINT {
            assert!(
                checkpoint.is_none(),
                "[{label}] this interval must never checkpoint, or the test is \
                 not exercising pure WAL replay"
            );
        } else {
            assert!(
                checkpoint.is_some(),
                "[{label}] this interval must checkpoint every block"
            );
        }
        // Transitions are empty: a coinbase-only block carries none, which keeps
        // the test about the BLOCK's signature fields and the seeded set rather
        // than about transition replay (covered by #2055's WAL test).
        storage
            .persist_block(block_1.clone(), Vec::new(), checkpoint)
            .await
            .expect("persist_block");
    }

    let recovered = Storage::open(&path, interval)
        .expect("re-open storage")
        .load_state()
        .expect("load_state must not error")
        .expect("load_state must return a chain after a block was written");
    let _ = std::fs::remove_file(&path);

    assert_eq!(recovered.height, 1, "[{label}] recovered tip must be block 1");

    // ---- 1. the signature fields survived -----------------------------------
    let recovered_block = recovered
        .block_at_height(1)
        .expect("[{label}] recovered chain must hold block 1");
    assert_eq!(
        recovered_block.producer_pubkey, signed_pubkey,
        "[{label}] producer_pubkey must survive persistence. Both signature \
         fields are #[serde(default)], so an encoding that omitted them would \
         return all-zero here with no error — and every recovered block would \
         then fail authentication on a chain that had been producing valid ones"
    );
    assert_eq!(
        recovered_block.producer_sig, signed_sig,
        "[{label}] producer_sig must survive persistence"
    );
    assert_eq!(
        recovered_block.hash, block_1.hash,
        "[{label}] the recovered block's sealed hash must be unchanged — \
         producer_pubkey folds into it, so a dropped key would move the identity"
    );

    // ---- 2. the seeded producer set survived --------------------------------
    // The genesis set is NOT a transition: it is written straight into genesis
    // `State` by `seed_genesis_producer_set`, which `main.rs` calls only on the
    // fresh-genesis branch. On restart `main.rs` takes the loaded state as-is and
    // does not re-seed, so if recovery loses the set, producer authority becomes
    // UNENFORCED after a reboot — silently, because an empty set is the
    // "not configured" signal.
    // ---- 0. WHAT PRODUCTION STARTUP ACTUALLY DOES ---------------------------
    // `main.rs:1466` runs this immediately after the protocol gate installs the
    // signing version, and turns any `Err` into a refusal to start. Calling the
    // version-explicit variant at V2 is exactly the production contract; the
    // `active()` wrapper would read the same value here.
    let startup_verdict = wrfcoin_native_chain::storage::verify_tip_state_root_commitment_at(
        SigningProtocolVersion::V2,
        &recovered,
    );
    eprintln!(
        "S6 [{label}] startup tip-commitment verdict: {}",
        match &startup_verdict {
            Ok(()) => "Ok — node STARTS".to_string(),
            Err(e) => format!("Err({}) — node REFUSES TO START", e.error_code()),
        }
    );

    if !seeded_set_survives {
        // CHARACTERISATION — see the module docs. This route cannot reconstruct
        // the seeded set, and the assertions below record exactly that rather
        // than a fix that does not exist yet.
        assert!(
            recovered.state.producer_set_history.is_empty(),
            "[{label}] this route is expected to LOSE the seeded producer set; \
             if it now survives, the gap was fixed and this test should become \
             the positive assertion the checkpoint route already makes"
        );
        assert_eq!(
            verify_block_producer_authorized(
                recovered_block,
                &recovered.state.producer_set_history
            ),
            Ok(()),
            "[{label}] the recovered block 'passes' the producer gate only \
             because the empty set reads as 'producer auth not configured' — the \
             verdict is Ok for the wrong reason. What stops that mattering is the \
             startup check asserted next, NOT this gate"
        );

        // THE SEVERITY QUESTION, SETTLED BY EXECUTION.
        //
        // At V2 the producer stamps its post-state root into hashed block
        // metadata and `verify_tip_state_root_commitment` compares the recovered
        // root against it, so `main.rs:1466` turns this degraded state into a
        // refusal to start. FAIL-CLOSED: a node cannot come up unenforced by
        // this route.
        //
        // ATTRIBUTION, stated precisely because a mutation caught me
        // overclaiming it: this assertion shows the recovered chain is refused,
        // NOT that it is refused *because* the producer set is missing. In these
        // fixtures WAL replay reconstructs nothing at all (empty transition
        // vector), so the root differs for a larger reason. Removing
        // `producer_set_history` from `state_root_material` does not lift this
        // refusal — proof that the test alone does not isolate the set.
        //
        // The set's contribution to the root is proven separately and directly
        // by `producer_set_is_folded_into_state_root`, below. The two together
        // support the structural claim; neither does on its own.
        let err = startup_verdict.expect_err(
            "[{label}] a WAL-recovered chain that lost its producer set MUST fail \
             the tip state-root commitment check. If this ever returns Ok, the \
             producer set has stopped contributing to state_root, or the \
             commitment is absent — and in either case the loss becomes a SILENT \
             authentication downgrade instead of a loud refusal",
        );
        assert_eq!(
            err.error_code(),
            "EXIT_STATE_ROOT_COMMITMENT_MISMATCH",
            "[{label}] and it must fail for the right reason — a different code \
             means the refusal is incidental rather than the commitment gate"
        );

        // THE SCOPE OF THAT GUARANTEE: it is V2-ONLY.
        //
        // `verify_tip_state_root_commitment_at` returns `Ok(())` for any version
        // below V2, and at V1 the producer does not stamp the commitment at all
        // (V1 hashes are frozen, so adding a metadata key would move every one).
        // So the SAME degraded chain starts cleanly at V1.
        //
        // That matters because V2 activates only via the #2034 cut, while a
        // producer set is seeded today by `WRFCOIN_NETWORK_ID=testnet`. A
        // pre-cut node therefore has the loss with none of the detection.
        // Asserted rather than reasoned about, so the boundary is a checked fact.
        assert_eq!(
            wrfcoin_native_chain::storage::verify_tip_state_root_commitment_at(
                SigningProtocolVersion::V1,
                &recovered,
            ),
            Ok(()),
            "[{label}] at V1 the same degraded chain must START — the commitment \
             gate is V2-only, so the fail-closed property does NOT extend to a \
             pre-cut node that seeded a producer set"
        );
        return;
    }

    assert!(
        !recovered.state.producer_set_history.is_empty(),
        "[{label}] the genesis producer set must survive recovery. An empty \
         history here means producer authority is UNENFORCED after restart: \
         main.rs re-seeds only on a fresh genesis, and the gate reads an empty \
         set as 'this chain never configured producer auth' (#2290)"
    );
    assert_eq!(
        recovered.state.producer_set_history, produced.state.producer_set_history,
        "[{label}] the recovered producer set must be byte-identical — it is \
         folded into state_root, so any drift forks the chain"
    );

    // ---- 3. state_root parity ----------------------------------------------
    assert_eq!(
        recovered.state.state_root.0, producer_root,
        "[{label}] producer and recovered state_root must match"
    );

    // ---- 4. the recovered block STILL AUTHENTICATES -------------------------
    // The end-to-end property: not merely that the bytes round-tripped, but that
    // the chain a restarted node holds still satisfies the gate its peers apply.
    assert_eq!(
        verify_block_producer_authorized(
            recovered_block,
            &recovered.state.producer_set_history
        ),
        Ok(()),
        "[{label}] the recovered block must still authenticate against the \
         recovered producer set. This is the property the boot check \
         (verify_persisted_producer_authority) enforces at startup — failing it \
         means a node that produced valid blocks refuses to restart"
    );

    // ---- 5. and production startup accepts it -------------------------------
    assert!(
        startup_verdict.is_ok(),
        "[{label}] a correctly recovered chain must PASS the tip state-root \
         commitment check, or the refusal on the other route proves nothing \
         about the producer set — it would just mean the check rejects every \
         recovery: {startup_verdict:?}"
    );
}

/// Recovery from a checkpoint: `State` is deserialized whole, so the seeded
/// producer set comes back with it.
#[tokio::test]
async fn signed_block_1_survives_checkpoint_recovery_at_v2() {
    assert_signed_block_survives_recovery(ALWAYS_CHECKPOINT, "checkpoint", true).await;
}

/// Recovery by **pure WAL replay** — no checkpoint. This is the route with real
/// exposure to the seeded-set question, because replay rebuilds state by
/// applying transitions from a default genesis rather than by loading a
/// serialized `State`.
#[tokio::test]
async fn signed_block_1_survives_pure_wal_replay_at_v2() {
    assert_signed_block_survives_recovery(NEVER_CHECKPOINT, "pure-wal-replay", false).await;
}

/// The mechanism behind the refusal above, isolated: `producer_set_history`
/// contributes to `state_root`, so a state that differs *only* by having lost it
/// necessarily hashes differently.
///
/// This is the assertion that makes the WAL refusal structural rather than
/// incidental. It is separate because the WAL fixtures cannot isolate it — their
/// recovered state is empty for unrelated reasons — and a mutation removing the
/// producer set from `state_root_material` proved exactly that by failing to
/// change the WAL verdict.
#[tokio::test]
async fn producer_set_is_folded_into_state_root() {
    let (sk, pk) = keypair(2_226_601);
    let member = member_of(&pk, "n5-node-a");

    let produced = produce_signed_block_1(&sk, &member).await;
    let with_set = produced.state.state_root.0;
    assert!(
        !produced.state.producer_set_history.is_empty(),
        "precondition: the produced state must carry the seeded set"
    );

    // Same state, producer set removed, root recomputed. Nothing else changes.
    let mut without_set = produced.state.clone();
    without_set.producer_set_history.clear();
    without_set.recalculate_state_root();

    assert_ne!(
        without_set.state_root.0, with_set,
        "losing the producer set MUST move state_root. If it does not, the set is \
         outside the hashed material and the loss becomes undetectable by the tip \
         commitment check — turning the WAL gap from a loud refusal into a silent \
         authentication downgrade (#2501)"
    );
}
