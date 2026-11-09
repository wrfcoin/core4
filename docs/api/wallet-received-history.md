# Address transaction and received-credit history

`GET /addresses/{address}/transactions` requires the Reader role. It is a
read-only projection; it does not apply transitions or change consensus state.

The response keeps the existing `transactions`, `limit`, `offset`,
`total_in_window`, `window_start_height` and `window_limited:true` fields.
Transactions remain ordered by descending retained block height and reverse
transaction position. `limit` defaults to 20 and is clamped to 1–100.

Address-specific transaction rows add `event_id` (`transaction:<tx_id>`),
`direction`, `counterparty`, `from`, `to`, `amount_base_units`, `kind` and
`settlement`. Supported transparent transfers use the parsed payload's canonical
sender/recipient addresses and decimal-string integer amount. Directions are
`received`, `sent`, or `self` (no net-income claim); settlement is `included`.
Unsupported kinds keep direction `unknown` and nullable amount/counterparty.
The existing `sender` compatibility field is preserved. Payout transaction
outputs are not promoted to confirmed income merely because they were included.

Transfer membership uses canonical payload parties for positive opted-out
transfers. This intentionally differs from the prior legacy-sender OR recipient
filter: a legacy sender inconsistent with valid payload parties is excluded,
and a non-sender recipient of a zero/non-opted-out/invalid payload is excluded.
Unsupported payloads still match the legacy sender, with unknown direction and
amount. Consequently `total_in_window` can differ for those edge-case rows.

The additive `credit_events` section contains canonical positive Real
contribution/forecast records whose commit ID is in
`finalized_contribution_rewards`. This membership establishes that the amount
was actually credited once. Pending, synthetic, unclassified and zero decisions
are excluded. The amount is the canonical **paid** amount, which can be smaller
than a nominal decision in block metadata. A commit appears once regardless of
repeated observations. State-backed records remain available when their blocks
have left the retained transaction window.

Each credit has `event_id` (`contribution:<commit_id>` or `forecast:<commit_id>`,
stable while its canonical commit ID remains unchanged),
`direction:received`, `to`, `amount_base_units` (u128 decimal string),
`kind:weather_reward|forecast_reward`, and `settlement:paid`. Its `tx_id`,
`sender`, `counterparty`, `block_height` and `timestamp` are null: the state
record does not establish a transaction hash or payment height/time. The payee
is the recorded beneficiary, not the current station owner or reveal sender.

Credits have independent bounded keyset pagination: `credit_limit` defaults to
the transaction limit and is clamped to 1–100; `credit_after` is an exclusive
commit ID cursor used to seek via `BTreeMap::range`. At most `credit_limit+1`
records are examined **before** beneficiary/provenance filtering; the extra
record only detects whether another scan page exists. `credit_next_cursor` is
the last processed record key when more records remain, including on an empty
credit page. Clients must follow this cursor rather than stop at an empty page.
`credit_records_examined` and `credit_scan_limit` report bounded work, not a
global income count. No `credit_total` is computed. JSON credit projection runs
after releasing the chain read lock. `credit_order` is
`commit_id_ascending`, **not chronological**. This allows clients to show a
separate paid-rewards section without inventing settlement dates. Cursors are
positions in the current canonical state, not frozen snapshot handles; clients
should reset pagination on refresh/reorg.

## Credits by address

`GET /addresses/{address}/credits` requires the Reader role and returns the same
credit rows, order and cursor as `credit_events` above, but a full page of this
address's credits rather than whatever fell in the next chain-wide records.
`credit_limit` defaults to 50; 0 or above 100 is refused, not clamped. `credit_after` is the same
exclusive commit ID cursor (at most 256 bytes), so a walk can move from one route
to the other. `credit_next_cursor` is null only when no further paid credit is
known. A page can be short with a cursor if the read reached
`credit_keys_examined_limit` (1000 keys checked); follow the cursor as usual.

The route reads an in-memory index that lives outside consensus and is rebuilt
from state: at startup, and within about 10 seconds of the tip moving
(`index_height` is the height it was built from). Until the first build ends the
route answers `503 REWARD_INDEX_WARMING`; the transactions route keeps working.
Every row is re-checked against current state when read, so the amount and paid
status are current, and a rollback or snapshot sync cannot show a row state does
not hold. A new credit can be missing until the next rebuild. A credit paid during
a walk with a commit ID behind the cursor appears on the next walk from the start,
as with the transactions route.

### Newest first (`order=newest`)

`GET /addresses/{address}/credits?order=newest` returns the same rows, plus
`payout_height`, newest first: `credit_order` is `payout_height_descending`
(ties by commit ID descending). `credit_limit` is as above. `credit_after` takes
only a `credit_next_cursor` from an `order=newest` response; it is opaque, and
anything else (including a commit-ID cursor from the default order) is a `400
VALIDATION_ERROR`. Any other `order` value is a 400 too. Without `order` the
route is exactly as described above.

Example:

```json
{
  "address": "0x…",
  "credit_events": [
    { "event_id": "contribution:<commit_id>", "to": "0x…",
      "direction": "received", "amount_base_units": "5000000",
      "kind": "weather_reward", "settlement": "paid", "payout_height": 31207,
      "tx_id": null, "sender": null, "from": null, "counterparty": null,
      "tx_type": null, "block_height": null, "timestamp": null }
  ],
  "credit_limit": 50,
  "credit_next_cursor": "n1.30940.<commit_id>",
  "credit_order": "payout_height_descending",
  "credit_keys_examined": 51,
  "credit_keys_examined_limit": 1000,
  "index_height": 31210
}
```

`payout_height` is the latest block whose `reward_entries` metadata names the
commit, which is the block that paid it. The index learns it by reading each
block once (from disk at startup, then each new block), not from state. It is
null when this node does not hold that block (a node started from a snapshot);
those credits come after every dated credit, by commit ID descending.
`block_height` stays null as above, so a client that only reads the old fields
sees no change.

The cursor is a position `(payout_height, commit_id)`. A credit paid after it
was handed out has a higher payout height, so it appears at the top of a fresh
walk and never in, or ahead of, the rest of a walk already under way. A reorg
of recent blocks re-dates the credits in the replaced blocks. A credit paid in a
block the index has not scanned yet waits for the next rebuild (about 10
seconds) rather than being shown undated. On startup the first build also reads
the stored blocks, so `503 REWARD_INDEX_WARMING` can last longer than before.

Memory is about 270 bytes per indexed credit plus about 150 bytes per address
(each commit ID is stored once and shared), so 100,000 credits use about 27 MB.

The response is still incomplete as general wallet history. Achievement,
coinbase and implicit settlement attribution are excluded until a confirmed-paid
event source exists. Consumers must retain an incomplete-history notice and
must not infer income from reveal acceptance, estimates, or pending payouts.
