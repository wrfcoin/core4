#!/usr/bin/env python3
"""Independent, restoring production-source RED controls for #2848 S1.

Run only in an owned worktree with no concurrent editor. Every cargo command
uses the shared heavy lock and checks >=40 GiB before and inside the lock.
A compiler error, zero tests, unexpected pass or unrestored source is failure.
"""

import argparse
import os
from pathlib import Path
import subprocess
import sys
import signal


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--heavy-lock", type=Path, required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    parser.add_argument("--inside-lock", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if not args.inside_lock:
        available = int(subprocess.check_output(["df", "-B1G", "--output=avail", "/"], text=True).splitlines()[-1])
        if available < 40: raise RuntimeError(f"DISK GUARD STOP: {available} GiB")
        command = ["flock", "--close", str(args.heavy_lock), sys.executable,
                   str(Path(__file__).resolve()), "--inside-lock",
                   "--heavy-lock", str(args.heavy_lock.resolve()),
                   "--evidence-dir", str(args.evidence_dir.resolve())]
        raise SystemExit(subprocess.run(command).returncode)
    root = Path(__file__).resolve().parents[1]
    args.evidence_dir.mkdir(parents=True, exist_ok=True)
    os.environ.update(CARGO_TARGET_DIR=str(root / "target"), CARGO_BUILD_JOBS="8", CARGO_PROFILE_DEV_DEBUG="0",
                       CARGO_PROFILE_TEST_DEBUG="0", CARGO_INCREMENTAL="0")
    for name in ("WRFCOIN_REGEN_NODE_CONTRACT_VECTORS", "WRFCOIN_REGEN_GOLDEN_VECTORS",
                 "REGENERATE_DETERMINISM_FIXTURE"):
        os.environ.pop(name, None)

    def run(label, package, target, test=None, red=False):
        available = int(subprocess.check_output(
            ["df", "-B1G", "--output=avail", "/"], text=True).splitlines()[-1])
        if available < 40:
            raise RuntimeError(f"DISK GUARD STOP: {available} GiB")
        cargo = ["cargo", "test", "--locked", "-p", package, "--test", target]
        if test:
            cargo += [test, "--", "--exact"]
        # The re-executed script is wholly wrapped in flock --close. Recheck
        # disk before each cargo invocation while that parent owns the lock.
        command = cargo
        result = subprocess.run(command, cwd=root,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        (args.evidence_dir / f"{label}.log").write_text(f"LOCKED DISK_GUARD avail={available} GiB\n" + result.stdout)
        if result.returncode == 90:
            raise RuntimeError("DISK GUARD STOP inside lock")
        expected = "test result: FAILED" if red else "test result: ok"
        if expected not in result.stdout or "running 0 tests" in result.stdout:
            raise RuntimeError(f"{label}: missing nonzero executed {expected}; exit={result.returncode}")
        if red != (result.returncode != 0):
            raise RuntimeError(f"{label}: unexpected exit={result.returncode}")
        print(f"{label}: {'REAL-RED' if red else 'LOCAL-GREEN'}", flush=True)

    contracts = "shared-protocol/src/consensus/node_share_contracts.rs"
    gate = "shared-protocol/src/crypto/chain_binding.rs"
    state = "shared-protocol/src/blockchain/state.rs"
    receipt = "shared-protocol/src/consensus/node_service_receipt.rs"
    shared = "wrfcoin-shared-protocol"
    suite = "node_share_contracts"
    mutations = [
        ("receipt_commitment_encoding", contracts,
         ".bytes_field(&receipt.receipt.canonical_body())",
         ".bytes_field(&bincode::serialize(&receipt.receipt).unwrap())",
         shared, suite, "signed_commitments_match_independent_reference_and_bind_every_field"),
        ("manifest_commitment_finality", contracts, ".u64_field(p.finalized_height)",
         ".u64_field(p.height)", shared, suite,
         "signed_commitments_match_independent_reference_and_bind_every_field"),
        ("ticket_reference_order", contracts,
         ".bytes_field(&self.server_id)\n            .bytes_field(&self.consumer_id)",
         ".bytes_field(&self.consumer_id)\n            .bytes_field(&self.server_id)",
         shared, suite, "signed_contract_goldens_execute"),
        ("bond_reference_order", contracts,
         ".u64_field(self.expected_nonce)\n            .u64_field(self.effective_epoch)",
         ".u64_field(self.effective_epoch)\n            .u64_field(self.expected_nonce)",
         shared, suite, "signed_contract_goldens_execute"),
        ("domain", contracts, 'b"wrfcoin.node_service_ticket.v1"',
         'b"wrfcoin.node_service_ticket.RED"', shared, suite, "signed_contract_goldens_execute"),
        ("genesis", contracts, "self.genesis_hash == *genesis", "true", shared, suite,
         "tampered_signed_bytes_genesis_domains_and_aliases_are_rejected"),
        ("alias", contracts, "|| self.ticket.server_id == self.ticket.consumer_id", "",
         shared, suite, "validly_signed_self_identity_and_replaced_receipt_still_fail"),
        ("receipt_bytes", contracts,
         "|| self.ticket.signed_receipt_hash != signed_receipt_hash(&self.receipt)", "",
         shared, suite, "validly_signed_self_identity_and_replaced_receipt_still_fail"),
        ("signature_count", contracts,
         "        verify(\n            &NodeSettlementEndorsement {\n"
         "                ticket_preimage_hash: node_contract_digest(&self.ticket, identity),\n"
         "            },\n            consumer_key,\n            &self.consumer_signature,\n"
         "            identity,\n        )", "        Ok(())", shared, suite,
         "exact_three_service_signatures_and_bounded_wire"),
        ("activation", gate, "Self::StakeSubmissionNonce | Self::NodeShareSettlement => false,",
         "Self::StakeSubmissionNonce => false,\n            Self::NodeShareSettlement => true,",
         shared, suite, "node_gate_is_default_off_but_signed_extensions_are_admissible"),
        ("default_activation", gate,
         "    pub fn active(&self, feature: ConsensusFeature, height: u64) -> bool {",
         "    pub fn active(&self, feature: ConsensusFeature, height: u64) -> bool {\n"
         "        if feature == ConsensusFeature::NodeShareSettlement { return true; }",
         shared, suite, "node_gate_is_default_off_but_signed_extensions_are_admissible"),
        ("wire", contracts, ".with_fixint_encoding()", ".with_varint_encoding()",
         shared, suite, "bounded_batch_and_bond_wire_pins"),
        ("legacy_receipt_wire", receipt, '.bytes_field(&self.nonce)', '',
         shared, "signing_preimage_v2_golden", "golden_vectors_reproduce_byte_for_byte_and_tampered_twins_fail"),
        ("off_root", state, 'material.extend_from_slice(b"wrfcoin_state_root_v14");',
         'material.extend_from_slice(b"wrfcoin_state_root_RED");',
         "test_server", "determinism_differential", "full_sequence_root_parity"),
        ("off_restart_replay", state, "    pub stake_locked: Balance,",
         "    #[serde(skip)]\n    pub stake_locked: Balance,", "test_server",
         "determinism_differential", "restart_midsequence_preserves_roots"),
    ]
    def stop(signum, _frame):
        raise SystemExit(128 + signum)
    signal.signal(signal.SIGTERM, stop)
    originals = {path: (root / path).read_bytes() for _, path, *_ in mutations}
    try:
        run("baseline_contracts", shared, suite)
        run("baseline_replay", "test_server", "determinism_differential")
        for label, path, old, new, package, target, test in mutations:
            source = originals[path].decode()
            if source.count(old) != 1:
                raise RuntimeError(f"{label}: expected exactly one source anchor")
            try:
                (root / path).write_text(source.replace(old, new))
                run(label, package, target, test, red=True)
            finally:
                (root / path).write_bytes(originals[path])
        run("restored_contracts", shared, suite)
        run("restored_inventory", shared, "signed_type_inventory")
        run("restored_legacy_goldens", shared, "signing_preimage_v2_golden")
        run("restored_replay", "test_server", "determinism_differential")
    finally:
        for path, original in originals.items():
            (root / path).write_bytes(original)
        assert all((root / path).read_bytes() == original for path, original in originals.items())
    print("NODE_SHARE_S1_RED_OK mutations=15 restored=byte-identical", flush=True)


if __name__ == "__main__":
    main()
