#!/usr/bin/env python3
"""#2953: aggregate executed-crate floor for crate-execution-gates.yml.

The four crate lanes run as SEQUENTIAL steps, so any early failure skips
every later lane. Before #2953 nothing in the job distinguished "16 crates
passed" from "0 crates ran" — the #2923 shape, in the gate built to enforce
#2923. The real case: main job 99552091866 (2026-08-31 16:00:49Z) — cargo
was off PATH, the first lane died, every later lane skipped, the wrapper
self-test stayed green, and the job's own output looked healthy.

Called with NAME=OUTCOME pairs, one per crate lane, where OUTCOME is the
literal ``steps.<id>.outcome``: success | failure | cancelled | skipped.

ATTRIBUTION RULE (#2953, inherited from the partial fix): any NON-SKIPPED
outcome counts as EXECUTED. A lane that ran and FAILED was executed — its
red belongs to the lane and is reported as EXECUTED-and-red. Counting a
failure as "not executed" (the natural success-only mistake) lets a
genuinely failing crate silently LOWER the executed count instead of
failing the gate under its own name.

Output contract (stdout, machine-parseable, stable prefixes):
  CRATE_EXECUTION_RAN <name> outcome=<success|failure|cancelled>
  CRATE_EXECUTION_SKIPPED <name>
  CRATE_EXECUTION_ATTEMPTED count=<n> of=<total>
  CRATE_EXECUTION_FLOOR_OK executed=<n> of=<total>        (GREEN only)
Errors (stderr):
  CRATE_EXECUTION_FLOOR_ERROR <reason>

Exit 0 only when EVERY lane's outcome is success. Exit 1 otherwise —
including zero lanes attempted (ZERO_CRATE_LANES_EXECUTED, the exact #2953
shape), any skipped lane (LANE_SKIPPED), any executed-but-red lane
(LANE_EXECUTED_RED), and any malformed invocation (fail closed: a floor
that cannot read its inputs must not pass).
"""

from __future__ import annotations

import sys

VALID_OUTCOMES = ("success", "failure", "cancelled", "skipped")


def evaluate(pairs: list[str]) -> tuple[list[str], list[str]]:
    """Return (report_lines, errors). GREEN iff errors is empty."""
    errors: list[str] = []
    lanes: list[tuple[str, str]] = []
    seen: set[str] = set()

    if not pairs:
        return [], [
            "NO_LANES_DECLARED — the floor was invoked with zero lanes; a "
            "floor over nothing is vacuous by construction"
        ]

    for pair in pairs:
        name, sep, outcome = pair.partition("=")
        if not sep or not name or not outcome:
            errors.append(f"MALFORMED_LANE {pair!r} — expected NAME=OUTCOME")
            continue
        if outcome not in VALID_OUTCOMES:
            errors.append(
                f"UNKNOWN_OUTCOME {name}={outcome!r} — expected one of "
                f"{VALID_OUTCOMES}; refusing to guess which side of the "
                "floor it lands on"
            )
            continue
        if name in seen:
            errors.append(f"DUPLICATE_LANE {name}")
            continue
        seen.add(name)
        lanes.append((name, outcome))

    executed = [(name, outcome) for name, outcome in lanes if outcome != "skipped"]
    skipped = [name for name, outcome in lanes if outcome == "skipped"]

    report: list[str] = []
    for name, outcome in executed:
        report.append(f"CRATE_EXECUTION_RAN {name} outcome={outcome}")
    for name in skipped:
        report.append(f"CRATE_EXECUTION_SKIPPED {name}")
    report.append(
        f"CRATE_EXECUTION_ATTEMPTED count={len(executed)} of={len(lanes)}"
    )

    if lanes and not executed:
        errors.append(
            "ZERO_CRATE_LANES_EXECUTED — every crate lane was skipped; the "
            "job performed no crate test work at all (#2953, the "
            "job-99552091866 shape)"
        )
    for name in skipped:
        errors.append(
            f"LANE_SKIPPED {name} — this lane never attempted its crates"
        )
    for name, outcome in executed:
        if outcome != "success":
            errors.append(
                f"LANE_EXECUTED_RED {name} outcome={outcome} — this lane "
                "EXECUTED and went red; it counts toward the attempted "
                "floor and fails the gate under its own name, never as "
                "'not executed'"
            )

    return report, errors


def main() -> int:
    report, errors = evaluate(sys.argv[1:])
    for line in report:
        print(line)
    if errors:
        for error in errors:
            print(f"CRATE_EXECUTION_FLOOR_ERROR {error}", file=sys.stderr)
        return 1
    executed = sum(1 for line in report if line.startswith("CRATE_EXECUTION_RAN "))
    print(f"CRATE_EXECUTION_FLOOR_OK executed={executed} of={executed}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
