"""Offline tests for scorer_job (no network, no keys, nothing signed).

Run: python -m unittest discover -s tools/forecast-kit/tests -v
The end-to-end tests need the scorer binary (cargo build -p wrfcoin-forecast-score --bin forecast-score-batch);
point FORECAST_SCORER_BIN at it, or they look in $CARGO_TARGET_DIR/debug and skip when it is absent.
"""

import contextlib
import importlib.util
import io
import json
import os
import shutil
import stat
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
spec = importlib.util.spec_from_file_location("scorer_job", os.path.join(HERE, "..", "scorer_job.py"))
job = importlib.util.module_from_spec(spec)
spec.loader.exec_module(job)
import scorer_fixtures as fx  # noqa: E402


def scorer_bin():
    candidates = [os.environ.get("FORECAST_SCORER_BIN", "")]
    for root in (os.environ.get("CARGO_TARGET_DIR", ""), os.path.join(HERE, "..", "..", "..", "target")):
        if root:
            candidates.append(os.path.join(root, "debug", "forecast-score-batch"))
    return next((c for c in candidates if c and os.access(c, os.X_OK)), None)


BIN = scorer_bin()
ENABLED = {"FORECAST_SIGNER_ENABLED": "1"}


class Gates(unittest.TestCase):
    def test_switched_off_without_the_enable_flag(self):
        for env in ({}, {"FORECAST_SIGNER_ENABLED": "0", "SKILL_DEFINITION": "rmse_sigma"}, {"FORECAST_SIGNER_ENABLED": "true"}):
            with self.assertRaises(job.Refusal) as ctx:
                job.check_env(env)
            self.assertEqual(ctx.exception.code, job.EXIT_DISABLED)

    def test_no_default_skill_definition(self):
        for value in (None, "", "rmse", "MAE_CLIM", "default"):
            env = dict(ENABLED)
            if value is not None:
                env["SKILL_DEFINITION"] = value
            with self.assertRaises(job.Refusal) as ctx:
                job.check_env(env)
            self.assertEqual(ctx.exception.code, job.EXIT_CONFIG)
        for value in job.SKILL_DEFINITIONS:
            self.assertEqual(job.check_env({**ENABLED, "SKILL_DEFINITION": value}), value)

    def test_key_binding_must_be_active_for_the_next_block(self):
        self.assertTrue(job.key_binding_active(fx.node_status(True)))
        self.assertFalse(job.key_binding_active(fx.node_status(False)))
        self.assertFalse(job.key_binding_active(fx.node_status(True, known=False)))
        self.assertFalse(job.key_binding_active({"scheduled": {"entries": []}}))
        self.assertFalse(job.key_binding_active({}))


class AntiGamingConfig(unittest.TestCase):
    def test_off_by_default_and_strictly_parsed(self):
        self.assertFalse(job.anti_gaming_config({})["enabled"])
        self.assertTrue(job.anti_gaming_config({"FORECAST_ANTI_GAMING": "1"})["enabled"])
        for bad in ({"FORECAST_ANTI_GAMING": "yes"}, {"FORECAST_COPY_TOLERANCE": "[1]"},
                    {"FORECAST_COPY_TOLERANCE": '{"temperature_c": -1}'}):
            with self.assertRaises(job.Refusal):
                job.anti_gaming_config(bad)
        cfg = job.anti_gaming_config({"FORECAST_ANTI_GAMING": "1", "FORECAST_COPY_TOLERANCE": '{"temperature_c": 0.2}'})
        self.assertEqual(cfg["copy_tolerance"]["temperature_c"], 0.2)
        self.assertEqual(cfg["copy_tolerance"]["pressure_hpa"], job.DEFAULT_COPY_TOLERANCE["pressure_hpa"])


