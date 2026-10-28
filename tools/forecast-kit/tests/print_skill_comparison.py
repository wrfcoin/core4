#!/usr/bin/env python3
"""Print the skill-definition comparison on the fixture set (markdown), for the owner's decision.

Usage: FORECAST_SCORER_BIN=<path to forecast-score-batch> python tools/forecast-kit/tests/print_skill_comparison.py
Scores every fixture forecast under each SKILL_DEFINITION (cap disabled, baseline 0) so the table shows the raw
receipt skill, then the decision the job would make with the default cap and the 0.1 baseline.
"""

import contextlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import scorer_fixtures as fx  # noqa: E402
from test_scorer_job import BIN, job  # noqa: E402

CONFIGS = [(d, {}) for d in job.SKILL_DEFINITIONS]


def score_all(tmp, index, definition, extra, cap, baseline):
    reveals = [job.load_reveal(os.path.join(tmp, "reveals", n)) for n in sorted(index)]
    request, _ = job.build_request(reveals, os.path.join(tmp, "obs"), definition, cap)
    request["baseline"] = baseline
    request.update(extra)
    out = json.loads(subprocess.run([BIN], input=json.dumps(request), capture_output=True, text=True, check=True).stdout)
    names = {r["record"]["commit_id"]: os.path.basename(r["dir"]) for r in reveals}
    return {names[f["commit_id"]]: f for f in out["forecasts"]}


def main(configs=CONFIGS):
    if not BIN:
        sys.exit("set FORECAST_SCORER_BIN (cargo build -p wrfcoin-forecast-score --bin forecast-score-batch)")
    tmp = tempfile.mkdtemp()
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            index = fx.build(tmp, job.kit)
        raw = {c: score_all(tmp, index, c[0], dict(c[1]), 10**6, 0.0) for c in map(lambda c: (c[0], tuple(c[1].items())), configs)}
        capped = {c: score_all(tmp, index, c[0], dict(c[1]), job.DEFAULT_CAP_PER_CELL_LEAD, job.BASELINE_FORECAST_SKILL)
                  for c in raw}
        labels = [f"{d}{' +A6' if extra else ''}" for d, extra in raw]
        print("| forecast | station | " + " | ".join(f"{l} skill" for l in labels) + " | "
              + " | ".join(f"{l} decision" for l in labels) + " |")
        print("|---|---|" + "---:|" * len(labels) + "---|" * len(labels))
        for name in sorted(index):
            strategy, station = index[name]
            first = next(iter(raw.values()))
            if name not in first:
                print(f"| {strategy} | {station} | " + " | ".join(["refused"] * (2 * len(labels))) + " |")
                continue
            skills = [f"{raw[c][name]['score']['metrics']['skill_score']:.3f}" if "score" in raw[c][name] else "error"
                      for c in raw]
            decisions = [capped[c][name].get("decision", {}).get("reason", "error") for c in raw]
            print(f"| {strategy} | {station} | " + " | ".join(skills) + " | " + " | ".join(decisions) + " |")
    finally:
        shutil.rmtree(tmp)


if __name__ == "__main__":
    main()
