#!/usr/bin/env python3
"""Tests for gen_chain888_config.py. Run: python3 -m unittest packaging/windows/test_gen_chain888_config.py"""
import re
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gen_chain888_config as gen  # noqa: E402

H = "ab" * 32
PINS = f"""# header comment
WRFCOIN_CUT_CORE4_COMMIT=de9d0d268e1a914109444c1d49b2dcfffe896d80
WRFCOIN_CUT_GENESIS_HASH={H}
WRFCOIN_CUT_CHAIN_ID=888
WRFCOIN_CUT_NETWORK_ID=testnet
WRFCOIN_CUT_BOOTSTRAP="/dns4/seed.example.com/tcp/9000/p2p/12D3KooWMM7ukbQrn4BH2SjSNvyvAW2SaKUkHmDRng9KnifgYeWr"
WRFCOIN_CUT_SNAPSHOT_TRUSTED_SIGNERS={H}
WRFCOIN_CUT_GENESIS_FAUCET_ADDRESS=0xf72059c3b4a6d3c431920c3cf3e40dbf68a6b5d3
WRFCOIN_SIGNING_PROTOCOL_VERSION='1'
WRFCOIN_GENESIS_ACTIVATION_SCHEDULE='0:v1,2:schedule_extension'
WRFCOIN_GENESIS_PRODUCER_SET='{{"version":1,"members":[{{"producer_id":"a","public_key":"{H}"}}]}}'  # gitleaks:allow (public keys)
WRFCOIN_GENESIS_ORACLE_SET='{{"oracle_set":["{H}"],"oracle_quorum":1}}'
WRFCOIN_GENESIS_SCHEDULE_GOVERNANCE_KEYS='{H}'  # trailing comment
WRFCOIN_GENESIS_FAUCET_ALLOCATION_BASE_UNITS='10000000000000'
WRFCOIN_GENESIS_FLAT_REWARD_BASE_UNITS='2222222222'
WRFCOIN_GENESIS_FLAT_REWARD_SECONDS='600'
WRFCOIN_GENESIS_NODE_SHARE_EPOCH_BLOCKS='512'
WRFCOIN_GENESIS_NODE_SHARE_CHALLENGE_BLOCKS='512'
"""
COMPOSE = f"""services:
  postgres:
    environment:
      WRFCOIN_REQUIRE_PRODUCER_AUTH: "0"
  wrfcoin-node:
    image: x
    environment:
      WRFCOIN_REQUIRE_PRODUCER_AUTH: "1"
      # a comment
      WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS: "{H}"
      WRFCOIN_ORACLE_GOVERNANCE_PUBKEY_COUNT: "1"
  wrfcoin-seed:
    environment:
      WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS: "${{X:-}}"
"""


def env(pairs):
    return dict(pairs)


