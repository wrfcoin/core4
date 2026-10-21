#!/usr/bin/env python3
"""Tests for testnet_gate_check.py (#2160), beyond what `--self-test` covers.

`--self-test` proves the pass/fail/missing/stale wiring end to end with
fixture `GateInputs`. This file exercises the `attestations.yaml` parser's
edge cases directly, since a parser bug there is a silent-trust bug: a human
gate must never read as `pass` because of a parsing mistake.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import testnet_gate_check as gate  # noqa: E402


class ParseAttestationsYaml(unittest.TestCase):
    def test_empty_file_parses_to_no_entries_no_errors(self) -> None:
        entries, errors = gate.parse_attestations_yaml("# just comments\n\n")
        self.assertEqual(entries, {})
        self.assertEqual(errors, [])

    def test_one_well_formed_entry(self) -> None:
        text = (
            "- gate_id: incident-response-plan-reviewed\n"
            "  attested_by: alice\n"
            "  attested_at: \"2026-08-01\"\n"
            "  evidence_url: \"https://example/doc\"\n"
        )
        entries, errors = gate.parse_attestations_yaml(text)
        self.assertEqual(errors, [])
        self.assertIn("incident-response-plan-reviewed", entries)
        self.assertEqual(entries["incident-response-plan-reviewed"]["attested_by"], "alice")

    def test_missing_required_field_is_an_error_and_the_entry_is_dropped(self) -> None:
        text = (
            "- gate_id: key-ceremony-witnessed\n"
            "  attested_by: alice\n"
            "  attested_at: \"2026-08-01\"\n"
            # evidence_url omitted
        )
        entries, errors = gate.parse_attestations_yaml(text)
        self.assertNotIn("key-ceremony-witnessed", entries)
        self.assertTrue(any("evidence_url" in e for e in errors))

    def test_duplicate_gate_id_is_an_error_not_a_silent_overwrite(self) -> None:
        text = (
            "- gate_id: mobile-launch-criteria-met\n"
            "  attested_by: alice\n"
            "  attested_at: \"2026-08-01\"\n"
            "  evidence_url: \"https://example/a\"\n"
            "- gate_id: mobile-launch-criteria-met\n"
            "  attested_by: bob\n"
            "  attested_at: \"2026-08-02\"\n"
            "  evidence_url: \"https://example/b\"\n"
        )
        entries, errors = gate.parse_attestations_yaml(text)
        self.assertTrue(any("attested more than once" in e for e in errors))
        # The first (alice's) entry stands; the duplicate is reported, not silently applied.
        self.assertEqual(entries["mobile-launch-criteria-met"]["attested_by"], "alice")

    def test_multiple_entries_all_parse(self) -> None:
        text = (
            "- gate_id: a\n"
            "  attested_by: alice\n"
            "  attested_at: \"2026-08-01\"\n"
            "  evidence_url: \"https://example/a\"\n"
            "- gate_id: b\n"
            "  attested_by: bob\n"
            "  attested_at: \"2026-08-02\"\n"
            "  evidence_url: \"https://example/b\"\n"
        )
        entries, errors = gate.parse_attestations_yaml(text)
        self.assertEqual(errors, [])
        self.assertEqual(set(entries), {"a", "b"})


class GateHumanAttestation(unittest.TestCase):
    def test_parse_errors_make_the_gate_missing_even_if_this_gate_id_is_present(self) -> None:
        entries = {"x": {"gate_id": "x", "attested_by": "a", "attested_at": "2026-08-01", "evidence_url": "u"}}
        result = gate.gate_human_attestation("x", entries, ["some unrelated parse error"], 30, 1_800_000_000.0)
        self.assertEqual(result.status, gate.MISSING)

    def test_malformed_date_is_missing_not_a_crash(self) -> None:
        entries = {"x": {"gate_id": "x", "attested_by": "a", "attested_at": "not-a-date", "evidence_url": "u"}}
        result = gate.gate_human_attestation("x", entries, [], 30, 1_800_000_000.0)
        self.assertEqual(result.status, gate.MISSING)


class GateTestnetIdentityDiff(unittest.TestCase):
    def test_malformed_genesis_hash_fails_even_with_a_non_mainnet_identity(self) -> None:
        result = gate.gate_testnet_identity_diff("777", "testnet", "not-hex")
        self.assertEqual(result.status, gate.FAIL)

    def test_case_insensitive_mainnet_match(self) -> None:
        result = gate.gate_testnet_identity_diff("777", "MainNet", "a" * 64)
        self.assertEqual(result.status, gate.FAIL)


if __name__ == "__main__":
    unittest.main()
