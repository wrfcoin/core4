#!/usr/bin/env python3
"""Tests for check_repository_input_admission.py (Refs #2433). Run: python3 scripts/test_check_repository_input_admission.py"""

from __future__ import annotations

import copy
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_repository_input_admission as adm  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
M = adm.load_manifest((ROOT / adm.MANIFEST).read_text())
HEAD = "a" * 40
JOB_A, JOB_B = M["required_jobs"]


def run(rid=1, status="completed", conclusion="success", event="pull_request", head=HEAD, created="2026-10-03T10:00:00Z"):
    return {"id": rid, "status": status, "conclusion": conclusion, "event": event, "head_sha": head, "created_at": created}


BASE = "b" * 40
OLD_BASE = "c" * 40


def job(name, conclusion="success", runner="n5pro-ct102", steps=None):
    if steps is None:
        steps = [{"name": "Observe — run it", "conclusion": "success"}]
    return {"id": 100 + (name == JOB_B), "name": name, "conclusion": conclusion, "runner_name": runner, "steps": steps}


MERGE_SHA = "0a1b2c3d4e" + "5" * 30  # pragma: allowlist secret (synthetic merge commit sha)
STALE = "HEAD is now at d02ccb9fc5 Merge %s into %s" % ("e" * 40, OLD_BASE)
PR_LOG = (
    "[command]/usr/bin/git reset --hard HEAD\n" + STALE + "\n"
    "##[group]Checking out the ref\n[command]/usr/bin/git checkout --progress --force refs/remotes/pull/9/merge\n"
    "HEAD is now at 0a1b2c3d4e Merge %s into %s\n##[endgroup]\n[command]/usr/bin/git log -1 --format=%%H\n"
    "%s\n" % (HEAD, BASE, MERGE_SHA))
PUSH_LOG = (
    "[command]/usr/bin/git reset --hard HEAD\n" + STALE + "\n"
    "##[group]Checking out the ref\n[command]/usr/bin/git checkout --progress --force -B main refs/remotes/origin/main\n"
    "Previous HEAD position was d02ccb9fc5 Merge\nSwitched to a new branch 'main'\n##[endgroup]\n"
    "[command]/usr/bin/git log -1 --format=%%H\n%s\n" % HEAD)


def merge_subject(head=HEAD, base=BASE):
    return f"abc1234 Merge {head} into {base}"


def good_identities(subject=None):
    return {100: subject or merge_subject(), 101: subject or merge_subject()}


def good_jobs():
    return [job(JOB_A), job(JOB_B)]


class Classifier(unittest.TestCase):
    def test_shipped_manifest_controls_hold(self):
        self.assertEqual(adm.self_test(M), [])

    def test_the_3436_json_and_cut_pins_env_are_relevant(self):
        for p in ("docs/deployment/live-feature-schedule/777-testnet.json", "testnet/cut-pins.env"):
            self.assertTrue(adm.is_relevant(M["relevant_globs"], p), p)

    def test_unrelated_docs_are_not(self):
        for p in ("README.md", "docs/ci/REQUIRED-CHECKS-R21.md", "docs/a.png"):
            self.assertFalse(adm.is_relevant(M["relevant_globs"], p), p)

    def _without(self, glob):
        m = copy.deepcopy(M)
        m["relevant_globs"].remove(glob)
        return m

    def test_removing_the_json_pattern_is_caught(self):
        self.assertTrue(any("777-testnet.json" in p for p in adm.self_test(self._without("**/*.json"))))

    def test_removing_the_env_pattern_is_caught(self):
        self.assertTrue(any("cut-pins.env" in p for p in adm.self_test(self._without("**/*.env"))))

    def test_a_dead_glob_is_caught(self):
        m = copy.deepcopy(M)
        m["relevant_globs"].append("nothing/**/*.zzz")
        self.assertTrue(any("nothing/**/*.zzz" in p for p in adm.self_test(m)))

    def test_an_always_yes_glob_is_caught(self):
        m = copy.deepcopy(M)
        m["relevant_globs"].append("**")
        self.assertTrue(any("must NOT be relevant" in p for p in adm.self_test(m)))

    def test_empty_matcher_says_no(self):
        self.assertFalse(adm.is_relevant([], "native-chain/src/lib.rs"))


