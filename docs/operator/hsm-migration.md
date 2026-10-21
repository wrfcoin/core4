# HSM migration for validator signing (core4#1394)

This guide walks a node operator through migrating from the legacy env-var
signing key (`WRFCOIN_SNAPSHOT_SIGNER_PRIVATE_KEY`) to a PKCS#11-backed
block signer. It covers three target tokens:

1. `softhsm2` — software token for CI and local smoke tests.
2. **YubiKey OpenPGP applet via `opensc-pkcs11.so`** — testnet validators.
3. **AWS CloudHSM / Thales Luna** — mainnet validators (reference only).

The background for these decisions lives in
[`VALIDATOR_KEY_CEREMONY_AND_OPSEC.md`](../operations/VALIDATOR_KEY_CEREMONY_AND_OPSEC.md)
§11.1 (ed25519 / OpenPGP applet, resolved 2026-04-11) and §11.8 (native-chain
HSM wiring TODO — this document closes that TODO for the code path).

---

## 1. What changed

- Release builds of `launch_api` **no longer compile** the env-var code
  path. Setting `WRFCOIN_SNAPSHOT_SIGNER_PRIVATE_KEY` on a release binary
  has no effect; the process will exit 1 with a fail-closed message.
- Debug builds keep the env-var path as a fallback so local development
  and in-tree tests run without a token.
- The field that previously held a raw `PrivateKey` on `AppState` is now
  `Option<Arc<dyn BlockSigner>>`; signing is delegated to the token via
  `CKM_EDDSA`.

---

## 2. Required env vars

| Var | Purpose | Example |
|---|---|---|
| `WRFCOIN_HSM_MODULE_PATH` | Path to the PKCS#11 shared object | `/usr/lib/softhsm/libsofthsm2.so` |
| `WRFCOIN_HSM_TOKEN_LABEL` | `CKA_LABEL` of the token slot | `wrfcoin-validator` |
| `WRFCOIN_HSM_KEY_LABEL` | `CKA_LABEL` of the keypair (priv + pub share this label) | `wrfcoin-signing-key` |
| `WRFCOIN_HSM_PIN_ENV` | Name of the env var that holds the PIN | `WRFCOIN_HSM_PIN` |
| *(above var)* | Actual PIN value | `1234` (dev) / systemd `LoadCredential=` (prod) |

The PIN is referenced by a double-indirection — the unit file names the
env var, and a separate mechanism (`LoadCredential=`, `EnvironmentFile=`,
or your secrets manager) populates it. The PIN never appears in the unit
file and never ends up in `ps` / `/proc/$pid/environ` captures that only
capture the non-PIN vars.

---

## 3. softhsm2 (CI / local smoke)

### Install

```bash
# Debian / Ubuntu
sudo apt-get install -y softhsm2 opensc gnutls-bin
# Arch
sudo pacman -S softhsm opensc
# macOS (Homebrew)
brew install softhsm opensc
```

### Provision

```bash
# 1. Initialise a fresh token.
TOKEN_DIR=$(mktemp -d)
SOFTHSM2_CONF="$TOKEN_DIR/softhsm2.conf"
cat > "$SOFTHSM2_CONF" <<EOF
directories.tokendir = $TOKEN_DIR/tokens
objectstore.backend = file
log.level = INFO
EOF
mkdir -p "$TOKEN_DIR/tokens"
export SOFTHSM2_CONF

softhsm2-util --init-token --free \
  --label wrfcoin-validator \
  --pin 1234 --so-pin 0000

# 2. Generate an Ed25519 keypair inside the token.
pkcs11-tool --module /usr/lib/softhsm/libsofthsm2.so \
  --login --pin 1234 \
  --token-label wrfcoin-validator \
  --keypairgen --key-type EC:edwards25519 \
  --label wrfcoin-signing-key --id 01

# 3. Verify: list objects; you should see both the public and private key
pkcs11-tool --module /usr/lib/softhsm/libsofthsm2.so \
  --token-label wrfcoin-validator --list-objects
```

### Run launch_api against softhsm2

```bash
export WRFCOIN_HSM_MODULE_PATH=/usr/lib/softhsm/libsofthsm2.so
export WRFCOIN_HSM_TOKEN_LABEL=wrfcoin-validator
export WRFCOIN_HSM_KEY_LABEL=wrfcoin-signing-key
export WRFCOIN_HSM_PIN_ENV=WRFCOIN_HSM_PIN
export WRFCOIN_HSM_PIN=1234

cargo run -p wrfcoin-native-chain --features hsm --bin launch_api
```

You should see a startup banner like:

```
INFO  [signer] HSM active — Pkcs11Signer(module=/usr/lib/softhsm/libsofthsm2.so, token=wrfcoin-validator, key=wrfcoin-signing-key)
```

---

## 4. YubiKey OpenPGP applet via opensc-pkcs11 (testnet)

The OpenPGP applet supports `ed25519` natively. Use `opensc-pkcs11.so`
(not `libykcs11.so`, which is PIV-only and cannot load an OpenPGP-applet
ed25519 key — see VALIDATOR_KEY_CEREMONY_AND_OPSEC §11.1).

