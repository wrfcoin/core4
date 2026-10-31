"""Source checks on wrfcoin-node.ps1's testnet server key (no PowerShell needed).

-Action ServerKey creates %LOCALAPPDATA%\\WRFCoin\\server.key, the node's
software server key for bonding on testnet. The behaviour that matters is
checked on a real Windows runner by windows-node-build.yml; these checks pin
the properties that must not drift: testnet only, never overwritten, locked
before the seed is written, and handed to the node only when it exists.
"""

from __future__ import annotations

import unittest
from pathlib import Path

LAUNCHER = (Path(__file__).resolve().parent / "wrfcoin-node.ps1").read_text(encoding="utf-8")


def function_body(name: str) -> str:
    start = LAUNCHER.index(f"function {name}")
    end = LAUNCHER.index("\n}\n", start)
    return LAUNCHER[start:end]


class ServerKey(unittest.TestCase):
    def test_action_is_offered_and_dispatched(self) -> None:
        self.assertIn("'Setup', 'ServerKey')]", LAUNCHER)
        self.assertIn("'ServerKey' { Initialize-UserState; New-ServerKey }", LAUNCHER)

    def test_testnet_only_and_never_overwritten(self) -> None:
        body = function_body("New-ServerKey")
        self.assertIn("$cfg['WRFCOIN_NETWORK_ID'] -ne 'testnet'", body)
        exists = body.index("if (Test-Path -LiteralPath $ServerKeyFile)")
        write = body.index("WriteAllText($ServerKeyFile, (New-RandomHex))")
        self.assertLess(exists, write)
        self.assertIn("return", body[exists:write])

    def test_file_is_locked_before_the_seed_is_written(self) -> None:
        body = function_body("New-ServerKey")
        empty = body.index("WriteAllText($ServerKeyFile, '')")
        lock = body.index("Protect-UserOnly $ServerKeyFile", empty)
        seed = body.index("WriteAllText($ServerKeyFile, (New-RandomHex))")
        self.assertLess(empty, lock)
        self.assertLess(lock, seed)

    def test_start_relocks_and_passes_the_key_only_when_present(self) -> None:
        self.assertIn(
            "if (Test-Path -LiteralPath $ServerKeyFile) { Protect-UserOnly $ServerKeyFile }",
            function_body("Initialize-UserState"),
        )
        body = function_body("Start-Node")
        guard = body.index("if (Test-Path -LiteralPath $ServerKeyFile) {")
        self.assertIn(
            "SetEnvironmentVariable('WRFCOIN_SNAPSHOT_SERVER_KEY_FILE', $ServerKeyFile, 'Process')",
            body[guard:],
        )

    def test_seed_is_32_random_bytes_as_hex(self) -> None:
        body = function_body("New-RandomHex")
        self.assertIn("New-Object byte[] 32", body)
        self.assertIn("RandomNumberGenerator", body)
        self.assertIn("ToString('x2')", body)


if __name__ == "__main__":
    unittest.main()
