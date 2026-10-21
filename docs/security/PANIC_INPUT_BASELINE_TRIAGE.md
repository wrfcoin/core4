# Panic-input baseline triage (#2489)

Snapshot: merged main `ddefe8b23a7cacd89fd68463f23eba7f7d83e591`.

`check_no_panic_on_input.py` finds 69 lexical candidates. The old 106-entry
baseline did not describe the current scanner identity set, leaving every one
of those candidates NEW and making the novelty gate permanently red.

| Classification | Count | Reviewed scope |
| --- | ---: | --- |
| Real input-risk debt | 5 | `create_ring_signature` decoy capacity; snapshot-sync decoded entry capacity; NetCDF dimension, variable, and dimension-id capacities. Their counts come from request or parsed-file data without a local maximum. |
| Baseline drift, bounded or checked | 12 | Sync-message allocation is bounded by `MAX_SYNC_MESSAGE_BYTES`; mempool buckets use the caller-provided type slice; snapshot pagination clamps the range; GFS forecasts are normalized before allocation; NetCDF indexes check EOF/ranges before access. |
| Not directly attacker-reachable | 52 | Hard-coded configuration literals, signal-registration failures, mutex-poison assertions, and collections derived from local consensus state or response formatting rather than decoded peer/RPC/file lengths. |

The five real-risk entries remain visible as named baseline debt; this
regeneration does not claim they are safe. They need separate bounded-input
fixes before a security-clear claim can be made. The baseline exists only to
make a *new* matching site fail loudly instead of hiding it in 69 unrelated
findings.

## What this gate does not look at

- Any Rust module outside `SCANNED_MODULES`, including new trust boundaries
  until their directory is added there.
- Semantic reachability and the adequacy of an existing bound; this is a
  lexical heuristic, not taint analysis or a proof of safety.
- Panics/aborts expressed through macros, FFI, dependencies, allocation OOM,
  integer overflow, async cancellation, or process-level resource exhaustion.
- Runtime-only paths, live peers, hardware, and malformed inputs that require
  fuzzing, adversarial integration tests, or a real service process.
- Test code correctly masked by the source/path test classifier; a regression
  in that classifier is guarded separately by `--self-test` mutation probes.
