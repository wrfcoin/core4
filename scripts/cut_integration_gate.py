#!/usr/bin/env python3
"""Fail-closed execution gate for the 28 genesis-cut integration targets (#2490).

The inventory ratchet owns the broad workspace.  This narrower gate owns the
reviewed cut proof: every manifest pair must still exist in Cargo metadata and
must run at least one non-ignored test.  A successful Cargo process that ran
zero tests is therefore RED, rather than a green-shaped skip.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = Path(__file__).with_name("cut_integration_targets.json")
SUMMARY = re.compile(r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored")


def load_manifest(path: Path) -> list[dict[str, str]]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if value.get("schema") != 1:
        raise ValueError("manifest schema must be 1")
    targets = value.get("targets")
    if not isinstance(targets, list) or len(targets) != 28:
        raise ValueError("cut manifest must contain exactly 28 targets")
    pairs = [(entry.get("package"), entry.get("target")) for entry in targets]
    if any(not package or not target for package, target in pairs):
        raise ValueError("every target needs package and target")
    if len(pairs) != len(set(pairs)):
        raise ValueError("cut manifest contains duplicate package/target pairs")
    for entry in targets:
        if entry.get("hosted_runner") not in {"supported", "unavailable"}:
            raise ValueError("every target must declare hosted_runner supported/unavailable")
        if entry["hosted_runner"] == "unavailable" and not entry.get("reason"):
            raise ValueError("unavailable hosted target needs a reason")
        features = entry.get("features", [])
        if not isinstance(features, list) or not all(isinstance(feature, str) and feature for feature in features):
            raise ValueError("target features must be a list of nonempty strings")
        known = entry.get("known_failures", [])
        if not isinstance(known, list) or not all(isinstance(test, str) and test for test in known):
            raise ValueError("known_failures must be a list of nonempty test names")
    return targets


def discovered_pairs() -> set[tuple[str, str]]:
    completed = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT, text=True, capture_output=True, check=True,
    )
    metadata = json.loads(completed.stdout)
    return {
        (package["name"], target["name"])
        for package in metadata["packages"]
        for target in package["targets"]
        if "test" in target["kind"]
    }


def validate(targets: list[dict[str, str]], discovered: set[tuple[str, str]]) -> list[str]:
    errors: list[str] = []
    for entry in targets:
        pair = (entry["package"], entry["target"])
        if pair not in discovered:
            errors.append(f"manifest target not discovered: {pair}")
    return errors


def run_target(entry: dict[str, str]) -> tuple[int, int, int]:
    command = ["cargo", "test", "--locked", "-p", entry["package"], "--test", entry["target"], "--", "--test-threads=1"]
    features = entry.get("features", [])
    if features:
        command[5:5] = ["--features", ",".join(features)]
    environment = os.environ.copy()
    environment["RUSTC_WRAPPER"] = ""
    environment.setdefault("WRFCOIN_RUN_TLS_INTEGRATION_TESTS", "0")
    completed = subprocess.run(command, cwd=ROOT, env=environment, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    print(completed.stdout, end="")
    passed = failed = ignored = 0
    for match in SUMMARY.finditer(completed.stdout):
        passed += int(match.group(1)); failed += int(match.group(2)); ignored += int(match.group(3))
    print(f"CUT_INTEGRATION_TARGET {entry['package']}::{entry['target']} passed={passed} failed={failed} ignored={ignored}")
    if passed + failed == 0:
        raise RuntimeError(f"zero executed tests: {entry['package']}::{entry['target']}")
    known_failures = entry.get("known_failures", [])
    observed_known = [test for test in known_failures if f"test {test} ... FAILED" in completed.stdout]
    if completed.returncode and failed == len(known_failures) and observed_known == known_failures:
        print(f"CUT_INTEGRATION_KNOWN_FAILURE {entry['package']}::{entry['target']} count={failed}")
        return passed, failed, ignored
    if completed.returncode:
        raise RuntimeError(f"test target failed: {entry['package']}::{entry['target']}")
    return passed, failed, ignored


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--validate-only", action="store_true")
    args = parser.parse_args()
    try:
        targets = load_manifest(args.manifest)
        errors = validate(targets, discovered_pairs())
    except (ValueError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        errors = [str(error)]
    if errors:
        for error in errors:
            print(f"CUT_INTEGRATION_ERROR {error}", file=sys.stderr)
        return 1
    print(f"CUT_INTEGRATION_MANIFEST_OK targets={len(targets)}")
    for entry in targets:
        detail = entry.get("reason", "")
        print(f"CUT_INTEGRATION_HOSTED {entry['package']}::{entry['target']} {entry['hosted_runner']} {detail}".rstrip())
    if args.validate_only:
        return 0
    try:
        for entry in targets:
            if entry["hosted_runner"] == "unavailable":
                print(f"CUT_INTEGRATION_DEFERRED {entry['package']}::{entry['target']} {entry['reason']}")
                continue
            run_target(entry)
    except RuntimeError as error:
        print(f"CUT_INTEGRATION_ERROR {error}", file=sys.stderr)
        return 1
    print(f"CUT_INTEGRATION_OK targets={len(targets)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
