# ADR: one scarcity rule — the app shows what the chain pays

Status: **Accepted (owner, 2026-10-24).** An options document for the owner. It records no decision
and changes no reward, display or API.

Date: 2026-10-23.

Refs: `wrfcoin/core4-archive#3563`, a private backlog issue. It asks that the app show exactly
what the chain pays, unifying the paid rule, the displayed rule and the unused tables. It also
asks that the white paper's human-impact and network-demand terms be either added or explicitly
retired. Companion: `2026-10-23-3561-tokenomics-reconciliation.md`. The white paper (V0.2, §5.1)
says: "a proximity value will weight the reward based on data scarcity, human impact, and network
user demand."

Code references are to public main of `core4` (`bde93b2`), `backend` (`06d1988`) and `mobile`
(`7264df8`). "PoP" is `shared-protocol/src/consensus/proof_of_publishing.rs`.

## Context: four scarcity tables, one pays

| # | Where | Grid | Station source | Rule | Pays? |
|---|---|---|---|---|---|
| 1 | `core4 shared-protocol/src/consensus/data_desert.rs:6,30-37,49-67` | `DESERT_GRID_RESOLUTION_KM = 222` (≈ 2°) | `station_counts` in chain state: **active stations registered on-chain** (`state.rs:1935-1950`, part of state-root material) | count < 3 → **×1.5** (`desert_bonus_bps = 150`), else ×1.0 | **Yes.** Applied per reveal at PoP:5053-5077 (and PoP:5428-5475). Composed with the hyper-local accuracy multiplier and capped at **×2.0** combined (`hyper_local.rs:101-117`, PoP:2351). |
| 2 | `core4 shared-protocol/src/blockchain/state.rs:416-455` `ScarcityPolicy` | n/a | n/a | `data_desert_multiplier`, `proximity_multiplier`; default 1.0, max 5.0; governable | **No.** Its only reader is work-task prioritisation (`apply_useful_work_policy_hooks`, PoP:6752-6775), not station payout. |
| 3 | `backend server/routes/weather-routes.js:546-585` (`GET /coverage-zones`) | `ROUND(lat,1)`, `ROUND(lon,1)`: **0.1°** (≈ 11 km); the code comment says "~0.5°" | **Backend database** `weather_stations WHERE is_active`, not the chain registry | count ≥ 3 → 1.0, 1–2 → **1.5**, 0 → 2.0 (**unreachable**: a grouped row always has count ≥ 1) | Display only. The mobile app renders it (`lib/features/gamification/presentation/screens/data_desert_map_screen.dart:214`, fallback table 2.0/1.5/1.0 at :272-276). |
| 4 | `core4 geo-grid/src/lib.rs:511-518` `ScarcityRegion::reward_multiplier` | n/a | n/a | Dense 1.0 / Normal 1.5 / Sparse 2.5 | **No.** No caller outside `geo-grid`. |

No node API route exposes the paid multiplier: no `desert` or `scarcity` route exists in
`native-chain/src/bin/launch_api`.

**Consequences today.** Rule 1 and rule 3 differ in grid size (≈ 222 km vs ≈ 11 km) **and** in
which stations they count (chain registry vs backend database). So a station can be shown "1.0×"
and be paid 1.5×, or the reverse. The owner's observation of "1.5× in northern Minnesota" is
rule 3, a display bucket. Whether it matches the payout depends on how many registered stations
share that station's ≈ 2° cell. Neither rule implements the paper's human-impact or network-demand
terms.

## Decision drivers

- **Honesty:** a user must never be shown a multiplier the chain does not pay.
- **Determinism:** the paid rule is consensus. Any change to it is a state-root change behind an
  activation height.
- **Location privacy:** a public scarcity map must not reveal station positions more precisely than
  the project's public coarse-location rule. Finer cells with per-cell counts leak more.
- **Gaming resistance:** finer paid cells make it cheaper to place a station alone in an empty cell
  and collect the bonus. Registered-station counts are only as honest as station registration and
  ownership proofs (`#3568` / `#3569` in the private backlog).
- **Paper alignment:** scarcity, human impact and network demand (§5.1).

## Options

### A. Where the displayed number comes from

| Option | What | Consequences |
|---|---|---|
| **A1** The node exposes the paid rule; clients render it | A read-only node route, for example `GET /network/scarcity?cell=<id>` or `GET /stations/{id}/scarcity`, returns the multiplier from `desert_multiplier` against current `station_counts`. Backend `/coverage-zones` and mobile render only that. The backend's own rule and the mobile fallback table are retired. | **No consensus change.** One source of truth. The display moves to ≈ 2° cells, which are coarser and arguably better for privacy. The map becomes less "local-looking". |
| **A2** Change the paid rule to the display rule (0.1° cells) | Consensus moves to finer cells. | State-root change behind an activation height. More gameable (many empty cells). Exposes finer station density. Requires counting the same station source as the chain. |
| **A3** A middle grid for both (for example ≈ 0.5° / 50 km, which the backend comment already claims) | Re-tune both rules to one shared grid. | Consensus change, as A2. A compromise between gaming, privacy and local relevance. |

