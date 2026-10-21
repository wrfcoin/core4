# Signature-preimage migration: v1 → v2

Recorded for #2123. This document defines the version boundary at which every WRFCoin signature
preimage and block hash changes, and the procedure for crossing it.

## What changes at v2

| Construction | v1 (current) | v2 |
|---|---|---|
| Domain preimage (`DomainSeparated`) | `DOMAIN_TAG \|\| body` | `DOMAIN_TAG \|\| chain_id \|\| network_id \|\| protocol_version \|\| body` |
| Block hash | header fields only | `network_magic \|\| protocol_version \|\| header fields` |

All components are length-prefixed, so concatenation cannot be re-split.

### Scope, stated precisely

Review of PR #2232 correctly noted that the original description overclaimed. What v2 changes
today is **the block hash** and **the `DomainSeparated` preimage**. It does **not** change the
legacy per-type signing paths (`Transaction::signing_hash`,
`SlashingReportPayload::canonical_signing_message`, and the rest) — those still use their own
preimages and are migrated separately under #2226. So a v1→v2 flip alters block hashes without
altering every existing signature preimage, and both migrations must ride the same boundary.

## This invalidates every previously produced signature

That is not a side effect — it is the point. A v1 signature carries no statement about which
network or which protocol version it was made for, so it is valid on all of them. A signature
harvested from the testnet replays on mainnet. #2065 found the signing envelope hardwiring
mainnet identifiers; #2066 is the block-level instance.

Every signature produced before the boundary is therefore invalid after it, and every block hash
changes. **There is no compatibility window and one is not wanted** — a window in which both
encodings verify would preserve exactly the cross-network replay the change exists to remove.

## The gate

`SigningProtocolVersion::V1` is the default. Merging #2123 changes nothing:

- `domain_preimage_bound(..., V1, ...)` is byte-identical to the unbound construction.
  Test: `v1_is_byte_identical_to_the_unbound_construction`.
- `Block::calculate_hash_for(V1, _)` ignores the chain identity entirely and produces the
  pre-#2123 bytes. The added lines sit inside a `>= V2` guard; no existing byte-building line was
  modified. Tests: `v1_block_hash_is_unchanged_by_this_story`,
  `calculate_hash_deterministic_fixture`.

### Activation is chain-wide, not per-node

The first version of this gate was per-process: `WRFCOIN_SIGNING_PROTOCOL_VERSION=2` on one node
put *that node* on v2. Review of PR #2232 identified that this reproduces #2076 exactly — one node
at v2 while peers and external signers stay v1 — so the gate was rebuilt around a **chain-wide
boundary**.

**Genesis decides. Local configuration may only agree.** The chain's version is recorded in
genesis block metadata under `protocol_version`. At boot,
`assert_startup_protocol_compatibility` resolves the effective version and the node **refuses to
start** on any disagreement:

| genesis pins | env requests | result |
|---|---|---|
| nothing | nothing / v1 | v1 — every chain that exists today |
| nothing | **v2** | **refuse** — `UnilateralActivation` |
| v1 | v2 | refuse — `VersionMismatch` |
| v2 | v1 | refuse — `VersionMismatch` |
| v2 | nothing / v2 | v2, provided the identity is not the default fallback |
| unparseable | any | refuse — `UnknownGenesisVersion` |

Because a node whose genesis does not pin v2 **cannot** reach v2 by local configuration, a single
node can no longer move itself ahead of the fleet. That is the property that closes the review
finding; it holds without needing a peer handshake, though one is still worth adding as defence in
depth (#2254).

`SigningProtocolVersion::active()` reads **no environment**. It returns only what the startup gate
installed, defaulting to v1 — so there is no code path from an environment variable to v2 that
bypasses the gate.

v2 additionally refuses to start on the unconfigured mainnet fallback identity
(`wrfcoin-main`/`mainnet`), because at v2 the identity is inside every preimage and a testnet node
left on the fallback would mint mainnet-identified signatures.

## Why this must not be a rolling change

#2076 is this failure having already happened on this chain: a v1 collector signing against a v2
verifier silently halted the earn loop for 28 hours. Nothing errored loudly — signatures simply
stopped verifying, and the chain stopped advancing.

A rolling upgrade guarantees that state. During the roll, some nodes sign v1 and some verify v2;
every cross-pair fails. The failure is silent because a failed signature verification is
indistinguishable from an invalid signature.

Before a public testnet the cost of getting this wrong is a version bump. After, it is a
coordinated chain reset.

## Cutover procedure

1. **Freeze the chain.** Stop block production. Do not attempt this while producing.
0. **Record `protocol_version` in genesis.** v2 is unreachable without it — the node refuses to
   start rather than activating locally. Since block hashes change at v2, this happens as part of
   creating the new chain, not as an edit to an existing one.
2. **Confirm every signer is on a v2-capable build.** Node, collector, faucet, wallet, and any
   backend that signs. A v1 signer against a v2 verifier is #2076.
3. **Record the chain identity in genesis.** The genesis block's metadata must carry `chain_id`
   and `network_id` (`GENESIS_CHAIN_ID_KEY` / `GENESIS_NETWORK_ID_KEY`). Without them,
   `assert_chain_identity_matches_genesis` cannot detect a misconfigured node — it treats absent
   genesis identity as *unknown*, not as a mismatch, because refusing on absence would refuse to
   start every pre-#2123 node.
4. **Set `WRFCOIN_SIGNING_PROTOCOL_VERSION=2` on every node simultaneously**, along with the
   correct `WRFCOIN_CHAIN_ID` and `WRFCOIN_NETWORK_ID` for the network.
5. **Restart from a fresh genesis.** Block hashes change, so the existing chain cannot be
   continued across the boundary.
6. **Verify convergence on `state_root`, not block hash**, across at least two nodes before
   resuming production.

## Startup assertion

`assert_chain_identity_matches_genesis(configured, genesis)` must be called once at boot, and the
node must refuse to start on `Err`.

| Case | Result |
|---|---|
| configured == genesis | start |
| configured != genesis | **refuse** — `GenesisMismatch` |
| genesis has no recorded identity | start (unknown, not mismatched) |
| configured chain_id or network_id empty | **refuse** — `EmptyIdentifier` |

A node signing under one chain id against a chain created under another produces signatures no
peer will accept. That is worth refusing to start over, because the alternative is the node
appearing healthy while contributing nothing.

`ChainIdentity::is_default_fallback()` additionally detects a node running on the unconfigured
mainnet fallbacks (`wrfcoin-main` / `mainnet`). A testnet node in that state is misconfigured in
the direction that matters: it produces and accepts mainnet-identified signatures.

## Network magic (#2066)

Network magic is **derived**, not assigned:

```
network_magic = hash("wrfcoin.network_magic.v1" || chain_id || network_id)[0..4]
```

Derivation means a new network cannot forget to pick one, and two networks cannot collide unless
their identifiers already collide.

## Related

- #2065 — signing envelope hardwiring mainnet identifiers
- #2066 — fold network magic into block-hash preimage
- #2076 — v1 signer against v2 verifier, earn loop halted 28h
- #2034 — chain-id / network-id configuration variables
- #2122 — domain-separation coverage audit, `docs/security/DOMAIN_TAGS.md`
- #2226 — cutover of the deferred live signing paths
