"""Checks on wrfcoin-node.ps1's local port setting (two nodes on one PC).

%LOCALAPPDATA%\\WRFCoin\\reachable.conf may move the node's API port
(WRFCOIN_API_PORT, default 8556) and P2P port (WRFCOIN_P2P_PORT, default 9000),
so a second node on the same PC (for example one in WSL or Docker) keeps its
own. Every launcher action that talks to the local node must follow the chosen
API port, and the Start-menu status entry must not hard-code 8556.

The source checks need no PowerShell. The behaviour checks run the launcher's
own Read-LocalNodeConfig under PowerShell against the shipped chain-888.conf;
they are skipped where PowerShell is missing (the Windows CI job runs them).
"""

from __future__ import annotations

import json
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

from test_launcher_attester_autostart import find_powershell

HERE = Path(__file__).resolve().parent
LAUNCHER_PATH = HERE / "wrfcoin-node.ps1"
LAUNCHER = LAUNCHER_PATH.read_text(encoding="utf-8")
INSTALLER = (HERE / "wrfcoin-node.iss").read_text(encoding="utf-8")
WORKFLOW = (HERE.parent.parent / ".github" / "workflows" / "windows-node-build.yml").read_text(encoding="utf-8")
DOC = (HERE.parent.parent / "docs" / "operator" / "windows-node.md").read_text(encoding="utf-8")


def function_body(name: str) -> str:
    start = LAUNCHER.index(f"function {name}")
    end = LAUNCHER.index("\n}\n", start)
    return LAUNCHER[start:end]


class Source(unittest.TestCase):
    def test_port_keys_are_allowed_and_validated(self) -> None:
        self.assertIn("$PortKeys = @('WRFCOIN_API_PORT', 'WRFCOIN_P2P_PORT')", LAUNCHER)
        self.assertIn("{ $PortKeys -ccontains $_ } { return (Test-PortValue $Value) }", function_body("Test-ReachableValue"))
        body = function_body("Test-PortValue")
        self.assertIn("'^[1-9][0-9]{3,4}$'", body)  # plain decimal, no sign or leading zero
        self.assertIn("$port -ge 1024 -and $port -le 65535", body)
        self.assertIn("the API and P2P ports must differ", function_body("Read-LocalNodeConfig"))

    def test_no_action_builds_the_api_url_from_the_bare_chain_config(self) -> None:
        # The only place the URL is put together is Get-NodeApiUrl; every
        # caller passes it a config from Read-LocalNodeConfig.
        self.assertEqual(LAUNCHER.count('"http://$('), 1)
        self.assertIn('"http://$($Cfg[\'WRFCOIN_API_HOST\']):$($Cfg[\'WRFCOIN_API_PORT\'])"', function_body("Get-NodeApiUrl"))
        for fn in ("Start-Attester", "Assert-ReceiptReady", "Assert-AddressReady", "Invoke-AttesterAutoStart", "Show-StatusPage"):
            with self.subTest(fn):
                self.assertIn("Read-LocalNodeConfig", function_body(fn))
                self.assertNotIn("Read-NodeConfig $Config", function_body(fn))
        for fn in ("Start-Attester", "Invoke-Receipt", "Invoke-AddressPublish", "Invoke-AttesterAutoStart"):
            with self.subTest(fn):
                self.assertIn("$api = Get-NodeApiUrl $cfg", function_body(fn))
        self.assertIn("$api = Get-NodeApiUrl (Read-LocalNodeConfig $reach)", function_body("Start-Node"))

    def test_start_refuses_a_port_another_program_already_answers_on(self) -> None:
        body = function_body("Start-Node")
        running = body.index("if (@(Get-NodeProcess).Count -gt 0)")
        busy = body.index('if (Test-Url "$api/health") {')
        spawn = body.index("[System.Diagnostics.Process]::Start($psi)")
        self.assertLess(running, busy)
        self.assertLess(busy, spawn)
        self.assertIn("WRFCOIN_API_PORT=8566 and WRFCOIN_P2P_PORT=9010", body)

    def test_bonded_mode_message_names_the_chosen_p2p_port(self) -> None:
        body = function_body("Resolve-NodeMode")
        self.assertIn("/tcp/$p2pPort (see Open your port)", body)
        self.assertNotIn("/tcp/9000", body)

    def test_status_entry_asks_the_launcher_instead_of_a_fixed_url(self) -> None:
        self.assertNotIn("8556", INSTALLER.replace("; Earlier versions wrote a fixed http://127.0.0.1:8556 shortcut", ""))
        self.assertRegex(INSTALLER, r'Name: "\{group\}\\WRFCoin Node Status"; Filename: "\{#Launcher\}"; Parameters: "\{#LauncherArgs\} -Action Status"')
        # An upgrade removes the old fixed-port shortcut file.
        self.assertIn('[InstallDelete]\n', INSTALLER)
        self.assertIn("'Status' { Show-StatusPage }", LAUNCHER)
        self.assertIn("'Start', 'Stop', 'Status',", LAUNCHER)
        self.assertIn("Open-StatusPage $api", function_body("Show-StatusPage"))

    def test_windows_smoke_and_doc_cover_two_nodes_on_one_pc(self) -> None:
        self.assertIn("WRFCOIN_API_PORT=8566`r`nWRFCOIN_P2P_PORT=9010", WORKFLOW)
        self.assertIn("http://127.0.0.1:8566/network/info", WORKFLOW)
        self.assertIn("-LocalPort 9010 -State Listen", WORKFLOW)
        self.assertIn("another program already answers on http://127.0.0.1:8556", WORKFLOW)
        self.assertIn("## Two nodes on one PC", DOC)
        self.assertIn("WRFCOIN_API_PORT=8566", DOC)
        self.assertIn("WRFCOIN_P2P_PORT=9010", DOC)


