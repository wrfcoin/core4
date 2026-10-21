# HSM signer — local test & operations quick-reference (core4#1394 / #1422)

This is the short reference for *running and testing* the PKCS#11-backed
`BlockSigner`. For the full migration story, see the companion guide
[`hsm-migration.md`](hsm-migration.md).

Cross-reference: [VALIDATOR_KEY_CEREMONY_AND_OPSEC.md §11.8](../operations/VALIDATOR_KEY_CEREMONY_AND_OPSEC.md)
(native-chain HSM wiring) — this doc closes that TODO on the code path.

---

## 1. Feature flag

The PKCS#11 signer is gated behind the `hsm` cargo feature on
`wrfcoin-native-chain` (which forwards to `wrfcoin-shared-protocol/hsm`).
The feature is **OFF by default** so the testnet keeps the existing env-key
path intact until a token ceremony has been run.

```bash
# Default build (testnet — env-key path):
cargo build -p wrfcoin-native-chain --bin launch_api

# HSM-enabled build (opt-in):
cargo build -p wrfcoin-native-chain --features hsm --bin launch_api
```

## 2. Signer selection at startup

The logic lives in
`shared-protocol/src/crypto/signer.rs::load_block_signer`:

| Env + build                                       | Signer resolved          |
|---------------------------------------------------|--------------------------|
| `hsm` feature on, `WRFCOIN_HSM_MODULE_PATH` set   | `Pkcs11Signer`           |
| Debug build, `WRFCOIN_SNAPSHOT_SIGNER_PRIVATE_KEY` set | `EnvKeySigner` (WARN banner) |
| Release build, no HSM vars                        | `None` → `launch_api` exits 1 |
| Debug build, neither set                          | `None` → snapshot-sync payloads unsigned (WARN) |

The env-key fallback is `#[cfg(debug_assertions)]`, so it is **compiled
out of release binaries**. A release build without HSM config has no way
to sign.

## 3. Fail-closed semantics

- If `WRFCOIN_HSM_MODULE_PATH` is set but `Pkcs11Signer::new` fails
  (module missing, token missing, key missing, PIN wrong, …) then
  `load_block_signer` returns `Err`, and `load_snapshot_signer` in
  `launch_api/main.rs` `std::process::exit(1)`s with the underlying
  error. There is no silent fallback from HSM to env-key.
- Release builds with no HSM config also exit 1 with a message pointing
  at `docs/operator/hsm-migration.md`.

## 4. Running the integration test locally

The test lives in `native-chain/tests/hsm_signer.rs`. It is compiled
only with `--features hsm`, and both tests are `#[ignore]`d (#2748): a
default `cargo test` run on a machine without a provisioned token
reports `2 ignored` — a countable skip — instead of a false PASSED.
Running them takes an explicit `-- --ignored`, and a requested run with
incomplete `WRFCOIN_HSM_*` env fails loudly rather than pretending
success. (The old `HSM_TEST_ENABLED=1` runtime gate is gone: its early
`return` made an unprovisioned machine report both tests PASSED,
indistinguishable from a verified signer.)

```bash
# 1. Install softhsm2 and provision a token (one-time).
#    Full recipe in hsm-migration.md §3.
sudo apt-get install -y softhsm2 opensc
softhsm2-util --init-token --free \
  --label wrfcoin-validator --pin 1234 --so-pin 0000
pkcs11-tool --module /usr/lib/softhsm/libsofthsm2.so \
  --login --pin 1234 --token-label wrfcoin-validator \
  --keypairgen --key-type EC:edwards25519 \
  --label wrfcoin-signing-key --id 01

# 2. Run the tests (they are #[ignore]d, so ask for them explicitly).
WRFCOIN_HSM_MODULE_PATH=/usr/lib/softhsm/libsofthsm2.so \
WRFCOIN_HSM_TOKEN_LABEL=wrfcoin-validator \
WRFCOIN_HSM_KEY_LABEL=wrfcoin-signing-key \
WRFCOIN_HSM_PIN_ENV=WRFCOIN_HSM_PIN \
WRFCOIN_HSM_PIN=1234 \
cargo test -p wrfcoin-native-chain --features hsm --test hsm_signer -- --ignored
```

The test:

1. Builds a `Pkcs11Signer` from the env vars.
2. Signs a 32-byte block-header-shaped payload.
3. Verifies the signature against the public key the token advertises.
4. Asserts the provenance is `SignerProvenance::Pkcs11 { .. }`.
5. Signs the same payload a second time and asserts byte-identical
   signatures — ed25519 is deterministic; a randomised variant would
   fail this check.

## 5. CI provisioning

`.github/workflows/hsm-signer-ci.yml` (wrfcoin/infra#430) installs
softhsm2 on the runner, provisions a throwaway token, and runs the
tests with `-- --ignored` under both PIN policies, wrapped in
`scripts/cargo_test_nonvacuous.py --min-executed 2` (#2867) so a run
that executed fewer than both tests is red, never silently green.

## 6. Signer-path equivalence (unit-level)

Even without softhsm2 you can still confirm that the `BlockSigner`-routed
snapshot path produces byte-identical signed payloads to the raw-private-key
path it replaces:

```bash
cargo test -p wrfcoin-shared-protocol --lib \
  blockchain::snapshot_sync::tests::snapshot_sync_signer_path_matches_private_key_path
```

This runs on every PR and blocks #1394 from silently changing the wire
format.

## 7. Troubleshooting

| Symptom                                            | Cause / fix |
|----------------------------------------------------|-------------|
| `FATAL: no snapshot signer configured` on release  | Set the `WRFCOIN_HSM_*` vars; see §2. |
| `C_Login failed` at startup                        | Wrong PIN, or PIN env var (`WRFCOIN_HSM_PIN_ENV`) points at an unset var. |
| `no PKCS#11 token found with label …`              | Token label in env doesn't match `softhsm2-util --show-slots` output. |
| `expected 32-byte Ed25519 public key, PKCS#11 returned N bytes` | Key is not ed25519, or token returned the public point in an unexpected DER form. File a bug with the token vendor and the raw CKA_EC_POINT bytes. |
| `cargo test … --test hsm_signer` reports `2 ignored` | Expected on a machine without a provisioned token (#2748). To actually run the tests, provision per §4 and pass `-- --ignored`. |
| Requested run (`-- --ignored`) fails with `WRFCOIN_HSM_* env vars must all be set` | The run was asked for but the environment is incomplete — set every var from §4 (the failure names the missing one). |

For deeper debugging, run `launch_api` with
`RUST_LOG=wrfcoin_shared_protocol::crypto::signer=debug`.
