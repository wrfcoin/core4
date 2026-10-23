#!/usr/bin/env python3
"""WRFCoin forecaster kit: observations in, forecast out, commit/reveal files ready.

This tool never talks to a node except for read-only GETs, and it never
submits a transaction. It prepares the exact files and command lines that the
`wrfcoin forecast commit --from-file`-style CLI (#3084) consumes, so that the
owner of the wallet (a person, or an agent holding the key) performs the one
on-chain step deliberately. Every command line it writes carries
`--validate-only`: the CLI parses, signs and checks the body and prints the
commit id without sending anything. Dropping that flag is the submit step.

Subcommands
-----------
fetch-observations   Pull a station's recent readings (public backend tail, or a node's
                     Reader-role history route, or a trainer-format CSV) into a local store.
predict              Build the 24 h hourly window and run the ONNX model (CPU); falls back
                     to persistence-trend/v1 when the window is incomplete, and says so.
prepare-commit       Write data.json, manifest.json, commit.json and commit.sh.
prepare-reveal       Check timing and the optional receipt binding; write reveal.sh.
score                Score a forecast against the truth reading the store holds at valid_at,
                     over the variables the forecast declares AND the station reported; writes
                     the metrics file an oracle's `forecast sign-receipt` takes (#3149).
export-csv           Write a station's store as the trainer's hourly CSV (gpu/train_forecast.py).
status               Read back a commit's on-chain status (unauthenticated GET).

See README.md next to this file for the walk-through and the weekly-budget pattern.
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import hashlib
import json
import math
import os
import shlex
import stat
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

VARIABLES = ["temperature_c", "pressure_hpa", "humidity_pct"]  # the model's input/output order
WINDOW_HOURS = 24
HORIZON_HOURS = 6
MODEL_LEARNED = "learned-conv1d/v1"
MODEL_PERSISTENCE = "persistence-trend/v1"
TREND_DAMPING = 0.5
DEFAULT_PUBLIC_API = "https://wrfcoin.com"
DEFAULT_NODE = "http://127.0.0.1:8556"
READER_TOKEN_ENV = "WRFCOIN_READER_TOKEN"

# Public backend row fields -> canonical variables (backend station-detail-routes.ts).
PUBLIC_FIELDS = {"temperatureC": "temperature_c", "pressureHpa": "pressure_hpa", "humidityPct": "humidity_pct"}
# Node route fields (launch_api routes/transactions.rs get_station_observations).
NODE_FIELDS = {"temperature_c": "temperature_c", "pressure_hpa": "pressure_hpa", "humidity_percent": "humidity_pct"}


# ---------------------------------------------------------------------------
# Canonical JSON and digests (what the Rust side computes)
# ---------------------------------------------------------------------------

def canonical_value_json(value) -> str:
    """`serde_json::to_string(&serde_json::Value)`: compact, object keys sorted.

    serde_json's `Value::Object` is a BTreeMap, so keys come out sorted; floats
    print in shortest round-trip form, which Python's `repr` matches for the
    values this kit writes (it rounds forecast values to 4 decimals and never
    emits exponent notation). Confirm with `wrfcoin forecast commit --validate-only`,
    whose `commit_id` must equal the one in commit.json.
    """
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)


def canonical_struct_json(ordered: dict) -> str:
    """`serde_json::to_vec(&Struct)`: compact, fields in declaration order (NOT sorted)."""
    return json.dumps(ordered, separators=(",", ":"), ensure_ascii=False, allow_nan=False)


def sha3_hex(data: bytes) -> str:
    return hashlib.sha3_256(data).hexdigest()


def compute_commit_id(payload, nonce: str, station_id: str, timestamp: int, contribution_type: str = "forecast") -> str:
    """`Transaction::compute_commit_id` (shared-protocol/src/blockchain/transaction.rs):
    sha3-256 over `"{data}||{nonce}||{station_id}||{timestamp}||{type}"`."""
    preimage = f"{canonical_value_json(payload)}||{nonce}||{station_id}||{timestamp}||{contribution_type}"
    return sha3_hex(preimage.encode("utf-8"))


def manifest_digest(manifest: dict) -> str:
    """`ForecastArtifactManifest::manifest_digest`: sha3-256 of the struct's compact JSON."""
    return sha3_hex(canonical_struct_json(manifest).encode("utf-8"))


