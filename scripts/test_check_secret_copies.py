#!/usr/bin/env python3
"""Planted-leak regression suite for check_secret_copies.py (#2128).

Review of PR #2238 supplied `handle.expose_secret().to_vec()` plus
`println!("{copy:?}")` and the scanner exited 0 — it advertised a
no-owned-copy/no-format property it did not enforce. Every name-based rule is
blind to that by construction, because the binding can be called anything.

These fixtures pin both halves: the copy off the accessor, and the renamed
binding reaching a sink.

Run: python3 scripts/test_check_secret_copies.py
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIXTURES = HERE / "tests" / "secret_copy_fixtures"
SCANNER = HERE / "check_secret_copies.py"
REPO = HERE.parent

MUST_REJECT = {
    "expose_secret_to_vec.rs.txt": "owned copy off a secret accessor, then logged",
    "renamed_binding_logged.rs.txt": "secret renamed to an innocuous binding, then logged",
    "derived_debug_secret.rs.txt": "derived Debug on a secret-named struct",
}
MUST_ACCEPT = {"clean.rs.txt": "logs a fingerprint, never the material"}


def run(target: Path) -> int:
    return subprocess.run(
        [sys.executable, str(SCANNER), str(target)],
        capture_output=True, text=True, cwd=REPO,
    ).returncode


def main() -> int:
    failures = []
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        for name, why in MUST_REJECT.items():
            f = FIXTURES / name
            dest = tmp / name.removesuffix(".txt")
            dest.write_text(f.read_text())
            if run(dest) == 0:
                failures.append(f"NOT DETECTED: {name} [{why}] exited 0")
        for name, why in MUST_ACCEPT.items():
            f = FIXTURES / name
            dest = tmp / name.removesuffix(".txt")
            dest.write_text(f.read_text())
            if run(dest) != 0:
                failures.append(f"FALSE POSITIVE: {name} [{why}] was rejected")

    if failures:
        print(f"check_secret_copies self-test: {len(failures)} failure(s)")
        for f in failures:
            print(f"  {f}")
        return 1
    print(
        f"check_secret_copies self-test: all "
        f"{len(MUST_REJECT) + len(MUST_ACCEPT)} checks passed"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
