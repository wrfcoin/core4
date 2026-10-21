#![no_main]

use libfuzzer_sys::fuzz_target;
use wrfcoin_privacy::range_proof::RangeProof;

fuzz_target!(|data: &[u8]| {
    if let Ok(proof) = serde_json::from_slice::<RangeProof>(data) {
        // If deserialization succeeds, structure verification must not panic.
        let _ = proof.verify_structure();
    }
});
