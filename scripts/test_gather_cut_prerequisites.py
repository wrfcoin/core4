#!/usr/bin/env python3
"""gather-cut-prerequisites.py never publishes an unknown WRFCOIN_CUT_* value,
nor its length or any digest of it — only the variable name (review of
core4#3156: a truncated unsalted hash of a low-entropy secret is brute-forceable)."""

import hashlib
import importlib.util
import json
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).with_name("gather-cut-prerequisites.py")


def load():
    spec = importlib.util.spec_from_file_location("gather_cut_prerequisites", SCRIPT)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


UNLISTED_VALUE = "lowentropy7"
ENV = (
    "WRFCOIN_CUT_CORE4_COMMIT=abc123\n"
    "WRFCOIN_CUT_CHAIN_ID=t1\n"
    f"WRFCOIN_CUT_UNLISTED_SETTING={UNLISTED_VALUE}\n"
    "WRFCOIN_CUT_ANOTHER_UNKNOWN=x\n"
    "UNRELATED=ignored\n"
)


class InfraPinsTest(unittest.TestCase):
    def setUp(self):
        self.mod = load()
        self.mod.show = lambda repo, ref, path: ENV
        self.mod.git = lambda repo, *args: "0123abcd\n"

    def test_unknown_cut_keys_are_reported_by_name_only(self):
        out = self.mod.infra_pins("/infra", "origin/main")
        self.assertEqual(
            out["cut_pins_other_names"],
            ["WRFCOIN_CUT_ANOTHER_UNKNOWN", "WRFCOIN_CUT_UNLISTED_SETTING"],
        )
        self.assertEqual(
            out["cut_pins"],
            {"WRFCOIN_CUT_CORE4_COMMIT": "abc123", "WRFCOIN_CUT_CHAIN_ID": "t1"},
        )

    def test_no_value_length_or_digest_of_an_unknown_key_is_published(self):
        blob = json.dumps(self.mod.infra_pins("/infra", "origin/main"))
        self.assertNotIn(UNLISTED_VALUE, blob)
        digest = hashlib.sha256(UNLISTED_VALUE.encode()).hexdigest()
        for n in (8, 12, 16):
            self.assertNotIn(digest[:n], blob)
        self.assertNotIn("sha256", blob)
        self.assertNotIn('"len"', blob)


FAUCET = "0xf72059c3b4a6d3c431920c3cf3e40dbf68a6b5d3"


class GenesisFaucetBlockingTest(unittest.TestCase):
    """#3361 option B: a faucet chain's cut is a STOP without a faucet pin."""

    def setUp(self):
        self.mod = load()

    def test_faucet_pin_is_public_cut_identity(self):
        self.mod.show = lambda repo, ref, path: (
            "WRFCOIN_CUT_CHAIN_ID=777\n"
            f"WRFCOIN_CUT_GENESIS_FAUCET_ADDRESS={FAUCET}\n")
        self.mod.git = lambda repo, *args: "0123abcd\n"
        out = self.mod.infra_pins("/infra", "origin/main")
        self.assertEqual(out["cut_pins"]["WRFCOIN_CUT_GENESIS_FAUCET_ADDRESS"], FAUCET)
        self.assertEqual(out["cut_pins_other_names"], [])

    def test_faucet_chain_without_valid_pin_blocks(self):
        for value in (None, "", "   ", "0xnot-hex", FAUCET[2:], "0x" + "0" * 40):
            pins = {"WRFCOIN_CUT_CHAIN_ID": "777"}
            if value is not None:
                pins["WRFCOIN_CUT_GENESIS_FAUCET_ADDRESS"] = value
            reason = self.mod.genesis_faucet_blocking(pins)
            self.assertIsNotNone(reason, f"value {value!r} must block")
            self.assertIn("#3361", reason)

    def test_faucet_chain_with_valid_pin_does_not_block(self):
        for value in (FAUCET, FAUCET.upper().replace("0X", "0x"), f'"{FAUCET}"'):
            self.assertIsNone(self.mod.genesis_faucet_blocking(
                {"WRFCOIN_CUT_CHAIN_ID": "777",
                 "WRFCOIN_CUT_GENESIS_FAUCET_ADDRESS": value}))

    def test_zero_bps_chain_never_blocks(self):
        self.assertIsNone(self.mod.genesis_faucet_blocking(
            {"WRFCOIN_CUT_CHAIN_ID": "wrfcoin-main"}))


if __name__ == "__main__":
    unittest.main()
