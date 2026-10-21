#!/usr/bin/env python3
"""Integrity gate for the vendored Project Wycheproof corpus.

The four upstream vector files are excluded from `detect-secrets` scanning: they
are third-party test data whose whole purpose is to contain thousands of
high-entropy hex strings, and baselining ~1,800 of them would make the baseline
unreviewable.

That exclusion is only safe if something else guarantees the directory holds
exactly the audited bytes. This gate is that something else. It enforces two
properties:

  1. Each excluded file hashes to its pinned SHA-256 from PROVENANCE.md.
  2. The vendor directory contains no file outside the allow-list.

Without (2) the exclusion would be fail-open — a new file dropped into the
directory would inherit the exclusion and never be scanned. Without (1) an
existing file could be edited to carry a secret.

Run standalone, or with --self-test to check the gate itself.
"""

from __future__ import annotations

import argparse
import hashlib
import sys
from pathlib import Path

VENDOR_DIR = Path("shared-protocol/tests/fixtures/wycheproof/upstream")

# Pinned at C2SP/wycheproof b61843a9a5115bb758134b6a1f5d5e502d445342.
# These must match the table in PROVENANCE.md.
PINNED_SHA256 = {
    "ed25519_test.json": "70471c053c711731f2195ef4875b60ea7f5d6793939d99058ac12da810cb8e00",
    "ecdsa_secp256k1_sha256_test.json": "43db761c0a2eae71fb0755d355d5130e28ce64a5b07846cf27e7072082597a81",
    "ecdsa_secp256k1_sha3_256_test.json": "4748f02694d2391576cf28c8cb3d13149739b5602eeb079deae4577e111afbc1",
    "ecdsa_secp256r1_sha256_p1363_test.json": "c60de693930e386c3a5472d08081623ef8504decc54b38ac01ec6b2a2575c986",
}

# Files permitted in the vendor directory. The four JSON files are excluded
# from secret scanning and so must be hash-pinned; LICENSE and PROVENANCE.md are
# NOT excluded and are still scanned normally, but they are still allow-listed
# here so that an unexpected file is caught.
ALLOWED_UNPINNED = {"LICENSE", "PROVENANCE.md"}


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def check(root: Path) -> list[str]:
    """Return a list of failure messages; empty means the gate passes."""
    vendor = root / VENDOR_DIR
    errors: list[str] = []

    if not vendor.is_dir():
        return [f"vendor directory missing: {VENDOR_DIR}"]

    present = {p.name for p in vendor.iterdir() if p.is_file()}
    allowed = set(PINNED_SHA256) | ALLOWED_UNPINNED

    # (2) no unexpected files — this is what keeps the scan exclusion fail-closed
    for unexpected in sorted(present - allowed):
        errors.append(
            f"unexpected file in secret-scan-excluded vendor directory: "
            f"{VENDOR_DIR}/{unexpected}"
        )

    for missing in sorted(allowed - present):
        errors.append(f"missing expected vendor file: {VENDOR_DIR}/{missing}")

    # (1) pinned bytes
    for name, expected in sorted(PINNED_SHA256.items()):
        path = vendor / name
        if not path.is_file():
            continue  # already reported as missing
        actual = sha256_of(path)
        if actual != expected:
            errors.append(
                f"checksum mismatch for {VENDOR_DIR}/{name}\n"
                f"    expected {expected}\n"
                f"    actual   {actual}"
            )

    return errors


def self_test() -> int:
    """Prove the gate fails on both classes of violation it exists to catch."""
    import shutil
    import tempfile

    root = Path.cwd()
    if check(root):
        print("self-test: FAIL — the real tree does not pass the clean gate")
        return 1

    with tempfile.TemporaryDirectory() as tmp:
        sandbox = Path(tmp)
        (sandbox / VENDOR_DIR).parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(root / VENDOR_DIR, sandbox / VENDOR_DIR)

        if check(sandbox):
            print("self-test: FAIL — copied tree should pass")
            return 1

        # Violation A: an unexpected file appears in the excluded directory.
        planted = sandbox / VENDOR_DIR / "extra_vectors.json"
        planted.write_text('{"note": "not audited"}\n')
        if not any("unexpected file" in e for e in check(sandbox)):
            print("self-test: FAIL — unexpected file was not detected")
            return 1
        planted.unlink()

        # Violation B: an audited file is modified.
        target = sandbox / VENDOR_DIR / "ed25519_test.json"
        target.write_bytes(target.read_bytes() + b"\n")
        if not any("checksum mismatch" in e for e in check(sandbox)):
            print("self-test: FAIL — modified vector file was not detected")
            return 1

    print("self-test: OK — both unexpected-file and checksum violations detected")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="verify the gate detects an unexpected file and a modified vector",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    errors = check(Path.cwd())
    if errors:
        print("::error::vendored Wycheproof corpus failed its integrity gate:")
        for err in errors:
            print(f"  {err}")
        print()
        print(
            "These files are excluded from detect-secrets scanning, so their bytes "
            "must stay exactly as audited. If an upstream re-pin is intended, update "
            "PINNED_SHA256 in this script and the table in PROVENANCE.md together."
        )
        return 1

    count = len(PINNED_SHA256)
    print(f"wycheproof vendor: OK — {count} pinned file(s) match, no unexpected files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
