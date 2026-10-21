#![no_main]

//! Fuzz target for Proof-of-Publishing verify_block.
//!
//! The PoP consensus module's block verification path processes untrusted block
//! metadata, transaction types, oracle signatures, and compute receipts.  Any
//! panic here is a consensus failure that could fork the chain.

use libfuzzer_sys::fuzz_target;
use rustc_hash::FxHashSet;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;

fuzz_target!(|data: &[u8]| {
    if let Ok(block) = serde_json::from_slice::<Block>(data) {
        let mut pop = ProofOfPublishing::new(1, 15, 0, 500);
        let oracle_set: FxHashSet<String> = FxHashSet::default();

        // verify_block is async — spin up a minimal tokio runtime.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // Must never panic on any input, regardless of how malformed.
        let _ = rt.block_on(pop.verify_block(&block, &oracle_set, 1, &[]));
    }
});
