#!/usr/bin/env python3
"""Measure canonical JSON state-snapshot growth without proposing a price.

Input is either a JSON array or newline-delimited JSON snapshot objects:
{"height": 42, "state": {...}}.  The tool intentionally requires snapshots;
block bytes do not reveal state deletion, pruning, or database-local overhead.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, TextIO


@dataclass(frozen=True)
class Snapshot:
    height: int
    state: Any


@dataclass(frozen=True)
class Measurement:
    height: int
    state_bytes: int
    gross_member_bytes_added: int | None
    gross_member_bytes_removed: int | None
    net_snapshot_bytes: int | None


def canonical_json(state: Any) -> bytes:
    """Sorted compact UTF-8 JSON, never a database or consensus encoding."""
    return json.dumps(
        state, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def canonical_bytes(state: Any) -> int:
    """Logical snapshot bytes: canonical UTF-8 JSON, never database bytes."""
    return len(canonical_json(state))


def canonical_member_bytes(key: str, value: Any) -> int:
    """Bytes attributed to one top-level object member, excluding braces/commas."""
    return len(canonical_json(key)) + 1 + len(canonical_json(value))


def parse_snapshots(raw: str) -> list[Snapshot]:
    stripped = raw.strip()
    if not stripped:
        raise ValueError("input contains no snapshots")
    try:
        decoded = json.loads(stripped)
        records: Iterable[Any] = decoded if isinstance(decoded, list) else [decoded]
    except json.JSONDecodeError:
        records = [json.loads(line) for line in stripped.splitlines() if line.strip()]

    snapshots: list[Snapshot] = []
    previous: int | None = None
    for record in records:
        if not isinstance(record, dict) or set(record) != {"height", "state"}:
            raise ValueError("each snapshot must contain exactly height and state")
        height = record["height"]
        if isinstance(height, bool) or not isinstance(height, int) or height < 0:
            raise ValueError("snapshot height must be a non-negative integer")
        if previous is not None and height <= previous:
            raise ValueError("snapshot heights must be strictly increasing")
        snapshots.append(Snapshot(height, record["state"]))
        previous = height
    return snapshots


def measure(snapshots: Iterable[Snapshot]) -> list[Measurement]:
    previous: Snapshot | None = None
    out: list[Measurement] = []
    for snapshot in snapshots:
        current = canonical_bytes(snapshot.state)
        if previous is None:
            out.append(Measurement(snapshot.height, current, None, None, None))
            previous = snapshot
            continue
        if not isinstance(previous.state, dict) or not isinstance(snapshot.state, dict):
            raise ValueError(
                "state must be a JSON object for structural growth measurement"
            )
        previous_state = previous.state
        current_state = snapshot.state
        added = 0
        removed = 0
        for key in sorted(set(previous_state) | set(current_state)):
            before = previous_state.get(key)
            after = current_state.get(key)
            if key not in previous_state:
                added += canonical_member_bytes(key, after)
            elif key not in current_state:
                removed += canonical_member_bytes(key, before)
            elif canonical_json(before) != canonical_json(after):
                removed += canonical_member_bytes(key, before)
                added += canonical_member_bytes(key, after)
        out.append(
            Measurement(
                snapshot.height,
                current,
                added,
                removed,
                current - canonical_bytes(previous.state),
            )
        )
        previous = snapshot
    return out


def render(rows: list[Measurement], output_format: str) -> str:
    if output_format == "json":
        return json.dumps([row.__dict__ for row in rows], separators=(",", ":"))
    lines = [
        "| height | state bytes | gross member bytes added | gross member bytes removed | net snapshot bytes |",
        "| ---: | ---: | ---: | ---: | ---: |",
    ]
    lines.extend(
        f"| {r.height} | {r.state_bytes} | "
        f"{r.gross_member_bytes_added if r.gross_member_bytes_added is not None else '—'} | "
        f"{r.gross_member_bytes_removed if r.gross_member_bytes_removed is not None else '—'} | "
        f"{r.net_snapshot_bytes if r.net_snapshot_bytes is not None else '—'} |"
        for r in rows
    )
    return "\n".join(lines)


def self_test() -> None:
    rows = measure(
        parse_snapshots(
            '[{"height":1,"state":{"a":1}},{"height":2,"state":{"a":1,"b":2}},'
            '{"height":3,"state":{"a":2}},{"height":4,"state":{}}]'
        )
    )
    assert rows[0].gross_member_bytes_added is None
    assert rows[1].gross_member_bytes_added and rows[1].gross_member_bytes_added > 0
    assert rows[2].gross_member_bytes_added and rows[2].gross_member_bytes_added > 0
    assert rows[2].gross_member_bytes_removed and rows[2].gross_member_bytes_removed > 0
    assert rows[3].gross_member_bytes_removed and rows[3].gross_member_bytes_removed > 0
    assert canonical_bytes({"station": "Zürich-Höngg"}) == len(
        canonical_json({"station": "Zürich-Höngg"})
    )
    try:
        parse_snapshots('[{"height":2,"state":{}},{"height":2,"state":{}}]')
    except ValueError:
        return
    raise AssertionError("duplicate height must be refused")


def main(stdin: TextIO = sys.stdin) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", help="snapshot JSON file, or - for stdin")
    parser.add_argument("--format", choices=("markdown", "json"), default="markdown")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        print("measure_state_growth self-test: PASS")
        return 0
    if not args.input:
        parser.error("--input is required unless --self-test is used")
    raw = stdin.read() if args.input == "-" else Path(args.input).read_text(encoding="utf-8")
    print(render(measure(parse_snapshots(raw)), args.format))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"measure_state_growth: {error}", file=sys.stderr)
        raise SystemExit(2)
