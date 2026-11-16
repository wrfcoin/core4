"""Source checks on wrfcoin-node.ps1's bonded address-record timer (no PowerShell needed).

-Action AddressTimer registers a per-user Scheduled Task that runs
-Action PublishAddress every 60 minutes; -Action PublishAddress runs one pass of
`wrfcoin.exe node address publish` (core4 #188), which signs the record once per
epoch and re-posts the same bytes on later passes. The behaviour is exercised on
a real Windows runner by windows-node-build.yml; these checks pin what must not
drift: bonded mode and an explicit address.conf are required, no admin, testnet
only, keys and token by file path only, the mailbox never gets a credential,
"mailbox not open yet" is a quiet retry (exit 2, one log line, not repeated),
the log never holds the node's address, and the uninstaller removes the task.
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


class AddressTimer(unittest.TestCase):
    def test_actions_are_offered_and_dispatched(self) -> None:
        self.assertIn("'AddressTimer', 'PublishAddress', 'StopAddressTimer'", LAUNCHER)
        self.assertIn("'AddressTimer' { Initialize-UserState; Register-AddressTimer }", LAUNCHER)
        self.assertIn("'PublishAddress' { Initialize-UserState; Invoke-AddressPublish }", LAUNCHER)
        self.assertIn("'StopAddressTimer' { Unregister-AddressTimer }", LAUNCHER)

    def test_task_is_per_user_limited_hourly_and_runs_quiet(self) -> None:
        body = function_body("Register-AddressTimer")
        self.assertIn("-LogonType Interactive -RunLevel Limited", body)
        self.assertIn("GetCurrent().Name", body)
        self.assertIn("-RepetitionInterval (New-TimeSpan -Minutes $AddressIntervalMinutes)", body)
        self.assertIn("-MultipleInstances IgnoreNew", body)
        self.assertIn("-Action PublishAddress -Quiet", body)
        self.assertIn("-WindowStyle Hidden", body)
        self.assertIn("-TaskPath $AddressTaskPath -TaskName $AddressTaskName", body)
        self.assertIn("$AddressIntervalMinutes = 60", LAUNCHER)
        self.assertIn("$AddressTaskName = 'WRFCoin address'", LAUNCHER)
        # Everything is checked before a task exists.
        self.assertLess(body.index("Assert-AddressReady"), body.index("Register-ScheduledTask"))

    def test_ready_check_needs_testnet_bonded_mode_an_ip4_address_and_locks_the_server_key(self) -> None:
        body = function_body("Assert-AddressReady")
        self.assertIn("$cfg['WRFCOIN_NETWORK_ID'] -ne 'testnet'", body)
        self.assertIn("(Resolve-NodeMode $reach) -ne 'bonded'", body)
        self.assertIn("-cnotmatch '^/ip4/'", body)
        self.assertIn("Read-AddressConfig", body)
        self.assertIn("Protect-UserOnly $ServerKeyFile", body)
        self.assertLess(body.index("Resolve-NodeMode"), body.index("Read-AddressConfig"))

    def test_address_conf_allows_only_two_strict_keys(self) -> None:
        self.assertIn("$AddressKeys = @('WRFCOIN_BOND_DEPLOYMENT_ID', 'WRFCOIN_ADDRESS_MAILBOX_URL')", LAUNCHER)
        body = function_body("Read-AddressConfig")
        self.assertIn("$AddressKeys -cnotcontains $key", body)
        self.assertIn("defines $key twice", body)
        self.assertIn("is missing $key", body)
        self.assertIn("'^[0-9a-f]{64}$'", body)
        # https only, no userinfo, query or fragment.
        url = re.search(r"'WRFCOIN_ADDRESS_MAILBOX_URL' \{ \$value -cmatch '([^']+)' \}", body).group(1)
        pattern = re.compile(url)
        for good in ["https://wrfcoin.com/api/node-addresses", "https://x.example:8443/addresses"]:
            self.assertTrue(pattern.match(good), good)
        for bad in ["http://wrfcoin.com/api/node-addresses", "https://user:pw@wrfcoin.com/a",  # pragma: allowlist secret -- placeholder userinfo the URL check must reject
                    "https://wrfcoin.com/a?x=1", "https://wrfcoin.com/a#f", "https://wrfcoin.com/a b"]:
            self.assertFalse(pattern.match(bad), bad)

    def test_pass_hands_keys_by_path_and_publishes_what_the_node_announces(self) -> None:
        body = function_body("Invoke-AddressPublish")
        self.assertIn("'node', 'address', 'publish'", body)
        self.assertIn("'--server-key-file', $ServerKeyFile", body)
        self.assertIn("'--p2p-key-file', (Join-Path $DataDir 'peer_identity.key')", body)
        self.assertIn("'--operator-token-file', $TokenFile", body)
        self.assertIn("'--mailbox-url', $addr['WRFCOIN_ADDRESS_MAILBOX_URL']", body)
        self.assertIn("'--deployment-id', $addr['WRFCOIN_BOND_DEPLOYMENT_ID']", body)
        self.assertIn("'--genesis-hash', $cfg['WRFCOIN_GENESIS_HASH']", body)
        self.assertIn("'--state-dir', $AddressStateDir", body)
        # The multiaddr is the node's own announced address, checked before use.
        self.assertIn('"$api/network/peer_id"', body)
        self.assertIn("'--multiaddr', $announced", body)
        announced = re.search(r"\$announced -cnotmatch '([^']+)'", body).group(1)
        pattern = re.compile(announced)
        good_id = "12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp"  # pragma: allowlist secret -- a public libp2p peer id, test data
        self.assertTrue(pattern.match(f"/ip4/203.0.113.9/tcp/9000/p2p/{good_id}"))
        for bad in [f"/dns4/x.example/tcp/9000/p2p/{good_id}", "/ip4/203.0.113.9/tcp/9000",
                    f"/ip4/203.0.113.9/tcp/9000/p2p/{good_id};calc"]:
            self.assertFalse(pattern.match(bad), bad)
        self.assertNotIn("ReadAllText", body)  # never reads a key or the token itself

    def test_mailbox_not_open_is_a_quiet_retry(self) -> None:
        body = function_body("Invoke-AddressPublish")
        self.assertIn("'ADDRESSES_DISABLED|RECEIPT_HTTP_404'", body)
        self.assertIn("'mailbox not open yet, retry later'", body)
        self.assertIn("if ($code -ne 0 -and $code -ne 2) { exit 1 }", body)
        # A stopped node is a wait, not a failure: log and return (exit 0).
        stopped = body.index("node not running, retry later")
        self.assertEqual(body[stopped:].split("\n", 2)[1].strip(), "return")

    def test_log_writes_changes_only_rotates_and_hides_addresses(self) -> None:
        body = function_body("Write-AddressLog")
        self.assertIn("-ceq $Line)) { return }", body)
        self.assertIn("$AddressLogMaxBytes", body)
        self.assertIn('"$AddressLog.1"', body)
        self.assertIn("Hide-Addresses", function_body("Invoke-AddressPublish"))
        hide = function_body("Hide-Addresses")
        ip4 = re.search(r"-replace '(\\b[^']+)', '<addr>'", hide).group(1)
        self.assertEqual(re.sub(ip4, "<addr>", "dial 203.0.113.9 failed"), "dial <addr> failed")

    def test_uninstall_removes_the_task(self) -> None:
        self.assertIn('-Action StopAddressTimer -Quiet"; Flags: runhidden waituntilterminated; RunOnceId: "StopAddressTimer"', INSTALLER)
        body = function_body("Unregister-AddressTimer")
        self.assertIn("Unregister-ScheduledTask -TaskPath $AddressTaskPath -TaskName $AddressTaskName -Confirm:$false", body)


if __name__ == "__main__":
    unittest.main()
