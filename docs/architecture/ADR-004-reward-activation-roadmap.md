# ADR-004: Reward activation roadmap — getting every ADR-003 role paid on testnet 777

- **Status:** Proposed (drafted 2026-09-09 during the launch-hardening sprint; owner decisions
  listed in §6 are open)
- **Depends on:** ADR-002 (emission schedule), ADR-003 (role partition, ratified), core4#2969
  rulings (a), (a2), (c), bonus headroom, and the 2026-09-09 window-cap ruling (N = 2)
- **Verified against:** `origin/main` @ `4e24be02e` and PR #3045 @ `efb6dff80`

## 1. Why this ADR exists

ADR-003 defines five roles and the exact basis-point partition of every block's emission.
It does not say, per role, what is on `main` today, what the gap is, which change is a
genesis-cut-class change, and which questions are still the owner's. Tonight's audit answered
those questions from the code; this document records the answers so the activation order is a
decision made once, not re-derived by every sprint.

The single most important fact: **only two of the five slices are ever charged on `main`.**

| Role | Share (testnet bps) | `charge_emission_slice` call site on `main` | Paid today? |
|---|---|---|---|
| Contribution | 6 900 | `shared-protocol/src/blockchain/state.rs:3328` (coinbase + contribution + forecast records) | **yes** |
| Attester | 1 500 | `state.rs:3737` (`ApplyAttesterSettlements`) | code-complete, **not activated** (cut) |
| Producer | 1 000 | none — `EmissionRole::Producer` defined at `proof_of_publishing.rs:809`; `transition_extractor.rs:2609` asserts spent == 0 | **no** (Phase B) |
| Node | 500 | none — no `ServiceReceipt` symbol exists | **no** (Phase C) |
| Faucet | 100 (ADR) / **absent in code** | `EMISSION_ROLE_COUNT = 4` (`:757`), `EMISSION_SHARE_BPS = [7_000, 1_500, 1_000, 500]` (`:801`) | **no** — live ADR↔code divergence, core4#3039 |
| Peer | 0 | — | never, by design (ADR-003 §5) |

Under ADR-003's fail-closed rule an uncharged slice is simply not emitted, so realised issuance
runs ~15 % under the ADR-002 curve until Phases B/C land. Nothing is lost, but nobody in those
roles is paid either.

## 2. Per-role state, gap, and mechanism

### 2.1 Contributor (weather station) — paid; sizing fixed at the cut
- **Main:** `ContributionRewardRecord` → `charge_emission_slice(Contribution)`; per-user window
  cap `per_user_emission_cap = adjusted_block_reward / 100` (`proof_of_publishing.rs:722`) with the
  (a2) clamp `min(decided, remaining)` (`state.rs:5044`).
- **Gap:** decisions were sized against `E(t)` and clamped by `emission_rate`; the window cap
  forfeited ~98 % of the (c) budget at genesis (rehearsal 2026-09-03, defect 1).
- **Fix, on PR #3045 (cut-bound):** `contribution_reward_allowance(h) = slice − coinbase`,
  `contribution_decision_base(h) = allowance ÷ 2.0×` (bonus headroom), and — ruled 2026-09-09 —
  `per_user_emission_cap(h) = 2 × contribution_decision_base(h)` (the whole allowance; 420 WRF at
  genesis instead of 8). `emission_rate` remains hashed genesis content and sizes nothing.
- **Metadata:** PR #3030 makes `rewards_minted_total` report what was credited and adds
  `rewards_decided_total`.

### 2.2 Forecast producer / data analyzer — live lane, nobody feeds it
- **Main:** the forecast lane is **active at height 0** (`FORECAST_MANIFEST_ACTIVATION_HEIGHT = 0`,
  loop `proof_of_publishing.rs:4452–4655`). A `ForecastReveal` whose commit is quorum-attested and
  whose manifest validates mints a `ContributionRewardRecord { reason: "forecast:<commit>" }` from
  the **contribution** slice, sized by `calculate_forecast_skill_reward` (`rewards/emission.rs:175`)
  at skill = receipt score if a `ForecastVerificationReceipt` validates, is signed by a **genesis
  oracle-set key** (`is_authorized_oracle`, `:2162`/`:2643`), matches the manifest digest and
  commit id; else the execution-engine verifier; else `BASELINE_FORECAST_SKILL = 0.1`.
