#!/usr/bin/env python3
"""Tests for check_void_gates.py (#2923).

The detector's whole value is telling apart three things that look identical in a
check list: a real result, a VOID job, and a job that was cancelled. Each test
here is a synthetic fixture for one of those, plus the mutation cases that prove
the detector reddens rather than merely existing.
"""

from __future__ import annotations

import json
from datetime import date
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_void_gates as void  # noqa: E402

REAL = {"id": 1, "name": "real", "runner_name": "n5pro-ct102", "steps": [{"n": 1}],
        "conclusion": "success", "labels": ["self-hosted", "n5pro"]}
REAL_HOSTED = {"id": 10, "name": "real-hosted", "runner_name": "GitHub Actions 5",
               "steps": [{"n": 1}], "conclusion": "success", "labels": ["ubuntu-latest"]}
EARLY_FAIL = {"id": 2, "name": "early", "runner_name": "n5pro-ct102", "steps": [],
              "conclusion": "failure", "labels": ["self-hosted", "n5pro"]}
VOID = {"id": 3, "name": "void", "runner_name": "", "steps": [], "conclusion": "failure",
        "labels": ["ubuntu-latest"]}
CANCELLED = {"id": 4, "name": "cancelled", "runner_name": "", "steps": [], "conclusion": "cancelled",
             "labels": ["ubuntu-latest"]}
QUEUED = {"id": 5, "name": "queued", "runner_name": "", "steps": [], "conclusion": None,
          "labels": ["ubuntu-latest"]}
# `runner_name` comes back as null, not "", on a job that never started.
SKIPPED = {"id": 6, "name": "skipped", "runner_name": None, "steps": [], "conclusion": "skipped",
           "labels": ["ubuntu-latest"]}
VOID_SELF_HOSTED = {"id": 7, "name": "void-selfhosted", "runner_name": None, "steps": [],
                    "conclusion": "failure", "labels": ["self-hosted", "n5pro"]}


class Classification(unittest.TestCase):
    def test_a_real_job_is_not_void(self):
        self.assertFalse(void.is_void(REAL))

    def test_a_named_runner_with_zero_steps_is_a_real_early_failure(self):
        """The cry-wolf case. A job that failed in setup has zero steps but a
        runner, and flagging it would make the detector noisy enough to ignore —
        which is how a gate joins the taxonomy it was written for."""
        self.assertFalse(void.is_void(EARLY_FAIL))

    def test_empty_runner_and_zero_steps_is_void(self):
        self.assertTrue(void.is_void(VOID))

    def test_self_test_has_no_failures(self):
        self.assertEqual(void.self_test(), [])

    def test_null_runner_name_counts_as_unassigned(self):
        self.assertTrue(void.is_void(SKIPPED))

    def test_labels_survive_on_a_job_that_never_started(self):
        """The exemption key depends on this fact about someone else's API.

        CAPTURED VERBATIM from a real billing-VOID job rather than invented, so
        this pins the API's actual shape instead of a fixture written to agree
        with me. `gh api repos/wrfcoin/core4/actions/jobs/99477797301`, 2026-08-31:

            {"conclusion": "failure", "id": 99477797301,
             "labels": ["ubuntu-latest"], "name": "determinism-lint",
             "runner_name": "", "steps": 0}

        Measured over 52 consecutive never-started jobs on main: 52 carried a
        non-empty labels list, 0 empty, 0 missing the key.
        """
        real_billing_void = {
            "conclusion": "failure",
            "id": 99477797301,
            "labels": ["ubuntu-latest"],
            "name": "determinism-lint",
            "runner_name": "",
            "steps": [],
        }
        self.assertTrue(void.is_void(real_billing_void))
        self.assertEqual(void.runner_class(real_billing_void), "hosted")

        self.assertTrue(void.is_hosted(VOID))
        self.assertFalse(void.is_hosted(VOID_SELF_HOSTED))
        self.assertFalse(void.is_hosted(REAL))

    def test_an_undeterminable_runner_class_is_never_exempt(self):
        """The exemption rests entirely on labels being present. If the API ever
        stops supplying them, the detector must cry wolf rather than silently
        exempt a job it could not classify — wolf is recoverable, a silent
        exemption is the shape this whole issue is about."""
        for job in (
            {"id": 8, "runner_name": "", "steps": [], "conclusion": "failure", "labels": []},
            {"id": 9, "runner_name": "", "steps": [], "conclusion": "failure"},
        ):
            self.assertEqual(void.runner_class(job), "unknown")
            self.assertFalse(void.is_hosted(job), "unknown class must not be exempt")


