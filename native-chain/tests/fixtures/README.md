# Persistence fixtures

| file | what | source |
|---|---|---|
| `checkpoint_state_v2_golden.bin` | enveloped checkpoint, format 2 | written by the format-2 code (`9772684d6`, #2727) via `regenerate_golden_checkpoint_fixture` |
| `checkpoint_state_v3_golden.bin` | enveloped checkpoint, format 3 | written by the format-3 code (`3c7553a41`, #3081) via the regenerator |
| `checkpoint_state_v4_golden.bin` | enveloped checkpoint, format 4 (current) | written by the format-4 code (`3e92abbaa`, #3069) via the regenerator |
| `state_5007ce747_pre2508.bin` | raw pre-envelope `bincode(State)` | serialized by actual `5007ce747` code; the CT 101 #2727 incident shape |

The regenerator refuses to overwrite the current version's fixture (#2881 review), so each
golden is written exactly once by the code of its era and is a byte-exact witness of that
layout. The goldens are the fixtures for the #3091 migration framework
(`native-chain/src/storage/migrations.rs`, `native-chain/tests/storage_migration_2727.rs`):
`v3` is the input of `MigrationV3ToV4`, `v2` is the no-registered-path case.

## The 2026-09-10 pre-cut volume (not a fixture here)

#3091 planned a `checkpoint_v3_h56890.bin` captured from the workstation's set-aside
pre-cut volume (`testnet_node-data:/precut-20260911T160406Z/wrfcoin.redb`, 511 MB). A
read-only scan of that file on 2026-09-11 found **no checkpoint envelope at all** (zero
`WRFCKPT\0` occurrences; the WAL v2 magic appears 206 times, so the volume is not
encrypted at rest). The image that wrote it (`4c257417c`, 2026-08-28) predates the
versioned envelope (`9772684d6`, 2026-08-31): its checkpoints are raw pre-envelope
`bincode(State)` at the pre-#2846 layout, not format 3. Migrating it would need a
pre-envelope → 2 → 3 → 4 chain, and 2 → 3 is a mid-struct type change with no data-derivable
conversion, so that volume is out of scope for the framework's first path. Recorded here so
nobody hunts for a v3 checkpoint in it.

## Node bond Slice 2, format 12 (2026-10-11)

`checkpoint_state_v12_golden.bin` is a new synthetic layout witness. Its body
is the immutable format-11 fixture body followed by exactly one `0x00` byte
(the absent `Option<NodeBondRegistry>`); its envelope version is 12. It was
constructed by that append, without overwriting any older golden. The migration
unit test independently checks the complete append and exact result. Activated
registrations are exercised through real redb checkpoint/WAL round trips in
`node_bond_slice2`; this absent-marker fixture is not activated runtime evidence.

SHA-256: `fc1d257f284cb0b3d12a3380ab5cbc2f1b7b54132074da25712ec835550a30dd`.

S3 format 13 fixture is the immutable format 12 body plus one absent receipt marker byte 0x00, with header version changed to 13. Old fixture bytes are unchanged.
