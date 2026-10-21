# Crypto / ZK / Privacy Audit — 2026-06-20

Branch: `iris/crypto-zk-audit-r6` (based on `origin/main`)
Scope: `shared-protocol/src/crypto/`, `zk-circuits/src/`, `privacy/src/`, `native-chain/zk-params/`

## Findings

| # | File : Line | Severity | Status | Summary | Fix sketch |
|---|------------|----------|--------|---------|------------|
| 1 | `zk-circuits/src/circuits/weather_model.rs:80-118` | HIGH | REAL | `execution_time_secs` allocated as public input but **not absorbed into Poseidon sponge**. Circuit only checks `time != 0`. Prover can claim any non-zero execution time while the constraint set only covers `Poseidon(samples \|\| model_id \|\| input_hash) == output_hash`. Protocol comment says this field verifies "resource usage" — it does not. | Absorb `time_var` into the Poseidon sponge before squeezing output. **Requires key regeneration → flag for sign-off**. |
| 2 | `shared-protocol/src/crypto/domain_signing.rs:61` + `shared-protocol/src/blockchain/transaction.rs:801` | MEDIUM-HIGH | REAL | `payload_digest_hex` calls `serde_json::to_vec(payload).unwrap_or_default()`. `OracleAttestationSigningPayload.confidence: Option<f64>` can hold `f64::NAN` or `f64::INFINITY`. `serde_json` rejects non-finite floats with an error; `unwrap_or_default()` silently substitutes `hash(b"")`. Two distinct payloads with different non-finite confidence values produce the **same signing digest** for identical metadata — cross-payload signature validity. Applied fix: `expect()` replaces `unwrap_or_default()` (see commit). | Long-term: validate `confidence` before signing (`filter(|c| c.is_finite())`); change `payload_digest_hex` signature to `Result<String, serde_json::Error>`. |
| 3 | `native-chain/zk-params/confidential-transfer-1-1-64.bin` + `zk-circuits/src/circuits/confidential_transfer.rs:184-188` | CRITICAL | REAL | Embedded proving/verifying key was generated at commit `62d5d628d` (**before** the blinding conservation fix in `61c6571b5`, which closed #786). `git log --all -- native-chain/zk-params/confidential-transfer-1-1-64.bin` shows only `62d5d628d`; `git merge-base --is-ancestor 62d5d628d 61c6571b5` confirms the key predates the fix. Corroboration: `zk_service.rs:757-769` test `confidential_transfer_generate_and_verify` uses **mismatched blindings** (input=42, output=10) which only passes with the pre-fix key. Effect: either the `prove()` call panics on constraint-count mismatch (availability regression) or the old key is used to produce proofs without blinding conservation (security regression enabling value creation). | Regenerate `confidential-transfer-1-1-64.bin` from the current circuit code (which now includes the blinding conservation constraint) and update `zk_service.rs` test to use matched blindings. **Format/consensus change — requires key rotation sign-off and hard-fork coordination.** |

## MLSAG Budget Deferral (informational — not a new bug)

`privacy/src/mlsag_budget.rs` defers MLSAG ring-signature verification under wall-clock budget pressure. The deferred items are logged and are expected to be re-verified at state application. The caller in `shared-protocol/src/consensus/proof_of_publishing.rs:4251-4281` correctly logs the count. The contract ("deferred to state application") relies on `ring_sig.verify()` being called in the apply path; this was not audited here but the module comment acknowledges the dependency. No bypass found — informational only.

## Fix Applied (Finding #2 only)

`shared-protocol/src/crypto/domain_signing.rs:61`:
- Before: `serde_json::to_vec(payload).unwrap_or_default()`
- After:  `serde_json::to_vec(payload).expect("payload_digest_hex: protocol types must serialize cleanly — add is_finite() guard for any f64 fields")`

This makes the silent digest substitution LOUD without changing wire formats or consensus rules. No key regeneration required. All existing tests unaffected (no test uses NaN/Infinity in signed payloads).

## Sign-off Required

- Finding #1 (`execution_time_secs`): Circuit change + `weather-model-2.bin` key regeneration
- Finding #3 (stale CT key): `confidential-transfer-1-1-64.bin` key regeneration + hard-fork planning + `zk_service.rs` test fix
