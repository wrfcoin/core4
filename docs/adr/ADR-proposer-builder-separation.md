# ADR: reject proposer-builder separation before scale exists (#2203)

## Decision

Reject PBS for the pre-launch/public-testnet phase. Reconsider only at a named
scale threshold: independently operated validators and multiple independent
builders demonstrably competing for the same order flow.

## Rationale

At the current scale, one practical builder would be a centralized builder with
extra relay, bid, timeout and payment machinery. Relays and builders introduce
new concentration and availability dependencies; an outage requires a local
build fallback, which is itself a different ordering path to audit. Enshrined
PBS is still evolving, so adopting it now creates a migration burden before
WRFCoin has evidence of a builder market.

For the intended goals, #2199 gives observability; an inclusion-list design
would address censorship if later justified; a batch auction can address AMM
ordering at the application layer. A professional builder market could also
make observation-ordering extraction easier, not harder.

## What this does not solve

Rejecting PBS does not prevent MEV, prove equitable ordering, or prevent a
single producer from withholding transactions. It avoids claiming that a
single-builder relay is decentralization.
