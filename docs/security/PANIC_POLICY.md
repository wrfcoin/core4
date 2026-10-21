# Panic-on-Input Policy

Part of the determinism & Rust-assurance epic (#2102, story #2111).

## Rule

**A panic reachable from attacker-controlled input is a security finding, not a bug.** A
remotely triggered panic (a hostile peer or RPC client crashing a node) is a **launch
blocker**. Network input must never reach `.unwrap()`, `.expect(...)`, unchecked indexing, or
an unbounded allocation whose size comes from decoded bytes.

Input-facing code must fail by *returning an error*, never by panicking. Decoders validate
length and bounds before allocating.

## Triage and disclosure

When a panic reachable from network input is discovered (by fuzzing, review, or an incident):

1. **Do not** open a public GitHub issue with a reproducing input. File privately per
   [`SECURITY.md`](../../SECURITY.md) at the repository root.
2. Label the tracking issue `security` and `launch-blocker`.
3. Commit the crashing input to `fuzz-corpus/<target>/` as a permanent regression in the same
   PR as the fix (see the fuzz-regression corpus policy, #2110).
4. The fix returns an error at the trust boundary; it does not merely widen a `catch`.

## Enforcement

`scripts/check_no_panic_on_input.py` scans the **attacker-controlled input surface at
directory level** (`SCANNED_MODULES`), so a new decoder added under any of these paths is
covered automatically — no file has to be added to a list:

- `p2p/src/` — all peer/gossip message, handshake, sync and DA-challenge decoders;
- `shared-protocol/src/mempool/` — transaction admission from peers/RPC;
- `shared-protocol/src/cross_chain/` — bridge / cross-chain calldata;
- `native-chain/src/bin/launch_api/` — RPC handlers, websocket, **and the `p2p_events` peer
  consumer** (`bridge_weather_payload_to_mempool`, which a prior review showed was invisible
  under the old hand list);
- `execution-engine/src/` — contract calldata / VM input;
- `sensor-attestation/src/`, `oracle-packet/src/` — RF/IoT / oracle-packet ingest.

This is deliberately **not** a whole-tree scan: an `unwrap` in a pure-compute helper is not a
remote-panic vector, and scanning all ~2000 non-input sites would bury the security signal.
Add a directory here when a new trust boundary appears.

Escape hatch: `// panic-ok: <justification>` (same line or line above); an empty justification
fails. Every active allow is printed.

| Aspect | Value |
|---|---|
| Workflow | `.github/workflows/input-panic-lint.yml` |
| Runs on PRs? | **Yes — on ANY `**/*.rs` change** (not a hand-listed path set), so a new decoder cannot land without the gate running |
| Executes? | Yes — Python, not subject to the Rust-gate skip in #2233 |
| Gate scope | **Baseline mode** — fails on NEW panic-on-input risk only |
| Anti-rot ratchet | The gate compares the committed baseline to the merge-base baseline and **fails if it grew**, unless a reviewed entry is added to `scripts/panic_input_baseline.migrations` in the same diff. Debt may only shrink; growth must be signed off. |

Honest note: the scanned surface already contains 107 pre-existing `.unwrap()`/`.expect()`
and similar sites (frozen in `scripts/panic_input_baseline.json` — count as merged; an
earlier draft said ~300 against a wider pre-pruning scope), so a hard fail-on-any gate
would be red today. The gate blocks new risk and the ratchet prevents the frozen count from
silently growing; the baseline is debt to triage — the fuzz targets in #2107 are how the
genuinely-reachable ones get found and filed as security issues.
