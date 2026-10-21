"""#2924: adversarial tests for check_bincode_boundary_attrs.py.

The acceptance bar from the issue, run here directly (not just the script's
own `--self-test` smoke check):

1. Add `skip_serializing_if` to a type reachable across a bincode boundary
   -> the guard goes RED and NAMES that type.
2. Remove it -> the guard goes GREEN.
3. Add it to a type that does NOT cross a bincode boundary -> the guard does
   NOT fire. A guard that flags everything is as useless as one that flags
   nothing; only this experiment (not #1 or #2 alone) distinguishes them.
4. Reconstruct #1053's and #2922's historical shapes as fixtures and confirm
   the guard would have caught each.

Synthetic fixtures throughout (never the live workspace source), so these
tests never depend on this repo's current shape and cannot go stale the way
a test pinned to `fuzz-nightly.yml`'s contents would (#2919's own point,
reused here for the same reason).
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_bincode_boundary_attrs as guard  # noqa: E402


def build(root: Path, files: dict[str, str]) -> Path:
    (root / "scripts").mkdir(exist_ok=True)
    (root / "scripts" / "bincode_boundary_exceptions.json").write_text("{}", encoding="utf-8")
    for rel, content in files.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    return root


class AdversarialBothDirections(unittest.TestCase):
    """The core acceptance experiment: RED naming the type, then GREEN, then
    proven silent on a type that is never on the bincode path."""

    BOUNDARY_TYPE = (
        "#[derive(Serialize, Deserialize)]\n"
        "pub struct Envelope {{\n"
        "    pub id: u64,\n"
        "    pub payload: Payload,\n"
        "}}\n"
        "\n"
        "#[derive(Serialize, Deserialize)]\n"
        "pub struct Payload {{\n"
        "    pub name: String,\n"
        "{attr}"
        "    pub note: Option<String>,\n"
        "}}\n"
        "\n"
        "pub fn send(e: &Envelope) -> Vec<u8> {{\n"
        "    bincode::serialize(e).unwrap()\n"
        "}}\n"
    )

    def test_adding_skip_serializing_if_to_a_boundary_type_is_red_naming_it(self):
        with TemporaryDirectory() as d:
            root = build(
                Path(d),
                {
                    "crate/src/wire.rs": self.BOUNDARY_TYPE.format(
                        attr='    #[serde(default, skip_serializing_if = "Option::is_none")]\n'
                    )
                },
            )
            violations, unresolved, root_names, reached = guard.run(root)
            named = {(v.type_name, v.field_name) for v in violations}
            self.assertIn(
                ("Payload", "note"), named, f"guard must flag Payload.note, reachable from a real bincode root: {named}"
            )

    def test_removing_the_attribute_is_green(self):
        with TemporaryDirectory() as d:
            root = build(Path(d), {"crate/src/wire.rs": self.BOUNDARY_TYPE.format(attr="")})
            violations, unresolved, root_names, reached = guard.run(root)
            self.assertEqual(violations, [], f"no attribute anywhere -- must be clean: {violations}")

    def test_skip_serializing_if_on_a_type_never_touched_by_bincode_does_not_fire(self):
        """The other half of the experiment. A type that is real, has the
        attribute, round-trips fine as JSON -- and is never an argument to
        bincode::serialize/deserialize anywhere -- must not be flagged. A
        guard blind to this distinction is a "flag everything" guard, which
        the issue explicitly calls out as equally useless as flagging
        nothing."""
        with TemporaryDirectory() as d:
            root = build(
                Path(d),
                {
                    "crate/src/wire.rs": self.BOUNDARY_TYPE.format(attr=""),
                    "crate/src/http_api.rs": (
                        "#[derive(Serialize, Deserialize)]\n"
                        "pub struct ApiResponse {\n"
                        "    pub status: String,\n"
                        "    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n"
                        "    pub message: Option<String>,\n"
                        "}\n"
                        "\n"
                        "pub fn handle(r: &ApiResponse) -> String {\n"
                        "    serde_json::to_string(r).unwrap()\n"
                        "}\n"
                    ),
                },
            )
            violations, unresolved, root_names, reached = guard.run(root)
            named = {(v.type_name, v.field_name) for v in violations}
            self.assertNotIn(
                ("ApiResponse", "message"),
                named,
                "ApiResponse is JSON-only (serde_json::to_string, never bincode) and must not be flagged",
            )
            self.assertNotIn("ApiResponse", root_names, "ApiResponse must not even be discovered as a bincode root")


class HistoricalShapes(unittest.TestCase):
    """#1053 and #2922, reconstructed as fixtures. If the guard would not
    have caught either, it is the wrong guard (the issue's own words)."""

    def test_1053_shape_top_level_field_on_the_checkpointed_state_type(self):
        """#1053: `State`'s own top-level fields carried the attribute,
        undecodable when `force_checkpoint_sync`'s `bincode::serialize(state)`
        wrote a None-bearing checkpoint."""
        with TemporaryDirectory() as d:
            root = build(
                Path(d),
                {
                    "native-chain/src/storage.rs": (
                        "#[derive(Serialize, Deserialize)]\n"
                        "pub struct State {\n"
                        "    pub accounts: u64,\n"
                        "    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n"
                        "    pub last_finalized_at: Option<i64>,\n"
                        "}\n"
                        "\n"
                        "fn force_checkpoint_sync(state: &State) -> Result<Vec<u8>, ()> {\n"
                        "    Ok(bincode::serialize(state).unwrap())\n"
                        "}\n"
                    )
                },
            )
            violations, unresolved, root_names, reached = guard.run(root)
            named = {(v.type_name, v.field_name) for v in violations}
            self.assertIn(("State", "last_finalized_at"), named, f"the #1053 shape must be caught: {named}")

    def test_2922_shape_p2p_gossip_receipt(self):
        """#2922 instance 1: `ForecastVerificationReceipt`, decoded from a
        gossiped byte blob via a typed `let` binding, matching
        p2p_events.rs's real shape."""
        with TemporaryDirectory() as d:
            root = build(
                Path(d),
                {
                    "native-chain/src/bin/launch_api/p2p_events.rs": (
                        "#[derive(Serialize, Deserialize)]\n"
                        "pub struct ForecastVerificationReceipt {\n"
                        "    pub manifest_digest: String,\n"
                        "    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n"
                        "    pub zk_proof: Option<String>,\n"
                        "    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n"
                        "    pub signature: Option<String>,\n"
                        "}\n"
                        "\n"
                        "fn handle_forecast_result(receipt_bytes: &[u8]) {\n"
                        "    let receipt: ForecastVerificationReceipt = match bincode::deserialize(receipt_bytes) {\n"
                        "        Ok(r) => r,\n"
                        "        Err(_) => return,\n"
                        "    };\n"
                        "    let _ = receipt;\n"
                        "}\n"
                    )
                },
            )
            violations, unresolved, root_names, reached = guard.run(root)
            named = {(v.type_name, v.field_name) for v in violations}
            self.assertIn(("ForecastVerificationReceipt", "zk_proof"), named)
            self.assertIn(("ForecastVerificationReceipt", "signature"), named)

    def test_2922_shape_nested_field_added_after_the_first_fix(self):
        """#2922 instance 2: `TreasuryTransactionRecord`, added to
        `GovernanceState` (nested inside the checkpointed `State`) AFTER
        #1053's fix landed, never re-swept. This is the shape an instance
        guard (a canary tied to one hardcoded type) cannot catch by
        construction -- the whole reason #2924 exists. The guard must reach
        it purely by walking State's field graph, with State's own name as
        the only thing anyone had to get right."""
        with TemporaryDirectory() as d:
            root = build(
                Path(d),
                {
                    "shared-protocol/src/blockchain/state.rs": (
                        "#[derive(Serialize, Deserialize)]\n"
                        "pub struct State {\n"
                        "    pub accounts: u64,\n"
                        "    pub governance: GovernanceState,\n"
                        "}\n"
                        "\n"
                        "#[derive(Serialize, Deserialize)]\n"
                        "pub struct GovernanceState {\n"
                        "    pub treasury_transactions: Vec<TreasuryTransactionRecord>,\n"
                        "}\n"
                        "\n"
                        "#[derive(Serialize, Deserialize)]\n"
                        "pub struct TreasuryTransactionRecord {\n"
                        "    pub amount: u64,\n"
                        "    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n"
                        "    pub memo: Option<String>,\n"
                        "}\n"
                    ),
                    "native-chain/src/storage.rs": (
                        "fn encode_state_checkpoint(state: &State) -> Vec<u8> {\n"
                        "    bincode::serialize(state).unwrap()\n"
                        "}\n"
                    ),
                },
            )
            violations, unresolved, root_names, reached = guard.run(root)
            named = {(v.type_name, v.field_name) for v in violations}
            self.assertIn(
                ("TreasuryTransactionRecord", "memo"),
                named,
                f"must reach two levels of nesting below the root without a hand-added entry: {named}",
            )
            # Three types reached from the one root: State itself,
            # GovernanceState (one level of nesting), TreasuryTransactionRecord
            # (two levels) -- confirming the walk actually went two hops deep
            # rather than the assertion above passing by coincidence.
            self.assertGreaterEqual(reached, 3, f"expected to reach State, GovernanceState, and TreasuryTransactionRecord: count={reached}")


class SkipIsNotSkipSerializingIf(unittest.TestCase):
    """Regression for a false positive this guard's own development found
    and fixed against the real codebase: `#[serde(skip)]` is a DIFFERENT,
    symmetric, always-safe attribute (bincode writes and reads nothing for
    the field, in both directions) and must not propagate reachability into
    its type. Reconstructs the real shape:
    `Transaction.parsed_reveal: OnceLock<Option<RevealPayload>>` is
    `#[serde(skip)]`'d (a lazy JSON-parse cache, #1378);
    `RevealPayload`'s own `skip_serializing_if` fields are genuinely
    JSON-only and must not be flagged."""

    def test_serde_skip_field_does_not_propagate_reachability(self):
        with TemporaryDirectory() as d:
            root = build(
                Path(d),
                {
                    "shared-protocol/src/blockchain/transaction.rs": (
                        "#[derive(Serialize, Deserialize)]\n"
                        "pub struct Transaction {\n"
                        "    pub data: String,\n"
                        "    #[serde(skip)]\n"
                        "    pub parsed_reveal: std::sync::OnceLock<Option<RevealPayload>>,\n"
                        "}\n"
                        "\n"
                        "#[derive(Serialize, Deserialize)]\n"
                        "pub struct RevealPayload {\n"
                        "    pub station_id: String,\n"
                        "    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n"
                        "    pub data_cid: Option<String>,\n"
                        "}\n"
                        "\n"
                        "fn send(tx: &Transaction) -> Vec<u8> {\n"
                        "    bincode::serialize(tx).unwrap()\n"
                        "}\n"
                    )
                },
            )
            violations, unresolved, root_names, reached = guard.run(root)
            named = {(v.type_name, v.field_name) for v in violations}
            self.assertNotIn(
                ("RevealPayload", "data_cid"),
                named,
                "RevealPayload is reached ONLY via a #[serde(skip)] field, which never touches "
                "the wire in either direction -- must not be flagged",
            )


class MixedFormsInOneType(unittest.TestCase):
    def test_one_reachable_field_flagged_alongside_clean_siblings(self):
        with TemporaryDirectory() as d:
            root = build(
                Path(d),
                {
                    "crate/src/wire.rs": (
                        "#[derive(Serialize, Deserialize)]\n"
                        "pub struct Mixed {\n"
                        "    pub required: String,\n"
                        "    #[serde(default)]\n"
                        "    pub clean_default_only: Option<String>,\n"
                        "    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n"
                        "    pub the_defect: Option<String>,\n"
                        "}\n"
                        "\n"
                        "fn go(m: &Mixed) -> Vec<u8> {\n"
                        "    bincode::serialize(m).unwrap()\n"
                        "}\n"
                    )
                },
            )
            violations, unresolved, root_names, reached = guard.run(root)
            named = {(v.type_name, v.field_name) for v in violations}
            self.assertEqual(named, {("Mixed", "the_defect")}, named)


if __name__ == "__main__":
    unittest.main()
