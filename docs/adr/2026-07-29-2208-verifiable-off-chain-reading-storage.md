# ADR: Verifiable off-chain reading storage — #2208

**Status:** deferred until after public-testnet cut.
**Refs:** #2208, #2206, #1488.

## Decision

Do not treat a CID as a durability proof and do not implement a durability/reward gate in the closed cut. A later design must bind canonical serialized bytes, hash algorithm, size, and retention policy into #2206's versioned commitment.

Durability must be an evidence state, not a producer assertion: authenticated provider identities, independent replica receipts, unpredictable retention challenges, cadence, timeout, verifier, repair/migration, and `durable` / `not-yet-durable` / `availability-unknown` outcomes are required.

## Rationale and boundary

Content addressing proves retrieved bytes match a commitment; it does not prove a provider has them now or can recover them during a correlated outage. A replica list can name colluding or nonexistent providers. Reward eligibility or accepting a batch as durable changes consensus-visible state and must wait for an explicit consensus decision.

This design does not prove reading truth, prevent pre-commit withholding, prevent provider collusion, or guarantee availability.
