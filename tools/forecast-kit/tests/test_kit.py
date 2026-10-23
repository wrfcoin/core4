"""Offline tests for forecast_kit (no network, no model file needed).

Run: python -m unittest discover -s tools/forecast-kit/tests -v
"""

import io
from unittest import mock

import importlib.util
import json
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("forecast_kit", os.path.join(HERE, "..", "forecast_kit.py"))
kit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(kit)


class CanonicalForms(unittest.TestCase):
    def test_commit_id_matches_the_cli_contract_fixture(self):
        # Same inputs as tools/cli/WrfcoinCLI/tests/cli_api_contract.rs
        # `forecast_submission_uses_files_canonical_hash_and_bearer`; the expected
        # value is what `wrfcoin forecast commit --validate-only` printed for them
        # (see tools/forecast-kit/README.md, "Cross-check").
        got = kit.compute_commit_id({"temperature": 20}, "reveal-nonce", "station-forecast", 1700000000)
        # Printed by `wrfcoin --output json forecast commit --validate-only` for these inputs
        # (data.commit_id) on 2026-10-19; the Rust rule is Transaction::compute_commit_id.
        self.assertEqual(got, "6b5aa3ea14354680cb62048d12503440a7fae3fff8f4a4bae6a48fa4ce1450e7")  # pragma: allowlist secret

    def test_value_json_sorts_keys_and_is_compact(self):
        self.assertEqual(kit.canonical_value_json({"b": 1.5, "a": {"d": 2, "c": "x"}}), '{"a":{"c":"x","d":2},"b":1.5}')

    def test_struct_json_keeps_declaration_order(self):
        self.assertEqual(kit.canonical_struct_json({"schema_version": 1, "model_id": "m"}), '{"schema_version":1,"model_id":"m"}')

    def test_persistence_trend_matches_producer_maths(self):
        # y = 10 + 1*t over 24 h: slope 1, damping 0.5, horizon 6 -> last + 3
        series = [10.0 + t for t in range(24)]
        self.assertAlmostEqual(kit.persistence_trend(series, [i * 3600 for i in range(24)]), 33.0 + 3.0, places=9)
        self.assertAlmostEqual(kit.persistence_trend([5.0, 5.0, 5.0], [0, 3600, 7200]), 5.0, places=9)


