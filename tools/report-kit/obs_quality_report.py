#!/usr/bin/env python3
"""Daily regional observation-quality report: the first report work unit (epic #3604, story B3).

A deterministic, reproducible job over data already on chain (accepted contribution reveals and their oracle
attestation quorum). It emits the work-unit documents from the draft ADR
`docs/adr/2026-10-28-3604-report-work-unit.md` (wrfcoin/core4#52): an input manifest, a job description and an
output, each with a SHA3-256 digest over a domain-separated, length-prefixed body (the `BodyBuilder` encoding:
u64 little-endian length prefixes, `crypto::domain_signing::push_length_prefixed`). Two runs on the same inputs
produce the same digests, byte for byte, on any machine: every number in the summary is a fixed-point integer,
and every input is put in canonical order before anything is computed.

There is NO payment path. A report is paid only through the ratified work-unit ADR and a verifier pool
(owner ruling 2026-10-24); this job only makes the unit reproducible so a verifier could rerun it.

Inputs: one JSON file per station holding the node's `GET /transactions/contributions/<station_id>/observations`
items (`{"commit_id", "station_id", "height", "observed_at", "values": {...}, "attestation": {...}}`), the same
shape whether fetched page by page from a node or exported from an archive. Only items whose `height` is in the
manifest range are read.

Usage:
  python tools/report-kit/obs_quality_report.py run --inputs DIR --from-height A --to-height B \
      --chain-id ID --network-id NET [--stations s1,s2|'*'] [--variables v1,v2] --out DIR
  python tools/report-kit/obs_quality_report.py verify --unit DIR --inputs DIR   # rerun and compare digests

Only regional aggregates are published (per-station drift findings are a separate, owner-approved step).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import struct
import sys

JOB_TYPE = "obs_quality_daily/v1"
RUNNER = {"kind": "builtin", "name": "report-kit/obs_quality_report.py", "version": "1"}
SCHEMA_VERSION = 1
DEFAULT_VARIABLES = ("humidity_pct", "pressure_hpa", "temperature_c")
# Node route field names -> canonical variables (forecast_kit NODE_FIELDS).
NODE_FIELDS = {"temperature_c": "temperature_c", "pressure_hpa": "pressure_hpa", "humidity_percent": "humidity_pct",
               "humidity_pct": "humidity_pct"}
# Physical plausibility bounds per variable (inclusive), in canonical units.
PHYSICAL_BOUNDS = {"temperature_c": (-90.0, 60.0), "pressure_hpa": (850.0, 1090.0), "humidity_pct": (0.0, 100.0)}
OUTLIER_MAD_FACTOR = 5          # |x - median| > 5 * MAD (scaled) within a station's own series
DRIFT_THRESHOLD = {"temperature_c": 1.5, "pressure_hpa": 1.5, "humidity_pct": 8.0}  # |median residual| flagged
MIN_STATIONS_FOR_DRIFT = 3      # a regional median needs at least three stations in the same hour
MICRO = 1_000_000

DOMAIN_INPUT = b"wrfcoin.work_unit.input_manifest.v1"
DOMAIN_JOB = b"wrfcoin.work_unit.job.v1"
DOMAIN_OUTPUT = b"wrfcoin.work_unit.output.v1"
DOMAIN_UNIT = b"wrfcoin.work_unit.v1"
DOMAIN_INPUTS = b"wrfcoin.work_unit.chain_inputs.v1"


# ---------------------------------------------------------------------------
# Canonical encoding (BodyBuilder-compatible)
# ---------------------------------------------------------------------------

class Body:
    def __init__(self):
        self.buf = bytearray()

    def bytes_(self, b: bytes) -> "Body":
        self.buf += struct.pack("<Q", len(b)) + b
        return self

    def str_(self, s: str) -> "Body":
        return self.bytes_(s.encode("utf-8"))

    def u64(self, v: int) -> "Body":
        self.buf += struct.pack("<Q", v)
        return self

    def i64(self, v: int) -> "Body":
        self.buf += struct.pack("<q", v)
        return self

    def strs(self, items) -> "Body":
        items = list(items)
        self.u64(len(items))
        for s in items:
            self.str_(s)
        return self


def digest(domain: bytes, body: Body) -> str:
    pre = struct.pack("<Q", len(domain)) + domain + struct.pack("<Q", len(body.buf)) + bytes(body.buf)
    return hashlib.sha3_256(pre).hexdigest()


def micro(x: float) -> int:
    """Fixed-point (1e-6) with round-half-away-from-zero, so the summary never depends on float printing."""
    scaled = abs(x) * MICRO
    q = int(scaled)
    if scaled - q >= 0.5:
        q += 1
    return q if x >= 0 else -q


# ---------------------------------------------------------------------------
# Inputs
# ---------------------------------------------------------------------------

def load_inputs(inputs_dir: str, from_height: int, to_height: int, stations, variables) -> list[dict]:
    """Every in-range reading, canonical: (height, station_id, commit_id) order, canonical variable names."""
    rows = []
    for name in sorted(os.listdir(inputs_dir)):
        if not name.endswith(".json"):
            continue
        with open(os.path.join(inputs_dir, name)) as fh:
            body = json.load(fh)
        items = body.get("data", body).get("items", body) if isinstance(body, dict) else body
        for item in items:
            h = int(item["height"])
            if not (from_height <= h <= to_height):
                continue
            if stations != "*" and item["station_id"] not in stations:
                continue
            values = {}
            for field, v in (item.get("values") or {}).items():
                var = NODE_FIELDS.get(field)
                if var in variables and v is not None:
                    values[var] = float(v)
            att = item.get("attestation") or {}
            rows.append({"height": h, "station_id": item["station_id"], "commit_id": item["commit_id"],
                         "observed_at": int(item["observed_at"]), "values": values,
                         "attested": bool(att.get("has_quorum"))})
    rows.sort(key=lambda r: (r["height"], r["station_id"], r["commit_id"]))
    seen, unique = set(), []
    for r in rows:  # the same reveal exported twice is one reading
        if r["commit_id"] not in seen:
            seen.add(r["commit_id"])
            unique.append(r)
    return unique


def inputs_digest(rows: list[dict], variables) -> str:
    """ADR §1.4: the canonical chain-input extraction, floats as IEEE-754 bit patterns."""
    body = Body().u64(len(rows))
    for r in rows:
        body.u64(r["height"]).str_(r["station_id"]).str_(r["commit_id"]).i64(r["observed_at"])
        body.u64(1 if r["attested"] else 0)
        for var in variables:
            v = r["values"].get(var)
            if v is None:
                body.u64(0)
            else:
                body.u64(1).bytes_(struct.pack("<d", v))
    return digest(DOMAIN_INPUTS, body)


# ---------------------------------------------------------------------------
# The report
# ---------------------------------------------------------------------------

def median(xs: list[float]) -> float:
    s = sorted(xs)
    n = len(s)
    return s[n // 2] if n % 2 else (s[n // 2 - 1] + s[n // 2]) / 2


def compute(rows: list[dict], variables) -> list[tuple[str, int]]:
    """The summary vector: (name, fixed-point value) pairs in a fixed order. Regional aggregates only."""
    stations = sorted({r["station_id"] for r in rows})
    out: list[tuple[str, int]] = [("stations", len(stations)), ("readings", len(rows)),
                                  ("attested_readings", sum(r["attested"] for r in rows))]
    if not rows:
        return out
    first_hour = min(r["observed_at"] for r in rows) // 3600
    last_hour = max(r["observed_at"] for r in rows) // 3600
    span = last_hour - first_hour + 1
    hours_by_station = {s: set() for s in stations}
    for r in rows:
        hours_by_station[r["station_id"]].add(r["observed_at"] // 3600)
    coverage = [len(hours_by_station[s]) / span for s in stations]
    out += [("span_hours", span), ("coverage_mean_micro", micro(sum(coverage) / len(coverage))),
            ("coverage_min_micro", micro(min(coverage))),
            ("stations_below_half_coverage", sum(c < 0.5 for c in coverage))]
    # Hourly means per station and variable: the unit drift and outlier checks work on.
    hourly: dict[tuple[str, int], dict[str, list[float]]] = {}
    for r in rows:
        slot = hourly.setdefault((r["station_id"], r["observed_at"] // 3600), {})
        for var, v in r["values"].items():
            slot.setdefault(var, []).append(v)
    for var in variables:
        lo, hi = PHYSICAL_BOUNDS[var]
        values = [r["values"][var] for r in rows if var in r["values"]]
        out.append((f"{var}.readings", len(values)))
        out.append((f"{var}.out_of_bounds", sum(not (lo <= v <= hi) for v in values)))
        spikes = 0
        for s in stations:
            series = [r["values"][var] for r in rows if r["station_id"] == s and var in r["values"]]
            if len(series) < 5:
                continue
            m = median(series)
            mad = median([abs(x - m) for x in series]) * 1.4826
            if mad > 0:
                spikes += sum(abs(x - m) > OUTLIER_MAD_FACTOR * mad for x in series)
        out.append((f"{var}.spikes", spikes))
        # Drift: each station's median residual against the regional hourly median (>= 3 stations that hour).
        residuals = {s: [] for s in stations}
        for hour in range(first_hour, last_hour + 1):
            present = {s: sum(hourly[(s, hour)][var]) / len(hourly[(s, hour)][var])
                       for s in stations if (s, hour) in hourly and var in hourly[(s, hour)]}
            if len(present) < MIN_STATIONS_FOR_DRIFT:
                continue
            regional = median(list(present.values()))
            for s, v in present.items():
                residuals[s].append(v - regional)
        drifts = [abs(median(rs)) for rs in residuals.values() if len(rs) >= 3]
        out.append((f"{var}.drift_assessed_stations", len(drifts)))
        out.append((f"{var}.drift_flagged_stations", sum(d > DRIFT_THRESHOLD[var] for d in drifts)))
        out.append((f"{var}.drift_max_abs_micro", micro(max(drifts)) if drifts else 0))
    return out


# ---------------------------------------------------------------------------
# Work-unit documents
# ---------------------------------------------------------------------------

def input_manifest(chain_id, network_id, from_height, to_height, stations, variables, inp_digest) -> dict:
    return {"schema_version": SCHEMA_VERSION, "chain_id": chain_id, "network_id": network_id,
            "range": {"from_height": from_height, "to_height": to_height},
            "stations": "*" if stations == "*" else sorted(set(stations)), "variables": list(variables),
            "sources": [], "inputs_digest": inp_digest}


def input_manifest_digest(m: dict) -> str:
    stations = ["*"] if m["stations"] == "*" else m["stations"]
    body = (Body().u64(m["schema_version"]).str_(m["chain_id"]).str_(m["network_id"])
            .u64(m["range"]["from_height"]).u64(m["range"]["to_height"]).strs(stations).strs(m["variables"])
            .u64(len(m["sources"])).str_(m["inputs_digest"]))
    return digest(DOMAIN_INPUT, body)


def job_doc(args_list) -> dict:
    return {"job_type": JOB_TYPE, "runner": RUNNER, "args": list(args_list), "seed": 0,
            "tolerance": {"rule": "exact"}}


def job_digest(j: dict) -> str:
    r = j["runner"]
    body = (Body().str_(j["job_type"]).str_(r["kind"]).str_(r["name"]).str_(r["version"]).strs(j["args"])
            .u64(j["seed"]).str_(j["tolerance"]["rule"]))
    return digest(DOMAIN_JOB, body)


def output_doc(summary: list[tuple[str, int]]) -> dict:
    body = Body().u64(len(summary))
    for name, value in summary:
        body.str_(name).i64(value)
    return {"output_digest": digest(DOMAIN_OUTPUT, body), "summary": [{"name": n, "value": v} for n, v in summary]}


def work_unit(manifest_digest: str, job_dgst: str) -> str:
    return digest(DOMAIN_UNIT, Body().str_(manifest_digest).str_(job_dgst))


def build_unit(inputs_dir, from_height, to_height, chain_id, network_id, stations, variables) -> dict:
    variables = tuple(sorted(set(variables)))
    unknown = [v for v in variables if v not in PHYSICAL_BOUNDS]
    if unknown:
        raise SystemExit(f"unsupported variables {unknown}; supported: {sorted(PHYSICAL_BOUNDS)}")
    if from_height > to_height or from_height < 0:
        raise SystemExit("--from-height must be <= --to-height and non-negative")
    rows = load_inputs(inputs_dir, from_height, to_height, stations, variables)
    manifest = input_manifest(chain_id, network_id, from_height, to_height, stations, variables,
                              inputs_digest(rows, variables))
    job = job_doc([f"--from-height={from_height}", f"--to-height={to_height}",
                   f"--stations={'*' if stations == '*' else ','.join(sorted(set(stations)))}",
                   f"--variables={','.join(variables)}"])
    m_digest, j_digest = input_manifest_digest(manifest), job_digest(job)
    return {"work_unit_id": work_unit(m_digest, j_digest),
            "input_manifest": {**manifest, "digest": m_digest},
            "job": {**job, "digest": j_digest},
            "output": output_doc(compute(rows, variables))}


def parse_stations(value: str):
    return "*" if value.strip() == "*" else [s for s in (x.strip() for x in value.split(",")) if s]


def cmd_run(args) -> int:
    unit = build_unit(args.inputs, args.from_height, args.to_height, args.chain_id, args.network_id,
                      parse_stations(args.stations), args.variables.split(","))
    os.makedirs(args.out, exist_ok=True)
    for name in ("input_manifest", "job", "output"):
        with open(os.path.join(args.out, f"{name}.json"), "w") as fh:
            json.dump(unit[name], fh, indent=2, sort_keys=True)
    with open(os.path.join(args.out, "work_unit.json"), "w") as fh:
        json.dump({"work_unit_id": unit["work_unit_id"], "input_manifest_digest": unit["input_manifest"]["digest"],
                   "job_digest": unit["job"]["digest"], "output_digest": unit["output"]["output_digest"]},
                  fh, indent=2, sort_keys=True)
    print(f"{JOB_TYPE}: work_unit {unit['work_unit_id']} output {unit['output']['output_digest']} "
          f"({dict((s['name'], s['value']) for s in unit['output']['summary']).get('readings', 0)} readings) -> {args.out}")
    return 0


def cmd_verify(args) -> int:
    """A verifier's rerun: rebuild from the unit's own manifest and compare every digest (tolerance `exact`)."""
    with open(os.path.join(args.unit, "input_manifest.json")) as fh:
        m = json.load(fh)
    with open(os.path.join(args.unit, "work_unit.json")) as fh:
        claimed = json.load(fh)
    rerun = build_unit(args.inputs, m["range"]["from_height"], m["range"]["to_height"], m["chain_id"],
                       m["network_id"], m["stations"], m["variables"])
    if rerun["input_manifest"]["inputs_digest"] != m["inputs_digest"]:
        print("inputs_mismatch: the verifier's chain inputs differ from the worker's", file=sys.stderr)
        return 3
    checks = {"work_unit_id": rerun["work_unit_id"], "input_manifest_digest": rerun["input_manifest"]["digest"],
              "job_digest": rerun["job"]["digest"], "output_digest": rerun["output"]["output_digest"]}
    bad = [k for k, v in checks.items() if claimed.get(k) != v]
    if bad:
        print(f"disagree: {', '.join(bad)} differ", file=sys.stderr)
        return 1
    print(f"agree: work_unit {checks['work_unit_id']} output {checks['output_digest']}")
    return 0


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--inputs", required=True)
    r.add_argument("--from-height", type=int, required=True)
    r.add_argument("--to-height", type=int, required=True)
    r.add_argument("--chain-id", required=True)
    r.add_argument("--network-id", required=True)
    r.add_argument("--stations", default="*")
    r.add_argument("--variables", default=",".join(DEFAULT_VARIABLES))
    r.add_argument("--out", required=True)
    v = sub.add_parser("verify")
    v.add_argument("--unit", required=True)
    v.add_argument("--inputs", required=True)
    args = p.parse_args(argv)
    return cmd_run(args) if args.cmd == "run" else cmd_verify(args)


if __name__ == "__main__":
    sys.exit(main())