class Attribution(unittest.TestCase):
    """`steps=0` has causes with DIFFERENT FIXES. Conflating them sends someone to
    restore billing when the answer is a runner outage — the exact attribution
    failure #2923 is about."""

    def attribute_with(self, annotations, job):
        original = void.gh_json
        try:
            void.gh_json = lambda path: annotations
            return void.attribute("owner/repo", job)
        finally:
            void.gh_json = original

    def test_billing_annotation_is_attributed_to_billing(self):
        note = [{"message": "The job was not started because recent account payments have failed"}]
        self.assertEqual(self.attribute_with(note, VOID)[0], void.BILLING)

    def test_runner_annotation_is_attributed_to_capacity_not_billing(self):
        note = [{"message": "The job was not acquired by Runner of type hosted even after multiple attempts"}]
        self.assertEqual(self.attribute_with(note, VOID)[0], void.RUNNER_UNAVAILABLE)

    def test_cancelled_with_no_annotation_is_superseded(self):
        self.assertEqual(self.attribute_with([], CANCELLED)[0], void.SUPERSEDED)

    def test_skipped_is_not_reported_as_an_unexplained_void(self):
        """A skipped job matches the VOID shape exactly — no runner, no steps —
        but a filter excluded it, which is a different defect with a different
        fix. The first version of this detector reported exactly one finding
        against main, and it was a skipped job labelled UNATTRIBUTED."""
        self.assertEqual(self.attribute_with([], SKIPPED)[0], void.SKIPPED)

    def test_no_annotation_is_unattributed_never_guessed(self):
        """On 2026-08-06 this signature was misattributed to billing on pattern
        match alone; the annotation said the runner had not been acquired and the
        budget was intact. Unknown must stay unknown."""
        self.assertEqual(self.attribute_with([], VOID)[0], void.UNATTRIBUTED)


