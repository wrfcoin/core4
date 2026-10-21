# WRFCoin L1 Protocol Specification

**Status:** Normative. This document describes the protocol **as implemented**, not as
intended. Where implementation and intent diverge, the divergence is stated explicitly
and tracked in a GitHub issue (see [Divergences](#divergences-between-specified-and-implemented-behaviour)).

**Derived from:** `wrfcoin/core4` commit `798b33367` (2026-07-21).
**Audience:** external auditors and protocol engineers. Every section cites the Rust
modules it was derived from so the code can be held against this text.
**Companion:** the threat model (adversary classes and trust boundaries) is
[`THREAT_MODEL.md`](./THREAT_MODEL.md) in this directory (merged via PR #2222; both
files are on `main` as of 2026-07-22, so the link this header previously withheld
against a dangling-link risk is now live). Vulnerability
reporting and general policy live in [docs/SECURITY.md](../SECURITY.md) (which exists on
`main`), and the complete RPC method inventory in
[RPC_SURFACE.md](./RPC_SURFACE.md); neither is duplicated here.

The key words MUST, MUST NOT, SHOULD, and MAY are used as in RFC 2119. Each section ends
with one or more normative invariants tagged `INV-<AREA>-<n>`. Tags that already exist in
`shared-protocol/src/invariants/` (e.g. `INV-BAL-1`) are reused verbatim so the spec and
the runtime checker never drift apart. The [Invariant index](#invariant-index) maps every
tag to the module expected to enforce it.

---

## 1. Chain identity

- `chain_id`: string, from `WRFCOIN_CHAIN_ID`, default `"777"`
  (`native-chain/src/bin/launch_api/helpers.rs`).
- `network_id`: string, from `WRFCOIN_NETWORK_ID`, default `"unknown"`.
- Since #2034, both values are folded (length-prefixed) into every v2 signing preimage
  (`push_chain_domain`, `shared-protocol/src/blockchain/transaction.rs`), giving
  EIP-155-class cross-network replay protection for commits, transparent transfers, and
  stake authorizations.
- The chain identity is **not** part of the block hash preimage (see §3 and divergence D2).

**INV-ID-1.** A v2 authorization signature produced for chain identity `(c, n)` MUST NOT
verify on any chain whose identity differs in either component.

## 2. Consensus — Proof of Publishing (PoP)

**Sources:** `shared-protocol/src/consensus/proof_of_publishing.rs` (the engine),
`consensus-engines/src/algorithms/pop_adapter.rs` (the adapter the node runs),
`consensus-engines/src/engine.rs`, `traits.rs` (the pluggable-engine framework).

The running engine on every node is `TypedConsensusManager<PopConsensusAdapter>`
(`native-chain/src/bin/launch_api/main.rs`). The other engines in
`consensus-engines/src/algorithms/` (PoW, PoS, DPoS, HotStuff, Raft, DAG) compile and
have tests but are **not wired into the node binary**; an auditor should treat them as
dead code for L1 consensus purposes.

PoP has **no leader election and no proposer schedule**. A block is valid if its
*contents* are valid (§3). Who may cause a block to exist is decided entirely off-chain:

- The HTTP route `POST /blocks/mine` requires the `Operator` API role
  (`native-chain/src/bin/launch_api/routes/mining.rs`).
- A block arriving over P2P gossip is accepted if it passes content validation and fork
  choice (`p2p_events.rs::apply_p2p_block`). **Blocks carry no producer signature and no
  producer identity** (`shared-protocol/src/blockchain/block.rs` — the struct has no
  proposer field). See divergence D3.

### Byzantine fault threshold and quorum arithmetic

PoP itself has no validator voting, so there is no consensus-level fault threshold. The
two quorum formulas that do exist:

1. **Oracle attestation quorum** (`shared-protocol/src/common/constants.rs`):

   ```text
   quorum(N) = max(2, floor(2N/3) + 1)
   ```

   applied dynamically in `verify_block` when the oracle set has N ≥ 3 members
   (`DYNAMIC_QUORUM_ACTIVATION_HEIGHT = 0`); below 3 the governance-configured
   `oracle_quorum` (default `DEFAULT_ORACLE_QUORUM = 2`) is used directly.

2. **BFT finality supermajority** (`consensus-engines/src/selection/bft_finality.rs`):

   ```text
   required_weight = floor(total_stake_weight * 2 / 3) + 1
   ```

   Exactly 2/3 does NOT finalize; strictly more than 2/3 does. Safety claim: a finalized
   block cannot be reverted unless ≥ 1/3 of stake weight is Byzantine. **This gadget is
   not attached on the running network** — see §5 and divergence D1.

**INV-CONS-1.** The `publishing_score` claimed in block metadata MUST equal the score
recomputed from the block's transactions
(`proof_of_publishing.rs::verify_block`, score mismatch → reject).

**INV-CONS-2.** Every block MUST contain exactly one `Coinbase` transaction and it MUST
be `transactions[0]` (`verify_block`, #1023).

**INV-CONS-3.** A block whose recomputed publishing score is below
`min_publishing_score` MUST carry either qualifying useful-work evidence
(`useful_work_liveness_fallback = "true"`, evidence recomputed and matched against
metadata) or the `"empty_liveness"` marker with coinbase ≤ 1/10 of the adjusted reward
(`LIVENESS_BLOCK_REWARD_FRACTION = 10`).

**INV-CONS-4.** Nodes MUST reject blocks with `version > CURRENT_BLOCK_VERSION` (= 3).

## 3. Block lifecycle

**Sources:** `shared-protocol/src/blockchain/block.rs`, `chain.rs`,
`proof_of_publishing.rs::verify_block`, `native-chain/src/bin/launch_api/p2p_events.rs`.

### Block structure and hash

`Block { version, prev_block_hash, transactions, difficulty, height, nonce, timestamp,
metadata: map<String,String>, hash }`. The PoP block hash is blake3 over
`version ‖ prev_block_hash ‖ height ‖ difficulty ‖ nonce ‖ timestamp_millis ‖
tx_hashes… ‖ sorted(metadata)` (`block.rs::calculate_hash`). Metadata iteration is
sorted, so the hash is insertion-order independent.

The block hash commits to **neither** the post-state root **nor** the chain identity.
State agreement between nodes is checked out-of-band by comparing
`state.state_root` values (the `state_diverged` latch in `p2p_events.rs`), not by header
validation. This is divergence D2.

### Acceptance conditions (in order, all MUST hold)

1. **Hash present** — blocks without a precomputed hash are rejected
   (`chain.rs::add_block`, #1028). The hash is *not* recomputed at `add_block`
   (engine-specific hashing); content-level hash checking is the consensus engine's job.
2. **Continuity** — `height == tip_height + 1` and `prev_block_hash == tip` (#1018).
   Exception: the very first block loaded into an empty chain skips both checks
   (genesis load path; see §8).
3. **Version** ≤ 3 (#1029).
4. **Timestamp** (activation height 0, #691/#815):
   - strictly greater than the previous block's timestamp;
   - strictly greater than the median of the last 11 block timestamps (BIP-113 style
     median-time-past);
   - not more than `max_future_drift(height)` ahead of the local wall clock (node-local
     policy, deliberately outside the state root).
5. **Coinbase discipline** (INV-CONS-2) and reward ceiling (§7).
6. **Fused per-transaction validation** (`verify_block_transactions_fused`): submission
   timing, quality enforcement, privacy-level enforcement, compute-fee/receipt rules,
   selective-disclosure proof verification, Groth16 ZK verification, and per-tx ed25519
   signature verification for Transfers (`TX_SIGNATURE_VERIFICATION_ACTIVATION_HEIGHT
   = 0`, #1279).
7. **Role-based reward validation** (`verify_role_based_rewards`).

On the P2P apply path, state transitions extracted from the block are applied **before**
`add_block`; a transition failure rolls back to the pre-block state so a bad block cannot
half-apply (`p2p_events.rs`, #1682 producer/applier parity).

### Reorganization

Fork choice (§5) may produce a `ReorgPlan { fork_height, fork_hash, blocks_to_revert,
blocks_to_apply }`. `execute_reorg` truncates above the fork point, installs the
caller-supplied fork-point state, and re-applies the fork's blocks. Fork points below
the in-memory window (~200 blocks, `WRFCOIN_MAX_IN_MEMORY_BLOCKS`) are resolved from
disk (#980, #1277, #1973), bounded by `MAX_REORG_DEPTH`.

**INV-BLOCK-1.** For every non-genesis block B accepted into the chain:
`B.height = parent.height + 1` and `B.prev_block_hash = hash(parent)`.

**INV-BLOCK-2.** For every accepted block B:
`B.timestamp > median(last 11 timestamps)` and `B.timestamp > parent.timestamp`.

**INV-BLOCK-3.** No reorganization may revert more than `MAX_REORG_DEPTH = 100` blocks;
deeper forks require manual intervention or full resync.

**INV-BLOCK-4.** No reorganization may have its fork point below the BFT-finalized
height (`plan_reorg` guard, #1216). *On the current network this invariant is vacuously
true because `finalized_height` never advances — see divergence D1.*

## 4. Validator set

**Sources:** `consensus-engines/src/selection/validator_set_source.rs`, `vrf.rs`,
`shared-protocol/src/blockchain/state.rs` (accounts, `oracle_set`, `signing_keys`).

As implemented there is **no block-producing validator set**. The identities that matter:

- **Finality validator set (defined but unwired):** `validators_from_state(state,
  min_stake)` projects every account with `stake_locked ≥ min_stake` into a
  `FinalityValidator { id, stake_weight, public_key: None }`, sorted by id so all nodes
  derive the identical set. Stake above `u64::MAX` saturates. No epochs, no entry/exit
  delay, no unbonding period exists for this set.
- **Oracle set:** `state.oracle_set` (hex ed25519 keys) with `oracle_quorum`. Mutable
  only via the operator admin API when `WRFCOIN_ENABLE_ORACLE_ADMIN_API=1` (default
  off, because the mutation is out-of-band and forks the state root, #1775) — i.e. in
  practice the oracle set is fixed at genesis seeding.
- **Signing-key registry:** `state.signing_keys` maps protocol roles (oracle, bridge
  operator, governance operator, …) to epoch-versioned keys; register/revoke are
  operator-gated routes with the same out-of-band caveat.

Admission and removal of block producers is therefore an **operational** act (issuing or
revoking an `Operator` API token, deploying a node) with no on-chain representation.

**INV-VAL-1.** All nodes observing the same `State` MUST derive the identical
finality-validator set (sorted, deterministic saturation).

**INV-VAL-2.** Oracle-set and signing-key mutations MUST NOT occur on a multi-node
network except through a state transition every peer replays (currently guaranteed only
by keeping the admin API flags off).

## 5. Fork choice & finality

**Sources:** `shared-protocol/src/blockchain/chain.rs::plan_reorg_with_loader`,
`shared-protocol/src/consensus/finality.rs`,
`consensus-engines/src/selection/bft_finality.rs`, `finality.rs`,
`native-chain/src/bin/launch_api/finality_events.rs`, `p2p_events.rs`.

**Fork-choice rule (implemented and active):** longest chain wins. At equal height, the
candidate tip wins only if its hash is **strictly lower** than the local tip hash.
Candidates must form a contiguous, hash-linked chain from `fork_height + 1`, the fork
point must be found within `MAX_REORG_DEPTH`, and INV-BLOCK-3/4 apply.

**Deterministic finality (implemented but inert):** the `BftFinalityGadget` finalizes a
block when accumulated verified stake weight strictly exceeds 2/3 of the total
(`required_weight` above). Finality signals flow through
`finality_signal_channel` → `run_finality_event_loop` →
`Blockchain::set_finalized_height` (monotonic; lower heights silently dropped). Vote
signatures are ed25519 over `(block_hash ‖ height ‖ validator_id)`; equivocation (two
different blocks at one height by one validator) is detected, converted to
`EquivocationEvidence`, and persisted via the durable evidence store
(`native-chain/src/consensus/evidence_store.rs`) for on-chain `SlashingReport`
submission.

**On the running network the gadget is not constructed**: `main.rs` hard-codes
`bft_finality_for_p2p = None` (the #1501b–e wire-up has not landed), so no node ever
votes, inbound P2P `FinalityVote`s are dropped, and `finalized_height` remains 0
forever. Inbound `FinalityCertificate`s would be validated and accepted, but nothing on
the network produces them.

**"Economically irreversible" therefore does not exist on this chain today.** Practical
irreversibility rests on (a) the single-operator deployment producing all blocks and
(b) `MAX_REORG_DEPTH = 100` as a hard ceiling on automated reorgs. This is an
operational trust assumption, not a cryptoeconomic guarantee. See divergence D1 and
THREAT_MODEL.md §"Malicious validator".

**INV-FIN-1.** `finalized_height` is monotonically non-decreasing, and no block at or
below it is ever reverted by fork choice.

**INV-FIN-2.** (When the gadget is wired.) A block finalizes only when verified votes
representing strictly more than 2/3 of total stake weight endorse the same
`(hash, height)`; conflicting finalizations at one height therefore require ≥ 1/3
Byzantine stake.

**INV-FIN-3.** At equal chain length, a node MUST NOT replace its tip with a candidate
whose tip hash is greater than or equal to its own (deterministic tie-break).

## 6. Transaction validity & state transition

**Sources:** `shared-protocol/src/blockchain/transaction.rs`, `state.rs`,
`shared-protocol/src/mempool/pool.rs`, `proof_of_publishing.rs`
(`verify_block_transactions_fused`, `classify_block_transaction`).

### Transaction types

`TransactionType` has 33 variants (`transaction.rs`). The economically significant
groups and their validity predicates:

| Group | Types | Authorization / validity |
|---|---|---|
| Value transfer | `Transfer`, `TransparentTransferPayload`-carrying txs | ed25519 signature over v2 domain `wrfcoin_transparent_transfer_auth_v2` + chain identity + **account nonce**; verified at mempool admission (#1727) and in `verify_block` (#1279); balance sufficiency + nonce strictly-next at apply |
| Coinbase | `Coinbase` | position/count (INV-CONS-2), amount ceiling (§7); no signature |
| Staking | `Stake`, `Unstake` | ed25519 over v2 domain `wrfcoin_stake_auth_v2`, action- and nonce-bound (#1763); fail-closed at admission |
| Weather commit/reveal | `ContributionCommit/Reveal`, `ForecastCommit/Reveal` | commit-auth v2 signature (`wrfcoin_commit_auth_v2`, #1848/#2034); commit/reveal window arithmetic (§6.3); payload ≤ `MAX_COMMIT_REVEAL_PAYLOAD_BYTES`; reveal must match pending commit; CID + provenance field validation |
| Oracle | `OracleAttestation` | canonical signing message, signer must be in `oracle_set`, per-payload nonce in `spent_oracle_nonces` (replay-proof); quorum per §2 |
| Governance | `GovernanceProposal/Vote/Execute/Finalize/Delegate`, `Treasury*` | vote signatures verified (`verify_vote_signature`, active from genesis #933); proposal deposit; quorum `GOVERNANCE_DEFAULT_QUORUM_PERCENT = 33`, voting period 7 200 blocks |
| Privacy | `ConfidentialTransfer` | Pedersen commitments + range proofs, key-image double-spend set |
| Privacy (disabled) | `ShieldedDeposit`, `ShieldedWithdrawal` | **Rejected at admission and in `verify_block`** — the pool verified no value-backing proof, an unbacked-mint vector (#1762) |
| Rewards | `Reward`, `RewardClaim` | `RewardClaim` = MLSAG ring signature (min ring size 7) over eligible station keys, epoch = 1 000 blocks, key image dedup |
| Slashing | `SlashingReport` | evidence-hash dedup (`slashing_evidence_seen`, #1393) |
| L2 | `Layer2BatchSubmit` | ZK proof for batch finalization |

### Mempool admission (`pool.rs::add_transaction_with_signing_keys`)

Order of gates: partition-mode write gate (#1217) → tx id present → duplicate check →
structural validation → shielded-pool rejection (#1762) → governance-proposal rejection (#2024) →
per-transaction size cap → conditional minimum-fee floor (#2039; disabled at the default 0) →
per-sender count cap (#2039) → payload size
caps (commit/reveal, oracle attestation) → transparent-transfer authorization (#1727) →
non-transparent `Transfer` input authorization (#2024) → stake authorization (#1763) →
launch-protocol replay key check → intra-mempool double-spend check (two-pass) → capacity eviction
(fee-rate ordered) → insert. Fee ordering is
`(fee_rate desc, abs_fee desc)`.

The double-spend check runs **before** capacity eviction, not after: every rejection gate is
read-only against the live pool, so a rejected submission — the attacker-controlled case — costs no
eviction and mutates nothing (#2555). An earlier version of this list had those two reversed.

**Bounds, corrected for #2700.** This paragraph previously read *"Mempool capacity: 50 000 txs
(`MEMPOOL_CAPACITY`), max block 32 MiB, max tx data 2 MiB."* None of those three figures describes
code that runs — `MEMPOOL_CAPACITY`, `MAX_BLOCK_SIZE` and `MAX_TRANSACTION_DATA_SIZE` are consumed
by no code (#2700). What is actually enforced:

- **Mempool: bounded in bytes, not in transaction count** — `mempool_max_size_bytes`, default
  50 MiB, with fee-rate-ordered eviction on overflow (#2555), plus a per-sender cap of 1 024
  unconfirmed transactions (#2039). There is no global count bound. A 4-hour expiry horizon is
  configured, but the automatic cleaner has no production caller; age is therefore not a live
  bound. The operator purge route is manual and defaults to a separate 1-hour cutoff.
- **Mempool-admitted transaction size: 100 KB per transaction**
  (`config.max_transaction_size`), measured over the whole transaction, applied to every type at
  admission — about 20× tighter than the declared 2 MiB. Transactions arriving inside a peer block
  bypass this admission gate.
  Narrower payload caps apply on top: 64 KiB for contribution/forecast commit and reveal, 32 KiB
  for oracle attestation.
- **Block size: bounded by transport framing, and unequally** — by default 64 KiB on the gossip
  path (`inline_data_max`), with independent 1 MiB outer caps, versus 16 MiB on the block-sync path.
  Neither is an explicit block-validity rule, and the two ingress paths do not agree. The
  5 000-transaction-per-block cap is producer-side only and does not constrain an incoming block.

See `docs/security/RESOURCE_CEILINGS.md` for the file-and-line ledger behind each of these, and
#2700 for whether the declared constants should be enforced or deleted.

### Commit/reveal windows

`commit_window_blocks(h)` and `reveal_window_blocks(h)` scale the terminal-speed
baselines (720 / 1 440 blocks at 1 s) by the block-time progression, floored at 3 blocks
and capped at 4 hours wall clock (`MAX_COMMIT_REVEAL_WALL_CLOCK_SECONDS`, #884/#1040).

### State and state root

`State` (`state.rs`) is the consensus state. Its root is recomputed over exactly the 26
sections listed in `STATE_ROOT_MATERIAL_FIELD_NAMES` (accounts, nfts, pending commits/
reveals, oracle set/quorum/attestations, privacy replay sets, signing keys, station
registry, governance, spatial data, mining pools, …). All collections hashed into the
root are sorted or `BTreeMap`-backed; height-index maps and pruning metadata are
deliberately **excluded** so pruning never moves the root. Replay-protection sets are
pruned after `STATE_PRUNING_RETENTION_BLOCKS = 100 000` blocks, which MUST exceed
`MAX_REORG_DEPTH` (it does, by 1000×). Determinism rules: no floats, no wall-clock
reads, no unseeded randomness, no unordered-map iteration in any code path that feeds
the root.

**INV-BAL-1** (runtime-checked). `BalanceTransfer` conserves the total balance across
all accounts.

**INV-BAL-2** (runtime-checked). For every account:
`balance = stake_locked + spendable_balance`.

**INV-BAL-3** (runtime-checked). Stake lock/release changes composition, never total.

**INV-TX-1.** A transaction's authorization signature MUST bind the signer, the action,
the chain identity, and (for money-path types) the account nonce; nonces are enforced
strictly-increasing from genesis (`NONCE_ENFORCEMENT_ACTIVATION_HEIGHT = 0`, #1002).

**INV-TX-2.** No replay-protected payload (oracle nonce, launch replay key, reward-claim
key image, privacy key image/nullifier, slashing evidence hash) is ever applied twice
within the retention window.

**INV-TX-3.** Applying the same block sequence to the same genesis state yields the
same `state_root` on every node (producer/applier/WAL-replay parity, #1682).

## 7. Monetary policy

**Sources:** `proof_of_publishing.rs` (emission constants, reward paths),
`shared-protocol/src/invariants/rewards.rs`, `slashing.rs`,
`shared-protocol/src/security/slashing.rs`, `consensus-engines/src/economics/`.

- **Units:** all amounts are `u64`/`u128` base units; `10^8` base units = 1 WRF
  (PR #505). No floats on any value path.
- **Max supply:** `MAX_SUPPLY = 21 000 000 WRF = 2 100 000 000 000 000` base units.
  Reward budgeting computes `remaining_supply = MAX_SUPPLY − total_minted_supply` and
  never mints past it.
- **Issuance schedule:** Bitcoin-style halving, `base_reward(h) = 50 >> (h / 210 000)`
  WRF (zero after 64 halvings), scaled by block-time progression:
  `adjusted_block_reward(h) = base(h) × target_block_time(h) / 14 400`. The progression
  table runs 4 h at genesis down to 1 s at height 10 000 000
  (`BLOCK_TIME_PROGRESSION`, `common/constants.rs`), keeping *daily* emission
  approximately constant.
- **Per-block emission ceiling:** total new WRF per block (coinbase + all contribution
  and forecast rewards) ≤ `MAX_BLOCK_EMISSION = 200 WRF`.
- **Reward multipliers:** data-desert × hyper-local composition capped at
  `MAX_COMBINED_MULTIPLIER_BPS = 20 000` (2.0×) versus base reward.
- **Liveness discount:** empty-liveness blocks earn at most 1/10 of the adjusted reward.
- **Fees:** fee-rate-ordered mempool; a fee-less lane exists for weather data backed by
  compute receipts (`work_factor ≥ effective_min_work_factor(h)`, scaling inversely
  with block time) and rate-limited per block, per worker, and per epoch (#835, #879).
  `WRFCOIN_MIN_WORK_FACTOR=0` disables the receipt requirement (testnet only).
- **Slashing:** deterministic penalties per reason (`security/slashing.rs`), scaled
  1–100 base units, at most `MAX_SLASHING_EVENTS_PER_BLOCK = 100` events per block via
  a circuit breaker (correlated-weather-event protection), flushed in deterministic
  oracle-id order at end of block (#873).
- **Burn:** there is no burn mechanism.

**INV-RWD-1** (runtime-checked). `earned_balance_total` never decreases for any account.

**INV-RWD-2** (runtime-checked). No single `ApplyReward` transition exceeds
`MAX_BLOCK_EMISSION`.

**INV-SLASH-1** (runtime-checked). A `Slash` transition never increases the target's
balance, stake, or spendable balance.

**INV-ECON-1.** `total_minted_supply ≤ MAX_SUPPLY` at every height.

**INV-ECON-2.** `coinbase_amount ≤ adjusted_block_reward(height)` for every block
(`verify_block`, #809).

## 8. Genesis & upgrade

**Sources:** `chain.rs::add_block` (first-block path), `common/constants.rs`,
`p2p` genesis handshake topic (`p2p/src/message.rs`), deployment tooling in
`wrfcoin/infra`.

- **Genesis procedure:** genesis is *operational*, not in-protocol. The first block
  loaded into an empty chain bypasses the continuity checks entirely
  (`add_block`: `total_block_count == 0` → any height, any parent accepted).
  `GENESIS_BLOCK_PARENT_HASH` is the all-zero hash. The canonical testnet genesis hash
  is pinned in deployment configuration and cross-checked at connect time via the
  `wrfcoin/genesis-handshake/1` gossip topic: peers exchange genesis hashes and
  disconnect on mismatch. Initial account balances, oracle set, and signing keys are
  seeded deterministically at genesis by the deployment tooling.
- **Upgrade mechanism:** there is none on-chain. Upgrades are coordinated operator
  binary swaps. Two guard rails exist:
  1. version ceiling — nodes reject blocks with `version > 3` (INV-CONS-4), so an
     upgraded producer cannot silently drag non-upgraded peers onto a new format;
  2. signing-domain versioning — incompatible signature-preimage changes bump the
     domain tag (`…_v1` → `…_v2`, #2034) and are deployed as a hard cut at a fresh
     genesis.
- **Who may authorize an upgrade:** whoever controls the node deployments and the
  release-signing pipeline (`.github/workflows/release-signing.yml` — GPG detached
  signatures over binaries and a SHA-256 `CHECKSUMS.txt`; **no cosign/sigstore**, and
  no signed provenance, transparency log, or independently verified reproducibility — see
  THREAT_MODEL.md A5; #2263 removed the former misleading provenance output). There is no
  on-chain vote gating upgrades;
  the governance module's parameter updates (`governance/`) change *parameters*, not
  code.

**INV-GEN-1.** Two peers whose genesis hashes differ MUST NOT sync with each other
(genesis handshake disconnect).

**INV-GEN-2.** A node MUST refuse to follow a chain requiring protocol rules newer than
it implements (version ceiling).

## 9. Assumptions

The security argument of everything above rests on:

1. **Clock skew** — bounded by `max_future_drift(height)` (scaled with block time);
   nodes with clocks outside this bound will reject honest blocks (liveness, not
   safety, impact).
2. **Network delay** — gossip delivers blocks/txs well within one block time; the
   commit → reveal flow additionally assumes delivery within the (≥ 3 block, ≤ 4 h)
   windows of §6.3.
3. **Honest operator** — because finality is inert (D1) and blocks are unsigned (D3),
   chain safety today assumes the single block-producing operator (the N5 collector
   pipeline driving `/blocks/mine`) is honest and its `Operator` token uncompromised.
   This replaces the usual honest-majority assumption entirely.
4. **Oracle honesty** — fabricating accepted weather data requires ≥ quorum(N)
   colluding oracle keys (§2); below N = 3 the network is explicitly centralized.
5. **Key security** — API role tokens (env vars), the operator HSM/YubiKey signing
   setup, oracle keys, and the GitHub release-signing credentials are uncompromised.
6. **Dependency integrity** — the Rust/npm supply chain is honest to the extent CI
   scanning (`supply-chain-security.yml`, `sbom-generate.yml`) can enforce.

**INV-ASSUME-1.** Consensus-critical code reads no wall clock, floats, or unseeded
randomness (the future-drift check is explicitly node-local policy and excluded from
state).

## 10. Privileged capabilities

This section inventories the **admin / authority-granting operations** — every route gated
by the Operator token, plus the operations that grant privileged authority (mint, halt,
seed, sign, snapshot-trust) even when gated by a lower role. It is the security-relevant
subset; the **complete** per-method inventory with public / authenticated / admin
classification is [`RPC_SURFACE.md`](./RPC_SURFACE.md) (#2132), which this section does not
duplicate. Route strings and roles are verified against
`native-chain/src/bin/launch_api/routes/mod.rs` and each handler's `require_role` gate at
this commit (swept in full, not sampled). See THREAT_MODEL.md for reachability and attacker
value.

| Capability | Interface | Key / gate |
|---|---|---|
| Produce blocks | `POST /blocks/mine` | `WRFCOIN_OPERATOR_TOKEN` |
| Purge/clear mempool | `POST /operator/mempool/{purge,clear}` (plus alias `POST /mempool/purge`) | Operator token |
| Halt / resume bridge, update caps, open+resolve challenges, scan stuck | `POST /bridge/safety/{halt,resume,caps/update,challenge/{open,resolve},scan-stuck}` | Operator token |
| Submit a bridge challenge | `POST /bridge/safety/challenge/submit` | **Contributor** token (`submit_bridge_challenge`) |
| Set partition mode (write-gate the mempool) | `POST /operator/partition-mode` | Operator token |
| Submit / activate / finalize / execute governance proposals | `POST /governance/proposals/submit`, `PUT /governance/proposals/{id}/{activate,finalize,execute}` | Operator token (proposer header unauthenticated — see D5) |
| Deposit / withdraw governance treasury | `POST /governance/treasury/{deposit,withdraw}` | Operator token |
| Submit a layer-2 rollup batch | `POST /layer2/submit-batch` | Operator token |
| Ingest a state snapshot-sync payload | `POST /network/snapshot-sync/ingest` | Operator token **and** a trusted snapshot signature (`verify_snapshot_sync_trust`; signers from `WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS`) |
| Mutate oracle set / oracle config | `POST /transactions/oracle/operators`, `PUT /transactions/oracle/config` | Operator token **and** `WRFCOIN_ENABLE_ORACLE_ADMIN_API=1` (default off; state-forking, #1775) |
| Register / revoke signing-key epochs | `POST /transactions/signing-keys/{register,revoke}`, `POST /network/signing-keys/{register,revoke}` | Operator token (+ same `WRFCOIN_ENABLE_ORACLE_ADMIN_API` flag) |
| Mint balances from nothing | `POST /debug/seed-account` | Operator token **and** `WRFCOIN_ENABLE_DEBUG_SEED_API=1` (default off; state-forking, #1811) |
| Mint / transfer NFTs | `POST /nft/mint`, `POST /nft/{id}/transfer` | Operator token **and** `WRFCOIN_ENABLE_NFT_MUTATION_API=1` (default off; state-forking, #1764) |
| Verify NFTs | `POST /nft/{id}/verify` | **Contributor** token **and** `WRFCOIN_ENABLE_NFT_MUTATION_API=1` (default off; state-forking, #1764) |
| Disable all API auth (**debug builds only** — the bypass is `#[cfg(debug_assertions)]`-gated; release binaries ignore the variable and refuse to start with an empty token store) | `WRFCOIN_ALLOW_NO_AUTH=1` | node environment access + a debug-profile binary |
| Trusted snapshot signers | `WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS` | node environment access |
| Sign releases | `release-signing.yml` | GitHub repo secrets / signing keys |
| Submit contribution commits/reveals | `POST /transactions/contribution/{commit,reveal}` | `WRFCOIN_CONTRIBUTOR_TOKEN` |
| Submit oracle attestations | `POST /transactions/oracle/attestation` | `WRFCOIN_ORACLE_TOKEN` + oracle-set membership |
| Permissionless lane | `POST /transactions/permissionless/submit` | none (signature + nonce + timestamp + replay map + rate limit; `data_analysis` type only) |

Several **read-only** endpoints are also Operator-gated but grant no mutation or authority,
so they are outside this authority inventory: `GET /network/audit-log`,
`GET /node/metrics/host`, `GET /network/peers/reputation`, `GET /version`, and
`GET /{transactions,network}/signing-keys/{role}`. They are admin-gated for confidentiality,
not authority; their classification is in [`RPC_SURFACE.md`](./RPC_SURFACE.md).

Role lattice (`shared-protocol/src/security/api_auth.rs`): Operator ⊇ {Contributor,
Oracle, Reader}; Contributor and Oracle each ⊇ Reader; Reader is read-only. NFT
verification and bridge-challenge submission are deliberately Contributor operations, not
Operator — they touch protocol state but are not full admin capabilities.

**INV-PRIV-1.** On a multi-node network, no state-root-material field is mutated except
by a transaction every peer replays. The three flag-gated route families above — oracle/
signing-key admin (`WRFCOIN_ENABLE_ORACLE_ADMIN_API`), debug-seed
(`WRFCOIN_ENABLE_DEBUG_SEED_API`), and NFT mutation (`WRFCOIN_ENABLE_NFT_MUTATION_API`,
covering both the mint/transfer and verify rows) — violate this by design when enabled
and MUST remain disabled on any network with more than one node.

**INV-PRIV-2.** The node MUST refuse to start with no API tokens configured. The sole
exception is **debug builds only**: `WRFCOIN_ALLOW_NO_AUTH=1` bypasses the check under
`#[cfg(debug_assertions)]` (`collect_token_store` in `main.rs`). The bypass is compiled
out of release binaries, which reject an empty token store unconditionally — enforced by
a release-only regression test
(`collect_token_store_bypass_is_absent_in_release_builds`, `main.rs`).

## Invariant index

| Tag | Statement (short) | Enforcing module |
|---|---|---|
| INV-ID-1 | signatures bound to chain identity | `shared-protocol/src/blockchain/transaction.rs` (`push_chain_domain`) |
| INV-CONS-1 | claimed publishing score = recomputed | `shared-protocol/src/consensus/proof_of_publishing.rs::verify_block` |
| INV-CONS-2 | exactly one leading coinbase | `shared-protocol/src/consensus/proof_of_publishing.rs::verify_block` |
| INV-CONS-3 | low-score blocks need useful-work / reduced reward | `shared-protocol/src/consensus/proof_of_publishing.rs::verify_block` |
| INV-CONS-4 | version ceiling | `shared-protocol/src/consensus/proof_of_publishing.rs` (`CURRENT_BLOCK_VERSION`) |
| INV-BLOCK-1 | height/parent continuity | `shared-protocol/src/blockchain/chain.rs::add_block` |
| INV-BLOCK-2 | timestamp monotonic + MTP | `shared-protocol/src/consensus/proof_of_publishing.rs::verify_block` |
| INV-BLOCK-3 | reorg depth ≤ 100 | `shared-protocol/src/blockchain/chain.rs::plan_reorg_with_loader` |
| INV-BLOCK-4 | no reorg below finalized height | `shared-protocol/src/blockchain/chain.rs::plan_reorg_with_loader` |
| INV-VAL-1 | deterministic validator-set derivation | `consensus-engines/src/selection/validator_set_source.rs` |
| INV-VAL-2 | no out-of-band oracle/key mutation | `native-chain/src/bin/launch_api/main.rs` (admin-API flags) |
| INV-FIN-1 | finality monotone, finalized never reverted | `shared-protocol/src/blockchain/chain.rs::set_finalized_height` |
| INV-FIN-2 | finalize ⇒ > 2/3 stake | `consensus-engines/src/selection/bft_finality.rs::required_weight` |
| INV-FIN-3 | equal-height tie-break strictly lower hash | `shared-protocol/src/blockchain/chain.rs::plan_reorg_with_loader` |
| INV-BAL-1 | transfer conserves total balance | `shared-protocol/src/invariants/balance.rs` |
| INV-BAL-2 | balance = stake + spendable | `shared-protocol/src/invariants/balance.rs` |
| INV-BAL-3 | lock/release conserves per-account total | `shared-protocol/src/invariants/balance.rs` |
| INV-TX-1 | auth binds signer/action/chain/nonce | `shared-protocol/src/blockchain/transaction.rs`, `shared-protocol/src/mempool/pool.rs` |
| INV-TX-2 | no replay within retention | `shared-protocol/src/blockchain/state.rs` (replay sets), `shared-protocol/src/mempool/pool.rs` |
| INV-TX-3 | state-root determinism | `shared-protocol/src/blockchain/state.rs::recalculate_state_root` (+ parity tests) |
| INV-RWD-1 | earned totals monotone | `shared-protocol/src/invariants/rewards.rs` |
| INV-RWD-2 | per-transition emission cap | `shared-protocol/src/invariants/rewards.rs` |
| INV-SLASH-1 | slash never credits | `shared-protocol/src/invariants/slashing.rs` |
| INV-POP-1 | useful-work activation non-regression | `shared-protocol/src/invariants/pop.rs` |
| INV-ECON-1 | minted ≤ MAX_SUPPLY | `shared-protocol/src/consensus/proof_of_publishing.rs` (reward budget) |
| INV-ECON-2 | coinbase ≤ adjusted reward | `shared-protocol/src/consensus/proof_of_publishing.rs::verify_block` |
| INV-GEN-1 | genesis-hash handshake | `p2p/src/message.rs` (`GENESIS_HANDSHAKE_TOPIC`) + handshake handler |
| INV-GEN-2 | version-gated upgrades | `shared-protocol/src/consensus/proof_of_publishing.rs::verify_block` |
| INV-ASSUME-1 | determinism discipline | `shared-protocol/src/blockchain/state.rs` (BTreeMap/sorted collections) |
| INV-PRIV-1 | no out-of-band state-root mutation | `native-chain/src/bin/launch_api/main.rs` (flag gates, #1764/#1775/#1811) |
| INV-PRIV-2 | auth fail-fast at boot | `native-chain/src/bin/launch_api/main.rs` |

## Divergences between specified and implemented behaviour

Recorded per the epic-#2093 rule: *the disagreement is the finding.*

- **D1 — Deterministic finality is inert.** The BFT gadget, vote verification,
  certificates, equivocation evidence, and the chain-side reorg guard all exist and are
  tested, but `main.rs` constructs no gadget (`bft_finality_for_p2p = None`), so no
  vote is ever cast and `finalized_height` stays 0. INV-BLOCK-4/INV-FIN-1 are vacuous
  in production. Tracked: **#1501** (open).
- **D2 — Block hash commits to neither state root nor chain identity.** Peers detect
  state divergence only by out-of-band root comparison; a block header cannot prove
  which state it produced, and headers are replayable across networks with the same
  content. Filed: **#2219**.
- **D3 — Blocks are unsigned; producer authority is off-chain.** Any P2P peer can
  gossip content-valid blocks; production permissioning exists only as API-token
  gating on `/blocks/mine`. Combined with D1, chain safety is an operational trust
  assumption (§9.3). Filed: **#2220**.
- **D4 — `docs/SECURITY.md` overstates protections.** It claims "Protection against
  chain reorganization attacks" and "Multi-signature support for critical operations";
  the former is only `MAX_REORG_DEPTH` + an inert finality guard (D1), and no
  multi-sig exists on any privileged path (single bearer token). The claims file
  should be read as aspirational.
- **D5 — Governance proposer identity is unauthenticated at the API layer.** Proposal
  submission requires the Operator token, but the proposer address comes from an
  unverified `X-Proposer-Address` header (fallback `"admin"`); votes, by contrast,
  are signature-verified. Governance is effectively operator-controlled at this
  commit (consistent with §10, but distinct from the signed-vote design intent).
- **D6 — Six consensus algorithms ship un-wired.** PoW/PoS/DPoS/HotStuff/Raft/DAG in
  `consensus-engines/src/algorithms/` are compiled and tested but unreachable from the
  node binary; audit effort should not be spent on them as L1 attack surface (they
  remain supply-chain surface).

---

*Maintenance rule: any PR that changes a constant, activation height, validity
predicate, or privileged route referenced here MUST update this document in the same
change set.*
