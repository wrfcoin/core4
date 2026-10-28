"""Offline tests for the observation-quality report work unit (no network).

Run: python -m unittest discover -s tools/report-kit/tests -v
"""

import contextlib
import importlib.util
import io
import json
import math
import os
import random
import shutil
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("obs_quality_report", os.path.join(HERE, "..", "obs_quality_report.py"))
rep = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rep)

BASE_T = 1_706_400_000  # 2024-01-28T00:00Z
STATIONS = ("obs-cell-1", "obs-cell-2", "obs-cell-3", "obs-cell-4")


def node_items(station: str, *, temp_bias=0.0, spike_at=None, gap=False, from_height=100):
    """The node route's item shape: one reading per hour for 24 h, heights 100 + hour."""
    k = STATIONS.index(station)
    items = []
    for hour in range(24):
        if gap and 6 <= hour < 18:
            continue
        temp = 5 + 4 * math.sin(2 * math.pi * hour / 24) + 0.1 * k + temp_bias
        if spike_at == hour:
            temp += 40.0
        items.append({
            "commit_id": f"{station}-{hour:02d}", "station_id": station, "height": from_height + hour,
            "observed_at": BASE_T + hour * 3600 + 120 * k,
            "values": {"temperature_c": round(temp, 3), "pressure_hpa": round(1010 + 0.05 * hour + 0.2 * k, 3),
                       "humidity_percent": round(70 - 8 * math.sin(2 * math.pi * hour / 24), 3),
                       "wind_speed_ms": 3.0, "dewpoint_c": None},
            "provenance_class": "real", "attestation": {"approvals": 2, "has_quorum": hour % 5 != 0},
        })
    return items


class ReportTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.inputs = os.path.join(self.tmp, "inputs")
        os.makedirs(self.inputs)

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def write(self, station, items, wrap=True):
        body = {"success": True, "data": {"items": items, "limit": 500}} if wrap else items
        with open(os.path.join(self.inputs, f"{station}.json"), "w") as fh:
            json.dump(body, fh)

    def standard(self):
        self.write("obs-cell-1", node_items("obs-cell-1"))
        self.write("obs-cell-2", node_items("obs-cell-2", temp_bias=3.0))       # drifts warm
        self.write("obs-cell-3", node_items("obs-cell-3", spike_at=12))          # one spike
        self.write("obs-cell-4", node_items("obs-cell-4", gap=True), wrap=False) # half coverage, bare list

    def unit(self, stations="*", from_h=100, to_h=123):
        return rep.build_unit(self.inputs, from_h, to_h, "fixture", "fixture", stations, list(rep.DEFAULT_VARIABLES))

    def summary(self, unit):
        return {s["name"]: s["value"] for s in unit["output"]["summary"]}

    def test_two_runs_on_the_same_inputs_give_the_same_digests(self):
        self.standard()
        a, b = self.unit(), self.unit()
        self.assertEqual(a, b)
        for key in ("work_unit_id",):
            self.assertRegex(a[key], r"^[0-9a-f]{64}$")
        self.assertRegex(a["output"]["output_digest"], r"^[0-9a-f]{64}$")

    def test_input_order_does_not_matter(self):
        self.standard()
        before = self.unit()
        for name in os.listdir(self.inputs):
            path = os.path.join(self.inputs, name)
            with open(path) as fh:
                body = json.load(fh)
            items = body["data"]["items"] if isinstance(body, dict) else body
            random.Random(7).shuffle(items)
            with open(path, "w") as fh:
                json.dump(body, fh)
        os.rename(os.path.join(self.inputs, "obs-cell-1.json"), os.path.join(self.inputs, "zz-renamed.json"))
        self.assertEqual(self.unit(), before)

    def test_any_input_change_changes_the_digests(self):
        self.standard()
        before = self.unit()
        items = node_items("obs-cell-1")
        items[3]["values"]["temperature_c"] += 0.001
        self.write("obs-cell-1", items)
        after = self.unit()
        self.assertNotEqual(after["input_manifest"]["inputs_digest"], before["input_manifest"]["inputs_digest"])
        self.assertNotEqual(after["work_unit_id"], before["work_unit_id"])

    def test_range_and_station_selection_are_part_of_the_unit(self):
        self.standard()
        full, half = self.unit(), self.unit(to_h=111)
        self.assertEqual(self.summary(full)["readings"], 24 * 3 + 12)
        self.assertEqual(self.summary(half)["readings"], 12 * 3 + 6)
        self.assertNotEqual(full["work_unit_id"], half["work_unit_id"])
        some = self.unit(stations=["obs-cell-3", "obs-cell-1"])
        self.assertEqual(self.summary(some)["stations"], 2)
        self.assertEqual(some["input_manifest"]["stations"], ["obs-cell-1", "obs-cell-3"])

    def test_duplicate_exports_are_one_reading(self):
        self.standard()
        before = self.unit()
        self.write("obs-cell-1-copy", node_items("obs-cell-1"))
        self.assertEqual(self.unit(), before)

    def test_quality_findings(self):
        self.standard()
        s = self.summary(self.unit())
        self.assertEqual(s["stations"], 4)
        self.assertEqual(s["attested_readings"], sum(1 for st in ("obs-cell-1", "obs-cell-2", "obs-cell-3", "obs-cell-4")
                                                     for i in node_items(st, gap=st == "obs-cell-4")
                                                     if i["attestation"]["has_quorum"]))
        self.assertEqual(s["stations_below_half_coverage"], 0)  # 12 of 24 hours is exactly one half
        self.assertEqual(s["coverage_min_micro"], 500_000)
        self.assertEqual(s["temperature_c.spikes"], 1)
        self.assertEqual(s["temperature_c.out_of_bounds"], 0)
        self.assertEqual(s["temperature_c.drift_flagged_stations"], 1)
        self.assertGreater(s["temperature_c.drift_max_abs_micro"], 2_000_000)
        self.assertEqual(s["pressure_hpa.drift_flagged_stations"], 0)
        self.assertEqual(s["humidity_pct.readings"], 24 * 3 + 12)

    def test_out_of_bounds_values_are_counted(self):
        self.standard()
        items = node_items("obs-cell-1")
        items[0]["values"]["humidity_percent"] = 140.0
        self.write("obs-cell-1", items)
        self.assertEqual(self.summary(self.unit())["humidity_pct.out_of_bounds"], 1)

    def test_only_regional_aggregates_are_published(self):
        self.standard()
        out = json.dumps(self.unit()["output"])
        for station in STATIONS:
            self.assertNotIn(station, out)

    def test_cli_run_then_verify_agrees_and_detects_tampering(self):
        self.standard()
        unit_dir = os.path.join(self.tmp, "unit")
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(rep.main(["run", "--inputs", self.inputs, "--from-height", "100", "--to-height", "123",
                                       "--chain-id", "fixture", "--network-id", "fixture", "--out", unit_dir]), 0)
            self.assertEqual(rep.main(["verify", "--unit", unit_dir, "--inputs", self.inputs]), 0)
        with open(os.path.join(unit_dir, "work_unit.json")) as fh:
            claimed = json.load(fh)
        claimed["output_digest"] = "0" * 64
        with open(os.path.join(unit_dir, "work_unit.json"), "w") as fh:
            json.dump(claimed, fh)
        with contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertEqual(rep.main(["verify", "--unit", unit_dir, "--inputs", self.inputs]), 1)
        self.assertIn("output_digest", err.getvalue())
        # A verifier whose chain inputs differ reports inputs_mismatch, not a disagreement.
        self.write("obs-cell-1", node_items("obs-cell-1")[:-1])
        with contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertEqual(rep.main(["verify", "--unit", unit_dir, "--inputs", self.inputs]), 3)
        self.assertIn("inputs_mismatch", err.getvalue())

    def test_encoding_matches_bodybuilder_length_prefixing(self):
        body = rep.Body().str_("ab").u64(1)
        self.assertEqual(bytes(body.buf), (2).to_bytes(8, "little") + b"ab" + (1).to_bytes(8, "little"))
        self.assertEqual(rep.micro(1.5e-6), 2)
        self.assertEqual(rep.micro(-1.5e-6), -2)
        self.assertEqual(rep.micro(0.25), 250_000)


if __name__ == "__main__":
    unittest.main()
