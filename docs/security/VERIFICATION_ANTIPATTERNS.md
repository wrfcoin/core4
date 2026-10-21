# Verification Antipatterns — how a gate, test, or document passes while verifying nothing

Refs #2284. Patterns 1–20 were compiled 2026-07-22 from the primary review
record of the Crypto-2026 pre-testnet sprint. Pattern 21 and the sweep evidence
inside the earlier entries come from a systematic audit of nine workflows in
Rounds 55–57. Pattern 22 (§J — issue lifecycle, one level up from the gates)
was added the same day after two independent lanes caught the same
risk-retirement failure shape hours apart — once as a near-miss corrected
before any closure, once as completed closures. Every pattern below was found **in this repository, this
sprint**, by a reviewer who probed rather than read; every instance cites its
PR or issue and the command or control that exposed it. Nothing here is
hypothetical.

**How to use this document:** when writing or reviewing a gate, test suite, or
security document, walk the probe column. Each probe is a concrete action that
was actually performed and actually caught the defect. "Looks right" caught none
of these; the probes caught all of them.

The acceptance rule produced by the systematic sweep:

> Put one representative violation in every advertised class, prove the exact
> authoritative workflow command turns red, and prove the same workflow runs
> on the change surface it claims to gate.

The one meta-rule, from #2284, which generalizes across most of this catalogue:

> Testing that a gate catches a violation in a *fresh* location is the easy case
> and all three passed it. The coarse-granularity hole only shows up when the
> new violation hides inside something already accounted for.

### Systematic nine-workflow sweep — boundary and aggregate

The following numbers describe behaviour **at each audit head**; they are not a
claim that later remediation is absent. In particular, merged **#2304** now
hardens the release-signing and deployment gates, and **#2310** adds behavioural
detection for one of the five shared/native modules.

- **PR reachability: 0/9 (0%)** named workflows had a direct `pull_request`
  trigger at their audit heads.
- **Complete enforcement: 0/9 (0%)** rejected every violation represented by
  the workflow's own name and documentation.
- **Direct red demonstration: 8/9 (89%)** had an observed failing command or
  job from a controlled violation or current baseline. At mechanism level the
  number is **9/9**: the deployment gate's final aggregator could reject a
  failed job conclusion, while its concrete smoke/startup failures were being
  converted to success.
- **Permanent baseline red: 4/9 (44%)** were locally reproducibly red before a
  new relevant violation was added.

The evidence ledger is the complete sweep comment on #2284; the three final
workflow findings are #2316, #2317, and #2318.

---

## A. Baselines and ratchets

### 1. A violation inside an existing baselined unit

**What it looks like.** A debt baseline (fingerprints, counts) whose unit of
identity is coarser than one violation, so a new violation that lands inside an
already-recorded unit changes nothing the gate measures. From #2284:

> Each gate identifies or counts violations at a **coarser granularity than the
> violation itself**, so a new violation that falls inside an existing unit
> passes without touching the baseline.

**Real instances — found independently four times:**

- **#2247** (`unsafe` gate, fingerprint `path|text`, no line): *"The gate
  watched the unjustified count rise from 4 to 5 and reported `0 NEW`."* The
  planted block was a bare `unsafe {` in `consensus-engines/src/ffi.rs` — *"A
  bare `unsafe {` is the single most common form of an unsafe block in Rust,
  and `ffi.rs` is precisely the file this story exists to protect."*
- **#2248** (panic ban, fingerprint `path|rule|text`): *"Because the baseline is
  a set of `path|rule|text`, a new panic with identical source text anywhere in
  the same file reuses an existing fingerprint and passes."* The reviewer's
  probe was *"a remotely reachable panic accepted with no baseline change."*
- **#2278** (unwrap ratchet, counted lines): *"Added a second actual `.unwrap()`
  statement to an already-counted production line — blockchain remained 314 and
  the ratchet exited 0. The gate counts matching lines (`grep ... | wc -l`),
  not occurrences."*
- **#2288** (ZK params drift guard, shape-tuple fingerprint): *"a constraint
  coefficient, constant, or wire selection can change the exact A/B/C matrices
  while preserving public-input, variable, witness, and FFT-domain counts. A
  Groth16 key is bound to those exact matrices, so a same-shape stale key can
  be incompatible while `assert_bundle_matches!` still passes."*

