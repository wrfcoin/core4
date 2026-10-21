#!/usr/bin/env python3
"""The 'repository-input integration admission' context (Refs #2433, #2896, #3446).

WHY THIS EXISTS
---------------
docs/ci/REQUIRED-CHECKS-R21.md: three required contexts alone permit two incident paths --
a repository input (here `docs/deployment/live-feature-schedule/777-testnet.json`) that an
integration assertion reads, changed without the integration ratchet ever being selected.
A path-filtered or skipped check can satisfy a platform requirement; a check that is
unfiltered and refuses absent evidence cannot.

WHAT IT DECIDES
---------------
1. CLASSIFY the change (merge-ref parent 1 .. HEAD for a PR, HEAD^ .. HEAD on a main push)
   against the anchored input manifest `.github/repository-input-admission.json`.
2. NOT RELEVANT: the classifier and control assertions already ran; print the scope and ADMIT.
   This is a scope statement, never evidence for a runtime obligation.
3. RELEVANT: require current-head evidence from the integration ratchet workflow: the newest
   run for the head SHA is completed/success and every required job is success, ran on a NAMED
   runner and completed a step whose name starts with the required prefix ("Observe" -- the
   ratchet's own fail-closed step that refuses zero executed targets).
   Missing run, skipped/cancelled/queued job, empty runner name, or a missing Observe step is
   REFUSE. It is never a green wrapper.

PENDING (run exists, still queued or in progress) is REFUSE on a pull_request AND on a push to main
(SOL-R3 P2): admission must not pass before the evidence exists. This job does NOT wait -- it must not hold a CT102 runner
while the ratchet queues behind it. Re-run THIS job alone after the ratchet completes (the
evidence is read live by head SHA, so a re-run is not a replay of a stale merge ref). On a
push to main a still-running ratchet is also a failing (incomplete) context until it finishes.

BASE BINDING (SOL-R3 P1). The ratchet has no freshness guard, so its run may have merged this
head into an OLD base. For each required leg this job reads the checkout line from the first 64 KiB
of the job log ("HEAD is now at <sha> Merge <head> into <base>") and requires head == the PR head and
base == parent 1 of THIS job's merge ref. That parent is asserted equal to the target-branch tip by
the freshness step just before, so evidence is bound to the CURRENT base. Missing/unparseable identity
= REFUSE. On a push the leg's checkout must be the pushed head itself (full 40-hex sha, never a prefix).

ACTIVATION PREREQUISITE (SOL-R3 P1, recorded here and in docs/ci/REPOSITORY-INPUT-ADMISSION.md). This job
runs per PR event. If main advances while the PR head is unchanged, the already-completed status stays
attached to the head and nothing re-runs it, so on its own it is current-base only at the moment it ran.
Before this context is added to the required set the OWNER must also record one of: (a)
strict_required_status_checks_policy=true (the branch must be up to date, so every base advance forces a
push and a new run set), or (b) a reviewed merge queue that re-evaluates at merge time. The live applied
rule is strict=false today. Not enforced by this PR; no ruleset is changed here.

NOT CHECKED HERE (stated, not hidden): test counts are inferred from the ratchet's Observe step,
not read from logs.

ANCHORING
---------
`relevant_globs` may only grow. On a pull_request the base manifest is read from parent 1 of
the checked-out merge commit (a PR cannot edit it). Dropping a glob is an ERROR. If the base has
no manifest (first adoption) the run prints UNANCHORED loudly rather than pretending.

Every parse/IO/API problem is an ERROR (exit 2) -- fail closed. Stdlib only.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Callable

sys.path.insert(0, str(Path(__file__).resolve().parent))

# Imported, not copied: one glob implementation, so this gate and the path-coverage
# gate cannot disagree about what a pattern matches.
from check_workflow_path_coverage import glob_to_regex, matches_any  # noqa: E402

MANIFEST = ".github/repository-input-admission.json"
ADMIT, REFUSE, PENDING = "ADMIT", "REFUSE", "PENDING"
# Machine-readable PENDING signal (Refs #3446). Exit code 3 = PENDING; 1 = REFUSE; 2 = could not decide.
# The workflow turns exit 3 into a failing step with THIS name, which the Actions jobs API exposes as structured
# data (step name + conclusion), so rerun_pending_admission.py never greps free text. Keep in sync with the workflow.
PENDING_EXIT = 3
PENDING_STEP_NAME = "Verdict PENDING - integration ratchet not complete"


def exit_code_for(verdict: str) -> int:
    return {ADMIT: 0, PENDING: PENDING_EXIT}.get(verdict, 1)


class AdmissionError(Exception):
    """Fail-closed: the checker could not reach a verdict."""


def load_manifest(text: str) -> dict[str, Any]:
    try:
        m = json.loads(text)
    except json.JSONDecodeError as exc:
        raise AdmissionError(f"manifest is not valid JSON: {exc}") from exc
    for key in ("evidence_workflow", "required_jobs", "required_step_prefix", "relevant_globs", "corpus"):
        if key not in m:
            raise AdmissionError(f"manifest lacks '{key}'")
    for key in ("evidence_workflow", "required_step_prefix"):
        if not isinstance(m[key], str) or not m[key]:
            raise AdmissionError(f"manifest '{key}' must be a non-empty string")
    for key in ("required_jobs", "relevant_globs"):
        if not isinstance(m[key], list) or not m[key] or not all(isinstance(x, str) and x for x in m[key]):
            raise AdmissionError(f"manifest '{key}' must be a non-empty list of non-empty strings")
    for key in ("must_be_relevant", "must_not_be_relevant"):
        if not isinstance(m["corpus"].get(key), list) or not m["corpus"][key]:
            raise AdmissionError(f"manifest corpus '{key}' must be a non-empty list")
    return m


def is_relevant(globs: list[str], path: str) -> bool:
    return matches_any(globs, path)


def self_test(m: dict[str, Any]) -> list[str]:
    """Control assertions. Each returns a problem string; empty list == controls hold.

    A classifier that always says YES (or NO) would make every later step pass while
    guarding nothing, so the RED/GREEN corpus must reach both answers.
    """
    problems: list[str] = []
    globs = m["relevant_globs"]
    yes, no = m["corpus"]["must_be_relevant"], m["corpus"]["must_not_be_relevant"]
    for p in yes:
        if not is_relevant(globs, p):
            problems.append(f"corpus: '{p}' must be relevant but no glob matches it")
    for p in no:
        if is_relevant(globs, p):
            problems.append(f"corpus: '{p}' must NOT be relevant but a glob matches it")
    for g in globs:
        if not any(glob_to_regex(g).match(p) for p in yes):
            problems.append(f"glob '{g}' matches no must_be_relevant corpus example (dead or unprotected)")
    # Mutant: an empty manifest must flip every positive example, proving the matcher can say NO.
    if any(is_relevant([], p) for p in yes):
        problems.append("matcher says YES against an empty glob list")
    return problems


def anchor_problems(base: dict[str, Any], head: dict[str, Any]) -> list[str]:
    missing = [g for g in base["relevant_globs"] if g not in head["relevant_globs"]]
    out = [f"relevant_globs dropped vs the PR base: '{g}' (the list may only grow)" for g in missing]
    for key in ("evidence_workflow", "required_step_prefix"):
        if base[key] != head[key]:
            out.append(f"'{key}' changed vs the PR base ({base[key]!r} -> {head[key]!r})")
    gone = [j for j in base["required_jobs"] if j not in head["required_jobs"]]
    out += [f"required_jobs dropped vs the PR base: '{j}'" for j in gone]
    return out


MERGE_SUBJECT = re.compile(r"^(?P<short>[0-9a-f]{7,40}) Merge (?P<head>[0-9a-f]{40}) into (?P<base>[0-9a-f]{40})$")
HEAD_NOW_AT = re.compile(r"HEAD is now at (?P<short>[0-9a-f]{7,40}) (?P<subject>[^\r\n]*)")


CHECKOUT_MARKER = "Checking out the ref"
LOGGED_SHA = re.compile(r"git log -1 --format=%H\r?\n(?:[^\n]*?Z )?(?P<sha>[0-9a-f]{40})\b")


def parse_checkout_identity(log_text: str) -> str | None:
    """Identity of the commit actions/checkout actually checked out, or None.

    Only text AFTER the "Checking out the ref" group counts. The "Cleaning the repository" step before it
    prints `HEAD is now at ...` for the persistent self-hosted workspace's PREVIOUS commit (live logs show
    another PR's merge commit there); treating that as the identity would bind evidence to the wrong base.
    A merge-ref checkout prints `HEAD is now at <short> Merge <head> into <base>` in that group; its short sha
    must prefix the full sha the step then prints for `git log -1 --format=%H`. A branch checkout prints no
    such line; the logged full sha alone is the identity. Timestamps (`2026-..Z `) before values are tolerated.
    """
    idx = log_text.find(CHECKOUT_MARKER)
    if idx < 0:
        return None
    after = log_text[idx:]
    logged = LOGGED_SHA.search(after)
    if not logged:
        return None
    sha = logged["sha"]
    found = HEAD_NOW_AT.search(after[: logged.start()])
    if found:
        return f"{found['short']} {found['subject']}" if sha.startswith(found["short"]) else None
    return f"{sha} (branch checkout)"


def bind_identity(identity: str | None, head_sha: str, event: str, base_sha: str, job_name: str) -> str | None:
    """Return a refusal reason, or None when this leg's checkout is exactly the current head/base."""
    if not identity:
        return f"job '{job_name}': its checkout identity is unavailable; evidence cannot be bound to the current base"
    if event == "pull_request":
        got = MERGE_SUBJECT.match(identity)
        if not got:
            return f"job '{job_name}': checkout '{identity[:60]}' is not a '<sha> Merge <head> into <base>' merge ref"
        if got["head"] != head_sha:
            return f"job '{job_name}': merge ref carries head {got['head']}, not {head_sha}"
        if got["base"] != base_sha:
            return (f"job '{job_name}': evidence is for base {got['base']} but the current base is {base_sha}; "
                    "stale evidence (push or rebase to refresh)")
        return None
    if identity.split(" ", 1)[0] != head_sha:  # complete 40-hex sha only; a shared prefix proves nothing
        return f"job '{job_name}': checkout '{identity[:48]}' is not the pushed head {head_sha}"
    return None


