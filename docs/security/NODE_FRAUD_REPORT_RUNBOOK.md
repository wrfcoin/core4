# Node-fraud report route: "one node first" runbook

Refs `wrfcoin/core4-archive#3589`. Design: `docs/design/fraud-report-route.md`. This document is
**documentation only**. Nothing here has been deployed, and the owner's hold on node-share
deployment (until after the first node-share payout) still applies.

## What it is

`POST /node/fraud/submit` accepts a signed `NodeFraudReport` transaction and admits it to the mempool
and gossips it. It is a transport only: no validation or state rule changed. Consensus
(`node_fraud::apply`) alone decides whether anyone is jailed, and a false report cannot jail an
honest node because blame needs the accused party's own valid signatures. There is no reporter reward.

## Switch and gates

| Gate | Value |
| --- | --- |
| `WRFCOIN_ENABLE_NODE_FRAUD_REPORT_API` | `1`/`true` to mount the route. Default off: 404. |
| `NodeShareSettlement` | Must be active, as for `/node/receipts/submit`. Otherwise 404. |
| Operator bearer | Required (401 missing, 403 not Operator). The token only relays; it cannot forge a report. |
| Rate limit | 30 requests/minute per token (429 with `Retry-After`). Counted only after authentication. |

With the switch off, gossip-received reports are unaffected; only local submission is closed.

## One node first

1. **Pick exactly one operator node** (the owner chooses which). Leave every other node's switch off.
2. On that node alone, add `WRFCOIN_ENABLE_NODE_FRAUD_REPORT_API=1` to its environment and restart it
   through the normal operator procedure. Do not change the consensus feature schedule.
3. Confirm the closed state first on a node that was NOT changed: `POST /node/fraud/submit` returns 404.
4. Dry-run the evidence without signing anything:
   `wrfcoin node fraud report --claim <file> [--claim <file>] --sender <addr> --genesis-hash <hex> --chain-id <id> --dry-run`
   prints the report kind and `evidence_hash`.
5. Submit a real report only for evidence that is genuinely provable, using
   `--operator-token-file` (mode 0600, never argv) and `--out <packet>`. The signed packet is written
   before any network call and is never re-signed on retry.
6. Read back: `GET /node/bonds/{id}` shows `jailed` once a block carrying the report is applied.
   Block inclusion is the producer's choice; the challenge window is `epoch_close` plus 1,024 blocks on
   testnet (4,096 otherwise).
7. Judge the result, then decide separately whether to enable the switch on further nodes. One honest
   operator node is enough to reach producers by gossip.

## Responses an operator will see

| Status | Code | Meaning |
| --- | --- | --- |
| 202 | | Admitted and gossiped. Not yet included. Body lists `responsible` ids and `deadline_height`. |
| 400 | `NODE_FRAUD_PAYLOAD` | Malformed, wrong transaction type, or over 1-2 claims / 8 KiB. |
| 409 | `NODE_FRAUD_ALREADY_SANCTIONED` | Every responsible deployment is already jailed or has a terminal for the epoch. |
| 422 | `NODE_FRAUD_AUTHORIZATION` / `_EXPIRED` / `_UNPROVABLE` / `_REFUSED` | Bad reporter signature, past the deadline, not provable against current state, or mempool refusal. |
| 429 | `RATE_LIMITED` | Over 30/min for this token. |
| 503 | `NODE_FRAUD_ARCHIVE_UNAVAILABLE`, `NODE_SERVICE_INACTIVE` | Archive write failed (nothing admitted), or node service state absent. |

The 422/409 checks are advisory at the edge and use the consensus `responsible()` function read-only.
A report this node accepts can still be a no-op at apply time if state moves first.

## Rollback

Unset the variable and restart that node. The route returns to 404. Reports already admitted or
gossiped remain in the mempool and chain by the normal rules; this runbook adds no way to withdraw them.

## Out of scope

Reporter reward, a dedicated V2 signing domain, a mempool-side `responsible()` check, and producer
ordering by deadline are all unchanged and need separate owner-approved designs.
