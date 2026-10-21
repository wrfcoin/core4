#!/usr/bin/env python3
"""Fail closed if O2b loses its executable process-environment control."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


def o2b_section(document: str) -> str:
    start = document.find("### O2b.")
    end = document.find("### O3.", start)
    if start < 0 or end < 0:
        raise ValueError("cannot locate the O2b section")
    return document[start:end]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--document", type=Path, required=True)
    args = parser.parse_args()

    try:
        section = o2b_section(args.document.read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1

    # Do not let the prose describing this checker's mutation control satisfy
    # the control itself. Only the executable-probe portion of O2b counts.
    probe_section = section.split("`scripts/check_oracle_governance_ceremony_doc.py`", 1)[0]
    normalized = re.sub(r"\s+", " ", probe_section)
    probe_required = (
        "scripts/verify-oracle-governance-allowlist.sh",
        "--container <node-container>",
        "--expected-pubkey <O1 lowercase 64-hex governance public key>",
        "exactly equal",
        "fails closed on unreadable, missing, duplicate, or mismatched process state",
    )
    missing = [fragment for fragment in probe_required if fragment not in normalized]
    if "There is no safe end-to-end admission probe" not in section:
        missing.append("There is no safe end-to-end admission probe")
    if missing:
        print(
            "ERROR: O2b no longer makes the process-level equality control explicit: "
            + "; ".join(repr(fragment) for fragment in missing),
            file=sys.stderr,
        )
        return 1

    print("PASS: O2b retains an executable, fail-closed process-environment equality probe")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
