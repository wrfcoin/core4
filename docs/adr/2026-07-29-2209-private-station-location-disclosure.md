# ADR: Private station-location disclosure — #2209

**Status:** ADR first; no reward-policy implementation during the closed cut.
**Refs:** #2209, #2205, #2207.

## Decision

Use existing `geo-grid` only as the deterministic public coarse-cell representation. A future location commitment must bind precise coordinates plus blinding to station identity, attestation/key epoch, grid version, and policy version. Precise location is private by default; exceptional disclosure requires authorization and audit.

Region proof requests need a fixed disclosure lattice or a verifier-enforced request/disclosure budget. A future prototype must show valid-region GREEN plus out-of-region, swapped-commitment, and adaptive repeated-proof narrowing RED controls.

## Rationale and boundary

`privacy/` ring signatures and stealth addresses provide transaction privacy, while the present selective-disclosure circuit proves knowledge of arbitrary commitment preimages. Neither proves geographic membership or prevents an adversary from intersecting repeated valid regional proofs to narrow a home location. Coarse cells can support limited neighbour/climatology checks without publishing exact coordinates.

This does not prove installation, truthful location, independent neighbours, or sufficient quality at a privacy-preserving cell size.
