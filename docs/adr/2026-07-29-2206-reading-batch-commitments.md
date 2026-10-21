# ADR: Reading-batch commitments — #2206

**Status:** deferred until after public-testnet cut.
**Refs:** #2206, #2205, #2208, #2076.

## Decision

Do not implement an on-chain `ReadingBatch` during the closed cut. A later consensus proposal must define one canonical, versioned commitment containing the network, protocol version, station registration and device-key binding, signed monotonic reading counter, period semantics, canonical total order, count, Merkle root, and content-addressed storage commitment.

The transition must be activated/migrated explicitly. It must have deterministic positive and negative vectors across producer, P2P replay, WAL replay, and a state-root compatibility plan.

## Rationale and boundary

This changes accepted-observation state and is consensus work. A Merkle inclusion proof proves only that a reading is included. It does not prove that a producer did not omit inconvenient readings; any completeness claim needs a signed contiguous-counter/range policy.

This decision does not prove a reading truthful, a device hardware-backed, a clock honest, or off-chain data available.
