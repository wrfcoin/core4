# ADR-003: Role-based earnings — partitioning block emission across the roles that run the network

**Status**: Proposed (owner-commissioned, #2843)
**Date**: 2026-08-30
**Decision owner**: Core4 consensus / tokenomics
**Depends on**: ADR-002 (emission schedule), #2220 (producer authorization), #2809 (BFT rollout architecture), #841 (reachability receipts), #843 (snapshot-join)

## Context

On testnet 777 exactly two wallets earn WRF: the weather stations, through
Proof-of-Publishing contribution rewards. The three nodes that carry the
network — producing blocks, attesting contribution quality with hardware
keys, serving sync and (soon) snapshot bootstrap — earn nothing.

That is not a testnet cosmetic. Nobody operates infrastructure they cannot
earn from, token utility cannot be exercised by participants who cannot
acquire tokens through participation, and a mainnet launched with
contribution-only earnings would ask node operators to subsidize everyone
else. The earning surface must cover every role that does verifiable work —
**before** outside operators are invited, because retrofitting reward
consensus onto a running network is a hard fork per role.

Two constraints frame everything below:

1. **ADR-002 is untouched.** The per-block emission (800 WRF initial, 2/3
   retention per 126,000,000 schedule-seconds, 21,000,000 cap, integer-floor
   arithmetic) is a genesis-cut consensus decision. Role earnings PARTITION
   that emission; they never add to it. The supply identity test remains the
   supreme court.
2. **Reward calculation is consensus code.** Every rule here must be
   deterministic, integer-only, and replay-identical (the #1776 divergence
   and the state-root v6 sorting rules are the cautionary history).

## The roles, defined by what is verifiable about them

The definitions are ordered by trust weight, not prestige. A role earns
if and only if it performs a **scarce, verifiable, on-chain-provable act**;
a role posts stake if and only if it holds **censorship power** — the
ability to stop *others* from earning.

### 1. Contributor (weather station) — the economy's purpose

Submits committed-then-revealed weather observations; quality is attested
by the oracle quorum against reference bands; rewards carry
`provenance_class=real`. **Unchanged by this ADR** except that its share of
block emission becomes an explicit budget instead of an implicit 100%.

- *Verifiable act*: quorum-attested reveal, already on chain.
- *Censorship power*: none. No stake required.
- *Sybil exposure*: bounded by physical-station economics and attestation.

### 2. Attester / quorum holder — the trust root

An oracle-set member signing quality attestations with a hardware-held key.
"Quorum holder" is **not** a separate role: it is an attester whose commit
**lands inside the quorum that finalizes a decision**. That distinction is
the payment rule.

- *Verifiable act*: an attestation commit included in a finalized quorum.
  NOTE (design review, 2026-08-30): today's chain records only the COUNT of
  quorum commits per block (`oracle_reward_quorum_commit_count`), not the
  attester identities or per-decision allocations — Phase A must add the
  normative settlement record defined in "Attester settlement" below; the
  count alone cannot pay anyone deterministically.
- **Pay per landed quorum commit, never per set membership.** Membership
  payment rewards being on a list; landed-commit payment rewards *liveness*
  — precisely what the network has lost twice (the 08-28 usbipd outage,
  the near-miss this week). An attester whose YubiKey sits detached earns
  nothing, and the operator notices in the wallet before the dashboard.
- *Censorship power*: **maximal.** A starved quorum halts everyone's reward
  finalization. Therefore: an attester **bond** (staked WRF, locked while in
  the set), slashable on provable misbehavior — equivocation (two
  conflicting commits for one decision; the evidence store exists) or
  attestation provably outside the reference bands (the ≤0.5 physics-only
  comparison gives an objective floor). Sustained non-liveness is not
  slashed — it is already unpaid — but governance may rotate a persistently
  dead attester out (the #2712 rotation machinery).
- *Sybil exposure*: none in practice — set admission is genesis/governance
  content (#2464/#2712) plus a hardware key ceremony.

### 3. Producer — the block assembler

Today the collector drives `/blocks/mine`; #2220 (the next consensus
sprint) makes production an authorized, signed act. The producer reward
set IS the #2220 authorization set — one design, one sprint.

- *Verifiable act*: an accepted block signed by an authorized producer key.
- *Pay per accepted block*: a fixed producer slice of that block's emission
  (integer, from the partition below) plus transaction priority fees.
  Nothing is paid for orphaned or rejected blocks.
- *Censorship power*: real (transaction exclusion, reveal-window gaming).
  Producer stake + slashing on equivocation evidence; the deferred-reveal
  machinery already bounds reveal-window abuse.
- *Sybil exposure*: none — the set is authorized, not open, until a later
  ADR opens production (that would be a PoS transition decision, out of
  scope here).

### 4. Node — the service provider

Maintains full state and serves others: sync ranges to lagging peers,
snapshot-join bootstraps to new nodes (#843 — the only entry path across
the unreplayable 11,017 segment). This is the role the three current nodes
perform unpaid.

- *Verifiable act*: a **service receipt** — the #841 pattern generalized.
  The consumer of the service (the syncing/bootstrapping node) signs an
  Ed25519 receipt naming the server's peer ID, the byte range or snapshot
  manifest hash served, its own peer ID, and the chain height; the server
  submits batched receipts on chain for settlement from the node-service
  budget. Verification is signature + freshness + dedup — all cheap and
  deterministic.
- *Anti-self-dealing*: receipts from a peer ID bound to the same account,
  or from unstaked throwaway consumers, count zero. A **minimum node stake**
  makes receipt-farming rings cost more than they yield; per-epoch,
  per-server caps bound the damage of any residual ring.
- *Censorship power*: low (a node refusing service loses income, not
  others'). Stake floor only; no slashing beyond receipt fraud (submitting
  a forged receipt is slashable — the signature makes fraud provable).
- *Sybil exposure*: the binding constraint of this role; the stake floor,
  consumer-stake requirement, and caps are the mitigations, and the
  budget slice is deliberately the smallest.

### 5. Peer — connectivity, deliberately unpaid

Anyone speaking the P2P protocol, relaying gossip. **Peers do not earn.**
Relay work is unverifiable-in-principle (any claimed relay can be
fabricated between colluding peers at zero cost) and paying it converts
the gossip mesh into a sybil market — the disease that killed every
"proof of bandwidth" scheme. Peers benefit through access; peers that also
*serve* (role 4) earn as nodes.

## The partition

Each block's emission `E(t)` (from ADR-002, unchanged — normatively, the
existing iterative-floor schedule function in
`shared-protocol/src/consensus/proof_of_publishing.rs`) is split by
integer arithmetic, largest-remainder rounding to keep the sum exactly
`E(t)`:

| Role | Mainnet share | Testnet share | Rationale |
|---|---|---|---|
| Contributions (stations) | **70%** | **69%** | The data is the product; its dominance is the economy's identity. |
| Attesters | **15%** | **15%** | Highest trust, hardware cost, bonded capital, liveness-paid. |
| Producers | **10%** | **10%** | Per-block work, plus they keep priority fees on top. |
| Node service | **5%** | **5%** | Real but low-scarcity work; smallest slice bounds the sybil surface. |
| **Faucet** | **0%** | **1%** | Testnet only. Funds onboarding without a genesis allocation. Carved from contributions, so no other role's share moves. |

Both columns sum to exactly 10000 bps. The faucet is a role with a **zero
share on mainnet**, not a role that is absent from mainnet: a zero slice mints
zero under the fail-closed rule below, so one partition shape serves both
networks and the mainnet arithmetic is unchanged.

**The exact arithmetic (consensus-normative, per design review 2026-08-30):**
shares are consensus constants in basis points — mainnet `7_000 / 1_500 /
1_000 / 500 / 0`, testnet `6_900 / 1_500 / 1_000 / 500 / 100`, in the fixed
role order contribution → attester → producer → node → faucet. For each role `r`, `slice_r = floor(E(t) * share_bps_r / 10_000)`.
The remainder `R = E(t) - Σ slice_r` (an integer, `0 ≤ R ≤ 3`) is assigned
one base unit at a time in descending order of fractional remainder
`(E(t) * share_bps_r) mod 10_000`; ties break by the **fixed role order
`contribution → attester → producer → node → faucet`** (the table order
above — **all five roles**, faucet included; an ordering that omits a role
that can appear in a tie leaves the allocation undefined),
implemented as an ordered array — never map or iteration order. Required
test vectors: a remainder tie proving the faucet's **presence** in the order —
not its position, which provably cannot matter. Because the faucet is **last**
in the order, a stable sort places it last among equal remainders, so if a tie
is cut the faucet is always the role dropped, and if a tie is not cut every tied
role is paid and the order is irrelevant. **The faucet's position can therefore
never decide its own award**, at any `E(t)`; a requirement phrased in terms of
position is unsatisfiable. What must be tested is that the faucet is in the
array at all, stated as a with/without comparison. The witness is `E(t) = 75` on
the testnet row:

| | remainders (contribution, attester, producer, node, faucet) | leftover | units awarded |
|---|---|---|---|
| five-role order | `7500, 2500, 5000, 7500, 7500` | 3 | contribution, node, **faucet** |
| faucet omitted | `7500, 2500, 5000, 7500` | 3 | contribution, node, **producer** |

Omitting the faucet from the order does not leave the allocation merely
underspecified at this input — it pays **a different role**. A test that pins
the five-role result at `E(t) = 75` therefore fails if the faucet is dropped
from `EMISSION_ROLE_ORDER`, which is the property the vector exists to enforce.
Note this is a presence test, and a presence test is sufficient: since position
cannot matter for the last role, presence is the whole of what a tie vector can
check about the faucet.

Also `E(t) = 0`, the terminal-era small-`E(t)`
region, and the exact-sum identity on every vector.

Rules that are architecture, not tuning:

- **Fail-closed budgets** (the #940/#994 pattern): each role's slice is a
  per-block ceiling. Unclaimed contribution budget in a block (no reveals
  finalized) is **not emitted** — the schedule's realized issuance may run
  under ADR-002's curve, never over. No slice borrows from another; that
  keeps every slice's incentive analysis independent.
- **One partition ledger authorizes every mint** (design review 2026-08-30):
  a single per-block partition ledger, derived from `E(t)` by the arithmetic
  above, is the ONLY authorization for emission-drawing mints. Every
  existing emission path — coinbase, gamification achievement mints,
  contribution rewards (which today budget from `MAX_BLOCK_EMISSION` minus
  coinbase/gamification, an independent ceiling) — draws from the
  contribution slice through this ledger; attester/producer/node mints draw
  only from their own slices. Two ceilings that cannot see each other is
  exactly how a chain over-mints while every local test passes.
- **The percentages are an owner dial; the partition mechanism is
  consensus.** Changing a ratio is a hard fork like any reward change (per
  ADR-002's closing rule), so the appendix ratios must be settled before
  the genesis cut that ships this.
- **All stake/bond amounts, slash fractions, and epoch lengths live in the
  owner-decision appendix** (below), not in this section — they are
  parameters, and pretending otherwise would freeze guesses as
  architecture.

## Attester settlement (Phase A normative rules, added by design review 2026-08-30)

The per-block count `oracle_reward_quorum_commit_count` is evidence that
quorums happened, not a payment instruction: it carries no attester
identities, no per-decision boundaries, and no eligible cardinality. Phase A
therefore adds a typed, block-hashed **attester settlement record**, one per
finalizing decision, and these rules:

1. **The threshold-crossing block owns the settlement duty.** The block in
   which a decision's quorum threshold is crossed is the block that owes that
   decision's settlement record, and normally emits it and pays it from that
   block's attester slice. The obligation is **durable**: if the crossing block
   does not pay it — the producer omitted the settlement metadata, or the
   allocation did not fit that block's remaining attester slice — the debt
   persists and a later block MAY discharge it. Rule 4's terminal marker, not
   the block boundary, is what guarantees a decision is paid **at most once**.
   A decision is never paid twice, and a decision that crossed quorum is never
   permanently stranded because one producer misbehaved.
2. **Record contents**: decision/commit ID, finalization height, the
   canonically sorted eligible attester identities, and the exact integer
   allocation per attester. Producer, P2P applier, and WAL replay all
   consume this same typed record — settlement is replayed, never recomputed
   from local timing.
3. **Eligibility**: the canonical attestation commits for the decision
   present in state at the end of the threshold-crossing block — one per
   attester identity (the first canonical vote; a replacement or repeat vote
   is a no-op). ALL such commits are eligible, not just the first K: paying
   every pre-threshold voter rewards liveness beyond the minimum. Votes
   landing in later blocks are **unpaid**.
4. **Terminal marker**: settling a decision persists a decision-ID terminal
   marker. Any later vote, re-attestation, or late-quorum event for a marked
   decision is a payment no-op (the #2460/#2508 late-settlement machinery
   retries CONTRIBUTION rewards only; it never creates a second attester
   payment).
5. **Division**: a block's attester slice divides equally per eligible commit
   across ALL decisions settling in that block: `per_commit =
   floor(slice / total_eligible_commits)`, remainder assigned one unit at a
   time in ascending (decision ID, attester ID) order. Zero settling
   decisions ⇒ the entire attester slice is unclaimed and not emitted.
6. **Required tests**: initial quorum, late quorum (straggler in a later
   block), multiple decisions settling in one block, more-than-K votes,
   repeated/replacement attestations, and producer/applier/WAL parity on all
   of the above.

### Why the duty is durable (amended 2026-08-31)

The original rule said, “No other block ever pays that decision,” because the
block boundary was intended to enforce at-most-once payment. Read literally,
preserving the debt would instead require rejecting a crossing block that
omitted or could not fit settlement, turning one misbehaving producer into a
follower halt. Rule 4's decision-ID terminal marker independently prevents a
second payment, so a later block can safely discharge unpaid debt. PRs #2852
and #2855 implement this durable-duty route.

## Interaction with #2809 (BFT rollout)

Attester bonds change the BFT rollout arithmetic: a bonded attester set
has measurable economic weight, so #2809's inertia baseline and
fleet-atomic activation must treat bond changes as set changes. The
rotation path (#2712) must release bonds only after the departing
attester's last attestations pass their challenge window. ADR-003 defers
to #2809 on activation sequencing; #2809 should reference this ADR's bond
lifecycle when it revises.

## Sequencing (each phase is a separately gated consensus change)

1. **Phase A — attester rewards.** Smallest surface, but not zero new
   machinery (corrected by design review 2026-08-30): the QUORUM EVENTS are
   already recorded; the attester settlement record, terminal markers, and
   partition ledger above are new consensus state Phase A adds. Bond/slash
   still ships one cut later than payment — payment-first gets the liveness
   incentive live immediately — under the following owner-approved,
   **testnet-777-only unbonded interim exception** (granted 2026-08-30):
   - the attester set is the frozen genesis allowlist; no set changes until
     the bonded cut;
   - one canonical vote per identity per decision (rule 3 above);
   - post-threshold votes are excluded from payment (rule 3 above);
   - **Phase B's genesis cut ships the bond and removes this exception** —
     it is named here so the exception cannot silently outlive its phase;
   - a mainnet genesis requires the attester bond from block 0; this
     exception never applies off testnet 777.
2. **Phase B — producer rewards.** Rides #2220 exactly: the same cut that
   makes production authorized makes it paid. Equivocation slashing wires
   the existing evidence store to the bond.
3. **Phase C — node-service receipts.** After #843 lands (snapshot-join is
   the service worth paying first), generalize #841's receipt pattern to
   service receipts, settle per epoch from the node budget.
4. **Phase D — testnet validation gate.** All three infra roles earning on
   testnet 777, exercised by the three existing nodes, before any outside
   operator invitation. The dashboard grows per-role earning panels
   (auth-carrying — see the 08-30 quorum-panel lesson).

Because every phase changes reward consensus, each is a fresh-genesis-cut
class change on testnet (the #2034 precedent) — sequenced with, not inside,
the mesh-cut release process.

## Alternatives rejected

- **Pay peers for relay/uptime**: unverifiable work; converts gossip into a
  sybil market. Rejected on principle, not parameters.
- **Attester payment per set membership**: rewards list-presence, not
  liveness; would have paid the detached-YubiKey attester through both
  quorum outages. Rejected.
- **Separate emission for infra roles**: violates ADR-002's cap and
  schedule; rejected without analysis — the cap is a genesis promise.
- **Fee-only infra earnings (no emission slice)**: at current testnet
  volumes fees are ~zero; infra roles would earn nothing for years, which
  is the status quo this ADR exists to end. Revisit at fee maturity.

## Owner-decision appendix (RATIFIED 2026-08-30)

On 2026-08-30, the owner ratified all nine parameters below as proposed, with
no changes. The Proposed default column records the ratified values, and the
Notes column preserves their rationale.

The partition ratios, including the coinbase/contribution reward sub-split,
are now consensus-frozen. Per this ADR's owner-dial rule and ADR-002's closing
rule, changing a ratio after this ratification is a hard fork like any reward
change. In particular, changing the sub-split after the genesis cut is a hard
fork.

The ratified ratios agree with the Phase A implementation in core4 #2851,
which represents the partition as basis points `7_000 / 1_500 / 1_000 / 500`.
In Gina's Phase A work, `COINBASE_CONTRIBUTION_SHARE_BPS = 2_500`, and
`coinbase_allowance(height)` is the single source of truth for that allowance.

This ratification settles the parameters only. It does **not** authorize the
genesis cut, which remains owner-only and separately gated.

| Parameter | Proposed default | Notes |
|---|---|---|
| Partition ratios | 70 / 15 / 10 / 5 | Percent of E(t); largest-remainder rounding. |
| Coinbase / contribution reward sub-split | `COINBASE_CONTRIBUTION_SHARE_BPS = 2_500` (25 / 75) | Preserves the pre-ADR-003 genesis relationship: coinbase drew E and contribution rewards drew 3E, so Phase A changes total block emission without changing how these paths share their slice. |
| Attester bond | 2,000 WRF | ~2.5 blocks of initial emission; sized to hurt, not to exclude. |
| Attester slash fraction | 50% equivocation; 10% out-of-band attestation | Equivocation is unambiguous; band violations get a challenge window. |
| Producer stake | 1,000 WRF | Lower than attester — narrower censorship surface. |
| Node stake floor | 250 WRF | High enough to price out receipt rings with the caps below. |
| Node per-epoch receipt cap | 20% of node budget per server identity | Bounds any residual ring. |
| Epoch length | 1,024 blocks | Settlement batch for service receipts. |
| Challenge window | 4,096 blocks | For attestation/receipt fraud proofs before bond release. |

## Amendment 2026-10-11: bonded Node service (Refs #2848)

This amendment supersedes the Node cap, witness and testnet challenge terms
above and in #2848. Owner decisions: full bonded service before earning;
250 tWRF collateral shared by serve and consume roles; equal eligible-node
split with no percentage cap; ONE distinct non-self registered bonded consumer
per serving deployment per epoch. Separate accounts under one human owner are
not independent beneficial owners; the surviving-pair policy accepts collusion
risk. Demand includes scheduled real snapshot restore rehearsals into isolated
fresh receiver databases. Dry runs, fabricated receipts and production database
replacement do not qualify. No liveness slashing: no work means no earnings.

Epoch length is 1,024 blocks; the Node-service challenge window is **1,024
blocks on testnet, 4,096 on mainnet**. Evidence is accepted through the inclusive
close + challenge deadline; redemption is strictly later. Exit collateral stays
liable through outstanding service windows and the withdrawal buffer. These
Node terms do not shorten unrelated attestation challenge windows. Proven
attributable fraud forfeits 100% of remaining responsible collateral to an
immutable non-spendable protocol account and jails that deployment. Supply
continues to count forfeited balances; no reporter mint bounty or burn.

LEAD selected unminted per-block Node reservations with delayed epoch redemption
(option A). Reserves are authorization, not minted balances or earned totals;
all mint consumers must deduct outstanding reserves from supply headroom. Empty
epoch reserves are released unissued. Finalization derives close/maturity effects
once in the common producer/applier/WAL fold, never from submitted settlement
metadata. Equal integer splitting rotates remainder recipients by epoch.
Resource defaults: 32 registrations (including outstanding exiting/jailed
liabilities), 16 KiB batch, eight claims, 1,536 bytes per claim, 128-byte alias,
and 8 KiB two-claim fraud evidence. These bounds do not impose an earnings cap.

S1 preserves `SignedNodeServiceReceipt` v1 unchanged. New V2-framed domains bind
server tickets to exact signed receipts/manifests, genesis, chain/network,
epoch-start parent and frozen roster; consumers endorse the ticket digest.
Owners authorize bond lifecycle intents; dedicated online keys prove possession
with separate role-bound signatures. Those signatures establish attribution,
not actual service or snapshot canonicality. S2/S3 must resolve accounts and
frozen registry bindings, validate export signatures and enforce inclusion.

The S1 commitment grammar is explicit and independent of transport/serde:
`LP(x)` is u64 little-endian byte length followed by x; all u64/i64 integers
are fixed little-endian. Receipt commitment is SHA3-256 of
`LP("wrfcoin.node_service_receipt.v1") || LP(receipt canonical body) || LP(signature bytes)`.
The preserved receipt body order is LP(service snake-case name), LP(server alias),
LP(consumer public key), chain height, LP(subject hash), units, timestamp,
LP(nonce). Signed manifest commitment is SHA3-256 of
`LP("wrfcoin.node_snapshot_manifest_commitment.v1")`, LP(signer ID UTF-8),
timestamp, LP(nonce UTF-8), height, LP(state-root hex UTF-8),
LP(snapshot-root hex UTF-8), entry count, LP(tip-hash hex UTF-8), finalized
height, LP(export signature hex UTF-8), in that exact order. Strings commit to
exact UTF-8 spellings; no implicit hex decoding or normalization. Future
export fields require an explicit versioned protocol decision to participate;
serde changes alone cannot change this grammar. Bincode remains transport only.
Four signing goldens are generated from an independent reference encoder that
uses neither BodyBuilder nor production preimage/body helpers, and executed
checks require production bytes to match that reference.

Named S3 acceptance items (required before eligibility or activation):

- **S3-SERVER-ALIAS-KEY:** resolve the receipt server alias and ticket deployment
  IDs in the frozen epoch roster to the supplied server/consumer keys; reject
  aliases resolving to another deployment/key, including self aliases.
- **S3-MANIFEST-EXPORT-SIGNATURE:** verify the manifest's own export signature
  under the approved export signing context and frozen trusted exporter key;
  reject invalid/untrusted exports even with valid outer endorsements.
- **S3-TICKET-REPLAY-HEIGHTS:** enforce ticket-ID uniqueness across batches,
  blocks and restart/WAL replay, validate ticket/receipt/manifest heights against
  the epoch window and canonical inclusion, and reject reused tickets.
- **S3-OWNER-KEY-ACCOUNT:** resolve the offline owner key to the authorized
  account and frozen registration before lifecycle execution; reject a valid
  signature from a key that does not control the named owner account.

S1 signature verification does not satisfy these items. S3 must demonstrate
positive and negative cases through the production admission/execution path.
The mutation evidence script may edit production files only in an owned,
isolated worktree; it must restore them byte-for-byte before exit.

`node_share_settlement` remains unscheduled and non-schedulable.
Genesis resolution also refuses it. S1 added no transaction variant, state/root
field, WAL/checkpoint layout, mint path or default/bootstrap activation.
Scheduling remains refused until the complete reviewed implementation, supply,
fraud, migration and production parity slices land. This amendment authorizes
neither scheduling nor deployment, signing ceremonies, funding or GO.

### S2 withdrawal boundary — owner-ratified 2026-10-11

The owner ratified `NODE_BOND_WITHDRAWAL_BUFFER_BLOCKS = 100` on
2026-10-11. This is a named consensus constant, never an inline lifecycle
parameter. On testnet the challenge window is 1,024 blocks. Let `C_last`
be the close height of the last registered/eligible service epoch, specifically
the epoch containing the deployment's exit effective height. The principal
remains locked and slashable through `C_last + challenge_window` inclusive.
Evidence included at that deadline still slashes. Earliest owner withdrawal
is `max(C_last + challenge_window + NODE_BOND_WITHDRAWAL_BUFFER_BLOCKS + 1,
exit_effective_height + 1)`. The extra buffer delays withdrawal; it does not
extend the fraud inclusion deadline. Historical epoch bindings remain available
through their challenge windows. Slice 5 supplies actual fraud confiscation.

This policy is owner-ratified, rather than LEAD-provisional. The feature stays
unscheduled, default OFF and non-schedulable until all six slices pass review.

Lifecycle fees inherit the general fee model; a consensus fee debit, if ever
added, applies to all kinds together. S2 adds no consensus fee debit or burn.
Mempool admission uses the existing signature-count/bytes pricing policy.
`MAX_NODE_BOND_LIFECYCLE_TXS_PER_BLOCK = 16` bounds lifecycle transactions,
alongside the real 250 tWRF bond and one registration per identity. The common
verification-work admission price defaults to zero for every transaction kind;
when configured it uses `minimum_fee_for`, alongside the existing per-byte
floor. Those admission checks neither debit nor burn balances.

S2 makes registration and funding one atomic `NodeBondLifecycle` Deposit:
`250 * WRF_UNITS_PER_COIN` (25,000,000,000 testnet base units) moves from the
proved owner's spendable balance into a fresh protocol escrow account, fully
locked. Update and Exit take effect next epoch; the owner authorizes offline
Exit/Withdraw without online-key or governance cooperation. Deposit/Update
also require allowlisted governance approval and payout-key possession over
the existing explicit authorization subject. Online keys cannot alias another
registration's owner, payout or escrow address in either direction, including
retained historical bindings. The shared legacy-transition
guard prevents any generic transition, including recovery, from changing an
escrow account or removing its bonded owner account. Only the named lifecycle
transition can release principal in this slice. Lifecycle wallet authorization
also resolves the existing account policy: SignerIsSender is supported; an
in-force unsupported declared policy is refused rather than bypassed by the
wallet key. This uses the same height and unplumbed-time policy seam as current
debits. Fraud confiscation remains S5.

At activation the common block context writes the durable marker even for an
empty registry. Pre-activation roots retain v14 material; the marker selects
v15 material independently of height, process configuration or root hashing.
Checkpoint format 12 appends that optional field to frozen format 11; OFF
writers retain format 10/11 bytes. OFF snapshot exports retain format 2 and
omit the absent field; activated format 3 carries every consensus field.
Imports validate the marker against the anchor and refuse post-activation
legacy state rather than fabricate an activation marker. WAL replay validates
the epoch parent input against its archived block before applying it.

## Evidence and follow-up

- Implementation phases file as separate issues under #2843.
- The partition arithmetic lands in `shared-protocol` next to ADR-002's
  schedule (same crate). The extended supply-bound test must prove more than
  the helper identity (design review 2026-08-30): (a) the sum of slices
  equals `E(t)` exactly per block; (b) each realized role mint is at most
  its slice; (c) total realized role mints — including the existing
  coinbase/gamification/contribution paths routed through the partition
  ledger — are at most `E(t)`; (d) unclaimed slices mint zero; (e) all of
  the above hold across a reduction boundary and at `E(t) = 0`; (f)
  producer, P2P applier, and WAL replay agree. The ADR-002 cumulative
  supply bound remains the outer invariant.
- This ADR reconciles with, and must be cross-referenced from, #2809's
  next revision.

---

## Ratifications 2026-09-01

### The three Phase A ambiguities — ratified as (a), (a), (a)

All three resolve to **what the code already does**, so this is three sentences
of specification and zero code. The baseline was pinned to file and line on
`main` @ `0096a9aef` before the decision was taken; anything other than (a)
would have been a consensus change that had to land before the cut.

1. **Settlement height.** The settlement record's `finalization_height` is the
   height of the block that **pays** the debt, not the height at which quorum
   was crossed. Under the durable duty, a debt discharged at `h+k` carries
   `h+k`. (`proof_of_publishing.rs:3861`; the apply arm rejects any record whose
   `finalization_height` differs from the ledger height of the block being
   applied, `state.rs:3605-3611`.)

2. **Floor-division zero.** When `1 ≤ slice < total_eligible_commits`,
   `per_commit = floor(slice / commits)` is zero: the record is built with zero
   allocations, the apply arm skips paying them, and it still clears the debt
   and writes the decision's terminal marker. Those attesters are permanently
   unpaid for that decision. (`state.rs:4891`, `:3679`, `:3728`.)

   This is a **silent forfeiture**, and it is ratified with that named. It is
   reachable only when the attester slice has decayed below the eligible commit
   count, which is more than a century out on the ADR-002 schedule, and it
   forfeits at most one base unit per attester. The alternative — paying one
   base unit ascending until the slice is exhausted — was rejected because it
   leaves the decision **permanently unsettled** when the slice is zero, and a
   stranded decision is worse than a rounding loss.

3. **Registry-rejected votes.** A second vote from an identity that has already
   voted on a decision is returned `accepted: false` before `set.add` runs, so
   it never enters `AttestationSet::by_oracle`; eligibility reads `by_oracle`
   filtered to `approved`. A rejected vote is therefore not eligible, and the
   first accepted vote per identity is canonical. A flipped vote additionally
   raises `DoubleAttestation` slashing. (`attestation.rs:528`,
   `state.rs:4757-4764`.) Wording only.

### The testnet faucet slice — 100 bps, gated on GENESIS content

The faucet exists so the testnet can onboard participants without a genesis
allocation. It must never mint on mainnet.

**The gate is the chain id recorded in genesis (`GENESIS_CHAIN_ID_KEY`), and
the testnet value is the string `"777"`** — confirmed against the deployed
chain, whose genesis header records
`{"chain_height":1,"chain_id":"777","network_id":"testnet", ...}`
(`infra/testnet/cut-pins.env`) and whose deployment sets
`WRFCOIN_CHAIN_ID=777` (`infra/config/testnet.env.example:23`). Note it is a
**string**, not a number: core4's chain id is `&str` throughout
(`DEFAULT_CHAIN_ID = "wrfcoin-main"`), so the comparison is `== "777"`.

**The gate is genesis content, not the runtime chain id.** This distinction is normative, not stylistic:
`signing_chain_id()` resolves `WRFCOIN_CHAIN_ID` from the environment and
**falls back to `wrfcoin-main` when the variable is unset** (`domain_signing.rs`;
the fail-closed work in #2713 exists because of exactly this default). If the
partition were selected from that value, a node that lost its environment would
compute a different `E(t)` split from its peers — a different state root on
every block, from a configuration mistake rather than a protocol disagreement.
Genesis content is agreed by every node on the chain by construction and cannot
be changed by a misconfigured process, which is the same reason the oracle set
is genesis content.

Consequences that follow, and which the implementation must satisfy:

- `EmissionPartition` selection becomes a function of chain identity, so
  `from_emission` / `at_height` need the genesis chain id threaded to them, at
  all three parity surfaces (producer `create_next_block`, applier
  `extract_transitions`, and WAL replay) — these must agree or the #2846 parity
  tests fail by design.
- The faucet slice obeys every existing partition rule: it is bounded by the
  ledger, an unclaimed slice mints **zero**, and the shares still sum to exactly
  `10_000` so the largest-remainder bound stays `EMISSION_ROLE_COUNT - 1`.
- A mainnet genesis must produce a faucet slice of exactly zero, and that is a
  required test, not an inference from the constant.

### Bonded Node Slice 3 — receipt inclusion (2026-10-11)

Extends the preserved v1 SnapshotJoin receipt and S1 service ticket / settlement endorsement. Consensus verifies the frozen S2 server and consumer identities, different deployments AND different owning accounts, dedicated keys and real locked principal, canonical epoch-start parent, roster sequence/hash, signed export signer, delivered height, manifest height/root/entry count, positive units, inner receipt and both outer service signatures. Delivery and inclusion are in the same service epoch. Signed timestamps remain informational; no local clock or expiring HTTP acknowledgement enters eligibility. SyncRange remains refused. At least ONE distinct consumer is sufficient, owner-ratified; repeated tickets or key rotations never add a consumer deployment.

The actual verified fresh-receiver import writes the signed legacy receipt and manifest into a durable redb journal after atomic installation. A distinct registered consumer BlockSigner endorses a server ticket only when the exact installed receipt/manifest journal exists. Dry runs never create that journal. The server signer is the registered export key; the receiver signer is the registered consumer key. Signatures authenticate claims and parties, not independent proof of fetching or a globally canonical historical root; two bonded accounts under common human control can collude.

The snapshot CLI's --bonded path requires an actual import and --receipt-archive. Its dedicated bonded-ingest endpoint is gated by node_share_settlement and bounds the signed snapshot plus required block window to 2 MiB; the ordinary ingest endpoint retains its existing 64 KiB JSON budget. It archives before acknowledgement, obtains the server ticket and local receiver endorsement, then archives the complete public claim. An offline owner authorizes the exact canonical bounded batch and account nonce under wrfcoin.node_receipt_submission.v1. The submit-receipt command sends that signed transaction to common mempool admission; admission is explicitly distinct from inclusion. The submission endpoint verifies funded registered owner/claim bindings before consuming durable journal capacity. Larger future snapshot envelopes exceeding its 2 MiB ingress budget are explicitly unavailable through this endpoint and require capacity/recovery validation before activation. Failed inclusion retries the archived exact transaction. This adds no consensus fee debit or burn: lifecycle fees inherit the general fee model; a consensus fee debit, if ever added, applies to all kinds together. Receipt submissions likewise inherit existing fee and account-policy behavior.

Bounds: 16 KiB entire binary submission including authorization; 8 claims, 1,536 bytes each; 16 batches/block; 8 retained tickets per stable (epoch, server, consumer) pair. Maximum 32 nodes and 3 retained epochs bound records to 23,808; full claim transport is at most 36,569,088 bytes before fixed map/height overhead. The maximal serialized receipt-store shape (23,808 exact 1,536-byte claims, map keys and inclusion heights) is 37,521,432 bytes; its fixture round-trips and does not represent valid service evidence. Common verification-work pricing counts the ordinary input authorization, offline-owner authorization and four service signatures per claim, with malformed payloads charged the eight-claim maximum. Multiple legitimate tickets are permitted until the explicit storage bound, never extra earning weight. Identical accepted claims are no-ops in the receipt store; conflicting same-ticket claims are refused. Outer account nonces still prevent replay of an included transaction. Retention/pruning follows the S2 challenge plus owner-ratified 100-block buffer, using heights. The local archive independently bounds public records to 4,096 records of 40 KiB; exhaustion is explicit refusal, not evidence eviction.

Some(receipt store), even empty, selects root v16, checkpoint 13 and snapshot 4. OFF sections remain byte-absent with v14 / checkpoint 10 or 11 / snapshot 2. Frozen v12 migration appends an absent receipt store only; old fixture bytes and signed S1 subjects are unchanged. Post-activation missing sections are refused. Producer, transition_extractor follower and archived WAL inputs verify the same carried receipt batches.

Refs #3527 items 1 and 2: escrow is Address(first20(SHA3-256(BodyBuilder.bytes("wrfcoin.node_bond_escrow.v1").bytes(deployment_id)))), with no private key or owner-selected address. Existing accounts and known/future producer payout bindings, faucet sink and oracle recipients cannot collide. Later protocol-credit configuration is also refused atomically if it would claim an existing bond escrow; an independent payout address remains allowed. Direct block validation tests cover inactive lifecycle inputs and exactly16/17 lifecycle inputs. Items 3–5 remain deferred to activation hardening.

This slice records service evidence only. No reserve, entitlement, mint, confiscation, CLI signing ceremony or operator activation is implemented. node_share_settlement remains default OFF, unscheduled and non-schedulable until S4–S6 and independent gates are complete.

### Slice 4: unminted authorization and activation gates (2026-10-12)

`node_share_settlement` remains default OFF, unscheduled and NON-schedulable.
Source completion of this slice does not authorize activation. Each block first
executes the existing mint consumers in their existing order, then reserves
`R_h = min(NodeSlice(h), MAX_SUPPLY - balances_sum - outstanding_reserves)`.
The shared ledger counts new cash issuance plus new reservations against the
current emission partition; its transient reserved/minted distinction is rebuilt
on replay. Checked account supply is cached in the common block context, refreshed
after ordinary account mutations and independently checked at finalization.
Node payouts update this cache by one checked aggregate, with no account scan per
recipient. All mint consumers use reserve-aware headroom when the feature is ON.

At close, eligibility and payout bindings are frozen and the equal split is
provisional. For sorted deployment IDs, the `B_e % n` extra units start at
`epoch % n` and wrap. Empty epochs release the entire authorization as UNISSUED,
without minting or carrying it forward. Redemption first occurs at
`close + challenge(chain) + 1`, with 1,024 testnet / 4,096 mainnet challenge blocks.
Old reserves credit ordinary payout balances/earned totals and the current mint
journal exactly once. Their prior authorization is not charged to the current
Node slice. Thus current cash minting can exceed current `E(h)` at maturity;
**new** authorization still cannot exceed `E(h)`, and balance conservation,
role ceilings, and `balances_sum + outstanding_reserves <= MAX_SUPPLY` remain
mandatory. A persisted finalization cursor prevents duplicate redemption.

Experimental Reader endpoint `/node/settlement/{epoch}` is absent when OFF.
It reports decimal base-unit strings and OPEN/PROVISIONAL/PAID/UNISSUED statuses;
unknown/pruned records use UNKNOWN and null amounts, distinct from known zero.
It cannot submit or authorize settlement. Producer metadata carries no authority.
Checkpoint format 14 freezes the former format 13 layout and appends settlement;
ON snapshots use format 5. OFF checkpoint/snapshot formats and roots are preserved.

Activation **must never** be scheduled over a chain that already activated S2
alone: `node_bond_registry = Some`, `node_receipt_registry = None` at a height
other than activation is refused before mutation. An already active S3 chain
without a settlement cursor is refused too. Format-13 migration rejects either
prior activation marker; it cannot fabricate missed historical reservations.
A fresh activation must introduce all markers together and replay sequentially.

Receipt batches must add at least one new claim; a duplicate-only batch cannot
consume a block slot or owner nonce. Caps are 16 batches per block and **two per
owner per block**, alongside existing claim/byte/pair caps. Cheap owner equality
precedes inner claim signature verification. Submission validates a bounded
write set before mutation; producer pruning and peer replay copy their block
state once rather than copying retained receipts per submission. Retained-claim
bounds validation uses the frozen codec's allocation-free size walk rather than
encoding every retained claim into a fresh byte buffer each block. Whole-state
staging, escrow rollback for ordinary transitions, canonical hashing and size
walks still cost CPU; this is not a constant-cost finalization claim.

Two explicit activation gates remain OPEN:

- **CPU/memory:** measure clone, signature, retained-anchor, hashing and close /
  maturity costs on the intended runtime at maximum retained cardinality, agree
  a per-block budget, and demonstrate producer/peer/WAL parity within it. Local
  debug synthetic timings (optimized test profile, 1,400 repeated public fixture
  claims) measured about 632 microseconds per registry clone and 137 microseconds
  per allocation-free bounds walk over 20 repetitions; encoded registry size was
  1,629,632 bytes and exported entries JSON 8,622,710 bytes. These transport-only
  samples are recorded in the S4 evidence report; they do not
  establish deployment capacity. The size walk remains O(retained claims).
- **Bootstrap capacity:** the bonded-ingest request limit is 2 MiB, which review
  estimates is exhausted around **1,400 claims**. The maximal retained shape
  (23,808 claims, approximately 37.5 MB in review) does not fit. Establish and
  test an authenticated bounded/chunked bootstrap design or another explicit
  owner-approved capacity policy before activation. No resource cap reduction
  or request-limit bypass is authorized by this slice.

Cross-role alias refusal remains OPEN: an owner's account may alias another
deployment's payout account. This slice does not claim to reject that case;
resolve the policy and add negative lifecycle/restore tests before activation.
S5 attributable-fraud filtering and S6 activation evidence remain separate gates.


### Slice 4 review delta: recorded identity and restored ledger (2026-10-12)

Challenge selection reads the state's recorded genesis emission identity through
its existing testnet/mainnet schedule classification, never a process identity or
literal chain id. Both maturity creation and validation use that same selection.
Frozen-history and receipt bounds are derived from the selected challenge plus
the existing withdrawal buffer and epoch length: three retained epochs on
testnet, six on mainnet; the settlement store permits one additional epoch.
The old testnet constant remains a compatibility/test shape pin only. The
mainnet geometry test exercises a recorded-mainnet state fold through the full
retention window; mainnet activation remains refused and is not authorized.

Snapshot and checkpoint loaders recompute cached account supply and restore
Node reservations from the decoded Node ledger spending before returning state.
With settlement ON, current Node spending exclusively represents reservations;
prior-reserve cash redemption never charges that current Node slice. Therefore
`minted(Node)` remains zero immediately after restore, without requiring a
repeat finalization. These fields stay nonserialized; no wire pins change.
The gamification API pre-filter uses the same reserve-aware committed supply as
consensus. Multi-epoch tests check cumulative cash issued plus outstanding
reserves against cumulative scheduled emission after every block, alongside the
absolute supply bound. Near-cap tests separately exercise faucet, contribution,
gamification, attester and producer settlement.


### Node-pay S5: attributable sanctions (default OFF, unschedulable)

A bounded `NodeFraudReport` carries one counterfeit claim or two conflicting
claims in the same genesis/epoch/frozen registration and one-manifest ticket.
The reporter signs the existing Node receipt authorization subject with the
separate `wrfcoin.node_fraud_report.v1` purpose commitment, exact evidence hash,
sender, nonce and genesis. Receipt-admission signatures cannot authorize reports.
The existing transaction fee/verification-cost model applies; sanctions add no
reporter debit, emission bounty, mint or burn.

Counterfeit evidence needs the registered server's valid payment endorsement of
the exact invalid inner signature. Only that historical server bond is blamed.
Equivocation needs two contradictory ticket commitments signed by the same frozen
server key or consumer endorsement key. Each actually authenticated equivocal
signer is responsible. Different tickets, duplicate claims and ordinary work
never establish fraud. Inclusion is allowed through `C_e + challenge(chain)`
inclusively; later reports cannot reopen finalized payouts. Testnet challenge
remains 1,024 blocks and mainnet 4,096; existing chain-derived constants govern.

The transition atomically debits 100% of each responsible remaining locked
escrow and transfers it into the fixed `node_nonspendable_forfeiture.v1` account.
Supply and INV-SUPPLY-1 are unchanged. That account has zero spendable balance
and equal balance/locked/required identities, with ordinary mutations refused.
The responsible bounded registration remains jailed; future membership, key
updates, exit and withdrawal are refused after any slash. A top-up or jail
recovery protocol requires a separate owner-authorized slice and cannot be
inferred from this transition. Repeated evidence never debits a bond twice;
normal reporter transaction nonce and block admission accounting still apply.

Close excludes jailed registrations. Maturity withholds each jailed node's
original provisional share and releases that authorization as unissued; it never
redistributes the withheld share to other nodes or mints it. Already finalized
payouts remain closed. Principal withdrawal retains the existing checked bound
`max(C_last + challenge(chain) + 100 + 1, exit_effective_height + 1)`.

Activated state adds the root-covered S5 marker, bounded jail/terminal sets and
confiscation total: root v18, checkpoint v15 with frozen v14 refusal for activated
partial S4, and snapshot v6. OFF roots, legacy snapshot entries and compatible
checkpoint writer remain unchanged. Producer, extractor, peer applier, input WAL,
checkpoint, snapshot and rollback derive the same sanction and maturity fold.
Feature scheduling remains refused; S6 runtime/ceremony, capacity and activation
evidence are still required. Colluding keys signing one consistent lie cannot
be convicted by these objective proofs.

S5 review correction: activation adopts a pre-existing forfeiture account by
locking its entire balance without minting or burning. The adopted amount is
root-covered separately from confiscated bonds; OFF-path transfers are unchanged.
Server ticket signing and fraud verification must stay in lockstep. The signer
uses a bounded redb signing-protection ledger keyed by epoch, consumer, ticket id,
committed with immediate durability before returning a signature. Conflicting
tickets are refused; identical retries reuse the durable signature. Records
remain through the inclusive challenge deadline and survive process restart
and chain WAL rollback. At capacity the signer refuses new work; it never
evicts live protection records.

S6 open item: owner-authorized jail recovery / registry slot reclaim. Jailed
registrations remain pinned and consume bounded slots; S5 introduces no recovery
or slot-reclaim code. Entitlement withholding never redistributes an already
closed provisional share; eligibility exclusion before close uses the existing
S4 division among eligible nodes.