- **Gap:** nothing produces receipts. backend#867 (T+6h re-score + plausibility poller) is open
  with a stalled draft (backend#955); core4#2207 (calibration/plausibility attestation) is open;
  the standalone `forecast-score` repo (CRPS, reliability, anti-gaming) is not consumed by
  consensus. **Hazard:** `forecast-score/src/blockchain.rs:16–40` signs receipts under a chain id
  resolved from the environment with a mainnet fallback; a receipt bound to the wrong chain fails
  signature verification and silently falls to baseline.
- **Mechanism (no consensus change):** a scorer that (1) reads the revealed manifest, (2) waits
  for the target time, (3) scores against the attested observation (or a reference field from
  `oracle-reference`), (4) builds and signs a `ForecastVerificationReceipt` under chain id 777
  with an oracle-set key, (5) attaches it to the reveal. The overnight prototype (see
  `docs/plans/2026-09-09-forecast-proto-report.md` in docs-private) exercises exactly this loop
  on a throwaway chain. Two consensus tests pin the contract: receipt from an oracle-set key
  mints > baseline and charges Contribution; a mainnet-bound receipt on 777 falls to baseline.
- **Owner question:** which key signs (§6 Q3).

### 2.3 Attester — code-complete, activation is the cut
- **Main:** Phase A machinery (#2851 partition, #2852 settlement record + durable duty, #2855
  supply bound, #2897 parity): `stage_attester_settlements` (`proof_of_publishing.rs:3840`),
  `ApplyAttesterSettlements` (`state.rs:3652`), `derive_attester_settlements` (`:4883`).
- **Gap:** ADR-003 rules 3 (eligibility frozen at the crossing block) and 4 (terminal marker) —
  PR #3031, which also moves the state-root tag v12 → v13.
- **Mechanism:** merge #3031; activation = the fresh genesis (owner-only), under the ratified
  testnet-777-only unbonded interim exception. Bond/slash ships with Phase B.

### 2.4 Producer — Phase B, mirror the attester pattern
- **Main:** producer authority is live (`producer_authority.rs`, `block.producer_pubkey`,
  `attach_producer_signature`); the slice is defined, never charged; the coinbase draws from the
  **contribution** slice (`COINBASE_CONTRIBUTION_SHARE_BPS = 2_500`, `:1082`). A stale comment at
  `:4720` claims the coinbase draws the producer slice — it does not; fix in passing.
- **Mechanism (cut-bound, one PR, mirrors 2.3):**
  1. `ProducerSettlementRecord { height, payout_address, amount }` beside `AttesterSettlementRecord`
     (`shared-protocol/src/oracle/attestation.rs`) or a new `consensus/producer_settlement.rs`.
  2. `stage_producer_settlement` next to `:3840`: payout = the roster member's `payout_address`
     resolved for `block.producer_pubkey` via `producer_authority.rs:77`; amount =
     `min(ledger.remaining(Producer), MAX_SUPPLY - total_minted_supply)` (saturating
     headroom); the settlement receipt in metadata reports the actual credit. The unpaid
     remainder stays unspent in this block and is not carried forward.
  3. `StateTransition::ApplyProducerSettlement` beside `state.rs:3652` →
     `charge_emission_slice(Producer, ..)`, fail-closed (unknown producer ⇒ no mint, no halt).
  4. Flip the `transition_extractor.rs:2609` assertion; producer/applier/WAL-replay parity test.
- **Owner question:** bond on 777 (§6 Q2). Default: extend the attester interim exception.

### 2.5 Node service — Phase C, blocked on #843
- **Main:** nothing. Parameters are ratified (stake floor 250 WRF, 20 %/epoch cap, epoch 1 024,
  challenge 4 096 — ADR-003 appendix).
- **Mechanism:** after #843 snapshot-join lands, generalise the #841 receipt pattern: the consumer
  signs an Ed25519 receipt naming server peer id, byte range / manifest hash, its own peer id and
  height; the server batches receipts on chain; settlement per epoch from the Node slice with
  anti-self-dealing (same-account and unstaked consumers count zero).
- **Decision:** do not add a receipt type before there is a service to sign it — dead consensus
  code is worse than an uncharged slice.

### 2.6 Faucet — the fifth role (core4#3039)
- **Main:** four roles in code; the faucet is off-chain JS (`infra/testnet/services/faucet/server.js`),
  manually funded; infra#915 added the high-water refill; infra#908 asks where the funds come from.
- **Mechanism (cut-bound, stacked on #3045):** `EMISSION_ROLE_COUNT` 4 → 5, `EmissionRole::Faucet`,
  mainnet `7_000/1_500/1_000/500/0` and testnet `6_900/1_500/1_000/500/100` selected by the
  **genesis-recorded** chain id (`GENESIS_CHAIN_ID_KEY`, `chain_binding.rs:50`) — never by the
  environment (#2713/#3039 silent-self-fork class); five-element remainder order; the ADR's
  `E(t) = 75` presence vector and `E = 0` / mainnet-zero vectors; sink = a genesis `faucet_address`,
  fail-closed (absent ⇒ the slice mints zero).
- **Owner question:** sink (§6 Q1).

### 2.7 Peer — unpaid, by design
ADR-003 §5 and "Alternatives rejected": relay work is unverifiable; paying it turns the gossip mesh
into a sybil market. Peers that *serve* earn as nodes (2.5). No change.

## 3. Ordering — one reward-machinery change, golden-tested once

Every reward-consensus change is a fresh-genesis-cut-class change on testnet (#2034 precedent,
#2976 A3+A5+A6 guidance: "do it once, golden-test it once").

1. **#3031** attester rules 3+4 (state-root v13).
2. **#3045** (c) budget + bonus headroom + N = 2 window cap.
3. **#3039** faucet role — the enum/partition change every later slice depends on.
4. **Phase B** producer settlement, stacked on 3, authored after 1 merges (shared apply-arm regions).
5. Regenerate `tests/integration/fixtures/determinism/expected_roots.json` **once**, after 4,
   with `REGENERATE_DETERMINISM_FIXTURE=1`, and explain the diff in that PR.
6. **Forecast receipt signer + tests** — independent, no consensus bytes change; can land any time.
7. **#3030** credited metric — rebase last.
8. Genesis cut (owner) → Phase A/B/faucet/cap all activate together.
9. **Phase C** after #843. **Phase D gate:** all infra roles earning on 777, exercised by the
   three owner hosts, before any outside operator invitation (ADR-003 sequencing §4).

## 4. Shared-file conflict map (why the stack order matters)

| PR / change | `proof_of_publishing.rs` regions | `state.rs` regions | other |
|---|---|---|---|
| #3045 | `:717–724` cap, `:1121–1146` allowance/base, tests `:1440–1560`, forecast sizing `~:4700` | test module only | — |
| #3031 | `:3850` staging | `:1357` field, `:2277` root tag, `:3702`, `:4070–4110`, `:4967` | `attestation.rs`, fixture |
| #3039 | `:754–870` partition + tests `:1130–1420` | ledger default | `transition_extractor.rs` parity |
| #3001 (#2700 block size) | `create_next_block :2589+` | — | — |
| #2974 (refusal logging) | `create_next_block` exits | — | — |
| Phase B | `create_next_block` staging near `:3840` | apply arms near `:3652` | `attestation.rs`, `transition_extractor.rs:2609` |

Disjoint line regions ⇒ rebase-only conflicts; Phase B is the only change that overlaps two
open PRs at once, which is why it is authored last.

## 5. What "activated" means, per role — the acceptance evidence

| Role | Evidence that it pays (runbook step 9 extension) |
|---|---|
| Contributor | first max-quality decision in a window: `credited == decided` (no clamp), `rewards_minted_total == credited` (#3030), ≥ 1 000 000 base units |
| Forecast | a reveal with a receipt: `forecast_rewards_with_receipt_count == 1`, `forecast_rewards_baseline_count == 0`, the receipt's `skill_score` > `BASELINE_FORECAST_SKILL` (0.1), and the payee's `contribution_rewards.real_paid` increases by the credited amount. `forecast_rewards_minted_total` is evaluated only **after #3030** (until then it reports the decided, not credited, amount — pinned by the existing test at `proof_of_publishing.rs:~12346`). |
| Attester | for the settling block (normally the crossing block; under the durable duty possibly a later one — label which the fixture exercises): the sum of `AttesterSettlementRecord` allocations equals the available attester slice, and each eligible attester's balance increases by **the sum of its exact typed per-commit allocations across every decision settling in that block** — ADR-003 rule 5 divides the slice by all eligible commits across all settling decisions, floors, and hands the remainder out one base unit at a time in `(decision id, attester id)` order, so `slice / n` is only true for a single-decision, divisible fixture (state that explicitly if used). |
| Producer | block metadata carries the producer settlement; roster member's payout balance += `min(remaining(Producer), supply headroom)` |
| Faucet | `faucet_address` balance += `slice(Faucet)` per block on 777; zero on a mainnet-id chain |
| Node | (Phase C) per-epoch settlement from receipts |

All three hosts must agree on `state_root` after each of these blocks — the rehearsal's step-8
curl block with the VOID guard is the check.

## 6. Owner decisions (options, recommended default first)

1. **Faucet sink** — (a) on-chain slice → genesis `faucet_address`, the dispenser draws from it
   (#3039 + infra#915) **[default]**; (b) producer-payout transfer (infra#908); (c) manual top-up.
2. **Producer bond on testnet 777 — owner decision: none (2026-09-09).** Phase B
   extends the attester unbonded interim exception on 777; it introduces no producer-eligibility
   bond. Existing account stake allocation is separate from producer eligibility.
   [Owner ruling on #3069](https://github.com/wrfcoin/core4/pull/3069#issuecomment-5605479578).
3. **Forecast receipt signer** — (a) one existing genesis oracle key (oracle-reference on N5)
   signs receipts — zero consensus change **[default]**; (b) a dedicated scorer key in the oracle
   set (reopens #2976 A2); (c) leave at baseline 0.1.
4. **Contributor balance carry-forward at the cut** (#2976 A4) — (a) accept the reset, publish
   the pre-cut ledger as evidence **[default for testnet]**; (b) add a genesis allocation field and
   re-credit at the ×10⁸-corrected denomination.

## 7. Alternatives rejected
- **Pay peers for relay/uptime** — rejected on principle in ADR-003; not reopened.
- **A separate "analyst" slice for forecasts** — unnecessary: the forecast lane already draws the
  contribution slice by skill, and a fifth slice would re-split a ratified partition.
- **Activate Phase B before #2220 is confirmed on every import path** — the producer slice pays
  whoever signs the block; an unverified import path would pay a forger.
- **Node receipts before #843** — dead code with a consensus footprint.
