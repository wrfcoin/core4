"""Boundary tests for the offline preflight; expected pins come from production."""

import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import check_network_isolation as checker


class TestNetworkIsolationChecker(unittest.TestCase):
    def test_each_identity_and_key_mismatch_is_red_then_canonical_is_green(self):
        pins = checker.canonical()
        for field in pins:
            with self.subTest(field=field):
                changed = copy.deepcopy(pins)
                changed[field] = (
                    ["00" * 32] * 3
                    if isinstance(pins[field], list)
                    else pins[field] + "-probe"
                )
                self.assertTrue(checker.check(changed, pins))
        self.assertEqual(checker.check(pins, pins), [])

    def test_missing_fields_duplicate_keys_and_wrong_types_are_refused(self):
        pins = checker.canonical()
        for field in pins:
            changed = copy.deepcopy(pins)
            del changed[field]
            self.assertTrue(checker.check(changed, pins))
        for keys in (
            None,
            "secret-placeholder",
            [None] * 3,
            pins["producer_public_keys"][:2],
            [pins["producer_public_keys"][0]] * 3,
        ):
            changed = dict(pins, producer_public_keys=keys)
            self.assertTrue(checker.check(changed, pins))
        with self.assertRaises(ValueError):
            checker.unique_json('{"chain_id":"a","chain_id":"b"}')

    def test_cli_exits_and_never_prints_config_values(self):
        pins = checker.canonical()
        sentinel = "PRIVATE_INPUT_MUST_NOT_APPEAR"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.json"
            for value, code in [(dict(pins, chain_id=sentinel), 1), (pins, 0), ({}, 1)]:
                path.write_text(json.dumps(value))
                result = subprocess.run(
                    [
                        sys.executable,
                        str(checker.ROOT / "scripts/check_network_isolation.py"),
                        str(path),
                    ],
                    capture_output=True,
                    text=True,
                )
                self.assertEqual(result.returncode, code)
                self.assertNotIn(sentinel, result.stdout + result.stderr)
            path.write_text(sentinel)
            result = subprocess.run(
                [
                    sys.executable,
                    str(checker.ROOT / "scripts/check_network_isolation.py"),
                    str(path),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 2)
            self.assertNotIn(sentinel, result.stdout + result.stderr)

    def test_source_genesis_pin_disagreement_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in (
                "shared-protocol/src/crypto/chain_binding.rs",
                "p2p/src/config.rs",
                "native-chain/src/bin/launch_api/genesis_producer_pins.rs",
            ):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                text = (checker.ROOT / name).read_text()
                if name.endswith("config.rs"):
                    text = text.replace(checker.canonical()["genesis_hash"], "00" * 32)
                path.write_text(text)
            with self.assertRaisesRegex(ValueError, "genesis pins disagree"):
                checker.canonical(root)


if __name__ == "__main__":
    unittest.main()