# ---------------------------------------------------------------------------
# HTTP (read-only GET; a bearer is read from the environment and never printed)
# ---------------------------------------------------------------------------

def http_get_json(url: str, bearer_env: str | None = None, timeout: int = 60):
    headers = {"accept": "application/json", "user-agent": "wrfcoin-forecast-kit/0.1 (read-only)"}
    if bearer_env:
        token = os.environ.get(bearer_env, "")
        if not token:
            raise SystemExit(f"{bearer_env} is not set in the environment (the value is never printed)")
        headers["authorization"] = f"Bearer {token}"
    req = urllib.request.Request(url, headers=headers, method="GET")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        # Remote diagnostics may echo Authorization; never print response bodies.
        raise SystemExit(f"GET request failed: HTTP {exc.code}") from None


# ---------------------------------------------------------------------------
# Observation store: <out>/<station>.jsonl, one {"observed_at": unix, "values": {...}} per line
# ---------------------------------------------------------------------------

def store_path(out_dir: str, station: str) -> str:
    safe = "".join(c if c.isalnum() or c in "-_." else "_" for c in station)
    return os.path.join(out_dir, f"{safe}.jsonl")


def load_store(path: str) -> dict[int, dict]:
    rows = {}
    if os.path.exists(path):
        with open(path) as fh:
            for line in fh:
                line = line.strip()
                if line:
                    row = json.loads(line)
                    rows[int(row["observed_at"])] = row["values"]
    return rows


def save_store(path: str, rows: dict[int, dict]) -> None:
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "w") as fh:
        for ts in sorted(rows):
            fh.write(json.dumps({"observed_at": ts, "values": rows[ts]}, separators=(",", ":")) + "\n")


def merge_rows(rows: dict[int, dict], new_rows: list[tuple[int, dict]]) -> int:
    added = 0
    for ts, values in new_rows:
        if ts not in rows:
            added += 1
        rows[ts] = values
    return added


def fetch_public(api_base: str, alias: str) -> list[tuple[int, dict]]:
    """Backend `GET /api/stations/<alias>/observations`: the chain tail (~64 blocks).

    The public edge admits the bare path only (no query string), so there is no
    pagination here; call it periodically and the store accumulates history.
    """
    url = f"{api_base.rstrip('/')}/api/stations/{urllib.parse.quote(alias)}/observations"
    doc = http_get_json(url)
    if isinstance(doc.get("unavailable"), list) and "observations" in doc["unavailable"]:
        raise SystemExit("backend reports observations unavailable (core4 not reachable from the backend)")
    out = []
    for row in doc.get("observations", []):
        values = {}
        for src, dst in PUBLIC_FIELDS.items():
            if row.get(src) is not None:
                values[dst] = float(row[src])
        if "observedAt" in row and values:
            out.append((int(row["observedAt"]), values))
    return out


def fetch_node(node_base: str, station_id: str, hours: int, bearer_env: str, sleep_s: float, max_requests: int) -> list[tuple[int, dict]]:
    """Node `GET /transactions/contributions/<station_id>/observations` (Reader role).

    Scans blocks upward from `after` in pages of at most 100 reveals / 64 heights;
    the cursor is (`next_after`, `next_offset`). Stops when `hours` of history are
    covered from the newest reading, when the chain tip is reached, or at
    `max_requests`. The bearer comes from `bearer_env` and is never printed.
    """
    base = f"{node_base.rstrip('/')}/transactions/contributions/{urllib.parse.quote(station_id)}/observations"
    after, offset, out, requests_made = 0, 0, [], 0
    newest = None
    # Walk from the tip backwards is not offered by the route, so estimate a start
    # height from the tip and the block cadence, then refine by walking forward.
    info = http_get_json(f"{node_base.rstrip('/')}/network/info")
    tip = int(info.get("chain_height", 0))
    blocks = http_get_json(f"{node_base.rstrip('/')}/chain/blocks?limit=2").get("data", {}).get("blocks", [])
    cadence_s = 75.0
    if len(blocks) == 2:
        t0 = dt.datetime.fromisoformat(blocks[0]["timestamp"]).timestamp()
        t1 = dt.datetime.fromisoformat(blocks[1]["timestamp"]).timestamp()
        cadence_s = max(abs(t1 - t0), 1.0)
    after = max(0, tip - int(hours * 3600 / cadence_s) - 64)
    while requests_made < max_requests:
        query = urllib.parse.urlencode({"after": after, "offset": offset, "limit": 100})
        doc = http_get_json(f"{base}?{query}", bearer_env=bearer_env)
        data = doc.get("data", doc)
        requests_made += 1
        for item in data.get("items", []):
            values = {}
            for src, dst in NODE_FIELDS.items():
                v = (item.get("values") or {}).get(src)
                if v is not None:
                    values[dst] = float(v)
            if values:
                ts = int(item["observed_at"])
                out.append((ts, values))
                newest = ts if newest is None else max(newest, ts)
        after, offset = int(data.get("next_after", after)), int(data.get("next_offset", 0))
        if not data.get("has_more", False):
            break
        time.sleep(sleep_s)
    print(f"node history: {len(out)} readings in {requests_made} requests (tip {tip})", file=sys.stderr)
    return out


