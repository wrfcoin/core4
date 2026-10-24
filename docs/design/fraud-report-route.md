# Fraud-report route and CLI: design for ratification

Status: **Accepted (owner, 2026-10-24).** Design only. No route, CLI, schema, domain tag,
consensus feature or deployment is implemented or authorized by this document. Implementation and
deployment land in separate PR(s) (§9); see "Decision" at the end. The owner has also ruled (2026-10-18) that node-share
deployment is held until after the first node-share payout.

Refs: `wrfcoin/core4-archive#3589` (private backlog). It reports that a `NodeFraudReport` is accepted
by the mempool and applied deterministically, but no HTTP route submits one. Jailing, the main
enforcement lever against a dishonest node-service server or consumer, therefore cannot be
triggered by any participant on the live network. It asks for an Operator-gated
`POST /node/fraud/submit` that gossips, a CLI `node fraud report`, and tests showing identical
`state_root` on producer, applier and WAL replay.

## What already exists (public main `bde93b2`)

The consensus half is built. Only submission is missing.

| Piece | Where | Behaviour |
|---|---|---|
| Report type | `shared-protocol/src/consensus/node_fraud.rs:28` | `pub type NodeFraudReport = NodeReceiptSubmission`: one or two `BondedServiceClaim`s plus a reporter authorization. |
| Reporter signature | `node_fraud.rs:60-86`, `node_receipt_registry.rs` | The reporter signs a `NodeReceiptSubmissionAuthorization` (`wrfcoin.node_receipt_submission.v1`, V2 domain signing) over `genesis_hash, sender, nonce, batch_hash`. For a fraud report, `batch_hash` must equal `evidence_hash(p)`, a hash over the literal purpose label `wrfcoin.node_fraud_report.v1` and the claims. A receipt-submission signature therefore cannot be replayed as a fraud report. The sender must equal the address derived from the key. |
| Transaction parse | `node_fraud.rs:43-58` | `TransactionType::NodeFraudReport` with hex `data` ≤ 2 × `MAX_NODE_FRAUD_BYTES` (8 KiB), 1–2 claims, and `authorization.sender == tx.sender_address` and `authorization.nonce == tx.nonce`. |
| Mempool admission | `shared-protocol/src/mempool/pool.rs:455-466` | Runs the admission-cost check, then the `NodeShareSettlement` activation gate, `parse` and `verify_authorization`. It does **not** check the bond registry, the challenge deadline, provability or duplicates. The comment above it ("This type-only slice cannot stage or apply evidence reports yet.") predates the apply path below. |
| Blame rules | `node_fraud.rs:228-284` (`responsible`) | **One claim:** the server's ticket signature over a payment assertion is valid, but the consumer's receipt signature is not. This blames the server only; the named consumer is never blamed. **Two claims:** the same epoch, server, consumer and ticket id with different signed receipt or manifest hashes (equivocation). This blames the server if it signed both tickets, and the consumer if it endorsed both. Anything else is refused. |
| Freshness | `node_fraud.rs:188-225` (`binding`) | Refused after `epoch_close + node_challenge_blocks`: 1,024 blocks on testnet, 4,096 by default otherwise (`node_share_contracts.rs:22-49`). Every committed context (genesis, chain, network, roster sequence/hash, epoch parent, receipt/manifest hashes) must match the frozen epoch. |
| Sanction | `node_fraud.rs:287-405` (`apply`) | The responsible deployment's whole escrowed bond moves to the non-spendable forfeiture account, its principal is zeroed, and it is added to `jailed` (permanent until a separately reviewed recovery). The terminal record `(epoch, id) → evidence hash` is kept. An already-jailed id is a no-op. |
| Block rules | `node_fraud.rs:24`, `:408-440` | At most `MAX_NODE_FRAUD_REPORTS_PER_BLOCK = 8`. Every carried report must map 1:1 to an applied transition. |
| Reporter preconditions | `apply` | The reporter is an existing account with non-zero spendable balance, signs as itself (`SignerIsSender`), is not a node escrow and is not the forfeiture address. The nonce is consumed. **There is no reporter reward.** |
| Read-back | `native-chain/src/bin/launch_api/routes/node_bonds.rs` | `GET /node/bonds/{id}` already reports `jailed`. |
| Sibling routes | `native-chain/src/bin/launch_api/routes/{node_bonds,node_receipts}.rs`, `routes/mod.rs:305-317` | `POST /node/bonds/submit` and `POST /node/receipts/submit` follow one pattern. They are `require_role(.., ApiRole::Operator)` (`helpers.rs:345`), have a 40 KiB JSON body limit, are guarded by `node_receipts_configured` (`NodeShareSettlement` active), and admit via `add_transaction_with_signing_keys_at_height`. After admission they gossip with `queue_transaction_gossip` (`helpers.rs:1228`; best-effort, metered). They return `202 {"tx_id","admitted":true,"included":false}`. |