class Anchor(unittest.TestCase):
    def test_growth_is_allowed(self):
        head = copy.deepcopy(M)
        head["relevant_globs"].append("**/*.ini")
        self.assertEqual(adm.anchor_problems(M, head), [])

    def test_dropping_a_glob_is_refused(self):
        head = copy.deepcopy(M)
        head["relevant_globs"].remove("**/*.json")
        self.assertEqual(len(adm.anchor_problems(M, head)), 1)

    def test_dropping_a_required_job_or_changing_the_step_prefix_is_refused(self):
        head = copy.deepcopy(M)
        head["required_jobs"].pop()
        head["required_step_prefix"] = "Set up"
        self.assertEqual(len(adm.anchor_problems(M, head)), 2)


class Evidence(unittest.TestCase):
    def ev(self, runs, jobs, event="pull_request", base=BASE, identities="good"):
        ids = good_identities() if identities == "good" else identities
        return adm.evaluate_evidence(M, runs, jobs, HEAD, event, base, ids)

    def test_green_run_on_named_runners_admits(self):
        self.assertEqual(self.ev([run()], {1: good_jobs()})[0], adm.ADMIT)

    def test_no_run_is_refused_not_skipped(self):
        self.assertEqual(self.ev([], {})[0], adm.REFUSE)

    def test_run_for_another_head_or_event_does_not_count(self):
        self.assertEqual(self.ev([run(head="b" * 40)], {1: good_jobs()})[0], adm.REFUSE)
        self.assertEqual(self.ev([run(event="push")], {1: good_jobs()})[0], adm.REFUSE)

    def test_in_progress_is_pending(self):
        self.assertEqual(self.ev([run(status="in_progress", conclusion=None)], {})[0], adm.PENDING)

    def test_failed_or_cancelled_run_is_refused(self):
        for c in ("failure", "cancelled", "skipped", None):
            self.assertEqual(self.ev([run(conclusion=c)], {1: good_jobs()})[0], adm.REFUSE, c)

    def test_newest_run_decides(self):
        old_green = run(1, created="2026-10-03T09:00:00Z")
        new_red = run(2, conclusion="failure", created="2026-10-03T11:00:00Z")
        self.assertEqual(self.ev([old_green, new_red], {1: good_jobs(), 2: good_jobs()})[0], adm.REFUSE)

    def test_missing_job_is_refused(self):
        self.assertEqual(self.ev([run()], {1: [job(JOB_A)]})[0], adm.REFUSE)

    def test_skipped_job_under_a_green_run_is_refused(self):
        self.assertEqual(self.ev([run()], {1: [job(JOB_A), job(JOB_B, conclusion="skipped")]})[0], adm.REFUSE)

    def test_empty_runner_is_void_refused(self):
        self.assertEqual(self.ev([run()], {1: [job(JOB_A, runner=""), job(JOB_B)]})[0], adm.REFUSE)

    def test_wrapper_only_job_without_an_observe_step_is_refused(self):
        bare = [{"name": "Set up job", "conclusion": "success"}, {"name": "Complete job", "conclusion": "success"}]
        self.assertEqual(self.ev([run()], {1: [job(JOB_A, steps=bare), job(JOB_B)]})[0], adm.REFUSE)

    def test_failed_observe_step_is_refused(self):
        bad = [{"name": "Observe — run it", "conclusion": "failure"}]
        self.assertEqual(self.ev([run()], {1: [job(JOB_A, steps=bad), job(JOB_B)]})[0], adm.REFUSE)


