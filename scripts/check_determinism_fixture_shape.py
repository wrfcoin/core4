#!/usr/bin/env python3
"""Shape gate for the determinism golden fixture (secret-scanning#2885).

`tests/integration/fixtures/determinism/expected_roots.json` records
consensus state-root hashes, not credentials — but every regeneration
(`REGENERATE_DETERMINISM_FIXTURE=1`, per `determinism.rs`) rewrites all ten
hex values, and `detect-secrets` fingerprints candidate secrets by hashing
their own bytes. So each regeneration produces ten new "Hex High Entropy
String" findings the baseline has never seen, and marking them acknowledged
fixes exactly one regeneration: three rounds of that (2026-07-30, and twice
before) already left 30 stale, unmatchable entries sitting in
`.secrets.baseline` before this file existed. Excluding the file from the
scan by exact path is the durable fix — but a blank exclusion is fail-open:
anything could land in an unscanned file and nothing would notice. This gate
is what makes the exclusion safe. It does not pin exact bytes (the file is
SUPPOSED to change) — it pins the SHAPE every legitimate entry must have:

  1. The file is a JSON object.
  2. Every key parses as a non-negative integer (a block height).
  3. Every value is exactly 64 lowercase hex characters (a `Hash::to_hex()`
     state root — see `shared-protocol/src/crypto/hash.rs`).

A real secret (an API key, a JWT, a connection string) essentially never
happens to be a 64-character lowercase hex string by coincidence, so this
does not certify "no secret could ever pass," only "nothing that fails this
shape can be silently excluded here." That is the same qualified guarantee
`check_wycheproof_vendor.py` gives for its own excluded directory, by a
different mechanism (byte-pin, because that corpus is static; shape-pin,
because this fixture is not).

Independently, `determinism.rs`'s own test asserts every value against a
live-computed root and fails loudly on any mismatch — so a value that is
shape-valid but wrong would already break the test that consumes this file,
not just this gate.

Run standalone, or with --self-test to check the gate itself.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

FIXTURE_PATH = Path("tests/integration/fixtures/determinism/expected_roots.json")

HEX_ROOT = re.compile(r"^[0-9a-f]{64}$")


def check(root: Path) -> list[str]:
    """Return a list of failure messages; empty means the gate passes."""
    path = root / FIXTURE_PATH
    errors: list[str] = []

    if not path.is_file():
        return [f"fixture missing: {FIXTURE_PATH}"]

    try:
        raw = path.read_text()
    except OSError as exc:
        return [f"cannot read {FIXTURE_PATH}: {exc}"]

    try:
        data = json.loads(raw)
    except json.JSONDecodeError as exc:
        return [f"{FIXTURE_PATH} is not valid JSON: {exc}"]

    if not isinstance(data, dict):
        return [f"{FIXTURE_PATH} must be a JSON object, got {type(data).__name__}"]

    if not data:
        return [f"{FIXTURE_PATH} has no entries — an empty fixture is not a passing fixture"]

    for key, value in data.items():
        if not isinstance(key, str) or not key.isdigit():
            errors.append(f"key {key!r} is not a non-negative integer (block height)")
        if not isinstance(value, str) or not HEX_ROOT.match(value):
            shown = value if isinstance(value, str) else repr(value)
            errors.append(
                f"value at key {key!r} is not a 64-character lowercase hex string: {shown!r}"
            )

    return errors


def self_test() -> int:
    """Prove the gate fails on a shape violation and passes on the real file."""
    import shutil
    import tempfile

    root = Path.cwd()
    if check(root):
        print("self-test: FAIL — the real fixture does not pass the clean gate")
        return 1

    with tempfile.TemporaryDirectory() as tmp:
        sandbox = Path(tmp)
        (sandbox / FIXTURE_PATH).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(root / FIXTURE_PATH, sandbox / FIXTURE_PATH)

        if check(sandbox):
            print("self-test: FAIL — copied fixture should pass")
            return 1

        # Violation A: a non-hex value (what a smuggled secret would look like).
        target = sandbox / FIXTURE_PATH
        original = json.loads(target.read_text())
        planted = dict(original)
        first_key = next(iter(planted))
        planted[first_key] = "sk_live_not_a_state_root_at_all_1234567890"
        target.write_text(json.dumps(planted))
        if not any("not a 64-character lowercase hex string" in e for e in check(sandbox)):
            print("self-test: FAIL — non-hex value was not detected")
            return 1

        # Violation B: a non-integer key.
        target.write_text(json.dumps({"not-a-height": "a" * 64}))
        if not any("not a non-negative integer" in e for e in check(sandbox)):
            print("self-test: FAIL — non-integer key was not detected")
            return 1

        # Violation C: malformed JSON.
        target.write_text("{not json")
        if not any("not valid JSON" in e for e in check(sandbox)):
            print("self-test: FAIL — malformed JSON was not detected")
            return 1

    print("self-test: OK — non-hex value, non-integer key, and malformed JSON all detected")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="verify the gate detects shape violations and accepts the real fixture",
    )
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    errors = check(Path.cwd())
    if errors:
        print(f"::error::{FIXTURE_PATH} failed its shape gate:")
        for err in errors:
            print(f"  {err}")
        print()
        print(
            f"This file is excluded from detect-secrets scanning because it is "
            f"regenerated on every state-root material change, so every entry must "
            f"still look like a state root. If this is a legitimate regeneration, "
            f"re-run it via REGENERATE_DETERMINISM_FIXTURE=1 (see determinism.rs) "
            f"rather than hand-editing the file."
        )
        return 1

    count = len(json.loads((Path.cwd() / FIXTURE_PATH).read_text()))
    print(f"determinism fixture shape: OK — {count} entries, all 64-char hex state roots")
    return 0


if __name__ == "__main__":
    sys.exit(main())