Not to be confused with `SlashEvidenceReport` (`wrfcoin.slash_evidence_report.v1`), which is the
validator-slashing path and unrelated to node-service bonds.

## 1. Who can report

The rules are already fixed by consensus. Any funded, self-signing account may submit a provable
report. Nothing in `apply` restricts the role. The route adds a **transport** gate only:

- **Proposed:** `POST /node/fraud/submit` requires `ApiRole::Operator`, matching the sibling
  node-share submit routes. The Operator token belongs to the node's operator. The report itself
  must still be signed by the reporter's own account key. The bearer token does not authorize the
  report; it only lets that node accept and relay it.
- Because admitted reports are gossiped, **one** honest operator node is enough to reach the
  producers. A reporter without an Operator token must ask an operator to relay, which is open
  question Q2.
- Natural reporters:
  - a **consumer** holding a server ticket that asserts a payment the consumer never signed (the
    one-claim case);
  - **anyone** holding two conflicting claims for the same ticket, for example from the durable
    public receipt archive (`snapshot fetch --bonded --receipt-archive`; the equivocation case).

## 2. What evidence a report carries, and the domain tag

**Evidence:** one or two `BondedServiceClaim` wire objects, exactly as signed by the server
(`wrfcoin.node_service_ticket.v1`), the consumer (`wrfcoin.node_service_receipt.v1`,
`wrfcoin.node_settlement_endorsement.v1`) and their manifest. The reporter adds nothing to the
evidence except the authorization. Claims are self-authenticating, so the report carries no
free-text accusation, no reporter-chosen target and no off-chain reference.

**Reporter authorization, today:** the reporter reuses `NodeReceiptSubmissionAuthorization`
(`wrfcoin.node_receipt_submission.v1`). It is separated from a real receipt submission by the
purpose label `wrfcoin.node_fraud_report.v1` inside the committed `batch_hash`. That label is a
**hash-body label, not a registered signing domain**. It is safe because `verify_authorization`
recomputes `evidence_hash` and requires equality, but the separation lives one level below the
domain tag, where the registry tests (`docs/security/DOMAIN_TAGS.md`) cannot see it.

**Proposed new tag (not implemented):** `wrfcoin.node_fraud_report_authorization.v1` for a
dedicated `NodeFraudReportAuthorization` (same fields: `genesis_hash, sender, nonce,
evidence_hash`). It deliberately differs from the existing hash label, so the two strings are never
confused. This would:

- move the separation into the domain registry, where `all_domain_tags_are_unique`, the injectivity
  tests and `signed_type_inventory` enforce it;
- remove the coupling where a change to receipt-submission authorization silently changes the
  fraud-report preimage.

It is a **signing-preimage change, so it is consensus-breaking**. Following the existing pattern
(`docs/adr/2026-10-19-3290-forecast-identity-binding.md`), it needs:

- a separately discriminated V2 payload and a proposed `ConsensusFeature`, unscheduled and
  non-schedulable until wired;
- cross-language golden vectors;
- V1 accepted until activation and refused after it, with no in-place change to V1.

**The route does not need it.** The route can ship on the existing V1 encoding, and the tag can
follow later or never (Q4).

## 3. How the node validates a report at the route

The mempool deliberately checks only authorization. A route that admitted every authorized report
would let anyone fill the mempool with reports that can never apply. Proposed route pipeline, in
this order and failing closed:

1. `require_role(Operator)`. Body limit 40 KiB JSON (the report wire cap is 8 KiB). The route is
   only mounted when `NodeShareSettlement` is active **and** the runtime switch below is on.
2. Parse the `Transaction` and require `tx_type == NodeFraudReport`, then call
   `node_fraud::parse`. Failure: 400 `NODE_FRAUD_PAYLOAD`.
