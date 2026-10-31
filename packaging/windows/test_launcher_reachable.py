"""Source checks on wrfcoin-node.ps1's per-user reachable.conf (no PowerShell needed).

%LOCALAPPDATA%\\WRFCoin\\reachable.conf lets a user open a P2P port without
editing the installed chain-888.conf. It may set only WRFCOIN_P2P_HOST and
WRFCOIN_P2P_EXTERNAL_ADDR. The behaviour is exercised on a real Windows runner
by windows-node-build.yml; these checks pin the allow-list and the order.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

LAUNCHER = (Path(__file__).resolve().parent / "wrfcoin-node.ps1").read_text(encoding="utf-8")


def function_body(name: str) -> str:
    start = LAUNCHER.index(f"function {name}")
    end = LAUNCHER.index("\n}\n", start)
    return LAUNCHER[start:end]


class ReachableConfig(unittest.TestCase):
    def test_allow_list_is_exactly_the_two_p2p_keys(self) -> None:
        match = re.search(r"\$ReachableKeys = @\(([^)]*)\)", LAUNCHER)
        self.assertIsNotNone(match)
        keys = re.findall(r"'([A-Z0-9_]+)'", match.group(1))
        self.assertEqual(keys, ["WRFCOIN_P2P_HOST", "WRFCOIN_P2P_EXTERNAL_ADDR"])

    def test_reader_refuses_other_keys_duplicates_and_bad_values(self) -> None:
        body = function_body("Read-ReachableConfig")
        self.assertIn("$ReachableKeys -cnotcontains $key", body)  # case-sensitive
        self.assertIn("defines $key twice", body)
        self.assertIn("if (-not (Test-ReachableValue $key $value))", body)
        # A missing file is fine: nothing changes.
        self.assertIn("if (-not (Test-Path -LiteralPath $ReachableConfig)) { return $pairs }", body)

    def test_values_are_validated_per_key(self) -> None:
        body = function_body("Test-ReachableValue")
        self.assertIn("'WRFCOIN_P2P_HOST' { return (Test-Ipv4 $Value) }", body)
        self.assertIn("^/(ip4|dns4)/([^/\\s]+)/tcp/(\\d{1,5})$", body)
        self.assertIn("$port -lt 1 -or $port -gt 65535", body)
        # Any key the switch does not name is invalid.
        self.assertTrue(body.rstrip().endswith("return $false"))

    def test_overrides_apply_after_the_checked_chain_config(self) -> None:
        body = function_body("Start-Node")
        cfg = body.index("$cfg = Read-NodeConfig $Config")
        reach = body.index("$reach = Read-ReachableConfig")
        apply_cfg = body.index("foreach ($key in $cfg.Keys)")
        apply_reach = body.index("foreach ($key in $reach.Keys)")
        self.assertLess(cfg, reach)
        self.assertLess(apply_cfg, apply_reach)
        # chain-888.conf is still required to be the sync-only chain-888 file.
        self.assertIn("$pairs['WRFCOIN_SYNC_ONLY'] -ne '1'", function_body("Read-NodeConfig"))


if __name__ == "__main__":
    unittest.main()