HARNESS = r"""
param([string]$Launcher, [string]$Root, [string]$OutFile)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($Launcher, [ref]$null, [ref]$parseErrors)
if ($parseErrors.Count -gt 0) { throw "wrfcoin-node.ps1 does not parse: $($parseErrors[0])" }
foreach ($fn in $ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $false)) {
    . ([scriptblock]::Create($fn.Extent.Text))
}
# The launcher's own allow-lists, taken from its source.
foreach ($name in 'ReachableKeys', 'PortKeys') {
    $assign = $ast.Find({ param($n) $n -is [System.Management.Automation.Language.AssignmentStatementAst] -and "$($n.Left)" -eq "`$$name" }, $false)
    Set-Variable -Name $name -Value (Invoke-Expression "$($assign.Right)")
}
$Config = Join-Path (Split-Path -Parent $Launcher) 'chain-888.conf'
$ReachableConfig = Join-Path $Root 'reachable.conf'
$LogDir = Join-Path $Root 'no-logs'
$script:FailThrows = $true

$cases = [ordered]@{
    'none'          = $null
    'both'          = "# two nodes on one PC`r`nWRFCOIN_API_PORT=8566`r`nWRFCOIN_P2P_PORT=9010`r`n"
    'api only'      = "WRFCOIN_API_PORT=8566`r`n"
    'p2p only'      = "  WRFCOIN_P2P_PORT = 9010  `r`n"
    'with bonded'   = "WRFCOIN_P2P_HOST=0.0.0.0`r`nWRFCOIN_P2P_PORT=9010`r`nWRFCOIN_API_PORT=8566`r`n"
    'same'          = "WRFCOIN_API_PORT=8566`r`nWRFCOIN_P2P_PORT=8566`r`n"
    'api on 9000'   = "WRFCOIN_API_PORT=9000`r`n"
    'p2p on 8556'   = "WRFCOIN_P2P_PORT=8556`r`n"
    'low'           = "WRFCOIN_API_PORT=80`r`n"
    'high'          = "WRFCOIN_API_PORT=65536`r`n"
    'leading zero'  = "WRFCOIN_API_PORT=08566`r`n"
    'sign'          = "WRFCOIN_P2P_PORT=+9010`r`n"
    'not a number'  = "WRFCOIN_P2P_PORT=9010a`r`n"
    'empty'         = "WRFCOIN_API_PORT=`r`n"
    'twice'         = "WRFCOIN_API_PORT=8566`r`nWRFCOIN_API_PORT=8567`r`n"
    'lower case'    = "wrfcoin_api_port=8566`r`n"
    'api host'      = "WRFCOIN_API_HOST=0.0.0.0`r`n"
    'max'           = "WRFCOIN_API_PORT=65535`r`nWRFCOIN_P2P_PORT=1024`r`n"
}
$results = @()
foreach ($name in $cases.Keys) {
    if (Test-Path -LiteralPath $ReachableConfig) { Remove-Item -LiteralPath $ReachableConfig }
    if ($null -ne $cases[$name]) { [System.IO.File]::WriteAllText($ReachableConfig, $cases[$name]) }
    try {
        $cfg = Read-LocalNodeConfig
        $results += [pscustomobject]@{ name = $name; ok = $true; api = (Get-NodeApiUrl $cfg); p2p = [string]$cfg['WRFCOIN_P2P_PORT']; error = '' }
    } catch {
        $results += [pscustomobject]@{ name = $name; ok = $false; api = ''; p2p = ''; error = $_.Exception.Message }
    }
}
[System.IO.File]::WriteAllText($OutFile, (ConvertTo-Json -InputObject $results -Depth 3), (New-Object System.Text.UTF8Encoding($false)))
"""