class PrepareFlow(unittest.TestCase):
    def test_prepare_commit_and_reveal_write_validate_only_scripts(self):
        with tempfile.TemporaryDirectory() as tmp:
            forecast = {"model_id": "persistence-trend/v1", "issued_at": 1700000000, "valid_at": 1700021600,
                        "horizon_hours": 6, "station_id": "station-forecast",
                        "values": {"humidity_pct": 50.0, "pressure_hpa": 1000.1234, "temperature_c": 12.5}}
            window = [{"timestamp": 1700000000 - 3600 * (23 - i), "values": {"humidity_pct": 50.0, "pressure_hpa": 1000.0, "temperature_c": 12.0}} for i in range(24)]
            fpath, wpath = os.path.join(tmp, "forecast.json"), os.path.join(tmp, "window.json")
            json.dump(forecast, open(fpath, "w"))
            json.dump(window, open(wpath, "w"))
            out = os.path.join(tmp, "prepared")
            rc = kit.main(["prepare-commit", "--forecast", fpath, "--window", wpath, "--forecaster", "0xabc",
                           "--nonce", "n1", "--out", out, "--cli", "/opt/wrfcoin"])
            self.assertEqual(rc, 0)
            record = json.load(open(os.path.join(out, "commit.json")))
            self.assertFalse(record["submitted"])
            self.assertEqual(record["commit_id"], kit.compute_commit_id(forecast, "n1", "station-forecast", 1700000000))
            manifest = json.load(open(os.path.join(out, "manifest.json")))
            self.assertEqual(list(manifest), ["schema_version", "model_id", "input_hash", "output_hash", "produced_at",
                                              "region", "horizon_hours", "input_variables", "output_variables"])
            self.assertEqual(record["manifest_digest"], kit.manifest_digest(manifest))
            commit_sh = open(os.path.join(out, "commit.sh")).read()
            self.assertIn("--validate-only", commit_sh)
            self.assertIn("/opt/wrfcoin --output json forecast commit", commit_sh)
            self.assertNotIn("curl", commit_sh)
            # Reveal before valid_at is a problem unless --allow-early; receipt binding is checked.
            receipt = {"commit_id": record["commit_id"], "manifest_digest": record["manifest_digest"],
                       "oracle_id": "aa", "metrics": {"mae": 1, "rmse": 1, "bias": 0, "skill_score": 0.7},
                       "verified_at": 1700021700, "nonce": "r1", "signature": "bb"}
            rpath = os.path.join(tmp, "receipt.json")
            json.dump(receipt, open(rpath, "w"))
            rc = kit.main(["prepare-reveal", "--dir", out, "--receipt", rpath, "--allow-early", "--cli", "/opt/wrfcoin"])
            self.assertEqual(rc, 0)
            reveal_sh = open(os.path.join(out, "reveal.sh")).read()
            self.assertIn("--validate-only", reveal_sh)
            self.assertIn("--receipt-file", reveal_sh)
            receipt["commit_id"] = "0" * 64
            json.dump(receipt, open(rpath, "w"))
            rc = kit.main(["prepare-reveal", "--dir", out, "--receipt", rpath, "--allow-early", "--cli", "/opt/wrfcoin"])
            self.assertEqual(rc, 1)

    def test_predict_falls_back_when_humidity_is_missing(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = os.path.join(tmp, "obs")
            rows = {1700000000 + 3600 * i: {"temperature_c": 10.0 + i * 0.1, "pressure_hpa": 1000.0} for i in range(30)}
            kit.save_store(kit.store_path(store, "PWS-TEST"), rows)
            out = os.path.join(tmp, "fc")
            rc = kit.main(["predict", "--observations", store, "--station", "PWS-TEST", "--out", out])
            self.assertEqual(rc, 0)
            forecast = json.load(open(os.path.join(out, "forecast.json")))
            self.assertEqual(forecast["model_id"], "persistence-trend/v1")
            self.assertEqual(sorted(forecast["values"]), ["pressure_hpa", "temperature_c"])
            self.assertEqual(forecast["valid_at"] - forecast["issued_at"], 6 * 3600)


if __name__ == "__main__":
    unittest.main()


class Scoring(unittest.TestCase):
    def test_only_declared_and_reported_variables_are_scored(self):
        forecast = {"pressure_hpa": 1001.0, "temperature_c": 11.0}            # no humidity declared
        truth = {"pressure_hpa": 1000.0, "temperature_c": 10.0, "wind_speed_ms": 2.0, "humidity_pct": 50.0}
        clim = {"pressure_hpa": 1004.0, "temperature_c": 12.0}                 # |e_clim| = 4, 2 ; |e| = 1, 1
        report = kit.receipt_rule_score(forecast, truth, clim)
        self.assertEqual(report["scored_variables"], ["pressure_hpa", "temperature_c"])
        self.assertAlmostEqual(report["per_variable"]["pressure_hpa"]["skill"], 0.75)
        self.assertAlmostEqual(report["per_variable"]["temperature_c"]["skill"], 0.5)
        self.assertAlmostEqual(report["metrics"]["skill_score"], 0.625)
        self.assertAlmostEqual(report["metrics"]["mae"], 1.0)
        with self.assertRaises(ValueError):
            kit.receipt_rule_score({"humidity_pct": 1.0}, {"temperature_c": 2.0}, {})

    def test_score_command_writes_signer_metrics(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = os.path.join(tmp, "obs")
            base = 1700000000
            rows = {base + 3600 * i: {"temperature_c": 10.0, "pressure_hpa": 1000.0} for i in range(31)}
            rows[base + 3600 * 29]["temperature_c"] = 12.0                     # truth hour
            kit.save_store(kit.store_path(store, "PWS-TWO"), rows)
            forecast = {"model_id": "persistence-trend/v1", "issued_at": base + 3600 * 23, "valid_at": base + 3600 * 29,
                        "horizon_hours": 6, "station_id": "PWS-TWO", "values": {"pressure_hpa": 1000.0, "temperature_c": 11.0}}
            window = [{"timestamp": base + 3600 * i, "values": {"temperature_c": 10.0, "pressure_hpa": 1000.0}} for i in range(24)]
            fpath, wpath = os.path.join(tmp, "f.json"), os.path.join(tmp, "w.json")
            json.dump(forecast, open(fpath, "w")); json.dump(window, open(wpath, "w"))
            out = os.path.join(tmp, "score")
            rc = kit.main(["score", "--forecast", fpath, "--window", wpath, "--observations", store, "--station", "PWS-TWO", "--out", out])
            self.assertEqual(rc, 0)
            metrics = json.load(open(os.path.join(out, "metrics.json")))
            self.assertEqual(sorted(metrics), ["bias", "mae", "rmse", "skill_score"])
            # temperature: |e| = 1, |e_clim| = 2 -> 0.5 ; pressure: both exact -> 1.0 ; mean 0.75
            self.assertAlmostEqual(metrics["skill_score"], 0.75)
            report = json.load(open(os.path.join(out, "score.json")))
            self.assertEqual(report["scored_variables"], ["pressure_hpa", "temperature_c"])

    def test_export_csv_leaves_unreported_variables_blank(self):
        with tempfile.TemporaryDirectory() as tmp:
            store = os.path.join(tmp, "obs")
            kit.save_store(kit.store_path(store, "PWS-TWO"), {1700000000: {"temperature_c": 1.0, "pressure_hpa": 999.0}})
            rc = kit.main(["export-csv", "--observations", store, "--station", "PWS-TWO", "--out", os.path.join(tmp, "csv")])
            self.assertEqual(rc, 0)
            lines = open(os.path.join(tmp, "csv", "PWS-TWO.csv")).read().splitlines()
            self.assertEqual(lines[0], "time_utc,temperature_c,pressure_hpa,humidity_pct")
            self.assertTrue(lines[1].endswith(",1.00,999.00,"))


class HttpSafety(unittest.TestCase):
    def test_http_transport_is_get_only_and_keeps_bearer_out_of_output(self):
        bearer = "fixture-reader-canary"
        response = mock.MagicMock()
        response.__enter__.return_value.read.return_value = b'{"ok": true}'
        output = io.StringIO()
        with mock.patch.dict(os.environ, {"KIT_TEST_BEARER": bearer}), \
             mock.patch.object(kit.urllib.request, "urlopen", return_value=response) as call, \
             mock.patch("sys.stdout", output), mock.patch("sys.stderr", output):
            self.assertEqual(kit.http_get_json("http://example.invalid/read", "KIT_TEST_BEARER"), {"ok": True})
        request = call.call_args.args[0]
        self.assertEqual(request.get_method(), "GET")
        self.assertIsNone(request.data)
        self.assertEqual(request.get_header("Authorization"), "Bearer " + bearer)
        self.assertNotIn(bearer, output.getvalue())

    def test_http_error_cannot_echo_bearer(self):
        bearer = "fixture-reader-canary"
        error = kit.urllib.error.HTTPError("http://example.invalid/read", 403,
                                          "Forbidden", {}, io.BytesIO(bearer.encode()))
        with mock.patch.dict(os.environ, {"KIT_TEST_BEARER": bearer}), \
             mock.patch.object(kit.urllib.request, "urlopen", side_effect=error):
            with self.assertRaises(SystemExit) as raised:
                kit.http_get_json("http://example.invalid/read", "KIT_TEST_BEARER")
        self.assertEqual(str(raised.exception), "GET request failed: HTTP 403")
        self.assertNotIn(bearer, str(raised.exception))


class ReviewRegressions(unittest.TestCase):
    def test_gapped_prediction_uses_real_hour_offsets(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = 1700002800  # exact UTC hour
            rows = {base: {"temperature_c": 10.0}, base + 23 * 3600: {"temperature_c": 33.0}}
            kit.save_store(kit.store_path(tmp, "gapped"), rows)
            out = os.path.join(tmp, "prediction")
            with mock.patch.object(kit.time, "time", return_value=base + 30 * 3600):
                self.assertEqual(kit.main(["predict", "--observations", tmp, "--station", "gapped", "--out", out]), 0)
            with open(os.path.join(out, "forecast.json")) as fh:
                forecast = json.load(fh)
            self.assertEqual(forecast["values"]["temperature_c"], 36.0)

    def test_default_window_excludes_open_hour_but_at_can_override(self):
        current_hour = 500000
        hourly = {h: {"temperature_c": float(h)} for h in range(current_hour - 25, current_hour + 1)}
        with mock.patch.object(kit.time, "time", return_value=current_hour * 3600 + 3599):
            rows, end, _ = kit.build_window(hourly)
            self.assertEqual(end, current_hour - 1)
            self.assertEqual(rows[-1]["timestamp"], (current_hour - 1) * 3600)
            _, explicit_end, _ = kit.build_window(hourly, current_hour)
            self.assertEqual(explicit_end, current_hour)

    def test_reveal_refuses_edited_data_even_with_allow_early_and_old_cli_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            forecast = {"model_id": "persistence-trend/v1", "issued_at": 1700000000, "valid_at": 9999999999,
                        "horizon_hours": 6, "station_id": "station", "values": {"temperature_c": 12.0}}
            fpath, wpath = os.path.join(tmp, "f.json"), os.path.join(tmp, "w.json")
            with open(fpath, "w") as fh: json.dump(forecast, fh)
            with open(wpath, "w") as fh: json.dump([], fh)
            out = os.path.join(tmp, "prepared")
            kit.main(["prepare-commit", "--forecast", fpath, "--window", wpath, "--forecaster", "0xabc", "--out", out])
            record_path = os.path.join(out, "commit.json")
            with open(record_path) as fh: record = json.load(fh)
            original_record = open(record_path).read()
            cli_output = os.path.join(tmp, "cli.json")
            with open(cli_output, "w") as fh: json.dump({"data": {"commit_id": record["commit_id"]}}, fh)
            forecast["values"]["temperature_c"] = 99.0
            with open(record["files"]["data"], "w") as fh: json.dump(forecast, fh)
            error = io.StringIO()
            with mock.patch("sys.stderr", error):
                rc = kit.main(["prepare-reveal", "--dir", out, "--allow-early", "--cli-output", cli_output])
            self.assertEqual(rc, 1)
            self.assertIn("data.json", error.getvalue())
            self.assertFalse(os.path.exists(os.path.join(out, "reveal.sh")))
            self.assertEqual(open(record_path).read(), original_record)

    def test_backfill_shortfall_is_explicit_with_actual_count_and_span(self):
        docs = [
            {"chain_height": 10000},
            {"data": {"blocks": [{"timestamp": "2026-10-23T00:00:00+00:00"},
                                  {"timestamp": "2026-10-22T00:00:00+00:00"}]}},
            {"items": [{"observed_at": 1700000000, "values": {"temperature_c": 10}},
                       {"observed_at": 1700003600, "values": {"temperature_c": 11}}], "has_more": False},
        ]
        error = io.StringIO()
        with mock.patch.object(kit, "http_get_json", side_effect=docs), mock.patch("sys.stderr", error):
            rows = kit.fetch_node("http://example.invalid", "station", 240, "TEST_READER", 0, 5)
        self.assertEqual(len(rows), 2)
        self.assertIn("WARNING", error.getvalue())
        self.assertIn("240", error.getvalue())
        self.assertIn("2 readings", error.getvalue())
        self.assertIn("1.0 h", error.getvalue())

    def test_prediction_timing_starts_at_last_closed_hour_boundary(self):
        with tempfile.TemporaryDirectory() as tmp:
            current_hour = 500000
            rows = {h * 3600: {"temperature_c": 10.0} for h in range(current_hour - 24, current_hour)}
            rows[current_hour * 3600] = {"temperature_c": 999.0}
            kit.save_store(kit.store_path(tmp, "closed"), rows)
            out = os.path.join(tmp, "prediction")
            with mock.patch.object(kit.time, "time", return_value=current_hour * 3600 + 3599):
                self.assertEqual(kit.main(["predict", "--observations", tmp, "--station", "closed", "--out", out]), 0)
            with open(os.path.join(out, "forecast.json")) as fh: forecast = json.load(fh)
            self.assertEqual(forecast["values"]["temperature_c"], 10.0)
            self.assertEqual(forecast["issued_at"], current_hour * 3600)
            self.assertEqual(forecast["valid_at"], (current_hour + 6) * 3600)

    def test_backfill_complete_span_does_not_warn(self):
        docs = [{"chain_height": 100}, {},
                {"items": [{"observed_at": ts, "values": {"temperature_c": 10}}
                           for ts in [1700000000, 1700000000 + 240 * 3600]], "has_more": False}]
        error = io.StringIO()
        with mock.patch.object(kit, "http_get_json", side_effect=docs), mock.patch("sys.stderr", error):
            self.assertEqual(len(kit.fetch_node("http://example.invalid", "station", 240, "READER", 0, 5)), 2)
        self.assertIn("240.0 h", error.getvalue())
        self.assertNotIn("WARNING", error.getvalue())
