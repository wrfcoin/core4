"""Checks on wrfcoin-node.ps1's attester auto-start (the seat-gated Scheduled Task).

-Action AttesterTimer registers a per-user Scheduled Task, \\WRFCoin\\WRFCoin Attester,
that runs -Action AttesterAutoStart every 5 minutes. One pass starts the attester
only when this node's oracle id is in the chain's oracle set and the tip is at
least 3 blocks past the first height the launcher saw it seated; leaving the set
restarts the count and never stops a running attester. Node Start and 'Create Attester Key'
register the task by themselves once oracle.key exists, unless the operator
turned auto-start off (-Action AttesterAutoStartOff, attester-autostart.off).

The source checks need no PowerShell. The behaviour checks run the launcher's
own functions under PowerShell with the node, HTTP and process calls stubbed;
they are skipped where PowerShell is missing (the Windows CI job runs them).
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
INSTALLER = (HERE / "wrfcoin-node.iss").read_text(encoding="utf-8")
WORKFLOW = (HERE.parent.parent / ".github" / "workflows" / "windows-node-build.yml").read_text(encoding="utf-8")

ORACLE_ID = "c4dbd69bc84a4651db0a03ccc142a019c0856126ac3c920e361cba2269034725"
OTHER_ID = "781fd34338fa7f846cd4032256179b6f389404cd519fa6eaa3ed097a5a83dcdb"


def function_body(name: str) -> str:
    start = LAUNCHER.index(f"function {name}")
    end = LAUNCHER.index("\n}\n", start)
    return LAUNCHER[start:end]


class Source(unittest.TestCase):
    def test_actions_are_offered_and_dispatched(self) -> None:
        self.assertIn("'AttesterTimer', 'AttesterAutoStart', 'AttesterAutoStartOff', 'StopAttesterTimer')]", LAUNCHER)
        self.assertIn("'AttesterTimer' { Initialize-UserState; Register-AttesterTimer }", LAUNCHER)
        self.assertIn("'AttesterAutoStart' { Initialize-UserState; Invoke-AttesterAutoStart }", LAUNCHER)
        self.assertIn("'AttesterAutoStartOff' { Initialize-UserState; Disable-AttesterAutoStart }", LAUNCHER)
        self.assertIn("'StopAttesterTimer' { Unregister-AttesterTimer }", LAUNCHER)

    def test_start_and_key_creation_turn_it_on_by_themselves(self) -> None:
        self.assertIn("'Start' { Initialize-UserState; Start-Node; Enable-AttesterAutoStart }", LAUNCHER)
        self.assertIn("'NewOracleKey' { Initialize-UserState; New-OracleKey; Enable-AttesterAutoStart }", LAUNCHER)
        body = function_body("Enable-AttesterAutoStart")
        # The opt-out and a missing key both skip; an existing task is kept.
        self.assertIn("(Test-Path -LiteralPath $AttesterOptOutFile) -or -not (Test-OracleKey)", body)
        self.assertIn("Get-ScheduledTask -TaskPath $AttesterTaskPath -TaskName $AttesterTaskName", body)
        self.assertIn("Register-AttesterTimer $true", body)
        # Never fails the node start: refusals throw into its own catch.
        self.assertIn("$script:FailThrows = $true", body)
        self.assertIn("} catch {", body)

    def test_task_is_per_user_limited_every_five_minutes_and_quiet(self) -> None:
        body = function_body("Register-AttesterTimer")
        self.assertIn("-LogonType Interactive -RunLevel Limited", body)
        self.assertIn("GetCurrent().Name", body)
        self.assertIn("-RepetitionInterval (New-TimeSpan -Minutes $AttesterIntervalMinutes)", body)
        self.assertIn("-MultipleInstances IgnoreNew", body)
        self.assertIn("-Action AttesterAutoStart -Quiet", body)
        self.assertIn("-WindowStyle Hidden", body)
        self.assertIn("-TaskPath $AttesterTaskPath -TaskName $AttesterTaskName", body)
        self.assertIn("$AttesterIntervalMinutes = 5", LAUNCHER)
        self.assertIn("$AttesterTaskName = 'WRFCoin Attester'", LAUNCHER)
        self.assertIn("$AttesterSeatDepthBlocks = 3", LAUNCHER)
        # Testnet and a key are checked before a task exists.
        self.assertLess(body.index("Test-OracleKey"), body.index("Register-ScheduledTask"))
        self.assertLess(body.index("'testnet'"), body.index("Register-ScheduledTask"))
        # Only an explicit "on" clears the opt-out; the automatic path never does.
        self.assertIn("if (-not $Automatic -and (Test-Path -LiteralPath $AttesterOptOutFile))", body)

    def test_opt_out_is_explicit_and_sticky(self) -> None:
        body = function_body("Disable-AttesterAutoStart")
        self.assertIn("WriteAllText($AttesterOptOutFile", body)
        self.assertIn("Unregister-ScheduledTask -TaskPath $AttesterTaskPath -TaskName $AttesterTaskName -Confirm:$false", body)
        self.assertIn("$AttesterOptOutFile = Join-Path $Root 'attester-autostart.off'", LAUNCHER)
        self.assertIn("auto-start is off", function_body("Invoke-AttesterAutoStart"))

    def test_pass_reads_only_public_facts_and_never_logs_the_token(self) -> None:
        body = function_body("Invoke-AttesterAutoStart")
        self.assertIn('"$api/transactions/oracle/config"', body)
        self.assertIn('"$api/network/info"', body)
        self.assertIn("Test-AttesterSeatDepth", body)
        self.assertIn("$depth -lt $AttesterSeatDepthBlocks", body)
        for line in body.splitlines():
            if "Write-AttesterAutoLog" in line:
                self.assertNotIn("$token", line)
        # The id comes from the public file or wrfcoin.exe, never oracle.key's bytes.
        self.assertNotIn("ReadAllText($OracleKeyFile", LAUNCHER)
        self.assertIn("$OraclePublicFile", function_body("Get-AttesterOracleId"))

    def test_fail_throws_only_when_a_pass_asks(self) -> None:
        body = function_body("Fail")
        self.assertIn("if ($script:FailThrows) { throw", body)
        self.assertIn("$script:FailThrows = $false", LAUNCHER)

    def test_installer_offers_on_off_and_removes_the_task(self) -> None:
        self.assertRegex(INSTALLER, r'Name: "\{group\}\\Attester Auto-Start On";[^\n]*-Action AttesterTimer"')
        self.assertRegex(INSTALLER, r'Name: "\{group\}\\Attester Auto-Start Off";[^\n]*-Action AttesterAutoStartOff"')
        self.assertIn('-Action StopAttesterTimer -Quiet"; Flags: runhidden waituntilterminated; RunOnceId: "StopAttesterTimer"', INSTALLER)
        body = function_body("Unregister-AttesterTimer")
        self.assertIn("Unregister-ScheduledTask -TaskPath $AttesterTaskPath -TaskName $AttesterTaskName -Confirm:$false", body)

    def test_windows_smoke_exercises_the_timer(self) -> None:
        self.assertIn("'Attester Auto-Start On.lnk'", WORKFLOW)
        self.assertIn("'Attester Auto-Start Off.lnk'", WORKFLOW)
        self.assertIn("-TaskName 'WRFCoin Attester'", WORKFLOW)
        self.assertIn("-Action AttesterAutoStart -Quiet", WORKFLOW)


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
$Config = Join-Path $Root 'chain-888.conf'
$TokenFile = Join-Path $Root 'operator.token'
$OracleKeyFile = Join-Path $Root 'oracle.key'
$OraclePublicFile = Join-Path $Root 'oracle-public.txt'
$AttesterSeatFile = Join-Path $Root 'attester-seat.json'
$AttesterOptOutFile = Join-Path $Root 'attester-autostart.off'
$AttesterAutoLog = Join-Path $Root 'attester-auto.log'
$AttesterAutoLogMaxBytes = 1MB
$AttesterSeatDepthBlocks = 3
$script:FailThrows = $false
[System.IO.File]::WriteAllText($TokenFile, 'test-operator-token')
[System.IO.File]::WriteAllText($OraclePublicFile, "ORACLE_ID`r`n")

# Stubs for everything outside the gate's own logic.
$script:NodeUp = $false; $script:AttUp = $false; $script:Set = @(); $script:Height = 0
$script:Starts = 0; $script:Stops = 0; $script:Tokens = @()
function Read-NodeConfig([string]$Path) { @{ WRFCOIN_NETWORK_ID = 'testnet'; WRFCOIN_API_HOST = '127.0.0.1'; WRFCOIN_API_PORT = '8556' } }
function Test-OracleKey { $true }
function Get-NodeProcess { if ($script:NodeUp) { @(1) } else { @() } }
function Get-AttesterProcess { if ($script:AttUp) { @(1) } else { @() } }
function Start-Attester { $script:AttUp = $true; $script:Starts++ }
function Stop-Attester([bool]$Report = $true) { $script:AttUp = $false; $script:Stops++ }
function Invoke-RestMethod([string]$Uri, $TimeoutSec, $Headers) {
    if ($Uri -like '*/transactions/oracle/config') {
        $script:Tokens += $Headers['Authorization']
        return [pscustomobject]@{ oracle_set = $script:Set; oracle_quorum = 2 }
    }
    return [pscustomobject]@{ chain_height = $script:Height }
}

$results = @()
function Step([string]$Name) {
    Invoke-AttesterAutoStart
    $last = (Get-Content -LiteralPath $AttesterAutoLog -Tail 1) -replace '^\S+ ', ''
    $script:results += [pscustomobject]@{
        name = $Name; log = $last; starts = $script:Starts; stops = $script:Stops
        seat = (Test-Path -LiteralPath $AttesterSeatFile)
        lines = @(Get-Content -LiteralPath $AttesterAutoLog).Count
    }
}

[System.IO.File]::WriteAllText($AttesterOptOutFile, 'off')
Step 'opted out'
Remove-Item -LiteralPath $AttesterOptOutFile
Step 'node down'
Step 'node down again'
$script:NodeUp = $true; $script:Set = @('OTHER_ID'); $script:Height = 100
Step 'not seated'
$script:Set = @('OTHER_ID', 'ORACLE_ID')
Step 'seated at 100'
$script:Height = 102
Step 'seated, 2 deep'
$script:Height = 103
Step 'seated, 3 deep'
$script:Height = 104
Step 'already running'
$script:Set = @('OTHER_ID')
Step 'removed from the set'
$script:AttUp = $false   # the operator stopped it by hand
$script:Set = @('ORACLE_ID'); $script:Height = 200
Step 'seated again at 200'
[System.IO.File]::WriteAllText($AttesterSeatFile, '{"oracle_id":"ORACLE_ID"}')
$script:Height = 300
Step 'seat file without a height'
[System.IO.File]::WriteAllText($AttesterSeatFile, '{"oracle_id":"OTHER_ID","first_seen_height":1}')
Step 'seat file for another key'
$results += [pscustomobject]@{ name = 'tokens'; log = ($script:Tokens | Select-Object -Unique) -join ','; starts = 0; stops = 0; seat = $false; lines = 0 }
$results += [pscustomobject]@{ name = 'log text'; log = (Get-Content -LiteralPath $AttesterAutoLog -Raw); starts = 0; stops = 0; seat = $false; lines = 0 }
[System.IO.File]::WriteAllText($OutFile, (ConvertTo-Json -InputObject $results -Depth 3), (New-Object System.Text.UTF8Encoding($false)))
""".replace("OTHER_ID", OTHER_ID).replace("ORACLE_ID", ORACLE_ID)


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