class Baseline(unittest.TestCase):
    def write(self, payload):
        tmp = Path(tempfile.mkdtemp()) / "baseline.json"
        tmp.write_text(json.dumps(payload), encoding="utf-8")
        return str(tmp)

    WELL_FORMED = {
        "active": True,
        "issue": "#1",
        "why": "example",
        "review_by": "2099-01-01",
        "clears_when": "the example condition holds",
    }

    def exemption(self, **overrides):
        entry = dict(self.WELL_FORMED)
        entry.update(overrides)
        return {"exemptions": {"example": entry}}

    def test_a_well_formed_exemption_loads(self):
        path = self.write(self.exemption())
        data, failure = void.load_baseline(path)
        self.assertIsNone(failure)
        self.assertTrue(data["exemptions"]["example"]["active"])

    def test_a_baseline_without_the_exemptions_key_is_a_failure(self):
        path = self.write({"something_else": 1})
        _, failure = void.load_baseline(path)
        self.assertIsNotNone(failure)

    def test_an_exemption_without_a_review_date_is_REFUSED(self):
        """#2921 story 3, the whole point. An exemption with no stated end is how
        `hosted_void_expected` outlived its cause. Refusing at load is what makes
        a future stale exemption impossible rather than merely noticed — and it
        must be a refusal, not a skipped entry, because silently ignoring a
        malformed exemption un-gates exactly the jobs it covers."""
        for missing in ("review_by", "clears_when", "issue"):
            with self.subTest(missing=missing):
                path = self.write(self.exemption(**{missing: None}))
                _, failure = void.load_baseline(path)
                self.assertIsNotNone(failure, f"missing {missing} must be refused")
                self.assertIn(missing, failure)

    def test_an_unparseable_review_date_is_refused_not_treated_as_no_expiry(self):
        """The dangerous coercion: a date the loader cannot read must not become
        'no expiry', which would restore the defect under a typo."""
        path = self.write(self.exemption(review_by="whenever"))
        _, failure = void.load_baseline(path)
        self.assertIsNotNone(failure)
        self.assertIn("ISO date", failure)

    def test_an_expired_exemption_STOPS_APPLYING_and_is_reported(self):
        """Both halves, because either alone is the bug. Reporting without
        stopping leaves a stale exemption suppressing jobs while something
        complains elsewhere; stopping without reporting hides why coverage
        changed."""
        path = self.write(self.exemption(review_by="2020-01-01"))
        data, failure = void.load_baseline(path)
        self.assertIsNone(failure)
        today = date(2026, 9, 1)
        self.assertEqual(void.expired_exemptions(data, today), ["example"])
        self.assertFalse(
            void.active_exemption(data, "example", today),
            "an expired exemption must stop suppressing the jobs it covered",
        )

    def test_an_unexpired_exemption_still_applies(self):
        """The control. Without it this suite would pass with an implementation
        that simply disabled every exemption."""
        path = self.write(self.exemption(review_by="2099-01-01"))
        data, _ = void.load_baseline(path)
        today = date(2026, 9, 1)
        self.assertEqual(void.expired_exemptions(data, today), [])
        self.assertTrue(void.active_exemption(data, "example", today))

    def test_an_inactive_exemption_does_not_apply_and_is_not_reported_expired(self):
        path = self.write(self.exemption(active=False, review_by="2020-01-01"))
        data, _ = void.load_baseline(path)
        today = date(2026, 9, 1)
        self.assertFalse(void.active_exemption(data, "example", today))
        self.assertEqual(void.expired_exemptions(data, today), [])

    def test_the_baseline_names_no_workflows(self):
        """BLOCKER 3 regression, structural. Two of the ten workflow names in the
        first version did not match GitHub's, so their known-VOID jobs would have
        been reported as regressions. There are no names to mistype now, and this
        asserts the shipped baseline keeps it that way."""
        shipped = json.loads(Path(__file__).with_name("void_gates_baseline.json").read_text())
        self.assertNotIn("known_void", shipped)
        self.assertIn("exemptions", shipped)

    def test_the_shipped_baseline_has_retired_the_hosted_exemption(self):
        """#2921 story 1. Hosted execution came back, the exemption covered 0
        jobs, and the detector reported STALE EXEMPTION. Pinned so a re-add is a
        deliberate act with a review date rather than a silent restoration."""
        shipped = json.loads(Path(__file__).with_name("void_gates_baseline.json").read_text())
        self.assertNotIn("hosted_void_expected", shipped.get("exemptions", {}))
        self.assertIn("hosted_void_expected", shipped.get("$retired", {}))

    def test_every_shipped_exemption_carries_an_expiry(self):
        """Structural, against the real file: whatever is added later cannot skip
        the fields, because the shipped baseline is loaded through the same
        validation CI uses."""
        path = str(Path(__file__).with_name("void_gates_baseline.json"))
        _, failure = void.load_baseline(path)
        self.assertIsNone(failure, f"the shipped baseline must load: {failure}")

    def test_a_missing_baseline_is_a_failure_not_an_empty_set(self):
        """Otherwise a deleted baseline silently turns every VOID job into an
        expected one."""
        _, failure = void.load_baseline("/nonexistent/baseline.json")
        self.assertIsNotNone(failure)
        self.assertIn("refusing", failure)

    def test_malformed_baseline_is_a_failure(self):
        tmp = Path(tempfile.mkdtemp()) / "bad.json"
        tmp.write_text("{not json", encoding="utf-8")
        _, failure = void.load_baseline(str(tmp))
        self.assertIsNotNone(failure)