### Prerequisites

```bash
sudo apt-get install -y opensc gnupg2 scdaemon pcsc-tools
pcsc_scan   # confirm the YubiKey is visible
```

### Provision (one-time ceremony)

Generate the keypair on the YubiKey using `gpg --expert --full-generate-key`
with curve `ed25519`. Then the key is visible via `opensc-pkcs11.so`
because OpenSC exposes OpenPGP smart-card objects through PKCS#11.

> **Ceremony** — this step **must** run inside the signed procedure in
> [VALIDATOR_KEY_CEREMONY_AND_OPSEC.md](../operations/VALIDATOR_KEY_CEREMONY_AND_OPSEC.md)
> §4, not ad-hoc on an operator's laptop.

### Run launch_api against YubiKey

```bash
export WRFCOIN_HSM_MODULE_PATH=/usr/lib/x86_64-linux-gnu/opensc-pkcs11.so
export WRFCOIN_HSM_TOKEN_LABEL="OpenPGP card"
export WRFCOIN_HSM_KEY_LABEL="Signature key"
export WRFCOIN_HSM_PIN_ENV=WRFCOIN_HSM_PIN
# PIN supplied by systemd LoadCredential= or a secrets sidecar.

cargo run --release -p wrfcoin-native-chain --features hsm --bin launch_api
```

The exact token and key labels are printed by `pkcs11-tool --list-slots`
and `pkcs11-tool --list-objects` on your provisioned card.

---

## 5. systemd unit example

```ini
[Unit]
Description=WRFCoin validator (launch_api)
After=network.target pcscd.service
Requires=pcscd.service

[Service]
Type=simple
User=wrfcoin
Group=wrfcoin

Environment=WRFCOIN_HSM_MODULE_PATH=/usr/lib/x86_64-linux-gnu/opensc-pkcs11.so
Environment=WRFCOIN_HSM_TOKEN_LABEL=wrfcoin-validator
Environment=WRFCOIN_HSM_KEY_LABEL=wrfcoin-signing-key
Environment=WRFCOIN_HSM_PIN_ENV=WRFCOIN_HSM_PIN

# PIN is loaded as a credential — never written into this unit file.
LoadCredential=wrfcoin-hsm-pin:/run/secrets/wrfcoin-hsm-pin
Environment=WRFCOIN_HSM_PIN=%d/wrfcoin-hsm-pin
# ... or use ExecStartPre= that reads the credential and re-exports it.

ExecStart=/usr/local/bin/launch_api

# Hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/wrfcoin

[Install]
WantedBy=multi-user.target
```

---

## 6. Importing an existing env-var key

Keys from the legacy `WRFCOIN_SNAPSHOT_SIGNER_PRIVATE_KEY` env var **cannot
be imported** directly into most HSMs — that is the point of an HSM.
Instead, the ceremony mints a new key inside the token, and the retired
env-var key is rotated out via the standard operator ceremony in
`VALIDATOR_KEY_CEREMONY_AND_OPSEC.md` §7.

If you are running a local testnet and want to keep the same key
material, softhsm2 accepts import via `softhsm2-util --import`. Do **not**
do this on a mainnet token — the whole security benefit is that the key
was generated *inside* the token.

```bash
# Convert hex to raw 32-byte seed
echo "$WRFCOIN_SNAPSHOT_SIGNER_PRIVATE_KEY" | xxd -r -p > /tmp/sk.raw
# softhsm2-util expects PKCS#8; wrap the raw seed with a tiny Python helper
# or use openssl to produce a PKCS#8 container. Details out of scope here.
```

---

## 7. Verification checklist

- [ ] `pkcs11-tool --list-slots` shows the token with the expected label.
- [ ] `pkcs11-tool --list-objects --token-label <TOKEN>` shows a private
      and a public key object both with the expected `CKA_LABEL`.
- [ ] `launch_api` startup prints `INFO [signer] HSM active — ...`.
- [ ] `curl -s http://localhost:8556/state/snapshot/sync-payload | jq .signed_payload.signer_id`
      returns a non-empty hex-encoded ed25519 public key that matches the
      one on the token.
- [ ] Release build (`cargo build --release --features hsm`) **without**
      `WRFCOIN_HSM_MODULE_PATH` exits 1 at startup with the fail-closed
      message.

---

## 8. Known limitations

- Public key is discovered by matching `CKA_LABEL` on a public-key
  object. If your token stores only the private key, configure the
  public key out-of-band in a follow-up (tracked in #1394 follow-ups).
- Concurrent sign calls are serialised on a single PKCS#11 session via a
  `Mutex`. For a single-writer block producer this is fine; an HSM-aware
  batch signer (one session per core) is a follow-up.
- Error codes from the token are wrapped via `anyhow::Context`. Operators
  troubleshooting a startup failure should run `launch_api` with
  `RUST_LOG=wrfcoin_shared_protocol::crypto::signer=debug`.

See [VALIDATOR_KEY_CEREMONY_AND_OPSEC.md §11.8](../operations/VALIDATOR_KEY_CEREMONY_AND_OPSEC.md)
for the full follow-up list.
