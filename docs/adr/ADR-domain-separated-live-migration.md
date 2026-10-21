# ADR: hold live DomainSeparated migration for the next coordinated cut (#2226)

## Decision

Do not migrate any remaining live signing path in this cut. The additive
preimage definitions are valuable, but switching a verifier invalidates legacy
signatures and is consensus-breaking.

## Remaining coupled work

The live paths still requiring an activation checklist are transaction signing,
finality signals (which have no authenticated wire form today), slashing and
snapshot payloads, oracle/calibration attestations, and bridge messages. Each
needs a producer/signer and every gossip, IBD, persistence, WAL/replay,
historical-verification and cross-repo client consumer migrated at one explicit
height/version. A mixed deployment repeats the #2076 signer/verifier halt.

## Timing and non-goals

Schedule only with a new coordinated genesis/reset decision, full fixture and
golden-vector inventory, and compatibility tests across all live callers. The
public testnet has no real monetary value or production bridges (#2158), so
there is no launch justification to take this risk now. This ADR does not
authenticate finality signals, repair legacy preimages, or make an additive
`DomainSeparated` implementation live.