def evaluate_evidence(
    m: dict[str, Any], runs: list[dict[str, Any]], jobs_by_run: dict[int, list[dict[str, Any]]], head_sha: str, event: str,
    base_sha: str = "", identities: dict[int, str | None] | None = None,
) -> tuple[str, list[str]]:
    """Return (ADMIT|REFUSE|PENDING, reasons) for a RELEVANT change.

    Evidence is bound to the head AND, on a pull_request, to the CURRENT merge base (SOL-R3 P1): each
    required leg's checkout identity (`identities`, job id -> checkout subject) must be the merge of
    exactly this head into exactly `base_sha`. A missing identity or base is a refusal, never a pass.
    """
    if event == "pull_request" and not base_sha:
        return REFUSE, ["the current merge base is unknown; evidence cannot be bound to it"]
    runs = [r for r in runs if r.get("head_sha") == head_sha and r.get("event") == event]
    if not runs:
        return REFUSE, [
            f"no {m['evidence_workflow']} run for {event} head {head_sha}: a relevant input with no selected "
            "integration gate is refused (the workflow's own path filter did not select it, or it never started)"
        ]
    run = max(runs, key=lambda r: r["created_at"])
    if run.get("status") != "completed":
        return PENDING, [f"newest {m['evidence_workflow']} run {run['id']} is {run.get('status')}; evidence does not exist yet"]
    if run.get("conclusion") != "success":
        return REFUSE, [f"newest {m['evidence_workflow']} run {run['id']} concluded {run.get('conclusion')!r}"]
    jobs = jobs_by_run.get(run["id"], [])
    reasons: list[str] = []
    for name in m["required_jobs"]:
        job = next((j for j in jobs if j.get("name") == name), None)
        if job is None:
            reasons.append(f"required job '{name}' is absent from run {run['id']}")
            continue
        if job.get("conclusion") != "success":
            reasons.append(f"job '{name}' concluded {job.get('conclusion')!r}")
            continue
        if not job.get("runner_name"):
            reasons.append(f"job '{name}' has no runner identity (VOID: nothing executed it)")
        observed = [
            s for s in job.get("steps", [])
            if s.get("name", "").startswith(m["required_step_prefix"]) and s.get("conclusion") == "success"
        ]
        if not observed:
            reasons.append(f"job '{name}' completed no successful '{m['required_step_prefix']}*' step")
        why = bind_identity((identities or {}).get(job.get("id")), head_sha, event, base_sha, name)
        if why:
            reasons.append(why)
    if reasons:
        return REFUSE, reasons
    return ADMIT, [f"run {run['id']} on head {head_sha}" + (f" / base {base_sha}" if event == "pull_request" else "")
                   + f": {len(m['required_jobs'])} required jobs success on named runners"]


