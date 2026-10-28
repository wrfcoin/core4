"""Deterministic fixture set for the scorer job (epic #3604 A1/A6). No network, no keys.

Two stations, hourly observations around one issue time, and one forecast per strategy, each prepared by
the kit's own `prepare-commit` so the scorer reads exactly what a forecaster produces. Strategies:

  skilled        truth plus a small error
  near_miss      truth plus a moderate error
  climatology    the 24 h window mean (a "climatology dump")
  persistence    the last window value (a "persistence copy")
  reference      a supplied external reference forecast, copied (stands in for a NOAA point value)
  poor           far from truth
  skilled_dup    the skilled forecaster again, same cell and lead (exercises one-per-forecaster)
  label          a forecaster that is not an address (refused: the key binding needs a key-derived address)
  tampered       data.json edited after the commit id was computed (refused)
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os

ISSUE_HOUR = 474_000  # 2024-01-28T00:00Z, a fixed unix hour
HORIZON = 6
STATIONS = ("fcst-cell-a", "fcst-cell-b")
VARIABLES = ("humidity_pct", "pressure_hpa", "temperature_c")


def address(tag: str) -> str:
    return "0x" + hashlib.sha3_256(tag.encode()).hexdigest()[:40]


def truth_series(station: str, hour: int) -> dict:
    k = 0.0 if station.endswith("a") else 1.3
    t = hour - ISSUE_HOUR
    return {
        "temperature_c": round(8 + 5 * math.sin(2 * math.pi * (t + 3 + k) / 24) + 0.04 * t, 3),
        "pressure_hpa": round(1012 + 1.5 * math.sin(2 * math.pi * (t + k) / 36) - 0.08 * t, 3),
        "humidity_pct": round(72 - 12 * math.sin(2 * math.pi * (t + 3 + k) / 24), 3),
    }


def write_observations(obs_dir: str, kit) -> None:
    for station in STATIONS:
        rows = {}
        for hour in range(ISSUE_HOUR - 30, ISSUE_HOUR + HORIZON + 2):
            rows[hour * 3600 + 600] = truth_series(station, hour)  # one reading 10 min into each hour
        kit.save_store(kit.store_path(obs_dir, station), rows)


def window_values(station: str) -> dict:
    hours = range(ISSUE_HOUR - 24, ISSUE_HOUR)
    return {v: [truth_series(station, h)[v] for h in hours] for v in VARIABLES}


def reference_forecast(station: str) -> dict:
    """A deterministic 'external model' value (stands in for a NOAA/GFS point forecast)."""
    truth = truth_series(station, ISSUE_HOUR + HORIZON)
    return {"temperature_c": round(truth["temperature_c"] + 0.9, 3),
            "pressure_hpa": round(truth["pressure_hpa"] - 0.6, 3),
            "humidity_pct": round(truth["humidity_pct"] + 4.0, 3)}


def strategy_values(strategy: str, station: str) -> dict:
    truth = truth_series(station, ISSUE_HOUR + HORIZON)
    window = window_values(station)
    offsets = {"skilled": (0.3, -0.2, 1.5), "skilled_dup": (0.5, -0.3, 2.0), "near_miss": (1.4, 0.9, 6.0),
               "poor": (5.0, -4.0, 20.0), "label": (0.4, 0.2, 2.0), "tampered": (0.4, 0.2, 2.0)}
    if strategy == "climatology":
        return {v: round(sum(xs) / len(xs), 4) for v, xs in window.items()}
    if strategy == "persistence":
        return {v: xs[-1] for v, xs in window.items()}
    if strategy == "reference":
        return reference_forecast(station)
    dt, dp, dh = offsets[strategy]
    return {"temperature_c": round(truth["temperature_c"] + dt, 4),
            "pressure_hpa": round(truth["pressure_hpa"] + dp, 4),
            "humidity_pct": round(truth["humidity_pct"] + dh, 4)}


FORECASTS = [  # (strategy, station, forecaster tag)
    ("skilled", "fcst-cell-a", "forecaster-1"),
    ("skilled_dup", "fcst-cell-a", "forecaster-1"),
    ("near_miss", "fcst-cell-a", "forecaster-2"),
    ("climatology", "fcst-cell-a", "forecaster-3"),
    ("persistence", "fcst-cell-a", "forecaster-4"),
    ("reference", "fcst-cell-a", "forecaster-5"),
    ("poor", "fcst-cell-a", "forecaster-6"),
    ("skilled", "fcst-cell-b", "forecaster-1"),
    ("climatology", "fcst-cell-b", "forecaster-3"),
    ("persistence", "fcst-cell-b", "forecaster-4"),
    ("reference", "fcst-cell-b", "forecaster-5"),
    ("label", "fcst-cell-b", None),
    ("tampered", "fcst-cell-b", "forecaster-7"),
]


def build(root: str, kit) -> dict:
    """Write <root>/obs and <root>/reveals/<n>-<strategy>-<station>; return {dir_name: (strategy, station)}."""
    obs, reveals = os.path.join(root, "obs"), os.path.join(root, "reveals")
    write_observations(obs, kit)
    index = {}
    for n, (strategy, station, tag) in enumerate(FORECASTS):
        name = f"{n:02d}-{strategy}-{station}"
        work = os.path.join(root, "work", name)
        os.makedirs(work, exist_ok=True)
        forecast = {"model_id": f"fixture-{strategy}/v1", "issued_at": ISSUE_HOUR * 3600,
                    "valid_at": (ISSUE_HOUR + HORIZON) * 3600, "horizon_hours": HORIZON,
                    "station_id": station, "values": strategy_values(strategy, station)}
        with open(os.path.join(work, "forecast.json"), "w") as fh:
            json.dump(forecast, fh)
        with open(os.path.join(work, "window.json"), "w") as fh:
            json.dump([{"timestamp": 0, "values": {}}], fh)
        args = argparse.Namespace(forecast=os.path.join(work, "forecast.json"), window=os.path.join(work, "window.json"),
                                  out=os.path.join(reveals, name), nonce=f"fixture-{n}",
                                  forecaster=address(tag) if tag else "WRF1Forecast",
                                  chain_id="fixture", network_id="fixture", node="http://127.0.0.1:1", cli="wrfcoin")
        kit.cmd_prepare_commit(args)
        if strategy == "tampered":
            data_path = os.path.join(reveals, name, "data.json")
            with open(data_path) as fh:
                data = json.load(fh)
            data["values"]["temperature_c"] += 1.0
            with open(data_path, "w") as fh:
                fh.write(kit.canonical_value_json(data) + "\n")
        index[name] = (strategy, station)
    return index


def node_status(active: bool, known: bool = True) -> dict:
    return {"status": "ready", "scheduled": {"entries": [
        {"feature": "forecaster_key_binding", "activation_height": 100, "source": "extension",
         "known": known, "active_at_next": active, "blocks_remaining": 0}]}}
