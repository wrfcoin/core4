# Exploration: hashquilt sub-chains, historical blocks and addendum blocks vs the domain-tagged chain

Status: **Exploration (draft for an owner decision).** It recommends and builds nothing. Any
element the owner adopts gets its own story.

Date: 2026-10-28.

Refs: `wrfcoin/core4-archive#3567`, epic `wrfcoin/core4-archive#3560`. Related: `#3565` (one
submission per device per block and the late-data penalty, the paper's addendum intent), `#3566`
(block speed), core4#2793 (production BFT finality). Code references are to public `core4` main
`cc7f4d4`.

## 1. The two shapes

### White paper V0.2

- **§4.2 Hashquilt.** Each data stream (station weather, METAR, buoys, soil, satellites…) is its
  own sub-chain with its own consensus, and the sub-chain heads are hashed into a financial chain.
- **§4.3 Historical blocks.** Archives such as NOAA are back-filled, valued by the community, and
  sealed.
- **§4.4 Addendum blocks.** Late data and flags against false submitters attach to a past block as
  sub-blocks.
- **§5.1 Throttle.** Each of the first twelve sub-chains adds 1/12 of the emission throttle.

### As built

- **One chain, one consensus.** Proof of Publishing is live. Other engines in
  `consensus-engines` are chain-wide, not per stream.
- **Streams are a `domain` tag.** The domain is a metadata key on a contribution
  (`MULTIDOMAIN_DOMAIN_KEY = "domain"`, `proof_of_publishing.rs:118`). The domains are weighted by
  `MultidomainRewardPolicy` (soil, ocean and hydrology toggles and weights, `state.rs:458`), inside
  the single contribution slice.
- **One state root and one finality path.** `MAX_REORG_DEPTH = 100` (`blockchain/chain.rs:47`).
  BFT finality is still open in core4#2793.
- **No sub-chain code.** A grep for hashquilt or sub-chain finds nothing. The closest building
  blocks are the layer-2 rollup types (`layer2/rollup.rs`, `L1BatchReceipt` at :171), which commit
  a batch root to the base chain, and the `da` crate (data-availability sampling).

## 2. What sub-chains would buy

| Claimed benefit | With sub-chains | With today's domain tags | Net |
|---|---|---|---|
| Per-stream throughput | Each stream scales on its own | All streams share block space. The testnet has a handful of stations, so blocks are far from full | **Not needed now.** Block space is nowhere near a limit |
| Per-stream rules (validation, quorum, cadence) | Natural: each sub-chain has its own | Rules already key on `contribution_type` and `domain` (the weather schema, the forecast manifest path, multidomain weights) | **Achievable without sub-chains** |
| Fault isolation (a bad stream cannot halt the others) | Strong | A malformed contribution is refused per transaction (`classify_block_transaction`); a consensus bug in one stream's rule still halts the chain | **Partial gap.** Better addressed by per-domain feature gates and per-transaction refusal than by separate chains |
| Per-stream emission throttle (§5.1, 1/12 per chain) | Direct | Domain weights inside the contribution slice | **Same lever**, different form |
| Different consensus per stream (§4.6: dPoS, proof of ownership, capacity, analysis) | Possible | Not possible: one engine | **The real difference.** No current stream needs a different engine. Forecasts use commit/reveal plus oracle attestation; keepers and processors use receipts, as in node-share settlement |

## 3. Cost and migration risk

| Item | Sub-chains | Domain tags |
|---|---|---|
| New code | Per-chain block production, finality, fork choice, cross-chain header commitments in the financial chain, cross-chain reward settlement, P2P topics per chain, sync per chain. **21+ points** by the gap analysis, realistically several epics | None |
| Consensus risk | High. A second fork-choice and finality surface while BFT finality (core4#2793) is not yet production-grade on the one chain | — |
| Migration of the live testnet | A new genesis or a long gated transition. Every state-root, WAL and snapshot path is per chain | — |
| Operator load | One node set per stream, or one node running many chains | One node set |
| Emission | A cross-chain partition must stay `≤ E(t)` across chains. The single `EmissionPartitionLedger` (PoP:996 rules) would have to become a cross-chain ledger | Unchanged |

## 4. Recommendation per element

| Element | Recommendation | Why |
|---|---|---|
| **Sub-chains (§4.2)** | **Supersede.** Keep the single chain with domain tags. Record this as a deliberate departure | Each benefit except per-stream consensus is reachable with tags, gates and weights. Per-stream consensus has no current customer. The cost lands on finality, which is the release blocker. Revisit when one stream measurably needs its own engine or its own throughput |
| **Historical blocks (§4.3)** | **Adapt.** Treat a back-filled archive as a domain (`domain = "historical"`) whose records are sealed by a signed import receipt (an archive source, a range and a content hash) and pinned in storage (`storage-providers`). Do not create a chain | It gives provenance and permanence without a second consensus. Pay through bounties rather than emission, since nobody observed the data on WRFCoin |
| **Addendum blocks (§4.4)** | **Adapt.** Late data goes into the normal chain in a later block and is paid less by the `#3565` late-data curve. Flags against false submitters are the existing quality-slashing and fraud-report paths | `#3565` is that adaptation in code (core4 PR for `device_submission_limit`). A separate sub-block structure would only duplicate it |
| **Per-stream throttle (§5.1)** | **Adapt.** Use domain weights (`MultidomainRewardPolicy`) to throttle a new stream's share | Already the mechanism |

## 5. Interaction with finality and emission

- Finality (core4#2793) is defined for one chain. Sub-chains would need either their own finality
  or a commitment rule ("a sub-chain head is final once the financial-chain block that commits it is
  final"). The second is the rollup pattern `layer2/rollup.rs` already models. It is the path to
  take **if** a stream ever needs its own chain.
- Emission stays one partition. A future stream is weighted inside the contribution slice, or it
  gets its own `EmissionRole` (the sinks ADR shows how), rather than its own curve.

## 6. Questions for the owner (each with a recommended answer)

1. **Sub-chains:** supersede, recorded as a deliberate departure, revisited only when one stream
   needs its own engine or throughput? *Recommended: yes.*
2. **Historical blocks:** adapt as a `historical` domain with signed import receipts and pinned
   storage, paid by bounties? *Recommended: yes. File it as its own story when an archive partner
   is named.*
3. **Addendum blocks:** adapt as late data in later blocks with the `#3565` curve? *Recommended:
   yes.*
4. **Throttle:** domain weights instead of 1/12 per chain? *Recommended: yes.*
