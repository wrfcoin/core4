# ADR: storage keepers — archive retrieval with random audits on the bonded-service model

Status: **PROPOSED (draft for owner ratification).** Design only. It authorizes no emission share,
transaction type, `ConsensusFeature`, deployment or wiring change. The owner's open questions are at
the end, each with a recommended answer.

Date: 2026-10-28.

Refs: `wrfcoin/core4-archive#3564` (storage keepers), `wrfcoin/core4-archive#3604` (epic, story B1),
`docs/design/agents-earning-phase-b-scope.md` §B-6 (owner, 2026-10-24: "In this phase"),
`wrfcoin/infra-archive#943` / `#944`, `wrfcoin/storage-providers#65` (the orphaned wiring),
`wrfcoin/core4-archive#1488` (Keeper in ComputeReceipt). Code references are to public `core4` main
`cc7f4d4` and public `storage-providers` main.

## Context

White paper §5.2: *data keepers* are paid for storage, randomly audited for retrieval, lose stake on
failure, and carry reputation. What exists today:

| Piece | Where | State |
|---|---|---|
| Keeper attestation types | `keeper-proof/src/lib.rs`: `StorageAttestation` (signed, domain `wrfcoin.storage_attestation.v1`), `RetrievalProof` (unsigned), `KeeperRecord`, `PinProof`, `HealthMonitor` | Types only. Nothing on chain reads them. |
| Bonded service + receipts (r20.9) | `shared-protocol/src/consensus/node_service_receipt.rs:41-60` (`NodeServiceReceipt`, consumer-signed, `NodeService::{SnapshotJoin, SyncRange}`), `node_share_contracts.rs:20-26,213-219` (`NODE_BOND_WRF = 250`, `NODE_EPOCH_BLOCKS = 1 024`, challenge windows, `BondedServiceClaim`), `node_receipt_registry.rs`, `node_bond_registry.rs`, `node_fraud.rs` | Live for snapshot serving, behind `node_share_settlement`. This is the model to generalise. |
| Real storage clients | `storage-providers` (public): `ipfs`, `filecoin`, `arweave` providers; `data-tiering` (`dedup_registry.rs` with per-replica `update_verification`, `retention_scheduler.rs`); `storage-common` (`chunking.rs` 16 KiB–1 MiB chunk heuristics, `integrity.rs` whole-object hashing) | Real and tested, but orphaned: infra's storage service packages backend's mocked `storage_networks_server.js` (infra#943/#944, storage-providers#65). |
| Chunk-level commitment | — | **Missing.** Integrity today is a whole-object hash, so a challenger can only ask for the whole object. |

## Decision drivers

1. **Checkable without trust** (the epic's principle). An audit passes only if the keeper returns
   bytes that verify against a commitment fixed before the challenge.
2. **Unpredictable challenges.** The keeper cannot know which bytes will be asked for until it is too
   late to fetch them from somewhere else.
3. **Reuse the bonded-service machinery**: bond, epoch, challenge window, fraud path and receipt
   batches, instead of a parallel economy.
4. **Cheap to verify**: a challenge costs one chunk and a log-size proof, not a whole archive.
5. **No real-money credentials assumed.** Whether funded Filecoin and Arweave wallets exist is unverified:
   storage-providers#65 Phase 0 is only a check, and it has not been run. The design therefore needs
   none. Keepers on testnet hold data on IPFS or local disk.

## 1. What a keeper stores, and the commitment it is audited against

An **archived object** is any content-addressed blob the network wants retrievable later: block
ranges for the B-2 work-unit inputs, signed snapshots, report artifacts. At archive time the uploader
records an `ArchiveCommitment`:

```
dataset_cid     content identifier (IPFS CID or sha3 digest)
byte_len        total length
chunk_size      fixed (proposed 64 KiB; within storage-common's 16 KiB–1 MiB range)
chunk_count     ceil(byte_len / chunk_size)
merkle_root     SHA3-256 binary Merkle root over the chunk hashes, domain-separated
                (leaf = H("wrfcoin.archive_chunk.leaf.v1" ‖ index ‖ chunk), node = H("…node.v1" ‖ l ‖ r))
```

The commitment, not the CID, is what audits verify against. A CID alone cannot verify a single chunk
without the whole DAG. Recording commitments on chain (who may, and at what cost) is a later
consensus slice (§5). The off-chain challenger takes them as input.

## 2. Keepers are bonded servers

A keeper is a **wallet** that bonds exactly like a snapshot server (`NodeBondLifecycle`,
`NODE_BOND_WRF`) and registers the archive objects it holds for an epoch. There is no new identity
class: the same wallet can serve snapshots and keep archives, and the bond covers both. Whether
keeping needs its own bond, or a larger one, is Q2.

## 3. Random audits

Per keeper, per epoch:

1. **Seed.** One block hash is not enough: the producer of that block can grind it, retrying until the
   seed lands on chunks a colluding keeper holds. The seed is therefore a hash over the hashes of
   `SEED_SPAN_BLOCKS` (proposed 8) **consecutive blocks that all come after** the keeper's holdings for
   the epoch are committed (`combine_seed` in the companion code PR). To steer it, an attacker must
   produce every block in the span. The keeper cannot predict the seed when it commits its holdings,
   and nobody can choose it afterwards. A commit-reveal beacon from bonded parties is the stronger
   alternative if the producer set is small enough for one party to produce eight blocks in a row
   (Q7).
2. **Challenge.** `k` audits (proposed `k = 4`) are drawn deterministically from
   `H(seed ‖ keeper ‖ i)`. Each picks one of the keeper's registered objects and then one chunk
   index in it. Anyone can recompute the challenge from public data, so no auditor can single a
   keeper out.
3. **Response.** Within the response window (proposed 64 blocks, well inside
   `TESTNET_CHALLENGE_BLOCKS = 1 024`) the keeper returns the chunk bytes and the Merkle path.
4. **Verdict.** The challenger checks `leaf(index, chunk)` against `merkle_root` along the path.
   - **Pass:** the chunk is the right size and the path verifies.
   - **Fail:** a wrong chunk, a wrong path, the wrong index, or a missing response by the deadline.
5. **Receipt.** The challenger signs a `RetrievalAuditReceipt`: a domain-separated preimage in the
   `NodeServiceReceipt` style, with its own tag `wrfcoin.retrieval_audit_receipt.v1`. It carries the
   challenge (epoch, seed, keeper, object, chunk index), the verdict and the challenger's key. Like a
   consumer's snapshot receipt it proves *what the signer checked*, and anyone can recheck a pass
   from the chunk and path.

A pass receipt can be re-verified by anyone holding the chunk and path. A fail receipt is only an
accusation: a lying challenger could sign "fail" on an honest keeper. So **fails go through the fraud
path, not straight to slashing** (§4).

## 4. Pay, forfeit and slash (the bonded-service model)

*Not activated by this ADR.* The emission split is untouched. The tokenomics ADR (`wrfcoin/core4#27`)
is merged and accepted, but its economics are not ratified, and a keeper share is Q1.

- **Epoch share.** A keeper earns its epoch share for keeping only if **every** audit drawn for it
  that epoch has a pass receipt. One failed or missing audit **forfeits the whole epoch share**.
  This proposal is not yet an owner ruling (Q5). It is the bonded-service rule: service unproven →
  unpaid.
- **Fraud and slashing.** A keeper with a fail receipt may answer within the challenge window by
  publishing the chunk and path itself. Verifying that clears the keeper and marks the receipt false,
  and the challenger forfeits. An unanswered fail stands. The bond is slashed only after **repeated**
  unanswered fails (proposed: two epochs in a rolling four), never for one, because a network
  partition can look like a missing response. The existing `node_fraud` path carries the evidence,
  generalised from snapshot fraud to retrieval fraud.
- **Reputation.** Pass rate per epoch is public (an index, like the forecaster track record), and
  placement prefers high pass rates (`HealthMonitor`). Reputation never multiplies pay.
- **No reporter reward** (operator-only fraud route ruling): challenges are derived by everyone from
  the seed, not raised by bounty hunters.

## 5. Path to consensus (each a separate, gated, owner-approved slice)

1. **Off-chain, flag-off (companion code PR core4#65):** `ArchiveCommitment` construction, the
   multi-block seed, deterministic challenge derivation, chunk-and-path verification and signed
   `RetrievalAuditReceipt`s in `keeper-proof`, behind a default-off `retrieval-audit` cargo feature. No
   transaction, no state. At this step an audit is **not replayable on chain**: nothing records the
   commitments or the holdings, so a receipt can be rechecked only by someone given the commitment
   off chain.
2. **Commitment registry:** a transaction that records `ArchiveCommitment`s and each keeper's
   per-epoch holdings, behind a new unscheduled `ConsensusFeature`, with gate-off parity. **From this
   step an audit becomes replayable on chain:** the commitment, the holdings and the seed blocks are
   all chain data, so any node can rederive every challenge and recheck every pass receipt.
3. **Keeper registration and audit receipts in settlement:** extend `node_receipt_registry` claims
   with audit receipts, and the settlement rule with the forfeit-on-fail check. Behind a feature, with
   producer / applier / WAL parity.
4. **Fraud answer and slashing:** `node_fraud` gains a retrieval-fraud variant.
5. **Wiring the real clients** (storage-providers#65, infra#944) so keepers store on the
   already-deployed IPFS daemons instead of backend's mock. Steps 2–4 do not depend on it, and a
   keeper on local disk audits the same way.

## 6. Reconciling with wrfcoin/core4-archive#1488 (Keeper in ComputeReceipt)

wrfcoin/core4-archive#1488 records keeper participation inside compute receipts. This ADR keeps keeping and computing
separate: an audit receipt proves custody of bytes, and a compute receipt proves work. A job that
reads an archived object can cite the object's `ArchiveCommitment` and the keeper's latest pass
receipt as input provenance, which is the link wrfcoin/core4-archive#1488 wants, without making the compute receipt the
audit.

## Open questions for the owner (each with a recommended answer)

1. **How are keepers paid?** *Recommended: a fee-funded pool first (uploaders pay per committed
   byte-epoch), with no emission share until the tokenomics economics are ratified.* Emission for keeping
   is a tokenomics decision.
2. **Bond.** *Recommended: reuse the node bond (250 test WRF) and the same lifecycle. A wallet that
   both serves snapshots and keeps archives posts one bond.* A separate bond can come later if
   keeping volumes justify it.
3. **Audits per keeper per epoch.** *Recommended: `k = 4`, 64 KiB chunks, a 64-block response
   window.* Four random chunks catch a keeper holding half an archive with probability 1 − 0.5⁴ ≈ 94 %
   per epoch, and the forfeit compounds across epochs.
4. **Who may challenge?** *Recommended: any bonded wallet, because challenges are derived from the
   seed, so challenging is recomputation, not selection. A false fail receipt costs the challenger
   its bond share once the keeper answers with the chunk.*
5. **Slashing threshold.** *Recommended: forfeit the epoch share on any failed audit; slash bond only
   after unanswered fails in two epochs out of a rolling four.*
6. **Which objects are archived first?** *Recommended: the block ranges that B-2 report work units
   read, so a verifier can always fetch a unit's inputs.* That makes "inputs remain retrievable" an
   audited property.
7. **Seed source.** *Recommended: the 8-block span now, and a commit-reveal beacon among bonded parties
   if the producer set is ever small enough for one party to produce eight consecutive blocks.* The
   span costs nothing to compute and is the same for every node.
