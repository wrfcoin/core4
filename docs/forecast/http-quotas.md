# Forecast HTTP request quotas

`/transactions/forecast/commit` and `/transactions/forecast/reveal` have separate
wallet-wide request budgets. They use the existing bearer-bound canonical
forecaster identity; changing station id, case or whitespace does not create a
new budget. Existing request fields, forecast data, CLI and kit are unchanged.
Cell and lead-window quotas are not implemented.

Each route defaults to 30 requests per wallet in a rolling 60-second window.
Set `WRFCOIN_FORECAST_QUOTA_LIMIT` (1–1,000) or
`WRFCOIN_FORECAST_QUOTA_WINDOW_SECS` (1–86,400) before starting the node. Missing,
zero, malformed or out-of-range values use safe defaults. The existing general
contribution commit limiter still applies and can impose a stricter limit.

After bearer validation, attempts count even if later validation or mempool
admission refuses them. HTTP 429 carries `FORECAST_QUOTA_EXCEEDED` in the existing
error envelope plus `Retry-After` seconds. At 10,000 active wallet/route keys,
new keys receive 503 `FORECAST_QUOTA_CAPACITY`; active keys are never evicted to
make room. Expired entries are reclaimed.

These are in-memory, node-local API limits. Restarts reset the counters. Other
nodes and peer transactions do not share them; this is not network-wide Sybil
protection or consensus enforcement. No validator, transaction encoding, commit
hash or activation schedule changes are part of this slice.