class Generate(unittest.TestCase):
    def test_good_inputs_produce_flat_config(self):
        cfg = env(gen.build(PINS, COMPOSE))
        self.assertEqual(cfg["WRFCOIN_CHAIN_ID"], "888")
        self.assertEqual(cfg["WRFCOIN_GENESIS_HASH"], H)
        # the inline "# gitleaks" comment after quoted JSON is not part of the value
        self.assertTrue(cfg["WRFCOIN_GENESIS_PRODUCER_SET"].endswith("]}"))
        self.assertEqual(cfg["WRFCOIN_GENESIS_SCHEDULE_GOVERNANCE_KEYS"], H)
        self.assertEqual(cfg["WRFCOIN_SIGNING_PROTOCOL_VERSION"], "1")
        self.assertTrue(cfg["WRFCOIN_P2P_BOOTSTRAP"].startswith("/dns4/"))
        self.assertEqual(cfg["WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS"], H)
        self.assertEqual(cfg["WRFCOIN_REQUIRE_PRODUCER_AUTH"], "1")
        self.assertEqual(cfg["WRFCOIN_SYNC_ONLY"], "1")
        self.assertEqual(cfg["WRFCOIN_API_HOST"], "127.0.0.1")
        self.assertNotIn("WRFCOIN_DATA_DIR", cfg)  # set per user by the launcher
        self.assertNotIn("WRFCOIN_OPERATOR_TOKEN", cfg)  # per-install secret, never shipped

    def test_rendered_lines_are_plain_key_value(self):
        for line in gen.render(gen.build(PINS, COMPOSE)).splitlines():
            self.assertRegex(line, r"^[A-Z][A-Z0-9_]*=\S")
            self.assertFalse(line.startswith("#"))
            key, value = line.split("=", 1)
            self.assertFalse(value[:1] in "'\"" and value[-1:] == value[:1], key)

    def fails(self, pins=PINS, compose=COMPOSE, match=None):
        with self.assertRaises(gen.ConfigError) as ctx:
            gen.build(pins, compose)
        if match:
            self.assertIn(match, str(ctx.exception))

    def test_placeholder_bootstrap_refused(self):
        self.fails(re.sub(r'WRFCOIN_CUT_BOOTSTRAP=.*', 'WRFCOIN_CUT_BOOTSTRAP="/ip4/<N5_PUBLIC_IP>/tcp/9000/p2p/12D3KooWx"', PINS),
                   match="placeholder")

    def test_missing_pin_refused(self):
        self.fails(re.sub(r"WRFCOIN_GENESIS_FLAT_REWARD_SECONDS=.*\n", "", PINS), match="FLAT_REWARD_SECONDS")

    def test_empty_faucet_refused(self):
        self.fails(re.sub(r"WRFCOIN_CUT_GENESIS_FAUCET_ADDRESS=.*", "WRFCOIN_CUT_GENESIS_FAUCET_ADDRESS=", PINS))

    def test_wrong_chain_refused(self):
        self.fails(PINS.replace("WRFCOIN_CUT_CHAIN_ID=888", "WRFCOIN_CUT_CHAIN_ID=777"), match="chain 888")

    def test_duplicate_pin_refused(self):
        self.fails(PINS + f"WRFCOIN_CUT_GENESIS_HASH={H}\n", match="more than once")

    def test_text_after_closing_quote_refused(self):
        self.fails(PINS.replace("WRFCOIN_SIGNING_PROTOCOL_VERSION='1'", "WRFCOIN_SIGNING_PROTOCOL_VERSION='1' 2"),
                   match="malformed")

    def test_zero_genesis_refused(self):
        self.fails(PINS.replace(f"WRFCOIN_CUT_GENESIS_HASH={H}", "WRFCOIN_CUT_GENESIS_HASH=" + "0" * 64))

    def test_compose_interpolated_governance_key_refused(self):
        self.fails(compose=COMPOSE.replace(f'PUBKEYS: "{H}"', 'PUBKEYS: "${K:-}"'), match="interpolated")

    def test_compose_count_mismatch_refused(self):
        self.fails(compose=COMPOSE.replace('PUBKEY_COUNT: "1"', 'PUBKEY_COUNT: "2"'), match="COUNT")

    def test_compose_without_node_service_refused(self):
        self.fails(compose=COMPOSE.replace("wrfcoin-node:", "other-node:"), match="no wrfcoin-node")


class Cli(unittest.TestCase):
    def test_write_then_check_then_stale(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "testnet").mkdir()
            (tmp / "testnet/cut-pins.env").write_text(PINS)
            (tmp / "testnet/docker-compose.testnet.yml").write_text(COMPOSE)
            out = tmp / "chain-888.conf"
            self.assertEqual(gen.main(["--infra", str(tmp), "--out", str(out)]), 0)
            self.assertEqual(gen.main(["--infra", str(tmp), "--out", str(out), "--check"]), 0)
            out.write_text(out.read_text() + "EXTRA=1\n")
            self.assertEqual(gen.main(["--infra", str(tmp), "--out", str(out), "--check"]), 1)
            (tmp / "testnet/cut-pins.env").write_text(PINS.replace("WRFCOIN_CUT_CHAIN_ID=888", ""))
            self.assertEqual(gen.main(["--infra", str(tmp), "--out", str(out)]), 1)


class Committed(unittest.TestCase):
    """The file the package ships: shape only (the pins themselves come from infra)."""

    def test_committed_config_shape(self):
        text = (Path(__file__).resolve().parent / "chain-888.conf").read_text(encoding="utf-8")
        self.assertTrue(text.endswith("\n"))
        self.assertNotIn("\r", text)
        keys = []
        for line in text.splitlines():
            self.assertRegex(line, r"^[A-Z][A-Z0-9_]*=[^\s<>].*$")
            self.assertNotRegex(line, r"\s#")
            keys.append(line.split("=", 1)[0])
        self.assertEqual(len(keys), len(set(keys)))
        expected = (["WRFCOIN_CHAIN_ID", "WRFCOIN_NETWORK_ID"] + list(gen.GENESIS_888)
                    + ["WRFCOIN_P2P_BOOTSTRAP", "WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS"]
                    + gen.COMPOSE_LITERALS + [k for k, _ in gen.NATIVE_SYNC_NODE])
        self.assertEqual(keys, expected)
        cfg = dict(line.split("=", 1) for line in text.splitlines())
        self.assertEqual(cfg["WRFCOIN_CHAIN_ID"], "888")
        self.assertEqual(cfg["WRFCOIN_SYNC_ONLY"], "1")


if __name__ == "__main__":
    unittest.main()
