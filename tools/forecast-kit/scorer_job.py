#!/usr/bin/env python3
"""Oracle-operated forecast scorer job (epic #3604, story A1). Runbook: docs/testnet/forecast-scorer-job.md.

Revealed forecasts in, signed verification receipts out. Run by an ORACLE OPERATOR whose key is in the
chain's oracle set: consensus mints a reveal at whatever `skill_score` an authorized, signed receipt carries
(and at the 0.1 baseline without one), so this job decides what forecasters are paid. It refuses to run
unless every gate below is satisfied:

  FORECAST_SIGNER_ENABLED=1   the operator switched the job on;
  SKILL_DEFINITION            rmse_sigma | mae_clim, set explicitly (there is NO default: which definition a
                              receipt carries is an owner decision, see the runbook's comparison);
  key binding active          the node reports `forecaster_key_binding` active for the next block
                              (GET /network/upgrade); without it a gossiped commit can name any forecaster,
                              and the owner ruled that receipts wait for the binding (#3290).

Optional: FORECAST_ANTI_GAMING=1 (story A6, #3244) measures skill against the best trivial baseline
(climatology, persistence, and any --reference-forecasts value such as a NOAA point forecast) and scores a
variable zero when the forecast copies one of them within FORECAST_COPY_TOLERANCE (per-variable defaults below).

Pipeline: verify every reveal record against its own files (commit id, manifest digest, region), pair it with
the scorer's OWN truth store (the truth reading at valid_at and the 24 h window before issued_at, from
`forecast_kit.py fetch-observations`), score with the `forecast-score` crate (`forecast-score-batch`), apply
the cap per cell and lead time, then write metrics.json + score.json per selected forecast and run (or, with
--dry-run, only print) `wrfcoin forecast sign-receipt`. Receipts are never submitted: the forecaster attaches
one to its own reveal.

Never point this at the live chain without the owner's go-ahead; tests use fixtures only.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import shlex
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("forecast_kit", os.path.join(HERE, "forecast_kit.py"))
kit = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(kit)

SKILL_DEFINITIONS = ("rmse_sigma", "mae_clim")
KEY_BINDING_FEATURE = "forecaster_key_binding"
BASELINE_FORECAST_SKILL = 0.1  # shared-protocol proof_of_publishing BASELINE_FORECAST_SKILL
DEFAULT_CAP_PER_CELL_LEAD = 3
# A6 copy tolerances in each variable's units: about the resolution a forecaster would round to.
DEFAULT_COPY_TOLERANCE = {"temperature_c": 0.05, "pressure_hpa": 0.05, "humidity_pct": 0.5}
WINDOW_HOURS = kit.WINDOW_HOURS

EXIT_DISABLED, EXIT_CONFIG, EXIT_BINDING, EXIT_INPUT = 3, 2, 4, 5


class Refusal(Exception):
    def __init__(self, code: int, message: str):
        super().__init__(message)
        self.code = code


# ---------------------------------------------------------------------------
# Gates
# ---------------------------------------------------------------------------

def anti_gaming_config(env) -> dict:
    """FORECAST_ANTI_GAMING=1 switches A6 on; FORECAST_COPY_TOLERANCE is an optional JSON object override."""
    flag = env.get("FORECAST_ANTI_GAMING", "")
    if flag not in ("", "0", "1"):
        raise Refusal(EXIT_CONFIG, f"FORECAST_ANTI_GAMING must be 0 or 1 (got {flag!r})")
    tolerance = dict(DEFAULT_COPY_TOLERANCE)
    if env.get("FORECAST_COPY_TOLERANCE"):
        try:
            override = json.loads(env["FORECAST_COPY_TOLERANCE"])
            tolerance.update({str(k): float(v) for k, v in override.items()})
        except (ValueError, AttributeError, TypeError) as exc:
            raise Refusal(EXIT_CONFIG, f"FORECAST_COPY_TOLERANCE is not a JSON object of numbers: {exc}") from None
    if any(not (v >= 0) for v in tolerance.values()):
        raise Refusal(EXIT_CONFIG, "copy tolerances must be non-negative")
    return {"enabled": flag == "1", "copy_tolerance": tolerance}


def check_env(env) -> str:
    """Return the configured skill definition, or raise Refusal."""
    if env.get("FORECAST_SIGNER_ENABLED") != "1":
        raise Refusal(EXIT_DISABLED, "FORECAST_SIGNER_ENABLED is not 1: the scorer job is switched off")
    definition = (env.get("SKILL_DEFINITION") or "").strip()
    if definition not in SKILL_DEFINITIONS:
        raise Refusal(EXIT_CONFIG, f"SKILL_DEFINITION must be set explicitly to one of {', '.join(SKILL_DEFINITIONS)} "
                                   f"(got {definition!r}); there is no default")
    return definition


def key_binding_active(upgrade: dict) -> bool:
    """True when GET /network/upgrade reports forecaster_key_binding in force for the next block."""
    entries = ((upgrade or {}).get("scheduled") or {}).get("entries") or []
    return any(e.get("feature") == KEY_BINDING_FEATURE and e.get("known") is True and e.get("active_at_next") is True
               for e in entries)


def fetch_upgrade(node: str, status_file: str | None) -> dict:
    if status_file:
        with open(status_file) as fh:
            body = json.load(fh)
    else:
        body = kit.http_get_json(node.rstrip("/") + "/network/upgrade")
    return body.get("data", body) if isinstance(body, dict) else {}


# ---------------------------------------------------------------------------
# Inputs: reveal records and the scorer's own truth store
# ---------------------------------------------------------------------------

def is_address(value: str) -> bool:
    v = value[2:] if value.startswith(("0x", "0X")) else value
    return len(v) == 40 and all(c in "0123456789abcdefABCDEF" for c in v)


def load_reveal(record_dir: str) -> dict:
    """One prepared forecast (forecast_kit prepare-commit output), checked against its own files."""
    with open(os.path.join(record_dir, "commit.json")) as fh:
        record = json.load(fh)
    with open(record["files"]["data"]) as fh:
        data = json.load(fh)
    with open(record["files"]["manifest"]) as fh:
        manifest = json.load(fh)
    problems = []
    derived = kit.compute_commit_id(data, record["nonce"], record["station_id"], int(record["timestamp"]))
    if derived != record["commit_id"]:
        problems.append(f"commit_id {record['commit_id']} != derived {derived}")
    if kit.manifest_digest(manifest) != record.get("manifest_digest"):
        problems.append("manifest digest does not match the record")
    if manifest.get("region") != record["station_id"] or data.get("station_id") != record["station_id"]:
        problems.append("manifest region / data station_id do not name the record's station")
    if sorted(data.get("values", {})) != manifest.get("output_variables"):
        problems.append("manifest output_variables are not the forecast's variables")
    if not is_address(str(record.get("forecaster", ""))):
        problems.append("forecaster is not a 20-byte address (key binding requires a key-derived address)")
    return {"dir": record_dir, "record": record, "data": data, "manifest": manifest, "problems": problems}


def load_reference_forecasts(path: str | None) -> dict:
    """{station_id: {str(valid_at): {variable: value | [values]}}} — external forecasts for the copy check."""
    if not path:
        return {}
    with open(path) as fh:
        return json.load(fh)


def truth_inputs(reveal: dict, observations_dir: str, references: dict | None = None) -> tuple[dict, list[str]]:
    """Per declared variable: forecast, truth at valid_at, and the WINDOW_HOURS hourly means before issued_at."""
    data = reveal["data"]
    hourly = kit.hourly_means(kit.load_store(kit.store_path(observations_dir, data["station_id"])))
    truth = hourly.get(int(data["valid_at"]) // 3600)
    if truth is None:
        return {}, ["no truth reading stored for valid_at"]
    issue_hour = int(data["issued_at"]) // 3600
    window_hours = range(issue_hour - WINDOW_HOURS, issue_hour)
    variables, notes = {}, []
    for var in kit.scored_variables(data["values"], truth):
        window = [hourly[h][var] for h in window_hours if h in hourly and var in hourly[h]]
        if len(window) < 2:
            notes.append(f"{var}: fewer than 2 window readings; not scored")
            continue
        entry = {"forecast": float(data["values"][var]), "truth": float(truth[var]), "window": window}
        ref = ((references or {}).get(data["station_id"]) or {}).get(str(int(data["valid_at"])), {}).get(var)
        if ref is not None:
            entry["reference_forecasts"] = [float(x) for x in (ref if isinstance(ref, list) else [ref])]
        variables[var] = entry
    if not variables:
        notes.append("nothing to score (declared ∩ reported variables with a window is empty)")
    return variables, notes


def build_request(reveals: list[dict], observations_dir: str, definition: str, cap: int,
                  anti_gaming: dict | None = None, references: dict | None = None) -> tuple[dict, dict]:
    forecasts, skipped = [], {}
    for r in reveals:
        if r["problems"]:
            skipped[r["record"]["commit_id"]] = r["problems"]
            continue
        variables, notes = truth_inputs(r, observations_dir, references)
        if not variables:
            skipped[r["record"]["commit_id"]] = notes
            continue
        forecasts.append({"commit_id": r["record"]["commit_id"], "forecaster": r["record"]["forecaster"],
                          "cell": r["record"]["station_id"], "horizon_hours": int(r["manifest"]["horizon_hours"]),
                          "valid_at": int(r["data"]["valid_at"]), "variables": variables})
    request = {"skill_definition": definition, "cap_per_cell_lead": cap,
               "baseline": BASELINE_FORECAST_SKILL, "forecasts": forecasts,
               "anti_gaming": anti_gaming or {"enabled": False, "copy_tolerance": {}}}
    return request, skipped


def run_scorer(scorer_bin: str, request: dict) -> dict:
    proc = subprocess.run([scorer_bin], input=json.dumps(request), capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        raise Refusal(EXIT_INPUT, f"forecast-score-batch failed: {proc.stderr.strip()}")
    return json.loads(proc.stdout)


# ---------------------------------------------------------------------------
# Receipts
# ---------------------------------------------------------------------------

def sign_command(args, reveal: dict, metrics_path: str, receipt_path: str) -> list[str]:
    rec = reveal["record"]
    cmd = [args.cli, "--output", "json", "forecast", "sign-receipt",
           "--manifest-file", rec["files"]["manifest"], "--metrics-file", metrics_path,
           "--forecaster", rec["forecaster"], "--station-id", rec["station_id"],
           "--data-file", rec["files"]["data"], "--nonce", rec["nonce"], "--timestamp", str(rec["timestamp"]),
           "--chain-id", args.chain_id, "--network-id", args.network_id, "--out", receipt_path]
    if args.rehearsal_oracle_key:
        cmd += ["--oracle-key", args.rehearsal_oracle_key, "--rehearsal"]
    return cmd


def run(args, env=os.environ) -> int:
    definition = check_env(env)
    anti_gaming = anti_gaming_config(env)
    if not key_binding_active(fetch_upgrade(args.node, args.node_status_file)):
        raise Refusal(EXIT_BINDING, f"node does not report {KEY_BINDING_FEATURE} active for the next block; "
                                    "receipts wait for the forecaster key binding (#3290, owner ruling)")
    scorer_bin = args.scorer_bin or env.get("FORECAST_SCORER_BIN") or shutil.which("forecast-score-batch")
    if not scorer_bin:
        raise Refusal(EXIT_CONFIG, "forecast-score-batch not found (build it: cargo build -p wrfcoin-forecast-score "
                                   "--bin forecast-score-batch) and pass --scorer-bin or FORECAST_SCORER_BIN")
    record_dirs = sorted(os.path.join(args.reveals, d) for d in os.listdir(args.reveals)
                         if os.path.isfile(os.path.join(args.reveals, d, "commit.json")))
    reveals = [load_reveal(d) for d in record_dirs]
    request, skipped = build_request(reveals, args.observations, definition, args.cap_per_cell_lead,
                                     anti_gaming, load_reference_forecasts(args.reference_forecasts))
    response = run_scorer(scorer_bin, request) if request["forecasts"] else {"forecasts": []}
    by_id = {r["record"]["commit_id"]: r for r in reveals}

    os.makedirs(args.out, exist_ok=True)
    summary = {"skill_definition": definition, "anti_gaming": anti_gaming["enabled"], "dry_run": args.dry_run, "cap_per_cell_lead": args.cap_per_cell_lead,
               "skipped": skipped, "forecasts": []}
    signed = 0
    for item in response["forecasts"]:
        cid = item["commit_id"]
        decision = item.get("decision") or {"selected": False, "reason": item.get("error", "not scored")}
        entry = {"commit_id": cid, "decision": decision.get("reason")}
        if "score" in item:
            entry["skill_score"] = item["score"]["metrics"]["skill_score"]
        if decision.get("selected"):
            out_dir = os.path.join(args.out, cid)
            os.makedirs(out_dir, exist_ok=True)
            metrics_path = os.path.join(out_dir, "metrics.json")
            with open(metrics_path, "w") as fh:
                json.dump(item["score"]["metrics"], fh, indent=2)
            with open(os.path.join(out_dir, "score.json"), "w") as fh:
                json.dump(item["score"], fh, indent=2)
            cmd = sign_command(args, by_id[cid], metrics_path, os.path.join(out_dir, "receipt.json"))
            entry["sign_command"] = " ".join(shlex.quote(c) for c in cmd)
            if not args.dry_run:
                proc = subprocess.run(cmd, capture_output=True, text=True, check=False)
                entry["signed"] = proc.returncode == 0
                if proc.returncode != 0:
                    entry["sign_error"] = proc.stderr.strip()[-500:]
                signed += proc.returncode == 0
        summary["forecasts"].append(entry)
    with open(os.path.join(args.out, "summary.json"), "w") as fh:
        json.dump(summary, fh, indent=2)
    selected = sum(1 for f in summary["forecasts"] if f["decision"] == "selected")
    print(f"scored {len(response['forecasts'])} forecast(s) under {definition}"
          f"{' with anti-gaming' if anti_gaming['enabled'] else ''}; {selected} selected for a receipt, "
          f"{len(skipped)} skipped; {'dry run: nothing signed' if args.dry_run else f'{signed} receipt(s) signed'}; "
          f"summary -> {os.path.join(args.out, 'summary.json')}")
    return 0


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("--reveals", required=True, help="directory of forecast_kit prepare-commit record directories")
    p.add_argument("--observations", required=True, help="the scorer's own forecast_kit observation store")
    p.add_argument("--node", default=kit.DEFAULT_NODE, help="node base URL for GET /network/upgrade")
    p.add_argument("--node-status-file", help="read /network/upgrade JSON from a file (fixtures, air-gapped runs)")
    p.add_argument("--out", required=True)
    p.add_argument("--chain-id", required=True)
    p.add_argument("--network-id", required=True)
    p.add_argument("--cap-per-cell-lead", type=int, default=DEFAULT_CAP_PER_CELL_LEAD)
    p.add_argument("--reference-forecasts", help="JSON of external reference forecasts for the A6 copy check")
    p.add_argument("--scorer-bin", help="path to forecast-score-batch")
    p.add_argument("--cli", default="wrfcoin")
    p.add_argument("--rehearsal-oracle-key", help="debug-build file key for a rehearsal (never production)")
    p.add_argument("--dry-run", action="store_true", help="score and write the sign commands; sign nothing")
    return p


def main(argv=None) -> int:
    args = parser().parse_args(argv)
    try:
        return run(args)
    except Refusal as refusal:
        print(f"refused: {refusal}", file=sys.stderr)
        return refusal.code


if __name__ == "__main__":
    sys.exit(main())
