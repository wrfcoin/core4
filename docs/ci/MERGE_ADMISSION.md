# Merge admission — what must be true before a commit reaches `main`

**This is a contract.** `scripts/check_merge_admission.py` compares the live effective rules on
`main` with `.github/merge-admission.json` and checks that this page names every required context.
The `Merge admission contract` workflow runs on every main push and contract-relevant PR.
A mismatch is checker exit 1 (DRIFT); an unavailable read is exit 2 (VOID).
**The live-drift workflow step is advisory under the 2026-10-04 owner ruling (Refs #3198):**
it reports the exit, warning and summary without blocking the job. Comparator controls and
fixture tests remain blocking. A green workflow alone does not prove live equality.
Rulesets are owner-only; no script or change here edits them.

Verified 2026-10-07 using `gh api repos/wrfcoin/core4/rules/branches/main` and the analogous
infra endpoint, plus both ruleset metadata endpoints. Core4 source baseline: `620902bf7b01cf09b11b2bb0ff5a22571e7cb694`.
Earlier incident measurements below are historical evidence, not today's required-context inventory.

## 1. The contract

A pull request may merge into `core4/main` when all of the following hold. Nothing else is required.
Everything that looks required but is not listed here is a review convention, and §4 says which ones.

1. **The ruleset admits it.** `main-protection` (19875840) requires linear history, a pull
   request and exactly four status contexts (§2). `strict_required_status_checks_policy=true`:
   the PR must be up to date with `main`. OrganizationAdmin retains its recorded `always` bypass.
2. **Each required context is REAL and FRESH.** Require a named runner and nonzero executed steps.
   Merge-ref freshness and repository-input admission bind relevant evidence to the candidate head
   and base. Strict policy protects the completed status when the base advances. Review exact SHA,
   runner, executed steps and base identity; a rerun of a stale merge ref is not a refresh (§3).
3. **Every other failing check on the PR is attributed by file path** in a PR comment — *mine* /
   *main's* (cite the run and the fixing PR) / *VOID* / *contaminated* (§3.1) — before the PR is
   called ready. A red on a file the diff does not touch is not the author's, and the author must
   not be asked to prove it twice (#2981, #3005). **A RED that names paths absent from the PR's
   diff is attributed, never "fixed"** — a fix for someone else's error in your PR is how a
   contaminated build becomes a real regression (#3033). And **a red on a file the diff does not
   touch is a contamination *candidate*, not a contamination *finding*; the difference is whether
   anyone read the failing test.** #3012's red was outside its diff and was *not* contamination —
   it was an observer race in the test's own window (#3035). Without that line, §3.1 becomes a
   universal excuse for every red anyone dislikes.
4. **The reviewer read the runner field** (`gh api repos/wrfcoin/core4/actions/jobs/<id>
   --jq '.runner_name, (.steps|length)'`) for any check they cite as evidence. Step count is not
   runner identity, and duration is not either.

For `infra/main`, `main-protection` (19875881) requires a PR, linear history and the two contexts
`scripts/tests suites` and `dependabot-coverage`, with `strict_required_status_checks_policy=false`.
Its OrganizationAdmin `always` bypass is unchanged. Reviewer evidence and freshness obligations
still apply; strict is not enabled there.

## 2. Required contexts, and why these four

| Context | Workflow / job | Fires on | Proves | Evidence limit |
|---|---|---|---|---|
| `schedule-lint` | `schedule-lint.yml` | every PR and main push, unfiltered | schedules agree with the explicit owner-acknowledged allowlist (Refs #2498) | no test/build evidence; nightly scheduling does not itself prove execution |
| `detect-secrets baseline audit (merge tree)` | `secret-scanning.yml` / `detect-secrets` | every PR plus every branch push (branch-tip name) | merge-tree secret baseline audit | advisory secret-runtime work is separate; inspect freshness and checkout identity |
| `verify-terminal-skip-guards` | `terminal-skip-guard-integrity.yml` | every PR and main push, unfiltered | manifest-pinned terminal guards reject failed required workers | structural evidence, not proof those suites executed |
| `repository-input integration admission` | `repository-input-admission.yml` / `admission` | every PR and main push, unfiltered | relevant inputs require both integration-ratchet jobs at the candidate head/base, named runners, completed Observe steps and execution floors | irrelevant prose is an executed scope decision, not runtime evidence; PENDING, skipped, failed or unavailable ratchet evidence cannot admit |

### 2.1 Adding a required context

A required context must fire on every PR and evaluate main on push, execute with a named runner
and nonzero steps, refuse skipped/missing/failed obligations, and have observed negative controls.
Availability and latency must be measured rather than inferred from runner labels. These four
currently run on `[self-hosted, node-host]`; this contract does not change runner placement.
Repository-input admission is a short Python job but depends on heavy integration-ratchet evidence
on relevant inputs. Rust workflows outside that dependency remain advisory review evidence.

The 2026-10-07 owner ruling permits input-relevant PR runs plus nightly main runs for heavy
non-required workflows. It does not filter any of the four required workflows or narrow the ratchet.
`schedule-lint` therefore permits the acknowledged nightly allowlist rather than banning every cron.

### 2.2 Strict is on for core4

Core4's live strict flag is **true**. Repository-input admission requires exact head/base evidence
but does not itself schedule new checks merely because main advances; strict closes that admission
window. The September #3005 refresh-cost observations remain historical queue measurements.
They do not authorize disabling today's strict flag. No merge-queue activation or ruleset edits
are part of this reconciliation. Infra's live strict flag remains **false**.

## 3. Evidence: what counts, what is stale, what is void

One sentence covers all of it: **a verdict earned on tree A is not a verdict about tree B.**

| Class | Meaning | How you tell | Counts as |
|---|---|---|---|
| **REAL-GREEN / REAL-RED** | a named runner executed ≥1 step and passed / failed | `runner_name` non-empty, `steps \| length > 0` | evidence about the tree it checked out — **for build container Rust jobs, only subject to §3.1**: "executed and passed" does not by itself mean "compiled *this PR's* code" |
| **CONTAMINATED** (#3033) | executed, but linked artifacts from a tree the PR does not contain | a RED naming a path, type or symbol absent from the PR's diff *and* from its merge base; or a GREEN from a build container Rust job while an unmerged sibling PR touching the same crates was building (undetectable from the run record alone) | a RED is **attributed, not fixed** (§1 rule 3); a GREEN is *not* evidence about this PR's code until §3.1's stronger evidence exists |
| **VOID** | nothing executed | `runner_name` empty or null and `steps=0` (billing; cascade-skip); a run with **no jobs** (workflow YAML did not parse); a `cancelled` conclusion (#2945) | evidence in neither direction — never a pass, never a failure |
| **STALE** | executed, but on a merge ref `main` has since moved past | `runs/<id>.created_at` earlier than `main`'s last commit time. **A re-run does not refresh it** — `run_attempt: 2` replays the same merge ref; only a push (`synchronize`) recomputes it | VOID for admission, even when green; also produces false positives from *main's* older copies of untouched files (#3005) |
| **VACUOUS** | executed and passed without testing the claim | `skipped` legs under a `success` rollup; `continue-on-error` steps with a green tick; a "nonvacuous" floor met by zero | not evidence; a skipped job **satisfies** a required check, which is why §2's third context exists |
| **UNKNOWN** | evidence could not be retrieved twice | API error, `gh` exit ≠ 0 | cannot admit; say so rather than infer |

The unit for "old" is **commits behind**, not days: on 2026-08-31 a context two days old was 212
commits behind (#2960). `git rev-list --count <run head_sha>..origin/main`.

### 3.1 build container Rust build evidence is non-isolated (#3033) — what that does to REAL-GREEN

**The fact.** The three build container runners (`build-runner{,-2,-3}`) share one unisolated Rust build
cache — one `CARGO_TARGET_DIR` (#2964) and one Cargo/rustup home (accepted as a trade-off in
#2929). Proven on 2026-09-01: #3025's `deploy feature set compile + test --no-run` (run
33547533376, job 99988643346, 20 real steps) failed with `E0599`/`E0271` inside
`native-chain/src/bin/launch_api/transition_extractor.rs` on symbols that exist only in #3028's
never-merged branch; #3025 touches neither that file nor that crate. The same mechanism runs the
other way: a GREEN can link a sibling PR's artifacts and pass without ever compiling this PR's
source. #3033 has the full account.

**What the evidence rules are, while this holds** (this section records rules, not a plan):

1. **A build container Rust GREEN is REAL but not sufficient.** It proves steps ran and exited 0. It does
   not prove the PR's own code compiled or its tests ran against its own tree. For a PR whose
   diff touches Rust, the reviewer needs one of:
   - a **local run in an isolated `CARGO_TARGET_DIR`** — `CARGO_TARGET_DIR=$(mktemp -d)
     cargo test -p <crate>` on the PR head, pasted with the head SHA — which is currently
     **stronger evidence than a hosted or build container green**; or
   - the same job green on a run whose window overlapped **no** other in-progress build container Rust
     job touching the same crates (verifiable from `runs?status=in_progress` timestamps, and
     rarely true on a contended queue).
2. **A build container Rust RED naming paths absent from the diff is a CONTAMINATION CANDIDATE**: record
   run id, job id and the exact error on the PR, then **read the failing test or error** before
   naming the cause. It is a contamination *finding* only when the error names a symbol, type or
   module that exists in no ancestor of the PR's merge tree (or is missing one the PR itself
   adds), or the log shows `Blocking waiting for file lock on build directory` during a sibling
   Rust job. A test that ran and failed on its own logic is something else (#3012 → #3035). Either
   way, **do not touch the named code**: a push "to fix it" is a synchronize event that re-rolls
   the same dice and may carry someone else's change into the PR.
3. **Required evidence now includes Rust indirectly.** Three direct contexts are Python/YAML;
   `repository-input integration admission` also reads integration-ratchet Rust execution on
   relevant inputs. Its green is subject to candidate/base identity and non-vacuity checks; the
   historical cache concern cannot be dismissed merely because admission's own job runs Python.
4. **Strict is currently enabled** (§2.2). Historical refresh costs do not supersede that rule.

**Status of the cause, kept separate on purpose.** The isolation fix is on build container and is the
**owner's**; it is **escalated as #3033, not scheduled**, and nothing in this repository can
perform it. This document does not assume it is coming. When it lands and #3033 closes, rule 1's
"stronger evidence" clause is what changes; until then, it stands.

## 4. What a reviewer may take on trust, and what not

**May take on trust**

- The ruleset contents in `.github/merge-admission.json` — the control's job is to keep them true.
- `runner_name` and step counts as the API prints them.
- A PR comment that attributes each red check by file path *and* cites the run id — once, from
  whoever did the work; re-deriving it per reviewer is what #2981 cost.
- A green `verify-terminal-skip-guards` as meaning the *structure* of fan-out workflows is intact.

**May not take on trust**

- A green rollup, `mergeStateStatus: CLEAN`, or "checks passed" from a PR body. CLEAN means "no
  failing required check and no conflict"; a PR with no checks at all is CLEAN.
- Any timestamp on a re-run as evidence of freshness (#3005).
- `steps > 0` as "it ran self-hosted"; a duration as a runner class; a green tick on a job whose
  name renders `${{ … }}` unexpanded (that is a skipped job, #3015).
- A `404` from `branches/main/protection` as "unprotected". The requirement lives in a **ruleset**;
  read `rules/branches/main` (#2896 correction, #2960's premise).
- That a gate covers what its name says. Read the run record, not the workflow (#2990's
  observation: a checker tells you what it intends to check; only the run record tells you what it
  checked).
- **A build container Rust green as proof that this PR's code compiled or passed** (§3.1, #3033). Ask for
  the isolated local run; a RED there that names files outside the diff is someone else's build.

## 5. Changing the contract

The live ruleset (owner-only), `.github/merge-admission.json` and this page must agree.
A PR may reconcile stale files to already-live rules without changing settings, as on 2026-10-07.
For a future policy change, publish the exact proposed manifest/doc diff for review; the owner
alone applies the corresponding ruleset change, then verify effective rules via
`gh api repos/wrfcoin/core4/rules/branches/main` (and each other affected repository).
Re-read metadata and run the comparator against all repositories. Never change rules to quiet a
checker finding. Do not infer equality from a green `Merge admission contract` job: its live step
is advisory (Refs #3198), while the comparator itself still reports DRIFT/VOID.

## 6. Worked examples — four incidents, four rules

### Emission helper test arity drift (#3074)

Changing an emission helper's signature can auto-merge with concurrent test
call sites without a textual conflict. `cargo check --workspace` does not
compile those test bodies: its green result is **VOID evidence for test-code
arity drift**. Run `cargo test -p wrfcoin-shared-protocol --lib` on the exact
candidate head as the gate for this surface.

In tests, use `chain.state.emission_for_tests()` (or
`state.emission_for_tests()` for a standalone state fixture). This hidden test
convenience view's `per_user_emission_cap`, `coinbase_allowance`,
`contribution_reward_allowance`, and `contribution_decision_base` methods take
only the height and resolve the recorded genesis chain ID from that state.
Do not pass literal `None` or read a process environment chain ID when asserting
allowances for a chain fixture. Arithmetic-only legacy vectors may explicitly
construct an unconfigured state; chain 777 vectors must use their configured
state. Tests comparing the identity tables directly may still call the
production helpers with explicit identities.

The view is compiled only under `cfg(test)` or the existing `test-support`
feature. Shared-protocol integration tests and native-chain test targets enable
that feature through dev-dependencies, so the migrated tests remain runnable
without changing their call sites or silently skipping targets. Normal builds
with resolver 2 do not activate these dev-only features. Validate both required
integration targets as well:

```sh
cargo test -p wrfcoin-shared-protocol --test operator_lifecycle_and_supply_cap
cargo test -p wrfcoin-native-chain --test cut_composition_lifecycle_2498 --features cut-composition-harness
cargo check -p wrfcoin-shared-protocol
```

For the production API boundary, a separate consumer importing `EmissionForTests`
or calling `State::emission_for_tests()` must fail to compile without
`test-support`, and compile when that feature is explicitly enabled. A green
library check alone does not prove absence of an exported helper.

Each rule below has a scar attached, because a rule without one gets re-litigated.

### 6.1 A required check that never evaluated `main` (#2981 → #2990)

`schedule-lint` became required on 2026-08-31. Its workflow triggered on `pull_request` and
`workflow_dispatch` only, so **`main` itself was never within its scope**: `main`'s check-runs
contained no `schedule-lint` at all (`3a056d8b0`). Every PR was gated on a property nobody measured
on the protected branch. The one-line fix (#2990) adds `push: [main]`.

**Rule: a required context must evaluate `main` on push.** Otherwise the default branch's state
under that rule is unmeasurable, and a violation that lands by any non-PR path is invisible until
the next PR is blamed for it.

### 6.2 A required merge-tree check that passed on a tree that no longer existed (#3005 → #3008)

`detect-secrets baseline audit (merge tree)` checks out `refs/pull/N/merge`, computed when the run
is *created*. When `main` moves, GitHub does not recompute it and does not re-run. With strict off,
#2982/#2989/#2983 were mergeable on a scan of their branch against a nine-hour-old `main`. Worse:
`gh run rerun` on #2980 replayed **three findings #2984 had already deleted from `main`**, with a
fresh timestamp, a named runner and a full step list. On 2026-09-01 at 18:30Z, **15 of 15** open
PRs had every required-context result older than `main`'s last commit.

**Rule: freshness is `run.created_at` versus `main`'s last commit, and a re-run is not a refresh.**
Push to refresh a stale merge ref. This incident predates the current in-job freshness guard
and core4 strict=true policy; §2.2 records today's rule.

### 6.3 Green contexts about trees nobody evaluated, and a tool asserting a state that did not exist (#2960 → #2963, #2965, #3012)

`unsafe policy` was green on `main` from 2026-08-01 — 359 commits behind — while every current run
of it was VOID (`runner="" steps=0`), so the gate had produced no evidence in 30 days and the badge
said otherwise. `wrfcoin-cli-integration` was 467 behind and its one real run had *failed*. Both are
fixed by evaluating `main` on push. `Setup Branch Protection` was green on `main` while asserting a
protection state that did not exist — and reading its job showed it would `PUT` legacy protection
requiring **nine contexts no workflow produces**, with `enforce_admins: true`: one dispatch from
locking every merge. It is deleted (#3012), because the honest version of that tool is this page.

**Rule: commits-behind is the unit of staleness, and a tool that asserts a state is a gate — if
the state it asserts is false, delete the tool rather than relabel it.**

### 6.4 A repository where nothing is required (infra#916)

**Historical 2026-09-01 incident (superseded):** `wrfcoin/infra`'s `main-protection` ruleset had deletion, linear-history and pull-request rules and
**no status rule**. Twelve merges landed there on 2026-09-01; each was admitted by a reviewer reading
a checks list in which a RED, a VOID and an absent check look different only if you know to look.
The proposal (two self-hosted, unfiltered contexts; zero open PRs blocked; single-runner blast
radius stated) is infra#916 and is owner-only.

Today infra requires `scripts/tests suites` and `dependabot-coverage` with strict=false (§1).

**Rule: read `rules/branches/main`, not `branches/main/protection`, to learn what gates a merge; if
there is no `required_status_checks` rule, say plainly that admission is a reviewer's word — and
propose, never apply, the fix.**

### 6.5 The small one: a skipped job that looked broken (#2981 → #3015)

A skipped job's `name:` template is never evaluated, so `secret-leak … (${{ … }})` rendered
literally on every branch push and produced a false finding. Names are now static and say why the
job is skipped. **Rule: a skipped-by-design job says so in a stable name.**

### 6.6 A green that ran, and proved nothing: the shared build cache (#3033)

One run, one PR, contamination in **both directions**. #3025 touches nine `lib.rs` files and adds
one module, `primitives/src/integration_inventory.rs`. Its deploy-feature run 33547533376 on
2026-09-01 failed twice, for two opposite reasons, on a tree that was internally consistent:

- **Direction 1 — a foreign artifact broke code the PR does not contain.** Job 99988643346
  (`build-runner`, 20 real steps) failed compiling `transition_extractor.rs:2066,2385` — main's
  own lines, untouched by #3025 — with `E0599`/`E0271` on `BTreeMap<String,
  AttesterSettlementDebt>`. That type exists in exactly one tree in the repository: an unmerged
  branch of a *different* PR (#3028/#3031), whose jobs were compiling `shared-protocol` into the
  same `/build/target` on `build-runner-2` at 19:48–19:50 while #3025 checked out at 19:49:19.
  #3025's log shows `Blocking waiting for file lock on build directory` three times — a lock can
  only contend on the same path.
- **Direction 2 — a stale artifact hid the PR's own new code.** Job 99988642349 (`build-runner`,
  19:38–19:41) failed with `unresolved import wrf_primitives::integration_inventory` at
  `shared-protocol/src/lib.rs:72`. #3025 *adds* that module. The `wrf_primitives` rlib the job
  linked was built from a tree without it — a `main`-tree job started on another runner at
  19:39:54 — so the build could not see the PR's own change.

**Direction 2 is the important half.** A build that cannot see the PR's new code and *fails* is
one flipped bit away from a build that cannot see the PR's new code and *passes*: the same
mechanism that hid a new module behind an old rlib will, when the old rlib happens to be
compatible, produce a green that executed every step against somebody else's compilation of the
PR. That is the contaminated green, and since this run it is not hypothetical. The mechanism was
then confirmed rather than inferred: all three runner `.env` files set
`CARGO_TARGET_DIR=/build/target`; artifact identity is path-independent across checkouts; and a
local probe with two worktrees showed Cargo's path-dependency freshness is **mtime-based, not
content-based** — a content-different, older-mtime source accepted the foreign artifact with
`Finished` and no compile, and the accepted rmeta contained zero occurrences of the new symbol.

**Rule: on a shared, unisolated build cache, "executed nonzero steps and passed" is not evidence
that the PR's code compiled — ask for the isolated local run (§3.1), read the failing error
before calling a red contamination (§1 rule 3), and never fix code the diff does not own.**
The cause is escalated to the owner in #3033; this document records the rule, not the repair.

## 7. Gates this epic recommended deleting

Listed here because this is the part most likely to be wrong and it should be the easiest to find.

| Deleted / removed | Where | Why |
|---|---|---|
| `branch-protection-setup.yml` and `.github/scripts/validate-cicd-setup.sh` | core4 #3012 | would install nine phantom required contexts with admins enforced; the script asserted a workflow deleted in February |
| the `Shellcheck utility scripts (non-blocking)` step (`… \|\| true`) | infra#917 | a step that cannot fail is not a check; with the trigger narrowed to the linted list, the badge now means what it says |

Not deleted, recorded: `VOID gate detector` exits 2 ("INCONCLUSIVE: only 3 job(s) inspected, need
20") on quiet pushes to `main` and that renders as a RED check. Exit 2 is its VOID class, which is
correct; that GitHub paints it red is the same rendering problem as §6.5, one level up.
