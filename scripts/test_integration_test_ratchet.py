#!/usr/bin/env python3
"""Regression tests for the #2490 integration-target ratchet."""

from __future__ import annotations

import importlib.util
import io
import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("integration_test_ratchet.py")
SPEC = importlib.util.spec_from_file_location("integration_test_ratchet", SCRIPT)
assert SPEC and SPEC.loader
RATCHET = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = RATCHET
SPEC.loader.exec_module(RATCHET)


class IntegrationRatchetTests(unittest.TestCase):
    def manifest(self):
        return {
            "schema": 1,
            "inventory": [{"package": "pkg", "target": "target"}],
            "external_coverage": [],
            "known_failures": [
                {
                    "package": "pkg",
                    "target": "target",
                    "test": "broken",
                    "pattern": "expected diagnostic",
                    "issue": "#1",
                }
            ],
            "docker_gated": [],
            "zero_test_exceptions": [],
        }

    def observation(self, diagnostic="expected diagnostic", test_counts=None):
        return RATCHET.Observation(
            executed={("pkg", "target")},
            failures={("pkg", "target", "broken"): diagnostic},
            outcomes={("pkg", "target", "broken"): "FAILED"},
            unknown_executables=[],
            test_counts=test_counts or {("pkg", "target"): (6, 1, 0)},
        )

    def test_exact_known_failure_is_green(self):
        errors = RATCHET.evaluate(
            self.manifest(), {("pkg", "target")}, self.observation(), 101
        )
        self.assertEqual(errors, [])

    def test_unlisted_failure_is_red(self):
        observation = self.observation()
        observation.failures[("pkg", "target", "new_failure")] = "boom"
        errors = RATCHET.evaluate(
            self.manifest(), {("pkg", "target")}, observation, 101
        )
        self.assertTrue(any("unexpected integration failures" in e for e in errors))

    def test_unexpected_pass_is_red(self):
        observation = RATCHET.Observation(
            executed={("pkg", "target")},
            failures={},
            outcomes={("pkg", "target", "broken"): "ok"},
            unknown_executables=[],
            test_counts={("pkg", "target"): (7, 0, 0)},
        )
        errors = RATCHET.evaluate(
            self.manifest(), {("pkg", "target")}, observation, 0
        )
        self.assertTrue(any("known failures now pass" in e for e in errors))
        # #2902: a genuine pass must NOT be reported as possible non-execution.
        self.assertFalse(any("did not execute" in e for e in errors))

    def test_changed_failure_reason_is_red(self):
        errors = RATCHET.evaluate(
            self.manifest(),
            {("pkg", "target")},
            self.observation("different diagnostic"),
            101,
        )
        self.assertTrue(any("failure reason changed" in e for e in errors))

    def test_unexecuted_target_is_red(self):
        observation = RATCHET.Observation(
            executed=set(),
            failures={("pkg", "target", "broken"): "expected diagnostic"},
            outcomes={("pkg", "target", "broken"): "FAILED"},
            unknown_executables=[],
            test_counts={},
        )
        errors = RATCHET.evaluate(
            self.manifest(), {("pkg", "target")}, observation, 101
        )
        self.assertTrue(any("did not execute" in e for e in errors))

    def test_inventory_drift_is_red(self):
        errors = RATCHET.validate_manifest(
            self.manifest(),
            {("pkg", "target"), ("pkg", "new_target")},
            "",
        )
        self.assertTrue(any("unreviewed discovered targets" in e for e in errors))

    def test_valid_inventory_review_metadata_is_green(self):
        manifest = self.manifest()
        manifest["inventory"][0]["review"] = {
            "source_pr": 10,
            "owning_issue": 20,
            "covers": "The reviewed behavior.",
        }
        errors = RATCHET.validate_manifest(manifest, {("pkg", "target")}, "")
        self.assertEqual(errors, [])

    def test_malformed_inventory_review_metadata_is_red(self):
        invalid_reviews = [
            "PR #10",
            {"source_pr": 0, "owning_issue": 20, "covers": "coverage"},
            {"source_pr": 10, "owning_issue": True, "covers": "coverage"},
            {"source_pr": 10, "owning_issue": 20, "covers": "   "},
        ]
        for review in invalid_reviews:
            with self.subTest(review=review):
                manifest = self.manifest()
                manifest["inventory"][0]["review"] = review
                errors = RATCHET.validate_manifest(
                    manifest, {("pkg", "target")}, ""
                )
                self.assertTrue(
                    any("inventory review metadata" in error for error in errors)
                )

    def test_parser_maps_artifact_target_and_diagnostic(self):
        metadata = {
            "packages": [
                {
                    "id": "path+file:///repo/pkg#0.1.0",
                    "name": "pkg",
                    "targets": [],
                }
            ]
        }
        artifact = {
            "reason": "compiler-artifact",
            "package_id": "path+file:///repo/pkg#0.1.0",
            "target": {"kind": ["test"], "name": "target"},
            "executable": str(RATCHET.ROOT / "target/debug/deps/target-abc"),
        }
        lines = [
            json.dumps(artifact),
            "     Running tests/target.rs (target/debug/deps/target-abc)",
            "running 1 test",
            "test broken ... FAILED",
            "",
            "failures:",
            "",
            "---- broken stdout ----",
            "thread 'broken' panicked:",
            "expected diagnostic",
            "",
            "failures:",
            "    broken",
        ]
        observation = RATCHET.parse_observation(
            lines, metadata, {("pkg", "target")}
        )
        self.assertEqual(observation.executed, {("pkg", "target")})
        self.assertIn(
            "expected diagnostic",
            observation.failures[("pkg", "target", "broken")],
        )
        self.assertEqual(
            observation.outcomes[("pkg", "target", "broken")], "FAILED"
        )

    # -- #2902: "now passes" vs "did not execute" must be different states ----
    # `stale = expected - actual` is satisfied identically by a known-failing
    # test that genuinely passed and by one that never ran (#[ignore]d,
    # filtered, renamed) inside a target that still executed. The first means
    # "retire the exemption"; the second means "a guard's test vanished" and
    # must NOT instruct removal. These tests pin both directions separately
    # AND assert the two runs produce different verdicts on identical input
    # shapes — a single one-direction test is not acceptance for #2902.

    def _observation_from_libtest(self, body_lines):
        metadata = {
            "packages": [
                {"id": "path+file:///repo/pkg#0.1.0", "name": "pkg", "targets": []}
            ]
        }
        artifact = {
            "reason": "compiler-artifact",
            "package_id": "path+file:///repo/pkg#0.1.0",
            "target": {"kind": ["test"], "name": "target"},
            "executable": str(RATCHET.ROOT / "target/debug/deps/target-abc"),
        }
        lines = [
            json.dumps(artifact),
            "     Running tests/target.rs (target/debug/deps/target-abc)",
            "",
        ] + body_lines
        return RATCHET.parse_observation(lines, metadata, {("pkg", "target")})

    # -- #3276: a FAILED verdict split from its test name by child output ----
    # Verbatim CT102 output (job 107508405369, zk_key_persistence on main
    # ab1b1a493): the test re-execs its own binary with inherited stdout, so
    # libtest prints `test X ... `, the child's output, then a bare `FAILED`.
    # Before #3276 this real failure was attributed to nothing and the run
    # reported only the unrelated known #3131 red.

    INTERLEAVED_3276 = (
        Path(__file__).with_name("fixtures") / "ratchet_interleaved_child_output_3276.txt"
    )
    INTERLEAVED_TEST_3276 = "gated::corrupt_persisted_keys_are_restored_from_embedded_bundle"

    def _interleaved_observation_3276(self, drop_name_list=False):
        body = self.INTERLEAVED_3276.read_text().splitlines()
        if drop_name_list:
            # The same run with libtest's closing name list removed: only
            # the summary still says `1 failed`.
            at = max(i for i, line in enumerate(body) if line.strip() == "failures:")
            body = body[:at] + [line for line in body[at:] if line.startswith("test result:")]
        return self._observation_from_libtest(body)

    def test_interleaved_child_output_failure_is_attributed_and_unexpected_3276(self):
        observation = self._interleaved_observation_3276()
        key = ("pkg", "target", self.INTERLEAVED_TEST_3276)
        self.assertIn(key, observation.failures)
        self.assertIn("restored key file should remain readable", observation.failures[key])
        self.assertEqual(observation.test_counts[("pkg", "target")][1], 1)
        errors = RATCHET.evaluate(self.manifest(), {("pkg", "target")}, observation, 101)
        unexpected = [e for e in errors if e.startswith("unexpected integration failures")]
        self.assertEqual(len(unexpected), 1, errors)
        self.assertIn(self.INTERLEAVED_TEST_3276, unexpected[0])
        self.assertFalse(any("could not attribute" in e for e in errors), errors)
        # The split line itself never produced a bogus outcome.
        self.assertNotIn(("pkg", "target", "gated::zk_keys_persist_across_process_restart"), observation.failures)

    def test_unattributable_failed_count_fails_closed_3276(self):
        observation = self._interleaved_observation_3276(drop_name_list=True)
        self.assertEqual(observation.failures, {})
        errors = RATCHET.evaluate(self.manifest(), {("pkg", "target")}, observation, 101)
        attribution = [e for e in errors if "could not attribute" in e]
        self.assertEqual(len(attribution), 1, errors)
        self.assertIn("('pkg', 'target', 1, 0)", attribution[0])

    def test_known_failure_counts_attribute_exactly_no_false_positive_3276(self):
        observation = self._observation_from_libtest(
            [
                "running 2 tests",
                "test broken ... FAILED",
                "test other ... ok",
                "",
                "failures:",
                "",
                "---- broken stdout ----",
                "expected diagnostic",
                "",
                "",
                "failures:",
                "    broken",
                "",
                "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s",
            ]
        )
        errors = RATCHET.evaluate(self.manifest(), {("pkg", "target")}, observation, 101)
        self.assertEqual(errors, [])

    def test_acceptance_known_failure_now_passes_says_so_specifically(self):
        # Direction 1: the known-failing test genuinely ran and PASSED.
        observation = self._observation_from_libtest(
            [
                "running 2 tests",
                "test broken ... ok",
                "test other ... ok",
                "",
                "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s",
            ]
        )
        errors = RATCHET.evaluate(
            self.manifest(), {("pkg", "target")}, observation, 0
        )
        self.assertTrue(any("known failures now pass" in e for e in errors))
        self.assertTrue(any("remove/fix entries" in e for e in errors))
        self.assertFalse(any("did not execute" in e for e in errors))

    def test_acceptance_ignored_known_failure_says_did_not_execute(self):
        # Direction 2: the SAME test #[ignore]d — the target still executes
        # and its other tests pass, so neither missing_targets nor the #2898
        # zero-test floor fires. Before #2902 this was byte-identical to
        # direction 1. It must now say the test did not execute and must NOT
        # instruct entry removal.
        observation = self._observation_from_libtest(
            [
                "running 2 tests",
                "test broken ... ignored",
                "test other ... ok",
                "",
                "test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s",
            ]
        )
        errors = RATCHET.evaluate(
            self.manifest(), {("pkg", "target")}, observation, 0
        )
        self.assertTrue(
            any("known-failure tests did not execute" in e for e in errors)
        )
        self.assertFalse(any("now pass" in e for e in errors))
        self.assertFalse(any("remove/fix" in e for e in errors))

    def test_acceptance_filtered_out_known_failure_says_did_not_execute(self):
        # Direction 2 variant: the test is renamed/filtered out entirely — no
        # outcome line at all, target still executing.
        observation = self._observation_from_libtest(
            [
                "running 1 test",
                "test other ... ok",
                "",
                "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.01s",
            ]
        )
        errors = RATCHET.evaluate(
            self.manifest(), {("pkg", "target")}, observation, 0
        )
        self.assertTrue(
            any("known-failure tests did not execute" in e for e in errors)
        )
        self.assertFalse(any("now pass" in e for e in errors))

    def test_acceptance_two_directions_produce_different_errors(self):
        # The issue's A/B experiment, rerun against the fix: the two
        # observations that used to evaluate byte-identically must diverge.
        # Both runs keep a second passing test so the #2898 zero-test floor
        # stays quiet in BOTH — otherwise its target-level error would make
        # the lists differ even under the pre-#2902 joint handling and this
        # assertion would pass vacuously (checked by mutation: reverting
        # evaluate() to the joint message must fail this test).
        passed = self._observation_from_libtest(
            [
                "running 2 tests",
                "test broken ... ok",
                "test other ... ok",
                "",
                "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s",
            ]
        )
        skipped = self._observation_from_libtest(
            [
                "running 2 tests",
                "test broken ... ignored",
                "test other ... ok",
                "",
                "test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s",
            ]
        )
        errors_passed = RATCHET.evaluate(
            self.manifest(), {("pkg", "target")}, passed, 0
        )
        errors_skipped = RATCHET.evaluate(
            self.manifest(), {("pkg", "target")}, skipped, 0
        )
        self.assertNotEqual(errors_passed, errors_skipped)

    def test_unexecuted_target_owns_the_target_level_report(self):
        # Relation to the pre-existing missing_targets check: when the WHOLE
        # target did not run, that check already reports it, and the per-test
        # #2902 split must neither call the entry "now passes" nor repeat the
        # non-execution per test. The run stays RED via missing_targets.
        observation = RATCHET.Observation(
            executed=set(),
            failures={},
            outcomes={},
            unknown_executables=[],
            test_counts={},
        )
        errors = RATCHET.evaluate(
            self.manifest(), {("pkg", "target")}, observation, 101
        )
        self.assertTrue(
            any("selected targets did not execute" in e for e in errors)
        )
        self.assertFalse(any("now pass" in e for e in errors))
        self.assertFalse(
            any("known-failure tests did not execute" in e for e in errors)
        )

    def test_parser_records_all_outcomes(self):
        observation = self._observation_from_libtest(
            [
                "running 3 tests",
                "test a ... ok",
                "test b ... ignored, needs hardware",
                "test c ... FAILED",
                "",
                "test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.02s",
            ]
        )
        self.assertEqual(
            observation.outcomes,
            {
                ("pkg", "target", "a"): "ok",
                ("pkg", "target", "b"): "ignored, needs hardware",
                ("pkg", "target", "c"): "FAILED",
            },
        )

    # -- #2902 round 2: the two states must differ at the PROCESS level ------
    # Review of f5f249966 confirmed the data and messages split but found both
    # paths still mapped to exit 1 — the report improved, the hazard did not.
    # unexecuted_known_failures() is the single source both evaluate() and
    # main()'s exit mapping read, and these tests drive the REAL CLI (a fake
    # `cargo` on PATH feeding canned metadata + libtest output through the
    # unmodified script) and assert on returncode, so collapsing the two paths
    # back onto one exit code cannot pass.

    def test_unexecuted_helper_ignores_passed_and_failed(self):
        observation = self.observation()  # broken FAILED
        self.assertEqual(
            RATCHET.unexecuted_known_failures(
                self.manifest(), {("pkg", "target")}, observation
            ),
            [],
        )
        passed = RATCHET.Observation(
            executed={("pkg", "target")},
            failures={},
            outcomes={("pkg", "target", "broken"): "ok"},
            unknown_executables=[],
            test_counts={("pkg", "target"): (1, 0, 0)},
        )
        self.assertEqual(
            RATCHET.unexecuted_known_failures(
                self.manifest(), {("pkg", "target")}, passed
            ),
            [],
        )

    def test_unexecuted_helper_reports_vanished_test_in_executed_target(self):
        vanished = RATCHET.Observation(
            executed={("pkg", "target")},
            failures={},
            outcomes={("pkg", "target", "other"): "ok"},
            unknown_executables=[],
            test_counts={("pkg", "target"): (1, 0, 0)},
        )
        self.assertEqual(
            RATCHET.unexecuted_known_failures(
                self.manifest(), {("pkg", "target")}, vanished
            ),
            [("pkg", "target", "broken")],
        )

    def test_unexecuted_helper_excludes_unexecuted_targets(self):
        # Whole-target absence belongs to missing_targets (exit 1), not to
        # the per-test guard-vanished state (exit 3).
        observation = RATCHET.Observation(
            executed=set(),
            failures={},
            outcomes={},
            unknown_executables=[],
            test_counts={},
        )
        self.assertEqual(
            RATCHET.unexecuted_known_failures(
                self.manifest(), {("pkg", "target")}, observation
            ),
            [],
        )

    def _run_cli(self, test_phase_body, cargo_test_exit=0):
        """Run the real script as a subprocess against a fake `cargo`.

        The fake cargo answers `cargo metadata` with canned metadata and
        `cargo test` with `test_phase_body` (artifact JSON + Running line are
        prepended here), exiting `cargo_test_exit`. Everything else — arg
        parsing, manifest validation, parsing, evaluation, exit mapping — is
        the shipped code path.
        """
        tmp = tempfile.mkdtemp(prefix="ratchet-cli-")
        exe = os.path.join(tmp, "deps", "target-abc")
        metadata = {
            "packages": [
                {
                    "id": "path+file:///repo/pkg#0.1.0",
                    "name": "pkg",
                    "targets": [{"kind": ["test"], "name": "target"}],
                }
            ]
        }
        artifact = {
            "reason": "compiler-artifact",
            "package_id": "path+file:///repo/pkg#0.1.0",
            "target": {"kind": ["test"], "name": "target"},
            "executable": exe,
        }
        if test_phase_body is None:
            # Whole-target-missing shape: cargo compiled the artifact but
            # never printed a Running line for it.
            test_output = json.dumps(artifact) + "\n"
        else:
            test_output = (
                json.dumps(artifact)
                + f"\n     Running tests/target.rs ({exe})\n\n"
                + test_phase_body
            )
        manifest = self.manifest()
        paths = {}
        for name, content in (
            ("metadata.json", json.dumps(metadata)),
            ("test_output.txt", test_output),
            ("manifest.json", json.dumps(manifest)),
            ("workflow.yml", ""),
        ):
            paths[name] = os.path.join(tmp, name)
            with open(paths[name], "w", encoding="utf-8") as handle:
                handle.write(content)
        cargo = os.path.join(tmp, "cargo")
        with open(cargo, "w", encoding="utf-8") as handle:
            handle.write(
                "#!/usr/bin/env python3\n"
                "import os, sys\n"
                "if sys.argv[1] == 'metadata':\n"
                "    sys.stdout.write(open(os.environ['FAKE_CARGO_METADATA']).read())\n"
                "    sys.exit(0)\n"
                "sys.stdout.write(open(os.environ['FAKE_CARGO_TEST_OUTPUT']).read())\n"
                "sys.exit(int(os.environ['FAKE_CARGO_TEST_EXIT']))\n"
            )
        os.chmod(cargo, os.stat(cargo).st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
        env = os.environ.copy()
        env["PATH"] = tmp + os.pathsep + env["PATH"]
        env["FAKE_CARGO_METADATA"] = paths["metadata.json"]
        env["FAKE_CARGO_TEST_OUTPUT"] = paths["test_output.txt"]
        env["FAKE_CARGO_TEST_EXIT"] = str(cargo_test_exit)
        return subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--manifest",
                paths["manifest.json"],
                "--workflow",
                paths["workflow.yml"],
                "--timeout-seconds",
                "60",
            ],
            env=env,
            capture_output=True,
            text=True,
            timeout=120,
        )

    def test_cli_now_passes_exits_red(self):
        completed = self._run_cli(
            "running 2 tests\n"
            "test broken ... ok\n"
            "test other ... ok\n"
            "\n"
            "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
        )
        self.assertEqual(completed.returncode, RATCHET.EXIT_RED)
        self.assertIn("known failures now pass", completed.stderr)
        self.assertNotIn("did not execute", completed.stderr)
        self.assertIn("[passed]", completed.stdout)

    def test_cli_did_not_execute_exits_guard_not_executed(self):
        completed = self._run_cli(
            "running 2 tests\n"
            "test broken ... ignored\n"
            "test other ... ok\n"
            "\n"
            "test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
        )
        self.assertEqual(completed.returncode, RATCHET.EXIT_GUARD_NOT_EXECUTED)
        self.assertIn("known-failure tests did not execute", completed.stderr)
        self.assertNotIn("now pass", completed.stderr)
        self.assertIn("[did-not-run:ignored]", completed.stdout)

    def test_cli_exit_codes_differ_between_the_two_states(self):
        # The contract's acceptance, end to end: same manifest, same target,
        # same test — the exit codes must differ, and 3 must be the vanished-
        # guard state so exit 1's "remove the entry" playbook can never be
        # applied to it.
        passed = self._run_cli(
            "running 2 tests\n"
            "test broken ... ok\n"
            "test other ... ok\n"
            "\n"
            "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
        )
        vanished = self._run_cli(
            "running 2 tests\n"
            "test broken ... ignored\n"
            "test other ... ok\n"
            "\n"
            "test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
        )
        self.assertNotEqual(passed.returncode, vanished.returncode)
        self.assertEqual(
            (passed.returncode, vanished.returncode),
            (RATCHET.EXIT_RED, RATCHET.EXIT_GUARD_NOT_EXECUTED),
        )

    def test_cli_whole_target_missing_still_exits_red(self):
        # missing_targets owns whole-target absence, unchanged: generic exit 1,
        # not the per-test exit 3. No Running line is emitted for the target.
        completed = self._run_cli(None, cargo_test_exit=101)
        self.assertEqual(completed.returncode, RATCHET.EXIT_RED)
        self.assertIn("selected targets did not execute", completed.stderr)
        self.assertNotIn("known-failure tests did not execute", completed.stderr)

    # -- #2898 Blocker 3: the nonzero-test floor -----------------------------
    # A target compiled out by a missing feature flag still prints "Running
    # ..." and "test result: ok. 0 passed; 0 failed; 0 ignored" -- the exact
    # shape `executed` alone accepted as coverage before this fix. These
    # tests prove the floor catches that shape and that a reviewed exception
    # is the only way past it -- both directions, not just the happy path.

    def test_known_failure_outside_selected_is_not_flagged_stale(self):
        # Real bug, caught by actually running --docker-gated locally: a
        # manifest-wide known_failures entry for a target NOT in this
        # invocation's `selected` set must not be treated as "now passes or
        # did not execute" just because this invocation never ran it.
        manifest = self.manifest()
        manifest["inventory"].append({"package": "pkg", "target": "other"})
        manifest["known_failures"].append(
            {
                "package": "pkg",
                "target": "other",
                "test": "elsewhere",
                "pattern": "irrelevant",
            }
        )
        observation = self.observation()  # only covers ("pkg", "target")
        errors = RATCHET.evaluate(manifest, {("pkg", "target")}, observation, 101)
        self.assertEqual(errors, [])

    def test_zero_test_exception_outside_selected_is_not_flagged_stale(self):
        manifest = self.manifest()
        manifest["known_failures"] = []
        manifest["inventory"].append({"package": "pkg", "target": "other"})
        manifest["zero_test_exceptions"] = [
            {"package": "pkg", "target": "other", "reason": "empty by design"}
        ]
        observation = RATCHET.Observation(
            executed={("pkg", "target")},
            failures={},
            outcomes={},
            unknown_executables=[],
            test_counts={("pkg", "target"): (3, 0, 0)},
        )
        errors = RATCHET.evaluate(manifest, {("pkg", "target")}, observation, 0)
        self.assertEqual(errors, [])

    def test_all_ignored_target_is_red_same_as_genuinely_empty(self):
        # Real shape found running --docker-gated locally against the real
        # manifest: hsm_signer/yubikey_signer/cold_start_convergence have
        # real #[test] functions, all #[ignore]d pending hardware/live infra
        # this runner doesn't have -- "0 passed; 0 failed; N ignored", N>0.
        # The floor must catch this the same as a genuinely empty target
        # (0, 0, 0): 0 passed and 0 failed either way means nothing was
        # actually asserted here.
        observation = RATCHET.Observation(
            executed={("pkg", "target")},
            failures={},
            outcomes={},
            unknown_executables=[],
            test_counts={("pkg", "target"): (0, 0, 2)},
        )
        manifest = self.manifest()
        manifest["known_failures"] = []
        errors = RATCHET.evaluate(manifest, {("pkg", "target")}, observation, 0)
        self.assertTrue(
            any("asserted nothing" in e and "not a reviewed" in e for e in errors)
        )

    def test_zero_test_target_is_red(self):
        observation = RATCHET.Observation(
            executed={("pkg", "target")},
            failures={},
            outcomes={},
            unknown_executables=[],
            test_counts={("pkg", "target"): (0, 0, 0)},
        )
        manifest = self.manifest()
        manifest["known_failures"] = []
        errors = RATCHET.evaluate(manifest, {("pkg", "target")}, observation, 0)
        self.assertTrue(
            any("asserted nothing" in e and "not a reviewed" in e for e in errors)
        )

    def test_zero_test_target_with_reviewed_exception_is_green(self):
        observation = RATCHET.Observation(
            executed={("pkg", "target")},
            failures={},
            outcomes={},
            unknown_executables=[],
            test_counts={("pkg", "target"): (0, 0, 0)},
        )
        manifest = self.manifest()
        manifest["known_failures"] = []
        manifest["zero_test_exceptions"] = [
            {"package": "pkg", "target": "target", "reason": "deliberately empty"}
        ]
        errors = RATCHET.evaluate(manifest, {("pkg", "target")}, observation, 0)
        self.assertEqual(errors, [])

    def test_stale_zero_test_exception_is_red(self):
        # The target listed as a reviewed zero-test exception now executes
        # real tests -- shrink-only discipline requires removing the entry,
        # same as a stale known_failures entry.
        observation = RATCHET.Observation(
            executed={("pkg", "target")},
            failures={},
            outcomes={},
            unknown_executables=[],
            test_counts={("pkg", "target"): (4, 0, 0)},
        )
        manifest = self.manifest()
        manifest["known_failures"] = []
        manifest["zero_test_exceptions"] = [
            {"package": "pkg", "target": "target", "reason": "was deliberately empty"}
        ]
        errors = RATCHET.evaluate(manifest, {("pkg", "target")}, observation, 0)
        self.assertTrue(any("no longer test-free" in e for e in errors))

    def test_docker_gated_and_zero_test_exception_is_mutually_exclusive(self):
        manifest = self.manifest()
        manifest["docker_gated"] = [
            {"package": "pkg", "target": "target", "reason": "needs docker"}
        ]
        manifest["zero_test_exceptions"] = [
            {"package": "pkg", "target": "target", "reason": "also empty"}
        ]
        errors = RATCHET.validate_manifest(manifest, {("pkg", "target")}, "")
        self.assertTrue(any("pick one" in e for e in errors))

    def test_docker_gated_entry_without_reason_is_red(self):
        manifest = self.manifest()
        manifest["docker_gated"] = [{"package": "pkg", "target": "target"}]
        errors = RATCHET.validate_manifest(manifest, {("pkg", "target")}, "")
        self.assertTrue(any("has no reason" in e for e in errors))

    def test_selected_targets_excludes_docker_gated(self):
        manifest = self.manifest()
        manifest["inventory"] = [
            {"package": "pkg", "target": "a"},
            {"package": "pkg", "target": "b"},
        ]
        manifest["known_failures"] = []
        manifest["docker_gated"] = [
            {"package": "pkg", "target": "b", "reason": "needs docker"}
        ]
        self.assertEqual(RATCHET.selected_targets(manifest), [("pkg", "a")])
        self.assertEqual(RATCHET.docker_gated_targets(manifest), [("pkg", "b")])

    def test_parser_extracts_zero_test_counts(self):
        # Same shape as the real bug: a compiled-out target still prints a
        # "Running" line and a genuine "test result:" summary, just with
        # everything at zero.
        metadata = {
            "packages": [
                {"id": "path+file:///repo/pkg#0.1.0", "name": "pkg", "targets": []}
            ]
        }
        artifact = {
            "reason": "compiler-artifact",
            "package_id": "path+file:///repo/pkg#0.1.0",
            "target": {"kind": ["test"], "name": "target"},
            "executable": str(RATCHET.ROOT / "target/debug/deps/target-abc"),
        }
        lines = [
            json.dumps(artifact),
            "     Running tests/target.rs (target/debug/deps/target-abc)",
            "",
            "running 0 tests",
            "",
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s",
        ]
        observation = RATCHET.parse_observation(lines, metadata, {("pkg", "target")})
        self.assertEqual(observation.test_counts[("pkg", "target")], (0, 0, 0))

    def test_parser_extracts_nonzero_test_counts(self):
        metadata = {
            "packages": [
                {"id": "path+file:///repo/pkg#0.1.0", "name": "pkg", "targets": []}
            ]
        }
        artifact = {
            "reason": "compiler-artifact",
            "package_id": "path+file:///repo/pkg#0.1.0",
            "target": {"kind": ["test"], "name": "target"},
            "executable": str(RATCHET.ROOT / "target/debug/deps/target-abc"),
        }
        lines = [
            json.dumps(artifact),
            "     Running tests/target.rs (target/debug/deps/target-abc)",
            "",
            "running 3 tests",
            "test a ... ok",
            "test b ... ok",
            "test c ... FAILED",
            "",
            "test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s",
        ]
        observation = RATCHET.parse_observation(lines, metadata, {("pkg", "target")})
        self.assertEqual(observation.test_counts[("pkg", "target")], (2, 1, 0))

    # -- #2898 Blocker 1: the Docker capability preflight ---------------------

    def test_docker_available_false_when_binary_missing(self):
        original_run = RATCHET.subprocess.run

        def fake_run(command, **kwargs):
            if command[:1] == ["docker"]:
                raise FileNotFoundError("docker: not found")
            return original_run(command, **kwargs)

        RATCHET.subprocess.run = fake_run
        try:
            self.assertFalse(RATCHET.docker_available())
        finally:
            RATCHET.subprocess.run = original_run

    def test_docker_preflight_classifies_missing_binary(self):
        original_run = RATCHET.subprocess.run

        def fake_run(command, **kwargs):
            if command[:1] == ["docker"]:
                raise FileNotFoundError("docker: not found")
            return original_run(command, **kwargs)

        RATCHET.subprocess.run = fake_run
        try:
            self.assertEqual(
                RATCHET.docker_preflight(),
                RATCHET.DockerPreflight(
                    False, "DOCKER_CLI_MISSING", "docker executable was not found"
                ),
            )
        finally:
            RATCHET.subprocess.run = original_run

    def test_docker_available_false_on_nonzero_exit(self):
        import subprocess as _subprocess

        original_run = RATCHET.subprocess.run

        def fake_run(command, **kwargs):
            if command[:1] == ["docker"]:
                return _subprocess.CompletedProcess(command, returncode=1)
            return original_run(command, **kwargs)

        RATCHET.subprocess.run = fake_run
        try:
            self.assertFalse(RATCHET.docker_available())
        finally:
            RATCHET.subprocess.run = original_run

    def test_docker_preflight_classifies_daemon_failure_detail(self):
        import subprocess as _subprocess

        original_run = RATCHET.subprocess.run

        def fake_run(command, **kwargs):
            if command[:1] == ["docker"]:
                return _subprocess.CompletedProcess(
                    command,
                    returncode=1,
                    stdout="",
                    stderr="permission denied\nwhile connecting to daemon",
                )
            return original_run(command, **kwargs)

        RATCHET.subprocess.run = fake_run
        try:
            self.assertEqual(
                RATCHET.docker_preflight(),
                RATCHET.DockerPreflight(
                    False,
                    "DOCKER_DAEMON_UNAVAILABLE",
                    "permission denied while connecting to daemon",
                ),
            )
        finally:
            RATCHET.subprocess.run = original_run

    def test_docker_available_true_on_zero_exit(self):
        import subprocess as _subprocess

        original_run = RATCHET.subprocess.run

        def fake_run(command, **kwargs):
            if command[:1] == ["docker"]:
                return _subprocess.CompletedProcess(command, returncode=0)
            return original_run(command, **kwargs)

        RATCHET.subprocess.run = fake_run
        try:
            self.assertTrue(RATCHET.docker_available())
        finally:
            RATCHET.subprocess.run = original_run

    def test_main_distinguishes_preflight_refusal_from_target_failure(self):
        manifest = {
            "schema": 1,
            "inventory": [{"package": "pkg", "target": "target"}],
            "external_coverage": [],
            "known_failures": [],
            "docker_gated": [
                {"package": "pkg", "target": "target", "reason": "needs Docker"}
            ],
            "zero_test_exceptions": [],
        }
        metadata = {
            "packages": [
                {
                    "id": "pkg-id",
                    "name": "pkg",
                    "targets": [{"kind": ["test"], "name": "target"}],
                }
            ]
        }
        executable = str(RATCHET.ROOT / "target/debug/deps/target-abc")
        cargo_failure = [
            json.dumps(
                {
                    "reason": "compiler-artifact",
                    "package_id": "pkg-id",
                    "target": {"kind": ["test"], "name": "target"},
                    "executable": executable,
                }
            ),
            f"     Running tests/target.rs ({executable})",
            "running 1 test",
            "test broken ... FAILED",
            "",
            "---- broken stdout ----",
            "genuine target assertion failed",
            "",
            "failures:",
            "    broken",
            "",
            "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s",
        ]

        with tempfile.TemporaryDirectory() as directory:
            manifest_path = Path(directory) / "manifest.json"
            workflow_path = Path(directory) / "workflow.yml"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            workflow_path.write_text("", encoding="utf-8")

            def invoke(preflight, cargo_result):
                stdout = io.StringIO()
                stderr = io.StringIO()
                with (
                    patch.object(
                        RATCHET.sys,
                        "argv",
                        [
                            "integration_test_ratchet.py",
                            "--manifest",
                            str(manifest_path),
                            "--workflow",
                            str(workflow_path),
                            "--docker-gated",
                        ],
                    ),
                    patch.object(RATCHET, "cargo_metadata", return_value=metadata),
                    patch.object(RATCHET, "docker_preflight", return_value=preflight),
                    patch.object(RATCHET, "run_cargo", return_value=cargo_result) as run,
                    redirect_stdout(stdout),
                    redirect_stderr(stderr),
                ):
                    exit_code = RATCHET.main()
                return exit_code, stdout.getvalue(), stderr.getvalue(), run

            refused_code, _refused_stdout, refused_stderr, refused_run = invoke(
                RATCHET.DockerPreflight(
                    False, "DOCKER_DAEMON_UNAVAILABLE", "permission denied"
                ),
                (101, cargo_failure),
            )
            self.assertEqual(refused_code, RATCHET.EXIT_CAPABILITY_REFUSED)
            self.assertIn(
                "DOCKER_PREFLIGHT_REFUSED reason=DOCKER_DAEMON_UNAVAILABLE",
                refused_stderr,
            )
            self.assertIn("detail=permission denied", refused_stderr)
            refused_run.assert_not_called()

            failure_code, failure_stdout, failure_stderr, failure_run = invoke(
                RATCHET.DockerPreflight(True, "DOCKER_READY"),
                (101, cargo_failure),
            )
            self.assertEqual(failure_code, RATCHET.EXIT_RED)
            self.assertIn("DOCKER_PREFLIGHT_OK reason=DOCKER_READY", failure_stdout)
            self.assertIn("unexpected integration failures", failure_stderr)
            self.assertNotIn("DOCKER_PREFLIGHT_REFUSED", failure_stderr)
            failure_run.assert_called_once()


