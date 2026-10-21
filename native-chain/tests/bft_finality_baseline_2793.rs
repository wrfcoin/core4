//! #2793 pre-implementation baseline: block application NEVER advances raw
//! BFT finality.
//!
//! The 2026-08-12 inert audit (SHA-256 f95b12f6…) established that the
//! production `launch_api` attaches no BFT gadget (`bft_finality_for_p2p =
//! None`), so `Blockchain::finalized_height` — the value that arms the
//! reorg guard and the raw BFT gauges — can only ever move via the accepted
//! FinalitySignal arm (`set_finalized_height`). Its most dangerous
//! misreading is the inverse: treating advancing blocks as finality.
//! `wrfcoin_chain_finalized_height` (the derived metric,
//! `max(tip - MAX_REORG_DEPTH, finalized_height)`) climbs forever on a
//! healthy chain with zero BFT.
//!
//! This test pins that separation as an executable statement on the
//! production `Blockchain` type: applying real blocks — including far past
//! MAX_REORG_DEPTH — leaves raw finality at exactly zero. It is the
//! REQUIRED-GREEN baseline for the #2792/#2793 milestones: any
//! implementation change that starts advancing finality as a side effect of
//! block application (rather than through an accepted, validated finality
//! signal) turns this red, and the activation milestone must flip this test
//! DELIBERATELY, never incidentally.
//!
//! What this does NOT prove, stated plainly: it is not the release gate.
//! The public-release proof is the audit's six-point process-boundary test
//! (real gadget, real votes, real certificates, across real processes on
//! release images) — see docs/plans/2026-08-16-bft-finality-rollout-architecture.md.

use chrono::Utc;
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::crypto::hash::Hash;

fn sealed_block(height: u64, prev: Hash) -> Block {
    let mut block = Block::new(1, prev, Vec::new(), 1, height);
    block.timestamp = Utc::now();
    let hash = block.calculate_hash();
    block.set_hash(hash);
    block
}

/// Applying blocks — even hundreds, far past MAX_REORG_DEPTH (100) — must
/// leave raw BFT finality at zero. Only the finality-signal arm moves it.
#[test]
fn block_application_never_advances_raw_bft_finality() {
    let mut chain = Blockchain::default();

    let genesis = sealed_block(1, Hash::default());
    let mut prev = genesis.hash.expect("genesis hash set");
    chain.add_block(genesis).expect("genesis applies");
    assert_eq!(chain.finalized_height(), 0, "genesis must not finalize anything");

    // 250 blocks: more than 2x MAX_REORG_DEPTH, so the derived
    // `finalized_height` metric (tip - 100) would read 151 here. Raw BFT
    // finality must still read zero — the two are different claims.
    for height in 2..=251u64 {
        let block = sealed_block(height, prev);
        prev = block.hash.expect("block hash set");
        chain.add_block(block).expect("block applies");
        assert_eq!(
            chain.finalized_height(),
            0,
            "raw BFT finality moved at height {height} without a finality signal — \
             block application must never be a finality source"
        );
    }

    // The signal arm is the one legitimate mover (monotonicity is pinned by
    // the shared-protocol unit test; this is the cross-check that the arm
    // still works after 250 real applications).
    chain.set_finalized_height(200);
    assert_eq!(chain.finalized_height(), 200);
}
