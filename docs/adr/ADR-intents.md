# ADR: Intent-centric cross-chain execution

**Status:** deferred
**Decision date:** 2026-09-13
**Refs:** #2196

## Context

An intent lets a user describe an outcome instead of a route. For example, a data buyer could offer an asset on another chain, require at least a stated amount of data access, and let competing solvers choose the exchange, bridge, and settlement path.

The plausible WRFCoin-specific benefit is onboarding a buyer who neither holds WRF nor understands the chain. That demand is not yet evidenced, however. There is no measured request volume for cross-chain data purchases, no approved stablecoin or data-credit rail, and production bridges are intentionally excluded from the first public testnet. Building an intent protocol now would therefore optimize an assumed journey over a trust surface that is deliberately disabled.

## Decision

Defer the intent representation and solver interface. Do not add an `Intent` signed type, solver registry, order book, bridge route, or settlement state machine at this stage.

The smaller account-abstraction paymaster mechanism already addresses the immediate problem of a user lacking the native fee token. Current account-abstraction code binds the paymaster identity and fee cap into domain-separated authorization and enforces sponsorship policy limits. A paymaster does not exchange assets or purchase data, but it removes the prerequisite to hold WRF without introducing solver selection, cross-chain execution, or partial-settlement risk. Until real demand shows that sponsored execution is insufficient, intents would duplicate its onboarding benefit at much greater security and operational cost.

## Solver ecosystem assessment

WRFCoin does not currently have independent solvers with committed liquidity, supported routes, availability objectives, or slashable performance guarantees. Running the sole solver ourselves would not create a solver market; it would create a centralized routing service behind a more complex protocol interface. If a single operator is initially necessary, an explicit quoted-purchase service with narrow allowlists and ordinary failure semantics is easier to audit and operate.

A future proposal must identify at least two operationally independent solver candidates, the assets and routes each supports, how quotes are compared, how censorship and collusion are detected, and who bears failed-route costs. A hypothetical future ecosystem is not sufficient.

## Settlement guarantee

No safe atomic settlement guarantee exists across the currently contemplated paths. A solver can fail after one chain has finalized while the destination action remains pending, a counterparty can reorganize, or a bridge/light client can freeze. A local revert cannot undo a finalized external transfer.

Any reconsidered design must specify one of the following before implementation:

- a genuinely atomic protocol with proofs and timeout/refund behavior on every involved chain; or
- an explicitly non-atomic escrow state machine defining custody, finality, compensation, cancellation, and recovery for every intermediate state.

In either case, settlement below `want_min` must be impossible at the protocol boundary. A solver promise, compensating transaction, or eventual refund is not atomic settlement and must not be described as such.

## Security and operational boundary

An intent system would add a high-value signed message, nonce persistence, deadline and clock semantics, solver authorization, quote manipulation, external asset pricing, bridge/light-client dependencies, and potentially persisted settlement state. Those changes require signed-type registration and golden review, state migration discipline, per-path rate limits and circuit breakers, and production bridge authorization. None of those boundaries is opened by this ADR.

Intents also do not prove sensor data truthful, guarantee external-chain finality, make a centralized solver non-custodial, or remove sanctions, custody, and market-manipulation obligations from any party operating the route.

## Reconsideration gates

Reopen implementation only when all of the following are available:

1. measured demand that cannot be served by paymaster-sponsored native execution or a narrow quoted-purchase service;
2. at least two independent solver operators with concrete supported routes and failure responsibility;
3. an approved cross-chain trust model whose light-client, replay, rate-limit, circuit-breaker, and reorg controls have landed;
4. a settlement specification covering every partial-failure state and proving the `want_min` boundary;
5. explicit approval for the signed-type registry/golden and persisted-state migration work; and
6. a deployment decision made after the no-production-bridge public-testnet gate is lifted.

Until those gates are met, the successful outcome for #2196 is this recorded defer decision, not speculative protocol code.
