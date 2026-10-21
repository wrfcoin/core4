# Vacuous gates — four checks, and the four that got past us this sprint

A gate is **vacuous** when it cannot fail for the reason it exists. It still runs, still reports,
still gets cited in a review. It just carries no information — and a green that carries no
information is worse than no gate at all, because the next reader stops looking.

Everyone already knows to be suspicious of this. The generic version of this document exists in
everyone's head and has not helped. So this one is short and made of things that actually happened
here in one sprint, with issue numbers you can open.

## The four checks

### 1. Can you make it go red?

Run the gate against a tree where the thing it guards is genuinely broken. If you cannot produce a
red, it is not a gate.

The mirror image counts too: a gate that cannot go **green** is equally uninformative, and it is
easier to miss because a red looks like it is working. **core4 #2698** — the only two-node
acceptance script in the repo could not cut block 1, because `WRFCOIN_GENESIS_ORACLE_SET` was
unset. It could not pass on any tree, so its red said nothing about the system.

**That example has since been fixed, and saying so is the point.** #2709 merged on 2026-08-04
(merge commit `23e8d62abe1a`, on `main`), and `scripts/verify-oracle-registration-twonode.sh` now
sets `WRFCOIN_GENESIS_ORACLE_SET` at line 142, so the script can both pass and refuse. An earlier
revision of this document said the defect was "still true of `main`" and that #2709 "was still a
draft" — that went stale within a day of being written. A stale example in a document about stale
claims would be its own entry in the list, so this paragraph is the worked example: **re-check
every issue number below against current `main` before you quote it.**

### 2. Did it execute anything?

A **skipped** job reads as success in a rollup. So does a zero-step job, a stale-head result, and a
wrapper job whose only content is `if: always()`. Say which of three you have — REAL-GREEN (nonzero
steps, passed), REAL-RED (nonzero steps, failed), or VOID (everything else). VOID can neither admit
nor block.

`continue-on-error: true` *manufactures* VOID: it converts a real failure into wrapper-only success.
It is the most common way a gate is silenced without anyone deciding to silence it.
**core4 #2711** deliberately did not use it, and instead asserts `2 passed` in its control step so a
filter that matched nothing cannot report green with zero assertions executed.

### 3. Does it touch the production thing?

**frontend #274** (issue, now closed) — `Dashboard.test.jsx`: 18 tests, zero production coverage. It imported no
component; it tested a mock defined in the same file. Eighteen green assertions about a fixture.

The check: delete the production module and re-run. If the gate is still green, it was never
measuring it.

### 4. Is the claim demonstrated, or asserted?

**core4 #2700** — `MAX_BLOCK_SIZE` is credited in the threat model as a resource-exhaustion
mitigation and is consumed by **zero lines of code**. The workspace's one block-size comparison is
dead three times over: no call sites, a different constant (a 2 MiB config default, not the declared
32 MiB), and a size function that ignores `tx.data` and so could not detect an oversized block even
if it ran. **#2704** strikes the claim.

## The one that matters most: it recurs one level up

**core4 #2701** is the RED test written to expose #2700. Its redness lived in a hand-run terminal
and a PR body. Its head reported **20 passing checks, 5 skipped, zero red** — because not one of
them ran the test the PR adds. A PR whose entire content is a deliberately failing test showed an
all-green rollup, and **#2704** cited that undemonstrated red as evidence.

So: a correction to a document about an unfalsifiable mitigation was itself resting on an
unfalsifiable gate. The defect reproduced one level up, in the work written to fix it, by people who
had the defect fresh in mind. **#2711** wires the check.

That is the actual lesson. Not "watch out for vacuous gates" — everyone does. It is that the
recursion is the normal case, so after fixing one, ask the same question about the evidence you just
produced.

## The shape that fixes all four: control first, then observation

The four checks above are things to *ask*. This is the thing to *build*. It is two steps in one job,
in this order, and it is not specific to any of the examples here.

