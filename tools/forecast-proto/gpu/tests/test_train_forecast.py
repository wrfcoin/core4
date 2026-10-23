"""Offline tests for the trainer's observations loader and scoring helpers.

Run: python -m unittest discover -s tools/forecast-proto/gpu/tests -v   (numpy only; no torch)
"""

import importlib.util
import os
import sys
import tempfile
import unittest

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("train_forecast", os.path.join(HERE, "..", "train_forecast.py"))
tf = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tf)


def write_station(directory, name, hours, skip=(), drop_var_at=()):
    path = os.path.join(directory, f"{name}.csv")
    with open(path, "w") as fh:
        fh.write("time_utc,temperature_c,pressure_hpa,humidity_pct\n")
        for h in range(hours):
            if h in skip:
                continue
            t = 1_750_000_000 + h * 3600
            iso = tf.dt.datetime.fromtimestamp(t, tf.dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
            hum = "" if h in drop_var_at else f"{60 + (h % 5):.2f}"
            fh.write(f"{iso},{10 + np.sin(h / 24 * 2 * np.pi) * 5:.2f},{1010 + h * 0.01:.2f},{hum}\n")
    return path


class Loader(unittest.TestCase):
    def test_short_gaps_are_interpolated_and_long_gaps_break_windows(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_station(tmp, "A", 120, skip=(10, 11), drop_var_at=(50,))   # 2 h gap: filled
            write_station(tmp, "B", 120, skip=(40, 41, 42, 43))             # 4 h gap: stays NaN
            stations = tf.load_observations(tmp, max_gap_hours=2)
            self.assertEqual(sorted(stations), ["A", "B"])
            hours_a, values_a = stations["A"]
            self.assertEqual(len(hours_a), 120)
            self.assertFalse(np.isnan(values_a[10:12]).any(), "2 h gap must be interpolated")
            self.assertFalse(np.isnan(values_a[50, 2]), "single missing humidity must be interpolated")
            _, values_b = stations["B"]
            self.assertTrue(np.isnan(values_b[40:44]).all(), "4 h gap must stay missing")
            xs_a, ys_a, ts_a = tf.make_station_windows(*stations["A"])
            self.assertEqual(len(xs_a), 120 - tf.WINDOW_HOURS - tf.HORIZON_HOURS + 1)
            xs_b, _, _ = tf.make_station_windows(*stations["B"])
            # Every window touching hours 40..43 as input or target is dropped.
            self.assertLess(len(xs_b), len(xs_a))
            self.assertEqual(xs_a.shape[1:], (tf.WINDOW_HOURS, 3))
            self.assertEqual(ts_a[0], hours_a[tf.WINDOW_HOURS + tf.HORIZON_HOURS - 1])

    def test_missing_column_is_an_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            with open(os.path.join(tmp, "X.csv"), "w") as fh:
                fh.write("time_utc,temperature_c\n2026-01-01T00:00:00Z,1\n")
            with self.assertRaises(SystemExit):
                tf.load_observations(tmp)


class Scoring(unittest.TestCase):
    def test_per_forecast_skill_is_the_receipt_rule(self):
        truth = np.array([[10.0, 1000.0, 50.0]])
        clim = np.array([[12.0, 1004.0, 60.0]])          # |e_clim| = 2, 4, 10
        pred = np.array([[11.0, 1000.0, 70.0]])          # |e| = 1, 0, 20
        skill = tf.per_forecast_skill(pred, truth, clim)
        np.testing.assert_allclose(skill, [[0.5, 1.0, 0.0]])
        # climatology exactly right and forecast exactly right -> 1; forecast wrong -> 0
        np.testing.assert_allclose(tf.per_forecast_skill(truth, truth, truth), [[1.0, 1.0, 1.0]])
        np.testing.assert_allclose(tf.per_forecast_skill(pred, truth, truth), [[0.0, 1.0, 0.0]])

    def test_persistence_trend_matches_the_producer(self):
        window = np.stack([10.0 + np.arange(24), np.full(24, 1000.0), 50.0 - np.arange(24)], axis=1)
        out = tf.persistence_trend(window)
        np.testing.assert_allclose(out, [33.0 + 0.5 * 1.0 * 6, 1000.0, 27.0 - 0.5 * 1.0 * 6])


if __name__ == "__main__":
    unittest.main()


class CommittedModels(unittest.TestCase):
    """The ONNX files next to models/README.md are binaries: a text diff shows them as
    "Binary files differ", so this is the `strings`-style check that their doc_string,
    metadata and raw bytes carry no station identifiers, personal names, city names or
    user paths (privacy ruling 2026-10-19; see core4#3597 / #3614)."""

    # Tokens are split so that this file itself never contains them: the reviewer's
    # `git log -p … | grep` for the same words must stay at zero on the branch.
    FORBIDDEN = tuple("".join(parts).encode() for parts in (
        ("du", "luth"), ("ti", "oga"), ("san ", "jose"), ("ste", "ven"), ("/ho", "me/"), ("pws", "-")))

    def test_committed_onnx_metadata_is_neutral(self):
        import glob
        import onnx
        models_dir = os.path.join(HERE, "..", "..", "models")
        paths = sorted(glob.glob(os.path.join(models_dir, "*.onnx")))
        self.assertTrue(paths, "no committed models found")
        for path in paths:
            with open(path, "rb") as fh:
                raw = fh.read().lower()
            for token in self.FORBIDDEN:
                self.assertNotIn(token, raw, f"{os.path.basename(path)} contains {token!r}")
            graph = onnx.load(path)
            text = (graph.doc_string + " ".join(f"{m.key}={m.value}" for m in graph.metadata_props)).lower()
            for token in self.FORBIDDEN:
                self.assertNotIn(token.decode(), text)
            record = os.path.splitext(path)[0] + "-export.json"
            if os.path.exists(record):
                with open(record, "rb") as fh:
                    blob = fh.read().lower()
                for token in self.FORBIDDEN:
                    self.assertNotIn(token, blob, f"{os.path.basename(record)} contains {token!r}")
