#!/usr/bin/env python3
"""Write a release build's measured build-info sidecar (#2151).

Run inside the build job, right after `cargo build`, so the two provenance
fields that describe HOW the artifact was built are measured where it was
built, not asserted later by another job:

    python3 scripts/write_build_info.py \
        --out artifacts/<asset>.buildinfo.json \
        --command-file buildinfo/build-command.txt \
        --rustc-vv-file buildinfo/rustc-vV.txt \
        --expect-release 1.98.0

- `--command-file` holds the exact command line the build step executed
  (written by the step itself with `printf '%q '`).
- `--rustc-vv-file` holds `rustc -vV` output captured in the build job.
- `--expect-release` is the pinned RUST_VERSION; a mismatch between the pin
  and the toolchain that actually ran is a hard error, not a recorded fact.

`generate_provenance.py --build-info <sidecar>` reads the result.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

FORMAT_VERSION = 1


def parse_rustc_vv(text: str) -> dict:
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    if not lines or not lines[0].startswith("rustc "):
        raise ValueError("rustc -vV output does not start with a 'rustc ' version line")
    fields = {}
    for line in lines[1:]:
        key, sep, value = line.partition(":")
        if sep:
            fields[key.strip()] = value.strip()
    if not fields.get("release"):
        raise ValueError("rustc -vV output has no 'release:' field")
    return {"version_line": lines[0], "release": fields["release"], "host": fields.get("host", "")}


def build_info(command: str, rustc_vv: str, expect_release: str) -> dict:
    command = command.strip()
    if not command:
        raise ValueError("build command is empty")
    parsed = parse_rustc_vv(rustc_vv)
    if parsed["release"] != expect_release:
        raise ValueError(
            f"toolchain that ran ({parsed['release']}) is not the pinned RUST_VERSION ({expect_release})"
        )
    return {
        "formatVersion": FORMAT_VERSION,
        "buildCommand": command,
        "toolchain": parsed["version_line"],
        "rustcRelease": parsed["release"],
        "rustcHost": parsed["host"],
        "rustcVerbose": rustc_vv.strip(),
    }


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--command-file", required=True, type=Path)
    parser.add_argument("--rustc-vv-file", required=True, type=Path)
    parser.add_argument("--expect-release", required=True)
    args = parser.parse_args(argv)
    try:
        info = build_info(
            args.command_file.read_text(encoding="utf-8"),
            args.rustc_vv_file.read_text(encoding="utf-8"),
            args.expect_release,
        )
    except (OSError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(info, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {args.out}: {info['toolchain']} / {info['buildCommand']}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
