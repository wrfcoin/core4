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

The response is still incomplete as general wallet history. Achievement,
coinbase and implicit settlement attribution are excluded until a confirmed-paid
event source exists. Consumers must retain an incomplete-history notice and
must not infer income from reveal acceptance, estimates, or pending payouts.
