# Canonical-encoding negative fixtures (#2124)

One committed file per rejection class from `crypto::canonical::CanonicalError`. Each decodes as
`CanonicalProbe` — the conformance vector type that exercises every codec primitive (uint, zigzag
int, u128, bool, f64, length-prefixed string, fixed bytes, option, sorted map) in one object.

| File | Expected error | Corruption |
|---|---|---|
| `valid.bin` | *(decodes)* | — |
| `trailing_bytes.bin` | `TrailingBytes { extra: 1 }` | one `0xff` appended after a complete object |
| `non_minimal_integer.bin` | `NonMinimalInteger` | leading `0x01` replaced with the overlong `0x81 0x00` |
| `duplicate_map_key.bin` | `DuplicateMapKey` | map carries `alpha` twice |
| `unordered_map_key.bin` | `UnorderedMapKey` | map emits `beta` before `alpha` |
| `non_canonical_encoding.bin` | `NonCanonicalEncoding` | `bool` byte is `0x02` |

Asserted by `shared-protocol/tests/canonical_encoding.rs`, which checks the **specific** error
variant — "rejected for the wrong reason" is a real failure mode.

## Reuse by fuzz targets

`CanonicalProbe` is `pub` and not `#[cfg(test)]` precisely so these are reusable as a fuzz corpus:

```rust
use wrfcoin_shared_protocol::crypto::canonical::{Canonical, CanonicalProbe};
fuzz_target!(|data: &[u8]| { let _ = CanonicalProbe::from_canonical(data); });
```

Seed the corpus from this directory. A fuzzer reaching every codec primitive through one entry
point beats one target per real type.

## Regenerating

These are hand-built byte strings, not golden outputs — do not regenerate them from the encoder,
or a bug in the encoder would silently rewrite the fixture that is supposed to catch it. If the
wire format changes, rebuild them from the format spec in `crypto/canonical.rs` and confirm each
still fails with its documented variant.
