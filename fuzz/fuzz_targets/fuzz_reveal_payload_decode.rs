#![no_main]
//! Fuzz the contribution/forecast RevealPayload decoder — attacker-controlled
//! bytes submitted to /transactions/contribution/reveal. A panic on decode or on
//! the deterministic validation helpers is a security finding (#2107, #2111).
//!
//! # Two entry paths, because the payload has two front doors
//!
//! **Path A — `Transaction::parse_reveal_payload()`.** This is how the payload
//! actually reaches consensus: as a `data` *string* inside a submitted
//! `Transaction`. Driving it exercises three things the bare deserializer does
//! not — the `tx_type` gate (`ContributionReveal`/`ForecastReveal`), the nested
//! string-inside-JSON decode, and the memoized `OnceCell` (`parsed_reveal`,
//! #1378) that the consensus reveal-verify path hits 14+ times per transaction.
//!
//! **Path B — the bare `RevealPayload`.** Kept because the DTO layer decodes
//! this shape directly, and because it stays reachable on inputs that are not
//! valid `Transaction`s.
//!
//! # Why the seeds matter here more than usual
//!
//! Both paths are guarded by a successful decode, and `RevealPayload` has six
//! required fields — for path A, nested inside a `Transaction` that has its own
//! required fields. Random bytes essentially never produce either, so without a
//! seed corpus this target fuzzes a `serde_json` early return. Seeds live in
//! `fuzz-corpus/fuzz_reveal_payload_decode/` and are replayed on every PR by
//! the `corpus-replay` gate (#2110).

use libfuzzer_sys::fuzz_target;
use wrfcoin_shared_protocol::blockchain::transaction::{RevealPayload, Transaction};

fuzz_target!(|data: &[u8]| {
    // Path A: the real attacker path — payload nested in a submitted tx.
    if let Ok(tx) = serde_json::from_slice::<Transaction>(data) {
        if let Some(payload) = tx.parse_reveal_payload() {
            let _ = payload.validate_cid_fields();
            let _ = payload.validate_provenance_fields();
            let _ = payload.is_cid_referenced();
        }
        // Second call reads the populated memo cell rather than re-parsing.
        // Both branches of the OnceCell must be panic-free.
        let _ = tx.parse_reveal_payload();
    }

    // Path B: the bare payload, as the DTO layer decodes it.
    if let Ok(payload) = serde_json::from_slice::<RevealPayload>(data) {
        // Deterministic, key-free validation paths must never panic.
        let _ = payload.validate_cid_fields();
        let _ = payload.validate_provenance_fields();
        let _ = payload.is_cid_referenced();
    }
});
