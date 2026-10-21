# Reading revealed station observations

`GET /transactions/contributions/{station_id}/observations` requires a Reader
token (Contributor, Oracle and Operator also satisfy Reader in the existing role policy). This node route
does not add any public ingress allowlist entry.

The response uses the normal API envelope. `data.items` contains committed
`ContributionReveal` weather observations for exactly the requested station:
commit id, station id, block height, reveal timestamp (`observed_at`), six parsed
weather values with canonical units, provenance class and attestation summary.
Only temperature (C), humidity (%), pressure (hPa), wind speed (m/s), precipitation
(mm), and dewpoint (C) are exposed. Optional absent values are null. Arbitrary
payload fields, radio metadata, owner addresses and balances are not returned.
Forecast and non-weather reveals are excluded. This is a weather projection,
not a byte-for-byte copy of the commit preimage.

Provenance is the accepted reveal/reward snapshot, never a caller-supplied
provenance claim. Attestation approvals/quorum reflect retained chain state at
read time, not necessarily at reveal height. Either metadata field is null if
its retained record is unavailable. A quorum does not assert BFT finality.

Start with `?after=0&offset=0&limit=50`. `after` is the last fully scanned block
height; `offset` is the next transaction index in `after + 1`. Copy **both**
`next_after` and `next_offset` into the next request. This preserves observations
when a single block spans multiple pages. `limit` is clamped to 1..100 items.
Each request scans at most 64 blocks and 1000 transactions, so an empty page with
`has_more: true` is valid for a sparse station. Continue until `has_more` is false.
The response reports `chain_height`, `state_tip` and scanned transaction count.

Reads use in-memory blocks and fall back to persistent block history. A missing
or unreadable requested block returns 503, rather than silently skipping data.
Malformed cursors return 400. A cursor beyond the current tip returns an empty
page. Pagination is over the node's current chain, not an immutable snapshot:
consumers should reconcile their observations if that chain reorganizes.

No station ownership lookup or account data is needed. No deployment or public
endpoint publication is implied by adding this Reader-gated node route.
