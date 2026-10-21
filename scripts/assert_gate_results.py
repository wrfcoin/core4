#!/usr/bin/env python3
"""Fail closed when a terminal GitHub Actions gate did not actually run.

The job invoking this receives the complete ``needs`` object as JSON.  Required
jobs must finish with exactly ``success``.  Conditional jobs are explicitly
allowed to be ``skipped`` only because their own event predicate excluded them;
they still fail on every other non-success result.  Every dependency must be
classified, so adding a dependency cannot silently evade the terminal verdict.
"""

from __future__ import annotations

import json
import os
import sys


def names(value: str) -> set[str]:
    return {item for item in value.split() if item}


def check(needs_text: str, required: set[str], conditional: set[str]) -> list[str]:
    try:
        needs = json.loads(needs_text)
    except json.JSONDecodeError as error:
        return [f"GATE_NEEDS is not valid JSON: {error}"]
    if not isinstance(needs, dict):
        return ["GATE_NEEDS must be a JSON object"]

    errors: list[str] = []
    overlap = required & conditional
    if overlap:
        errors.append(f"jobs are both required and conditional: {', '.join(sorted(overlap))}")

    actual = set(needs)
    classified = required | conditional
    unclassified = actual - classified
    unknown = classified - actual
    if unclassified:
        errors.append(f"unclassified needs dependency: {', '.join(sorted(unclassified))}")
    if unknown:
        errors.append(f"classified job missing from needs: {', '.join(sorted(unknown))}")

    for job in sorted(required):
        result = needs.get(job, {}).get("result") if isinstance(needs.get(job), dict) else None
        if result != "success":
            errors.append(f"required gate {job!r} result={result!r}; expected 'success'")
    for job in sorted(conditional):
        result = needs.get(job, {}).get("result") if isinstance(needs.get(job), dict) else None
        if result not in {"success", "skipped"}:
            errors.append(f"conditional gate {job!r} result={result!r}; expected 'success' or intentional 'skipped'")
    return errors


def main() -> int:
    errors = check(
        os.environ.get("GATE_NEEDS", ""),
        names(os.environ.get("GATE_REQUIRED", "")),
        names(os.environ.get("GATE_CONDITIONAL", "")),
    )
    if errors:
        for error in errors:
            print(f"::error::{error}")
        return 1
    print("terminal gate results are complete and successful")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
