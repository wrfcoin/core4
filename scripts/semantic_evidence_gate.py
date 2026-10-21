#!/usr/bin/env python3
"""Fail closed when mock-authored transitions re-enter integration evidence (#2470)."""

from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = Path(__file__).with_name("semantic_evidence_manifest.json")
ARCHIVE_MARKER = "ARCHIVED MOCK CONTRACT"
CUT_STATUSES = {"bounded", "branch_sensitive", "stale_for_cut"}
DISCOVERED_TEST_ROOTS = ("tests/integration", "tests/e2e")
DISCOVERED_TEST_SUFFIXES = tuple(
    f".{kind}.{extension}"
    for kind in ("test", "spec")
    for extension in ("js", "cjs", "jsx", "ts", "tsx")
)


def load_manifest(path: Path) -> dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if data.get("schema_version") != 1:
        raise ValueError("semantic evidence manifest schema_version must be 1")
    return data


def discovered_test_paths(root: Path) -> list[Path]:
    paths: list[Path] = []
    for relative_root in DISCOVERED_TEST_ROOTS:
        discovery_root = root / relative_root
        if not discovery_root.is_dir():
            continue
        paths.extend(
            path
            for path in discovery_root.rglob("*")
            if path.is_file() and path.name.endswith(DISCOVERED_TEST_SUFFIXES)
        )
    return sorted(paths)


def validate(
    root: Path, manifest: dict[str, Any], require_cut_ready: bool = False
) -> list[str]:
    errors: list[str] = []
    ids: set[str] = set()

    cases = manifest.get("archived_mock_cases", [])
    evidence = manifest.get("production_evidence", [])
    if not cases:
        errors.append("manifest has no archived_mock_cases")
    if not evidence:
        errors.append("manifest has no production_evidence")

    old_active_paths = {
        relative
        for case in cases
        for relative in case.get("old_active_paths", [])
    }
    archived_fingerprints: list[tuple[str, str, list[str]]] = []

    for entry in [*cases, *evidence]:
        entry_id = entry.get("id")
        if not isinstance(entry_id, str) or not entry_id:
            errors.append("every manifest entry needs a non-empty id")
        elif entry_id in ids:
            errors.append(f"duplicate manifest id: {entry_id}")
        else:
            ids.add(entry_id)

    for case in cases:
        case_id = case.get("id", "<missing>")
        if not case.get("claims"):
            errors.append(f"{case_id}: claims must not be empty")
        if not case.get("disposition"):
            errors.append(f"{case_id}: disposition must not be empty")

        for relative in case.get("old_active_paths", []):
            if (root / relative).exists():
                errors.append(
                    f"{case_id}: ACTIVE MOCK EVIDENCE PATH RESTORED: {relative}"
                )

        fixtures = case.get("fixtures", [])
        if not fixtures:
            errors.append(f"{case_id}: no archived fixtures declared")
        for fixture in fixtures:
            relative = fixture.get("path", "")
            path = root / relative
            if not relative.endswith((".fixture.js", ".fixture.ts")):
                errors.append(
                    f"{case_id}: archive must use a non-test .fixture suffix: {relative}"
                )
            if not path.is_file():
                errors.append(f"{case_id}: archived fixture missing: {relative}")
                continue
            text = path.read_text(encoding="utf-8")
            if ARCHIVE_MARKER not in text.splitlines()[0]:
                errors.append(f"{case_id}: archive marker missing from first line: {relative}")
            tokens = fixture.get("tokens", [])
            for token in tokens:
                if token not in text:
                    errors.append(f"{case_id}: fixture token missing from {relative}: {token}")
            if tokens:
                archived_fingerprints.append((case_id, relative, tokens))

    for path in discovered_test_paths(root):
        relative = path.relative_to(root).as_posix()
        if relative in old_active_paths:
            continue
        text = path.read_text(encoding="utf-8")
        if ARCHIVE_MARKER in text:
            errors.append(
                f"ACTIVE ARCHIVED MOCK DISCOVERED AS TEST: {relative}"
            )
            continue
        for case_id, fixture, tokens in archived_fingerprints:
            if all(token in text for token in tokens):
                errors.append(
                    f"{case_id}: ACTIVE MOCK EVIDENCE DISCOVERED: {relative} "
                    f"matches archived fingerprint {fixture}"
                )
                break

    for item in evidence:
        item_id = item.get("id", "<missing>")
        for field in ("command", "proves", "limit"):
            if not item.get(field):
                errors.append(f"{item_id}: {field} must not be empty")
        if item.get("cut_status") not in CUT_STATUSES:
            errors.append(
                f"{item_id}: cut_status must be one of {sorted(CUT_STATUSES)}"
            )
        subjects = item.get("subjects", [])
        if not subjects:
            errors.append(f"{item_id}: no production subjects declared")
        for subject in subjects:
            relative = subject.get("path", "")
            path = root / relative
            if not path.is_file():
                errors.append(f"{item_id}: production subject missing: {relative}")
                continue
            text = path.read_text(encoding="utf-8")
            for token in subject.get("tokens", []):
                if token not in text:
                    errors.append(
                        f"{item_id}: evidence token missing from {relative}: {token}"
                    )
        if require_cut_ready and item.get("cut_status") == "stale_for_cut":
            errors.append(f"{item_id}: CUT EVIDENCE STALE: {item.get('limit', '')}")

    return errors


