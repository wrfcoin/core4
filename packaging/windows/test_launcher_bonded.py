"""Bonded mode in wrfcoin-node.ps1's reachable.conf.

A Windows node switches to bonded mode only when the per-user reachable.conf
says so: WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS=1 plus WRFCOIN_SERVICE_ID, together
with an open P2P port (WRFCOIN_P2P_HOST, WRFCOIN_P2P_EXTERNAL_ADDR) and the
testnet server.key. Without that file the node is the sync-only node the
installer sets up, and the shipped chain-888.conf never turns bonded mode on.

Two kinds of checks:

* source checks (always run): the shipped defaults and the order in Start-Node;
* behaviour checks (need PowerShell: pwsh/powershell, or pwsh.exe from WSL):
  the launcher's own Read-ReachableConfig and Resolve-NodeMode, loaded from the
  script's syntax tree with Fail turned into an exception, run against real
  reachable.conf files. The Windows CI job runs both.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
LAUNCHER_PATH = HERE / "wrfcoin-node.ps1"
LAUNCHER = LAUNCHER_PATH.read_text(encoding="utf-8")
CHAIN_CONF = (HERE / "chain-888.conf").read_text(encoding="utf-8")

HOST = "WRFCOIN_P2P_HOST=0.0.0.0"
EXT = "WRFCOIN_P2P_EXTERNAL_ADDR=/ip4/203.0.113.7/tcp/9000"
SERVE = "WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS=1"
SID = "WRFCOIN_SERVICE_ID=laptop-1"


def function_body(name: str) -> str:
    start = LAUNCHER.index(f"function {name}")
    end = LAUNCHER.index("\n}\n", start)
    return LAUNCHER[start:end]


def lines(*items: str) -> str:
    return "\r\n".join(items) + "\r\n"


# name -> (reachable.conf text or None for no file, server.key present, expected)
# expected: "sync" / "bonded", or a fragment of the refusal message.
CASES: dict[str, tuple[str | None, bool, str]] = {
    "no file": (None, False, "sync"),
    "no file, server key": (None, True, "sync"),
    "open port only": (lines(HOST, EXT), False, "sync"),
    "bonded": (lines("# bonded laptop", HOST, EXT, SERVE, SID), True, "bonded"),
    "bonded, 128-char id": (lines(HOST, EXT, SERVE, "WRFCOIN_SERVICE_ID=" + "a" * 128), True, "bonded"),
    "bonded, dotted id": (lines(HOST, EXT, SERVE, "WRFCOIN_SERVICE_ID=tony.laptop_2-b"), True, "bonded"),
    "bonded, no server key": (lines(HOST, EXT, SERVE, SID), False, "needs the server key"),
    "bonded, no service id": (lines(HOST, EXT, SERVE), True, "needs WRFCOIN_SERVICE_ID"),
    "service id alone": (lines(HOST, EXT, SID), True, "only used in bonded mode"),
    "bonded, no p2p host": (lines(EXT, SERVE, SID), True, "needs WRFCOIN_P2P_HOST=0.0.0.0"),
    "bonded, loopback p2p host": (lines("WRFCOIN_P2P_HOST=127.0.0.1", EXT, SERVE, SID), True, "needs WRFCOIN_P2P_HOST=0.0.0.0"),
    "bonded, no external addr": (lines(HOST, SERVE, SID), True, "needs WRFCOIN_P2P_EXTERNAL_ADDR"),
    "serve=true": (lines(HOST, EXT, "WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS=true", SID), True, "is not a valid WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS"),
    "serve=0": (lines(HOST, EXT, "WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS=0", SID), True, "is not a valid WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS"),
    "serve empty": (lines(HOST, EXT, "WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS=", SID), True, "is not a valid WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS"),
    "serve twice": (lines(HOST, EXT, SERVE, SERVE, SID), True, "defines WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS twice"),
    "id with a space": (lines(HOST, EXT, SERVE, "WRFCOIN_SERVICE_ID=laptop 1"), True, "is not a valid WRFCOIN_SERVICE_ID"),
    "id starting with a dash": (lines(HOST, EXT, SERVE, "WRFCOIN_SERVICE_ID=-laptop"), True, "is not a valid WRFCOIN_SERVICE_ID"),
    "id of 129 chars": (lines(HOST, EXT, SERVE, "WRFCOIN_SERVICE_ID=" + "a" * 129), True, "is not a valid WRFCOIN_SERVICE_ID"),
    "id non-ascii": (lines(HOST, EXT, SERVE, "WRFCOIN_SERVICE_ID=laptöp"), True, "is not a valid WRFCOIN_SERVICE_ID"),
    "id empty": (lines(HOST, EXT, SERVE, "WRFCOIN_SERVICE_ID="), True, "is not a valid WRFCOIN_SERVICE_ID"),
    "lower-case key": (lines(HOST, EXT, SERVE, "wrfcoin_service_id=laptop-1"), True, "cannot be set there"),
    "sync-only cannot be switched off": (lines(HOST, EXT, SERVE, SID, "WRFCOIN_SYNC_ONLY=0"), True, "WRFCOIN_SYNC_ONLY cannot be set there"),
    "api host cannot be opened": (lines(HOST, EXT, SERVE, SID, "WRFCOIN_API_HOST=0.0.0.0"), True, "WRFCOIN_API_HOST cannot be set there"),
}

HARNESS = r"""
param([string]$Launcher, [string]$Root, [string]$CasesFile, [string]$OutFile)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($Launcher, [ref]$null, [ref]$parseErrors)
if ($parseErrors.Count -gt 0) { throw "wrfcoin-node.ps1 does not parse: $($parseErrors[0])" }
# The launcher's own functions and allow-list, without running its body.
foreach ($fn in $ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $false)) {
    . ([scriptblock]::Create($fn.Extent.Text))
}
foreach ($a in $ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq '$ReachableKeys' }, $false)) {
    . ([scriptblock]::Create($a.Extent.Text))
}
function Fail([string]$Message) { throw [System.Exception]::new("FAIL: $Message") }
$ReachableConfig = Join-Path $Root 'reachable.conf'
$ServerKeyFile = Join-Path $Root 'server.key'
$utf8 = New-Object System.Text.UTF8Encoding($false)
$results = @()
foreach ($case in (Get-Content -LiteralPath $CasesFile -Raw -Encoding UTF8 | ConvertFrom-Json)) {
    Remove-Item -LiteralPath $ReachableConfig, $ServerKeyFile -ErrorAction SilentlyContinue
    if ($null -ne $case.conf) { [System.IO.File]::WriteAllText($ReachableConfig, $case.conf, $utf8) }
    if ($case.key) { [System.IO.File]::WriteAllText($ServerKeyFile, ('ab' * 32)) }
    try {
        $reach = Read-ReachableConfig
        $results += [pscustomobject]@{ name = $case.name; result = (Resolve-NodeMode $reach) }
    } catch {
        $results += [pscustomobject]@{ name = $case.name; result = $_.Exception.Message }
    }
}
# A file, not stdout: the console code page would mangle non-ASCII messages.
[System.IO.File]::WriteAllText($OutFile, (ConvertTo-Json -InputObject $results -Depth 3), $utf8)
"""


def find_powershell() -> tuple[str, bool] | None:
    """(executable, needs Windows paths). Prefers PowerShell 7."""
    for name in ("pwsh", "powershell"):
        exe = shutil.which(name)
        if exe:
            return exe, False
    if sys.platform.startswith("linux") and shutil.which("wslpath"):
        for name in ("pwsh.exe", "powershell.exe"):
            exe = shutil.which(name)
            if exe:
                return exe, True
    return None


class BondedModeDefaults(unittest.TestCase):
    def test_shipped_config_is_sync_only_and_not_bonded(self) -> None:
        keys = dict(line.split("=", 1) for line in CHAIN_CONF.splitlines() if line)
        self.assertEqual(keys["WRFCOIN_SYNC_ONLY"], "1")
        self.assertEqual(keys["WRFCOIN_P2P_HOST"], "127.0.0.1")
        self.assertNotIn("WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS", keys)
        self.assertNotIn("WRFCOIN_SERVICE_ID", keys)

    def test_chain_config_must_still_be_sync_only(self) -> None:
        # Bonded mode keeps WRFCOIN_SYNC_ONLY=1: the node never makes blocks, and
        # the server key (resolved before the sync-only check) still signs.
        self.assertIn("$pairs['WRFCOIN_SYNC_ONLY'] -ne '1'", function_body("Read-NodeConfig"))

    def test_mode_is_resolved_before_any_setting_reaches_the_node(self) -> None:
        body = function_body("Start-Node")
        reach = body.index("$reach = Read-ReachableConfig")
        mode = body.index("$mode = Resolve-NodeMode $reach")
        apply_cfg = body.index("foreach ($key in $cfg.Keys)")
        self.assertLess(reach, mode)
        self.assertLess(mode, apply_cfg)

    def test_values_are_strict(self) -> None:
        body = function_body("Test-ReachableValue")
        self.assertIn("'WRFCOIN_P2P_SERVE_BONDED_SNAPSHOTS' { return ($Value -ceq '1') }", body)
        self.assertIn("'WRFCOIN_SERVICE_ID' { return ($Value -cmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$') }", body)


class BondedModeBehaviour(unittest.TestCase):
    """The launcher's own parser and mode check against real files."""

    results: dict[str, str] = {}

    @classmethod
    def setUpClass(cls) -> None:
        ps = find_powershell()
        if ps is None:
            raise unittest.SkipTest("PowerShell not available (the Windows CI job runs these)")
        exe, windows_paths = ps
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "state").mkdir()
            harness = root / "harness.ps1"
            harness.write_text(HARNESS, encoding="utf-8")
            cases = root / "cases.json"
            cases.write_text(
                json.dumps([{"name": n, "conf": c, "key": k} for n, (c, k, _) in CASES.items()]),
                encoding="utf-8",
            )

            def arg(p: Path) -> str:
                if not windows_paths:
                    return str(p)
                return subprocess.run(["wslpath", "-w", str(p)], check=True, capture_output=True, text=True).stdout.strip()

            out = subprocess.run(
                [exe, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", arg(harness),
                 "-Launcher", arg(LAUNCHER_PATH), "-Root", arg(root / "state"), "-CasesFile", arg(cases), "-OutFile", arg(root / "results.json")],
                capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=180,
            )
            if out.returncode != 0:
                raise AssertionError(f"harness failed ({out.returncode}): {out.stderr or out.stdout}")
            results = json.loads((root / "results.json").read_text(encoding="utf-8-sig"))
            cls.results = {r["name"]: r["result"] for r in results}

    def test_every_case(self) -> None:
        self.assertEqual(set(self.results), set(CASES))
        for name, (_, _, expected) in CASES.items():
            with self.subTest(name):
                got = self.results[name]
                if expected in ("sync", "bonded"):
                    self.assertEqual(got, expected)
                else:
                    self.assertTrue(got.startswith("FAIL: "), got)
                    self.assertIn(expected, got)


if __name__ == "__main__":
    unittest.main()
