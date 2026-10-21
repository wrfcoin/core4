# ADR: permanent-state pricing must follow measured state ownership

**Status:** accepted for measurement; implementation deferred to follow-on.

## Context

`State` contains permanent account records, station/oracle configuration,
commit/reveal and attestation records, replay-protection sets, privacy records,
reward records, and governance/producer history. `native-chain::Storage` also
persists blocks, checkpoints, and WAL entries in redb. This is consensus and
operator state, but it is not one homogeneous, append-only byte store.

Weather/sensor data is especially important: reveal records and durable
contribution reward records are on-chain today, while their payload/provenance
may refer to off-chain data. Treating every weather write as a simple account
creation fee would price neither its actual retained state nor its deletion
semantics correctly.

## Decision

Do **not** add `StatePrice`, `StateDelta`, deposits, refunds, or a consensus
parameter in this story. No measured retained-byte baseline, ownership model,
or governance-approved parameter exists. Guessing any of them would create a
consensus economic rule without evidence.

First measure canonical state snapshots using
`scripts/measure_state_growth.py`. The script deliberately reports bytes, not
price: its sorted, compact JSON representation is a stable measurement format,
not a proposed consensus encoding.

## Store classification for the follow-on

| Class | Current examples | Pricing/release question |
| --- | --- | --- |
| User-owned and releasable | account records, user-created commitments where a valid spend/remove path exists | Deposit may be refundable only after an authenticated deletion proves the state was released. |
| Protocol replay/safety | spent nullifiers, key images, oracle nonces, slashing evidence, producer history | Never refund merely because a user asks; pruning/retention must preserve replay and fork safety. |
| Weather/contribution | pending commits/reveals, attestations, station registry, durable reward decisions | Determine which records are prunable, which must become off-chain commitments, and who pays before charging stations. |
| System/history | block table, checkpoints, WAL, governance history | These are not automatically payer-owned; a policy must state whether they are funded by fees, treasury, or explicit rent. |

The follow-on must select one accounting boundary and apply it identically on
producer, validator, P2P replay, WAL replay, snapshot, and state-root paths.
It must not charge serialized redb bytes: encryption and database overhead are
node-local and would fork consensus. It must define deterministic logical byte
accounting, maximum delta/overflow handling, an owner/payer for every charged
record, and exact removal authorization.

## Supply-conservation requirement

A refundable deposit is a balance movement, not a mint. Charging must debit a
payer and credit an explicit state-deposit escrow in the same deterministic
transition; refunding must debit that escrow and credit the entitled payer.
The supply-conservation invariant must include every new balance location and
reject underfunded charges, duplicate refunds, negative/overflowing deltas, and
unrecognized releases. Burning, if governance later selects it, needs a
separate explicitly-accounted sink. Current `total_minted_supply()` only sums
account balances, so the follow-on must either keep escrow as an account balance
or revise the invariant and all callers together.

## Migration posture

No retroactive debt is charged to existing state. A future activation must be
height/version-gated, preserve legacy records under explicitly documented
rules, and make new records carry enough deterministic ownership/size metadata
to support later release. It must include a snapshot/WAL migration and a
rollback plan. Environment variables and local storage measurements are not
valid consensus activation inputs.

## Measurement protocol

Export ordered `(height, state)` snapshots from a node/storage-aware exporter,
then run:

```bash
python3 scripts/measure_state_growth.py --input snapshots.json --format markdown
```

The input is a JSON array or NDJSON of objects with a non-negative integer
`height` and JSON-object `state`. The report gives canonical UTF-8 snapshot
bytes at each height. The first row is a baseline, so its change columns are
null (JSON) or `—` (Markdown), not a fictitious delta from zero. Later rows
report (1) net whole-snapshot bytes and (2) gross top-level member bytes added
and removed. A changed member counts once as removal of its previous canonical
`"key":value` encoding and once as addition of its new encoding; braces and
commas are deliberately excluded from that structural attribution. Therefore
gross member churn is not interchangeable with whole-snapshot net growth.

It fails closed on missing, duplicate, descending, or non-object follow-on
states and does not claim that serialized block bytes equal permanent state
growth.

## Consequences and residual

This ADR prevents invented state-byte pricing but leaves permanent-state bloat
unpriced. No local real block/snapshot dataset is available in this worktree,
and N5 is outside this assignment, so the new tool is self-tested only here.
The follow-on must run it against exported real block/state data and attach the
artifact before proposing parameters or consensus code.

**Follow-on:** [#2609](https://github.com/wrfcoin/core4/issues/2609) owns the
consensus-wide logical-byte schema, weather retention/off-chain commitment
policy, escrow/refund lifecycle, supply invariant, migration, and production
dataset.