class Behaviour(unittest.TestCase):
    """The launcher's own seat gate against stubbed node answers."""

    steps: dict[str, dict] = {}

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
            cls.steps = {r["name"]: r for r in results}

    def test_opt_out_and_a_stopped_node_only_wait(self) -> None:
        self.assertEqual(self.steps["opted out"]["log"], "auto-start is off (attester-autostart.off)")
        self.assertEqual(self.steps["node down"]["log"], "waiting (node not running)")
        # A repeated outcome is not written again.
        self.assertEqual(self.steps["node down again"]["lines"], self.steps["node down"]["lines"])
        self.assertEqual(self.steps["node down again"]["starts"], 0)

    def test_not_seated_waits_and_keeps_no_seat_file(self) -> None:
        step = self.steps["not seated"]
        self.assertEqual(step["log"], "waiting for your seat (oracle c4dbd69b is not in the oracle set)")
        self.assertFalse(step["seat"])
        self.assertEqual(step["starts"], 0)

    def test_starts_only_once_the_seat_is_three_blocks_deep(self) -> None:
        for name in ("seated at 100", "seated, 2 deep"):
            with self.subTest(name):
                self.assertEqual(self.steps[name]["starts"], 0)
                self.assertTrue(self.steps[name]["seat"])
                self.assertIn("waiting until the seat is 3 blocks deep", self.steps[name]["log"])
        self.assertEqual(self.steps["seated, 3 deep"]["starts"], 1)
        self.assertEqual(self.steps["seated, 3 deep"]["log"], "seated (oracle c4dbd69b): attester started")
        self.assertEqual(self.steps["already running"]["starts"], 1)
        self.assertEqual(self.steps["already running"]["log"], "seated (oracle c4dbd69b): attester running")

    def test_leaving_the_set_restarts_the_count_and_never_stops_the_attester(self) -> None:
        step = self.steps["removed from the set"]
        self.assertEqual(step["stops"], 0)
        self.assertFalse(step["seat"])
        self.assertEqual(step["log"], "waiting for your seat (oracle c4dbd69b is not in the oracle set)")
        again = self.steps["seated again at 200"]
        self.assertEqual(again["starts"], 1)
        self.assertIn("waiting until the seat is 3 blocks deep", again["log"])

    def test_a_bad_or_foreign_seat_file_never_skips_the_wait(self) -> None:
        for name in ("seat file without a height", "seat file for another key"):
            with self.subTest(name):
                self.assertEqual(self.steps[name]["starts"], 1)
                self.assertIn("waiting until the seat is 3 blocks deep", self.steps[name]["log"])

    def test_token_goes_only_to_the_node_and_never_to_the_log(self) -> None:
        self.assertEqual(self.steps["tokens"]["log"], "Bearer test-operator-token")
        self.assertNotIn("test-operator-token", self.steps["log text"]["log"])


if __name__ == "__main__":
    unittest.main()