```
STEP 1 — CONTROL.   Run the assertions that prove the gate can see.  MUST PASS.
                    Assert the count that ran, so a filter matching nothing
                    cannot report green with zero assertions executed.
STEP 2 — OBSERVE.   Run the assertion the gate exists for. Classify the result:
                      exit 0  → the guarded condition holds
                      exit 1  → REAL-RED, attributable to the guarded condition
                      exit 2  → GATE BROKEN / VOID — not evidence either way
```

**Step 1 is the gate proving it works before it reports.** A scanner that finds nothing looks
identical to a codebase with nothing to find; a suite that imports no production module looks
identical to a module with no bugs. The control is what separates them, and it must run *first*, so
that when step 2 goes red the green above it is already on the screen.

**Step 2's three-way split is the part usually missing.** Most gates have two outcomes, pass and
fail, which forces every failure to mean the same thing. It does not: a compile error, a renamed
test, a missing fixture and a genuine finding all exit non-zero. Collapsing them is how a broken gate
gets read as a finding, and — worse — how a real finding gets dismissed as "probably just the
harness". Separating them costs about ten lines of shell.

> **A red can never be a compile error wearing a finding's clothes.**

That is the whole principle. Exit 1 says *the thing I was built to detect is present*; exit 2 says
*I could not tell, and you should not conclude anything from this run in either direction*. A gate
that cannot say the second one will eventually say the first when it means the second.

