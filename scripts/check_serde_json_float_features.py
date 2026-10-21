#!/usr/bin/env python3
"""Fail if serde_json's float-parsing mode could differ between node builds (#3333).

# The hazard

Signed consensus payloads such as the station registration encode f64 fields as their raw
IEEE-754 bits, and every verifier recovers those bits by parsing the tx's JSON
with serde_json. serde_json's default float parser is NOT correctly rounded.
Two cargo features change that parse:

* ``float_roundtrip`` switches to a correctly rounded parser; and
* ``arbitrary_precision`` routes numbers through a string representation.

Cargo unifies features across the build graph, so if ANY crate in a node's
graph (a new dependency, or a dev-dependency pulled in by a workspace-wide build)
enables either one, that binary parses some floats one ulp differently from a
binary built without it. Two such nodes then disagree on whether a signature
verifies, and on the lat/lon written to state: a chain split that no unit test
of either build would see. #3333 is the benign half of this (the CLI signed
bits that serde_json then parsed differently). This guard keeps it benign.

# What this checks

``cargo tree --workspace -e features -i serde_json`` for the default and the
``--all-features`` feature sets, across all targets. It fails (exit 1) if:

* a forbidden feature appears;
* the output shows no serde_json feature line at all. A parse that sees nothing
  must not pass: that is a blind checker, not a clean tree; or
* cargo itself fails. This includes cargo refusing ``-i serde_json`` because
  two serde_json versions are in the graph, which is a divergence risk in its own
  right and so should also stop the build.

The durable fix, a preimage that does not depend on the lossy parse, is
consensus-changing and tracked separately. Removing this guard is safe only
after that fix has activated.

Usage:
    python3 scripts/check_serde_json_float_features.py            # run cargo tree
    python3 scripts/check_serde_json_float_features.py --tree-file OUT.txt
    python3 scripts/check_serde_json_float_features.py --self-test
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys

FORBIDDEN = ("float_roundtrip", "arbitrary_precision")
FEATURE_LINE = re.compile(r'serde_json feature "([A-Za-z0-9_\-]+)"')

# Each is (label, extra cargo args). `--target all` so a target-gated dependency
# cannot hide an activation from a host-only resolve.
FEATURE_SETS = (
    ("default features", []),
    ("--all-features", ["--all-features"]),
)


def analyze(tree_output: str) -> tuple[set[str], list[str]]:
    """Return (features seen, problems) for one `cargo tree` output."""
    seen = set(FEATURE_LINE.findall(tree_output))
    problems = []
    if not seen:
        problems.append(
            "no `serde_json feature \"…\"` line found: the scan is blind, refusing to pass"
        )
    for feature in FORBIDDEN:
        if feature in seen:
            problems.append(
                f"serde_json feature `{feature}` is enabled in the build graph: it changes "
                "how every node parses signed float fields, so builds with and without it "
                "can disagree on signature validity and state (#3333)"
            )
    return seen, problems


def run_cargo_tree(extra: list[str]) -> tuple[int, str]:
    cmd = [
        "cargo", "tree", "--workspace", "--locked", "--target", "all",
        "-e", "features", "-i", "serde_json", *extra,
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    return proc.returncode, proc.stdout + proc.stderr


def check_live() -> int:
    failed = False
    for label, extra in FEATURE_SETS:
        code, output = run_cargo_tree(extra)
        if code != 0:
            print(f"FAIL [{label}]: cargo tree exited {code}:\n{output}", file=sys.stderr)
            failed = True
            continue
        seen, problems = analyze(output)
        for problem in problems:
            print(f"FAIL [{label}]: {problem}", file=sys.stderr)
        failed |= bool(problems)
        print(f"[{label}] serde_json features: {', '.join(sorted(seen)) or '(none)'}")
    if failed:
        return 1
    print("check_serde_json_float_features: no float-parse-changing serde_json feature in the graph")
    return 0


def self_test() -> int:
    clean = (
        'serde_json v1.0.149\n'
        '├── serde_json feature "alloc"\n'
        '├── serde_json feature "default"\n'
        '└── serde_json feature "std"\n'
    )
    cases = [
        ("clean tree passes", clean, False),
        ("float_roundtrip fails", clean + '└── serde_json feature "float_roundtrip"\n', True),
        ("arbitrary_precision fails",
         clean + '    └── serde_json feature "arbitrary_precision" (*)\n', True),
        ("blind (no feature lines) fails", "serde_json v1.0.149\n", True),
        ("empty output fails", "", True),
    ]
    ok = True
    for name, text, expect_fail in cases:
        _, problems = analyze(text)
        passed = bool(problems) == expect_fail
        ok &= passed
        print(f"{'ok  ' if passed else 'FAIL'} self-test: {name}")
    return 0 if ok else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--self-test", action="store_true")
    group.add_argument("--tree-file", help="analyze saved `cargo tree` output instead")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.tree_file:
        with open(args.tree_file, encoding="utf-8") as handle:
            seen, problems = analyze(handle.read())
        for problem in problems:
            print(f"FAIL: {problem}", file=sys.stderr)
        print(f"serde_json features: {', '.join(sorted(seen)) or '(none)'}")
        return 1 if problems else 0
    return check_live()


if __name__ == "__main__":
    sys.exit(main())