3. `verify_authorization` against the node's chain identity and bond-registry genesis. Failure:
   422 `NODE_FRAUD_AUTHORIZATION`.
4. **Dry-run blame against current state** (read lock, no mutation): call
   `node_fraud::responsible(state, p, tip_height + 1, identity)`. This reuses the consensus
   function, not a copy. Failures:
   - past the challenge deadline: 422 `NODE_FRAUD_EXPIRED`;
   - context mismatch or not provable: 422 `NODE_FRAUD_UNPROVABLE`.

   The route returns those two codes by re-checking the deadline separately for a precise
   message.
5. **Duplicate check:** if every responsible id is already in `node_fraud.jailed`, or the
   `(epoch, id)` terminal already exists, return 409 `NODE_FRAUD_ALREADY_SANCTIONED`. Consensus
   would apply it as a no-op that still uses one of the 8 per-block slots.
6. Archive the exact transaction bytes (as `node_receipts::submit` does, keyed by hash). This is
   for the reporter's and the operator's records. Failure: 503 `NODE_FRAUD_ARCHIVE_UNAVAILABLE`.
7. Admit with `add_transaction_with_signing_keys_at_height(tx, None, Some(tip_height))`. Failure:
   422 `NODE_FRAUD_REFUSED`.
8. Gossip with `queue_transaction_gossip`. Respond `202 {"tx_id","admitted":true,"included":false,
   "responsible":[<deployment ids>],"deadline_height":<h>}`.

Steps 4–5 are **advisory at the edge**. Consensus (`apply`) remains the only authority. A report
the route accepts can still be refused at apply time if state moves, for example when another
report lands first. Peers that receive it over gossip apply the same consensus rules.

**Runtime switch (rollout):** `WRFCOIN_ENABLE_NODE_FRAUD_REPORT_API`, default **off**. This follows
the existing `WRFCOIN_ENABLE_*_API` convention (for example `WRFCOIN_ENABLE_ORACLE_ADMIN_API`). With
the switch off the route is not mounted (404), and gossip-received reports are unaffected.

## 4. Rate limits and anti-spam

A false report **cannot jail anyone**: blame requires the accused party's own valid signatures. The
spam risks are resource use and slot crowding:

| Vector | Existing control | Proposed addition |
|---|---|---|
| Junk bytes | 40 KiB route body limit; 8 KiB report wire cap; mempool `max_transaction_size` | none |
| Signature-verification CPU | Operator gate; `verify_cost` prices a report at `1 + 4 × claims` units (the fee per unit defaults to 0) | Per-token route limiter (proposal: 30/min/token; to be tuned) |
| Mempool flooding with authorized-but-unprovable reports | Per-sender cap (`mempool_max_txs_per_sender`, default 1024) | Route dry-run (step 4) refuses them before admission. Q3 asks whether the **mempool** should also run `responsible()`, since gossip bypasses the route. |
| Crowding the 8-per-block cap to push a real report past its deadline | Block cap; producer selection | Route duplicate refusal (step 5); producer-side ordering by earliest `deadline_height` (Q5) |
| Producer withholding | none at the route; the challenge window is 1,024+ blocks on testnet | Gossip from every operator node plus producer rotation (out of scope here; the infra producer-rotation work) |

## 5. Interaction with jailing (`node_fraud` state; read only)

- Jailing is **permanent**: the whole bond is confiscated to the non-spendable forfeiture account.
  A recovery path is explicitly a separate, owner-authorized design. The route adds no new
  sanction, partial penalty or appeal.
- The route never mutates state. All effects happen in `apply` inside block execution, so producer,
  applier and WAL replay derive the same `state_root` (the test plan below asserts this).
- Missing work is **not** fraud (ADR-003). The route has no "liveness" or "non-delivery" report
  kind, and adding one is out of scope.
- `GET /node/bonds/{id}` already exposes `jailed` for read-back. No new read route is needed.

## 6. API shape

