#![no_main]

use libfuzzer_sys::fuzz_target;
use wrfcoin_shared_protocol::blockchain::transaction::Transaction;

fuzz_target!(|data: &[u8]| {
    if let Ok(tx) = serde_json::from_slice::<Transaction>(data) {
        // If deserialization succeeds, exercise deterministic methods.
        // Any panic here is a bug.
        let _ = tx.calculate_hash();
        let _ = tx.validate_structure();
    }
});
