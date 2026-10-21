#![no_main]
//! Fuzz the OracleAttestationPayload decoder — attacker-controlled bytes
//! submitted to /transactions/oracle/attestation. A panic on decode or on the
//! deterministic signing-message construction is a security finding (#2107).
//!
//! # Two entry paths, because the payload has two front doors
//!
//! **Path A — `Transaction::parse_oracle_attestation_payload()`.** How the
//! payload actually reaches consensus: a `data` *string* inside a submitted
//! `Transaction`. Exercises the `tx_type` gate (`OracleAttestation`), the
//! nested string-inside-JSON decode, and the memoized `OnceCell`
//! (`parsed_oracle_attestation`, #1378) — none of which the bare deserializer
//! touches.
//!
//! **Path B — the bare `OracleAttestationPayload`,** as the DTO layer decodes
//! it, and so this target keeps working on inputs that are not valid
//! `Transaction`s.
//!
//! `canonical_signing_message()` is the interesting callee: it is the preimage
//! an oracle signature commits to, so a panic there is reachable by any
//! attacker who can submit an attestation. `is_signable()` guards the
//! non-finite `f64` hazard that `payload_digest_hex` documents — `confidence`
//! is an `Option<f64>` and JSON can carry values that break float assumptions.
//!
//! # Why the seeds matter here more than usual
//!
//! Both paths are guarded by a successful decode. Random bytes essentially
//! never produce one, so without a seed corpus this target fuzzes a
//! `serde_json` early return. Seeds live in
//! `fuzz-corpus/fuzz_oracle_attestation_decode/` and are replayed on every PR
//! by the `corpus-replay` gate (#2110).

use libfuzzer_sys::fuzz_target;
use wrfcoin_shared_protocol::blockchain::transaction::{OracleAttestationPayload, Transaction};

fuzz_target!(|data: &[u8]| {
    // Path A: the real attacker path — payload nested in a submitted tx.
    if let Ok(tx) = serde_json::from_slice::<Transaction>(data) {
        if let Some(payload) = tx.parse_oracle_attestation_payload() {
            let _ = payload.canonical_signing_message();
            let _ = payload.is_signable();
        }
        // Second call reads the populated memo cell rather than re-parsing.
        let _ = tx.parse_oracle_attestation_payload();
    }

    // Path B: the bare payload, as the DTO layer decodes it.
    if let Ok(payload) = serde_json::from_slice::<OracleAttestationPayload>(data) {
        // Deterministic, key-free paths must never panic.
        let _ = payload.canonical_signing_message();
        let _ = payload.is_signable();
    }
});