def _git(*args: str) -> str:
    try:
        return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout
    except (subprocess.CalledProcessError, FileNotFoundError) as exc:
        raise AdmissionError(f"git {' '.join(args)} failed: {exc}") from exc


def changed_files(event: str) -> tuple[list[str], list[str]]:
    """(paths, parents). --no-renames so a deleted/moved input counts on both sides."""
    parents = [l.split()[1] for l in _git("cat-file", "-p", "HEAD").splitlines() if l.startswith("parent ")]
    if event == "pull_request" and len(parents) != 2:
        raise AdmissionError(f"pull_request HEAD has {len(parents)} parents, not 2 -- not a merge tree")
    if not parents:
        raise AdmissionError("HEAD has no parent; cannot compute a diff")
    out = _git("diff", "--name-only", "--no-renames", "-z", f"{parents[0]}", "HEAD")
    return [p for p in out.split("\0") if p], parents


def api_get(repo: str, path: str, token: str) -> Any:
    req = urllib.request.Request(
        f"https://api.github.com/repos/{repo}/{path}",
        headers={"Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"},
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return json.load(resp)
    except (urllib.error.URLError, json.JSONDecodeError, TimeoutError) as exc:
        raise AdmissionError(f"GitHub API {path} failed: {exc}") from exc


def collect_evidence(repo: str, token: str, m: dict[str, Any], head_sha: str, get: Callable[[str, str, str], Any] = api_get):
    wf = m["evidence_workflow"]
    runs = get(repo, f"actions/workflows/{wf}/runs?head_sha={head_sha}&per_page=100", token)
    if "workflow_runs" not in runs:
        raise AdmissionError("runs response lacks workflow_runs")
    if runs.get("total_count", 0) > len(runs["workflow_runs"]):
        raise AdmissionError("runs response truncated (total_count > page); refusing to guess")
    selected = set()
    for event in ("pull_request", "push"):
        candidates = [r for r in runs["workflow_runs"] if r.get("head_sha") == head_sha and r.get("event") == event]
        if candidates:
            selected.add(max(candidates, key=lambda r: r["created_at"])["id"])
    jobs_by_run: dict[int, list[dict[str, Any]]] = {}
    for r in runs["workflow_runs"]:
        j = get(repo, f"actions/runs/{r['id']}/jobs?per_page=100", token)
        if j.get("total_count", 0) > len(j.get("jobs", [])):
            raise AdmissionError(f"jobs of run {r['id']} truncated; refusing to guess")
        jobs_by_run[r["id"]] = j["jobs"]
        # A completed jobs-list response can omit or lag step metadata even
        # after Observe has executed (#3536). Read that same job once through
        # its detail endpoint; never infer success from a job conclusion/log.
        if r["id"] not in selected or r.get("status") != "completed" or r.get("conclusion") != "success":
            continue
        for index, job in enumerate(jobs_by_run[r["id"]]):
            if (job.get("name") not in m["required_jobs"] or job.get("status") != "completed"
                    or job.get("conclusion") != "success" or not job.get("runner_name")):
                continue
            observed = [s for s in job.get("steps", [])
                        if s.get("name", "").startswith(m["required_step_prefix"])]
            # Preserve explicit negative evidence. Only absent/pending step
            # metadata warrants a refresh; no polling and no rerun is performed.
            if any(s.get("conclusion") is not None for s in observed):
                continue
            detail = get(repo, f"actions/jobs/{job['id']}", token)
            expected = {"id": job["id"], "name": job["name"], "head_sha": head_sha,
                        "run_id": r["id"], "run_attempt": r.get("run_attempt", 1),
                        "runner_name": job["runner_name"], "status": "completed"}
            if not isinstance(detail, dict) or any(detail.get(key) != value for key, value in expected.items()):
                raise AdmissionError(f"job {job['id']} detail identity disagrees with selected run/attempt/runner")
            jobs_by_run[r["id"]][index] = detail
    return runs["workflow_runs"], jobs_by_run


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *a, **k):  # noqa: D401 - we follow the redirect by hand
        return None