def fetch_csv(path: str) -> list[tuple[int, dict]]:
    """Trainer-format CSV (`time_utc,temperature_c,pressure_hpa,humidity_pct`)."""
    out = []
    with open(path, newline="") as fh:
        for row in csv.DictReader(fh):
            ts = dt.datetime.fromisoformat(row["time_utc"].replace("Z", "+00:00"))
            values = {v: float(row[v]) for v in VARIABLES if (row.get(v) or "").strip()}
            if values:
                out.append((int(ts.timestamp()), values))
    return out


def cmd_fetch_observations(args) -> int:
    path = store_path(args.out, args.station)
    rows = load_store(path)
    if args.source == "public":
        new_rows = fetch_public(args.api, args.station)
    elif args.source == "node":
        new_rows = fetch_node(args.node, args.station, args.hours, args.bearer_env, args.sleep, args.max_requests)
    else:
        new_rows = fetch_csv(args.csv)
    added = merge_rows(rows, new_rows)
    save_store(path, rows)
    if rows:
        first, last = min(rows), max(rows)
        span_h = (last - first) / 3600
        have_hum = sum(1 for v in rows.values() if "humidity_pct" in v)
        print(f"{args.station}: +{added} new, {len(rows)} readings stored, span {span_h:.1f} h "
              f"({iso(first)} .. {iso(last)}), humidity present in {have_hum}/{len(rows)} -> {path}")
    else:
        print(f"{args.station}: no readings fetched")
    return 0


# ---------------------------------------------------------------------------
# Window building and prediction
# ---------------------------------------------------------------------------