**The probe** (verbatim from #2284): *"Introduce a new violation **inside an
existing baselined unit** — same file, same line, same rule, whatever the
gate's unit happens to be — and confirm the gate reports it."* If it passes,
the unit is too coarse.

### 2. A baseline that can grow in the same change it gates

**What it looks like.** The gate trusts whatever baseline file is in the PR, and
the script ships an `--update-baseline` command. A violation plus a one-command
baseline regeneration is green; nothing compares the baseline to its merge-base
state.

**Real instances:**

- **#2247**: *"`--update-baseline` in the same change converts a planted
  failure into a green gate, directly contradicting the policy statement that
  counts may only shrink."*
- **#2245** (determinism lint): *"I generated a temporary expanded baseline
  containing the planted float/clock/RNG findings: it grew from 726 to 730 and
  the gate then passed with those new violations. CI permits changing
  `determinism_baseline.json` in the same PR, so review is the only guard."*
- **#2248**, including the half-fix: a migration escape hatch that is a boolean
  rather than a ledger — *"Any modification to `panic_input_baseline.migrations`
  grants a blanket `--allow-migration`; the log is not parsed or matched to the
  added fingerprints."*

**The probe.** Plant a violation, regenerate the baseline in the same working
tree, run the gate. It must still fail. The accepted fix shape (from #2248's
approval): *"ratchet growth is accepted only when each added fingerprint is
explicitly named."*

### 3. A mutation test drawn from the class the gate already detects

**What it looks like.** The gate's author does mutation-test it — using the
historical failure that motivated the gate, which is exactly the case the
gate's fingerprint was designed around. The test passes and appears to validate
the guard; the adjacent undetectable class is never exercised. From #2284:

> #2288's author *did* mutation-test the guard — by swapping in a stale bundle
> that changed variable count 308 → 68. That mutation is in the class the
> coarse fingerprint already detects, so it passed and appeared to validate the
> guard. **A mutation test that only exercises the detectable class proves less
> than it appears to.** The mutation has to be chosen adversarially against the
> fingerprint's own definition, not against the historical failure that
> motivated the guard.

**Real instance.** #2288, resolved by exactly such an adversarial mutation in
the follow-up review: *"swapped only the allocation order of `value` and
`blinding` in `RangeProofCircuit`. The witness stayed valid and the reported
shape remained exactly `public_inputs=2 variables=308 domain=511
witnesses=306`; proof generation succeeded, verification rejected …"*

**The probe.** Write down the gate's unit of identity, then construct a
violation that holds that unit fixed. The mutation is chosen against the
fingerprint definition, not against history.

---

## B. Coverage and scope

### 4. A registry that never notices an unregistered member

**What it looks like.** A harness runs everything on a manually curated central
list, and every test exercises only things on the list — so omission from the
list is structurally invisible.

**Real instance.** **#2260** (invariant harness): *"`with_defaults()` is
manually curated in `harness.rs:91-126`, while `assert_unique_ids()` at
`:213-225` validates only entries already in that list."* Consequence, against
sibling #2250: *"every #2260 harness test remains green and the supply
predicate still runs nowhere."*

**The probe.** Write a predicate, do not register it, and assert the harness
fails. The accepted fix demonstrated exactly this: *"adding an unregistered
predicate to `balance.rs` caused `shipped_registration_is_complete` to fail,
naming `INV-PROBE-9`, its file, and `with_defaults()`."*

### 5. The vacuous gate — a correct mechanism that nothing exercises

**What it looks like.** The gate's logic is verifiably correct, and its input
set is empty, so it does zero work and exits 0 forever.

**Real instance.** **#2251** (fuzz corpus replay, empty corpus):

> **Step 7 ran for zero seconds.** The job reports a green "Fuzz corpus replay
> (regression gate)" check having replayed nothing, and will keep doing so
> indefinitely — including if the script were deleted and replaced with
> `exit 0`. Nothing distinguishes a working gate from a removed one.

And the reviewer's naming of the class: *"This is the 'correct mechanism that
nothing exercises' pattern, and the empty corpus makes it self-perpetuating:
the gate cannot fail, so no one will notice it never ran."* Also: *"an empty
corpus turns a correct gate into a permanent green light, and that is worse
than no gate, because the check name asserts coverage that does not exist."*

**The probe.** Check the step's wall-clock (a zero-second load-bearing step is
the tell); make empty input a failure, and add an in-CI self-test that plants a
deliberately-failing input and asserts the gate exits non-zero.

### 6. A test that exists but nothing runs

**What it looks like.** The suite is committed, correct, and green locally —
and no CI invocation executes it. Compilation (`--no-run`, clippy) masquerades
as execution.

**Real instances:**

- **#2288**: *"The new integration guard is not executed by CI.
  `.github/workflows/ci-readiness.yml:405-408` runs `cargo test --workspace
  --lib --all-features`, which excludes `native-chain/tests/zk_params_drift.rs`;
  the all-targets clippy job only compiles it."*
- **#2236**: *"The live PR checks compile shared-protocol tests with
  `--no-run`, but none executes `shared-protocol/tests/wycheproof.rs`."*
- **#2311** (`adversarial-tests.yml`): the exact workflow command
  `cargo test --locked -p wrfcoin-native-chain --bin launch_api -- --nocapture`
  stayed green at 258/258 after a deliberately failing
  `native-chain/tests/adversarial_r56_probe.rs` was added. Running that test
  explicitly with `--test adversarial_r56_probe` exited 101. The test existed;
  the command selected a different test target.

