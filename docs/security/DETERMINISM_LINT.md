# Determinism Lint

Part of the determinism & Rust-assurance epic (#2102, story #2105). Blocks **newly-introduced**
nondeterministic constructs from the consensus-critical crates.

## The exact claim

> Flags six construct classes across four scanned roots, on pull requests that touch those
> directories, failing only on findings not already frozen in `scripts/determinism_baseline.json`.

Nothing broader. It is a line-oriented heuristic lint, not a proof of determinism — that is what the
determinism matrix (#2104) and its committed root anchor are for.

## What it flags

| Rule | Construct |
|---|---|
| `float-type` | `f32` / `f64` in consensus code |
| `float-literal` | floating-point literals |
| `wall-clock` | `SystemTime::now`, `Instant::now`, `Utc::now` |
| `unseeded-rng` | `rand::random`, `thread_rng`, `OsRng` |
| `readdir-unsorted` | `read_dir(` without a subsequent sort |
| `env-var` | `env::var` / `env::var_os` in a consensus path |
| `unordered-iteration` | iteration over `HashMap`/`HashSet`/`FxHash*`/`AHash*`, tracked **across lines** |

`BTreeMap`/`BTreeSet`/`IndexMap` are deliberately absent: their iteration order is deterministic.

`// determinism-ok: <reason>` on or above a line suppresses it. An empty reason is an error, not a
suppression.

## Scan roots and trigger paths must agree

These are two lists in two languages, and they can disagree silently — which is exactly how the
original defect arose: the lint omitted `shared-protocol/src/blockchain`, the live consensus state
code, **and** would not start on a PR touching only that directory. A gate that skips its most
important subject and cannot be triggered by changes to it.

| `SCANNED_ROOTS` | covered by `pull_request` paths |
|---|---|
| `consensus-engines/src` | `consensus-engines/src/**` |
| `primitives/src` | `primitives/src/**` |
| `shared-protocol/src` | `shared-protocol/src/**` (includes `blockchain/`) |
| `native-chain/src` | `native-chain/src/**` |

`--check-workflow-paths` asserts this, **per trigger**. An earlier version collected every
single-quoted list item in the workflow and unioned them, so `pull_request` and `push` were
indistinguishable: deleting `shared-protocol/src/**` from `pull_request` alone still passed because
`push` retained it. PR reachability is the only reachability that gates a PR.

The detector and its baseline are themselves in the trigger paths, so neither can be changed without
running the gate that depends on them.

## The baseline is anchored to the base branch — a PR cannot expand it to cover itself

This is the mechanism the #2245 review kept breaking, and the fix has to be exactly right.

- The tree carries substantial pre-existing debt, so a fail-on-any gate would be permanently red.
  The gate runs in baseline mode: it fails on findings **not** already frozen.
- **On a pull request, the suppression set is the baseline as committed on the PR's base commit —
  not the baseline in the PR's own tree.** The workflow passes `--anchor-ref <base sha>`, and the
  gate reads `git show <base>:scripts/determinism_baseline.json`. A finding is suppressed only if
  its fingerprint is frozen *there*. Entries a PR adds to its own baseline are **inert**: they do
  not suppress anything until they land on the base branch through a separate, reviewable merge.
- **A duplicate of an already-frozen line is still growth** (occurrences are counted per fingerprint).

### Why a digest could not do this, and the base branch can

Every earlier form anchored the baseline on a digest — first over `findings`, then over
`findings + authorizations`. Both failed the same way, and the reviewer demonstrated it: a PR grows
the baseline to cover its violation, removes the authorization, and **recomputes the digest**, since
the PR runs the very script that computes it. *No value the PR's own run writes can anchor the PR.*
The anchor has to come from content the PR does not control. The base branch is that content — the
PR proposes changes to the baseline, but the committed base version is fixed.

The digest is retained, but only as an **integrity checksum** that flags a hand-edited or corrupt
committed file. It is explicitly not the security anchor, and `--update-baseline` now says so.

### The consequence for authors, stated plainly

You cannot introduce a new nondeterministic construct and grandfather it in the same PR. Two honest
routes remain:

- For genuinely-acceptable new nondeterminism, use the inline `// determinism-ok: <reason>` marker —
  reviewable in the diff, and it takes effect immediately.
- To grow the frozen baseline itself, land that growth on the base branch first (a deliberate,
  separately-reviewed debt acknowledgement). Then the feature PR sees it in the anchor.

### Bootstrap: anchoring is active on the introducing PR too

The PR that first introduces `determinism_baseline.json` has no baseline on its base commit — and an
earlier revision handled this by falling back to trusting the PR's own working baseline, so the
grow-to-cover attack still worked *on the bootstrap PR itself* (#2245: "anchoring is not active in
this bootstrap PR"). It does not fall back any more.

- When `git show <base>:scripts/determinism_baseline.json` finds no file, the gate anchors the
  suppression set to the **base commit's CODE scan** instead — it exports the scanned roots as
  committed at the base (`git archive <base> -- <root> | tar -x`) and scans them. That set is
  content the PR cannot edit, exactly like a committed baseline would be. A finding that already
  exists in the base code is suppressed; a construct the PR introduces is absent from the base and is
  therefore **NEW**, no matter what the PR writes into its own baseline file. The gate prints
  `BOOTSTRAP` and the fingerprint count it anchored to.
- The bootstrap baseline shipped in the diff is still reviewed as the human-readable record of the
  frozen debt, but it is **not** what suppresses findings on this PR — the base-code scan is. Once
  the PR merges, later PRs anchor to the committed baseline (the path above).
- The only degenerate case that yields no anchor is a base commit with **neither** a baseline **nor**
  any scanned code (an empty base); there is genuinely nothing to suppress.
- If `--anchor-ref` is given but the ref cannot be resolved (e.g. a shallow clone without the base
  commit), the gate **fails closed** rather than silently running unanchored. The workflow uses
  `fetch-depth: 0` so the base is present.
- On `push` to main and on `schedule`/`workflow_dispatch` there is no PR base, so no anchor is
  passed and the committed baseline is authoritative — that is the merge/monitoring context, not the
  self-authorization one.

## What it does NOT catch

Stated plainly, because a gate that only lists its strengths teaches people to over-trust it:

- **Anything outside the four scan roots.** Nondeterminism elsewhere in the workspace is invisible.
- **Whether a `// determinism-ok:` reason is true.** It checks that a justification exists, never
  that it is correct.
- **Constructs that never write the token.** The rules are line-local regexes. A float reached
  through an alias, or a clock read behind a helper, is not seen. **Rule ablation in the self-test
  proves each rule is load-bearing; it cannot prove any rule is complete.**
- **Nondeterminism that is not one of the seven classes** — thread scheduling, iteration over a
  `Vec` built in nondeterministic order, floating-point in a dependency.
- **`unsafe`, UB, and leaks** — see `UNSAFE_POLICY.md`.

## Proving it can fail

`scripts/determinism_lint_probe.sh` — self-restoring, ~10s. This gate has been wrong twice in ways
that read as green, so it is held to demonstrating each claim failing:

| Probe | Plants | Requires |
|---|---|---|
| **A** | `SystemTime::now()` in `shared-protocol/src/blockchain` | gate RED |
| **B** | `HashMap` iteration feeding a hash, binding and iteration on different lines | gate RED |
| **C** | production code **below** a `#[cfg(test)]` module | gate RED, and the in-test read stays exempt |
| **D** | removal of the live-blockchain glob from `pull_request` **only** | `--check-workflow-paths` RED |
| **E** | deletion of the authorization record that justified a growth | gate RED (digest mismatch) |
| **F** | a plant **plus** growing the baseline to cover it, anchored to a base that **has** a committed baseline | gate RED (unanchored it is green — that is the defect) |
| **G** | the same, but anchored to a **bootstrap** base with **no** committed baseline — this PR's real gate path | gate RED against the base-code scan |

**F and G are the #2245 blocker made runnable.** Each grows the working baseline to freeze the
plant and confirms an unanchored gate is fooled. **F** then runs `--anchor-ref HEAD` (HEAD standing
in for a base that already carries the committed baseline — the post-merge path) and requires the
plant to be NEW again. **G** runs `--anchor-ref <merge-base with origin/main>` — a base with no
committed baseline, so the suppression set is the base-code scan — and requires the same. G is the
scenario that actually gates the introducing PR; if G passed, bootstrap anchoring would be inert. (G
self-skips, loudly, only if no baseline-free base ref is reachable — e.g. `origin/main` not fetched —
and points to `--self-test`'s `_anchor_self_test`, which proves the same property hermetically.)

Each probe requires its finding to be **attributed to its own plant** — the gate output is captured
and tested with `[[ -n ]]`, never piped through `head`/`sed` (whose exit status would mask a
no-match and make the check unfalsifiable). An earlier revision both grepped the whole report,
printing evidence from unrelated files, and left that masking pipe in F — red for the wrong reason,
which is the failure this script exists to catch. Cleanup runs on every exit path and the final step
asserts every touched file was restored to its **exact starting bytes**.

## Self-test independence

The second defect this story had was a self-test written in the shape the buggy regex matched, so it
agreed with the implementation instead of checking it.

Fixtures alone cannot settle that — a rule for `f64` can only be exercised by a fixture containing
`f64`. What the self-test settles instead is **causality**:

- **Rule ablation** — each rule is removed in turn and its detection must disappear, and must have
  been present with the full rule set. A case that survives its rule's removal was being satisfied
  by something else.
- **Mask regression guard** — a production fn below `mod tests` must not be masked, and a `#[test]`
  body must still be masked.
- **Lifetime guard** — `MutexGuard<'static, ()> {` must keep its brace.

Both new guards were proved load-bearing by reverting the corresponding fix and confirming the
self-test fails.

### The lifetime bug, recorded because it is easy to reintroduce

`_strip_literals` treated `'` as a char-literal opener. Rust lifetimes are not char literals, so
`MutexGuard<'static, ()> {` had its trailing brace consumed. That desynchronised brace-depth
tracking and closed `mod tests` 187 lines early in `launch_api/main.rs`, so module-level test
helpers were scanned as production code. A `'` now opens a char literal only if it closes.

Net effect of the mask work, measured: **15 production findings** that the old backward-scanning
heuristic had hidden became visible (env-var and float reads at `launch_api/main.rs:172, 275, 425`
and others, all above the `mod tests` at 1436), and **46 false positives** inside the test module
were removed.