class EndToEnd(unittest.TestCase):
    """Drive void_jobs over synthetic runs, so the mutation is a real zero-step job."""

    def run_with(self, jobs, annotations=None, jobs_readable=True, annotations_readable=True):
        original = void.gh_json
        try:
            def fake(path):
                if "/jobs" in path:
                    if not jobs_readable:
                        raise void.ApiUnreadable(path)
                    return {"jobs": jobs}
                if "/annotations" in path:
                    if not annotations_readable:
                        raise void.ApiUnreadable(path)
                    return annotations if annotations is not None else []
                raise void.ApiUnreadable(path)
            void.gh_json = fake
            return void.void_jobs("owner/repo", [{"id": 9, "name": "Example Workflow"}])
        finally:
            void.gh_json = original

    def test_a_synthetic_zero_step_job_is_detected(self):
        """THE MUTATION: inject a job with no runner and no steps; the detector
        must find it. A detector that found nothing here would pass every real
        run for the wrong reason."""
        found, inspected, _ = self.run_with([VOID])
        self.assertEqual(len(found), 1, found)
        self.assertEqual(found[0]["workflow"], "Example Workflow")
        self.assertEqual(found[0]["cause"], void.UNATTRIBUTED)
        self.assertEqual(inspected, 1)

    def test_a_healthy_run_produces_no_findings(self):
        """The converse. Without this, a detector that flagged everything would
        pass the mutation case above."""
        self.assertEqual(self.run_with([REAL])[0], [])

    def test_a_job_still_running_is_not_reported(self):
        self.assertEqual(self.run_with([QUEUED])[0], [])

    def test_billing_and_capacity_are_reported_separately(self):
        billing = [{"message": "recent account payments have failed"}]
        found, _, _ = self.run_with([VOID], billing)
        self.assertEqual(found[0]["cause"], void.BILLING)

    def test_an_unreadable_jobs_payload_raises_rather_than_returning_empty(self):
        """ROUND 1 blocker, now closed as a CLASS. A failed read is no longer a
        value any caller can coerce — it raises, so a new call site inherits the
        behaviour instead of depending on its author remembering."""
        with self.assertRaises(void.ApiUnreadable):
            self.run_with([VOID], jobs_readable=False)

    def test_an_unreadable_annotations_payload_raises_too(self):
        """ROUND 2 blocker 3. This call site independently coerced a failed read
        to [], making 'the API did not answer' indistinguishable from 'the API
        answered with nothing' — and the second is what UNATTRIBUTED means."""
        with self.assertRaises(void.ApiUnreadable):
            self.run_with([VOID], annotations_readable=False)

    def test_a_genuinely_empty_annotation_list_still_yields_unattributed(self):
        """The converse the reviewer asked for: the class fix must not blind the
        TRUE-empty case. A real [] is data, and it means UNATTRIBUTED."""
        found, _, _ = self.run_with([VOID], annotations=[])
        self.assertEqual(found[0]["cause"], void.UNATTRIBUTED)

    def test_hosted_running_for_real_is_computed_in_the_same_pass(self):
        """ROUND 2 blocker 2. The stale check used to re-fetch the jobs and
        collapse a failed second read to empty. There is no second fetch now, so
        there is no second call site to get wrong."""
        _, _, ran = self.run_with([REAL_HOSTED])
        self.assertTrue(ran)
        _, _, ran_void_only = self.run_with([VOID])
        self.assertFalse(ran_void_only)

    def test_floors_reject_zero_and_negative(self):
        """BLOCKER 1. argparse's `int` is not a constraint, and this repo has had
        the identical bug twice — cargo_test_nonvacuous.py's --min-summaries
        accepted -1 until #2899. The validator is IMPORTED from that file rather
        than copied a third time."""
        import argparse as _argparse
        for flag in ("--min-runs", "--min-jobs", "--limit"):
            parse = void.floor(flag)
            self.assertEqual(parse("1"), 1)
            for bad in ("0", "-1", "-99"):
                with self.assertRaises(_argparse.ArgumentTypeError, msg=f"{flag} {bad}"):
                    parse(bad)

    def test_a_self_hosted_void_job_is_never_exempt(self):
        """BLOCKER 2 regression. The exemption is keyed on the runner CLASS, so a
        newly added job cannot inherit an exemption granted to a workflow name. A
        hosted job is explained by the systemic billing fact; a self-hosted one is
        not explained by anything and must always be reported."""
        found, _, _ = self.run_with([VOID_SELF_HOSTED])
        self.assertEqual(len(found), 1)
        self.assertFalse(found[0]["hosted"], "a self-hosted VOID job must not be marked hosted")

    def test_a_new_hosted_job_carries_its_own_class_not_an_inherited_name(self):
        found, _, _ = self.run_with([VOID, VOID_SELF_HOSTED])
        by_id = {f["id"]: f for f in found}
        self.assertTrue(by_id[3]["hosted"])
        self.assertFalse(by_id[7]["hosted"])