**The probe.** Find the exact CI line that invokes the test; if none exists,
that is the finding. Then prove enforcement with one deliberately red run
(pattern 17's probe).

### 7. Hand-listed coverage — scan roots, trigger paths, and carve-outs

**What it looks like.** The scanner's roots, the workflow's `paths:` filter, or
a scan exclusion is a hand-maintained list, so the checked set silently differs
from the true set — in either direction: attack surface omitted, or an excluded
hole with no boundary.

**Real instances:**

- **#2248** (roots): *"I planted `payload.get("elsa-required").unwrap()` inside
  `bridge_weather_payload_to_mempool`, which parses peer-delivered raw weather
  bytes in `native-chain/src/bin/launch_api/p2p_events.rs:277-312`; the gate
  still exited 0 with 'no new panic-on-input risk.'"* And on the duplicated
  list: *"The claimed 'single editable constant' is therefore not true
  operationally."*
- **#2245** (roots + trigger): *"I planted `SystemTime::now` under the live
  shared-protocol blockchain path and the baseline gate exited 0 with `0 NEW`.
  The workflow path filter also means a PR changing only that directory does
  not start this job at all."*
- **#2251** (trigger): *"A PR touching only `privacy/**` or `zk-circuits/**` —
  the change most likely to reintroduce a crash in those targets — skips the
  gate entirely, which is the exact failure mode the story exists to prevent."*
- **#2247** (unchecked root, live UB candidate): *"`tools/cli/WrfcoinCLI/cli/
  commands.rs:378`, which is `static mut BURN_WALLET_REGISTRY` handed out as
  `&'static mut` from an `unsafe` block with no `// SAFETY:`"*.
- **#2282** (carve-out with no boundary): *"the blanket directory exclusion is
  fail-open. ... A modified vector or a newly added file containing a real
  secret will now be invisible to `detect-secrets`."* Accepted fix: exclude
  exact paths and pin their hashes, so *"excluded bytes cannot change unnoticed,
  while all other files remain subject to the normal scan."*

(The same class was found on #2238 and #2239's scanner roots; those probes are
recorded in their review threads and are not quoted here.)

**The probe.** Plant a violation in the most plausible *omitted* path — the
reviewer picked real attacker-facing code, not a toy file — and run the gate.
Separately: make a change that touches only an omitted path and check whether
the job starts at all. For every exclusion, ask what enforces its boundary.

---

## C. The test that tests nothing

### 8. Test and bug share the same misunderstanding

**What it looks like.** The self-test's fixtures are written in the shape the
detector's regex matches, so the fixture suite can never disagree with the
detector — they encode the same wrong model of the input.

**Real instance.** **#2245**: *"The regex at `scripts/check_determinism.py:68-71`
requires `HashMap`/`HashSet` and `.iter/.keys/.values` on the same source line;
the self-test uses exactly that artificial one-line shape. A normal parameter
declaration followed by `for value in values.values()` on a later line bypasses
it."*

**The probe.** Write fixtures from real code in the tree (or realistic
multi-line shapes), never by copying the pattern out of the detector. If the
fixture and the regex were written by the same hand in the same hour, assume
this pattern until proven otherwise.

### 9. The subject under test is a model of the subject

**What it looks like.** The suite constructs its own local implementation of
the mechanism (its own preimage builder, its own tag table) and tests that —
so production can lose the property entirely while the suite stays green.

**Real instance.** **#2235** (cross-domain replay): *"It tests a synthetic
local preimage model, not real signed objects or production verification
paths."* And the decisive consequence: *"Removing or breaking domain separation
in production would still leave this suite green, because both signing and
verification use the independent local helper."*

**Systematic-sweep instance.** **#2302**
(`rust-shared-native-parity.yml`) compared `native::` and `shared::` paths that
re-exported the same implementation. Disabling the production stale-weather
guard made the dedicated `temporal_rejects_stale_timestamp` test exit 101, while
all three workflow commands stayed green (`50 + 24 + 3` tests passed). The
suite's second implementation was only a second name for its first.

**The probe.** Break the production mechanism and require the suite to fail.
The accepted revision proved it: *"removed `push_length_prefixed(&mut buf,
tag);` from `crypto::domain_signing::domain_preimage` and reran the full
suite. Result: 7 of 14 failed."*

### 10. The assertion consults the test's own data

**What it looks like.** A "coverage" assertion that compares the test's inputs
to the test's inputs — a hard-coded length, or a prose constant — instead of
reconciling against an external inventory.

**Real instances:**

- **#2235**: *"This matrix has 7 and `matrix_covers_every_object_kind_and_context`
  merely asserts that its own slice length is 7"* — while the real registry had
  18 kinds.
- **#2236**: *"`k256_scope_is_recorded` only asserts that a prose constant
  contains `2126`; it neither executes vectors nor emits a skipped count/reason
  in the harness summary."*

**The probe.** Delete or un-list one real member and require the coverage
assertion to fail. From #2235's accepted revision: *"left `OraclePacket`
registered but removed it from `ALL_SIGNED_OBJECT_KINDS`.
`the_matrix_covers_the_whole_registry` failed with 17 matrix kinds versus 18
registry rows."*

### 11. The name claims what the artifact does not do

**What it looks like.** A fixture, test, or harness whose *name* asserts a
property its *body* does not establish — so every future reader inherits a
false belief with the code's own authority behind it.

**Real instances:**

- **#2237**: *"The fixture named NON_CANONICAL_ORDER_8 uses 01 followed by zero
  bytes. That is the Edwards identity encoding, not a genuine non-identity
  small-subgroup test point."*
- **#2235**: *"The named `removing_the_domain_tag_would_break_this_suite` test
  does not run the matrix untagged; it signs one untagged preimage, verifies
  that same preimage, compares it to itself ... It therefore demonstrates the
  model, not the production replay barrier."*
- **#2236** (a whole harness): the corpus was locally generated, yet every
  surface said Wycheproof — the fix demanded was to *"label the PR body/title,
  fixture name/path, and executed summary as local controls so a pass cannot
  be read as Project Wycheproof coverage."*
- **#2318** (`wrfcoin-cli-integration.yml`): the mocked CLI suites passed
  2/2 and 16/16, but the named live-contract command
  `tools/cli/WrfcoinCLI/scripts/live_contract_smoke.sh` tried to start the
  deleted `backend/server/main_api_server.ts`, exhausted 30 health probes, and
  exited 7. It never reached a live API contract; the job name supplied the
  property the body did not exercise.

**The probe.** Prove class membership in-tree rather than trusting the label.
#2237's accepted fix: *"The in-tree proof checks decompression,
`is_small_order()`, non-identity, `8P = identity`, and `4P != identity`,
establishing exact order 8 rather than trusting the label."*

### 12. Guarding the delegate instead of the thing

**What it looks like.** The test exercises the convenience wrapper; production
calls the inner function directly. A guard placed only in the wrapper passes
the test and misses production.

**Real instance.** **#2294** (#1762 shielded fence): *"`add_transaction`
delegates to `add_transaction_with_signing_keys`, while native-chain and P2P
production callers use the latter directly."* The reviewer's probe: *"Temporarily
moved the shielded gate to the wrapper only:
`both_mempool_entrypoints_reject_shielded_1762` failed on the direct signed-key
entrypoint, proving the assertions cover the load-bearing production path."*

**The probe.** Move (don't just weaken) the guard to the outermost layer only,
and require the test to fail — that proves it reaches the entrypoint production
actually uses. Trace real callers first; the wrapper is rarely it.

### 13. An absence assertion that was never shown falsifiable

**What it looks like.** A test asserting that something does *not* exist (no
second gate, no deeper rejection, no leaked field) that would also pass if the
assertion were wired to nothing.

**Real instance.** **#2294**, proved falsifiable by the reviewer: *"Temporarily
added a deeper shielded rejection:
`nothing_below_the_fence_rejects_a_shielded_transaction` failed on the injected
error, proving the absence test detects a second fence."* The follow-up review
records the design property worth copying: *"the absence assertion expects
successful below-fence application and therefore fails if a redundant deeper
rejection is added, as established by the mutation probe."*

**The probe.** Inject the thing whose absence is asserted; the test must fail.
An absence assertion without this probe on record is presumed vacuous.

---

## D. Counting and grammar

### 14. Fixing one prefix per round instead of closing the grammar

**What it looks like.** A lexer-ish filter fixes each adversarial input class as
a reviewer finds it — `r#"…"#` this round, `cr#"…"#` next round — instead of
enumerating the input grammar as a closed set once.

