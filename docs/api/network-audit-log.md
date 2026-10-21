# Network Audit Log

`GET /network/audit-log` is the unified durable audit surface for critical network actions covered by issue `#653`.

## Included Event Kinds

- `governance_proposal_created`
- `governance_proposal_executed`
- `quality_challenge_triggered`
- `quality_slashing_applied`
- `bridge_safety_halt`
- `bridge_safety_resume`
- `bridge_challenge_opened`
- `bridge_challenge_resolved`
- `bridge_caps_updated`

## Durability And Ordering

- Records are stored in blockchain state through `StateTransition::RecordAuditEvent`.
- `event_id` is assigned monotonically in append order.
- API results are returned in `event_id` order after filtering.
- Covered records survive process restart because they live in chain state.

## Query Parameters

- `kind`
- `subject_id`
- `actor_id`
- `from_block`
- `to_block`
- `offset`
- `limit`

## Included Sources

- Governance proposal creation and successful finalization.
- Quality-policy challenge and slashing outcomes.
- Bridge safety mutations emitted by `launch_api` bridge handlers.

## Out Of Scope

- Per-vote governance audit history.
- Oracle config history outside executed governance actions.
- Signing-key lifecycle history.
- Snapshot-sync and generic system observability events.
- Historical bridge actions recorded before this audit-log surface existed.
