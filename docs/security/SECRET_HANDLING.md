# Secret-handling policy

Recorded for #2129.

## The rule

> No secret material may reach a log line, a metric label, an error message, or a panic payload.

"No secrets in logs, crash reports or metrics" is a review item, and a review item only stays true
if something enforces it. Two automated passes enforce it, both wired into
`.github/workflows/secret-scanning.yml`.

## Metric labels are the commonly missed path

A log line usually stays on the host, is rotated, and is read by an operator. A **metric label**
becomes a Prometheus series: scraped on an interval, persisted in a time-series database, and
frequently shipped onward to a hosted dashboard. A secret in a label therefore crosses more trust
boundaries, more permanently, than the same secret in a log file — and unlike a log line it is
never rotated away.

It is also the easiest to write by accident, because a label looks like a harmless string:

```rust
counter.with_label_values(&[&signer_id]).inc();   // fine
counter.with_label_values(&[&signing_key]).inc(); // catastrophic, and looks identical
```

`check_secret_leak.py` labels these findings `METRIC LABEL` and reports the count separately.

## Scope: all production Rust roots

The first version defaulted to `shared-protocol/src`, `privacy/src`, `p2p/src` and the CI job
passed no paths — so it reported "clean" over 149 files while never looking at `native-chain`,
which contained a real bearer-token leak. It now scans **17 production crate roots** and prints
the omitted roots (tests, tools, fuzz, sdks, demos) on every run, so a partial tree can never
again be reported as clean.

`scripts/secret_leak_baseline.txt` records the existing backlog as `path|sink|count`. Counts, not
bare suppression: a **new** leak of the same sink in an already-baselined file pushes the count
over and still fails. The baseline is a backlog to work down, not an acceptance — two of its
entries are the filed production defects in #2259.

## Pass 1 — static: `scripts/check_secret_leak.py`

Flags a secret-shaped identifier reaching a sink.

`#2453`: this pass is intentionally name-based and therefore incomplete. A secret that flows into
an output sink through a neutral identifier is not detected here; we still need the runtime pass and
the follow-on evidence model to cover that class.

| Sink | Examples |
|---|---|
| log macros | `log::{error,warn,info,debug,trace}!`, `tracing::*!` |
| formatting | `format!`, `write!`, `writeln!`, `print*!`, `eprint*!` |
| failure paths | `panic!`, `unreachable!`, `todo!`, `.expect(..)`, `assert*!` |
| **error constructors** | `anyhow!`, `bail!`, `ensure!`, `.context()`, `.with_context()` — an error message is an output path like any other |
| **derived exposure** | a struct deriving `Debug`/`Serialize` over a secret-named field; no call-site scan can find this, because the leak *is* the derive |
| **metric labels** | `with_label_values`, `const_label*`, `.label(..)` |

Secret-name segments: `secret`, `priv`, `private`, `privkey`, `seed`, `mnemonic`, `passphrase`,
`token`, `jwt`, `signing`, `sk`, `credential`, `pin`.

Matching runs over **whole macro invocations, not single lines**. Real Rust wraps arguments across
lines, and the line-based first version rejected a one-line leak while silently passing the
identical multiline form — the single most important finding in the review of #2239.

Two further decisions make it usable as a gate rather than noise:

1. **Identifier-segment matching, not substring.** Substring matching flags `PrivacyError` (for
   `priv`) and `task_id` (for `sk_`). Identifiers are split on `_` and camelCase boundaries and
   compared segment-wise.
2. **Only value-bearing positions count.** String-literal *text* is dropped; inline captures
   (`{ident}`) inside it are kept, as is everything outside literals.
   `assert!(x, "private key must round-trip")` mentions a private key; it does not print one.
   `debug!("{private_key}")` does.

Without both, the first run reported 75 findings on a clean tree — almost all prose — which is a
check nobody can gate on.

Allowlist a verified false positive with a trailing comment stating **why**:

```rust
assert!(!is_docker_bridge_addr(&private_10));  // secret-leak-ok: private IP range constant, not a key
```

## Pass 2 — runtime: `shared-protocol/tests/no_secret_in_output.rs`

A static scan cannot see through a `Display` impl that *derives* its output from a secret field,
or a `Serialize` that happens to include one. Those are invisible to grep and obvious to a
sentinel.

Each secret-bearing type is constructed with the sentinel `WRFCOIN-SENTINEL-9f3a7c21-DO-NOT-LEAK`,
then `Debug`, `Display`, `Serialize`, error rendering and a **forced panic payload** are exercised
and asserted not to contain it (plain or hex-encoded).

`a_deliberately_leaking_type_fails_the_check` is the negative control: a type that *does* leak,
asserted to be caught. A leak detector that cannot detect a leak is worse than none, because it is
believed.

## Pass 0 — the scanner tests itself: `scripts/test_check_secret_leak.py`

`scripts/tests/secret_leak_fixtures/` plants a leak on **every** claimed surface — log, `format!`,
`panic!`, metric label (all in multiline form), `bail!`, `.context()`, derived `Debug`/`Serialize`,
and a bearer token formatted into an identifier — plus one clean file that must **not** be
flagged. Every planted fixture must be rejected and the clean one accepted.

It additionally asserts the scanner detects the two production exposures filed as **#2259**
(`StealthKeys` derived exposure; the `native-chain` bearer-token format). Fixing those is out of
scope for a test-harness story; *detecting* them is the acceptance criterion.

This pass runs **first** in CI. A scanner nobody tests proves nothing.

## Preferred patterns

- **Do not implement `Debug`/`Display` on secret-bearing types at all.** Not a redacting `Debug` —
  none. A redacting formatter leaves the call site standing, and the next person to "improve" it
  has a working formatter to reach for. `security::key_roles::KeyHandle` (#2128) is the model:
  `format!("{handle:?}")` is a compile error.
- Expose a **salted, non-reversible fingerprint** when you need to compare or identify a key.
- Hand out **borrows, not owned copies** of secret bytes, so a copy cannot outlive zeroization.
- Derive `ZeroizeOnDrop` on anything holding key material.
- Log a **key id or role**, never key material.

## Triage

A finding is real if the *value* of a secret can reach the sink. Verify by reading the type, not
the name: `SigningKeyRegistry` matches `signing` but stores `KeyEpochRecord.public_key` — public
keys only.

Fix real findings. Allowlist verified false positives with a reason. Never silence a pass by
loosening its patterns.

## Related

- #2128 — key-role separation and zeroization; `scripts/check_secret_copies.py`
- `docs/security/DOMAIN_TAGS.md` — signed-object inventory (#2122)
