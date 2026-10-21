# Resource ceilings — what is declared, what is enforced, and what actually bounds it

> Re-verified against `main` @ `20c6fd00e` on 2026-08-30 for #2700. Line-pinned claims below name
> that tree. If you are reading this after the code moved, resolve the named symbol and re-run the
> checks in [Reproducing this](#reproducing-this) rather than trusting the line numbers.

`shared-protocol/src/common/constants.rs` declares three constants that read as protocol bounds.
None of the three is consumed by any code. That is #2700; draft PR #2701 carries a test that remains
RED until it stops being true, but that test is not on `main`.

But "unenforced" is not the same as "unbounded", and the difference is where the real risk lives.
Each resource is bounded somewhere today by something else — a transport framing constant, a
config field, a type-specific cap — and several of those bounds are doing resource-limit or
block-admission duty by accident. Changing the coupled limits for an unrelated reason can silently
move the effective boundary. This page is the ledger, so that cannot happen quietly.

The most useful result here is not the confirmation. It is the two places where the obvious reading
is **wrong**: transactions entering through the mempool are capped at 100 KB, about 20× tighter than
the ceiling that supposedly bounds them, and the mempool is genuinely bounded — in bytes, by a field
that merely shares a name with the dead constant. Peer-block transactions bypass the 100 KB mempool
gate, so this is not a protocol-wide transaction bound.

**One-line summary per ceiling:**

| Declared | Value | Enforced? | What actually bounds it today |
|---|---|---|---|
| `MAX_BLOCK_SIZE` | 32 MiB | **No** | Transport framing: **64 KiB by default** on the gossip path (inside independent 1 MiB outer caps), **16 MiB** on the block-sync path. The two ingress paths do not agree, and neither is 32 MiB. A 5 000-transaction-per-block cap exists but is producer-side only. |
| `MAX_TRANSACTION_DATA_SIZE` | 2 MiB | **No** | At mempool admission, a live total-size cap of **100 KB** (`config.max_transaction_size`) — about **20× tighter than the declared ceiling** — plus 64 KiB / 32 KiB type-specific payload caps. Peer-block transactions bypass these admission gates. |
| `MEMPOOL_CAPACITY` | 50 000 txs | **No — but the mempool is bounded** | `mempool_max_size_bytes` (default **50 MiB**) with fee-ordered eviction, plus a 1 024-tx **per-sender** cap. Bounded in **bytes**, never in total count. |

---

## `MAX_BLOCK_SIZE` — declared 32 MiB, enforced by nothing

Declared at `shared-protocol/src/common/constants.rs:143`. Zero consumers outside that line.

### The one block-size comparison in the workspace is dead three times over

`consensus-engines/src/security/attack_prevention.rs:279` compares a block size against a ceiling.
It does not enforce `MAX_BLOCK_SIZE`, and it does not run. Three independent reasons, each
sufficient on its own:

1. **It has no callers.** The comparison sits in `AttackPreventionSystem::validate_block`. Searching
   the workspace for calls to that method returns nothing — `AttackPreventionSystem` is constructed
   and stored by `consensus-engines/src/security/manager.rs:423` and
   `consensus-engines/src/security/coordinator.rs:222`, and neither ever calls `validate_block` on
   it. `native-chain` does not
   reference `SecurityManager` or `SecurityCoordinator` at all, so the type is not reachable from
   the node binary by any path.
2. **It compares against a different number.** The ceiling is `self.config.max_block_size`, whose
   default is **2 MiB** (`consensus-engines/src/security/attack_prevention.rs:86`) — an undeclared
   value 16× smaller than the
   declared 32 MiB. Wiring this path up would enforce a ceiling nothing documents.
3. **Its size function cannot see block size.** `estimate_block_size`
   (`consensus-engines/src/security/attack_prevention.rs:623`)
   computes `80 + Σ(inputs×40 + outputs×34 + 10)`. It **ignores `tx.data` entirely** — the field
   that carries commit/reveal payloads, oracle attestations and every other variable-length
   contribution. A transaction with a 2 MiB payload measures as ~10 bytes here. Even reachable and
   even pointed at the right constant, this function could not detect an oversized block.

Reason 3 is the one worth remembering: it is a mitigation that would still not mitigate after the
first two were fixed.

### What actually bounds a block today: transport framing, at two different values

| Ingress path | Bound | Where |
|---|---|---|
| Gossip (`NewBlock`) | **64 KiB by default; 1 MiB outer caps** | `p2p/src/config.rs:445` defaults `inline_data_max` to 65 536 bytes; `p2p/src/node.rs:4814` enforces it inbound and `p2p/src/node.rs:5942` outbound. Independent 1 MiB outer layers are `max_transmit_size` (`p2p/src/config.rs:444` → `p2p/src/behaviour.rs:187`), `MAX_MESSAGE_BYTES` / `limit_for(NewBlock)` (`p2p/src/message.rs:409`, `p2p/src/message.rs:523`), and `MAX_GOSSIP_MESSAGE_BYTES` (`p2p/src/node.rs:5853`, applied at `p2p/src/node.rs:5954`). Config validation requires `inline_data_max < max_transmit_size` (`p2p/src/config.rs:676`). |
| Block-sync (`block_sync` request/response — how bulk history arrives) | **16 MiB** per sync message | `p2p/src/sync.rs:58` (`MAX_SYNC_MESSAGE_BYTES`), checked on read in `read_message` and on write in `write_message` |

The `SyncServeLimits` amplification ceiling (#2119) would tighten a 1-block sync request to 1 MiB,
but it is **`enforce: false` by default** (`p2p/src/sync.rs:116`) — deliberately observe-only, so a
violation is reported and the response is still served. It is also a **serve-side** policy: it
constrains what this node sends, not what it accepts.

So the effective default ceiling on an incoming block depends on which door it came through:
**64 KiB via gossip, 16 MiB via sync.** The gossip threshold is operator-configurable but must stay
below `max_transmit_size`; the independent 1 MiB layers remain hard outer caps. That asymmetry is
not an explicit block-validity rule; it is transport policy doing the only size enforcement.

**Consequence for anyone changing the gossip limits:** raising `max_transmit_size` alone does
**not** raise the effective default block ceiling: `inline_data_max` still rejects above 64 KiB,
and the other 1 MiB checks remain. Raising `inline_data_max` can move the effective boundary only
up to the outer caps. Moving all coupled layers removes the gossip-path size boundary with no
explicit block-validity check to replace it.

### Two more block-size numbers that are not enforcement either

- **Five more declarations, still no general incoming-block rule.** The shared and legacy `Config`
  structs each default `max_block_size` to **1 MB**
  (`shared-protocol/src/config/settings.rs:185`, `native-chain/src/legacy/settings.rs:117`); their
  only runtime read is a parity assertion (`native-chain/src/parity.rs:86`). `ConsensusConfig`
  declares another 1 MB default (`consensus-engines/src/types.rs:271`), which
  `consensus-engines/src/factory.rs:269` only checks is non-zero. `AttackPreventionConfig` and
  `HotStuffConfig` each default to **2 MiB**
  (`consensus-engines/src/security/attack_prevention.rs:86`,
  `consensus-engines/src/algorithms/hotstuff/types.rs:453`); the former reaches only the dead
  comparison above, and the latter has no read. Together with `MAX_BLOCK_SIZE`, that is six
  declarations/defaults at three distinct values — 32 MiB, 1 MB and 2 MiB — none a general
  incoming-block size rule.
- **A producer-side count cap that is real but one-sided.** `max_transactions_per_block` = 5 000
  (`shared-protocol/src/config/settings.rs:187`) reaches `ProofOfPublishing::max_tx_per_block` via
  `consensus-engines/src/factory.rs:68` (construction) and
  `consensus-engines/src/algorithms/pop_adapter.rs:506` (reconfiguration),
  and is used at `shared-protocol/src/consensus/proof_of_publishing.rs:2666`,
  `shared-protocol/src/consensus/proof_of_publishing.rs:2706`, and
  `shared-protocol/src/consensus/proof_of_publishing.rs:2777` when **selecting** transactions for a
  block. It bounds what this node **builds**, not what it **accepts** — a peer's 50 000-transaction
  block meets no count check on the way in. Asymmetric bounds like this are the ones that look like
  protection in a code read and are not.

---

## `MAX_TRANSACTION_DATA_SIZE` — declared 2 MiB, enforced by nothing; mempool submissions are capped

Declared at `shared-protocol/src/common/constants.rs:89`. Zero consumers.

**This is the row most likely to be over-claimed, so state it carefully: transactions are not
unbounded at mempool admission, and the live admission bound is tighter than the declared one.**
Three live gates, none of which is the declared constant:

| Gate | Value | Where | Applies to |
|---|---|---|---|
| Total transaction size | **100 KB** (`config.max_transaction_size`) | default at `shared-protocol/src/config/settings.rs:186`, applied at `shared-protocol/src/mempool/pool.rs:292` | **every** transaction at mempool admission |
| Commit/reveal payload | 64 KiB (`MAX_COMMIT_REVEAL_PAYLOAD_BYTES`) | `shared-protocol/src/mempool/pool.rs:16`, applied at `shared-protocol/src/mempool/pool.rs:339` | `ContributionCommit`, `ContributionReveal`, `ForecastCommit`, `ForecastReveal` |
| Oracle attestation payload | 32 KiB (`MAX_ORACLE_ATTESTATION_PAYLOAD_BYTES`) | `shared-protocol/src/mempool/pool.rs:17`, applied at `shared-protocol/src/mempool/pool.rs:347` | `OracleAttestation` |

The first gate is the load-bearing one and it is easy to miss, because it does not mention `data`:
it measures `tx.size()`, which serializes with `serde_json::to_vec(self)`
(`shared-protocol/src/blockchain/transaction.rs:3781`) — the whole transaction, payload included.
At 100 KB it is about **20× tighter** than the 2 MiB the constant declares, so enforcing
`MAX_TRANSACTION_DATA_SIZE` as written would *loosen* the effective bound unless it were applied as
an additional, narrower check on `data` alone.

### Decision (2026-09-01, owner-ratified): the value stays at 2 MiB and the constant stays INERT

Re-measured on `main` `2b2b2ab97`: a transaction carrying a 2 MiB `data` payload is rejected by
`config.max_transaction_size` with *"Transaction exceeds maximum size"*. The declared ceiling is
never consulted. That was verified by putting such a transaction through admission, not inferred
from reading the caps.

**The constant is deliberately NOT re-pointed at 100 KB.** It belongs to the declared-ceilings
family (#2700), and a size limit becomes consensus-relevant the moment anything enforces it — so
silently changing its value would be an unratified parameter change dressed as a documentation fix.
Leaving it inert and clearly labelled is the safer state.

**It is also deliberately not enforced.** Any check against it at mempool admission would sit behind
a 20× tighter cap at the same call site and could never fire — a vacuous gate, which is the defect
#2700 exists to name, not a fix for it. Enforcing it where it *would* bind is the block-apply path,
and a node rejecting a block its peers accept forks; that is a consensus change requiring
ratification, not a lint-scoped edit.

What did happen instead: the constant's doc comment now states it is inert and names
`config.max_transaction_size` as the operative control, and a regression test pins that operative
limit at 100 KB so it cannot drift silently. **That test guards `config.max_transaction_size`, not
the constant** — the distinction matters, because a test named after the ceiling would be exactly
the false coverage this page warns about.

Sibling outcomes: `MEMPOOL_CAPACITY` was enforceable (nothing bounded the pool by count) and shipped
in #2989; `MAX_BLOCK_SIZE` was enforceable producer-side (nothing bounded what this node packs, the
transport limits being ingress-only) and shipped in #3001. `MAX_TRANSACTION_DATA_SIZE` is the one of
the three with no non-vacuous enforcement point.

Two things this does **not** cover, and they are the honest gaps:

- It is a **mempool** gate, not a block-validation gate. A transaction arriving inside a peer's
  block is not filtered by `add_transaction_with_signing_keys` and never meets this cap.
- It is a **config field**, so an operator can raise it. Nothing pins it to a protocol constant, and
  nothing warns when it is raised.

---

## `MEMPOOL_CAPACITY` — declared 50 000, enforced by nothing, and *the mempool is still bounded*

Declared at `shared-protocol/src/common/constants.rs:145`. Zero consumers.

**Read this section before concluding the mempool is unbounded.** It is not. The constant is dead,
but it shares a name with a live bound that is not it — which is exactly the trap that produces the
opposite error.

What actually bounds the mempool:

| Bound | Value | Where |
|---|---|---|
| Aggregate size, **in bytes** | `config.mempool_max_size_bytes`, default **50 MiB** | `shared-protocol/src/config/settings.rs:143`, wired at `shared-protocol/src/mempool/pool.rs:136` as `Mempool::max_size` |
| Over-capacity behaviour | **fee-rate-ordered eviction**, planned read-only then applied (#2555) | `plan_capacity_evictions` in `pool.rs` |
| Per-sender transaction count (#2039) | **1 024** | `shared-protocol/src/config/settings.rs:151`, applied at `shared-protocol/src/mempool/pool.rs:322` |
| Configured age horizon | **4 h, but not a live automatic bound** | `shared-protocol/src/config/settings.rs:139` feeds the shared `Mempool::max_age`; that `Mempool::clean_old_transactions` has no production caller. The operator-only `purge_older_than_secs` route defaults separately to 1 h (`native-chain/src/bin/launch_api/routes/operator.rs:42`). |
| Minimum fee rate | 0 — **floor disabled by default** | `shared-protocol/src/config/settings.rs:147` |

So the mempool is bounded by **bytes and by sender**, never by a global transaction count. The
implied count ceiling is `50 MiB ÷ minimum transaction size`, which is not 50 000 and is not a
number anyone chose.

**Do not "fix" this by enforcing the constant.** A hard `transactions.len() >= MEMPOOL_CAPACITY`
reject would bypass the fee-rate eviction planner, so a flood of cheap transactions could fill the
pool and lock out high-fee ones — strictly worse than today's behaviour. If a count bound is wanted,
it belongs *inside* the eviction planner, not in front of it. The likeliest correct action for this
constant is **deletion**, with `PROTOCOL_SPEC.md` corrected to describe the byte bound.

---

## Claims this change corrects

The first claim was present on the pre-change base. The companion threat-model claim has already
been corrected on current `main` by merged PR #2704.

- Pre-change `docs/security/PROTOCOL_SPEC.md` — *"Mempool capacity: 50 000 txs
  (`MEMPOOL_CAPACITY`), max block 32 MiB, max tx data 2 MiB."* All three figures describe constants
  that no code reads. The
  **same paragraph's gate list also had the last two gates reversed** — the intra-mempool
  double-spend check runs *before* capacity eviction, deliberately (#2555), so a rejected submission
  costs no eviction — and omitted three real gates (governance-proposal rejection, the minimum-fee
  floor, the per-sender count cap), plus the later-added non-transparent `Transfer` signature gate.
  Corrected in the same commit as this file.
- `docs/security/THREAT_MODEL.md` formerly credited `MAX_BLOCK_SIZE = 32 MiB` as a
  resource-exhaustion mitigation. Merged PR #2704 struck that claim before this refresh; it is not
  described as a current defect here. Its replacement sentence records the 1 MiB outer gossip cap
  but misses the newer/default 64 KiB `inline_data_max` gate; this ledger carries the narrower
  current-path result without reopening #2704's already-merged scope.

---

## Reproducing this

```bash
# 1. The three ceilings have no consumers (each command returns only its declaration):
rg --type rust '\bMAX_BLOCK_SIZE\b'
rg --type rust '\bMAX_TRANSACTION_DATA_SIZE\b'
rg --type rust '\bMEMPOOL_CAPACITY\b'

# 2. The one block-size comparison has no callers:
rg --type rust 'AttackPreventionSystem' --glob '!target'          # definition/imports/construction/tests; no stored-field call
rg --type rust 'SecurityManager|SecurityCoordinator' native-chain/src   # no hits

# The shared configured 4-hour cleaner likewise has no production caller:
rg -n 'clean_old_transactions\(' shared-protocol/src/mempool/pool.rs # definition + tests only

# 3. The transport bounds that are actually doing the work:
rg --type rust 'inline_data_max|MAX_GOSSIP_MESSAGE_BYTES|MAX_MESSAGE_BYTES|max_transmit_size|MAX_SYNC_MESSAGE_BYTES' p2p/src
```

Draft PR #2701 carries the deliberately-RED enforcement test. Draft PR #2711 wires it as an advisory
CI check (`.github/workflows/declared-ceiling-enforcement.yml`). Neither the test nor that workflow
is on `main` at `20c6fd00e`; the direct searches above are the current-tree evidence.

---

## If you are here to enforce one of these

Two current-tree constraints to settle **before** writing code:

1. **Enforcing `MAX_BLOCK_SIZE` is a consensus change.** `Blockchain::add_block` is reached on the
   peer block-acceptance path (`native-chain/src/bin/launch_api/p2p_events.rs:1023`). A size check
   there changes the block validity predicate: a node running it rejects a block its peers accept, and
   forks. It cannot be staged behind a feature flag, because two nodes with different flag values
   then disagree about the chain — the flag *is* the consensus rule.
2. **"Size" has no canonical definition yet.** A ceiling needs an encoding to measure against. The
   wire uses `bincode` (`p2p/src/sync.rs`), `estimate_block_size` uses a hand-rolled field count,
   and JSON is a third answer. Two nodes measuring differently disagree at the boundary. Name the
   encoding in `PROTOCOL_SPEC.md` before implementing anything.

Refs #2700, #2701, #2704, #2711.
