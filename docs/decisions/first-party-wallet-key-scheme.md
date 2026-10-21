## DECISION — Q1: A (ed25519 everywhere, SLIP-0010 master key) · Q2: 1 (receive-only legacy) · Q4: the client signs

_Decided 2026-09-18 by Claude (Opus 5) on the owner's explicit instruction to "make the Wave 0 decisions" in epic wrfcoin/infra#966. To overturn it, reopen this issue with a comment; nothing below has been implemented._

**Fixed constraint (re-verified at core4 `50e69fa42`).** Chain 777 accepts a debit only when an ed25519 public key's address equals `from` and the signature verifies. The address is `SHA3-256(pk)[..20]`, rendered `0x…`.
- `shared-protocol/src/accounts/authorization.rs:376-392`
- `common/types.rs:76-81`
- `crypto/hash.rs:127-135` (`Sha3_256`, not Keccak)

**Q1 — key scheme: Option A.** Every first-party wallet (CLI, mobile, web) derives one key the same way:
- BIP-39 mnemonic → seed with an empty passphrase.
- **SLIP-0010 ed25519 master key**: `HMAC-SHA512(key="ed25519 seed", seed)`, first 32 bytes, **no child path**.
- Address = `0x` + hex(`SHA3-256(pk)[..20]`).
- This is exactly what the CLI already derives (`tools/cli/WrfcoinCLI/src/crypto.rs:99-128`, re-read today), so CLI keys do not change; only the CLI's address rendering changes, from `Wrf`+SHA-256 to the chain rule.
- Before mainnet, revisit a hardened multi-account path, which would need a SLIP-0044 coin type. Mainnet is a fresh genesis anyway.
- Rejected: **B** (chain adds secp256k1) is a consensus change touching the signed preimage, state and every validator, out of scope for the testnet cut. **C** (backend signs custodially) makes the backend a hot wallet holding every tester's key.

**Q2 — existing wallets and funds: Option 1.**
- Existing mobile (`sha256(seed)` secp256k1) and web (BIP-44 `m/44'/60'/0'/0/0`) addresses and CLI `Wrf…` addresses are shown as **legacy, receive-only**, with a one-time notice. Nothing is silently re-keyed.
- Faucet drips already sent to secp256k1 addresses are unspendable. **They are not migrated.** The next genesis cut resets them.
- Until then, a tester can claim the faucet again at their new ed25519 address, within the normal faucet caps.

**Q3 — shared vector.** `contract-fixtures/wallet-key-scheme-vector.json`, reproduced byte-for-byte by core4 (shared-protocol test), the CLI, mobile and frontend. I computed it independently with Python (`hashlib` + `cryptography`); the seed matches the published BIP-39 vector:
```json
{
  "mnemonic": "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
  "passphrase": "",
  "derivation": "slip10-ed25519-master (HMAC-SHA512 key \"ed25519 seed\", IL, no child path)",
  "bip39_seed_hex_prefix": "5eb00bbddcf069084889a8ab91555681",
  "ed25519_public_key_hex": "e96b1c6b8769fdb0b34fbecfdf85c33b053cecad9517e1ab88cba614335775c1",
  "address": "0x3f995ceb27e8177077dbed8b1e4f696af74da492",
  "legacy_cli_address_for_the_same_key": "Wrfada8e3423e041d247dca60598e6d3d8834161fe5"
}
```
Each implementing story must assert this vector, plus a mutation control. Swapping SHA3 for Keccak, or SHA3 for SHA-256, must fail.

**Q4 — who signs.** The client signs the transparent-transfer preimage. The backend forwards the signed payload and **never holds user keys**.

**Unblocked now:** wrfcoin/core4#3236 (CLI wallet), wrfcoin/mobile#263 (mobile derivation), wrfcoin/frontend#346 (web wallet), wrfcoin/backend#1165 (client-signed send). `docs/decisions/first-party-wallet-key-scheme.md` lands with the first of them to merge; it copies this comment verbatim.

This decision authorizes those four stories. It authorizes no deploy and no consensus change.