```
POST /node/fraud/submit
Authorization: Bearer <Operator token>
Content-Type: application/json
Body: Transaction (tx_type = "NodeFraudReport", data = hex(NodeFraudReport wire), signed by the reporter)

202 {"tx_id":"<hex>","admitted":true,"included":false,"responsible":["<deployment id>",...],"deadline_height":<u64>}
400 NODE_FRAUD_PAYLOAD               malformed transaction or report
401/403                              missing token / not Operator
409 NODE_FRAUD_ALREADY_SANCTIONED    every responsible deployment already jailed for this epoch
422 NODE_FRAUD_AUTHORIZATION | NODE_FRAUD_EXPIRED | NODE_FRAUD_UNPROVABLE | NODE_FRAUD_REFUSED
429                                  route rate limit
503 NODE_FRAUD_ARCHIVE_UNAVAILABLE | NODE_SERVICE_INACTIVE
```

Errors use the existing `envelope_error` shape. The route echoes no evidence bytes and no keys.

## 7. CLI shape

Under the existing `wrfcoin node` group (`tools/cli/WrfcoinCLI/src/commands/node.rs`), alongside
`node bond` and `node receipt`:

```
wrfcoin node fraud check  --claim <file> [--claim <file>] --api-url <url> [--reader-token-file <f>]
wrfcoin node fraud report --claim <file> [--claim <file>]
                          --sender <addr> --genesis-hash <hex> --chain-id <id> [--network-id testnet]
                          (--key-file <f> | --mnemonic-stdin) --operator-token-file <f>
                          --api-url <url> [--fee <units>] [--out <packet>] [--dry-run]
```

- `--claim` takes `BondedServiceClaim` wire files, as kept in the durable receipt archive. One file
  means the forged-consumer case; two files the equivocation case. The CLI infers which and prints
  it.
- `check` is local and read-only. It verifies the signatures it can verify offline, fetches
  `GET /node/bonds/{id}` for the named server and consumer (jailed? epoch frozen?), and prints the
  expected blame and deadline. It signs nothing.
- `report` follows `node receipt submit`:
  1. fetch the nonce;
  2. build the authorization with `batch_hash = evidence_hash`;
  3. sign with the reporter key;
  4. write the signed packet to `--out` **before** any network call (durable, never re-signed on
     retry);
  5. POST it and read back inclusion and `jailed`.
- `--dry-run` stops after step 2 and prints the packet hash.

## 8. Threat model

| Threat | Mitigation |
|---|---|
| False report to jail an honest node | Impossible by construction: `responsible()` requires the accused's own valid signatures (a server ticket signature, a consumer endorsement), and the one-claim case never blames the consumer. |
| Framing a consumer with a forged consumer signature | One-claim case blames only the server whose ticket signature is valid. Two-claim case needs two valid consumer endorsements. |
| Griefing by flooding reports | Route dry-run and duplicate refusal; per-token rate limit; mempool per-sender cap; Q3 (mempool-side provability). |
| Replay of a receipt-submission signature as a report | `batch_hash` must equal `evidence_hash`, which commits the purpose label. The proposed dedicated tag would move this into the registry. |
| Replay of a report (same tx) | Account nonce consumed. The same evidence from another reporter is a deterministic no-op (refused at the route with 409). |
| Cross-chain or cross-network replay | Domain signing binds chain and network identity; the evidence binds genesis, chain, network, roster and epoch parent. |
| Stale evidence | Deadline `epoch_close + challenge_blocks`, enforced at apply; the route refuses early with 422. |
| Producer withholding a valid report until the deadline | Gossip from every operator node; producer rotation (infra track); per-epoch challenge window. Inclusion lists are deferred (`docs/adr/ADR-inclusion-lists.md`). |
| Operator token theft | The token can only relay reporter-signed reports; it cannot forge one. Normal token rotation applies. |
| Privacy | Evidence is already public node-service material (claims, tickets). The route adds no location or personal data. |

## 9. Rollout

1. Ratify this design (questions below).
2. Implementation PR(s), all behind `WRFCOIN_ENABLE_NODE_FRAUD_REPORT_API=0`:
   - **Route tests:**
     - happy path for each case: admitted, gossiped, applied, then identical `state_root` on
       producer, applier and WAL replay;
     - expired (422), unprovable (422), duplicate (409), wrong role (403), oversize (413);
     - switch off (404).
   - **CLI:** golden packet; `--dry-run` writes no network traffic; a retry reuses the packet.
3. Fix the stale `pool.rs:457` comment in the same PR. This is a comment-only change.
4. Deploy only after the owner's node-share payout hold lifts. Enable the switch on one operator
   node, then on all.
