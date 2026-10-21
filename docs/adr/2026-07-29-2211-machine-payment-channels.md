# ADR: Machine payment channels — #2211

**Status:** do not build for public testnet; reconsider post-cut only.
**Refs:** #2211, #1443, #2158.

## Decision

Do not introduce machine-to-machine payment channels on public testnet. They require consensus-visible deposits, cumulative vouchers, nonce/replay rules, unilateral close, dispute timing, settlement, and migration. No real-value channel, stablecoin, or production bridge may be attached to this testnet.

A future proposal must first close #1443's canonical signature-validation gap, then specify domain-separated voucher bytes, session-key authorization/revocation, supply/fee conservation, finality/reorg behavior, timeout clock, watchtower availability, and every failure state.

## Boundary

Channels do not make sensor data truthful, stop a seller from serving bad data, stop buyer-side exfiltration after delivery, or solve device provenance.
