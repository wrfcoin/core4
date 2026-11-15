#!/usr/bin/env python3
"""Source removal guard for core4-owned disclaimer surfaces (not deployment proof)."""
import argparse
from pathlib import Path

URL = "https://wrfcoin.com/testnet-disclaimer.html"
NOTICE = "This is a test network. It may be reset without notice. Balances and tokens have no monetary value. No migration to mainnet is promised or implied. Please attach no production bridges, exchange listings or irreplaceable assets."


def check(root):
    required = {
        "docs/TESTNET_DISCLAIMER.md": [NOTICE, URL, "security/TESTNET_GO_NO_GO.md#must-not-be-attached", "attacked on a schedule"],
        "README.md": ["[reset-and-expectations disclaimer](docs/TESTNET_DISCLAIMER.md)"],
        "native-chain/src/bin/launch_api/main.rs": [
            'if declared_identity.chain_id == "777" {',
            'log::warn!("' + NOTICE + ' ' + URL + '");',
        ],
        "native-chain/src/bin/launch_api/routes/network.rs": ['"disclaimer_url": "' + URL + '"'],
        ".github/workflows/disclaimer-surfaces.yml": ["pull_request:", "python3 scripts/check_disclaimer_surfaces.py"],
    }
    failures = []
    for path, needles in required.items():
        file = root / path
        content = file.read_text() if file.is_file() else ""
        for needle in needles:
            if needle not in content:
                failures.append(f"{path}: missing {needle!r}")
    return failures


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    failures = check(args.root)
    for failure in failures:
        print(failure)
    if failures:
        raise SystemExit(1)
    print("PASS: canonical document, README, testnet startup, network info and CI surfaces")
