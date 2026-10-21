# `unsafe` and Undefined-Behaviour Policy

Part of the determinism & Rust-assurance epic (#2102, story #2106). Memory safety does not
cover undefined behaviour once `unsafe`, FFI and hand-rolled serialization enter the picture;
Miri is the tool that does. This document states the rules and lists the current exceptions.

## Rules

1. **Every `unsafe` block or `unsafe fn` carries a `// SAFETY:` justification.** The scanner
   accepts it on the same line or within the **two** lines above, so a blank line between the
   comment and the block is fine. Recognised forms are `unsafe {`, `unsafe fn`, `unsafe impl`,
   `unsafe extern` and `unsafe trait`. Enforced by `scripts/check_unsafe_justified.py`, which
   fails CI on any **new** unjustified `unsafe` (see [Enforcement](#enforcement)).
2. **Consensus-critical crates should contain no `unsafe`.** Exceptions are listed below and
   must be either removed or Miri-tested.
3. **FFI boundaries are Miri-tested or explicitly exempted with a reason.** As of #2454 this
   repo has **no FFI surface**: `consensus-engines/src/ffi.rs` was the only one, and it was
   retired rather than hardened — it produced no loadable artifact (the crate is `rlib`-only),
   had no in-repo caller, and its sole consumer pointed at an absent path in the archived
   `blockchain-core` repo. The rule stands for any future boundary: if a test dereferences raw
   C pointers or crosses a boundary Miri cannot model, exempt it with `#[cfg_attr(miri, ignore)]`
   **and a reason comment**. No such exemption exists, because there is nothing to exempt — an
   earlier revision asserted exemptions were already in place, which was untrue of any scanned
   crate (review of #2247). Do not read this section as a description of work already done.

## Current exceptions (unjustified `unsafe`, frozen in `scripts/unsafe_baseline.json`)

There are currently no grandfathered exceptions: the four original FFI/PKCS#11 sites were
justified and retired from the baseline in #2106. #2454 then retired the bridge itself, including
the string-freeing export and its dormant consumer, so there are no remaining pointer-taking FFI
exports or a pending FFI audit in this repository.

The baseline (`scripts/unsafe_baseline.json`) freezes **individual sites**, each with a written
justification. It is currently `{}`. When non-empty it is a JSON object, not a list or count map;
for example, the retired FFI entry had this shape:

```json
{
  "consensus-engines/src/ffi.rs|fn consensus_ffi_free_string|unsafe {|0|body:76b8f1a9ee03":
    "FFI string free: reconstructs a CString from a pointer this module handed out ... (#2106)"
}
```

A site's identity is `path|enclosing item|normalized text|ordinal|body:<digest>`:

| Component | Why it is there |
|---|---|
| **enclosing item** | nearest `fn`/`impl`/`mod`/`trait`. Makes a *remove-and-replace swap* visible: justify the grandfathered block, add an unjustified one elsewhere, and the new one is a different site even when the source text is byte-identical. |
| **normalized text** | whitespace-collapsed, so reformatting does not churn the baseline. Churn is what pressures a gate back toward coarser units. |
| **ordinal** | index among identical `(item, text)` pairs, so a second identical `unsafe` in the *same* function is still a distinct site. This is why `pkcs11.rs` has two `let rv = unsafe {` entries, ordinals `0` and `1`. |
| **body digest** | `sha256[:12]` of the normalized construct body. Binds the reason to the **operation** it justifies. Without it the first four components are all preserved by replacing the block's contents: swapping `CString::from_raw(s)` for `core::ptr::read_volatile(&0u8)` inside the grandfathered FFI block reported `0 NEW`, exit 0, and an unrelated unsafe act inherited a justification written about reclaiming an FFI CString pointer (#2247 review). |

No line numbers, deliberately: they churn on every edit above the site.

**The digest's stated bound.** The body is taken through the same literal-stripping and
whitespace-normalizing used elsewhere, so comments, string/char literal *contents* and
reformatting do not churn it. The deliberate consequence: a change confined entirely to a string
literal or a comment is not seen as a new site. Safety-relevant operations are not string
contents, so this buys a baseline that survives `rustfmt` at the cost of a small, named blind
spot. The digest is short and the readable prefix is kept so a reviewer can still see which file,
which function and which opening line an entry refers to; the digest only answers *"is this still
the same code?"*.

**Every baseline entry must carry a justification citing an issue (`#NNNN`).**
`--update-baseline` writes placeholder text that *fails* the gate, so regenerating the baseline
in the same change as a new violation cannot launder it — grandfathering is a recorded decision
visible in the diff, not a silent count bump. Legacy list and count-map baselines are rejected
outright rather than reinterpreted.

**A real citation is necessary but not sufficient: the grandfather baseline may only shrink (the
ratchet).** The placeholder rule closes `--update-baseline` laundering, but a developer can still
sidestep it by hand: add a new `unsafe` block, and instead of a `// SAFETY:` comment, write a
baseline entry that cites a genuine issue, in the same change. Every current-tree check then
passes — the fingerprint is "in the baseline" and the citation is real — because nothing in the
current tree distinguishes a freshly-introduced site from debt that predates the policy (review of
#2247, round 92). The **base ref** is what distinguishes them. The gate takes `--base-root`, a
checkout of the PR base branch, and a baseline entry is legitimate only if the site it freezes
**already exists on the base ref**. An entry that freezes a site present now but absent on the base
ref was introduced by this change and is reported as illegal `GROWN` — the baseline may retire debt
freely but may not grow to cover `unsafe` the change itself created. `unsafe-policy.yml`
materializes the base ref (`git archive` of the PR's target branch) and passes `--base-root`; the
`fetch-depth: 0` checkout exists for exactly this. Pinned by self-test PROBE 7.

Retiring an exception is the same as it ever was: add a `// SAFETY:` comment to the block and
delete its baseline entry. A vanished baseline entry is reported, not an error — paying down
debt is the point. A *new* site is a new fingerprint and fails the gate regardless of how many
old ones disappeared in the same change, so a swap cannot pay for itself. And because of the
ratchet (above), the baseline cannot grow to *adopt* a new site either: freezing an `unsafe`
that does not exist on the base ref is reported as illegal `GROWN`.

### Two earlier baseline shapes, and why they were replaced

Recorded because both looked sufficient and both had the same defect — the unit of identity was
coarser than the violation it claimed to detect (#2284):

| Shape | Bypass |
|---|---|
| set of `path\|text` shapes | accepted *any number* of bare `unsafe {` in a baselined file |
| count per `path\|text` | accepted a remove-and-replace swap at constant count — counts detect *more* blocks, not *different* ones |

### What this gate does NOT catch

Stated plainly, because a policy that only lists its strengths teaches people to over-trust it:

- **`unsafe` outside `SCANNED_PATHS`.** Only `primitives/src`, `shared-protocol/src` and
  `consensus-engines/src` are scanned. `unsafe` elsewhere in the workspace is invisible to it.
- **Whether a `// SAFETY:` comment is *true*.** The gate checks that a justification is present,
  never that it is correct. A wrong `// SAFETY:` passes exactly like a right one; only review
  and Miri catch that.
- **`unsafe` inside test code** is excluded, by a brace-depth scope mask that tracks where a
  `#[test]` fn or a **test-only `cfg` module** actually *ends*.

  Test-only status is decided **by refutation, not by matching attribute text** (#2429). The
  checker parses the `cfg` predicate, fixes `test = false`, and searches for any assignment of
  the remaining atoms that still satisfies it. One satisfying witness means a build **without**
  `test` can reach the code, so it is production and stays scanned. No witness means the code
  cannot exist outside a test build, so it is exempt.

  | predicate | reachable with `test = false`? | treated as |
  |---|---|---|
  | `#[test]`, `#[tokio::test]`, `#[*::test]` | — | test (exempt) |
  | `#[cfg(test)]` | no | test (exempt) |
  | `#[cfg(all(test, debug_assertions))]` | no | test (exempt) |
  | `#[cfg(all(test, feature = "x"))]` | no | test (exempt) |
  | **`#[cfg(debug_assertions)]`** | **yes** | **production (scanned)** |
  | `#[cfg(any(test, feature = "x"))]` | yes, whenever `x` is on | production (scanned) |
  | `#[cfg(not(test))]`, `#[cfg(all(not(test), ..))]` | yes | production (scanned) |
  | `#[cfg(feature = "test-util")]` | yes | production (scanned) |
  | unparseable, or >12 non-`test` atoms | — | production (scanned) |

  **`debug_assertions` alone is NOT test-only** and is the trap worth naming: it is true in any
  shipping debug build, so `unsafe` under it is production-reachable and must carry a
  justification. Only its conjunction *with* `test` exempts. The same holds for `any(test, ..)`:
  reading it as test-only would silently exempt `unsafe` behind any feature flag — that is the
  one direction this mask must never fail in, and it is pinned by self-test probes asserting
  **both** sides, not just the exempt ones.

  Anything the parser does not understand **fails closed** — scanned as production. The failure
  direction is therefore still *over-report, never a missed violation*, which is the basis on
  which this gate was accepted; what changed in #2429 is only that genuinely test-only forms are
  no longer over-reported.

  Earlier revisions recognised the literal string `#[cfg(test)]` only. That misclassified plain
  (non-`#[test]`) helpers inside `#[cfg(all(test, debug_assertions))] mod tests` as production —
  `shared-protocol/src/crypto/signer.rs`'s `with_valid_rotation_env` was reported as two
  violations it had never earned, and `main` went red for every PR touching the scanned crates.

  The brace-depth mask correctly ignores braces inside string/char literals **and Rust
  lifetimes** (`Formatter<'_>) -> Result {` keeps its `{`); an earlier revision consumed a
  lifetime as a char literal and closed a test region early. Pinned by self-test PROBE 6.
  Attribute detection strips comments but **keeps** string literals, so `feature = "x"` parses
  while a commented-out `// #[cfg(test)]` above a `mod foo {` still cannot open a region.
- **Multiple `unsafe` blocks on one physical line** count as **one** finding. The scanner is
  line-oriented; the ordinal disambiguates repeats across lines, not within one. Verified: a line
  containing two distinct `unsafe { ... }` blocks reports a single site. This is the same
  shape as the `.unwrap()` ratchet defect in #2278, and shares the tracking issue #2284.
- **Whether the code is actually sound.** This is a documentation gate. Soundness is Miri's job,
  and Miri only runs on a schedule/dispatch, not on every PR.

## Miri coverage

`.github/workflows/miri.yml` runs `cargo +nightly miri test`. **It has no `pull_request`
trigger** — schedule and `workflow_dispatch` only. The `// SAFETY:` gate lives in a *separate*
workflow, `.github/workflows/unsafe-policy.yml`, which is the one that runs on PRs.

That split is the fix for a real incident and is worth keeping. Both used to live in one
workflow, with Miri behind `if: github.event_name != 'pull_request'`. On a PR the Miri matrix was
`skipped` with **zero steps** — its job name never even expanded past `Miri (${{ matrix.crate }})`
— while the run's overall conclusion was `success` because the cheap gate had passed. Run
`29892523607` was then cited as green proof Miri worked; it had executed nothing. Now the
workflow named `Miri` produces **no PR runs at all**, so no PR run can be mistaken for Miri
evidence. That is structural rather than a documented caution. See antipatterns 16 and 21.

**Cite only `schedule` or `workflow_dispatch` runs of `Miri` as Miri evidence.**

### Legs, and why they are scoped this way

| Leg | Package | Filter | Measured locally |
|---|---|---|---|
| `wrf-primitives` | `wrf-primitives` | *(whole lib)* | green — 6 tests, **0.7s** |
| `shared-protocol-crypto` | `wrfcoin-shared-protocol` | `crypto::` | green — 37 tests, **80s** |

Every leg in that table has been run to completion locally. None is shipped on the strength of
"it ought to pass" — that is the whole point of this story.

**The zero-steps guard.** `${{ matrix.filter }}` is a test *name* filter handed to the harness; a
filter that matches nothing makes `cargo miri test` print `running 0 tests`, exit 0, and turn the
leg green having interpreted no code — the exact "dispatch executed zero steps" that once got a
Miri run miscited as evidence (review of #2247, round 92). So each leg parses the harness's own
`running N tests` lines and **fails when the total is zero**. A leg cannot be green unless Miri
actually interpreted at least one test. Measured locally: `wrf-primitives` interprets 6, the
the `crypto::` leg 37. (The retired `ffi::` leg interpreted 6 with 411 filtered out — that
remains the clearest demonstration that a filter both matches and excludes, which is why the
guard exists; #2454 removed the leg with the module.)

Package names, not directory names: `consensus-engines/` publishes `wrfcoin-consensus-engines`,
and the directory name made `cargo pkgid` exit 101 so that leg could never start (#2247 review).
A `Verify the matrix package exists` step now fails fast and legibly on that class of typo.

**Why `shared-protocol` is scoped to `crypto::` rather than the whole crate.** This is not
scope-trimming to manufacture a green result, and the evidence is recorded so it cannot be read
that way. A whole-crate run reaches
`shared-protocol/src/consensus/proof_of_publishing.rs:4166` — `block.transactions.par_iter()` —
and rayon's work-stealing deque drives `crossbeam-epoch`'s epoch collector, which trips a Stacked
Borrows violation **inside `crossbeam-epoch-0.9.18`**:

```
error: Undefined Behavior: trying to retag from <964111933> for SharedReadWrite permission
       at alloc338715093[0x8], but that tag does not exist in the borrow stack
  --> crossbeam-epoch-0.9.18/src/internal.rs:549:9   (&*local_ptr)
```

Every frame in that backtrace is third-party (`crossbeam-epoch` → `crossbeam-deque` → `rayon`);
none is WRFCoin code. Miri itself notes the rules involved "are still experimental". The finding
is real and should be tracked, but it is a dependency issue outside this story, and keeping it as
a permanently-red leg would destroy this gate's ability to signal anything — the exact
"red noise that hides real red" failure this repo has been paying down. So the leg is scoped to
the modules #2106 actually names, and the whole-crate result is written down here instead of
being quietly dropped.

Two costs are worth knowing before anyone widens the scope again: the whole-crate run took
**2h23m** on a GitHub runner before failing (dispatch `29896851781`, 06:28:17 → 08:51:48), versus
**80s** for the scoped `crypto::` leg locally.

**Retired `consensus-engines` `ffi::` leg.** Before #2454, a whole-lib run did not finish within
45 minutes locally and the scoped `ffi::` leg was the only practical coverage for that bridge.
That module and its matrix entry are now deleted. It is historical evidence only, not a current
coverage claim: scheduled Miri runs no longer execute `wrfcoin-consensus-engines`.

### `-Zmiri-ignore-leaks`, and what it costs

The retired `ffi::` leg exposed exit-time leak reports in transitive `scc` and `sdd` lock-free
structures. Those structures defer reclamation, so allocations can legitimately outlive a test;
the same limitation applies to the remaining matrix workloads.

The two flags weaken different things, and neither touches UB detection:

| Flag | Turns off | Still checked |
|---|---|---|
| `-Zmiri-disable-isolation` | host access restrictions (clock, fs, env) | all UB checks |
| `-Zmiri-ignore-leaks` | the exit-time **leak** checker | all UB checks |

Stacked Borrows/aliasing, out-of-bounds, invalid-value and uninitialised-memory detection all
still run, and those are what #2106 exists to catch. **Stated cost: leak detection is off, so a
genuine leak in our own code would not fail this gate.**

### How Miri is run, and what that costs

The matrix runs with `MIRIFLAGS=-Zmiri-disable-isolation -Zmiri-ignore-leaks`. This is not a convenience: `Block::new`
calls `Utc::now()`, so *every* test that constructs a block reaches `clock_gettime`, which Miri
refuses under isolation. That is hundreds of hash-determinism tests in `shared-protocol`, and
annotating them all with `#[cfg_attr(miri, ignore)]` would delete most of the crate's coverage to
work around a clock call.

Stated plainly, because the distinction matters: isolation governs whether Miri lets the program
reach the host — clock, filesystem, environment. Disabling it does **not** disable the
undefined-behaviour checks. Stacked-Borrows/aliasing, out-of-bounds, invalid-value and
uninitialised-memory detection all still run, and those are what #2106 exists to catch. The cost
is that a test performing real I/O will now actually perform it, so network- and HSM-dependent
tests must remain behind their existing environment gates.

Tests that genuinely cannot run under Miri — real networking, or the actual PKCS#11 / C ABI —
should carry `#[cfg_attr(miri, ignore)]` with a one-line reason. None currently do; see the note
in rule 3.

## Enforcement

| Check | Workflow | Runs on PR? | Executes? |
|---|---|---|---|
| `check_unsafe_justified.py` (baseline gate + ratchet) | `unsafe-policy.yml` | **Yes** | Yes (Python, ~seconds); `--base-root` enforces the shrink-only ratchet |
| `cargo +nightly miri test` | `miri.yml` | **No — the workflow has no `pull_request` trigger** | Yes, on `schedule` / `workflow_dispatch`; each leg fails if it interprets zero tests |

So the honest, complete claim this story delivers is:

> **`unsafe` blocks carry `// SAFETY:` comments, enforced on every PR; Miri runs weekly over
> primitives, the FFI crate, and shared-protocol's crypto modules.**

Nothing broader. Miri is not a PR gate and must not become one without a cost decision — see the
2h23m measurement above. The `unsafe` inventory is uploaded as a CI artifact for audit.

**Reading a Miri result correctly.** The previous shape let a `pull_request` run report `success`
while every Miri leg was `skipped`, and that run was cited as proof the gate worked. The split
removes the trap at the source: `miri.yml` has no PR trigger, so it has no PR runs. If you are
ever handed a Miri run as evidence, still check `event` and the **per-job conclusions** rather
than the run's top-level status — `runner_name` and step count distinguish executed from skipped.

## Proving these gates can fail

`scripts/unsafe_miri_probe.sh` is a self-restoring acceptance probe. It plants a real violation
against each gate and requires each to go red, so nobody has to reconstruct the author's
reasoning to trust them:

| Probe | Plants | Requires |
|---|---|---|
| **A** | a bare `unsafe {}` with no `// SAFETY:` in production code | gate exits non-zero, naming the site |
| **B** | the #2247 body swap — same opener, same function, same ordinal, different operation | gate exits non-zero |
| **D** | a new unjustified `unsafe` **plus** a baseline entry citing a real issue, in one change (base ref = `HEAD`) | gate passes without `--base-root`, exits non-zero **with** it (`GROWN`) |
| **C** | a genuine Stacked Borrows violation in `wrf-primitives` | `cargo miri test` reports `Undefined Behavior` |

Probe C is the one that distinguishes *Miri ran* from *Miri detects*. Last local run:

```
A: bare unjustified unsafe        -> gate RED  (11 sites, 5 unjustified, 1 NEW)
B: body swap under same opener    -> gate RED  (1 NEW)
C: deliberate UB in primitives    -> error: Undefined Behavior: attempting a read access
                                     using <135703> ... tag does not exist in the borrow stack
All three gates are load-bearing.
```

Cleanup runs on every exit path via `trap`, and the script re-verifies the clean gate afterwards.
`--no-miri` skips C where nightly is unavailable.
