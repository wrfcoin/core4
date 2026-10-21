# YubiKey 5 — PKCS#11 validator key provisioning

This guide covers using a **YubiKey 5 (firmware ≥ 5.7.0)** as the
PKCS#11 token backing `Pkcs11Signer`. It is the operator counterpart to
[`docs/operator/hsm.md`](../operator/hsm.md) (which covers `softhsm2`)
and [`docs/operator/hsm-migration.md`](../operator/hsm-migration.md)
(which covers the env-key → HSM cutover).

Companion infra ticket: [`wrfcoin/infra#435`](https://github.com/wrfcoin/infra/issues/435).

> **Firmware requirement.** YubiKey 5.7.0 (April 2024) was the first
> release to expose **Ed25519** in PIV slots. WRFCoin signs blocks with
> Ed25519 — see `shared-protocol/src/crypto/signature.rs`. Older
> YubiKey 5 firmware (5.2.3 – 5.6.x) only supports RSA / ECDSA-P256 /
> ECDSA-P384 in PIV and will **not** work as a drop-in block signer
> without changing the on-chain signature format. Check with
> `ykman info`.

---

## 1. Prerequisites

```bash
# Linux host packages.
sudo apt-get install -y opensc opensc-pkcs11 yubikey-manager
# Verify the PKCS#11 module is present.
ls -l /usr/lib/x86_64-linux-gnu/opensc-pkcs11.so
# Verify firmware ≥ 5.7.0.
ykman info
```

If `ykman info` reports `Firmware version: 5.6.x` or older, the
Ed25519 path below will fail at `ykman piv keys generate` with
`unsupported algorithm`. Upgrade firmware before proceeding.

---

## 2. Generate the Ed25519 key on the device

Slot **9c** ("digital signature") is the canonical choice — it forces
PIN entry on every signing operation, matching the validator-key
threat model. The private key is generated **inside** the YubiKey and
cannot be exported.

```bash
# Set a custom Management Key (out of scope for this doc — use ykman
# piv access change-management-key).
# Set the PIV PIN; this is the value the signer will read via
# WRFCOIN_HSM_PIN.
ykman piv access change-pin --pin 123456 --new-pin <NEW_PIN>

# Generate Ed25519 keypair in slot 9c, export public key as PEM.
ykman piv keys generate --algorithm ED25519 9c yubikey-9c-pub.pem

# (Optional but recommended) self-sign a certificate so the slot is
# "occupied" from PKCS#11's perspective. Some PKCS#11 consumers refuse
# to surface a key object with no associated CKO_CERTIFICATE.
ykman piv certificates generate --subject "CN=wrfcoin-validator" 9c yubikey-9c-pub.pem
```

The PEM file holds the **public key only** — keep it; you will
register this as the validator's on-chain identity.

---

## 3. Discover labels for the env-var surface

`Pkcs11Signer::new` looks up key objects by label. OpenSC populates
PKCS#11 labels from the PIV slot name. Confirm what your YubiKey is
exposing:

```bash
# List tokens.
pkcs11-tool --module /usr/lib/x86_64-linux-gnu/opensc-pkcs11.so -T

# List objects on the token. Note the labels of the
# private and public key objects in slot 9c.
pkcs11-tool --module /usr/lib/x86_64-linux-gnu/opensc-pkcs11.so \
  --login --pin <PIN> -O
```

Typical output:

```
Private Key Object; EC
  label:      SIGN key
  ID:         02
  Usage:      sign
Public Key Object; EC EDWARDS
  label:      SIGN pubkey
  ID:         02
```

Token label appears as `PIV_II` or similar — extract it from `-T`
output.

---

## 4. Operator env vars

The same surface as the `softhsm2` path. Dan's lane wires these into
the systemd unit; nothing in core4 needs to change.

```bash
# Prefer Yubico's libykcs11 — opensc-pkcs11.so does not expose the
# private key for CKM_EDDSA sign on YubiKey PIV slot 9c (see infra#511).
WRFCOIN_HSM_MODULE_PATH=/usr/lib/x86_64-linux-gnu/libykcs11.so
WRFCOIN_HSM_TOKEN_LABEL=<from pkcs11-tool -T, e.g. "YubiKey PIV #35278470">
WRFCOIN_HSM_KEY_LABEL=Private key for Digital Signature
WRFCOIN_HSM_PUBKEY_LABEL=Public key for Digital Signature
WRFCOIN_HSM_PIN_ENV=WRFCOIN_HSM_PIN
# The PIN itself is delivered via systemd LoadCredential= — never put
# it in the unit file or in the process environment via a static
# Environment= line. See infra#435 for the deployment recipe.
WRFCOIN_HSM_PIN=<PIV PIN set in §2>
```

Note: the YubiKey **public-key label** and **private-key label** are
not identical with `libykcs11.so` — it exposes them as
`"Private key for Digital Signature"` and `"Public key for Digital Signature"`
respectively (different first word, same suffix). `Pkcs11Signer::new`
looks up each class separately, so set both `WRFCOIN_HSM_KEY_LABEL`
(private) and `WRFCOIN_HSM_PUBKEY_LABEL` (public). If `WRFCOIN_HSM_PUBKEY_LABEL`
is unset the signer falls back to `WRFCOIN_HSM_KEY_LABEL` for both lookups —
that works for modules that expose both classes under the same label.

---

## 5. Verify with the operator integration test

```bash
WRFCOIN_HSM_TEST_YUBIKEY=1 \
WRFCOIN_HSM_MODULE_PATH=/usr/lib/x86_64-linux-gnu/opensc-pkcs11.so \
WRFCOIN_HSM_TOKEN_LABEL=<token label> \
WRFCOIN_HSM_KEY_LABEL="SIGN key" \
WRFCOIN_HSM_PIN_ENV=WRFCOIN_HSM_PIN \
WRFCOIN_HSM_PIN=<PIN> \
cargo test -p wrfcoin-shared-protocol --features hsm \
  --test yubikey_signer -- --ignored
```

The test:

1. Builds a `Pkcs11Signer` against the YubiKey.
2. Signs a 32-byte block-header-shaped payload.
3. Verifies the signature against the public key the token advertises.
4. Re-signs the same payload and asserts byte-identical output
   (Ed25519 is deterministic; any randomised variant will fail this).

If step (4) fails, the YubiKey firmware or PKCS#11 module is producing
a randomised EdDSA variant — file a bug; do **not** ship that token to
mainnet, the chain's verifier assumes deterministic signatures for
some equivocation-detection paths.

---

## 6. Compatibility shim

`Pkcs11Signer::sign` normalizes the C_Sign return value before passing
to `Signature::from_bytes`:

- Raw 64-byte `r || s` (per PKCS#11 v3.x spec) → pass through.
- DER-wrapped (`0x04 0x40 <64 bytes>`) → strip wrapper, pass through.

This is defensive — the YubiKey 5 + OpenSC stack returns the raw form
on every version tested. The shim exists because earlier softhsm2 4.x
builds were observed to return the DER-wrapped form, and a future
PKCS#11 module update should not silently break block production.

The same defensive logic already exists for `CKA_EC_POINT` on the
public-key read path (`strip_der_octet_string` in `pkcs11.rs`).

---

## 7. Backup / loss

A YubiKey **cannot be cloned**. Two YubiKeys are provisioned per
validator — one production, one cold backup with the same on-chain
identity. To do this *correctly* (without ever exporting the private
key), you must register **both** public keys against the same
validator identity in advance and operate the chain in a mode that
accepts either as the validator's signature. That mechanism is
**not implemented** in core4 today — see follow-up
[`wrfcoin/core4#????`](https://github.com/wrfcoin/core4) (to be filed).
In the meantime, the operational fallback is:

- Run on the primary YubiKey.
- If lost / damaged, operator follows the validator-key rotation
  ceremony (see `docs/operations/VALIDATOR_KEY_CEREMONY_AND_OPSEC.md`)
  to promote the cold-backup YubiKey's pubkey as a *new* validator
  identity and retire the old one via governance.

---

_Last updated: 2026-05-22 — for round R101._