class BaseBinding(unittest.TestCase):
    """SOL-R3 P1: evidence must be bound to the CURRENT merge base, not just the head SHA."""

    def ev(self, identities, base=BASE, event="pull_request"):
        return adm.evaluate_evidence(M, [run(event=event)], {1: good_jobs()}, HEAD, event, base, identities)

    def test_matching_head_and_current_base_admits(self):
        self.assertEqual(self.ev(good_identities())[0], adm.ADMIT)

    def test_evidence_computed_against_an_old_base_is_refused(self):
        v, why = self.ev(good_identities(merge_subject(base=OLD_BASE)))
        self.assertEqual(v, adm.REFUSE)
        self.assertTrue(any("base" in r for r in why), why)

    def test_one_stale_leg_is_enough_to_refuse(self):
        ids = {100: merge_subject(), 101: merge_subject(base=OLD_BASE)}
        self.assertEqual(self.ev(ids)[0], adm.REFUSE)

    def test_missing_checkout_identity_is_refused(self):
        self.assertEqual(self.ev({100: merge_subject()})[0], adm.REFUSE)
        self.assertEqual(self.ev({100: None, 101: None})[0], adm.REFUSE)
        self.assertEqual(self.ev(None)[0], adm.REFUSE)

    def test_unparseable_checkout_identity_is_refused(self):
        self.assertEqual(self.ev(good_identities("abc1234 some other commit"))[0], adm.REFUSE)

    def test_merge_of_another_head_is_refused(self):
        self.assertEqual(self.ev(good_identities(merge_subject(head="d" * 40)))[0], adm.REFUSE)

    def test_pull_request_without_a_known_base_is_refused(self):
        self.assertEqual(self.ev(good_identities(), base="")[0], adm.REFUSE)

    def test_push_evidence_must_be_a_checkout_of_the_head_itself(self):
        ok = good_identities(f"{HEAD} (branch checkout)")
        self.assertEqual(self.ev(ok, event="push")[0], adm.ADMIT)
        bad = good_identities("1234567" + "1" * 33 + " (branch checkout)")
        self.assertEqual(self.ev(bad, event="push")[0], adm.REFUSE)

    def test_push_checkout_with_the_same_7_char_prefix_but_another_full_sha_is_refused(self):
        # SOL-R3 P2 (33c5e090): compare complete 40-hex shas only.
        other = HEAD[:7] + "d" * 33
        self.assertNotEqual(other, HEAD)
        self.assertEqual(self.ev(good_identities(f"{other} (branch checkout)"), event="push")[0], adm.REFUSE)
        self.assertEqual(self.ev(good_identities(f"{HEAD[:7]} (branch checkout)"), event="push")[0], adm.REFUSE)

    def test_the_parser_keeps_the_complete_logged_sha(self):
        ident = adm.parse_checkout_identity(PUSH_LOG)
        self.assertEqual(ident.split()[0], HEAD)
        self.assertEqual(len(ident.split()[0]), 40)

    def test_activation_prerequisite_is_recorded_and_no_auto_invalidation_is_claimed(self):
        # SOL-R3 P1 (33c5e090): a completed status is not re-run when only the base advances.
        for rel in (".github/workflows/repository-input-admission.yml", "scripts/check_repository_input_admission.py"):
            text = (ROOT / rel).read_text()
            self.assertIn("ACTIVATION PREREQUISITE", text, rel)
            self.assertNotIn("never carries over", text, rel)
            self.assertNotIn("goes red", text, rel)
        doc = (ROOT / "docs/ci/REPOSITORY-INPUT-ADMISSION.md").read_text()
        for needle in ("strict_required_status_checks_policy", "merge queue", "Not enforced by this PR"):
            self.assertIn(needle, doc)

    def test_parse_checkout_identity(self):
        self.assertEqual(adm.parse_checkout_identity(PR_LOG), f"0a1b2c3d4e Merge {HEAD} into {BASE}")
        self.assertIsNone(adm.parse_checkout_identity("no checkout line here"))

    def test_the_stale_workspace_reset_line_is_not_the_checkout(self):
        # Live logs (runner n5pro-ct102-2, job 111258250970): the persistent workspace's `git reset --hard HEAD`
        # prints the PREVIOUS run's merge commit BEFORE "Checking out the ref". Reading that line as the identity
        # would bind evidence to another PR's base.
        self.assertEqual(adm.parse_checkout_identity(PUSH_LOG), f"{HEAD} (branch checkout)")
        self.assertNotIn("Merge", adm.parse_checkout_identity(PUSH_LOG))

    def test_identity_without_the_checked_out_sha_is_unavailable(self):
        self.assertIsNone(adm.parse_checkout_identity(PR_LOG.replace("git log -1 --format=%H", "git log -1 --oneline")))

    def test_post_marker_line_must_agree_with_the_checked_out_sha(self):
        self.assertIsNone(adm.parse_checkout_identity(PR_LOG.replace(MERGE_SHA, "f" * 40)))


class MainPathFailsClosed(unittest.TestCase):
    """SOL-R3 P2: through main(), pending evidence must never be a successful context, on PR or push."""

    def run_main(self, event, runs, jobs, files=("settings.json",)):
        import os
        saved = (adm.changed_files, adm.collect_evidence, adm.collect_identities, dict(os.environ))
        env = {"GITHUB_EVENT_NAME": event, "GITHUB_TOKEN": "t", "GITHUB_REPOSITORY": "o/r", "GITHUB_SHA": HEAD, "PR_HEAD_SHA": HEAD}
        adm.changed_files = lambda ev: (list(files), [BASE, HEAD] if ev == "pull_request" else [BASE])
        adm.collect_evidence = lambda *a: (runs, jobs)
        adm.collect_identities = lambda *a: good_identities(
            merge_subject() if event == "pull_request" else f"{HEAD} (branch checkout)")
        os.environ.update(env)
        try:
            return adm.main([])
        finally:
            adm.changed_files, adm.collect_evidence, adm.collect_identities = saved[:3]
            os.environ.clear(); os.environ.update(saved[3])

    def test_push_with_in_progress_ratchet_fails(self):
        self.assertNotEqual(self.run_main("push", [run(event="push", status="in_progress", conclusion=None)], {}), 0)

    def test_pull_request_with_in_progress_ratchet_fails(self):
        self.assertNotEqual(self.run_main("pull_request", [run(status="in_progress", conclusion=None)], {}), 0)

    def test_push_with_no_ratchet_run_fails(self):
        self.assertNotEqual(self.run_main("push", [], {}), 0)

    def test_push_with_green_current_evidence_passes(self):
        self.assertEqual(self.run_main("push", [run(event="push")], {1: good_jobs()}), 0)

    def test_irrelevant_change_passes_without_evidence(self):
        self.assertEqual(self.run_main("push", [], {}, files=("README.md",)), 0)


