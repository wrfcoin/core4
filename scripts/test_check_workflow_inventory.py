#!/usr/bin/env python3
"""Hermetic regression controls for the #2890 workflow-inventory checker.

These drive the SHIPPED classifier and the SHIPPED CLI, not a re-typed copy of either
(the #2602 rule). Three of these cases are regressions for defects this checker had in
its first draft and that only running it against the real tree exposed — they are kept as
tests rather than as prose.
"""
import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check_workflow_inventory.py")
SPEC = importlib.util.spec_from_file_location("check_workflow_inventory", SCRIPT)
assert SPEC and SPEC.loader
GATE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = GATE
SPEC.loader.exec_module(GATE)

WORKFLOWS = SCRIPT.parents[1] / ".github/workflows"
RED_FIXTURE = SCRIPT.parent / "fixtures/workflow_inventory_red"


def codes(text):
    return {f.code for f in GATE.check_workflow(Path("f.yml"), text)[1]}


class TriggerClassification(unittest.TestCase):
    def test_on_key_is_not_eaten_by_yaml_11_boolean_resolution(self):
        # REGRESSION. PyYAML resolves a bare `on:` key to True. Without the patched
        # loader every workflow classifies as NONE and the checker is uniformly,
        # confidently wrong about the entire directory.
        document = GATE.yaml.load(GATE.AUTOMATIC_GOOD, Loader=GATE.WorkflowLoader)
        self.assertEqual(GATE.triggers(document), {"pull_request"})
        self.assertEqual(GATE.classify(GATE.triggers(document)), GATE.AUTOMATIC)

    def test_trigger_list_and_string_forms(self):
        self.assertEqual(GATE.classify(GATE.triggers({"on": "push"})), GATE.AUTOMATIC)
        self.assertEqual(GATE.classify(GATE.triggers({"on": ["push", "fork"]})), GATE.AUTOMATIC)
        self.assertEqual(GATE.classify(GATE.triggers({"on": {"workflow_dispatch": None}})), GATE.MANUAL)
        self.assertEqual(GATE.classify(GATE.triggers({})), GATE.NONE)

    def test_dispatch_plus_an_automatic_trigger_is_automatic(self):
        # The common shape: dispatchable AND gating. It must not be asked to declare
        # itself as manual.
        self.assertEqual(
            GATE.classify(GATE.triggers({"on": {"pull_request": None, "workflow_dispatch": None}})),
            GATE.AUTOMATIC,
        )


