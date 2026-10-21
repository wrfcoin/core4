#!/usr/bin/env python3
"""Tests for rerun_pending_admission.py and the PENDING signal of the admission check (Refs #3446).

Run: python3 scripts/test_rerun_pending_admission.py
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_repository_input_admission as adm  # noqa: E402
import rerun_pending_admission as rr  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
HEAD = "a" * 40
JOB_NAME = rr.ADMISSION_JOB_NAME


def arun(rid=10, status="completed", conclusion="failure", event="pull_request", head=HEAD,
         created="2026-10-06T10:00:00Z", attempt=1):
    return {"id": rid, "status": status, "conclusion": conclusion, "event": event, "head_sha": head,
            "created_at": created, "run_attempt": attempt}


def ajob(jid=500, conclusion="failure", status="completed", pending_step=True, name=JOB_NAME):
    steps = [{"name": "Classify", "conclusion": "success"}]
    steps.append({"name": adm.PENDING_STEP_NAME if pending_step else "Fail closed: REFUSE",
                  "conclusion": "failure"})
    return {"id": jid, "name": name, "status": status, "conclusion": conclusion, "steps": steps}


class PendingSignal(unittest.TestCase):
    def test_exit_codes_distinguish_pending_from_refuse(self):
        self.assertEqual(adm.exit_code_for(adm.ADMIT), 0)
        self.assertEqual(adm.exit_code_for(adm.REFUSE), 1)
        self.assertEqual(adm.exit_code_for(adm.PENDING), adm.PENDING_EXIT)
        self.assertNotIn(adm.PENDING_EXIT, (0, 1, 2))

    def test_workflow_publishes_the_pending_step_name(self):
        text = (ROOT / ".github/workflows/repository-input-admission.yml").read_text()
        self.assertIn(f"name: {adm.PENDING_STEP_NAME}", text)
        self.assertIn(f"name: {rr.ADMISSION_JOB_NAME}", text)


class Selector(unittest.TestCase):
    def sel(self, runs, jobs, event="pull_request"):
        return rr.select(runs, jobs, HEAD, event)

    def test_pending_failure_is_rerun(self):
        d = self.sel([arun()], {10: [ajob()]})
        self.assertEqual((d.action, d.job_id), (rr.RERUN, 500))

    def test_real_refuse_is_never_rerun(self):
        d = self.sel([arun()], {10: [ajob(pending_step=False)]})
        self.assertEqual(d.action, rr.SKIP)

    def test_success_is_not_rerun(self):
        d = self.sel([arun(conclusion="success")], {10: [ajob(conclusion="success")]})
        self.assertEqual(d.action, rr.SKIP)

    def test_cancelled_is_not_rerun(self):
        d = self.sel([arun(conclusion="cancelled")], {10: [ajob(conclusion="cancelled")]})
        self.assertEqual(d.action, rr.SKIP)

    def test_other_head_or_event_is_ignored(self):
        runs = [arun(head="b" * 40), arun(rid=11, event="push")]
        self.assertEqual(self.sel(runs, {10: [ajob()], 11: [ajob()]}).action, rr.SKIP)

    def test_only_the_newest_run_counts(self):
        runs = [arun(rid=10, created="2026-10-06T10:00:00Z"),
                arun(rid=11, created="2026-10-06T11:00:00Z", conclusion="success")]
        d = self.sel(runs, {10: [ajob()], 11: [ajob(conclusion="success")]})
        self.assertEqual(d.action, rr.SKIP)

    def test_running_admission_is_a_race_to_retry(self):
        d = self.sel([arun(status="in_progress", conclusion=None)], {10: [ajob(status="in_progress", conclusion=None)]})
        self.assertEqual(d.action, rr.RETRY)

    def test_no_admission_run_skips(self):
        self.assertEqual(self.sel([], {}).action, rr.SKIP)

    def test_attempt_cap_prevents_loops(self):
        d = self.sel([arun(attempt=rr.MAX_ATTEMPTS)], {10: [ajob()]})
        self.assertEqual(d.action, rr.SKIP)

    def test_other_jobs_are_never_touched(self):
        d = self.sel([arun()], {10: [ajob(name="something else")]})
        self.assertEqual(d.action, rr.SKIP)

    def test_pending_step_must_have_failed(self):
        j = ajob()
        j["steps"][-1]["conclusion"] = "skipped"
        self.assertEqual(self.sel([arun()], {10: [j]}).action, rr.SKIP)


class Driver(unittest.TestCase):
    def fake(self, runs, jobs):
        posts = []

        def get(path):
            if "/runs?" in path:
                return {"total_count": len(runs), "workflow_runs": runs}
            rid = int(path.split("/runs/")[1].split("/")[0])
            return {"total_count": len(jobs[rid]), "jobs": jobs[rid]}

        return get, (lambda path: posts.append(path)), posts

    def test_rerun_posts_once_to_the_job_endpoint(self):
        get, post, posts = self.fake([arun()], {10: [ajob()]})
        rc = rr.run("o/r", HEAD, "pull_request", get, post, sleep=lambda s: None)
        self.assertEqual((rc, posts), (0, ["actions/jobs/500/rerun"]))

    def test_refuse_posts_nothing(self):
        get, post, posts = self.fake([arun()], {10: [ajob(pending_step=False)]})
        self.assertEqual(rr.run("o/r", HEAD, "pull_request", get, post, sleep=lambda s: None), 0)
        self.assertEqual(posts, [])

    def test_race_retries_then_reruns(self):
        state = {"n": 0}
        running = ([arun(status="in_progress", conclusion=None)], {10: [ajob(status="in_progress", conclusion=None)]})
        done = ([arun()], {10: [ajob()]})
        posts = []

        def get(path):
            runs, jobs = running if state["n"] < 2 else done
            if "/runs?" in path:
                state["n"] += 1
                return {"total_count": 1, "workflow_runs": runs}
            return {"total_count": 1, "jobs": jobs[10]}

        rc = rr.run("o/r", HEAD, "pull_request", get, posts.append, sleep=lambda s: None)
        self.assertEqual((rc, posts), (0, ["actions/jobs/500/rerun"]))

    def test_bad_inputs_are_rejected(self):
        for sha, ev in (("zz", "pull_request"), (HEAD, "schedule"), (HEAD[:39], "push")):
            with self.assertRaises(rr.RerunError):
                rr.run("o/r", sha, ev, lambda p: {}, lambda p: None, sleep=lambda s: None)

    def test_truncated_response_fails_closed(self):
        def get(path):
            return {"total_count": 5, "workflow_runs": [arun()]}
        with self.assertRaises(rr.RerunError):
            rr.run("o/r", HEAD, "pull_request", get, lambda p: None, sleep=lambda s: None)


if __name__ == "__main__":
    unittest.main(verbosity=1)
