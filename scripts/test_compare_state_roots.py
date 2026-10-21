#!/usr/bin/env python3
"""Regression tests for scripts/compare_state_roots.py.

Pins the #2410 defect: the comparator crashed with a raw
`json.JSONDecodeError` when a leg's roots file was corrupted by toolchain
output, which on a real run read as "the determinism gate failed" when in fact
all five legs had agreed and only the *reader* was broken.

Run: python3 scripts/test_compare_state_roots.py
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "compare_state_roots.py"

CLEAN = {
    "0": "c6e94a15a3f37c00b4eedcd757e69ae84534339035019501de8c37413c2f89f3",
    "1": "ef17328cfe2468f9cb0c465df797b0633bc2cbaf8bbf5c05a38fba0cbfb9032b",
}

# Verbatim from the aarch64 leg of run 30139096624: rustup writes this to
# stdout inside the `cross` container, so a `>` redirect captured it ahead of
# the JSON. `json.loads` parses the leading "1.88" as a number and reports the
# remainder as `Extra data: line 2 column 7`.
TOOLCHAIN_PREAMBLE = "\n  1.98.0-x86_64-unknown-linux-gnu unchanged - rustc 1.98.0 (88d9e12ae 2026-08-18)\n\n"

failures: list[str] = []


def check(name: str, cond: bool, detail: str = "") -> None:
    if cond:
        print(f"  ok   {name}")
    else:
        print(f"  FAIL {name}: {detail}")
        failures.append(name)


def run(paths: list[Path]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--no-expected", *map(str, paths)],
        capture_output=True,
        text=True,
    )


def write(d: Path, name: str, body: str) -> Path:
    p = d / name
    p.write_text(body, encoding="utf-8")
    return p


def main() -> int:
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        clean = json.dumps(CLEAN, indent=2)

        # 1. All legs agree -> pass.
        legs = [write(d, f"roots-{n}.json", clean) for n in "abcde"]
        r = run(legs)
        check("agreeing legs pass", r.returncode == 0, r.stdout + r.stderr)

        # 2. A corrupted leg is reported as CORRUPT INPUT, not as divergence,
        #    and never as a bare traceback.
        bad = write(d, "roots-aarch64.json", TOOLCHAIN_PREAMBLE + clean)
        r = run(legs + [bad])
        out = r.stdout + r.stderr
        check("corrupt leg still fails the gate", r.returncode != 0, f"rc={r.returncode}")
        check("corrupt leg is not a traceback", "Traceback" not in out, out)
        check("corrupt leg names the file", "roots-aarch64.json" in out, out)
        check(
            "corrupt leg is distinguished from divergence",
            "CORRUPT INPUT" in out and "not a state-root divergence" in out,
            out,
        )

        # 3. THE ONE THAT MATTERS MOST: tolerating corrupt input must not make
        #    the gate permissive. A genuine divergence must still fail.
        diverged = dict(CLEAN)
        diverged["1"] = "0" * 64
        div = write(d, "roots-div.json", json.dumps(diverged, indent=2))
        r = run(legs + [div])
        out = r.stdout + r.stderr
        check("real divergence still fails", r.returncode != 0, f"rc={r.returncode}")
        check(
            "real divergence is reported as a root mismatch",
            "CORRUPT INPUT" not in out,
            out,
        )

        # 4. Truncated / empty files are corrupt input too, not agreement.
        empty = write(d, "roots-empty.json", "")
        r = run(legs + [empty])
        check("empty leg fails", r.returncode != 0, r.stdout + r.stderr)

    if failures:
        print(f"\nFAILED: {len(failures)} check(s): {', '.join(failures)}")
        return 1
    print("\nAll compare_state_roots regression checks passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
