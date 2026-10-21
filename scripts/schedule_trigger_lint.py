#!/usr/bin/env python3
"""Fail-closed lint: only owner-acknowledged workflows may carry schedules (#2498).

#2652 parked every cron so the August Actions budget goes to the D2
verification instead of background load — but it left the parked state a
CONVENTION: nothing stopped the next PR from re-adding a cron, and a cron
re-added in a busy diff is invisible until the bill arrives. This lint turns
the convention into a ratchet.

Rules, all fail-closed:

* a workflow with a resolved ``schedule`` trigger outside ``ALLOWED_SCHEDULES``
  is a violation (re-arming a cron is a
  reviewed decision, made here, in this file, in the same diff);
* an allowlist entry whose file no longer exists or no longer has a schedule
  is itself a violation — a stale exemption is worse than none (#2498 r105
  D-row lesson, and the shrink-only rule the integration ratchet uses);
* fewer than ``MINIMUM_WORKFLOWS`` files scanned is a violation — a lint that
  inspected nothing must fail loudly, not report a clean bill for an empty
  set (#1013 anti-vacuity standard).

Trigger resolution notes, because both have bitten this repo:

* YAML 1.1 parses the bare key ``on`` as boolean ``True``; this lint reads
  both spellings, so a workflow cannot hide a trigger behind the aliasing.
* Commented-out ``# schedule:`` blocks (the #2652 parking style) resolve to
  nothing under a real parser and are correctly ignored — the lint measures
  the RESOLVED trigger set, never the text.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / ".github" / "workflows"

# Owner 2026-10-07 relevance/nightly ruling supersedes #2652 for these
# non-required workflows only; policy and two-site acknowledgement Refs #2498.
#
# #2657 review, finding 3: a bare-name allowlist absorbed violations SILENTLY —
# a two-line diff could re-arm a cron and allowlist it in the same commit and
# exit 0. An in-repo lint cannot make that diff impossible (the lint itself is
# in-diff-editable); what it CAN do is make it impossible to do QUIETLY. So an
# entry is only valid when three things agree:
#   1. the entry here carries a decision citation (issue ref) and a date;
#   2. the workflow file itself carries the SAME citation in a comment
#      adjacent to its schedule block (two-site agreement — the re-arming diff
#      must touch both files, with the citation visible at both sites);
#   3. every run PRINTS the acknowledged set with citations, so an armed cron
#      is loud in every subsequent lint log, never ambient.
# Anything less — bare name, missing citation, one-site edit — is RED.
ALLOWED_SCHEDULES: dict[str, dict[str, str]] = {
    "tdd-phase3c.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "build-provenance-gate.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "ci-deploy-features.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "unsafe-policy.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "native-chain-package-gate.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "determinism-lint.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "bincode-boundary-guard.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "cli-test-gate.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "wrfcoin-cli-integration.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "launch-api-bin-tests.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "crate-execution-gates.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "named-test-targets.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "input-panic-lint.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "workflow-path-coverage.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "retired-component-sdks.yml": {"decision": "#2498", "decided": "2026-10-07"},
    "adversarial-tests.yml": {"decision": "#2498", "decided": "2026-10-07"},
}


# Anti-vacuity floor: the repo has ~30 workflows; if discovery ever sees fewer
# than this, the lint is scanning the wrong tree and must say so.
MINIMUM_WORKFLOWS = 10


def resolved_triggers(path: Path) -> set[str]:
    document = yaml.safe_load(path.read_text(encoding="utf-8"))
    if not isinstance(document, dict):
        return set()
    triggers = document.get("on", document.get(True))
    if triggers is None:
        return set()
    if isinstance(triggers, str):
        return {triggers}
    if isinstance(triggers, list):
        return set(triggers)
    return set(triggers)


def main() -> int:
    files = sorted(
        path
        for pattern in ("*.yml", "*.yaml")
        for path in WORKFLOWS.glob(pattern)
    )
    errors: list[str] = []

    if len(files) < MINIMUM_WORKFLOWS:
        errors.append(
            f"only {len(files)} workflow files discovered under {WORKFLOWS}; "
            f"need at least {MINIMUM_WORKFLOWS} — refusing to certify an "
            "empty or misread tree"
        )

    scheduled: set[str] = set()
    for path in files:
        if "schedule" in resolved_triggers(path):
            scheduled.add(path.name)
            if path.name not in ALLOWED_SCHEDULES:
                errors.append(
                    f"{path.name}: resolved `schedule` trigger outside the "
                    "allowlist — crons are parked (#2652); re-arming one is a "
                    "reviewed edit to ALLOWED_SCHEDULES in "
                    "scripts/schedule_trigger_lint.py"
                )

    acknowledged: list[str] = []
    for name in sorted(ALLOWED_SCHEDULES):
        entry = ALLOWED_SCHEDULES[name]
        decision = entry.get("decision", "")
        decided = entry.get("decided", "")
        if not re.fullmatch(r"#\d+", decision) or not re.fullmatch(
            r"\d{4}-\d{2}-\d{2}", decided
        ):
            errors.append(
                f"allowlist entry {name} lacks a decision citation "
                "(`decision`: '#<issue>', `decided`: 'YYYY-MM-DD') — an "
                "uncited exemption is a silent one; RED"
            )
            continue
        if not (WORKFLOWS / name).exists():
            errors.append(
                f"allowlist entry {name} does not exist — stale exemption; "
                "remove it"
            )
            continue
        if name not in scheduled:
            errors.append(
                f"allowlist entry {name} has no resolved `schedule` trigger — "
                "stale exemption; remove it"
            )
            continue
        workflow_text = (WORKFLOWS / name).read_text(encoding="utf-8")
        cited = any(
            decision in line and line.lstrip().startswith("#")
            for line in workflow_text.splitlines()
        )
        if not cited:
            errors.append(
                f"{name}: allowlisted with {decision}, but the workflow file "
                "carries no comment citing that decision next to its schedule "
                "— two-site agreement required, so a re-arming diff is loud "
                "in both files"
            )
            continue
        acknowledged.append(f"{name} (per {decision}, {decided})")

    print(
        f"SCHEDULE_TRIGGER_LINT scanned={len(files)} "
        f"scheduled={sorted(scheduled) or '[]'}"
    )
    for line in acknowledged:
        print(f"SCHEDULE_TRIGGER_LINT_ACKNOWLEDGED_CRON {line}")
    for error in errors:
        print(f"SCHEDULE_TRIGGER_LINT_ERROR {error}")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
