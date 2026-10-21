#![no_main]

use libfuzzer_sys::fuzz_target;
use wrfcoin_shared_protocol::blockchain::block::Block;

fuzz_target!(|data: &[u8]| {
    if let Ok(block) = serde_json::from_slice::<Block>(data) {
        // If deserialization succeeds, hash computation must not panic.
        let _ = block.calculate_hash();
    }
});