class CollectEvidenceFailsClosed(unittest.TestCase):
    def collect_with_detail(self, detail=None, listing=None, evidence_run=None):
        evidence_run = evidence_run or {**run(), "run_attempt": 1}
        full = [{**j, "run_id": 1, "run_attempt": 1, "head_sha": HEAD, "status": "completed"}
                for j in good_jobs()]
        if detail is None:
            detail = full[0]
        if listing is None:
            listing = [{**full[0], "steps": []}, full[1]]
        paths = []

        def get(repo, path, tok):
            paths.append(path)
            if path.startswith("actions/workflows/"):
                return {"workflow_runs": [evidence_run], "total_count": 1}
            if path == "actions/runs/1/jobs?per_page=100":
                return {"jobs": listing, "total_count": len(listing)}
            if path == "actions/jobs/100":
                return detail
            self.fail(f"unexpected metadata read: {path}")

        runs, jobs = adm.collect_evidence("o/r", "t", M, HEAD, get)
        verdict = adm.evaluate_evidence(M, runs, jobs, HEAD, "pull_request", BASE, good_identities())[0]
        return verdict, paths

    def test_empty_list_steps_refresh_from_same_completed_job_before_admission(self):
        verdict, paths = self.collect_with_detail()
        self.assertEqual(verdict, adm.ADMIT)
        self.assertEqual(paths.count("actions/jobs/100"), 1)

    def test_incomplete_observe_metadata_refreshes_from_same_completed_job(self):
        full = [{**j, "run_id": 1, "run_attempt": 1, "head_sha": HEAD, "status": "completed"}
                for j in good_jobs()]
        listing = [{**full[0], "steps": [{"name": "Observe — run it", "conclusion": None}]}, full[1]]
        self.assertEqual(self.collect_with_detail(listing=listing)[0], adm.ADMIT)

    def test_refreshed_metadata_still_requires_successful_observe(self):
        for conclusion in (None, "failure", "skipped", "cancelled"):
            with self.subTest(conclusion=conclusion):
                detail = {**job(JOB_A, steps=[{"name": "Observe — run it", "conclusion": conclusion}]),
                          "run_id": 1, "run_attempt": 1, "head_sha": HEAD, "status": "completed"}
                self.assertEqual(self.collect_with_detail(detail=detail)[0], adm.REFUSE)

    def test_refresh_cannot_substitute_another_job_head_run_attempt_or_runner(self):
        full = {**job(JOB_A), "run_id": 1, "run_attempt": 1, "head_sha": HEAD, "status": "completed"}
        for field, value in (("id", 999), ("name", JOB_B), ("head_sha", "d" * 40),
                             ("run_id", 2), ("run_attempt", 2), ("runner_name", "another-runner")):
            with self.subTest(field=field), self.assertRaises(adm.AdmissionError):
                self.collect_with_detail(detail={**full, field: value})

    def test_missing_detail_identity_fails_closed(self):
        full = {**job(JOB_A), "run_id": 1, "run_attempt": 1, "head_sha": HEAD, "status": "completed"}
        for field in ("id", "name", "head_sha", "run_id", "run_attempt", "runner_name", "status"):
            with self.subTest(field=field), self.assertRaises(adm.AdmissionError):
                self.collect_with_detail(detail={k: v for k, v in full.items() if k != field})

    def test_detail_of_a_noncompleted_job_cannot_admit(self):
        detail = {**job(JOB_A), "run_id": 1, "run_attempt": 1, "head_sha": HEAD, "status": "in_progress"}
        with self.assertRaises(adm.AdmissionError):
            self.collect_with_detail(detail=detail)

    def test_superseded_run_metadata_does_not_trigger_detail_refresh(self):
        older = {**run(), "run_attempt": 1}
        newer = {**run(rid=2, created="2026-10-03T11:00:00Z"), "run_attempt": 1}
        reads = []

        def get(repo, path, tok):
            reads.append(path)
            if path.startswith("actions/workflows/"):
                return {"workflow_runs": [older, newer], "total_count": 2}
            if path == "actions/runs/1/jobs?per_page=100":
                return {"jobs": [{**job(JOB_A, steps=[]), "status": "completed"}], "total_count": 1}
            if path == "actions/runs/2/jobs?per_page=100":
                return {"jobs": good_jobs(), "total_count": 2}
            self.fail(f"superseded evidence was refreshed: {path}")

        runs, jobs = adm.collect_evidence("o/r", "t", M, HEAD, get)
        self.assertEqual(adm.evaluate_evidence(M, runs, jobs, HEAD, "pull_request", BASE, good_identities())[0], adm.ADMIT)
        self.assertFalse(any(path.startswith("actions/jobs/") for path in reads))

    def test_complete_successful_list_metadata_needs_no_refresh(self):
        verdict, paths = self.collect_with_detail(listing=good_jobs())
        self.assertEqual(verdict, adm.ADMIT)
        self.assertNotIn("actions/jobs/100", paths)

    def test_explicit_failed_or_skipped_observe_is_not_refreshed_away(self):
        for conclusion in ("failure", "skipped", "cancelled", "timed_out"):
            with self.subTest(conclusion=conclusion):
                bad = job(JOB_A, steps=[{"name": "Observe — run it", "conclusion": conclusion}])
                verdict, paths = self.collect_with_detail(listing=[bad, job(JOB_B)])
                self.assertEqual(verdict, adm.REFUSE)
                self.assertNotIn("actions/jobs/100", paths)

    def test_failed_pending_or_runnerless_jobs_are_not_refreshed(self):
        for change in ({"conclusion": "failure"}, {"status": "in_progress"}, {"runner_name": ""}):
            with self.subTest(change=change):
                bad = {**job(JOB_A, steps=[]), "status": "completed", **change}
                verdict, paths = self.collect_with_detail(listing=[bad, job(JOB_B)])
                self.assertEqual(verdict, adm.REFUSE)
                self.assertNotIn("actions/jobs/100", paths)

    def test_running_or_failed_runs_are_not_refreshed(self):
        for change, expected in (({"status": "in_progress"}, adm.PENDING),
                                 ({"conclusion": "failure"}, adm.REFUSE)):
            with self.subTest(change=change):
                verdict, paths = self.collect_with_detail(evidence_run={**run(), "run_attempt": 1, **change})
                self.assertEqual(verdict, expected)
                self.assertNotIn("actions/jobs/100", paths)

    def test_truncated_runs_page_is_an_error(self):
        get = lambda repo, path, tok: {"workflow_runs": [run()], "total_count": 2}
        with self.assertRaises(adm.AdmissionError):
            adm.collect_evidence("o/r", "t", M, HEAD, get)

    def test_truncated_jobs_page_is_an_error(self):
        def get(repo, path, tok):
            if "/jobs" in path:
                return {"jobs": [job(JOB_A)], "total_count": 2}
            return {"workflow_runs": [run()], "total_count": 1}
        with self.assertRaises(adm.AdmissionError):
            adm.collect_evidence("o/r", "t", M, HEAD, get)

    def test_malformed_runs_response_is_an_error(self):
        with self.assertRaises(adm.AdmissionError):
            adm.collect_evidence("o/r", "t", M, HEAD, lambda *a: {"message": "Not Found"})


class ManifestShape(unittest.TestCase):
    def test_required_jobs_exist_in_the_evidence_workflow(self):
        # Control against drift: a renamed ratchet job would make every relevant PR unadmittable.
        text = (ROOT / ".github/workflows" / M["evidence_workflow"]).read_text()
        for name in M["required_jobs"]:
            self.assertIn(name, text, name)

    def test_required_step_prefix_exists_in_every_required_job_workflow(self):
        text = (ROOT / ".github/workflows" / M["evidence_workflow"]).read_text()
        self.assertIn(f"name: {M['required_step_prefix']}", text)

    def test_load_manifest_rejects_garbage(self):
        for bad in ("not json", "{}", json.dumps({**M, "relevant_globs": []}), json.dumps({**M, "required_jobs": [""]})):
            with self.assertRaises(adm.AdmissionError):
                adm.load_manifest(bad)


if __name__ == "__main__":
    unittest.main(verbosity=1)