**Real instance.** **#2278**, round n: the 8-hash raw-string lookahead; round
n+1: *"`strip_comments_and_strings` recognizes unbounded `r...` and `br...`
openers, but not the valid Rust raw C-string prefix `cr...`."* Resolution,
round n+2: *"The committed suite now treats Rust's string/char opener grammar
as a closed set"* — verified against the compiler itself: *"Closed-set check
with repository-pinned `rustc 1.98.0`, edition 2021: every recognized opener
family above compiled. `rb`, `rc`, `bc`, `cb`, `bcr`, `cbr`, `rr`, and `c'x'`
were each rejected."*

**The probe.** Ask "what is the complete set?" and check it against the
authority (here: what the pinned compiler accepts), not against the failures
found so far. Membership and non-membership both get tests.

### 15. Silent under-count in a blocking gate — the wrong polarity

**What it looks like.** A counting gate whose parser, when confused, *erases*
real findings. Over-counting blocks a PR and gets fixed loudly;
under-counting waves violations through and is never seen.

**Real instance.** **#2278**: *"This remains silent under-counting in the
blocking safety ratchet, the dangerous direction."* The accepted end-state
inverted the polarity by design: *"uncertain/malformed constructs move in the
conservative over-count direction"* and *"Deliberate conservative over-counts,
independently observed ... These are documented; none silently erase production
code."*