class Behaviour(unittest.TestCase):
    """The launcher's own Read-LocalNodeConfig on the shipped chain-888.conf."""

    cases: dict[str, dict] = {}

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

            def arg(p: Path) -> str:
                if not windows_paths:
                    return str(p)
                return subprocess.run(["wslpath", "-w", str(p)], check=True, capture_output=True, text=True).stdout.strip()

            out = subprocess.run(
                [exe, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", arg(harness),
                 "-Launcher", arg(LAUNCHER_PATH), "-Root", arg(root / "state"), "-OutFile", arg(root / "results.json")],
                capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=180,
            )
            if out.returncode != 0:
                raise AssertionError(f"harness failed ({out.returncode}): {out.stderr or out.stdout}")
            results = json.loads((root / "results.json").read_text(encoding="utf-8-sig"))
            cls.cases = {r["name"]: r for r in results}

    def test_defaults_are_unchanged_when_nothing_is_set(self) -> None:
        self.assertEqual(self.cases["none"]["api"], "http://127.0.0.1:8556")
        self.assertEqual(self.cases["none"]["p2p"], "9000")

    def test_chosen_ports_are_used(self) -> None:
        expected = {
            "both": ("http://127.0.0.1:8566", "9010"),
            "api only": ("http://127.0.0.1:8566", "9000"),
            "p2p only": ("http://127.0.0.1:8556", "9010"),
            "with bonded": ("http://127.0.0.1:8566", "9010"),
            "max": ("http://127.0.0.1:65535", "1024"),
        }
        for name, (api, p2p) in expected.items():
            with self.subTest(name):
                case = self.cases[name]
                self.assertTrue(case["ok"], case["error"])
                self.assertEqual((case["api"], case["p2p"]), (api, p2p))

    def test_clashing_ports_stop_the_start(self) -> None:
        for name in ("same", "api on 9000", "p2p on 8556"):
            with self.subTest(name):
                self.assertFalse(self.cases[name]["ok"])
                self.assertIn("the API and P2P ports must differ", self.cases[name]["error"])

    def test_malformed_ports_stop_the_start(self) -> None:
        for name in ("low", "high", "leading zero", "sign", "not a number", "empty"):
            with self.subTest(name):
                self.assertFalse(self.cases[name]["ok"])
                self.assertRegex(self.cases[name]["error"], r"is not a valid WRFCOIN_(API|P2P)_PORT$")
        self.assertIn("defines WRFCOIN_API_PORT twice", self.cases["twice"]["error"])
        for name in ("lower case", "api host"):
            with self.subTest(name):
                self.assertFalse(self.cases[name]["ok"])
                self.assertIn("cannot be set there", self.cases[name]["error"])


if __name__ == "__main__":
    unittest.main()
