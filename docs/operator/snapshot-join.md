# Joining from a snapshot (core4#3126)

A fresh node can reach a trusted node's state height without replaying history:
fetch a **signed** snapshot, verify it **locally**, ingest it, and hand the server
a signed service claim (settlement policy remains deferred to #2848).

```text
wrfcoin snapshot fetch --from http://seed.example:8556 \
    --trusted-signers <hex ed25519 pubkey>[,<hex>...] \
    [--to http://127.0.0.1:8556] [--dry-run]
```

What the command refuses, **before any request reaches the local node**:

| Condition | Result |
|---|---|
| `signed_payload.signer_id` not in `--trusted-signers` (or the list is empty) | refused |
| payload signature does not verify | refused |
| an export page's height/root differ from the signed payload (server state moved) | refused, retry |
| exported entries do not reproduce the signed `snapshot_root_hex` | refused |

The local node must still be fresh and pin the trusted genesis through
`WRFCOIN_GENESIS_HASH`. Its `POST /network/snapshot-sync/ingest` re-verifies the
signer against its own `WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS`, checks the genesis and
carried block window against the signed tip, and root-checks the applied state.
Export pages include both state and chain entries; the snapshot commits to the
tip hash and source finalized height. Installation persists atomically before
updating the in-memory chain. A source-signed bootstrap is not an independent
BFT-finality proof; the ingest response reports `bootstrap_trust`.

## The receipt

If the local node has a snapshot signer, the ingest response carries a
`NodeServiceReceipt` signed by that key — the consumer's statement that
`served_by` delivered snapshot `snapshot_root_hex` at `height` with
`entry_count` entries. The CLI posts it to the server's
`POST /network/snapshot-sync/receipt`; the server verifies the signature, the
freshness window (±15 min) and that the receipt names *it*, records it once per
`(consumer, server, snapshot)`, and lists it at `GET /network/snapshot-sync/receipts`
(operator). A sync-only joiner without a signer gets `receipt: null` with the
reason. A valid signature proves control of the named key, not actual fetching
or application of the bytes; keys may be self-minted. Settlement must not treat
these claims as proof against self-dealing.

Routes: `GET /state/snapshot/export?offset&limit` (values, Reader),
`POST /network/snapshot-sync/receipt` (Reader), `GET /network/snapshot-sync/receipts`
(Operator). The receipt ledger is process-local operator evidence; #2848 gates
on-chain settlement.

## Export and admission bounds

The signed payload and value pages share one immutable node-local export cut.
Pages reuse that cut until the height, tip, finalized height or stored state-root
commitment changes; local mutable state at the same cut is not mixed into later
pages. A moved cut requires refetching the payload and starting again.

Receipts must name one of the last 32 exported snapshot cuts retained for up to
15 minutes: height, subject root, exact entry count and `snapshot_join` must
match. Older/unsupported/unserved claims return `422 RECEIPT_SUBJECT_NOT_SERVED`.
This small descriptor history permits a delayed receipt after height changes;
it does not retain old entry pages. A moved cut still requires restarting a fetch.

Admission budgets are 32 new receipts per authenticated bearer per 60-second
monotonic window and eight per consumer key per minute. A key can retain 16
receipts, a caller can retain 64, and the node can retain 4,096. Consumer-key
rotation cannot bypass the caller's budget; one caller cannot fill the ledger.
Here, **caller means the bearer token**, hashed in memory, not an individual
client or source address. Authentication configures one token per role, so all
clients sharing `WRFCOIN_READER_TOKEN` are **one caller**. One Reader-token holder
can exhaust the shared 32/minute budget or 64-receipt retained share, causing
other holders of that same token to receive 429. The eight/minute per-key budget
is the only per-consumer rate isolation within that shared caller; the 16-per-key
retained cap also applies, and neither reserves admission capacity. A Reader
flood preserves caller budgets for distinct other-role tokens (for example,
Operator), subject to the node-wide cap and per-key limits.
Duplicates return 409 without charging
budgets; refused admissions charge neither the caller nor the key. A full or
rate-limited window returns `429 RECEIPT_CAPACITY_OR_RATE_LIMIT`.

This is a **15-minute rolling operator evidence window**, measured monotonically
from admission, not permanent archival. No unexpired accepted evidence is evicted.
Expired receipts and their dedup/count records are removed on admission or ledger
read, so capacity recovers without restart. Window expiry also ends duplicate
suppression; a still-valid freshly signed claim can be admitted again. Operators
must collect `GET /network/snapshot-sync/receipts` with Operator authorization
before each receipt expires 15 minutes after admission and archive it externally
if they need longer retention. Reading does not extend the receipt lifetime.
The ledger and
served-cut history are scoped to the node instance; restart loses them. Durable
archival and settlement eligibility remain deferred to #2848; settlement needs
a durable sink rather than this expiring in-memory ledger. Distinct authorized
bearers can collectively consume node capacity; this is not a Sybil-proof policy.
