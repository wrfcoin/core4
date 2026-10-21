# Blacksmith runner evaluation — 2026-07-23

Issue: [#2305](https://github.com/wrfcoin/core4/issues/2305)  
Source snapshot: `7b0b3179b26202fdf50af49b38d26f93227bd621` (`main`)  
Decision: **NO-GO for migration pending a same-commit measurement**

## What is proved

The repository is private and currently has 27 workflow YAML files. The last useful
`ci-readiness` cost sample is workflow run
[29900839358](https://github.com/wrfcoin/core4/actions/runs/29900839358), dispatched at
`39612d80b7ed67bf104aea4a8ae52cf060ccfbf2`:

| Observation | Live Actions evidence |
|---|---|
| Job | `rust-tests`, job `88861018397` |
| Runner | `GitHub Actions 1000068720`, group `GitHub Actions` |
| Execution | 2026-07-22 07:36:59Z–08:15:21Z = 38m22s |
| Billed duration | 39 minutes (GitHub rounds each job up to a whole minute) |
| Cache step | `actions/cache@v5`, 07:38:00Z–07:38:13Z = 13s, success |
| Result | success, 15 recorded steps |

GitHub's current standard private-repository Linux x64 rate is
[$0.006/minute](https://docs.github.com/en/billing/reference/actions-runner-pricing), so that
observed job represents **$0.234** of marginal hosted-runner usage before any included-minute
allowance.

Blacksmith's current pricing page lists its 2-vCPU Ubuntu x64 runner at
[$0.004/minute](https://www.blacksmith.sh/pricing). At the *same* 39 billed minutes that would
be $0.156, but this is only sticker-price arithmetic — **not a measured saving**. Blacksmith also
claims that `actions/cache` transparently uses a colocated cache
([vendor quickstart](https://docs.blacksmith.sh/introduction/quickstart)); no Blacksmith job has
run on this repository, so cache compatibility, restore time, wall clock, and billed cost remain
unverified.

## Current CI boundary

Actions is presently runner-starved. For example, run
[30041876751](https://github.com/wrfcoin/core4/actions/runs/30041876751), job `89323502665`,
records `runner_name: ""`, `runner_group_name: ""`, and zero steps. Its top-level `failure` is
**void evidence**, not a code or test failure.

The secret-scanning push + pull-request pair is an accepted standing cost after #2306: the two
events inspect the branch-tip and merge trees respectively. This evaluation does not count their
overlap as a pending saving.

The N5 self-hosted path has one successful smoke execution, but no heavy Rust workload result.
That is a separate in-house option, not evidence about Blacksmith performance.

## Security scope

Only jobs that receive no privileged secrets and whose purpose is compilation or static checking
are candidates for a future trial:

| Candidate | Trial disposition |
|---|---|
| `ci-readiness` → `rust-tests` | Primary same-SHA benchmark |
| `ci-readiness` → `rust-lint` | Eligible after `rust-tests` |
| `ci-readiness` → `preflight-rust` | Eligible, but too short to drive the decision |
| `ci-readiness` → `typecheck` | Eligible, lower-value follow-up |

The following workflows are explicitly excluded from any initial third-party trial:

- `release-signing.yml` — imports GPG release-signing material.
- `hsm-signer-ci.yml` — exercises remote-signer/HSM paths.
- `secret-scanning.yml` — scans the private source tree for secrets.
- `deploy-gate.yml` — is deployment-adjacent and is not a compile benchmark.

No conclusion here approves sending those jobs or their secrets to a third-party runner.

## Same-SHA trial protocol

This protocol is intentionally not executed until the owner separately connects Blacksmith and
authorizes runner use.

1. Choose one commit and record its full SHA. Do not compare different merge trees.
2. On a temporary, unmerged evaluation workflow, copy the current `rust-tests` job byte-for-byte
   except for runner label and a provider-specific cache-key prefix.
3. Run one GitHub-hosted 2-vCPU job and one Blacksmith 2-vCPU job. Require a non-empty
   `runner_name`, at least one executed step, and the expected runner label before treating either
   result as evidence.
4. Run each provider twice at the same SHA:
   - first run: cold provider-specific cache key;
   - second run: identical warm key.
5. Record for every job:
   - queue, start, and completion timestamps;
   - runner name/group and executed-step count;
   - cache restore start/completion and hit/miss;
   - Rust-test command durations and final conclusion;
   - billed whole minutes and contemporaneous per-minute rate.
6. Compute cold and warm cost separately. Included/free minutes may be reported, but the adoption
   comparison must use marginal list price so it remains meaningful after allowances are spent.
7. Confirm the temporary workflow exposes no repository or environment secret beyond the
   automatic read-only token, then delete or close the unmerged experiment after evidence is
   captured.

Acceptance for a recommendation stronger than NO-GO requires successful same-SHA jobs on both
providers, observed cache behavior on both cold and warm runs, and a written security review of the
exact workflow permissions. Vendor performance claims are not a substitute.

## Recommendation

**Do not migrate any job today.** The GitHub baseline is measured; the Blacksmith side is not.
The price table makes a narrowly scoped experiment plausible, but it does not establish runtime,
cache behavior, reliability, or total cost for this workspace. Keep #2305 open and this PR in draft
until separately authorized runner access produces the required same-commit evidence.