**The probe.** Feed the counter inputs it cannot understand (malformed,
unterminated, exotic-but-valid) and check which direction it errs. Every
uncertain case must move toward over-count; any input that can blank a real
finding is a blocker.

---

## E. What a CI result is evidence of

### 16. A red check that is no evidence — and red noise that hides real red

**What it looks like.** A job "fails" without ever executing (spend limit,
runner failure), and because the gate is red for everyone, nobody reads it —
so a genuine failure of the same gate is invisible. From issue #2295:

> During that window every job returned `conclusion: failure` with
> `runner_name: ""` and `steps: 0` in 2-4 seconds — the job never executed.
> `preflight` was red for everyone, so a *genuinely* red `preflight` was
> indistinguishable from the void ones, and the convention error hid inside
> the noise.

> **The general lesson, which cost this sprint more than once:** a gate that is
> red for an unrelated infrastructure reason stops being read, and then a real
> failure arrives wearing the same clothes.

Reviewers applied the distinction repeatedly, e.g. #2235: *"Exact-head Actions
failures are infrastructure-void: all six executing jobs had `runner_name=""`,
`steps=0`, and ended in 2–4 seconds. They are neither positive nor negative
code evidence."*

**Systematic-sweep extension — standing baseline red.** Pattern 16 is not only
an infrastructure-failure pattern. Four of nine audited workflows were already
red for a reproducible repository/runtime reason before a new relevant
violation was introduced:

- **#2312** (`docker-security-scan.yml`): the exact compose-validation loop
  exited 1 on six standing failures; a malformed-compose control raised the
  count to seven but produced the same red conclusion.
- **#2083** (`tdd-phase3c.yml`):
  `bash scripts/run-phase3c-security-api.sh --rust-only --no-color` returned
  two passes, two failures, and exit 1 before any new probe.
- **#2316** (`supply-chain-security.yml`):
  `bash scripts/check-dep-budget.sh --verbose` reported 31 direct runtime
  dependencies against a budget of 30 and exited 1.
- **#2318** (`wrfcoin-cli-integration.yml`): the live smoke command exited 7
  because its server entrypoint had been deleted during repository extraction.

These are real reds, unlike runnerless voids, but operationally they converge:
once every scheduled run is expected to be red, a new vulnerability, consensus
regression, or contract failure arrives wearing the routine baseline's clothes.
The gate is executing, yet its result no longer discriminates new information.

