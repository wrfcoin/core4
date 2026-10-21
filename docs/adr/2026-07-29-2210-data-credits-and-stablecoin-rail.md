# ADR: Data credits and stablecoin rail — #2210

**Status:** do not build for public testnet; reconsider post-cut only.
**Refs:** #2210, #2158.

## Decision

Do not attach a stablecoin, redeemable credit, rate feed, custody path, or production bridge to public testnet. A non-transferable label alone is not enough when a credit is redeemable for valuable data or transferable indirectly through accounts.

Any later economic proposal must name issuer/redemption and custody ownership; immutable asset/bridge and oracle allowlists; rate freshness, source quorum, and fail-closed behavior; sponsored-gas payer and limits; purchase/refund/access states; supply reconciliation; and compromise/rollback ownership.

## Boundary

This cannot prove purchased data truthful, protect a compromised rate/payment authority, or solve the hardware-provenance assertion that presently determines reward outcome.
