# Clone-derive inventory — the six acknowledged secret-leak baseline sites (#2287)

Facts only, per the #2287 contract: for each site, (1) is the derive actually used —
every call site with `file:line` — and (2) what stops compiling if it is removed.
**No safe-to-keep verdicts here**; that judgement goes back to the manager with these
facts attached.

**Method.** Verified against `main` (2026-07-22, `docs/2287-derive-inventory` base).
Call sites were found by *actually removing each derive and compiling*
(`cargo check --all-targets`, plus a `--keep-going` workspace pass), not by grep alone —
the same call-site-first pattern that retired the three #2266 derives. Textual sweeps
(type-flow from `PrivateKey`/`KeyPair` uses, common key-variable-name clone patterns)
were run on downstream crates that a failing dependency prevented from compiling. All
experiment edits were reverted; nothing in this PR touches code.

## Summary

| # | Site | Derive | Call sites | Of which production |
|---|---|---|---:|---:|
| 1 | `shared-protocol/src/crypto/signature.rs:18` `PrivateKey` | `Clone` | **3** | 2 (both are other `#[derive(Clone)]`s, not runtime clones) |
| 2 | `consensus-engines/src/selection/vrf.rs` `VrfSecretKey` | — | **already removed on `main`** (merged #2266) | — |
| 3 | `consensus-engines/src/algorithms/hotstuff/crypto.rs:32` `HotStuffSigningKey` | `Clone` | **16** | **0** — all `#[cfg(test)]` |
| 4 | `native-chain/src/bin/launch_api/state.rs:347` `AppState` | `Clone` | **7** | **0** — all `#[cfg(test)]` |
| 5 | `native-chain/src/bin/launch_api/rate_limit.rs:188` `RateLimiterState` | `Clone` | **3** | **3** — incl. one per-request |
| 6 | `shared-protocol/src/crypto/signer.rs:100` `EnvKeySigner` | `Clone` | **0** | 0 |

Zero-call-site derives (the easy-removal class per #2266's precedent): **#6 EnvKeySigner**
(and #2 is already gone). Zero *production* call sites: #3 and #4 additionally.

---

## 1. `PrivateKey(ed25519_dalek::SigningKey)` — `shared-protocol/src/crypto/signature.rs:17–18`

**Is the derive used?** Yes — 3 compile-level dependents:

1. `shared-protocol/src/crypto/signature.rs:21–23` — **`KeyPair` derives `Clone`** and
   contains `private_key: PrivateKey`. This is a derive-site dependency, not a runtime
   clone (E0277 at the derive if removed).
2. `shared-protocol/src/crypto/signer.rs:99–101` — **`EnvKeySigner` derives `Clone`**
   over `private_key: PrivateKey` (E0277 at the derive). `EnvKeySigner` is itself
   `#[cfg(debug_assertions)]`-gated — and per site 6 below, *its* Clone has zero users.
3. `shared-protocol/src/blockchain/snapshot_sync.rs:419` — `sk.clone()` inside
   `#[test] fn snapshot_sync_signer_path_matches_private_key_path`. **Test-only** direct
   clone (E0599).

**What breaks if removed?** Exactly those three sites (compile-verified). Cascade check:
if `KeyPair` consequently lost its `Clone`, no call site clones `KeyPair` anywhere in the
workspace (the one textual hit, `p2p/src/behaviour.rs:182`, clones *libp2p's*
`identity::Keypair`, a different type). The issue's first-look note said "keypairs are
cloned into workers/services pervasively" — **the compile evidence does not support
that on current `main`**: the only launch_api "signing key clone" is
`state.snapshot_signing_key.clone()` (`main.rs:1252`), which is an
`Option<Arc<dyn BlockSigner>>` — an Arc refcount bump, not a key-material copy.

## 2. `VrfSecretKey` — `consensus-engines/src/selection/vrf.rs`

**Already resolved on `main`.** Merged PR #2266 removed the `Clone`/`Debug` derives; the
type now has a manual redacted `Debug` impl (`vrf.rs:28–31`) and a comment documenting
why (`vrf.rs:24–25`). The baseline entry self-retires when PR #2239's baseline rebases,
exactly as the issue predicted. Nothing to audit.

## 3. `HotStuffSigningKey` — `consensus-engines/src/algorithms/hotstuff/crypto.rs:31–32`

**Is the derive used?** Yes — **16 clone call sites, every one inside the
`#[cfg(test)]` module** of `consensus-engines/src/algorithms/hotstuff/mod.rs` (module
starts at `:1196`): lines 1248, 1264, 1279, 1295, 1318, 1343, 1355, 1416, 1449, 1457,
1465, 1514, 1549, 1634, 1663, 1679 — all of the shape "distribute the generated test
key to N simulated validators".

**What breaks if removed?** Only those 16 test lines (compile-verified with
`--all-targets`; no derive-site dependencies exist — no other `Clone`-deriving struct
contains a `HotStuffSigningKey`). Non-test code constructs keys via
`HotStuffSigningKey::generate` (`factory.rs:135`, `mod.rs:187`) and never clones them.
Context from the #2244 triage: the HotStuff engine itself is not constructed by the
production node (factory-constructible only).

## 4. `AppState` — `native-chain/src/bin/launch_api/state.rs:346–347`

**Is the derive used?** Yes — **7 clone call sites, all in test code**
(`main.rs` test modules; the relevant `#[cfg(test)]` starts at `main.rs:1553`;
line numbers re-verified against `main` after the #2272 test rewrite, which
shifted every cite in this section and added one site):

- `main.rs:2945`, `main.rs:2987` — snapshot-sync route tests:
  `(*test_state(...).get_ref()).clone()` to mutate a copy of the test AppState.
- `main.rs:1950`, `main.rs:6315`, `main.rs:6357` — test-helper functions building
  a modified `web::Data<AppState>` (`:1950` is `test_state_with_p2p`, added
  since the original inventory).
- `main.rs:6530`, `main.rs:6573` — `#[actix_web::test]` height-1 conflict tests, same
  pattern.

**What breaks if removed?** Only those 7 test lines (compile-verified at 6 sites on
the original audit head; `:1950` is the same `test_state` pattern). The issue's
premise "Actix state is per-worker-cloned by design" turns out **not to apply here**:
production wraps the state once in `web::Data<AppState>` (an `Arc`) and workers clone
the `Data` handle, never `AppState` itself. No full token-table copy happens in
production today; the copies happen per test invocation instead.

## 5. `RateLimiterState` — `native-chain/src/bin/launch_api/rate_limit.rs:187–188`

**Is the derive used?** Yes — **3 call sites, all production** (the file's test module
starts at `:422`; all three sit before it):

- `main.rs:1464` — `rate_limit::RateLimiter::new(rl_state.clone())` inside the
  `HttpServer::new(move || …)` app factory: **one clone per actix worker** at startup.
- `rate_limit.rs:295` — `state: self.state.clone()` in
  `Transform::new_transform` (middleware construction).
- `rate_limit.rs:330` — `let state = self.state.clone()` in `Service::call`:
  **one clone per HTTP request**.

**What breaks if removed?** All three (compile-verified) — i.e. the middleware stops
compiling entirely; this derive is load-bearing for the current design. Fact relevant to
the coming decision (recorded, not decided): what gets cloned per request is whatever
`RateLimiterState` holds — if its `token_store` is already behind an `Arc`/shared handle
the per-request clone is cheap; if not, site 5 is the one hot-path full-copy in this
inventory. (Same `token_store` shape question as site 4 — one decision covers both.)

## 6. `EnvKeySigner` — `shared-protocol/src/crypto/signer.rs:98–100`

**Is the derive used?** **No. Zero call sites.** A workspace-wide
`cargo check --workspace --all-targets --keep-going` with the derive removed produced
**zero errors**. Every consumer (`native-chain/tests/*`, `main.rs:1500`,
`snapshot_sync.rs` tests) constructs it and immediately wraps it in
`Arc<dyn BlockSigner>`; the Arc is what gets cloned.

**What breaks if removed?** Nothing, on any target, anywhere in the workspace.

---

## Hand-back questions raised by the facts (decisions owed by the manager, not taken here)

1. Site 6 has zero call sites and site 2's precedent (#2266) removed exactly this class —
   is removal approved?
2. Sites 3 and 4 have call sites only in tests. Options are remove-the-derive-and-add
   explicit test clones (e.g. regenerate/`cfg(test)` impl) vs. annotate
   `// secret-leak-ok:` — which does the manager prefer as the pattern for
   "test-only-cloned secrets"?
3. Site 5 is load-bearing and per-request. The audit question it raises is about the
   *contents* (`token_store` sharing), which is the same question the issue already
   flags for AppState — one `Arc`-the-map decision would cover both 4 and 5.
4. Site 1's two production dependents are themselves derives (`KeyPair`, `EnvKeySigner`).
   If 6 is removed, `KeyPair` becomes the only production reason `PrivateKey` is `Clone`
   — and nothing clones `KeyPair` either. The chain `PrivateKey ← KeyPair ← (no users)`
   means the whole ed25519 secret-clone surface could in principle go in one PR; whether
   API compatibility (SDKs, unbuilt consumers) forbids that is the manager's call.
5. The issue's note stands: if AppState is ever annotated instead of changed, the
   annotation should state that `debug_seed_api_enabled` is a naming false positive
   (faucet seeding flag, not key material).
