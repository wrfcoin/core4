# ADR: `evm_compatibility` is PARKED for this testnet phase (#3208)

- **Status:** Proposed 2026-09-16 — **sequenced to follow `wrfcoin/defi-protocols#69`**, whose
  ADR 0001 is itself Proposed (PR wrfcoin/defi-protocols#71). This record becomes final the moment
  #69 is approved as proposed; if the owner instead chooses #69's option (c), this record flips to
  "in scope" and the follow-up story in §Next step is filed. Either way the answer is written down
  and dated, which is what #3208 asks for.
- **Epic:** wrfcoin/infra#945. **Related:** core4#2998 (confirms the current OFF state is
  intentional and correctly wired, not a bug), core4#2080 / #2176 / #3023 (the `privacy`, `zk`,
  `execution` features, each of which — unlike this one — has an open epic driving it).

## What exists (measured on `main` = `72a993552`, 2026-09-16)

| Fact | Value |
|---|---|
| Crate | `evm/` (`wrfcoin-evm`): `lib.rs`, `runtime.rs`, `state.rs`, `transaction.rs` — **712 lines, 16 `#[test]`** |
| Wiring | `native-chain/Cargo.toml`: `wrfcoin-evm` is an **optional** dependency behind the `evm_compatibility` feature; not in the default set |
| Route surface | `GET /evm_compatibility/status` → `chain::evm_compatibility_feature_disabled`, a `feature_disabled_envelope` (explicit, intentional — #2998 audited it) |
| Last change | `748abf8ad` 2026-04-09 "fix(evm): spec-compliant CREATE address derivation and value transfer (#1207)" — **160 days** without a commit |
| Consumers | none: no launch_api route reaches the crate, no CLI group, no backend route, no frontend surface |
| Tracking | no open issue or epic names EVM compatibility as a product goal |

## The decision this one follows

`defi-protocols#69` ADR 0001 evaluated three ways DeFi reaches users. Its option **(c) EVM-deployed**
— deploy the mirrored Solidity contracts to a real EVM chain — is the only path in the platform's
current plans that would make `evm_compatibility` load-bearing. ADR 0001 **rejects (c) for this
phase** (needs a live core4↔EVM bridge, wrapped WRF, wallet-connect in frontend, a second chain to
operate, covers 3 of 12 DeFi mechanisms, bypasses 454 Rust tests) and **recommends (b)**, a
standalone Rust service proxied by backend. Nothing else on the roadmap consumes an EVM runtime
inside the node.

## Decision

**Park `evm_compatibility` for the testnet phase.** Concretely:

1. The feature stays compiled-out (no change to `native-chain/Cargo.toml` defaults) and keeps its
   explicit `feature_disabled_envelope` route — it already answers honestly; do not remove the
   route, which is what tells a caller the feature exists and is off.
2. No `FeatureSchedule` entry, no genesis-cut coupling, no wiring story.
3. **The crate is not deleted.** 712 lines with 16 tests and a spec-compliance fix (#1207) is real
   work; deletion is a separate, later decision if the park outlasts the next genesis cut. Until
   then it costs nothing at runtime (optional dep) and only CI time when someone builds with the
   feature on.
4. Ownership: this record is the owner-of-record for the feature's status; it replaces "nothing is
   tracking this".

### Revisit triggers (any one re-opens this)

- The owner amends `defi-protocols#69` to choose option (c), or a later DeFi epic picks the EVM path.
- A core4↔EVM bridge with wrapped WRF goes live end-to-end (the prerequisite ADR 0001 names for (c)).
- A product decision to run WRFCoin contracts on-chain natively (e.g. the `smart-contracts` repo
  gaining a real deployment target that is *this* node rather than an external EVM).

## Next step

- **If #69 is approved as proposed (expected):** nothing to file. Close #3208 pointing here. Note in
  the next quarterly feature-matrix pass whether the park should become deletion.
- **If #69 is amended to (c):** file `core4` follow-up "Activate `evm_compatibility`: FeatureSchedule
  entry, launch_api routes replacing the disabled envelope, `wrfcoin evm` CLI group, bridge
  contract" as an sp:13 story under the DeFi wiring epic, blocked-by the bridge story. Not filed
  now — that would presume the outcome #3208 explicitly says not to presume.

## Consequences

- **Positive:** one less silently-rotting feature; a caller hitting `/evm_compatibility/status`
  gets the same explicit "disabled" answer plus a dated reason on record.
- **Negative:** if the EVM path is chosen later, the crate will be ~6+ months behind the node's
  state/transaction types and will need a re-integration pass, not a flag flip.
- **Neutral:** `privacy`/`zk`/`execution` are unaffected; they remain gated by their own epics.
