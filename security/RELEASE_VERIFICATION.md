# WRFCoin Release Verification Guide

This document explains how to verify the authenticity and integrity of WRFCoin release binaries.

## Quick Verification

### Step 1: Download the release files

Download both the binary and its signature from the [releases page](https://github.com/wrfcoin/core4/releases):

- `wrfcoin-node-linux-x86_64` (the binary)
- `wrfcoin-node-linux-x86_64.asc` (GPG signature)
- `wrfcoin-node-linux-x86_64.sha256` (checksum)
- `CHECKSUMS.txt` (all checksums)
- `CHECKSUMS.txt.asc` (signed checksums)

### Step 2: Import the WRFCoin signing key

```bash
# Import from keyserver
gpg --keyserver keyserver.ubuntu.com --recv-keys <KEY_ID>

# Or import from this repository
curl -sSL https://raw.githubusercontent.com/wrfcoin/core4/main/security/SIGNING_KEY.asc | gpg --import

# Verify the key fingerprint matches:
# Expected fingerprint: XXXX XXXX XXXX XXXX XXXX  XXXX XXXX XXXX XXXX XXXX
```

### Step 3: Verify the signature

```bash
# Verify the binary signature
gpg --verify wrfcoin-node-linux-x86_64.asc wrfcoin-node-linux-x86_64

# Expected output should include:
# gpg: Good signature from "WRFCoin Release Signing Key <releases@wrfcoin.com>"
```

### Step 4: Verify the checksum

```bash
# Verify SHA256 checksum
sha256sum -c wrfcoin-node-linux-x86_64.sha256

# Or verify against the signed checksums file
gpg --verify CHECKSUMS.txt.asc CHECKSUMS.txt
sha256sum -c CHECKSUMS.txt
```

## Assurance boundary

The release workflow authenticates each binary and `CHECKSUMS.txt` with detached signatures
under the same WRFCoin GPG release key. Verification establishes that the downloaded bytes were
signed by that key and match the signed checksum manifest.

The workflow does **not** publish SLSA/Sigstore provenance, use a transparency log, or perform an
independent rebuild-and-compare. The build uses some deterministic inputs, but byte-for-byte
reproducibility has not been established. A valid release signature therefore does not protect
against theft or misuse of the release key, or against a compromised build pipeline that can use
that key.

## Signing Key Details

| Property | Value |
|----------|-------|
| Key ID | `<TO_BE_FILLED>` |
| Key Type | RSA 4096 |
| Created | `<DATE>` |
| Expires | `<DATE>` |
| Email | releases@wrfcoin.com |

### Key Fingerprint

```
XXXX XXXX XXXX XXXX XXXX  XXXX XXXX XXXX XXXX XXXX
```

## Security Best Practices

1. **Always verify signatures** before running any binary
2. **Verify the key fingerprint** matches the one published here
3. **Treat GPG authentication as the implemented boundary**; there is no signed provenance
4. **Build from source independently** for consensus-critical deployments, without assuming the
   result will be byte-identical until reproducibility is separately verified
5. **Report issues** to security@wrfcoin.com if verification fails

## Reporting Security Issues

If you find a discrepancy between published checksums and downloaded files, or if signature verification fails, please report immediately:

- **Email**: security@wrfcoin.com
- **GitHub Security Advisories**: [Report a vulnerability](https://github.com/wrfcoin/core4/security/advisories/new)

Do NOT run unverified binaries in production environments.