def run_cut_ready_commands(root: Path, evidence: list[dict[str, Any]]) -> list[str]:
    """Run every manifest command before a cut-ready workflow can turn green.

    The manifest is repository-controlled release evidence. Its command is
    deliberately executed through the shell because several recorded commands
    use shell composition (for example `cargo test ... && cargo test ...`).
    A nonzero result is an error, never an informational log: a green strict
    verdict must mean both that no item is stale and that its exact recorded
    evidence passed at this candidate checkout.
    """
    errors: list[str] = []
    for item in evidence:
        item_id = item.get("id", "<missing>")
        command = item.get("command")
        if not isinstance(command, str) or not command:
            errors.append(f"{item_id}: command must not be empty before execution")
            continue
        print(f"SEMANTIC_EVIDENCE_COMMAND id={item_id} command={command}")
        result = subprocess.run(command, shell=True, cwd=root, check=False)
        if result.returncode != 0:
            errors.append(
                f"{item_id}: evidence command exited {result.returncode}: {command}"
            )
    return errors


def materialize_self_test(root: Path, manifest: dict[str, Any]) -> None:
    for case in manifest["archived_mock_cases"]:
        for fixture in case["fixtures"]:
            path = root / fixture["path"]
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(
                "// ARCHIVED MOCK CONTRACT\n" + "\n".join(fixture["tokens"]) + "\n",
                encoding="utf-8",
            )
    for item in manifest["production_evidence"]:
        for subject in item["subjects"]:
            path = root / subject["path"]
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("\n".join(subject["tokens"]) + "\n", encoding="utf-8")


def self_test(manifest: dict[str, Any]) -> int:
    with tempfile.TemporaryDirectory(prefix="semantic-evidence-self-test-") as raw:
        root = Path(raw)
        materialize_self_test(root, manifest)
        baseline = validate(root, manifest)
        if baseline:
            print("SELF_TEST_ERROR baseline fixture was not valid:")
            print("\n".join(baseline))
            return 1

        first_case = manifest["archived_mock_cases"][0]
        restored = root / first_case["old_active_paths"][0]
        restored.parent.mkdir(parents=True, exist_ok=True)
        restored.write_text("mock transition\n", encoding="utf-8")
        active_errors = validate(root, manifest)
        if not any("ACTIVE MOCK EVIDENCE PATH RESTORED" in err for err in active_errors):
            print("SELF_TEST_ERROR restored active mock path was not rejected")
            return 1
        restored.unlink()

        renamed = root / "tests/integration/renamed/staking-evidence.test.js"
        renamed.parent.mkdir(parents=True, exist_ok=True)
        fingerprint = first_case["fixtures"][0]["tokens"]
        renamed.write_text("\n".join(fingerprint) + "\n", encoding="utf-8")
        renamed_errors = validate(root, manifest)
        if not any(
            "ACTIVE MOCK EVIDENCE DISCOVERED" in err for err in renamed_errors
        ):
            print("SELF_TEST_ERROR renamed discovered mock path was not rejected")
            return 1
        renamed.unlink()

        first_subject = manifest["production_evidence"][0]["subjects"][0]
        subject_path = root / first_subject["path"]
        subject_path.write_text("production subject drifted\n", encoding="utf-8")
        drift_errors = validate(root, manifest)
        if not any("evidence token missing" in err for err in drift_errors):
            print("SELF_TEST_ERROR production evidence drift was not rejected")
            return 1

        materialize_self_test(root, manifest)
        stale_manifest = json.loads(json.dumps(manifest))
        stale_item = stale_manifest["production_evidence"][0]
        # RED mutation, verbatim: `cut_status = "stale_for_cut"`.
        # Plant it here rather than relying on the live manifest to remain
        # stale forever; otherwise fixing the last stale seam makes this
        # anti-vacuity control fail for the wrong reason.
        stale_item["cut_status"] = "stale_for_cut"
        stale_item["limit"] = "self-test planted stale cut evidence"
        cut_errors = validate(root, stale_manifest, require_cut_ready=True)
        if not any(
            stale_item["id"] in err and "CUT EVIDENCE STALE" in err
            for err in cut_errors
        ):
            print("SELF_TEST_ERROR stale cut evidence was not rejected")
            return 1

    print(
        "SEMANTIC_EVIDENCE_SELF_TEST_OK "
        f"archived_cases={len(manifest['archived_mock_cases'])} red_controls=4"
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--require-cut-ready", action="store_true")
    parser.add_argument("--run-cut-ready-commands", action="store_true")
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--manifest", type=Path, default=MANIFEST)
    args = parser.parse_args()

    try:
        manifest = load_manifest(args.manifest)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"SEMANTIC_EVIDENCE_GATE_ERROR {error}")
        return 1

    if args.self_test:
        return self_test(manifest)

    if args.run_cut_ready_commands and not args.require_cut_ready:
        print(
            "SEMANTIC_EVIDENCE_GATE_ERROR "
            "--run-cut-ready-commands requires --require-cut-ready"
        )
        return 1

    errors = validate(
        args.root.resolve(), manifest, require_cut_ready=args.require_cut_ready
    )
    if not errors and args.run_cut_ready_commands:
        errors.extend(run_cut_ready_commands(args.root.resolve(), manifest["production_evidence"]))
    if errors:
        print("SEMANTIC_EVIDENCE_GATE_ERROR")
        for error in errors:
            print(f"  - {error}")
        return 1

    fixture_count = sum(
        len(case["fixtures"]) for case in manifest["archived_mock_cases"]
    )
    stale_count = sum(
        item["cut_status"] == "stale_for_cut"
        for item in manifest["production_evidence"]
    )
    print(
        "SEMANTIC_EVIDENCE_GATE_OK "
        f"archived_cases={len(manifest['archived_mock_cases'])} "
        f"archived_files={fixture_count} "
        f"production_evidence={len(manifest['production_evidence'])} "
        f"cut_stale={stale_count}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