class DeclarationRules(unittest.TestCase):
    def test_undeclared_dispatch_only_is_a_finding(self):
        self.assertEqual(codes(GATE.MANUAL_UNDECLARED), {"UNDECLARED_MANUAL"})

    def test_a_fully_declared_tool_is_clean(self):
        self.assertEqual(codes(GATE.MANUAL_GOOD), set())

    def test_a_fully_declared_parked_gate_is_clean(self):
        self.assertEqual(codes(GATE.PARKED_GOOD), set())

    def test_preamble_declaration_without_a_name_marker_is_a_finding(self):
        # The name is the only thing visible from the Actions sidebar, which is where
        # the misreading in #2890 happened.
        self.assertEqual(codes(GATE.MANUAL_NO_NAME), {"UNDECLARED_MANUAL"})

    def test_name_marker_without_a_reason_is_a_finding(self):
        self.assertEqual(codes(GATE.MANUAL_NO_PREAMBLE), {"UNDECLARED_MANUAL"})

    def test_declaration_below_jobs_does_not_count(self):
        self.assertEqual(codes(GATE.MARKER_BELOW_JOBS), {"UNDECLARED_MANUAL"})

    def test_parked_must_name_what_restores_it(self):
        self.assertEqual(codes(GATE.PARKED_NO_RESTORE), {"PARKED_WITHOUT_RESTORE"})

    def test_restore_pointer_is_searched_across_the_whole_declaration_block(self):
        # REGRESSION. Searching only the marker LINE made every correctly-declared
        # parked workflow report PARKED_WITHOUT_RESTORE — the rule firing on its own
        # formatting rather than on the fact it checks.
        multiline = (
            "name: Scan (parked)\n"
            "# NO-AUTOMATIC-TRIGGER: parked — schedule commented out for the budget.\n"
            "# Register: docs/ci/parked-workflows.txt\n"
            "on:\n  workflow_dispatch: {}\n"
            "jobs:\n  a: {runs-on: ubuntu-latest, steps: [{run: \"true\"}]}\n"
        )
        self.assertEqual(codes(multiline), set())

    def test_a_marker_with_no_reason_is_a_finding_for_both_kinds(self):
        # #2942 review, blocking finding 1. The marker text was captured as
        # `(?P<why>.*)`, which matches the empty string, so a bare
        # `# NO-AUTOMATIC-TRIGGER: manual` was accepted — contradicting Rule 1's
        # documented `<why>` and the checker's own "a label without a reason" message.
        self.assertEqual(codes(GATE.MANUAL_NO_REASON), {"DECLARATION_WITHOUT_REASON"})
        self.assertEqual(
            codes(GATE.PARKED_NO_REASON),
            {"DECLARATION_WITHOUT_REASON", "PARKED_WITHOUT_RESTORE"},
        )

    def test_a_restore_pointer_cannot_substitute_for_the_explanation(self):
        # The narrower half of the same finding: an issue number or the register says
        # WHERE the work is tracked, never WHY the trigger is off. Both pointer forms,
        # because removing the matched substring alone used to leave `docs/ci/.txt`
        # behind — non-empty residue reading as prose.
        self.assertEqual(codes(GATE.PARKED_POINTER_ONLY), {"DECLARATION_WITHOUT_REASON"})
        self.assertEqual(codes(GATE.PARKED_ISSUE_ONLY), {"DECLARATION_WITHOUT_REASON"})
        # ...and a real reason that also carries a pointer stays clean.
        self.assertEqual(codes(GATE.PARKED_GOOD), set())

    def test_declaration_reason_separates_prose_from_pointers(self):
        reason, explanation = GATE.declaration_reason(
            "parked", "# NO-AUTOMATIC-TRIGGER: parked — budget cut (#2695)"
        )
        self.assertIn("budget cut", reason)
        self.assertIn("budget cut", explanation)
        self.assertNotIn("#2695", explanation)
        _r, only_pointer = GATE.declaration_reason(
            "parked", "# NO-AUTOMATIC-TRIGGER: parked — #2695"
        )
        self.assertEqual(only_pointer, "")

    def test_claiming_both_kinds_is_a_finding(self):
        self.assertEqual(codes(GATE.BOTH_KINDS), {"AMBIGUOUS_DECLARATION"})

    def test_a_live_gate_must_not_carry_a_declaration(self):
        self.assertEqual(codes(GATE.AUTOMATIC_STALE_NAME), {"STALE_DECLARATION"})
        self.assertEqual(codes(GATE.AUTOMATIC_STALE_PREAMBLE), {"STALE_DECLARATION"})

    def test_prose_about_a_parked_SUB_trigger_is_not_a_declaration(self):
        # REGRESSION. The first draft keyed on the bare word "PARKED" and flagged seven
        # real workflows that have a live `pull_request` and a parked weekly `schedule`
        # — a legitimate and common state here. The whole-workflow claim needs its own
        # token, or the rule fires on prose.
        prose = (
            "name: Real Gate\n"
            "# The weekly Tue 04:00 schedule is PARKED (#2652); the PR leg is untouched.\n"
            "# MANUAL-ONLY dispatch is also available for operators.\n"
            "on:\n  pull_request:\n    paths: ['src/**']\n"
            "jobs:\n  a: {runs-on: ubuntu-latest, steps: [{run: \"true\"}]}\n"
        )
        self.assertEqual(codes(prose), set())

    def test_no_trigger_at_all_is_a_finding(self):
        self.assertEqual(codes(GATE.NO_TRIGGER), {"NO_TRIGGER"})

    def test_declared_gate_must_evaluate_main(self):
        text = (
            "name: Example gate\n"
            "# WORKFLOW-GATE: protects the default branch\n"
            "on:\n  pull_request:\n"
            "jobs:\n  a: {runs-on: ubuntu-latest, steps: [{run: 'true'}]}\n"
        )
        self.assertEqual(codes(text), {"GATE_WITHOUT_MAIN_TRIGGER"})

    def test_declared_gate_with_main_push_is_clean(self):
        text = (
            "name: Example gate\n"
            "# WORKFLOW-GATE: protects the default branch\n"
            "on:\n  pull_request:\n  push:\n    branches: [main]\n"
            "jobs:\n  a: {runs-on: ubuntu-latest, steps: [{run: 'true'}]}\n"
        )
        self.assertEqual(codes(text), set())

    def test_gate_declaration_must_explain_itself(self):
        text = (
            "name: Example gate\n"
            "# WORKFLOW-GATE:\n"
            "on:\n  push:\n    branches: [main]\n"
            "jobs:\n  a: {runs-on: ubuntu-latest, steps: [{run: 'true'}]}\n"
        )
        self.assertEqual(codes(text), {"GATE_DECLARATION_WITHOUT_REASON"})


