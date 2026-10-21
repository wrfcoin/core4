#!/usr/bin/env python3
"""Fail closed on dispatch assumptions that have a mechanical source of truth.

The sprint repeatedly lost time to issue text that was true in the abstract but
false in this repository: an absent branch-protection rule, a named mechanism
that was already gone, and work merged while its linked issue remained open.
This checker does not attempt to infer intent from prose.  Instead, a dispatch
manifest makes each mechanically testable premise explicit and this program
refuses the dispatch when any premise is false.

Use before assigning a lane:

    python3 scripts/pre_dispatch_truth_check.py manifest.json

The manifest may require repository paths and symbols, branch-protection state,
and linked PR/issue lifecycle state.  Network facts are read with ``gh``; pass
``--facts FILE`` for hermetic review or self-test fixtures.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


def _load_json(path: Path) -> dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot read manifest {path}: {exc}") from exc
    if not isinstance(data, dict):
        raise ValueError(f"JSON object required in {path}")
    return data


def _load_manifest(path: Path) -> dict[str, Any]:
    data = _load_json(path)
    if not isinstance(data.get("checks"), list):
        raise ValueError("manifest must contain a list-valued 'checks'")
    return data


def _gh_json(args: list[str]) -> Any:
    result = subprocess.run(
        ["gh", *args], text=True, capture_output=True, check=False
    )
    if result.returncode:
        return {"_error": result.stderr.strip() or f"gh exited {result.returncode}"}
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        return {"_error": f"gh returned invalid JSON: {exc}"}


def _live_facts(repo: str, issue: int | None, branch: str | None,
                prs: list[int]) -> dict[str, Any]:
    facts: dict[str, Any] = {"issues": {}, "prs": {}, "branch_protection": {}}
    if issue is not None:
        facts["issues"][str(issue)] = _gh_json([
            "issue", "view", str(issue), "--repo", repo, "--json", "state"
        ])
    if branch:
        facts["branch_protection"][branch] = _gh_json([
            "api", f"repos/{repo}/branches/{branch}/protection"
        ])
    for pr in prs:
        facts["prs"][str(pr)] = _gh_json([
            "pr", "view", str(pr), "--repo", repo, "--json", "state"
        ])
    return facts


def _fact(facts: dict[str, Any], group: str, key: str) -> dict[str, Any]:
    value = facts.get(group, {}).get(key, {})
    return value if isinstance(value, dict) else {"_error": f"missing {group}/{key}"}


def _check_one(check: dict[str, Any], root: Path, facts: dict[str, Any]) -> list[str]:
    label = str(check.get("label", "unnamed check"))
    errors: list[str] = []
    for raw in check.get("paths_exist", []):
        path = root / str(raw)
        if not path.is_file() and not path.is_dir():
            errors.append(f"{label}: required path is absent: {raw}")
    for raw in check.get("paths_absent", []):
        path = root / str(raw)
        if path.exists():
            errors.append(f"{label}: retired path is still present: {raw}")
    for symbol in check.get("symbols", []):
        if not isinstance(symbol, dict):
            errors.append(f"{label}: symbol entry is not an object")
            continue
        path = root / str(symbol.get("path", ""))
        pattern = str(symbol.get("regex", ""))
        if not path.is_file():
            errors.append(f"{label}: symbol file is absent: {path.relative_to(root)}")
        elif not pattern or re.search(pattern, path.read_text(encoding="utf-8"), re.M) is None:
            errors.append(f"{label}: required symbol regex not found: {path.relative_to(root)} / {pattern}")

    issue = check.get("issue")
    branch = check.get("branch")
    expected_protection = check.get("branch_protection")
    if expected_protection is not None:
        fact = _fact(facts, "branch_protection", str(branch))
        actual = "_error" not in fact
        if actual != bool(expected_protection):
            detail = fact.get("_error", "protection object present")
            errors.append(f"{label}: branch {branch!r} protection expected={bool(expected_protection)} actual={actual} ({detail})")

    for pr in check.get("linked_prs", []):
        fact = _fact(facts, "prs", str(pr))
        state = fact.get("state")
        if state != "MERGED":
            errors.append(f"{label}: linked PR #{pr} is not merged (state={state!r}; {fact.get('_error', '')})")
        elif issue is not None:
            issue_fact = _fact(facts, "issues", str(issue))
            if issue_fact.get("state") != "CLOSED":
                errors.append(f"{label}: linked PR #{pr} is merged but issue #{issue} remains open — update/close the issue before dispatch")
    return errors


def run(manifest: Path, root: Path, facts_path: Path | None) -> int:
    data = _load_manifest(manifest)
    all_errors: list[str] = []
    supplied_facts = _load_json(facts_path) if facts_path else None
    for check in data["checks"]:
        if not isinstance(check, dict):
            all_errors.append("manifest: check entry is not an object")
            continue
        repo = str(check.get("repo", data.get("repo", "")))
        issue = check.get("issue")
        branch = check.get("branch")
        prs = [int(value) for value in check.get("linked_prs", [])]
        facts = supplied_facts if supplied_facts is not None else _live_facts(
            repo, int(issue) if issue is not None else None,
            str(branch) if branch else None, prs
        )
        all_errors.extend(_check_one(check, root, facts))
    if all_errors:
        print("FAIL: pre-dispatch truth check rejected this lane:")
        for error in all_errors:
            print(f"  - {error}")
        return 1
    print("pre-dispatch truth check passed: every declared premise matched the tree and lifecycle facts")
    return 0


def self_test() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "live.txt").write_text("reachable_consumer()\n", encoding="utf-8")
        manifest = root / "manifest.json"
        facts = root / "facts.json"
        manifest.write_text(json.dumps({"checks": [{
            "label": "healthy lane", "issue": 7, "branch": "main",
            "branch_protection": True, "paths_exist": ["live.txt"],
            "symbols": [{"path": "live.txt", "regex": "reachable_consumer"}],
            "linked_prs": [8]
        }]}), encoding="utf-8")
        facts.write_text(json.dumps({"issues": {"7": {"state": "CLOSED"}},
            "prs": {"8": {"state": "MERGED"}},
            "branch_protection": {"main": {"required_status_checks": {}}}}), encoding="utf-8")
        assert run(manifest, root, facts) == 0

        # Four real dispatch-waste shapes: named path/symbol absent, a claimed
        # branch rule absent, and merged work whose issue was never updated.
        bad = json.loads(manifest.read_text(encoding="utf-8"))
        bad["checks"][0]["paths_exist"] = ["retired_bridge.rs"]
        bad["checks"][0]["symbols"] = [{"path": "live.txt", "regex": "missing_owner"}]
        bad["checks"][0]["branch_protection"] = False
        facts.write_text(json.dumps({"issues": {"7": {"state": "OPEN"}},
            "prs": {"8": {"state": "MERGED"}},
            "branch_protection": {"main": {"required_status_checks": {}}}}), encoding="utf-8")
        manifest.write_text(json.dumps(bad), encoding="utf-8")
        assert run(manifest, root, facts) == 1
    print("pre_dispatch_truth_check.py self-test passed (path, symbol, branch-rule, and stale-issue RED cases)")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", nargs="?", type=Path)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--facts", type=Path, help="hermetic GitHub-facts JSON")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.manifest is None:
        parser.error("manifest is required unless --self-test is used")
    try:
        return run(args.manifest, args.root.resolve(), args.facts)
    except ValueError as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