class NaiveCallSite(unittest.TestCase):
    """The class claim, measured at the PUBLIC HELPER BOUNDARY rather than argued.

    Round 3 claimed `gh_json` makes a failed read impossible to ignore. A reviewer
    tested that claim by writing the naive call site in the shape of the original
    bug — `items = gh_json(path) or []` — and running it against three input
    shapes. Transport failure and malformed JSON both raised. An rc-0 response
    whose body was literally `null` parsed to `None`, coerced to `[]`, and slipped
    through without anything being caught.

    So the claim is now a test rather than an argument, and it is written the way
    a future author would misuse the helper, not the way the helper is meant to be
    used. `json.loads("null") or []` is byte-identical to a genuine `[]` — the
    same "did not answer versus answered with nothing" confusion this whole issue
    is about, one level below where it was being applied.
    """

    def call_with(self, returncode: int, stdout: str):
        """Drive the real gh_json through a faked subprocess result."""
        original = void.subprocess.run

        class Result:
            pass

        try:
            def fake(*_a, **_k):
                r = Result()
                r.returncode = returncode
                r.stdout = stdout
                return r
            void.subprocess.run = fake
            # DELIBERATELY the naive shape a future author would write.
            return void.gh_json("repos/x/y/z") or []
        finally:
            void.subprocess.run = original

    def test_transport_failure_cannot_be_coerced(self):
        with self.assertRaises(void.ApiUnreadable):
            self.call_with(1, "")

    def test_malformed_json_cannot_be_coerced(self):
        with self.assertRaises(void.ApiUnreadable):
            self.call_with(0, "{not json")

    def test_an_rc0_null_body_cannot_be_coerced(self):
        """THE ROUND 3 GAP. rc 0, valid JSON, and no data — the one shape neither
        the class fix nor its author enumerated."""
        with self.assertRaises(void.ApiUnreadable):
            self.call_with(0, "null")

    def test_a_non_container_body_cannot_be_coerced(self):
        for body in ("5", '"a string"', "true"):
            with self.assertRaises(void.ApiUnreadable, msg=body):
                self.call_with(0, body)

    def test_a_genuinely_empty_array_is_still_valid_data(self):
        """The constraint the fix must not break: `[]` is a real answer. If this
        raised, UNATTRIBUTED would become unreachable and the fix would have
        traded one blindness for another."""
        self.assertEqual(self.call_with(0, "[]"), [])

    def test_a_genuinely_empty_object_is_still_valid_data(self):
        self.assertEqual(self.call_with(0, "{}"), [])  # `{} or []` is [], and that is fine

    def test_a_populated_response_passes_through(self):
        self.assertEqual(self.call_with(0, '{"jobs": [1]}'), {"jobs": [1]})


if __name__ == "__main__":
    unittest.main(verbosity=2)