class FailClosed(unittest.TestCase):
    def test_unparseable_yaml_is_gate_broken_not_a_finding(self):
        _class, findings, broken = GATE.check_workflow(Path("f.yml"), GATE.UNPARSEABLE)
        self.assertEqual(findings, [])
        self.assertTrue(any(f.code == "UNPARSEABLE" for f in broken))

    def test_an_empty_directory_refuses_rather_than_reporting_success(self):
        with tempfile.TemporaryDirectory() as empty:
            self.assertTrue(GATE.take_inventory(Path(empty)).broken)


class RegistrationClasses(unittest.TestCase):
    """The four classes exist because 'delete the ones with no file' is wrong for three."""

    def test_a_file_on_this_tree_is_live(self):
        self.assertEqual(GATE.classify_registration(".github/workflows/x.yml", True, []), GATE.LIVE)

    def test_dependabot_dynamic_workflows_are_github_managed(self):
        # These are not files and are still running. Never "clean up".
        self.assertEqual(
            GATE.classify_registration("dynamic/dependabot/dependabot-updates", False, []),
            GATE.GITHUB_MANAGED,
        )

    def test_a_file_on_an_unmerged_branch_is_branch_only_not_a_ghost(self):
        self.assertEqual(
            GATE.classify_registration(".github/workflows/x.yml", False, ["origin/feature"]),
            GATE.BRANCH_ONLY,
        )

    def test_no_file_anywhere_is_a_ghost(self):
        self.assertEqual(GATE.classify_registration(".github/workflows/x.yml", False, []), GATE.GHOST)


class ShippedTree(unittest.TestCase):
    def test_this_repo_passes_its_own_rule(self):
        inventory = GATE.take_inventory(WORKFLOWS)
        self.assertEqual(inventory.broken, [], [str(f) for f in inventory.broken])
        self.assertEqual(inventory.findings, [], [str(f) for f in inventory.findings])

    def test_every_declared_manual_workflow_names_a_kind(self):
        for name, kind in GATE.take_inventory(WORKFLOWS).manual:
            self.assertIn(kind, ("manual", "parked"), name)


class ShippedTreeTriggerProse(unittest.TestCase):
    """#2942 review, blocking finding 2: prose that contradicts the YAML beneath it."""

    def test_ci_readiness_does_not_claim_pull_request_is_untouched(self):
        # The file carried two accounts of its own triggers: the added declaration said
        # both automatic triggers are suppressed, while a retained banner said only
        # `schedule` was parked and "PR gating is unchanged". The YAML has only
        # `workflow_dispatch`. In a PR about making coverage legible, leaving both
        # stories in the flagship workflow keeps it misleading.
        path = WORKFLOWS / "ci-readiness.yml"
        text = path.read_text(encoding="utf-8")
        document = GATE.yaml.load(text, Loader=GATE.WorkflowLoader)
        self.assertEqual(
            GATE.classify(GATE.triggers(document)), GATE.MANUAL,
            "this test is anchored on ci-readiness being dispatch-only; if a trigger "
            "was restored, delete this test along with the parked declaration",
        )
        # The claim is only false while the workflow is dispatch-only, which the
        # assertion above pins — so this cannot become a stale prohibition.
        flat = " ".join(text.split())
        for claim in (
            "PR gating is unchanged",
            "`pull_request` and `workflow_dispatch` are untouched",
        ):
            self.assertNotIn(
                claim, flat,
                f"ci-readiness.yml still claims {claim!r} while only "
                "workflow_dispatch is live",
            )


class Cli(unittest.TestCase):
    def _run(self, *args):
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args], capture_output=True, text=True
        )

    def test_self_test_exits_zero(self):
        done = self._run("--self-test")
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertIn("RED reachable for every finding code", done.stdout)

    def test_the_red_fixture_makes_the_shipped_cli_exit_one(self):
        # The load-bearing control: the classifier being right is not the same as
        # main() returning nonzero, and that gap is how a gate ships unable to fail.
        done = self._run("--workflows-dir", str(RED_FIXTURE))
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        self.assertIn("UNDECLARED_MANUAL", done.stdout)

    def test_the_real_workflow_directory_exits_zero(self):
        done = self._run()
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)

    def test_an_empty_directory_exits_two_not_zero(self):
        with tempfile.TemporaryDirectory() as empty:
            done = self._run("--workflows-dir", empty)
            self.assertEqual(done.returncode, 2, done.stdout + done.stderr)


if __name__ == "__main__":
    unittest.main()