5. Optional, separately ratified: the dedicated domain tag (§2) and a mempool-side provability
   check (Q3). Both are consensus-adjacent and follow the feature-gate pattern.

## Open questions for owner ratification

1. **Ratify the route** as specified: Operator-gated, gossiping, advisory dry-run, switch default off.
2. **Who may submit without an operator?** Keep Operator-only (reporters ask an operator), or also
   accept `NodeFraudReport` on the permissionless submit path with its existing rate limits? The
   permissionless path's handling of this transaction type was not verified for this draft.
3. **Mempool provability:** should the pool itself run `responsible()` (stateful admission), so
   gossiped junk is dropped too? This is consensus-adjacent and needs its own review.
4. **Dedicated domain tag** `wrfcoin.node_fraud_report_authorization.v1`: adopt as a later
   feature-gated V2, or keep the current purpose-label separation?
5. **Producer ordering:** should block builders prefer reports with the earliest deadline when more
   than 8 are pending?
6. **Reporter incentive:** consensus pays no bounty, and confiscated bonds go to a non-spendable
   account. Is "no reward" intended (it avoids incentives to manufacture disputes), or should a
   bounty be designed separately?
7. **Rate-limit numbers** (proposal 30/min per Operator token) and the deployment order after the
   payout hold.

## Decision (accepted 2026-10-24)

Owner ruling, 2026-10-24: "Merge as accepted, recommended answers." Each open question gets the
answer this document itself recommends, quoted with its line. Where the document makes no
recommendation, the answer is **UNANSWERED — owner**: none was invented.

Ratifying the design authorizes no deployment by itself: per §9, implementation lands in separate
PR(s) behind the default-off switch, and deployment waits for the node-share payout hold.

1. *Ratify the route as specified (Operator-gated, gossiping, advisory dry-run, switch default
   off)?* **Ratified as specified in §1, §3, §6 and §7.** Recommended by the document: line 41:
   "**Proposed:** `POST /node/fraud/submit` requires `ApiRole::Operator`"; line 120: "Steps 4–5 are
   **advisory at the edge**."; line 124: "**Runtime switch (rollout):**
   `WRFCOIN_ENABLE_NODE_FRAUD_REPORT_API`, default **off**.".
2. *Who may submit without an operator: Operator-only, or also the permissionless submit path?*
   **Operator-only, as proposed. A reporter without an Operator token asks an operator to relay.**
   Recommended by the document: line 41: "**Proposed:** `POST /node/fraud/submit` requires
   `ApiRole::Operator`"; line 46: "A reporter without an Operator token must ask an operator to
   relay". The permissionless path is not part of the proposal, and the draft did not verify it
   (line 235: "handling of this transaction type was not verified for this draft").
3. *Mempool provability: should the pool itself run `responsible()`?* **UNANSWERED — owner.**
   Relevant text (not a recommendation): line 237: "consensus-adjacent and needs its own review";
   line 227: "Optional, separately ratified".
4. *Dedicated domain tag `wrfcoin.node_fraud_report_authorization.v1`: later feature-gated V2, or
   keep the purpose-label separation?* **UNANSWERED — owner.** Relevant text (not a recommendation):
   line 88: "follow later or never (Q4)". The route ships on the existing V1 encoding either way.
5. *Producer ordering: prefer reports with the earliest deadline when more than 8 are pending?*
   **Yes: producer-side ordering by earliest `deadline_height` (listed under "Proposed addition").
   Block-builder policy; implementation requires a separate PR + owner deploy.** Recommended by the
   document: line 133: "| Vector | Existing control | Proposed addition |"; line 138: "producer-side
   ordering by earliest `deadline_height` (Q5)".
6. *Reporter incentive: is "no reward" intended, or should a bounty be designed separately?*
   **UNANSWERED — owner.** Relevant text (not a recommendation): line 243: "(it avoids incentives to
   manufacture disputes)". The rationale is offered inside the question, not as a recommendation.
7. *Rate-limit numbers and deployment order after the payout hold?* **30 requests/min per Operator
   token, to be tuned. Deploy only after the payout hold lifts: one operator node first, then all.**
   Recommended by the document: line 136: "Per-token route limiter (proposal: 30/min/token; to be
   tuned)"; line 225: "Deploy only after the owner's node-share payout hold lifts"; line 226: "node,
   then on all.".
