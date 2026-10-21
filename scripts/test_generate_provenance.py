#!/usr/bin/env python3
"""Tests for generate_provenance.py and verify_provenance.sh (#2151)."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import generate_provenance as gen  # noqa: E402
import write_build_info as wbi  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parent.parent
VERIFY_SCRIPT = REPO_ROOT / "scripts" / "verify_provenance.sh"


def run_verify(artifact: Path, provenance: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["bash", str(VERIFY_SCRIPT), str(artifact), str(provenance)],
        capture_output=True, text=True, check=False,
    )


class GenerateProvenance(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp())
        self.artifact = self.tmp / "wrfcoin-node-linux-x86_64"
        self.artifact.write_bytes(b"fixture binary content")
        self.out = self.tmp / "wrfcoin-node-linux-x86_64.provenance.json"

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp, ignore_errors=True)

    def generate(self) -> dict:
        rc = gen.main([
            "--artifact", str(self.artifact),
            "--out", str(self.out),
            "--source-repo", "wrfcoin/core4",
            "--source-commit", "deadbeef",
            "--workflow-ref", ".github/workflows/release-signing.yml@refs/tags/v1.0.0",
            "--run-id", "1", "--run-attempt", "1",
            "--builder-id", "https://github.com/wrfcoin/core4/actions/runs/1",
            "--toolchain", "rustc 1.88.0",
            "--build-command", "cargo build --release --locked",
        ])
        self.assertEqual(rc, 0)
        return json.loads(self.out.read_text())

    def test_subject_digest_matches_the_real_artifact_hash(self) -> None:
        stmt = self.generate()
        self.assertEqual(stmt["subject"][0]["digest"]["sha256"], gen.sha256_of(self.artifact))

    def test_subject_name_is_the_artifact_filename(self) -> None:
        stmt = self.generate()
        self.assertEqual(stmt["subject"][0]["name"], self.artifact.name)

    def test_statement_and_predicate_types_are_the_documented_ones(self) -> None:
        stmt = self.generate()
        self.assertEqual(stmt["_type"], gen.IN_TOTO_STATEMENT_TYPE)
        self.assertEqual(stmt["predicateType"], gen.SLSA_PREDICATE_TYPE)

    def test_source_commit_and_repo_are_recorded_verbatim(self) -> None:
        stmt = self.generate()
        source = stmt["predicate"]["buildDefinition"]["externalParameters"]["source"]
        self.assertEqual(source["ref"], "deadbeef")
        self.assertIn("wrfcoin/core4", source["repository"])

    def test_missing_artifact_is_an_error_not_a_fabricated_statement(self) -> None:
        missing = self.tmp / "does-not-exist"
        rc = gen.main([
            "--artifact", str(missing), "--out", str(self.out),
            "--source-repo", "wrfcoin/core4", "--source-commit", "x",
            "--workflow-ref", "x", "--run-id", "1", "--run-attempt", "1",
            "--builder-id", "x", "--toolchain", "x", "--build-command", "x",
        ])
        self.assertEqual(rc, 1)
        self.assertFalse(self.out.exists())

    def test_verify_accepts_a_matching_artifact(self) -> None:
        self.generate()
        result = run_verify(self.artifact, self.out)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("OK", result.stdout)

    def test_verify_rejects_a_tampered_artifact(self) -> None:
        self.generate()
        with self.artifact.open("ab") as handle:
            handle.write(b"tampered")
        result = run_verify(self.artifact, self.out)
        self.assertEqual(result.returncode, 1)
        self.assertIn("digest mismatch", result.stderr)

    def test_verify_rejects_a_filename_mismatch(self) -> None:
        self.generate()
        renamed = self.tmp / "renamed-binary"
        renamed.write_bytes(self.artifact.read_bytes())
        result = run_verify(renamed, self.out)
        self.assertEqual(result.returncode, 1)
        self.assertIn("does not match artifact filename", result.stderr)

    def test_verify_rejects_a_missing_provenance_file(self) -> None:
        self.generate()
        result = run_verify(self.artifact, self.tmp / "nope.json")
        self.assertEqual(result.returncode, 1)
        self.assertIn("not found", result.stderr)

    def test_verify_rejects_a_provenance_file_with_two_subjects(self) -> None:
        self.generate()
        stmt = json.loads(self.out.read_text())
        stmt["subject"].append(dict(stmt["subject"][0]))
        multi = self.tmp / "multi.json"
        multi.write_text(json.dumps(stmt))
        result = run_verify(self.artifact, multi)
        self.assertEqual(result.returncode, 1)
        self.assertIn("exactly one subject", result.stderr)


RUSTC_VV = """rustc 1.98.0 (abcdef012 2026-08-01)
binary: rustc
commit-hash: abcdef0123456789
commit-date: 2026-08-01
host: x86_64-unknown-linux-gnu
release: 1.98.0
LLVM version: 20.1.0
"""
EXACT_COMMAND = (
    "cargo build --release --locked --bin launch_api "
    "--target x86_64-unknown-linux-gnu --features p2p"
)


class BuildInfoSidecar(unittest.TestCase):
    """buildCommand and toolchain are measured in the build job (review of
    core4#3137 finding 3), not asserted by the provenance job."""

    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp())
        self.artifact = self.tmp / "wrfcoin-node-linux-x86_64"
        self.artifact.write_bytes(b"fixture binary content")
        self.sidecar = self.tmp / "wrfcoin-node-linux-x86_64.buildinfo.json"
        self.out = self.tmp / "wrfcoin-node-linux-x86_64.provenance.json"
        (self.tmp / "cmd.txt").write_text(EXACT_COMMAND + "\n")
        (self.tmp / "vv.txt").write_text(RUSTC_VV)

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp, ignore_errors=True)

    def write_sidecar(self, expect: str = "1.98.0") -> int:
        return wbi.main([
            "--out", str(self.sidecar),
            "--command-file", str(self.tmp / "cmd.txt"),
            "--rustc-vv-file", str(self.tmp / "vv.txt"),
            "--expect-release", expect,
        ])

    def generate(self, *extra: str) -> int:
        return gen.main([
            "--artifact", str(self.artifact), "--out", str(self.out),
            "--source-repo", "wrfcoin/core4", "--source-commit", "deadbeef",
            "--workflow-ref", "w", "--run-id", "1", "--run-attempt", "1",
            "--builder-id", "b", *extra,
        ])

    def test_provenance_records_the_build_jobs_measured_command_and_toolchain(self) -> None:
        self.assertEqual(self.write_sidecar(), 0)
        self.assertEqual(self.generate("--build-info", str(self.sidecar)), 0)
        stmt = json.loads(self.out.read_text())
        definition = stmt["predicate"]["buildDefinition"]
        self.assertEqual(definition["externalParameters"]["buildCommand"], EXACT_COMMAND)
        self.assertEqual(definition["internalParameters"]["toolchain"], "rustc 1.98.0 (abcdef012 2026-08-01)")

    def test_a_toolchain_other_than_the_pin_is_refused_in_the_build_job(self) -> None:
        self.assertEqual(self.write_sidecar(expect="1.97.0"), 1)
        self.assertFalse(self.sidecar.exists())

    def test_missing_or_empty_sidecar_is_an_error_not_a_default(self) -> None:
        self.assertEqual(self.generate("--build-info", str(self.tmp / "absent.json")), 1)
        self.sidecar.write_text(json.dumps({"toolchain": "rustc 1.98.0", "buildCommand": ""}))
        self.assertEqual(self.generate("--build-info", str(self.sidecar)), 1)
        self.assertFalse(self.out.exists())

    def test_an_empty_command_file_is_refused(self) -> None:
        (self.tmp / "cmd.txt").write_text("  \n")
        self.assertEqual(self.write_sidecar(), 1)

    def test_build_info_and_direct_flags_are_mutually_exclusive(self) -> None:
        self.assertEqual(self.write_sidecar(), 0)
        with self.assertRaises(SystemExit):
            self.generate("--build-info", str(self.sidecar), "--toolchain", "x")
        with self.assertRaises(SystemExit):
            self.generate()


if __name__ == "__main__":
    unittest.main()