**The probe** (verbatim from #2295):
`gh api repos/wrfcoin/core4/actions/jobs/<id> --jq '.runner_name, (.steps|length)'`
— *"`runner_name: ""` + `steps: 0` means the job never ran. That is **void, not
red** — no evidence either way."* A related variant — a re-run whose base
predates the fix that would make it pass ("stale-base red") — was flagged by
the sprint manager as the same evidence class; treat any red as unread until
its run's head SHA and execution are both confirmed. For the standing-baseline
variant, run the exact command on unchanged `main`, record its failure signature,
then plant a new violation and require the output and authoritative verdict to
distinguish the new red from the old one.

### 17. A green check that is no evidence — skips, `continue-on-error`, exit 0

**What it looks like.** The run is green because the interesting legs never
ran, or because step presentation hides a captured failure. Green answers "did
the job exit 0", never "is the property true".

**Real instances:**

- **#2247** (skipped matrix): *"The top-level-success PR run `29892523607`
  contains one skipped, unexpanded Miri matrix and must be disregarded."* And
  earlier: *"the unsafe gate passes and every Miri matrix leg is skipped. There
  is no green schedule/dispatch proof for the claimed three-crate subset."*
- **#2288** (`continue-on-error` presentation): *"The step displays success
  because of `continue-on-error`, but its interpolated outcome is
  `zk_params_drift:failure`; `Enforce Rust test results` reports that failure
  and exits 1."*

**Systematic-sweep instances:**

- **#2316** (`supply-chain-security.yml`): local `cargo audit --json` reported
  seven vulnerabilities and exited 1, but both workflow invocations end in
  `|| true`; the final aggregator reads `AUDIT_STATUS`, `OSV_STATUS`, and
  `VET_STATUS` but never makes them authoritative. A workflow-shaped probe set
  all three to `failure` and still returned `gate_failed=false`, exit 0.
- **#2317** (`sbom-generate.yml`): the pinned CycloneDX validator printed
  `Validation failed` for two malformed documents but returned 0 because the
  workflow omitted `--fail-on-errors` and appended `|| echo`. A fake Grype that
  printed a critical finding and exited 1 was likewise swallowed by the exact
  `--fail-on critical ... || true` shell shape.
- **#2312** (`docker-security-scan.yml`): an exact build-step control using a
  nonexistent Dockerfile printed Docker's error and `Build failed`, then exited
  0 because the command ended in `|| echo "Build failed"` under a
  `continue-on-error` step.

**The probe.** Open the run and confirm the load-bearing leg *executed on a
real runner at the exact head*. Then demand one deliberately red run as
enforcement proof — #2236's shape: *"isolated red run 29897328066 proves one
corrupted NIST byte alone turns that enforcement red."* A gate that has never
been seen red has never been seen working. A gate seen red for only one class
has proved only that class: plant one violation from **every advertised class**
and follow each result through the final authoritative decision.

---

## F. Hermeticity

### 18. A gate consulting a moving external oracle

**What it looks like.** The gate's verdict depends on a live third-party
response, so the result can change with the remote's availability — and the
dangerous polarity is that a definitive remote answer makes the gate
*quieter*. From #2292:

> #2284's pattern in its purest form yet: **a gate that can be green while the
> property is false**, because the mechanism consults something other than the
> repository. Prior instances measured at the wrong granularity; this one
> measures against a moving external oracle.

**Real instance.** **#2292** (detect-secrets Slack verification):
*"A controlled monkeypatch of Slack responses proves the directionality: a
definitive non-valid response yields **0** Slack findings (all three are
filtered as `VERIFIED_FALSE`); simulated connection exceptions yield **3**
retained findings (`UNVERIFIED`)."* — *"The direction is controlled by the
external response, not repository content."* The analysis also named the
secondary defect: *"The workflow does not log per-plugin verification results
or outbound response status ... That observability gap is itself part of the
non-hermetic problem."*

**Systematic-sweep extension — moving inputs found in workflow mechanics.**
The sweep found the same risk outside a scanner plugin:

- **#2300** (`release-signing.yml`) imported keys from
  `keyserver.ubuntu.com` or a raw file on `main`, swallowing both failures with
  `|| true`; the invalid-present-signature control exited 1, while an unsigned
  artifact set passed.
- **#2312** (`docker-security-scan.yml`) invoked
  `aquasecurity/trivy-action@master`, so the implementation being trusted could
  change without a repository diff; its active vulnerability result was also
  non-authoritative.
- **#2316** (`supply-chain-security.yml`) downloaded OSV tooling and consulted
  a live vulnerability database. Local `cargo audit --json` returned seven
  vulnerabilities and exit 1, while the workflow's downloaded-scanner/report
  failures could collapse to an approving final decision.
- **#2317** (`sbom-generate.yml`) downloaded CycloneDX and Grype tooling at run
  time; the critical-Grype control proved the external tool's nonzero result
  was suppressed before it reached the workflow verdict.

These observations inventory moving inputs; they do not by themselves prove
the remote changed a verdict. The probe below remains the required evidence:
force each external response both ways and keep the local authoritative result
stable.

**The probe.** Enumerate every network touch in the gate's mechanism (filters
and plugins included, not just the obvious fetches), then force the external
response both ways — mock a definitive answer and mock an outage — and assert
the verdict does not change. If it changes, the gate is measuring the remote.

---

## G. Documents

### 19. The document describes the mechanism it wishes existed

**What it looks like.** A policy, provenance file, or runbook states in the
present tense a mechanism, artifact, or protection that does not exist at that
head — written from memory or from intention, and it is exactly the sentence a
future reader will trust.

**Real instances:**

- **#2247** (`UNSAFE_POLICY.md` vs zero annotations in the tree): *"Nothing is
  isolated. This is the overstated-policy-document pattern the sprint keeps
  finding, and it is the sentence a future reader will most rely on."* And on
  the no-growth claim: *"That sentence is currently false."*
- **#2282** (`PROVENANCE.md`, across three rounds): *"`PROVENANCE.md` still
  states nonexistent artifacts as current facts."* ... *"This is the same
  surrounding-context-from-memory defect the fifth-round correction is meant
  to eliminate."* The approval came only when *"The document has no unsupported
  present-tree claim: each path is explicitly scoped to this tree or open PR
  #2236."*
- **#2309**: four maintained surfaces claimed
  `.github/workflows/automated-rollback.yml` existed after it had been deleted
  by PR #763. `git log` traced the deletion and a current workflow search found
  no replacement; incident readers were still directed to an absent gate.
