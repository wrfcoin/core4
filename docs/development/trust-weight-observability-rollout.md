# Trust-Weight Governance + Observability Rollout

This rollout adds API/indexer-facing trust-weight governance and reward-impact observability for issue #437.

## Governance Guardrail Endpoints

- `GET /governance/trust/weights`
  - Returns current trust policy, `[0,1]` bounds, and ordering guardrails.
- `POST /governance/trust/proposals/build`
  - Builds a protocol-compatible governance payload preview and validates:
    - bounds (`0 <= weight <= 1`)
    - ordering (`verified_station >= unverified`, `trusted_provider >= verified_station`)

Example:

```bash
curl -s http://127.0.0.1:8556/governance/trust/proposals/build \
  -H 'content-type: application/json' \
  -d '{"target":"verified_station_weight","value":0.9}' | jq
```

## Trust Reward Impact Endpoints

- `GET /transactions/rewards/trust/timeline`
  - Per-block trust-class distribution, quality-band counters, parsed multipliers, and joint matrix.
- `GET /transactions/rewards/trust/summary`
  - Aggregated trust/quality counters and weighted multiplier estimates.

Example timeline query:

```bash
curl -s "http://127.0.0.1:8556/transactions/rewards/trust/timeline?from_block=10&to_block=200&limit=50" | jq
```

Example summary query:

```bash
curl -s "http://127.0.0.1:8556/transactions/rewards/trust/summary?from_block=10&to_block=200" | jq
```

## Metrics Surface

- `GET /metrics` now includes:
  - `trust_impact_blocks_observed`
  - `trust_class_distribution_total`
  - `trust_weighted_multiplier_estimate`
  - `quality_weighted_multiplier_estimate`
  - `effective_weighted_multiplier_estimate`
  - `current_trust_weights`
  - `latest_trust_impact_block_height`

Example:

```bash
curl -s http://127.0.0.1:8556/metrics | jq '.data'
```
