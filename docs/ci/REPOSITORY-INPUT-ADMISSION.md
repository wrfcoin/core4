# repository-input integration admission: what it enforces and what must be decided first

Refs #2433, #2896, #3446. Implements owner option A of `REQUIRED-CHECKS-R21.md`. Workflow:
`.github/workflows/repository-input-admission.yml`; checker: `scripts/check_repository_input_admission.py`.

## Enforced by the check (when it runs)

- Unfiltered; fires on every PR and main push; controls and classifier always execute.
- A change touching a relevant input needs the integration ratchet's evidence for the exact head: the newest run
  completed/success, both required jobs success on a named runner with a successful `Observe*` step.
- Evidence is bound to the base: each required leg's checkout (read from its job log, after "Checking out the ref")
  must be the merge of exactly this head into exactly this job's freshness-checked base. On a push the checkout must be the pushed
  head, compared as the complete 40-hex sha. Missing identity or base is a refusal.
- Pending evidence fails on both PR and push (fail-closed, no polling, no runner held). The checker exits 3 for PENDING
  (1 = REFUSE, 2 = could not decide); the workflow turns that into a failing step named
  `Verdict PENDING - integration ratchet not complete`.

## Automatic re-evaluation of PENDING (Refs #3446)

`.github/workflows/rerun-pending-admission.yml` fires on `workflow_run` of "Integration Target Ratchet" `completed`
(pull_request and push events only) and runs `scripts/rerun_pending_admission.py` for that run's head SHA. It re-runs
the admission JOB (`POST /actions/jobs/{id}/rerun`) only when the newest admission run for that head+event failed AND
its job's PENDING marker step failed (structured step name/conclusion from the jobs API, no log grepping). It never
re-runs a REFUSE, error, success or cancelled run, never another workflow, at most once per ratchet completion, and not
beyond attempt 5. If the admission job is still finishing it retries up to ~2 minutes. The re-run re-reads live evidence,
so a failed/cancelled ratchet still REFUSES; verdict logic is unchanged and a running ratchet is still a failing context.

| Case | Behaviour |
|---|---|
| No relevant change | The admission job admits immediately; no ratchet may even exist; nothing re-runs. |
| Ratchet not selected by its path filter | No ratchet run, so no event and no re-run; the job refuses "no run for head" as designed (#3446 is that gap). |
| Fork PR | `workflow_run` runs in the base repo with the default branch's file; no PR code is checked out or run; only a validated head SHA and event are used. |
| New push, `cancel-in-progress` | The superseded admission run is `cancelled`, not `failure`, so it is not selected; selection is per head SHA. |
| Push to main | Same path: push-event ratchet completion re-runs the push-event admission job (no cancellation on main). |
| Re-run of a PR job | Reuses the original merge ref; the freshness guard refuses it if the base moved, so it can never admit stale evidence. |

Only takes effect once on the default branch. Verify after merge: open a PR (or push to main) touching a relevant input
while the ratchet is queued; the admission run should fail with the `Verdict PENDING` step, then the "Re-run pending
repository-input admission" run should appear when the ratchet completes, and the admission run should show attempt 2 with the real verdict.

## Not enforced by this PR (owner decision, recorded here)

**Later base advance.** The status is produced once per PR event. If main advances while the PR head is unchanged, the completed
status stays attached to the head and nothing re-runs it, so it is current-base only at the moment it ran. Live applied rules show
`strict_required_status_checks_policy: false`. Before this context is added to the required set, the owner must record one of:

1. `strict_required_status_checks_policy: true` (branch must be up to date), so every base advance forces a push and a fresh run set; or
2. a reviewed merge queue (or equivalent merge-time freshness check) that re-evaluates at merge.

Until one is recorded, requiring this context does not give a current-base guarantee. Not enforced by this PR: no ruleset changes here.
Also open before requiring it: #3446 (the ratchet's own path filters must select JSON/env/toml/yaml/sh/conf inputs, otherwise such PRs are refused for lack of a run).