- **#2322**: an entry-by-entry audit of `WORKFLOW_JUSTIFICATION.md` at
  `7e34ab93d` found only 4 accurate entries, 6 stale/overstated entries, a ghost
  `deploy-production.yml`, and 9 of 19 on-disk workflows missing entirely
  (summary issue #2321).

(The #2261 emergency-runbook review belongs to the same class — recovery steps
delegating to tooling that does not exist — and was resolved the same way:
every named command verified against the tree, dead ends marked at the top.)

**The probe.** Audit the document claim by claim against the tree at the exact
head: every path `ls`-ed, every symbol grepped, every present-tense sentence
either verified or rewritten as an explicit reference to the not-yet-landed
change.
Reviewing the finding is not enough; the prose is itself an artifact that can
be wrong.

---

## H. Interacting fixes

### 20. One fix masking another

**What it looks like.** Two fixes land together and one makes the other's code
path unreachable in the normal tree — so the masked branch ships untested, and
its next regression is invisible.

**Real instance.** **#2251**: the corpus-seeding fix made the self-test's
directory-cleanup branch invisible; the reviewer caught the interaction and
tested the masked branch in isolation: *"This also confirms the ordering hazard
is closed: seeding target 19 makes the created-directory branch invisible in
the normal tree, but it was tested independently before restoring the seed."*
The same PR had earlier shipped the inverse — a self-test whose residue broke
the gate it validated: *"The live regression job fails on its own self-test
residue. The self-test creates `fuzz-corpus/fuzz_p2p_message/`, deletes only
`crash-selftest01*`, and leaves the directory empty."*

**The probe.** For each fix in a multi-fix change, temporarily revert or
disable the *other* fixes and verify this one still exercises and passes. And
after any self-test that mutates shared state, assert residue-freedom — #2251's
accepted form checks `git status --porcelain` on the touched paths before
proceeding.

---

## I. Reachability over time

### 21. Reachability drift — the trigger is removed but the promise remains

**What it looks like.** A workflow once ran on PR, push, or release events; its
trigger is narrowed later for cost or operational reasons, while its name,
header, enforcement branches, and catalogue entries continue to advertise the
old coverage. Pattern 7 catches a statically omitted path and pattern 19 catches
false prose. Reachability drift is the time-dependent failure connecting them:
the YAML and documentation were once consistent, then only one side changed.

**Systematic result.** None of the nine named workflows in the Rounds 55–57
sweep had a direct `pull_request` trigger at its audit head. That 0/9 result is
not proof that all nine drifted — release/manual-only workflows can be designed
that way — so the instances below name the removal history rather than inferring
it from absence:

- **#2311** (`adversarial-tests.yml`): commit `27103f646` removed PR/push
  triggers to save about 15 minutes per PR. The exact remaining command
  `cargo test --locked -p wrfcoin-native-chain --bin launch_api -- --nocapture`
  could turn red for an in-scope unit test, but a deliberately failing
  adversarial integration test did not run.
- **#2312** (`docker-security-scan.yml`): commit `08a0a7937` removed its
  path-scoped PR/push triggers for Actions spend, while
  `WORKFLOW_JUSTIFICATION.md` still claimed them and the only blocking Trivy
  step remained guarded by the now-impossible `pull_request` event. The exact
  compose loop exited 1 on six baseline failures, proving the workflow could
  be red while no PR could ever reach its vulnerability-enforcement branch.
- **#2316** (`supply-chain-security.yml`): the same cost-control commit removed
  PR/push triggers while the catalogue retained them. The exact dependency
  budget command exited 1 at 31/30, but a PR changing the dependency surface
  never invoked this named gate.
- **#2317** (`sbom-generate.yml`): release and push triggers plus a manual
  `scan_vulnerabilities` input were removed, while the workflow kept an
  unreachable `github.event_name == 'release'` attachment step, referenced the
  deleted input, and `docs/SBOM.md` still promised release attachment and
  critical-finding enforcement. The malformed-document validation control
  printed failure twice and exited 0.
- **#2318** (`wrfcoin-cli-integration.yml`): path-scoped PR/push triggers were
  removed in `08a0a7937`; the mocked integration commands still passed 2/2 and
  16/16, while the scheduled live command exited 7 against a deleted server.
  Neither signal was available on a CLI/server PR.

The broader catalogue audit in **#2322** found this trigger-claim drift in five
entries, with **#2309** establishing the adjacent ghost-workflow form.

**The probe.** Read the current `on:` block, then run
`git log -p -- .github/workflows/<file>` to learn whether advertised events
were intentionally removed. Reconcile the result against the workflow header,
every conditional branch on `github.event_name`,
`WORKFLOW_JUSTIFICATION.md`, and user-facing docs.
Finally, change only the gate or harness and confirm that a PR for that change
actually creates the named check. A locally red command does not repair an
unreachable workflow.

**The fix boundary.** Either restore a path-scoped event, or rename and rewrite
the workflow/docs as schedule/manual monitoring. If cost is the reason to keep
it unreachable from PRs, record that choice and identify the separate PR gate
that covers changes to the mechanism. Do not leave the old promise in prose or
an event-specific enforcement branch that can no longer execute.

---

## J. Issue lifecycle — one level up from the gates

Patterns 1–21 are about a *check* that lies. This section is about a *closure* that lies: the
verification is real, the fix is real, and the risk still ships — because the issue was retired
on the strength of the wrong half. It sits in this catalogue because the consumers are the same
people walking the same probe column; it gets its own section because the artifact under audit is
not a gate but the decision to stop tracking something.

### 22. The fix repairs the mechanism; the stated consequence outlives it, untracked

**What it looks like.** An issue states a defect as *mechanism* ("this job starts a deleted
file", "this gate compares a thing to itself") **and** a consequence ("no CLI command has been
shown to speak to the supported API", "these modules have no independent verification"). A merged
PR genuinely repairs the mechanism — reviewed, verified, no gate in this catalogue's sense lied —
and the issue is closed against it. But the consequence was never discharged, and closing the
issue deleted the only tracking it had. The failure is not in the fix; it is in the closure: the
issue was retired on the mechanism half while the consequence half quietly became nobody's. The
same failure also exists one step earlier, at the **claim stage** — a merge comment asserting a
finding is discharged — and that is the cheapest place to catch it, before any closure happens.

**Why it is expensive** — in the words of the reviewer who first caught it: it *"retires the risk
from the list without retiring it from reality."* The risk stops being looked at precisely
*because* everyone correctly believes the fix was real. Closure removes the item from the normal
open queue — the place routine attention actually goes — so re-probing it then depends on an
exceptional event: a deliberate audit, or a successor issue that restores it to the queue.

**The evidence — the same shape caught twice in one day (2026-07-22), by different lanes, from
different directions: once as a near-miss before any closure, once as completed closures:**

- **The near-miss — #2318 → PR #2326.** The issue named a CLI live-smoke job starting a deleted
  server file. The PR removed the broken job — a real cleanup — and the sprint manager's merge
  comment claimed the merge discharged one of the three BLOCKS TESTNET findings. **The issue was
  never closed**: the manager corrected the claim two minutes later, stated explicitly that #2318
  was not discharged, and left it open; a reviewer's point stood — removing a broken verification
  attempt does not produce verification, and the stated consequence (**no CLI command has been
  shown to speak to the supported API**) was untouched. **#2328** now names that product-readiness
  question. This is the pattern's *assertion* caught before it became a closure — the
  risk-retirement claim was made, and independent review stopped it at the claim stage.
- **The completed closures — #2302 → PR #2310, and #2300/#2301 → PR #2304.** Found hours later
  by the Round 77 closure audit (`docs/plans/2026-07-22-closure-audit.md`), auditing twelve issue
  closures the manager made on the #2325 triage after spot-checking three. #2310 genuinely fixed
  the parity gate's self-comparison and proved violation detection — while the issue's acceptance
  criterion 1 (an independent behavioural oracle for `accounts`/`identity`/`staking`/`weather`)
  remained undone and, with #2302 closed, **no open issue tracked that gap**. #2304 made two
  gates fail closed with a 17-case fixture harness — which no workflow on any event invokes,
  leaving the PR-safe-regression criterion unmet. Successors: **#2330**, **#2331**.

The provenance is part of the evidence and is recorded unsoftened: both catches were of the
sprint manager's own risk-retirement decisions — an incorrect blocker-discharge assertion on
#2326 (corrected within minutes, before any closure), and the actual closure of twelve issues on
a "resolved in substance" column with three spot-checks — and both were caught by independent
verification lanes reading the *issue*, not the *diff*.

**The probe — the detection question.** At the moment of closing, re-read the issue as a
contract, not a pointer, and ask: **does the merged change discharge the issue's stated
*consequence*, or only the *mechanism* the issue happened to describe it through?** Concretely:
walk the issue's acceptance criteria (not its title) against the merged tree, and for each
criterion the fix does not meet, either keep the issue open with reduced scope or file a named
successor *in the same act as closing* — never as a follow-up intention. If the issue states a
consequence the fix does not touch, the closure comment must name where that consequence is now
tracked, or the closure is the defect.

**The fix boundary.** This is not an argument against partial fixes — #2310 and #2304 were both
correct merges, and holding issues open forever dilutes the tracker. The boundary is: a closure
is an assertion that *nothing stated in this issue remains untracked*, and it must be verified at
the same standard as the fix itself. "Resolved in substance" is exactly the phrasing to distrust
— substance is the mechanism; the consequence is what the issue existed for.

**Non-instance, recorded for calibration:** #2269 → PR #2278 passes the detection question
cleanly. Its stated consequence (lint gates red-for-unrelated-reasons on every PR) is discharged —
the gates now report truthfully — and its deferred residual, the workspace-wide fmt sweep, **is
tracked: open #2277** is explicitly *"Deferred from #2269"*, records the measured 3,174-hunk /
285-file sweep, and requires restoring fmt to blocking after it lands. (An earlier revision of
this entry claimed the sweep was "tracked by nothing" — that was false, and its correction is
itself a small demonstration of the loop this catalogue serves: a wrong fact, once written,
propagates into the next document that cites it.) The difference between #2269 and the instances
above is exactly the pattern's boundary: the consequence and the residual each ended up named in
something that survives closure.

---

## Leads examined and not catalogued

For the record, per the compilation contract (drop rather than invent):

- **"A coverage assertion checking its own hard-coded length" on #2234** — the
  #2235 instance is quoted in pattern 10; the #2234 review was not part of the
  compiled source set, so that instance is not cited here.
- **"A re-run's base predating the fix that would make it pass"** — no primary
  review quote was found in the compiled set; noted as a variant inside
  pattern 16 rather than as its own entry.

Patterns 2, 6, and the fail-open-exclusion instance in 7 were not on the
original lead list; they emerged from the reviews during compilation.
