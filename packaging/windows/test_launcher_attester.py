"""Source checks on wrfcoin-node.ps1's oracle attester actions (no PowerShell needed).

-Action NewOracleKey creates %LOCALAPPDATA%\\WRFCoin\\oracle.key, the attester's
testnet software key, and oracle.token, the node's Oracle-role API token.
-Action StartAttester / StopAttester run oracle_attester.exe against the local
node with attester-888.conf. The behaviour that matters is checked on a real
Windows runner by windows-node-build.yml; these checks pin the properties that
must not drift: testnet only, never overwritten, locked before the seed is
written, never printed, the token only reaches the node when the key exists,
and the shipped attester config agrees with chain-888.conf.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
LAUNCHER = (HERE / "wrfcoin-node.ps1").read_text(encoding="utf-8")
ISS = (HERE / "wrfcoin-node.iss").read_text(encoding="utf-8")
WORKFLOW = (HERE.parent.parent / ".github/workflows/windows-node-build.yml").read_text(encoding="utf-8")


def function_body(name: str) -> str:
    start = LAUNCHER.index(f"function {name}")
    end = LAUNCHER.index("\n}\n", start)
    return LAUNCHER[start:end]


def read_conf(name: str) -> dict[str, str]:
    pairs: dict[str, str] = {}
    for line in (HERE / name).read_text(encoding="utf-8").splitlines():
        if not line:
            continue
        key, value = line.split("=", 1)
        assert key not in pairs, f"{name} defines {key} twice"
        pairs[key] = value
    return pairs


HEX64 = re.compile(r"^[0-9a-f]{64}$")


class OracleKey(unittest.TestCase):
    def test_actions_are_offered_and_dispatched(self) -> None:
        self.assertIn("'NewOracleKey', 'StartAttester', 'StopAttester')]", LAUNCHER)
        self.assertIn("'NewOracleKey' { Initialize-UserState; New-OracleKey }", LAUNCHER)
        self.assertIn("'StartAttester' { Initialize-UserState; Start-Attester }", LAUNCHER)
        self.assertIn("'StopAttester' { Stop-Attester }", LAUNCHER)

    def test_testnet_only_and_never_overwritten(self) -> None:
        body = function_body("New-OracleKey")
        self.assertIn("$cfg['WRFCOIN_NETWORK_ID'] -ne 'testnet'", body)
        exists = body.index("if (Test-OracleKey)")
        write = body.index("WriteAllText($OracleKeyTmp, (New-RandomHex))")
        self.assertLess(exists, write)
        self.assertIn("return", body[exists:write])

    def test_seed_goes_to_a_locked_temp_file_then_is_renamed(self) -> None:
        body = function_body("New-OracleKey")
        empty = body.index("WriteAllText($OracleKeyTmp, '')")
        lock = body.index("Protect-UserOnly $OracleKeyTmp", empty)
        seed = body.index("WriteAllText($OracleKeyTmp, (New-RandomHex))")
        move = body.index("[System.IO.File]::Move($OracleKeyTmp, $OracleKeyFile)")
        self.assertLess(empty, lock)
        self.assertLess(lock, seed)
        self.assertLess(seed, move)
        self.assertNotIn("WriteAllText($OracleKeyFile", body)

    def test_zero_length_key_counts_as_absent(self) -> None:
        body = function_body("Test-OracleKey")
        self.assertIn("$item.Length -gt 0", body)
        self.assertNotIn("Test-Path -LiteralPath $OracleKeyFile", LAUNCHER)
        # The only removal of oracle.key is in the create path, after the
        # "exists" check returned (a real key is never deleted).
        create = function_body("New-OracleKey")
        exists = create.index("if (Test-OracleKey)")
        remove = create.index("Remove-Item -LiteralPath $stale -Force")
        self.assertLess(exists, remove)
        self.assertEqual(LAUNCHER.count("Remove-Item -LiteralPath $stale"), 1)

    def test_seed_is_never_read_back_or_shown(self) -> None:
        # The launcher never reads oracle.key itself: only wrfcoin.exe (public
        # half) and the attester open it. Messages carry the public key only.
        self.assertNotIn("ReadAllText($OracleKeyFile", LAUNCHER)
        self.assertNotIn("Get-Content -LiteralPath $OracleKeyFile", LAUNCHER)
        body = function_body("Get-OraclePublicKey")
        self.assertIn("oracle public-key --key-file $KeyFile", body)
        self.assertIn(".data.publicKey", body)
        self.assertIn("-notmatch '^[0-9a-f]{64}$'", body)
        for line in function_body("New-OracleKey").splitlines():
            if "Show-Message" in line:
                self.assertNotIn("New-RandomHex", line)
                self.assertIn("$public", line)

    def test_public_key_file_holds_only_the_public_half(self) -> None:
        body = function_body("Write-OraclePublic")
        self.assertIn("WriteAllText($OraclePublicFile, \"$Public`r`n\")", body)
        self.assertIn("wrfcoin-oracle-public.txt", body)


class OracleToken(unittest.TestCase):
    def test_token_created_locked_and_only_with_the_key(self) -> None:
        body = function_body("Initialize-UserState")
        guard = body.index("if (Test-OracleKey) {")
        rest = body[guard:]
        empty = rest.index("WriteAllText($OracleTokenFile, '')")
        lock = rest.index("Protect-UserOnly $OracleTokenFile", empty)
        token = rest.index("WriteAllText($OracleTokenFile, (New-RandomHex))")
        self.assertLess(empty, lock)
        self.assertLess(lock, token)
        self.assertIn("Protect-UserOnly $OracleKeyFile", rest)

    def test_node_gets_the_token_only_when_the_key_exists(self) -> None:
        body = function_body("Start-Node")
        guard = body.index("if ((Test-OracleKey) -and (Test-Path -LiteralPath $OracleTokenFile)) {")
        self.assertIn("SetEnvironmentVariable('WRFCOIN_ORACLE_TOKEN'", body[guard:])
        self.assertEqual(body.count("WRFCOIN_ORACLE_TOKEN"), 1)

    def test_token_is_never_shown(self) -> None:
        for name in ("New-OracleKey", "Start-Attester"):
            for line in function_body(name).splitlines():
                if "Show-Message" in line or "Add-Content" in line:
                    self.assertNotIn("$token", line, f"{name}: {line.strip()}")


class StartStopAttester(unittest.TestCase):
    def test_start_requires_key_running_node_and_filled_config(self) -> None:
        body = function_body("Start-Attester")
        key = body.index("if (-not (Test-OracleKey))")
        conf = body.index("$att = Read-AttesterConfig")
        node = body.index("if (@(Get-NodeProcess).Count -eq 0)")
        launch = body.index("[System.Diagnostics.Process]::Start($psi)")
        self.assertLess(key, launch)
        self.assertLess(conf, launch)
        self.assertLess(node, launch)
        self.assertIn("'the software oracle key is for testnet only'", body)

    def test_attester_environment(self) -> None:
        body = function_body("Start-Attester")
        for var, value in (
            ("WRFCOIN_ATTESTER_NODE_URL", "$api"),
            ("WRFCOIN_ATTESTER_API_TOKEN", "$token"),
            ("WRFCOIN_TESTNET_ORACLE_KEY_FILE", "$OracleKeyFile"),
        ):
            self.assertIn(f"SetEnvironmentVariable('{var}', {value}, 'Process')", body)
        # The node's own secrets never reach the attester.
        for var in ("WRFCOIN_OPERATOR_TOKEN", "WRFCOIN_SNAPSHOT_SERVER_KEY_FILE", "WRFCOIN_ORACLE_TOKEN"):
            self.assertIn(f"'{var}'", body)
        self.assertIn("SetEnvironmentVariable($key, $null, 'Process')", body)
        # This node's server key is refused as the oracle key.
        self.assertIn("$(Get-ServerPublicKey)", body)
        self.assertIn("'WRFCOIN_ATTESTER_FORBID_SNAPSHOT_PUBKEY', $snap", body)
        self.assertIn(">> `\"$AttesterLog`\" 2>&1", body)
        self.assertIn("$psi.CreateNoWindow = $true", body)

    def test_operator_settings_layer_and_refusals(self) -> None:
        body = function_body("Read-AttesterConfig")
        self.assertIn("'^WRFCOIN_ATTESTER_[A-Z0-9_]+$'", body)
        # The operator's file is read after the shipped one and overrides it.
        self.assertLess(body.index("ReadAllLines($AttesterConfig)"), body.index("ReadAllLines($AttesterUserConfig)"))
        self.assertIn("$AttesterUserKeys -cnotcontains $key", body)
        # The reference mode must be chosen; the message names the file to edit.
        self.assertIn('Fail "set WRFCOIN_ATTESTER_REFERENCE_MODE (fixed, shadow or per_station) in ${AttesterUserConfig}"', body)
        self.assertIn("needs WRFCOIN_ATTESTER_MAX_REFERENCE_KM", body)
        self.assertIn("$AttesterUserConfig = Join-Path $Root 'attester.conf'", LAUNCHER)
        self.assertNotIn("REPLACE_ME", LAUNCHER)

    def test_operator_cannot_override_refused_keys_or_credentials(self) -> None:
        start = LAUNCHER.index("$AttesterUserKeys = @(")
        allowed = LAUNCHER[start : LAUNCHER.index("\n\n", start)]
        for forbidden in ("FORBID", "NODE_URL", "API_TOKEN", "ONCE"):
            self.assertNotIn(forbidden, allowed)

    def test_stop_uses_ctrl_c_and_node_stop_stops_the_attester_first(self) -> None:
        body = function_body("Stop-Attester")
        self.assertIn("[WrfcoinConsoleCtrl]::SendCtrlC([uint32]$proc.Id)", body)
        node = function_body("Stop-Node")
        self.assertLess(node.index("Stop-Attester $false"), node.index("SendCtrlC"))

    def test_node_stop_path_never_blocks_on_the_attester(self) -> None:
        body = function_body("Stop-Attester")
        timeout = body.index("$left = @(Get-AttesterProcess)")
        rest = body[timeout:]
        self.assertIn('if ($Report) { Fail "the attester is still running after 30 s', rest)
        self.assertIn("force-stopping it", rest)
        self.assertIn("$left | Stop-Process -Force", rest)
        # A failed Ctrl+C on the node-stop path force-stops right away instead of waiting.
        failed = body.index("if ($err -ne 0) {")
        wait = body.index("$deadline = (Get-Date).AddSeconds(30)")
        self.assertLess(body.index("$proc | Stop-Process -Force", failed), wait)
        self.assertIn("Ctrl+C to the attester failed", body)

    def test_temp_key_file_is_removed_on_failure(self) -> None:
        body = function_body("New-OracleKey")
        try_ = body.index("try {")
        fin = body.index("} finally {", try_)
        self.assertLess(try_, body.index("WriteAllText($OracleKeyTmp, (New-RandomHex))"))
        self.assertLess(body.index("[System.IO.File]::Move($OracleKeyTmp, $OracleKeyFile)"), fin)
        self.assertIn("Remove-Item -LiteralPath $OracleKeyTmp -Force", body[fin:])


class AttesterConfig(unittest.TestCase):
    def setUp(self) -> None:
        self.att = read_conf("attester-888.conf")
        self.chain = read_conf("chain-888.conf")

    def test_only_attester_keys(self) -> None:
        for key in self.att:
            self.assertRegex(key, r"^WRFCOIN_ATTESTER_[A-Z0-9_]+$")

    def test_operator_choices_are_not_shipped(self) -> None:
        self.assertNotIn("WRFCOIN_ATTESTER_REFERENCE_MODE", self.att)
        self.assertNotIn("WRFCOIN_ATTESTER_MAX_REFERENCE_KM", self.att)
        for value in self.att.values():
            self.assertNotIn("REPLACE_ME", value)
        self.assertEqual(self.att["WRFCOIN_ATTESTER_NWS_STATION"], "KDLH")

    def test_refused_keys_match_chain_888(self) -> None:
        self.assertEqual(
            self.att["WRFCOIN_ATTESTER_FORBID_GOVERNANCE_PUBKEY"],
            self.chain["WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS"],
        )
        self.assertEqual(
            self.att["WRFCOIN_ATTESTER_FORBID_SNAPSHOT_PUBKEY"],
            self.chain["WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS"],
        )
        producers = [
            m["public_key"]
            for m in __import__("json").loads(self.chain["WRFCOIN_GENESIS_PRODUCER_SET"])["members"]
        ]
        self.assertEqual(self.att["WRFCOIN_ATTESTER_FORBID_PRODUCER_PUBKEY"].split(","), producers)
        for key in ("GOVERNANCE", "SNAPSHOT", "PRODUCER"):
            for value in self.att[f"WRFCOIN_ATTESTER_FORBID_{key}_PUBKEY"].split(","):
                self.assertRegex(value, HEX64)

    def test_no_oracle_id_is_refused(self) -> None:
        oracles = set(__import__("json").loads(self.chain["WRFCOIN_GENESIS_ORACLE_SET"])["oracle_set"])
        refused = set()
        for key in ("GOVERNANCE", "SNAPSHOT", "PRODUCER"):
            refused.update(self.att[f"WRFCOIN_ATTESTER_FORBID_{key}_PUBKEY"].split(","))
        self.assertFalse(oracles & refused)

    def test_no_secrets_or_identity_in_the_shipped_config(self) -> None:
        for key in self.att:
            self.assertNotIn("TOKEN", key)
            self.assertNotIn("KEY_FILE", key)
            self.assertNotIn("NODE_URL", key)


class Installer(unittest.TestCase):
    def test_ships_config_and_start_menu_entries(self) -> None:
        self.assertIn('Source: "attester-888.conf"; DestDir: "{app}"', ISS)
        for name, action in (
            ("Create Attester Key", "NewOracleKey"),
            ("Start WRFCoin Attester", "StartAttester"),
            ("Stop WRFCoin Attester", "StopAttester"),
        ):
            self.assertRegex(ISS, rf'Name: "{{group}}\\{name}";[^\n]*-Action {action}"')

    def test_workflow_checks_the_entries(self) -> None:
        for name in ("Create Attester Key.lnk", "Start WRFCoin Attester.lnk", "Stop WRFCoin Attester.lnk"):
            self.assertIn(f"'{name}'", WORKFLOW)
        self.assertIn("'attester-888.conf'", WORKFLOW)


if __name__ == "__main__":
    unittest.main()
