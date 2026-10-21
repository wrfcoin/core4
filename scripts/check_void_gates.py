#!/usr/bin/env python3
"""Fail when a gate produced no evidence at all — a VOID job (#2923).

WHAT VOID IS
------------
A job that concluded with **no runner assigned and zero completed steps**. It is
neither red nor green: nothing executed, so it says nothing about the code in
either direction. It still renders as a check in the PR list, which is why it is
mistaken for a tracked failure.

WHY THIS IS AUTOMATED AND THE REST OF #2923 IS NOT
--------------------------------------------------
Of the four shapes in the taxonomy, this is the only one where a cheap mechanical
check replaces a human who happened to look. Every instance found on 2026-08-31
was caught by someone reading `runner_name` out of the jobs API while doing
something else. Determinism Lint was VOID on `main` for NINETEEN DAYS (#2908,
last real run `31647703288` on 2026-08-12) and nothing reported it.

The other three shapes are deliberately NOT automated here. A checker that tries
to cover everything becomes the slow thing people route around, and then joins
the taxonomy it was written for.

IT MUST ATTRIBUTE, NOT JUST DETECT
----------------------------------
`steps=0` has at least three causes with different fixes, and conflating them
sends people to the wrong one:

  BILLING           "recent account payments have failed / spending limit"
                    -> owner action, and it is the only one anyone can fix here
  RUNNER_UNAVAILABLE "not acquired by Runner of type hosted"
                    -> GitHub-side capacity or an outage; only waiting helps
  SUPERSEDED        conclusion `cancelled` with no annotation
                    -> a newer push cancelled it; NOT a defect, not reported
  UNATTRIBUTED      zero steps, no annotation explaining why
                    -> reported as VOID, cause unknown, never guessed

That distinction is not theoretical: on 2026-08-06 the signature returned
identically and was misattributed to billing on pattern match, when the
annotation said the runner had not been acquired and the budget was intact.

A PARKED OR MISSING TRIGGER IS A DIFFERENT DEFECT and this script does not
detect it. #2887 (Secret Scanning never runs on `main`) has the same *observable*
— no evidence — but the cause is a trigger, and the fix is a trigger. Sending
someone to restore billing for it would be the exact attribution failure #2923 is
about. Absence of a run is out of scope here, on purpose.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from datetime import date
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

# IMPORTED, not reimplemented. This repo has had the unvalidated-floor bug twice
# already — cargo_test_nonvacuous.py's --min-summaries accepted -1 and returned
# rc=0 until #2899 — and the cure was already sitting in that file when it
# recurred. A third private copy would be the same mistake a third time.
from cargo_test_nonvacuous import positive_int  # noqa: E402


def floor(flag: str):
    """`positive_int` with this script's flag name in the message.

    A thin adapter, deliberately: the VALIDATION is imported and the wrapper only
    relabels the error. Reimplementing the check to fix the wording is how the
    same bug gets a third copy.
    """

    def parse(value: str) -> int:
        try:
            return positive_int(value)
        except argparse.ArgumentTypeError as exc:
            detail = str(exc).split(":", 1)[-1].strip()
            raise argparse.ArgumentTypeError(
                f"{flag} must be >= 1, got {value!r}: {detail}"
            ) from exc

    return parse

BASELINE = "scripts/void_gates_baseline.json"

BILLING = "BILLING"
RUNNER_UNAVAILABLE = "RUNNER_UNAVAILABLE"
SUPERSEDED = "SUPERSEDED"
SKIPPED = "SKIPPED"
UNATTRIBUTED = "UNATTRIBUTED"


class ApiUnreadable(Exception):
    """A GitHub API read failed, and no caller may treat that as empty data.

    THE CLASS FIX (#2923 round 3). Round 1 found that an unreadable jobs response
    failed OPEN. I fixed that call site. Round 2 then found the same defect at two
    OTHER call sites — the self-cleaning re-fetch, and the annotations read — each
    of which had independently coerced a failed read to `[]`.

    Fixing instances guarantees a round 4 at the fourth call site. So `gh_json`
    no longer RETURNS a failure that a caller can ignore; it RAISES one. A new
    call site inherits the behaviour by construction rather than by its author
    remembering, and "the API did not answer" can no longer be spelled the same
    way as "the API answered with nothing".

    That distinction is the entire subject of #2923, and this script had it
    backwards in three places.

    ROUND 4 closed the third input shape. The class fix covered a transport
    failure and malformed JSON, but an rc-0 response whose body is literally
    `null` parsed cleanly to `None` — a successful API response that means
    nothing — and a naive `gh_json(path) or []` coerced it straight back to the
    original bug without catching anything. Same thesis one level down: rc 0 with
    a body meaning "no data" was indistinguishable from rc 0 with a body meaning
    "here is data, and it is empty".
    """


def gh_json(path: str) -> object:
    result = subprocess.run(
        ["gh", "api", path], capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        raise ApiUnreadable(f"{path} (gh exited {result.returncode})")
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise ApiUnreadable(f"{path} (malformed JSON: {exc})") from exc

    # A `null` body is an rc-0 response carrying no data. Returning it would hand
    # back a failure-shaped value a caller can coerce with `or []` — the exact
    # property this class fix exists to remove.
    if payload is None:
        raise ApiUnreadable(f"{path} (rc 0 but the body is JSON null — a response carrying no data)")

    # These endpoints return an object or an array. Anything else is a response
    # this script cannot interpret, and an uninterpretable response is not data.
    # An EMPTY list or object is fine and stays fine: `[]` is a real answer, and
    # a genuine empty annotation list must still mean UNATTRIBUTED.
    if not isinstance(payload, (dict, list)):
        raise ApiUnreadable(
            f"{path} (rc 0 but the body is {type(payload).__name__}, not an object or array)"
        )
    return payload


def is_void(job: dict) -> bool:
    """No runner and no completed steps. Both, deliberately.

    A job with a named runner that failed in its first step has zero *successful*
    steps but is a real result. Requiring an empty `runner_name` too is what keeps
    this from flagging genuine early failures. `runner_name` may be `null` as well
    as `""` — both mean unassigned.
    """
    return not (job.get("runner_name") or "") and len(job.get("steps") or []) == 0


def runner_class(job: dict) -> str:
    """`hosted`, `self-hosted`, or `unknown`.

    `labels` survives even when the job never started — a VOID hosted job still
    reports `["ubuntu-latest"]` — which is what makes the runner CLASS usable as
    the exemption key where workflow names were not. Measured over 52 consecutive
    never-started jobs on `main`: 52 carried a non-empty labels list, 0 were empty
    and 0 omitted the key.

    But the whole exemption rests on that one fact about someone else's API, so
    `unknown` exists rather than a boolean. If labels are ever absent or empty the
    class cannot be determined, and an undeterminable class is NOT exempt — the
    detector cries wolf rather than silently exempting a job it could not
    classify. Wolf is the recoverable direction; a silent exemption is the shape
    this whole issue is about.
    """
    labels = [str(x).lower() for x in (job.get("labels") or [])]
    if not labels:
        return "unknown"
    return "self-hosted" if "self-hosted" in labels else "hosted"


def is_hosted(job: dict) -> bool:
    """Exempt-eligible only when the class is positively known to be hosted."""
    return runner_class(job) == "hosted"


def attribute(repo: str, job: dict) -> tuple[str, str]:
    """Which of the causes produced this zero-step job. Never guesses.

    `skipped` is checked BEFORE the annotations. A skipped job has no runner and
    no steps and so matches the VOID shape exactly, but its cause is a condition
    or filter that excluded it — a different defect with a different fix, already
    covered by the terminal-skip guards. Reporting it here as an unexplained VOID
    is the conflation this script exists to prevent, and the first version of it
    did exactly that: the one finding it produced against main was a skipped job
    labelled UNATTRIBUTED.
    """
    if job.get("conclusion") == "skipped":
        return SKIPPED, "skipped by a condition or filter — not a VOID job"
    # No `or []`. An unreadable annotations response raises, because "could not
    # read the annotations" and "there are no annotations" are different facts and
    # only the second may be attributed to UNATTRIBUTED.
    annotations = gh_json(f"repos/{repo}/check-runs/{job['id']}/annotations")
    for note in annotations:
        message = (note.get("message") or "").lower()
        if "payments have failed" in message or "spending limit" in message:
            return BILLING, note["message"]
        if "not acquired by runner" in message:
            return RUNNER_UNAVAILABLE, note["message"]
    if job.get("conclusion") == "cancelled":
        return SUPERSEDED, "cancelled with no annotation — superseded by a newer push"
    return UNATTRIBUTED, "zero steps with no annotation explaining why"


def void_jobs(repo: str, runs: list[dict]) -> tuple[list[dict], int, bool]:
    """Findings, unreadable run ids, and how many jobs were actually inspected.

    The unreadable list is returned SEPARATELY rather than mixed into the
    findings. In the first version it was appended as `{"unreadable": id}` and
    then silently dropped by a `cause is None` filter downstream, so a transient
    403 on the one run containing the only VOID job produced "no NEW VOID gates",
    exit 0. That is this tool's own vacuity floor failing on the single input
    class it exists for: an unreadable job IS having looked at nothing.
    """
    found: list[dict] = []
    inspected = 0
    # Answered in the SAME pass rather than by a second fetch. Round 2's stale
    # check re-read the jobs independently and collapsed a failed second read to
    # an empty list, so a broken API looked like "no hosted job ran". Removing the
    # second call site removes the second place to get it wrong.
    hosted_ran_for_real = False
    for run in runs:
        payload = gh_json(f"repos/{repo}/actions/runs/{run['id']}/jobs")
        for job in payload.get("jobs", []):
            inspected += 1
            if (
                runner_class(job) == "hosted"
                and (job.get("runner_name") or "")
                and len(job.get("steps") or []) > 0
            ):
                hosted_ran_for_real = True
            if job.get("conclusion") and is_void(job):
                cause, detail = attribute(repo, job)
                found.append(
                    {
                        "workflow": run.get("name", "?"),
                        "job": job.get("name", "?"),
                        "id": job["id"],
                        "run": run["id"],
                        "cause": cause,
                        "detail": detail,
                        "hosted": is_hosted(job),
                        "runner_class": runner_class(job),
                    }
                )
    return found, inspected, hosted_ran_for_real


def self_test() -> list[str]:
    """Prove the detector can say NO, and that it is not merely flagging everything."""
    failures = []
    real = {"runner_name": "n5pro-ct102", "steps": [{"n": 1}], "conclusion": "success"}
    early_failure = {"runner_name": "n5pro-ct102", "steps": [], "conclusion": "failure"}
    void = {"runner_name": "", "steps": [], "conclusion": "failure"}

    if is_void(real):
        failures.append("SELF_TEST_FAILED: a real job with a runner and steps must not be VOID")
    if is_void(early_failure):
        failures.append(
            "SELF_TEST_FAILED: a NAMED runner with zero steps is a real early failure, not VOID — "
            "flagging it would make this detector cry wolf and get ignored"
        )
    if not is_void(void):
        failures.append("SELF_TEST_FAILED: empty runner + zero steps must be VOID")
    return failures


def load_baseline(path: str) -> tuple[dict, str | None]:
    """The systemic exemption, so this gate reports REGRESSIONS not the status quo.

    Without an exemption the detector is red from birth — every hosted job in this
    org is VOID — and a permanently red gate is one people route around, which is
    how it would join the taxonomy it was written for.

    KEYED ON THE RUNNER CLASS, NOT ON WORKFLOW NAMES. The first version listed
    workflow names, and a reviewer showed two consequences. A NEW job added to a
    baselined workflow inherited the old exemption, which is the stale-exemption
    hazard arriving through the granularity of the key. And two of the ten names
    did not match GitHub's actual names at all, so their known-VOID jobs would
    have been reported as regressions — the baseline manufacturing the cry-wolf
    it existed to prevent.

    The runner class has neither problem. `labels` survives on a job that never
    started, a newly added HOSTED job is explained by the same systemic fact as
    every other hosted job, and a VOID job on a SELF-HOSTED runner is never
    exempt because nothing systemic explains it.

    Self-cleaning: if any hosted job in the sample ran for real, hosted execution
    has been restored and the exemption is stale — that is a failure, not a pass,
    because a stale exemption silently un-gates everything it covers.
    """
    try:
        with open(path, encoding="utf-8") as handle:
            data = json.load(handle)
    except FileNotFoundError:
        return set(), f"{path} not found — refusing to treat every VOID job as expected"
    except json.JSONDecodeError as exc:
        return set(), f"{path} is not valid JSON: {exc}"
    if "exemptions" not in data:
        return {}, (
            f"{path} must declare `exemptions` (an object, possibly empty). A baseline with no "
            "exemptions key is indistinguishable from one whose exemptions were dropped by an "
            "editing mistake, and the safe reading of that ambiguity is to refuse."
        )
    exemptions = data["exemptions"]
    if not isinstance(exemptions, dict):
        return {}, f"{path}: `exemptions` must be an object, got {type(exemptions).__name__}"

    # EXPIRY BY CONSTRUCTION (#2921 story 3). An exemption with no stated end is
    # the defect this validation exists to make impossible, so the required
    # fields are checked at LOAD time and a violation is a hard refusal rather
    # than a skipped entry. Refusing is the whole point: an unparseable exemption
    # that was merely ignored would un-gate exactly the jobs it covers, which is
    # the failure mode of the thing it replaced.
    for name, entry in exemptions.items():
        if not isinstance(entry, dict):
            return {}, f"{path}: exemption {name!r} must be an object"
        for field in ("review_by", "clears_when", "issue"):
            if not entry.get(field):
                return {}, (
                    f"{path}: exemption {name!r} is missing `{field}`. Every exemption must "
                    "carry a review date, the observable condition that ends it, and the issue "
                    "that owns it — an exemption nobody has to justify again is one that "
                    "outlives its cause (#2921)."
                )
        try:
            entry["_review_by"] = date.fromisoformat(str(entry["review_by"]))
        except ValueError:
            return {}, (
                f"{path}: exemption {name!r} has `review_by`={entry['review_by']!r}, which is not "
                "an ISO date (YYYY-MM-DD). An unparseable date is treated as a refusal, never as "
                "'no expiry'."
            )
    return data, None


def expired_exemptions(baseline: dict, today: date) -> list[str]:
    """Names of exemptions whose review date has passed.

    Separated from `load_baseline` so the expiry rule is testable against an
    injected date rather than only against the real clock — a rule that can only
    be exercised by waiting is one nothing verifies.
    """
    return sorted(
        name
        for name, entry in baseline.get("exemptions", {}).items()
        if entry.get("active") and entry.get("_review_by") and entry["_review_by"] < today
    )


def active_exemption(baseline: dict, name: str, today: date) -> bool:
    """True only if `name` is active AND its review date has not passed.

    The date check lives HERE, at the point of use, not only in the reporting
    path. If it lived only in reporting, an expired exemption would still
    suppress its jobs while a message elsewhere complained about it — which is
    precisely a stale exemption silently un-gating what it covers.
    """
    entry = baseline.get("exemptions", {}).get(name)
    if not entry or not entry.get("active"):
        return False
    review_by = entry.get("_review_by")
    return not (review_by and review_by < today)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="wrfcoin/core4")
    parser.add_argument("--branch", default="main")
    parser.add_argument("--limit", type=floor("--limit"), default=30, help="most recent runs to read")
    parser.add_argument(
        "--min-runs",
        type=floor("--min-runs"),
        default=5,
        help="fewest COMPLETED runs that make a verdict meaningful.",
    )
    parser.add_argument(
        "--min-jobs",
        type=floor("--min-jobs"),
        default=20,
        help="fewest JOBS that must be inspected. Runs is not enough — a sample of runs that all "
             "contained one trivial job would clear a run floor while checking almost nothing.",
    )
    parser.add_argument("--baseline", default=BASELINE)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        problems = self_test()
        for line in problems:
            print(line, file=sys.stderr)
        if problems:
            return 1
        print("check_void_gates.py self-test passed (VOID reaches both YES and NO)")
        return 0

    payload = gh_json(f"repos/{args.repo}/actions/runs?branch={args.branch}&per_page={args.limit}")
    if payload is None:
        print("could not read workflow runs — refusing to report that as 'no VOID'", file=sys.stderr)
        return 2
    runs = [r for r in payload.get("workflow_runs", []) if r.get("status") == "completed"]
    if len(runs) < args.min_runs:
        print(
            f"INCONCLUSIVE: only {len(runs)} completed run(s) on {args.branch}, need "
            f"{args.min_runs}. A sample too small to contain a VOID job must not report "
            "'no VOID gates' — that is the same vacuity this detector exists to catch.",
            file=sys.stderr,
        )
        return 2

    baseline, baseline_failure = load_baseline(args.baseline)
    if baseline_failure:
        print(baseline_failure, file=sys.stderr)
        return 2

    found, inspected, hosted_ran_for_real = void_jobs(args.repo, runs)

    if inspected < args.min_jobs:
        print(
            f"INCONCLUSIVE: only {inspected} job(s) inspected, need {args.min_jobs}.",
            file=sys.stderr,
        )
        return 2

    today = date.today()
    expired = expired_exemptions(baseline, today)
    hosted_exempt = active_exemption(baseline, "hosted_void_expected", today)
    issue = (
        baseline.get("exemptions", {}).get("hosted_void_expected", {}).get("issue", "?")
    )

    real_void = [f for f in found if f["cause"] not in (SUPERSEDED, SKIPPED)]
    skipped = [f for f in found if f["cause"] == SKIPPED]
    superseded = [f for f in found if f["cause"] == SUPERSEDED]
    reportable = [f for f in real_void if not (f["hosted"] and hosted_exempt)]

    print(f"read {len(runs)} completed runs on {args.branch}; inspected {inspected} jobs")
    if superseded:
        print(f"  {len(superseded)} cancelled/superseded — not a defect, not reported")
    if skipped:
        print(f"  {len(skipped)} skipped — a filter or condition excluded them, a different defect")
    if hosted_exempt:
        print(f"  hosted VOID is expected while {issue} is open ({len(real_void) - len(reportable)} job(s))")

    # EXPIRY IS A FAILURE, NOT A NOTE (#2921 story 3). Reported before the
    # staleness check so a baseline that is both expired and stale names the
    # cheaper cause first. `active_exemption` has already stopped applying these,
    # so their jobs are in `reportable` above — the exemption lapsed rather than
    # persisted, which is the whole design.
    if expired:
        print(
            "\nEXPIRED EXEMPTION(S): "
            + ", ".join(
                f"{name} (review_by {baseline['exemptions'][name]['review_by']}, "
                f"clears when: {baseline['exemptions'][name]['clears_when']})"
                for name in expired
            )
            + ". These stopped applying on their review date and any jobs they covered are "
            "reported above. Re-justify with a new review date or retire them.",
            file=sys.stderr,
        )
        return 1

    # Self-cleaning. If hosted execution has come back, the exemption is stale and
    # must be retired — a stale exemption silently un-gates everything it covers.
    # Computed in void_jobs' single pass; there is no second fetch to fail.
    if hosted_exempt and hosted_ran_for_real:
        print(
            f"\nSTALE EXEMPTION: a hosted job ran for real, so hosted execution is restored and "
            f"`hosted_void_expected` must be retired ({issue}). A stale exemption silently "
            "un-gates every job it covers.",
            file=sys.stderr,
        )
        return 1

    if not reportable:
        print("no NEW VOID gates")
        return 0

    by_cause: dict[str, list[dict]] = {}
    for entry in reportable:
        by_cause.setdefault(entry["cause"], []).append(entry)
    for cause, entries in sorted(by_cause.items()):
        print(f"\n{cause} — {len(entries)} job(s)", file=sys.stderr)
        for entry in entries:
            runner = entry["runner_class"].upper()
            print(
                f"  [{runner}] {entry['workflow']} / {entry['job'][:60]}  job={entry['id']}",
                file=sys.stderr,
            )
        print(f"  {entries[0]['detail'][:160]}", file=sys.stderr)

    print(
        f"\n{len(reportable)} VOID job(s) not explained by the systemic exemption: concluded with "
        "no runner and zero completed steps (#2923).",
        file=sys.stderr,
    )
    return 1


def run() -> int:
    """Any unreadable API read, at ANY call site, is INCONCLUSIVE.

    Handled once, here, rather than at each call site — which is what makes a new
    call site safe by default instead of safe only if its author remembers.
    """
    try:
        return main()
    except ApiUnreadable as exc:
        print(
            f"INCONCLUSIVE: could not read {exc}. A response this tool could not read is a "
            "check it did not make, and 'no VOID found' must never be reachable by having "
            "looked at nothing (#2923).",
            file=sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(run())
