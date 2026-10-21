#!/usr/bin/env python3
"""Count the test executables a `cargo test --no-run` produced, and enforce a floor.

WHY THIS IS A FILE AND NOT `python3 -c '...'` (#2899 review)
------------------------------------------------------------
This counter used to live inline in `test-target-compile.yml` inside a
single-quoted `python3 -c '...'`. A calibration comment containing the word
`floor's` **terminated that shell quote**: everything after it was parsed as
shell, the job counted 138 executables and then died with
`FLOOR: command not found`, direct exit 127, on the real `n5pro-ct102`
runner. An apostrophe in a comment took out the gate the comment documented.

Escaping the apostrophe would have fixed that instance. Moving the code into
a file removes the whole class — a `.py` file has no enclosing shell quote to
break — and it makes the counter testable, which an inline heredoc never was.

CALIBRATION LOG — append-only. A bare integer with no record of what it was
measured against is the decay #2889 exists to stop.

    2026-08-30, gate written:   measured 124  ->  FLOOR 110
    2026-08-31, main f2adb752f: measured 138  ->  FLOOR 122

Measurement method is this script's own counter, over the JSON stream of:

    cargo test --locked --workspace --features hsm --no-run --message-format=json

counting `compiler-artifact` messages that carry an `executable` and have
`profile.test` set. Doc-test targets emit no executable and are not counted.

MARGIN: 122 is 138 less ~11.6%, holding the ~11% relative margin the original
110/124 established. Raising the floor when targets grow is expected — add a
dated line above. Lowering it needs the same scrutiny as deleting a test.
"""

from __future__ import annotations

import argparse
import json
import sys
from typing import Iterable


def count_test_executables(lines: Iterable[str]) -> int:
    """Number of test executables in a cargo `--message-format=json` stream.

    Non-JSON lines are ignored: cargo interleaves human-readable output, and a
    parser that died on it would turn a cosmetic change into a gate failure.
    """
    executables = 0
    for line in lines:
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (
            message.get("reason") == "compiler-artifact"
            and message.get("executable")
            and message.get("profile", {}).get("test")
        ):
            executables += 1
    return executables


def positive_int(value: str) -> int:
    """argparse type for --floor: strictly positive.

    A floor of zero would accept a build that produced nothing at all, which
    is the vacuity this gate exists to reject. Same reasoning as
    `cargo_test_nonvacuous.py`'s `--min-executed`.
    """
    try:
        number = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError(f"not an integer: {value!r}") from error
    if number < 1:
        raise argparse.ArgumentTypeError(
            f"--floor must be >= 1, got {number}: a nonpositive floor would "
            "accept a build that compiled no test executables at all"
        )
    return number


def self_test() -> int:
    """Prove the counter and the floor can both reach RED, with no cargo."""
    artifact = json.dumps(
        {"reason": "compiler-artifact", "executable": "/t/x", "profile": {"test": True}}
    )
    not_a_test = json.dumps(
        {"reason": "compiler-artifact", "executable": "/t/x", "profile": {"test": False}}
    )
    no_exe = json.dumps(
        {"reason": "compiler-artifact", "executable": None, "profile": {"test": True}}
    )
    checks = [
        ("counts test executables", count_test_executables([artifact] * 3), 3),
        ("ignores non-test artifacts", count_test_executables([not_a_test] * 3), 0),
        ("ignores artifacts with no executable", count_test_executables([no_exe]), 0),
        ("ignores non-JSON noise", count_test_executables(["   Compiling foo\n", artifact]), 1),
        ("empty stream counts zero", count_test_executables([]), 0),
    ]
    for label, actual, expected in checks:
        if actual != expected:
            print(f"SELF_TEST_FAILED {label}: {actual} != {expected}", file=sys.stderr)
            return 1
    for bad in ("0", "-1"):
        try:
            positive_int(bad)
        except argparse.ArgumentTypeError:
            continue
        print(f"SELF_TEST_FAILED nonpositive floor {bad!r} was accepted", file=sys.stderr)
        return 1
    print("count_test_executables.py self-test passed (counter and floor both reach RED)")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--floor", type=positive_int)
    args = parser.parse_args()

    if args.self_test:
        return self_test()
    if args.floor is None:
        parser.error("--floor is required (or use --self-test)")

    executables = count_test_executables(sys.stdin)
    print(f"compiled test executables: {executables}")
    if executables < args.floor:
        print(
            f"FAIL: only {executables} test executables compiled; floor is {args.floor}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
