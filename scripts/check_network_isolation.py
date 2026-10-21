#!/usr/bin/env python3
"""Offline testnet preflight (#2161). Never connects to hosts or prints config values.

Input is a JSON public projection with chain_id, network_id, genesis_hash and
producer_public_keys (list of lowercase hex public keys). Export it locally from
the candidate deployment configuration; this tool does not attest a running node.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]


def unique_json(text: str) -> dict:
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("duplicate JSON field")
            result[key] = value
        return result

    value = json.loads(text, object_pairs_hook=pairs)
    if not isinstance(value, dict):
        raise ValueError("expected JSON object")
    return value


def canonical(root: Path = ROOT) -> dict:
    """Extract pins from production Rust literals; refuse unknown source shapes."""
    source = (root / "shared-protocol/src/crypto/chain_binding.rs").read_text()
    table = re.search(
        r"pub const LEGACY_CHAIN_BOOTSTRAP:.*?=\s*&\[(.*?)\];", source, re.S
    )
    if table is None:
        raise ValueError("production identity table unavailable")
    rows = re.findall(
        r'chain_id:\s*"([^"\n]+)",\s*network_id:\s*"([^"\n]+)",\s*genesis_hash:\s*"([0-9a-f]{64})"',
        table[1],
    )
    if (
        not rows
        or len(set(rows)) != 1
        or len(rows) != table[1].count("LegacyChainBootstrap {")
    ):
        raise ValueError("production identity table needs explicit selection")
    chain, network, genesis = rows[0]
    config = (root / "p2p/src/config.rs").read_text()
    p2p_pin = re.search(
        r'const TESTNET_GENESIS_HASH:\s*&str\s*=\s*"([0-9a-f]{64})"', config
    )
    if p2p_pin is None or p2p_pin[1] != genesis:
        raise ValueError("production genesis pins disagree")
    producer_source = (
        root / "native-chain/src/bin/launch_api/genesis_producer_pins.rs"
    ).read_text()
    artifact = re.search(
        r"pub const TESTNET_GENESIS_PRODUCER_SET:.*?Some\(concat!\((.*?)\)\);",
        producer_source,
        re.S,
    )
    if artifact is None:
        raise ValueError("production producer pin unavailable")
    # Tokenize only the concat! literal: comments cannot contribute quoted data.
    tokens = re.findall(r'//[^\n]*|r#".*?"#|"(?:\\.|[^"\\])*"', artifact[1], re.S)
    parts = [
        t[3:-2] if t.startswith('r#"') else json.loads(t)
        for t in tokens
        if not t.startswith("//")
    ]
    roster = unique_json("".join(parts))["members"]
    keys = [m["public_key"] for m in roster]
    if (
        len(keys) != 3
        or len(set(keys)) != 3
        or not all(re.fullmatch(r"[0-9a-f]{64}", k) for k in keys)
    ):
        raise ValueError("production producer roster is not three distinct keys")
    return dict(
        chain_id=chain,
        network_id=network,
        genesis_hash=genesis,
        producer_public_keys=sorted(keys),
    )


def check(config: dict, pins: dict) -> list[str]:
    problems = []
    for field in ("chain_id", "network_id", "genesis_hash"):
        if config.get(field) != pins[field]:
            problems.append(field + " missing or mismatched")
    keys = config.get("producer_public_keys")
    if (
        not isinstance(keys, list)
        or not all(isinstance(k, str) for k in keys)
        or len(keys) != 3
        or sorted(keys) != pins["producer_public_keys"]
    ):
        problems.append("producer_public_keys missing or mismatched")
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "config",
        type=Path,
        help="local JSON public projection of candidate testnet config",
    )
    args = parser.parse_args()
    try:
        problems = check(unique_json(args.config.read_text()), canonical())
    except (OSError, ValueError, KeyError, TypeError):
        print(
            "FAIL: unreadable/malformed config or unavailable canonical pins",
            file=sys.stderr,
        )
        return 2
    if problems:
        print("FAIL: " + "; ".join(problems), file=sys.stderr)
        return 1
    print(
        "PASS: testnet identity and three producer public keys match production pins (offline source comparison)"
    )
    print(
        "DEFERRED: mainnet genesis/key comparison and rejection before authentication (owner ruling 2026-10-07)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
