#!/usr/bin/env python3
"""Re-run the admission job that failed ONLY because the integration ratchet was still running (Refs #3446).

The `repository-input integration admission` job refuses PENDING evidence and deliberately does not poll (it would
hold a runner while the ratchet queues behind it). This script is the event-driven half: the
`rerun-pending-admission.yml` workflow calls it when the ratchet COMPLETES, for that run's head SHA.

SELECTION (all must hold, otherwise nothing is re-run):
  * newest admission run for exactly this head SHA and event (an older run is superseded);
  * that run completed with conclusion `failure`;
  * its job named ADMISSION_JOB_NAME failed AND the step named PENDING_STEP_NAME failed. That step exists only when
    the checker exited with PENDING_EXIT, so a real REFUSE / error is never selected and no free text is parsed;
  * run_attempt < MAX_ATTEMPTS (loop guard).
If the admission job is still running (it read PENDING a moment before the ratchet finished) we retry a few times.
One workflow_run event posts at most one rerun, to exactly one job id. Stdlib only; fail closed.
"""

from __future__ import annotations

import json
import os
import re
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any, Callable

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from check_repository_input_admission import PENDING_STEP_NAME  # noqa: E402

ADMISSION_WORKFLOW = "repository-input-admission.yml"
ADMISSION_JOB_NAME = "post-merge repository-input integration admission"
EVENTS = ("pull_request", "push")
MAX_ATTEMPTS = 5
RETRIES, RETRY_SLEEP = 6, 20
RERUN, SKIP, RETRY = "RERUN", "SKIP", "RETRY"


class RerunError(Exception):
    pass


@dataclass
class Decision:
    action: str
    reason: str
    job_id: int | None = None


def select(runs: list[dict[str, Any]], jobs_by_run: dict[int, list[dict[str, Any]]], head_sha: str, event: str) -> Decision:
    runs = [r for r in runs if r.get("head_sha") == head_sha and r.get("event") == event]
    if not runs:
        return Decision(SKIP, f"no admission run for {event} head {head_sha}")
    run = max(runs, key=lambda r: r["created_at"])
    if run.get("status") != "completed":
        return Decision(RETRY, f"admission run {run['id']} is {run.get('status')}; it may not have recorded its verdict yet")
    if run.get("conclusion") != "failure":
        return Decision(SKIP, f"admission run {run['id']} concluded {run.get('conclusion')!r}; nothing to re-run")
    if run.get("run_attempt", 1) >= MAX_ATTEMPTS:
        return Decision(SKIP, f"admission run {run['id']} already at attempt {run.get('run_attempt')}; loop guard")
    job = next((j for j in jobs_by_run.get(run["id"], []) if j.get("name") == ADMISSION_JOB_NAME), None)
    if job is None or job.get("conclusion") != "failure":
        return Decision(SKIP, f"admission job absent or not failed in run {run['id']}")
    if not any(s.get("name") == PENDING_STEP_NAME and s.get("conclusion") == "failure" for s in job.get("steps", [])):
        return Decision(SKIP, f"job {job['id']} failed for a reason other than PENDING (real REFUSE or error); not re-run")
    return Decision(RERUN, f"job {job['id']} of run {run['id']} failed PENDING", job["id"])


def api_get(repo: str, token: str) -> Callable[[str], Any]:
    def get(path: str) -> Any:
        req = urllib.request.Request(f"https://api.github.com/repos/{repo}/{path}", headers={
            "Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"})
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                return json.load(resp)
        except (urllib.error.URLError, json.JSONDecodeError, TimeoutError) as exc:
            raise RerunError(f"GitHub API GET {path} failed: {exc}") from exc
    return get


def api_post(repo: str, token: str) -> Callable[[str], None]:
    def post(path: str) -> None:
        req = urllib.request.Request(f"https://api.github.com/repos/{repo}/{path}", method="POST", data=b"", headers={
            "Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"})
        try:
            with urllib.request.urlopen(req, timeout=30):
                pass
        except (urllib.error.URLError, TimeoutError) as exc:
            raise RerunError(f"GitHub API POST {path} failed: {exc}") from exc
    return post


def _collect(get: Callable[[str], Any], head_sha: str) -> tuple[list[dict[str, Any]], dict[int, list[dict[str, Any]]]]:
    resp = get(f"actions/workflows/{ADMISSION_WORKFLOW}/runs?head_sha={head_sha}&per_page=100")
    if "workflow_runs" not in resp:
        raise RerunError("runs response lacks workflow_runs")
    if resp.get("total_count", 0) > len(resp["workflow_runs"]):
        raise RerunError("runs response truncated; refusing to guess")
    jobs: dict[int, list[dict[str, Any]]] = {}
    for r in resp["workflow_runs"]:
        j = get(f"actions/runs/{r['id']}/jobs?per_page=100")
        if j.get("total_count", 0) > len(j.get("jobs", [])):
            raise RerunError(f"jobs of run {r['id']} truncated; refusing to guess")
        jobs[r["id"]] = j["jobs"]
    return resp["workflow_runs"], jobs


def run(repo: str, head_sha: str, event: str, get: Callable[[str], Any], post: Callable[[str], None],
        sleep: Callable[[float], None] = time.sleep) -> int:
    if not re.fullmatch(r"[0-9a-f]{40}", head_sha or ""):
        raise RerunError(f"head sha {head_sha!r} is not a full 40-hex sha")
    if event not in EVENTS:
        raise RerunError(f"event {event!r} is not one of {EVENTS}")
    for attempt in range(RETRIES):
        d = select(*_collect(get, head_sha), head_sha, event)
        print(f"{d.action}: {d.reason}")
        if d.action == RETRY and attempt < RETRIES - 1:
            sleep(RETRY_SLEEP)
            continue
        if d.action == RERUN:
            post(f"actions/jobs/{d.job_id}/rerun")
            print(f"re-run requested for job {d.job_id}")
        return 0
    return 0


def main() -> int:
    try:
        repo, token = os.environ.get("GITHUB_REPOSITORY", ""), os.environ.get("GITHUB_TOKEN", "")
        if not repo or not token:
            raise RerunError("GITHUB_REPOSITORY/GITHUB_TOKEN unset")
        return run(repo, os.environ.get("RATCHET_HEAD_SHA", ""), os.environ.get("RATCHET_EVENT", ""),
                   api_get(repo, token), api_post(repo, token))
    except RerunError as exc:
        print(f"::error::rerun-pending-admission failed closed: {exc}")
        return 2


if __name__ == "__main__":
    sys.exit(main())
