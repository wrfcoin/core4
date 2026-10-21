#![no_main]

//! Fuzz target for the full gossipsub inbound message pipeline.
//!
//! Every raw gossipsub payload received from an untrusted peer goes through
//! three steps in the P2P node event loop (node.rs ~line 1136):
//!
//!   1. `WrfcoinMessage::from_bytes(data)` — bincode deserialization
//!   2. `message.validate_inline_size(inline_data_max)` — prevent oversized
//!      inline data blobs that bypass gossipsub's `max_transmit_size` guard
//!   3. `message.topic(&tm)` — route to the correct gossipsub topic
//!
//! `fuzz_p2p_message` (issue #946) covers step 1 in isolation.  This target
//! exercises all three steps together, including:
//!   - `ContentRef::validate` (called from `validate_inline_size` for
//!     `WeatherDataV2` and `ForecastTaskResultV2` variants)
//!   - The `TopicManager` dispatch table across all `WrfcoinMessage` variants
//!   - Both a tight inline limit (32 bytes) and an unlimited inline limit,
//!     so the fuzzer explores both rejection and acceptance branches
//!
//! All three steps must be total: no panic, no `unwrap`, for any byte sequence.
//!
//! Run (requires cargo-fuzz / nightly):
//!   cargo +nightly fuzz run fuzz_p2p_gossipsub_message_parse -- -max_total_time=60

use libfuzzer_sys::fuzz_target;
use wrfcoin_p2p::message::{TopicManager, WrfcoinMessage};

fuzz_target!(|data: &[u8]| {
    let Ok(msg) = WrfcoinMessage::from_bytes(data) else {
        // Deserialization error: expected for most random byte sequences.
        // No panic is the only requirement here.
        return;
    };

    // Step 2a: tight inline limit — exercises the rejection branch and
    // ContentRef::validate for offload variants.
    let _ = msg.validate_inline_size(32);

    // Step 2b: unlimited inline limit — exercises the acceptance branch and
    // the ContentRef::validate path for messages that carry a content_ref.
    let _ = msg.validate_inline_size(usize::MAX);

    // Step 3: topic routing — must map every valid WrfcoinMessage to a known
    // TopicHash without panic, including the FinalityVote / FinalityCertificate
    // variants added in #1388.
    let tm = TopicManager::new();
    let hash = msg.topic(&tm);
    // topic_name is informational; the important thing is topic() didn't panic.
    let _ = tm.topic_name(&hash);
});