def iso(ts: int) -> str:
    return dt.datetime.fromtimestamp(int(ts), dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def hourly_means(rows: dict[int, dict]) -> dict[int, dict]:
    """Average readings into their hour (unix hour -> {var: mean}); keeps only variables present."""
    buckets: dict[int, dict[str, list[float]]] = {}
    for ts, values in rows.items():
        slot = buckets.setdefault(ts // 3600, {})
        for var, v in values.items():
            slot.setdefault(var, []).append(v)
    return {h: {var: sum(xs) / len(xs) for var, xs in slot.items()} for h, slot in buckets.items()}


def build_window(hourly: dict[int, dict], end_hour: int | None = None):
    """Return (window_rows, end_hour, missing) for the 24 h ending at `end_hour` (inclusive)."""
    if not hourly:
        raise SystemExit("no observations in the store; run fetch-observations first")
    end_hour = max(hourly) if end_hour is None else end_hour
    rows, missing = [], []
    for h in range(end_hour - WINDOW_HOURS + 1, end_hour + 1):
        values = hourly.get(h)
        if values is None:
            missing.append(f"{iso(h * 3600)}: no reading")
            rows.append(None)
            continue
        absent = [v for v in VARIABLES if v not in values]
        if absent:
            missing.append(f"{iso(h * 3600)}: missing {','.join(absent)}")
        rows.append({"timestamp": h * 3600, "values": {k: values[k] for k in sorted(values)}})
    return rows, end_hour, missing


def persistence_trend(series: list[float], horizon_hours: float = HORIZON_HOURS) -> float:
    """`persistence-trend/v1` (tools/forecast-proto `forecast_variable`): last value plus the
    damped OLS slope over the window, hours since the first sample as the abscissa."""
    n = len(series)
    if n < 2:
        return series[-1]
    hours = list(range(n))
    mean_t = sum(hours) / n
    mean_y = sum(series) / n
    den = sum((t - mean_t) ** 2 for t in hours)
    slope = 0.0 if den == 0 else sum((t - mean_t) * (y - mean_y) for t, y in zip(hours, series)) / den
    return series[-1] + TREND_DAMPING * slope * horizon_hours


def model_variables(model_path: str) -> list[str]:
    """The variables a graph expects, from its `-export.json` parity record; default: the three canonical."""
    record = os.path.splitext(model_path)[0] + "-export.json"
    if os.path.exists(record):
        with open(record) as fh:
            return list(json.load(fh).get("variables", VARIABLES))
    return list(VARIABLES)


def run_onnx(model_path: str, window_rows: list[dict]) -> dict[str, float]:
    import numpy as np
    import onnxruntime as ort

    variables = model_variables(model_path)
    missing = [v for v in variables if any(v not in row["values"] for row in window_rows)]
    if missing:
        raise ValueError(f"window lacks model input(s) {missing}")
    x = np.asarray([[row["values"][v] for v in variables] for row in window_rows], dtype=np.float32)[None]
    session = ort.InferenceSession(model_path, providers=["CPUExecutionProvider"])
    name = session.get_inputs()[0].name
    y = session.run(None, {name: x})[0][0]
    if not all(math.isfinite(float(v)) for v in y):
        raise ValueError("non-finite model output")
    return {var: float(y[i]) for i, var in enumerate(variables)}


def cmd_predict(args) -> int:
    rows = load_store(store_path(args.observations, args.station))
    hourly = hourly_means(rows)
    end_hour = None
    if args.at:
        end_hour = int(dt.datetime.fromisoformat(args.at.replace("Z", "+00:00")).timestamp() // 3600)
    window, end_hour, missing = build_window(hourly, end_hour)
    issued_at = (end_hour + 1) * 3600 - 3600  # the last input hour, on the hour
    model_id, values, note = MODEL_PERSISTENCE, {}, ""
    needed = model_variables(args.model) if args.model and os.path.exists(args.model) else list(VARIABLES)
    complete = all(r is not None and all(v in r["values"] for v in needed) for r in window)
    if complete and args.model and os.path.exists(args.model):
        try:
            values = run_onnx(args.model, window)
            model_id = MODEL_LEARNED
        except Exception as exc:  # noqa: BLE001 — any model failure falls back, loudly
            note = f"learned model unavailable ({type(exc).__name__}: {exc}); using {MODEL_PERSISTENCE}"
    elif not complete:
        first = missing[0] if missing else f"model needs {needed}"
        note = f"window incomplete for the model ({len(missing)} problems, first: {first}); using {MODEL_PERSISTENCE} on the variables present"
    elif not args.model:
        note = f"no --model given; using {MODEL_PERSISTENCE}"
    else:
        note = f"model file {args.model} not found; using {MODEL_PERSISTENCE}"
    if model_id == MODEL_PERSISTENCE:
        present = [r for r in window if r is not None]
        if len(present) < 2:
            raise SystemExit("fewer than 2 hourly readings in the window; cannot forecast")
        for var in VARIABLES:
            series = [r["values"][var] for r in present if var in r["values"]]
            if len(series) >= 2:
                values[var] = persistence_trend(series)
    if not values:
        raise SystemExit("no variable could be forecast")
    forecast = {
        "model_id": model_id,
        "issued_at": issued_at,
        "valid_at": issued_at + HORIZON_HOURS * 3600,
        "horizon_hours": HORIZON_HOURS,
        "station_id": args.station,
        "values": {k: round(values[k], 4) for k in sorted(values)},
    }
    os.makedirs(args.out, exist_ok=True)
    with open(os.path.join(args.out, "forecast.json"), "w") as fh:
        json.dump(forecast, fh, indent=2)
    with open(os.path.join(args.out, "window.json"), "w") as fh:
        json.dump([r for r in window if r is not None], fh, indent=2)
    if note:
        print(f"note: {note}", file=sys.stderr)
    print(f"{model_id} forecast for {args.station}: issued {iso(issued_at)}, valid {iso(forecast['valid_at'])}, "
          f"values {forecast['values']} -> {os.path.join(args.out, 'forecast.json')}")
    return 0


# ---------------------------------------------------------------------------
# Commit / reveal preparation (files for `wrfcoin forecast commit|reveal`)
# ---------------------------------------------------------------------------

def write_executable(path: str, text: str) -> None:
    with open(path, "w") as fh:
        fh.write(text)
    os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR)


def cmd_prepare_commit(args) -> int:
    with open(args.forecast) as fh:
        forecast = json.load(fh)
    with open(args.window) as fh:
        window = json.load(fh)
    station_id = forecast["station_id"]
    variables = sorted(forecast["values"])
    manifest = {  # field order = ForecastArtifactManifest declaration order (digest depends on it)
        "schema_version": 1,
        "model_id": forecast["model_id"],
        "input_hash": sha3_hex(canonical_value_json(window).encode("utf-8")),
        "output_hash": sha3_hex(canonical_value_json(forecast).encode("utf-8")),
        "produced_at": int(forecast["issued_at"]),
        "region": station_id,
        "horizon_hours": int(forecast["horizon_hours"]),
        "input_variables": variables,
        "output_variables": variables,
    }
    timestamp = int(forecast["issued_at"])
    nonce = args.nonce or f"kit-{sha3_hex(canonical_value_json(forecast).encode())[:16]}-{timestamp}"
    commit_id = compute_commit_id(forecast, nonce, station_id, timestamp)
    os.makedirs(args.out, exist_ok=True)
    data_path, manifest_path = os.path.join(args.out, "data.json"), os.path.join(args.out, "manifest.json")
    with open(data_path, "w") as fh:
        fh.write(canonical_value_json(forecast) + "\n")
    with open(manifest_path, "w") as fh:
        fh.write(canonical_struct_json(manifest) + "\n")
    record = {
        "kind": "forecast", "forecaster": args.forecaster, "station_id": station_id,
        "nonce": nonce, "timestamp": timestamp, "commit_id": commit_id,
        "manifest_digest": manifest_digest(manifest), "valid_at": int(forecast["valid_at"]),
        "chain_id": args.chain_id, "network_id": args.network_id, "node_url": args.node,
        "files": {"data": os.path.abspath(data_path), "manifest": os.path.abspath(manifest_path)},
        "submitted": False,
        "note": "Prepared by forecast-kit. Nothing has been sent. commit.sh runs the CLI with --validate-only; "
                "the wallet owner removes that flag to submit. The CLI's commit_id must equal commit_id above.",
    }
    with open(os.path.join(args.out, "commit.json"), "w") as fh:
        json.dump(record, fh, indent=2)
    cli = shlex.quote(args.cli)
    common = (f"--forecaster {shlex.quote(args.forecaster)} --station-id {shlex.quote(station_id)} "
              f"--nonce {shlex.quote(nonce)} --timestamp {timestamp} --data-file {shlex.quote(record['files']['data'])}")
    commit_sh = f"""#!/bin/sh
# Generated by forecast-kit prepare-commit. VALIDATE ONLY: nothing is submitted.
# The wallet owner submits by running the same line WITHOUT --validate-only,
# with --signer-key pointing at the forecaster's hex private key file (mode 0600)
# and --token set to a Contributor bearer bound to that wallet (#3070).
set -eu
: "${{SIGNER_KEY:?set SIGNER_KEY to the forecaster private key file (mode 0600)}}"
{cli} --output json forecast commit --validate-only \\
  --node-url {shlex.quote(args.node)} --token "${{WRFCOIN_CONTRIBUTOR_TOKEN:-unused-for-validate-only}}" \\
  {common} \\
  --signer-key "$SIGNER_KEY" --chain-id {shlex.quote(args.chain_id)} --network-id {shlex.quote(args.network_id)}
"""
    write_executable(os.path.join(args.out, "commit.sh"), commit_sh)
    print(f"prepared commit for {station_id}: commit_id {commit_id}, nonce {nonce}, timestamp {timestamp}, "
          f"valid_at {iso(record['valid_at'])}; files in {args.out} (nothing submitted)")
    return 0


def cmd_prepare_reveal(args) -> int:
    with open(os.path.join(args.dir, "commit.json")) as fh:
        record = json.load(fh)
    with open(record["files"]["manifest"]) as fh:
        manifest = json.load(fh)
    now = int(time.time())
    problems = []
    if now < record["valid_at"] and not args.allow_early:
        problems.append(f"target time {iso(record['valid_at'])} not reached yet (now {iso(now)}); "
                        "reveal after it, or pass --allow-early for a baseline-scored reveal")
    if manifest_digest(manifest) != record["manifest_digest"]:
        problems.append("manifest.json changed since prepare-commit (digest mismatch)")
    receipt = None
    if args.receipt:
        with open(args.receipt) as fh:
            receipt = json.load(fh)
        if receipt.get("commit_id") != record["commit_id"]:
            problems.append("receipt.commit_id does not bind this commit")
        if receipt.get("manifest_digest") != record["manifest_digest"]:
            problems.append("receipt.manifest_digest does not bind this manifest")
        if not receipt.get("signature") or not (receipt.get("nonce") or "").strip():
            problems.append("receipt must be signed and carry a nonce")
        skill = (receipt.get("metrics") or {}).get("skill_score")
        if skill is not None and not (0.0 <= float(skill) <= 1.0):
            problems.append("receipt skill_score outside [0,1]")
    if args.cli_output:
        with open(args.cli_output) as fh:
            cli_out = json.load(fh)
        # `wrfcoin --output json` wraps the command result in {"command","data",...}.
        payload = cli_out.get("data") if isinstance(cli_out.get("data"), dict) else cli_out
        cli_id = payload.get("commit_id") or (payload.get("body") or {}).get("commit_hash")
        if cli_id != record["commit_id"]:
            problems.append(f"CLI commit_id {cli_id} differs from the kit's {record['commit_id']}")
    for p in problems:
        print(f"problem: {p}", file=sys.stderr)
    cli = shlex.quote(args.cli)
    common = (f"--forecaster {shlex.quote(record['forecaster'])} --station-id {shlex.quote(record['station_id'])} "
              f"--nonce {shlex.quote(record['nonce'])} --timestamp {record['timestamp']} "
              f"--data-file {shlex.quote(record['files']['data'])} --manifest-file {shlex.quote(record['files']['manifest'])}")
    receipt_arg = f" \\\n  --receipt-file {shlex.quote(os.path.abspath(args.receipt))}" if args.receipt else ""
    reveal_sh = f"""#!/bin/sh
# Generated by forecast-kit prepare-reveal. VALIDATE ONLY: nothing is submitted.
# The wallet owner submits by running the same line WITHOUT --validate-only and
# with --token set to the Contributor bearer bound to the forecaster wallet.
# Without a signed oracle receipt the reveal scores at the baseline (0.1).
set -eu
{cli} --output json forecast reveal --validate-only \\
  --node-url {shlex.quote(record['node_url'])} --token "${{WRFCOIN_CONTRIBUTOR_TOKEN:-unused-for-validate-only}}" \\
  {common}{receipt_arg}
"""
    write_executable(os.path.join(args.dir, "reveal.sh"), reveal_sh)
    record["reveal_prepared_at"] = now
    record["receipt_file"] = os.path.abspath(args.receipt) if args.receipt else None
    with open(os.path.join(args.dir, "commit.json"), "w") as fh:
        json.dump(record, fh, indent=2)
    status = "with problems listed above" if problems else "ok"
    print(f"prepared reveal for commit {record['commit_id']} ({status}); "
          f"{'receipt attached' if receipt else 'no receipt: baseline scoring'}; "
          f"{os.path.join(args.dir, 'reveal.sh')} validates only (nothing submitted)")
    return 1 if problems and not args.allow_early else 0


# ---------------------------------------------------------------------------
# Scoring: only the variables the forecast declares AND the station reports
# ---------------------------------------------------------------------------

def scored_variables(declared, reported) -> list[str]:
    """Sorted intersection — `wrfcoin_forecast_score::scored_variables` (owner ruling 2026-10-19)."""
    reported = set(reported)
    return sorted({v for v in declared if v in reported})


def receipt_rule_score(forecast_values: dict, truth_values: dict, climatology: dict) -> dict:
    """The prototype's receipt rule (`tools/forecast-proto` `score`): per scored variable
    `max(0, 1 - |e| / |e_climatology|)`; MAE/RMSE/bias and the mean skill over the scored set."""
    variables = scored_variables(forecast_values, truth_values)
    if not variables:
        raise ValueError(f"nothing to score: forecast declares {sorted(forecast_values)}, truth reports {sorted(truth_values)}")
    per_variable, errs, skills = {}, [], []
    eps = 2.220446049250313e-16
    for var in variables:
        f, t = float(forecast_values[var]), float(truth_values[var])
        c = float(climatology.get(var, f))
        err, clim_err = f - t, abs(c - t)
        if clim_err <= eps:
            skill = 1.0 if abs(err) <= eps else 0.0
        else:
            skill = min(1.0, max(0.0, 1.0 - abs(err) / clim_err))
        per_variable[var] = {"forecast": f, "truth": t, "climatology": c, "abs_error": abs(err),
                             "climatology_abs_error": clim_err, "skill": skill}
        errs.append(err)
        skills.append(skill)
    n = len(errs)
    metrics = {"mae": sum(abs(e) for e in errs) / n, "rmse": math.sqrt(sum(e * e for e in errs) / n),
               "bias": sum(errs) / n, "skill_score": min(1.0, max(0.0, sum(skills) / n))}
    return {"scored_variables": variables, "per_variable": per_variable, "metrics": metrics}


def cmd_score(args) -> int:
    with open(args.forecast) as fh:
        forecast = json.load(fh)
    with open(args.window) as fh:
        window = json.load(fh)
    rows = load_store(store_path(args.observations, args.station))
    hourly = hourly_means(rows)
    target_hour = int(forecast["valid_at"]) // 3600
    truth = hourly.get(target_hour)
    if truth is None:
        print(f"no reading stored for the target hour {iso(target_hour * 3600)}; fetch-observations again after it", file=sys.stderr)
        return 1
    climatology = {}
    for var in forecast["values"]:
        xs = [r["values"][var] for r in window if var in r["values"]]
        if xs:
            climatology[var] = sum(xs) / len(xs)
    try:
        report = receipt_rule_score(forecast["values"], truth, climatology)
    except ValueError as exc:
        print(f"problem: {exc}", file=sys.stderr)
        return 1
    report.update({"station_id": forecast["station_id"], "model_id": forecast["model_id"],
                   "valid_at": int(forecast["valid_at"]), "truth_hour": iso(target_hour * 3600),
                   "declared_variables": sorted(forecast["values"]), "reported_variables": sorted(truth),
                   "rule": "per scored variable max(0, 1 - |e|/|e_window-climatology|), mean over the "
                           "variables the forecast declares AND the truth reports (owner ruling 2026-10-19)"})
    os.makedirs(args.out, exist_ok=True)
    with open(os.path.join(args.out, "score.json"), "w") as fh:
        json.dump(report, fh, indent=2)
    # The metrics file an oracle operator hands to `wrfcoin forecast sign-receipt` (#3149).
    with open(os.path.join(args.out, "metrics.json"), "w") as fh:
        json.dump(report["metrics"], fh, indent=2)
    m = report["metrics"]
    print(f"scored {forecast['station_id']} over {report['scored_variables']}: skill_score {m['skill_score']:.4f} "
          f"(mae {m['mae']:.4f}, rmse {m['rmse']:.4f}, bias {m['bias']:+.4f}); "
          f"declared {report['declared_variables']}, reported {report['reported_variables']} -> {args.out}/metrics.json")
    return 0


def cmd_export_csv(args) -> int:
    rows = load_store(store_path(args.observations, args.station))
    hourly = hourly_means(rows)
    if not hourly:
        raise SystemExit("empty store")
    os.makedirs(args.out, exist_ok=True)
    path = os.path.join(args.out, f"{args.station}.csv")
    with open(path, "w", newline="") as fh:
        writer = csv.writer(fh)
        writer.writerow(["time_utc"] + VARIABLES)
        for h in sorted(hourly):
            writer.writerow([iso(h * 3600)] + [f"{hourly[h][v]:.2f}" if v in hourly[h] else "" for v in VARIABLES])
    present = {v: sum(1 for h in hourly.values() if v in h) for v in VARIABLES}
    print(f"{args.station}: {len(hourly)} hourly rows, variables present {present} -> {path}")
    return 0


def cmd_status(args) -> int:
    doc = http_get_json(f"{args.node.rstrip('/')}/transactions/commit/{urllib.parse.quote(args.commit_id)}/status")
    print(json.dumps(doc, indent=2))
    return 0


# ---------------------------------------------------------------------------

def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    f = sub.add_parser("fetch-observations", help="pull a station's readings into a local store")
    f.add_argument("--station", required=True, help="public alias (st_…) for --source public; on-chain station_id for --source node")
    f.add_argument("--source", choices=["public", "node", "csv"], default="public")
    f.add_argument("--out", required=True, help="store directory")
    f.add_argument("--api", default=DEFAULT_PUBLIC_API, help="public backend base URL")
    f.add_argument("--node", default=DEFAULT_NODE, help="node base URL for --source node")
    f.add_argument("--hours", type=int, default=240, help="history to cover for --source node")
    f.add_argument("--bearer-env", default=READER_TOKEN_ENV, help="env var holding the Reader bearer (value never printed)")
    f.add_argument("--sleep", type=float, default=0.2, help="pause between node pages")
    f.add_argument("--max-requests", type=int, default=2000)
    f.add_argument("--csv", help="trainer-format CSV for --source csv")
    f.set_defaults(func=cmd_fetch_observations)

    p = sub.add_parser("predict", help="build the 24 h window and run the model (CPU)")
    p.add_argument("--observations", required=True, help="store directory from fetch-observations")
    p.add_argument("--station", required=True, help="station as stored (and as the commit's station_id)")
    p.add_argument("--model", help="ONNX model path (learned-conv1d/v1); omit for persistence-trend/v1")
    p.add_argument("--at", help="end of the input window (ISO-8601 UTC); default: newest hour in the store")
    p.add_argument("--out", required=True)
    p.set_defaults(func=cmd_predict)

    c = sub.add_parser("prepare-commit", help="write data.json, manifest.json, commit.json, commit.sh")
    c.add_argument("--forecast", required=True, help="forecast.json from predict")
    c.add_argument("--window", required=True, help="window.json from predict")
    c.add_argument("--forecaster", required=True, help="forecaster wallet address (the bearer's wallet, #3070)")
    c.add_argument("--nonce", help="commit nonce; default derives from the forecast digest and timestamp")
    c.add_argument("--chain-id", default="777")
    c.add_argument("--network-id", default="testnet")
    c.add_argument("--node", default=DEFAULT_NODE)
    c.add_argument("--cli", default="wrfcoin", help="path to the wrfcoin CLI binary used in the generated scripts")
    c.add_argument("--out", required=True)
    c.set_defaults(func=cmd_prepare_commit)

    r = sub.add_parser("prepare-reveal", help="check timing/receipt binding and write reveal.sh")
    r.add_argument("--dir", required=True, help="directory from prepare-commit")
    r.add_argument("--receipt", help="signed ForecastVerificationReceipt JSON from the oracle (optional)")
    r.add_argument("--cli-output", help="JSON printed by `wrfcoin forecast commit --validate-only` to cross-check the commit id")
    r.add_argument("--allow-early", action="store_true", help="write reveal.sh even before valid_at (baseline scoring)")
    r.add_argument("--cli", default="wrfcoin")
    r.set_defaults(func=cmd_prepare_reveal)

    sc = sub.add_parser("score", help="score a forecast against the stored truth over declared-and-reported variables")
    sc.add_argument("--forecast", required=True, help="forecast.json from predict")
    sc.add_argument("--window", required=True, help="window.json from predict (climatology reference)")
    sc.add_argument("--observations", required=True, help="store directory holding the truth reading")
    sc.add_argument("--station", required=True)
    sc.add_argument("--out", required=True, help="writes score.json and metrics.json")
    sc.set_defaults(func=cmd_score)

    x = sub.add_parser("export-csv", help="write a station's store as the trainer's hourly CSV")
    x.add_argument("--observations", required=True)
    x.add_argument("--station", required=True)
    x.add_argument("--out", required=True)
    x.set_defaults(func=cmd_export_csv)

    s = sub.add_parser("status", help="GET /transactions/commit/<id>/status (no token needed)")
    s.add_argument("--commit-id", required=True)
    s.add_argument("--node", default=DEFAULT_NODE)
    s.set_defaults(func=cmd_status)

    args = ap.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