class Fixture(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        with contextlib.redirect_stdout(io.StringIO()):
            self.index = fx.build(self.tmp, job.kit)
        self.status = os.path.join(self.tmp, "status.json")
        self.write_status(True)
        self.references = os.path.join(self.tmp, "references.json")
        fx.write_reference_forecasts(self.references)

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def write_status(self, active):
        with open(self.status, "w") as fh:
            json.dump(fx.node_status(active), fh)

    def args(self, *extra):
        return job.parser().parse_args([
            "--reveals", os.path.join(self.tmp, "reveals"), "--observations", os.path.join(self.tmp, "obs"),
            "--node-status-file", self.status, "--out", os.path.join(self.tmp, "out"),
            "--chain-id", "fixture", "--network-id", "fixture", *extra])

    def run_job(self, env, *extra):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = job.run(self.args(*extra), env)
        with open(os.path.join(self.tmp, "out", "summary.json")) as fh:
            return code, json.load(fh), out.getvalue()


class Records(Fixture):
    def test_reveal_records_are_checked_against_their_own_files(self):
        problems = {name: job.load_reveal(os.path.join(self.tmp, "reveals", name))["problems"] for name in self.index}
        for name, (strategy, _) in self.index.items():
            if strategy == "tampered":
                self.assertTrue(any("commit_id" in p for p in problems[name]), problems[name])
            elif strategy == "label":
                self.assertTrue(any("20-byte address" in p for p in problems[name]), problems[name])
            else:
                self.assertEqual(problems[name], [], name)

    def test_truth_and_window_come_from_the_scorer_store(self):
        name = next(n for n, (s, st) in self.index.items() if s == "skilled" and st == "fcst-cell-a")
        reveal = job.load_reveal(os.path.join(self.tmp, "reveals", name))
        variables, notes = job.truth_inputs(reveal, os.path.join(self.tmp, "obs"))
        self.assertEqual(notes, [])
        self.assertEqual(sorted(variables), sorted(fx.VARIABLES))
        truth = fx.truth_series("fcst-cell-a", fx.ISSUE_HOUR + fx.HORIZON)
        for var, entry in variables.items():
            self.assertAlmostEqual(entry["truth"], truth[var])
            self.assertEqual(entry["window"], fx.window_values("fcst-cell-a")[var])

    def test_refuses_before_reading_anything_when_binding_is_inactive(self):
        self.write_status(False)
        with self.assertRaises(job.Refusal) as ctx:
            job.run(self.args("--dry-run"), {**ENABLED, "SKILL_DEFINITION": "mae_clim"})
        self.assertEqual(ctx.exception.code, job.EXIT_BINDING)
        self.assertFalse(os.path.exists(os.path.join(self.tmp, "out")))


@unittest.skipUnless(BIN, "forecast-score-batch not built")
class EndToEnd(Fixture):
    def decisions(self, summary):
        by_dir = {}
        for name in self.index:
            with open(os.path.join(self.tmp, "reveals", name, "commit.json")) as fh:
                by_dir[json.load(fh)["commit_id"]] = name
        return {by_dir[f["commit_id"]]: f for f in summary["forecasts"]}, {by_dir[c]: p for c, p in summary["skipped"].items()}

    def test_dry_run_scores_under_each_definition_and_signs_nothing(self):
        for definition in job.SKILL_DEFINITIONS:
            code, summary, printed = self.run_job({**ENABLED, "SKILL_DEFINITION": definition}, "--dry-run", "--scorer-bin", BIN)
            self.assertEqual(code, 0)
            self.assertIn("dry run: nothing signed", printed)
            self.assertEqual(summary["skill_definition"], definition)
            decided, skipped = self.decisions(summary)
            self.assertEqual(sorted(self.index[n][0] for n in skipped), ["label", "tampered"])
            dup = next(n for n in decided if self.index[n][0] == "skilled_dup")
            self.assertEqual(decided[dup]["decision"], "forecaster_already_selected")
            for name, entry in decided.items():
                receipt = os.path.join(self.tmp, "out", entry["commit_id"], "receipt.json")
                self.assertFalse(os.path.exists(receipt))
                if entry["decision"] == "selected":
                    self.assertGreater(entry["skill_score"], job.BASELINE_FORECAST_SKILL)
                    self.assertIn("forecast sign-receipt", entry["sign_command"])
                    self.assertIn("--metrics-file", entry["sign_command"])
                    with open(os.path.join(self.tmp, "out", entry["commit_id"], "metrics.json")) as fh:
                        metrics = json.load(fh)
                    self.assertEqual(sorted(metrics), ["bias", "mae", "rmse", "skill_score"])
                else:
                    self.assertNotIn("sign_command", entry)

    def test_cap_limits_receipts_per_cell_and_lead(self):
        _, summary, _ = self.run_job({**ENABLED, "SKILL_DEFINITION": "rmse_sigma"}, "--dry-run", "--scorer-bin", BIN,
                                     "--cap-per-cell-lead", "1")
        decided, _ = self.decisions(summary)
        for station in fx.STATIONS:
            selected = [n for n, e in decided.items() if self.index[n][1] == station and e["decision"] == "selected"]
            self.assertLessEqual(len(selected), 1, station)

    def test_anti_gaming_zeroes_copies_under_both_definitions(self):
        for definition in job.SKILL_DEFINITIONS:
            env = {**ENABLED, "SKILL_DEFINITION": definition, "FORECAST_ANTI_GAMING": "1"}
            _, summary, printed = self.run_job(env, "--dry-run", "--scorer-bin", BIN,
                                               "--reference-forecasts", self.references)
            self.assertTrue(summary["anti_gaming"])
            self.assertIn("with anti-gaming", printed)
            decided, _ = self.decisions(summary)
            for name, entry in decided.items():
                strategy = self.index[name][0]
                if strategy in ("climatology", "persistence", "reference"):
                    self.assertEqual(entry["skill_score"], 0.0, (definition, name))
                    self.assertEqual(entry["decision"], "below_baseline")
                    with open(os.path.join(self.tmp, "reveals", name, "commit.json")) as fh:
                        cid = json.load(fh)["commit_id"]
                    self.assertFalse(os.path.exists(os.path.join(self.tmp, "out", cid, "score.json")))
            skilled = [e for n, e in decided.items() if self.index[n][0] == "skilled"]
            self.assertTrue(all(e["decision"] == "selected" for e in skilled), definition)

    def test_signing_runs_the_cli_once_per_selected_forecast(self):
        log = os.path.join(self.tmp, "cli-calls.jsonl")
        fake = os.path.join(self.tmp, "fake-wrfcoin")
        with open(fake, "w") as fh:
            fh.write(f"#!{sys.executable}\nimport json, sys\nopen({log!r}, 'a').write(json.dumps(sys.argv[1:]) + '\\n')\n")
        os.chmod(fake, os.stat(fake).st_mode | stat.S_IXUSR)
        _, summary, printed = self.run_job({**ENABLED, "SKILL_DEFINITION": "mae_clim"}, "--scorer-bin", BIN, "--cli", fake)
        selected = [f for f in summary["forecasts"] if f["decision"] == "selected"]
        with open(log) as fh:
            calls = [json.loads(line) for line in fh]
        self.assertEqual(len(calls), len(selected))
        self.assertTrue(all(c[2:4] == ["forecast", "sign-receipt"] for c in calls))
        self.assertTrue(all(f["signed"] for f in selected))
        self.assertIn(f"{len(selected)} receipt(s) signed", printed)


if __name__ == "__main__":
    unittest.main()