def collect_identities(repo: str, token: str, m: dict[str, Any], jobs_by_run: dict[int, list[dict[str, Any]]],
                       runs: list[dict[str, Any]], head_sha: str, event: str) -> dict[int, str | None]:
    """Checkout identity of each required leg of the newest matching run, from the first 64 KiB of its log.

    The logs endpoint answers 302 to a signed blob URL. We follow it by hand WITHOUT the bearer token and with a
    Range header, so the token never reaches the blob host and only the checkout step (the start) is downloaded.
    A failure yields None for that job (the evaluator refuses), never a guess.
    """
    cand = [r for r in runs if r.get("head_sha") == head_sha and r.get("event") == event]
    if not cand:
        return {}
    run = max(cand, key=lambda r: r["created_at"])
    out: dict[int, str | None] = {}
    opener = urllib.request.build_opener(_NoRedirect)
    for j in jobs_by_run.get(run["id"], []):
        if j.get("name") not in m["required_jobs"]:
            continue
        out[j["id"]] = None
        req = urllib.request.Request(
            f"https://api.github.com/repos/{repo}/actions/jobs/{j['id']}/logs",
            headers={"Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json"})
        try:
            try:
                resp = opener.open(req, timeout=30)
                text = resp.read(65536).decode("utf-8", "replace")
            except urllib.error.HTTPError as exc:
                if exc.code not in (301, 302, 303, 307, 308) or not exc.headers.get("Location"):
                    continue
                blob = urllib.request.Request(exc.headers["Location"], headers={"Range": "bytes=0-65535"})
                with urllib.request.urlopen(blob, timeout=30) as resp:
                    text = resp.read(65536).decode("utf-8", "replace")
            out[j["id"]] = parse_checkout_identity(text)
        except (urllib.error.URLError, TimeoutError, OSError):
            out[j["id"]] = None
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--self-test", action="store_true", help="run only the control assertions")
    args = ap.parse_args(argv)
    try:
        head_m = load_manifest(Path(MANIFEST).read_text())
        problems = self_test(head_m)
        if problems:
            for p in problems:
                print(f"::error::admission control failed: {p}")
            return 1
        print(f"controls: OK ({len(head_m['relevant_globs'])} globs, "
              f"{len(head_m['corpus']['must_be_relevant'])} relevant / {len(head_m['corpus']['must_not_be_relevant'])} irrelevant examples)")
        if args.self_test:
            return 0

        event = os.environ.get("GITHUB_EVENT_NAME", "")
        if event not in ("pull_request", "push"):
            raise AdmissionError(f"unsupported event {event!r}; refusing to attest")
        files, parents = changed_files(event)

        if event == "pull_request":
            try:
                base_text = _git("show", f"{parents[0]}:{MANIFEST}")
            except AdmissionError:
                print("::warning::UNANCHORED: the PR base has no admission manifest (first adoption); glob-drop ratchet not applied")
            else:
                ap_problems = anchor_problems(load_manifest(base_text), head_m)
                if ap_problems:
                    for p in ap_problems:
                        print(f"::error::admission anchor: {p}")
                    return 1
                print("anchor: OK (relevant_globs/required_jobs only grew vs the PR base)")
            head_sha = parents[1]
            payload = os.environ.get("PR_HEAD_SHA", "")
            if payload and payload != head_sha:
                print(f"::error::event head {payload} is not the checked-out PR head {head_sha}; stale replay")
                return 1
        else:
            head_sha = os.environ.get("GITHUB_SHA", "")
            if not head_sha:
                raise AdmissionError("GITHUB_SHA unset")

        relevant = [f for f in files if is_relevant(head_m["relevant_globs"], f)]
        print(f"scope: {len(files)} changed path(s), {len(relevant)} relevant to integration assertions")
        for f in relevant[:20]:
            print(f"  relevant: {f}")
        if len(relevant) > 20:
            print(f"  ... and {len(relevant) - 20} more")
        if not relevant:
            print("ADMIT (scope only): no changed path is an integration input. This is a scope statement,"
                  " not integration evidence.")
            return 0

        token, repo = os.environ.get("GITHUB_TOKEN", ""), os.environ.get("GITHUB_REPOSITORY", "")
        if not token or not repo:
            raise AdmissionError("GITHUB_TOKEN/GITHUB_REPOSITORY unset; cannot read integration evidence")
        runs, jobs = collect_evidence(repo, token, head_m, head_sha)
        identities = collect_identities(repo, token, head_m, jobs, runs, head_sha, event)
        base_sha = parents[0] if event == "pull_request" else ""
        verdict, reasons = evaluate_evidence(head_m, runs, jobs, head_sha, event, base_sha, identities)
        for r in reasons:
            print(f"  {verdict}: {r}")
        if verdict == ADMIT:
            print(f"ADMIT: current-head integration evidence present for {head_sha}")
            return 0
        print(f"::error::REFUSE ({verdict}): relevant repository inputs changed without admitted integration evidence"
              + (" -- re-run THIS job after the ratchet completes (done automatically by "
                 "rerun-pending-admission.yml)" if verdict == PENDING else ""))
        return exit_code_for(verdict)
    except AdmissionError as exc:
        print(f"::error::admission could not reach a verdict (fail closed): {exc}")
        return 2


if __name__ == "__main__":
    sys.exit(main())
