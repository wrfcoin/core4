# ZK Proving Key Provenance

This directory contains Groth16 (BN254) proving and verification key bundles
embedded into the `wrfcoin-native-chain` binary at compile time.

## Generation Method

All `.bin` files were generated via **single-party trusted setup** using
`ark-groth16::Groth16::circuit_specific_setup(circuit, &mut OsRng)` in
`zk-circuits/src/proving.rs`. The random entropy source is the operating
system's cryptographically secure PRNG (`OsRng`).

**This is NOT an MPC ceremony.** A single party holds the toxic waste
(trapdoor) implicitly in memory during generation. Anyone who can recover
that randomness could forge proofs. For testnet this is acceptable; for
mainnet, a multi-party computation (MPC) ceremony is required (see
"Key Rotation Plan" below).

## Key Inventory

| File | Circuit | SHA-256 |
|------|---------|---------|
| `range-proof-64.bin` | 64-bit range proof | `5e7445455dfcff75f8fe07d1beb79f068cdc13f98cf79215f836ba8f3be4c302` |
| `confidential-transfer-1-1-64.bin` | 1-input 1-output confidential transfer (64-bit) | `31cbf5677d6b6dbd987a015fa91eba44bc0bc9828776a74f2f765f4300adb81d` |
| `selective-disclosure-3.bin` | 3-field selective disclosure | `54d414b06f4513fbfba4bf89c232a0cf309cb9c6f1ac6a1ad09f2343c98106e4` |
| `weather-model-2.bin` | 2-variable weather model proof | `0191bc6067cdb3b285bde60b119bc5cedc5e1e7cf782ee9d0c0f7fe40c6f4809` |

## Regeneration

**Regenerate whenever a circuit's constraint system changes.** A Groth16 proving
key is bound to the exact R1CS it was generated from; a key built for an older
circuit produces proofs that do not verify.

```bash
cargo run --release -p wrfcoin-zk-circuits --example regen_zk_params
```

The example prints a replacement for the Key Inventory table above. Paste it in.

`native-chain/tests/zk_params_drift.rs` fails if any committed bundle stops
matching its circuit, so this cannot drift silently again.

### History

These bundles were regenerated on **2026-07-21** (#2283) because they had drifted.
They were originally generated 2026-03-08 (#575); on 2026-03-16 the range-proof,
confidential-transfer and weather-model circuits moved from linear commitments to
Poseidon (#837, #819) and confidential transfer gained a blinding-conservation
constraint (#786). Nothing regenerated the keys, and every committed bundle was
4x-100x smaller than the circuit it claimed to serve for four months.

The failure was silent because the **public-input counts did not change**: the
bundles still deserialized, verification still ran to completion, and it simply
returned `false`. The SHA-256 table below pins file *integrity*, not *freshness* —
the hashes all still matched, so it certified stale keys as good. That is what the
drift test, not this table, is there to catch.

No deployed node was affected: `zk` is not a default feature and neither
`native-chain/Dockerfile` nor `infra/infra/docker/Dockerfile.core` enables it, so
no proof was ever generated or verified on the testnet with the stale keys.

## Serialization Format

Each `.bin` file contains the proving key (`ProvingKey<Bn254>`) followed by
the prepared verifying key (`PreparedVerifyingKey<Bn254>`), both serialized
with `ark-serialize` compressed encoding. See `zk_service.rs` functions
`serialize_key_pair` / `deserialize_key_pair` for the exact wire format.

## Verification

To verify the integrity of embedded keys:

```bash
sha256sum native-chain/zk-params/*.bin
```

Compare the output against the table above. Any mismatch indicates the
key files have been modified.

At runtime, the node computes a SHA-256 fingerprint of each embedded bundle
(see `bundle_fingerprint()` in `zk_service.rs`) and uses it as part of the
on-disk cache file name. Corrupt or tampered key files on disk are
automatically restored from the embedded bundle.

## Key Rotation Plan

When transitioning to mainnet, these single-party keys MUST be replaced with
MPC-ceremony-derived keys. The rotation process is:

1. **Conduct MPC ceremony** for each circuit type (see issues #717, #716).
   Multiple independent participants contribute randomness; the toxic waste
   is destroyed when any single participant discards their share.

2. **Publish ceremony transcript** with participant attestations, round
   hashes, and the final key hashes.

3. **Replace `.bin` files** in this directory with the ceremony output.
   Update the SHA-256 table above.

4. **Hard fork required.** Because proving keys are embedded at compile time
   and proofs generated with the old keys will not verify under the new
   verification key, all nodes must upgrade to the new binary simultaneously.
   Coordinate via a hard-fork activation height in `ChainConfig`.

5. **Verify backward compatibility.** Proofs generated before the fork must
   still be verifiable by replaying historical blocks with the old keys.
   The replay verifier should accept both old and new verification keys
   during a transition window.

## References

- Issue #964: ZK trusted setup uses single-party OsRng
- Issue #717: MPC ceremony planning
- Issue #716: ZK key rotation mechanism
- `zk-circuits/src/proving.rs`: Key generation, proving, verification
- `native-chain/src/zk_service.rs`: Key loading, caching, embedded bundles
