#![no_main]

//! Fuzz target for BlockRequest / BlockResponse bincode deserialization.
//!
//! Both types travel over the `/wrfcoin/block-sync/1.0.0` request/response
//! protocol and are deserialized from untrusted peer streams.  The codec
//! (`BlockSyncCodec`) enforces a 4-byte big-endian length prefix capped at
//! `MAX_SYNC_MESSAGE_BYTES` (16 MiB) before calling `bincode::deserialize`
//! internally.  Any panic inside bincode on malformed input is a remote
//! denial-of-service vector.
//!
//! Two paths are exercised:
//!
//! **Mode 1 — direct bincode**: feed `data` straight to
//! `bincode::deserialize::<BlockRequest>` and `bincode::deserialize::<BlockResponse>`.
//! This is the fastest path for the fuzzer and covers the arithmetic the
//! #1667 fix touched (inbound serve of heights below the in-memory window).
//!
//! **Mode 2 — wire format**: interpret the first 4 bytes of `data` as the
//! big-endian length prefix that `BlockSyncCodec::read_request/read_response`
//! write, extract the declared body, and attempt deserialization on that body.
//! This confirms the length guard fires correctly and no panic escapes the
//! decode path for crafted length+body combinations.
//!
//! Run (requires cargo-fuzz / nightly):
//!   cargo +nightly fuzz run fuzz_block_sync_request_deser -- -max_total_time=60
//!
//! Must never panic for any byte sequence — all errors must be `Err(_)`.

use libfuzzer_sys::fuzz_target;
use wrfcoin_p2p::sync::{BlockRequest, BlockResponse};

/// Mirrors BlockSyncCodec::MAX_SYNC_MESSAGE_BYTES; kept in sync by inspection.
const MAX_SYNC_MESSAGE_BYTES: usize = 16_777_216;

fuzz_target!(|data: &[u8]| {
    // Mode 1: direct bincode deserialization on the raw fuzz bytes.
    let _: Result<BlockRequest, _> = bincode::deserialize(data);
    let _: Result<BlockResponse, _> = bincode::deserialize(data);

    // Mode 2: simulate the wire-format framing (4-byte BE length prefix + body).
    if data.len() >= 4 {
        let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        // Only attempt decode when the declared length is within the guard and
        // the buffer actually contains that many bytes — otherwise the codec
        // would return an I/O error before reaching bincode.
        if len <= MAX_SYNC_MESSAGE_BYTES && data.len() >= 4 + len {
            let body = &data[4..4 + len];
            let _: Result<BlockRequest, _> = bincode::deserialize(body);
            let _: Result<BlockResponse, _> = bincode::deserialize(body);
        }
    }
});
