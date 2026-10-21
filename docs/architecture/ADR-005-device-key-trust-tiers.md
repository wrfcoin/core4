# ADR-005: Device-key trust tiers — earned by proof, never self-asserted

**Status:** Proposed (types and tier mapping shipped additively under #2205; no
consensus change; reward weighting is a feature-gated follow-up)
**Date:** 2026-09-11
**Story:** #2205 · **Epic:** #2204 (DePIN verifiable sensor data)
**Related:** #2569 (`provenance_class` immutability), #2207 (plausibility),
#2148 (Sybil caps), #3092 (feature schedule), ADR-003 (role earnings)

## Context

`sensor-attestation` already carries a domain-separated `DeviceAttestation`:
a reading hash signed by a device key, with a caller-supplied
`AttestationType`, calibration, tamper and freshness metadata, and
signature-tamper tests. That proves *a key signed a reading*. It does not
prove the key lives in hardware: `DeviceAttestation::sign` accepts any
`AttestationType` and signs with an ordinary ed25519 key, so a software key
can pass the check while labelling itself `SecureElement`.

Payment today rests on `provenance_class`, which is asserted at registration
by the authorised registrant (`Real` pays; `Synthetic` reaches quorum and
pays zero — Cindy's #2569 review). That is strong immutability of a label,
not proof that a physical device holds a non-exportable key. Requiring a
secure element would exclude every station already deployed. So trust must
be **tiered**, and a tier must be **earned by evidence a verifier accepted**,
never by the label a device chose for itself.

## Decision

1. **Backing carries evidence, not a label.** `KeyBacking` is
   `SecureElement { attestation, model }`, `Tpm { quote }`,
   `SoftwareKeystore` or `SoftwareFile`. A hardware claim is a claim until a
   `HardwareAttestationVerifier` with configured vendor roots accepts the
   evidence for *this* device key.
2. **Fail-closed tier mapping.** `Attested` requires an accepted
   secure-element chain; `Verified` an accepted TPM quote; `Basic` a software
   key with a proven-continuous observation history (seven days, at least
   hourly, no gap over six hours); everything else is `Unverified` — including
   a claimed hardware backing the verifier did not accept, which does **not**
   fall back to `Basic` however long its history. The shipped default
   verifier, `NoVendorRoots`, accepts nothing, so a deployment with no roots
   cannot mint `Attested` by accident. The mobile boundary follows: an
   Android/iOS keystore label, a Bluetooth accessory or a Flipper Zero
   presence elevates nothing without the enrollment proof.
3. **Unattested stations still participate.** A tier sets a proposed reward
   weight (10 000 / 9 000 / 7 500 / 5 000 bps) and a data-quality-tier
   ceiling (Gold / Silver / Bronze / Unverified). It never gates acceptance.
4. **Device keys are scoped to observation submission.** `DeviceKeyScope`
   permits `SubmitObservation` and `AttestReading` only; a device key cannot
   authorise a transfer, stake, vote, station registration or its own
   rotation.
5. **Enrollment binds key → operator → station** (`DeviceIdentity`,
   `bind_to_station`): the station must be active, registered to the
   device's operator, and be the station the device enrolled for. Devices per
   operator are bounded at the same number as stations per identity at the
   ingress edge (#2148).

## What this ADR does not decide (open, in order)

- **Enrollment challenge.** The challenge a device must sign at enrollment —
  station id, device public key, model, firmware, chain and network domain
  — and who issues it. Until it exists, `registered_at` is when the operator
  said so.
- **Vendor roots and quote formats.** Which secure-element vendors' root
  certificates and which TPM quote profile the verifier implementation
  accepts, and where roots are configured. Roots are deployment content, not
  crate content.
- **Revocation and rotation.** How a compromised or replaced device key is
  retired without losing the station's history.
- **Reward wiring.** Reading the tier in the reward path changes what
  consensus pays and must ship as a `ConsensusFeature` with a scheduled
  activation height under the #3092 schedule, with `determinism_differential`
  proving roots unchanged before activation. The weights above are the
  proposal for that change, not its activation.

## What this does not solve

Calibrated truth, location truth, a copied-but-valid hardware key, or an
attested device that has been compromised. Those remain #2207 and
operational risks; a trust tier says who signed, not whether the reading is
true.

## Consequences

- Positive: the label/proof distinction is now a type, the honest default
  is `Unverified`, and every later hardening (roots, challenge, revocation)
  slots in behind the same `HardwareAttestationVerifier` seam.
- Negative: until roots and a challenge exist, no device can reach
  `Attested` or `Verified`, and the tier's only effect is the `Basic` /
  `Unverified` split by history — which is the honest state of the network.