class ExternalCoverageMustFireTests(unittest.TestCase):
    """#2888: an external_coverage claim is only as good as the trigger of the
    workflow it points at. Each test isolates ONE rule so a single fixture
    cannot pass by tripping an earlier branch."""

    FIRING_WORKFLOW = "\n".join(
        [
            "name: covering",
            "on:",
            "  pull_request: {}",
            "  push:",
            "    branches: [main]",
            "jobs:",
            "  gate:",
            "    runs-on: [self-hosted, n5pro]",
            "    steps:",
            "      - name: run the target",
            "        id: covering_step",
            "        run: cargo test -p pkg --test target",
        ]
    )
    DISPATCH_ONLY_WORKFLOW = "\n".join(
        [
            "name: parked",
            "# pull_request: this comment claims a trigger the block below lacks",
            "on:",
            "  workflow_dispatch: {}",
            "jobs:",
            "  gate:",
            "    runs-on: [self-hosted, n5pro]",
            "    steps:",
            "      - name: run the target",
            "        id: covering_step",
            "        run: cargo test -p pkg --test target",
            "      - name: result",
            "        id: result",
            "        run: echo ${{ steps.covering_step.outcome }}",
            "      - name: summary",
            "        run: echo ${{ steps.covering_step.outcome }}",
        ]
    )

    def manifest(self, **entry):
        return {
            "schema": 1,
            "inventory": [{"package": "pkg", "target": "target"}],
            "external_coverage": [
                {"package": "pkg", "target": "target", "step": "covering_step", **entry}
            ],
            "known_failures": [],
            "docker_gated": [],
            "zero_test_exceptions": [],
        }

    def test_workflow_triggers_are_parsed_not_grepped(self):
        # The comment line names pull_request; the parsed block does not.
        self.assertEqual(
            RATCHET.workflow_triggers(self.DISPATCH_ONLY_WORKFLOW), ["workflow_dispatch"]
        )
        self.assertEqual(
            RATCHET.workflow_triggers(self.FIRING_WORKFLOW), ["pull_request", "push"]
        )
        self.assertEqual(RATCHET.workflow_triggers(""), [])

    def test_dispatch_only_default_workflow_is_red(self):
        errors = RATCHET.validate_manifest(
            self.manifest(), {("pkg", "target")}, self.DISPATCH_ONLY_WORKFLOW
        )
        self.assertTrue(
            any("does not fire on push/pull_request" in e for e in errors), errors
        )
        self.assertTrue(any("['workflow_dispatch'] only" in e for e in errors), errors)

    def test_named_firing_workflow_with_hard_step_is_green(self):
        workflows = {".github/workflows/covering.yml": self.FIRING_WORKFLOW}
        errors = RATCHET.validate_manifest(
            self.manifest(workflow=".github/workflows/covering.yml"),
            {("pkg", "target")},
            "",
            workflows,
        )
        self.assertEqual(errors, [])

    def test_named_workflow_missing_from_tree_is_red(self):
        errors = RATCHET.validate_manifest(
            self.manifest(workflow=".github/workflows/no-such.yml"),
            {("pkg", "target")},
            self.FIRING_WORKFLOW,
            {},
        )
        self.assertTrue(any("does not exist in the tree" in e for e in errors), errors)

    def test_firing_workflow_without_the_step_is_red(self):
        workflows = {".github/workflows/covering.yml": self.FIRING_WORKFLOW}
        errors = RATCHET.validate_manifest(
            self.manifest(workflow=".github/workflows/covering.yml", step="absent"),
            {("pkg", "target")},
            "",
            workflows,
        )
        self.assertTrue(any("has no workflow step id absent" in e for e in errors), errors)

    def test_firing_workflow_whose_step_is_soft_is_red(self):
        soft = self.FIRING_WORKFLOW.replace(
            "        run: cargo test -p pkg --test target",
            "        continue-on-error: true\n        run: cargo test -p pkg --test target",
        )
        workflows = {".github/workflows/covering.yml": soft}
        errors = RATCHET.validate_manifest(
            self.manifest(workflow=".github/workflows/covering.yml"),
            {("pkg", "target")},
            "",
            workflows,
        )
        self.assertTrue(any("reaches no verdict" in e for e in errors), errors)

    def test_soft_step_with_two_verdict_references_is_green(self):
        # The ci-readiness shape: a continue-on-error step whose outcome is
        # read by a result step and a summary step still reaches a verdict.
        summarised = self.FIRING_WORKFLOW.replace(
            "        run: cargo test -p pkg --test target",
            "        continue-on-error: true\n"
            "        run: cargo test -p pkg --test target\n"
            "      - name: result\n"
            "        run: echo ${{ steps.covering_step.outcome }}\n"
            "      - name: summary\n"
            "        run: echo ${{ steps.covering_step.outcome }}",
        )
        workflows = {".github/workflows/covering.yml": summarised}
        errors = RATCHET.validate_manifest(
            self.manifest(workflow=".github/workflows/covering.yml"),
            {("pkg", "target")},
            "",
            workflows,
        )
        self.assertEqual(errors, [])


if __name__ == "__main__":
    unittest.main()
