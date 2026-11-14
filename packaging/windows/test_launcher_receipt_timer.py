"""Source checks on wrfcoin-node.ps1's bonded receipt timer (no PowerShell needed).

-Action ReceiptTimer registers a per-user Scheduled Task that runs
-Action Receipt every 30 minutes; -Action Receipt runs one pass of
`wrfcoin.exe node receipt auto` (core4 #173). The behaviour is exercised on a
real Windows runner by windows-node-build.yml; these checks pin what must not
drift: no admin, user-only key, testnet only, secrets by file path only, the
mailbox never gets a credential, exit codes 0 and 2 are normal, and the
uninstaller removes the task.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
LAUNCHER = (HERE / "wrfcoin-node.ps1").read_text(encoding="utf-8")
INSTALLER = (HERE / "wrfcoin-node.iss").read_text(encoding="utf-8")


def function_body(name: str) -> str:
    start = LAUNCHER.index(f"function {name}")
    end = LAUNCHER.index("\n}\n", start)
    return LAUNCHER[start:end]


class ReceiptTimer(unittest.TestCase):
    def test_actions_are_offered_and_dispatched(self) -> None:
        self.assertIn("'ReceiptTimer', 'Receipt', 'StopReceiptTimer')]", LAUNCHER)
        self.assertIn("'ReceiptTimer' { Initialize-UserState; Register-ReceiptTimer }", LAUNCHER)
        self.assertIn("'Receipt' { Initialize-UserState; Invoke-Receipt }", LAUNCHER)
        self.assertIn("'StopReceiptTimer' { Unregister-ReceiptTimer }", LAUNCHER)

    def test_task_is_per_user_limited_every_30_minutes_and_runs_quiet(self) -> None:
        body = function_body("Register-ReceiptTimer")
        self.assertIn("-LogonType Interactive -RunLevel Limited", body)
        self.assertIn("GetCurrent().Name", body)
        self.assertIn("-RepetitionInterval (New-TimeSpan -Minutes $ReceiptIntervalMinutes)", body)
        self.assertIn("-MultipleInstances IgnoreNew", body)
        self.assertIn("-Action Receipt -Quiet", body)
        self.assertIn("-WindowStyle Hidden", body)
        self.assertIn("$ReceiptIntervalMinutes = 30", LAUNCHER)
        # Everything is checked before a task exists.
        self.assertLess(body.index("Assert-ReceiptReady"), body.index("Register-ScheduledTask"))

    def test_ready_check_is_testnet_only_and_locks_the_owner_key(self) -> None:
        body = function_body("Assert-ReceiptReady")
        self.assertIn("$cfg['WRFCOIN_NETWORK_ID'] -ne 'testnet'", body)
        self.assertIn("Read-BondConfig", body)
        self.assertIn("Protect-UserOnly $OwnerKeyFile", body)

    def test_bond_conf_allows_only_three_strict_keys(self) -> None:
        self.assertIn(
            "$BondKeys = @('WRFCOIN_BOND_DEPLOYMENT_ID', 'WRFCOIN_BOND_OWNER_ADDRESS', 'WRFCOIN_CLAIM_MAILBOX_URL')",
            LAUNCHER,
        )
        body = function_body("Read-BondConfig")
        self.assertIn("$BondKeys -cnotcontains $key", body)
        self.assertIn("defines $key twice", body)
        self.assertIn("is missing $key", body)
        self.assertIn("'^[0-9a-f]{64}$'", body)
        self.assertIn("'^0x[0-9a-f]{40}$'", body)
        # https only, no userinfo, query or fragment.
        url = re.search(r"'WRFCOIN_CLAIM_MAILBOX_URL' \{ \$value -cmatch '([^']+)' \}", body).group(1)
        pattern = re.compile(url)
        for good in ["https://wrfcoin.com/api/claim-mailbox/claims", "https://x.example:8443/claims"]:
            self.assertTrue(pattern.match(good), good)
        for bad in ["http://wrfcoin.com/claims", "https://user:pw@wrfcoin.com/claims", "https://wrfcoin.com/claims?x=1",
                    "https://wrfcoin.com/claims#f", "https://wrfcoin.com/a b"]:
            self.assertFalse(pattern.match(bad), bad)

    def test_pass_hands_secrets_by_path_and_treats_0_and_2_as_normal(self) -> None:
        body = function_body("Invoke-Receipt")
        self.assertIn("'node', 'receipt', 'auto'", body)
        self.assertIn("'--key-file', $OwnerKeyFile", body)
        self.assertIn("'--operator-token-file', $TokenFile", body)
        self.assertIn("'--mailbox-url', $bond['WRFCOIN_CLAIM_MAILBOX_URL']", body)
        self.assertIn("'--genesis-hash', $cfg['WRFCOIN_GENESIS_HASH']", body)
        self.assertNotIn("Get-Content", body)  # never reads the key or token itself
        self.assertIn("if ($code -ne 0 -and $code -ne 2) { exit 1 }", body)
        self.assertIn("Write-ReceiptLog", body)

    def test_log_rotates(self) -> None:
        body = function_body("Write-ReceiptLog")
        self.assertIn("$ReceiptLogMaxBytes", body)
        self.assertIn('"$ReceiptLog.1"', body)

    def test_uninstall_removes_the_task(self) -> None:
        self.assertIn('-Action StopReceiptTimer -Quiet"; Flags: runhidden waituntilterminated; RunOnceId: "StopReceiptTimer"', INSTALLER)
        body = function_body("Unregister-ReceiptTimer")
        self.assertIn("Unregister-ScheduledTask -TaskPath $ReceiptTaskPath -TaskName $ReceiptTaskName -Confirm:$false", body)


if __name__ == "__main__":
    unittest.main()
