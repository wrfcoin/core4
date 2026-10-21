# ADR: Cross-repository contract interface verification — #2450

**Status:** blocked pending ownership decision.
**Refs:** #2450, #2447, #2442.

## Decision

Do not restore the stale local `contracts/` workspace or add a speculative CI gate. Before implementation, the smart-contract producer owner must publish an authenticated immutable pin comprising producer commit, ABI digest, and reproducible-build digest. The consumer owner must own reviewed pin bumps.

The future check must fail closed with distinct, actionable outcomes for missing, unreadable, unauthenticated, stale, and mismatched artifacts. It must document how producer publication is authenticated and how the consumer fetches it.

## Boundary

ABI compatibility does not prove a deployed address is correct, prevent a compromise of an approved artifact producer, or replace deployment-specific attestation.