Worked example, and the reason this section exists: `declared-ceiling-enforcement.yml` (#2711) runs
#2701's two anti-vacuity tests as step 1 and requires `2 passed`, then runs the ceiling assertion as
step 2 and classifies it. On its first run the control was green and the observation was exit 1 —
which is what makes that red citable in #2704. The ceilings are the example; the two-step shape is
the reusable part.

One corollary about silencing: `continue-on-error: true` is not "step 2 with a softer outcome". It
deletes the distinction entirely, mapping all three outcomes onto green. If a red is genuinely not
allowed to block, make the check advisory by removing it from the required set — a decision recorded
where checks are configured — rather than by blinding the check itself.

## Two habits that cost nothing

**Pair every negative result with a positive control.** A scanner that finds nothing looks identical
to a codebase with nothing to find. #2701 asserts that >200 `.rs` files were walked and that
`MAX_BLOCK_EMISSION` — a constant that genuinely *is* enforced — is seen as enforced. Without those,
its three findings would be indistinguishable from a broken path.

**Keep the checker out of the thing it checks.** `terminal-skip-guard-integrity.yml` carries no
paths filter and lives in its own file, deliberately: verifying terminal guards from inside their own
filtered workflows is vacuous, because the change that breaks the guard is also the change that
stops the workflow running.

## The zero-match run — this class has a tool now (#2861)

`cargo test` **exits 0 when a name filter matches nothing** (`0 passed; 1747 filtered out`) and
when a feature-gated test compiled out of the binary entirely. That is a VOID result wearing a
green exit code, and it escaped twice in one night: **#2851** merged over a red payout assertion
behind `cut-composition-harness` (fixed in #2863), and two `test-support` emission tests went red
with no gate anywhere able to notice. Each earlier escape got a per-step `grep` patch
(`ci-deploy-features.yml` has several); a per-step patch protects exactly one invocation.

The single enforcement point is **`scripts/cargo_test_nonvacuous.py`**: it runs the given `cargo`
command and fails loudly when zero tests executed, without anyone having to know which features
exist. Use it for ANY workflow step or lane contract that names a test target or filter:

```
python3 scripts/cargo_test_nonvacuous.py [--min-executed N] -- test --locked -p <pkg> \
    [--features <feats>] [--test <target> | <filter>] [-- <libtest args>]
```

Two things it teaches beyond the obvious:

- **Partial vacuity is the sneakier variant.** A filter like `_2492` matches 8 tests with
  `test-support` enabled and still matches 4 without it — a green run that silently dropped the
  gated half. That is why `--min-executed N` exists: pin the count you expect, exactly as this
  document's control-step shape already demands ("assert the count that ran").
- **The wrapper proves its own RED** (`--self-test`, no cargo required), so a workflow can run the
  control before the observation.

## The standing red — a gate that cannot go green is also a coverage hole (#2911)

Section 1 says a gate that cannot go red is not a gate, and notes the mirror image. **#2911 is the
mirror image, and it cost 27 days.**

`chain_identity_split_2713` is RED BY DESIGN: it asserts the property the node should have and
#2713's divergent literals are still there, so its redness is the proof (`19af70d33`, 2026-08-04).
Nothing about that is wrong. What was wrong is that it made `cargo test -p wrfcoin-native-chain`
permanently exit `101`, and a permanently-red command **cannot report a fresh failure**. From
2026-08-04 to 2026-08-31 a new break anywhere in native-chain produced a byte-identical observable
to the accepted one, and telling them apart meant diffing failure names by eye against a 27-day-old
commit message.

Two mechanisms, and the second is the one that surprises people:

- **Verdict masking.** exit 101 == exit 101. The gate had one bit of output and the standing red had
  already spent it.
- **Execution masking.** Without `--no-fail-fast`, cargo *stops at the first failing test binary*.
  `chain_identity_split_2713` sorts third of 45 native-chain integration targets, so the package
  command never built most of the suite. Those tests did not pass — **they never ran**, and no count
  floor could see it, because there were no counts to floor.

The fix is the same as everywhere else in this document: **make the observable emit a positive,
attributed statement instead of a bare verdict.** `scripts/package_test_baseline_gate.py` +
`scripts/package_test_baselines.json` declare the standing failures *by identity*, run the package
with `--no-fail-fast`, and classify three ways — `NEW_FAILURE` (a failing test the baseline does not
declare, including one inside the standing-red binary), `HEALED` (a declared failure PASSED: promote
it, or the assertion was weakened), `MISSING` (a declared failure was deleted, the other way to make
a standing red disappear). `--no-fail-fast` is enforced by preflight rather than documented, because
it is the difference between "the rest passed" and "the rest never ran".

Three rules this class leaves behind:

- **Never fix a standing red by weakening the target.** Re-targeting the *observable* is the fix.
  #2911 exists to record that weakening is the failure mode, not the remedy.
- **A baseline is not an exemption.** An entry is a promise that its issue is open and that the RED
  is the proof; a declared failure that starts passing must be LOUDER than one that keeps failing,
  or the exception becomes permanent at the exact moment the defect is fixed.
- **Pin identities, never counts.** `chain_identity_split_2713` mixes two `control_*` tests with the
  four observations, and repairing #2713 deletes the literals `control_a` asserts are present. A
  count-based baseline would let that broken control stand in for the four and keep the aggregate
  looking like the accepted red (#2854).

## And two mechanical traps

- **Never pipe a gate through `head`/`tail`.** Twice this sprint a RED run was reported as `exit 0`
  because the summary got clipped. Read the exit status, and capture `rc=$?` on the very next line —
  a `printf` containing a subshell resets `$?`. Under `bash -e` (GitHub's default for `run:`), the
  step aborts *before* your classification runs unless you `set +e` first.
- **Name the failure you expect.** An assertion that matches any failure will happily match the
  wrong one — a guard test that passes because of an unrelated error is green for a reason nobody
  checked.

---

Examples: core4 #2698 (fixed by #2709), #2700 (open), #2701, #2704, #2711 (open drafts);
frontend issue #274 (closed). Precedent for advisory never-blocking checks: frontend #271, #272
(both merged 2026-08-04).

_Example states verified against core4 `main` `1cf2873e2` on 2026-08-30. `MAX_BLOCK_SIZE`,
`MAX_TRANSACTION_DATA_SIZE` and `MEMPOOL_CAPACITY` were re-checked on that tree and each still
appears in exactly one `.rs` file — its own definition in `shared-protocol/src/common/constants.rs`
— with no consuming call site, so the #2700 example in section 4 still holds._