In every option the display must read the **same station source** the chain counts, which is the
on-chain registry, and the unreachable 2.0 "desert" bucket must be removed or made real.

### B. The `ScarcityPolicy` governance knob

| Option | Consequences |
|---|---|
| **B1** Wire `data_desert_multiplier` into payout as the governed size of the desert bonus | Makes the bonus adjustable without a binary release. It is a consensus change, and the knob's 5.0 maximum exceeds the 2.0 combined cap, so the bound would need re-deriving. |
| **B2** Keep it for task prioritisation only, and document it | No change. Removes the false impression that it affects pay. |
| **B3** Remove it | Smaller surface. It is state-root material, so removal is itself a consensus change. |

### C. The unused `geo-grid` table

| Option | Consequences |
|---|---|
| **C1** Delete `ScarcityRegion::reward_multiplier` | Removes a misleading second table (2.5× is never paid). Library-only change. |
| **C2** Keep it, renamed or marked "illustrative, not paid" | Lower churn, but keeps the confusion. |

### D. The paper's human-impact and network-demand terms

| Option | Consequences |
|---|---|
| **D1** Explicitly retire them (record as a departure) | Nothing to build. The scarcity bonus stays a pure coverage measure. |
| **D2** Add human impact (for example population or exposure in the cell from a public dataset) | Needs a deterministic, versioned dataset committed into consensus inputs (licence and update process). |
| **D3** Add network demand (for example data-request volume per cell) | Demand signals are easy to inflate. A sybil client can "demand" its own cell. Needs anti-gaming design first. |
| **D4** Defer until the sinks program and the early-warnings pilot define "impact" | Keeps the paper's intent open without inventing a metric now. |

## Consequences common to all options

- A display-only fix (A1 with B2 and C1) can ship without consensus work and gives the honesty
  property immediately.
- Any change to what is **paid** (A2, A3, B1, D2, D3) needs an activation height via the
  governance-signed schedule extension, gate-off parity, a state-root differential and replay
  tests.
- Implementation spans three repositories: a core4 route, a backend `/coverage-zones` change and a
  mobile map change. That means three PRs in three repositories.
- **Acceptance test (any option):** for sample coordinates, the value the app shows equals the
  value the chain pays at that height.

## Questions for the owner

1. **Display source:** A1 (expose the paid ≈ 2° rule as-is), A2 (pay on the 0.1° display grid) or
   A3 (one middle grid for both)?
2. **`ScarcityPolicy`:** wire into payout (B1), keep for task prioritisation only (B2) or remove
   (B3)?
3. **The `geo-grid` 1.0/1.5/2.5 table:** delete (C1) or keep labelled (C2)?
4. **Human impact and network demand:** retire (D1), build (D2/D3) or defer (D4)?
5. Should the **"desert" (no-station) case** have a distinct, higher bonus? Today it cannot arise
   for a paying station, because a paying station is itself counted in its cell.

## Decision (accepted 2026-10-24)

Owner ruling, 2026-10-24: "Merge as accepted, recommended answers." Each open question gets the
answer this document itself recommends, quoted with its line. Where the document makes no
recommendation, the answer is **UNANSWERED — owner**: none was invented.

This ADR is an options document and recommends none of its options, so every question is UNANSWERED
— owner. Nothing here changes what pays on chain: an answer that changes the paid rule (A2, A3, B1,
D2, D3) is policy only, and implementation requires a separate consensus PR + owner deploy.

1. *Display source: A1 (expose the paid ≈ 2° rule), A2 (pay on the 0.1° grid) or A3 (one middle
   grid)?* **UNANSWERED — owner.** Relevant text (not a recommendation): line 88: "(A1 with B2 and
   C1) can ship without consensus work". Stated as a consequence, not a recommendation.
2. *`ScarcityPolicy`: wire into payout (B1), task prioritisation only (B2) or remove (B3)?*
   **UNANSWERED — owner.** See the note on question 1.
3. *The `geo-grid` 1.0/1.5/2.5 table: delete (C1) or keep labelled (C2)?* **UNANSWERED — owner.**
   See the note on question 1.
4. *Human impact and network demand: retire (D1), build (D2/D3) or defer (D4)?* **UNANSWERED —
   owner.**
5. *Should the no-station "desert" case get a distinct, higher bonus?* **UNANSWERED — owner.**

Stated in every option, so accepted as written (no owner choice involved):

- line 39: "**Honesty:** a user must never be shown a multiplier the chain does not pay." (display
  rule).
- line 59: "In every option the display must read the **same station source** the chain counts"
  (display rule).
- line 60: "the unreachable 2.0 "desert" bucket must be removed or made real" (display rule).
- line 95: "**Acceptance test (any option):**" (acceptance test).
