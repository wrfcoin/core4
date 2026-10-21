# WRFCoin light client (#2188, epic #2183)

A light node follows the chain by **headers**, verifies every header against a
**producer set it was told to trust** (rotated only by an operator-verified
checkpoint), requires and checks finality certificates when a verifier is
configured, and
establishes **data availability by sampling** — without downloading blocks and
without trusting the node that serves them. Library: `wrfcoin_da::light_client`.
Binary: `native-chain/src/bin/light_node.rs` (`cargo run --features light-node --bin light_node`).

## What is verified, in order

| Step | Check | Rejects with |
|---|---|---|
| 1 | The declared header hash equals the hash recomputed from the header fields + transaction hashes (`LightHeader` mirrors `Block::canonical_body` byte for byte; pinned by `header_hash_matches_block_hash_for_every_shape`) | `HeaderInvalid` |
| 2 | Next height, parent = trusted hash | `HeaderChainBroken` |
| 3 | The header source attached **no** `ProducerSetChangeEnvelope`. The chain has no on-chain rotation path yet (P5 fast-follow) and a change is not committed in the signed header, so any change is refused (fail closed); a rotation reaches the client only as a new operator-verified checkpoint (`LightClient::resume`). Review of core4#3128: a change signed by one member used to capture the client | `ValidatorSetTransitionInvalid` |
| 4 | Producer ed25519 signature over the header's own hash (`block_producer_auth` digest; pinned by `producer_verification_matches_block_producer_auth`) **and** the key is authorised at this height | `InvalidSignature` / `ProducerNotAuthorized` |
| 5 | With a `FinalityVerifier` configured, a certificate is **required** and must verify (2/3+ voting power); a source cannot strip finality by omitting it | `FinalityCertificateMissing` / `InsufficientVotingPower` |
| 6 | Data availability per policy: with a `da_commitment` in the header the sampler (#2186) must return `Available`. The header's **signed** commitment wins; a served commitment that differs rejects the header, and a served one is used only when the header has none | `DataUnavailable` (rejected) / `DataInconclusive` (not advanced) / `DaCommitmentConflict` |

A header that fails any step **does not advance the trusted checkpoint**.

## Trust model — what a light node must be given

* A **trusted checkpoint**: header height + hash, and the producer-set history
  in force there — from the genesis bundle or an operator-verified file, never
  from the node being followed. The client refuses to start with an empty
  producer set: with nothing to check producers against it would be trusting
  the server.
* A **node secret** (≥16 bytes, persisted): sample indices derive from it so
  the serving side cannot predict which chunks will be requested. The bin
  creates it owner-only (mode 0600) and refuses, rather than replaces, an
  existing file that is too short or unreadable.

## Data-availability policy and the #2185 gap

WRFCoin blocks do not yet commit a DA root (that is #2185, feature-gated). Until
they do, the header carries no `da_commitment` and the light node cannot verify
availability against anything the producer signed:

| `--da` | behaviour today | behaviour once #2185 activates |
|---|---|---|
| `required` | refuses every header (`CommitmentMissing`) — the honest strict mode | samples every committed header, but see the single-node note below |
| `when-committed` (default) | accepts headers and **counts** them in `headers_without_commitment` | samples every committed header, but see the single-node note below |
| `off` | header-only following | header-only following |

`--trust-served-da-root` samples against the root the *server* computes over
the block's canonical bytes. That is trusting the server for the layout — it
exercises the sampling path but proves nothing a hostile server could not
fake, which is why it is off by default and labelled.

A single HTTP node is one peer. The sampler needs `distinct_peers_required`
distinct servers before a missing chunk counts as withheld, so with one node a
missing chunk reads `Inconclusive`: a lone server can neither be accused nor
trusted. **Known gap:** the shipped bin takes one `--node`, so once #2185
activates every committed header reads `Inconclusive` and the loop retries:
it does not yet *sample every header* to a verdict. Accepting several
`--node` peers is a follow-up.

## Consensus signal today

The live consensus signal is the producer signature (#2290). BFT finality
certificates exist as a type but no in-tree source emits them (#1388), so
certificate verification is pluggable (`FinalityVerifier`) and the producer
check is mandatory.

## Resource budget (asserted by `resource_budget_holds_over_a_long_follow`)

| Budget | Value | Why |
|---|---|---|
| Bandwidth per block (`BANDWIDTH_BUDGET_PER_BLOCK`) | **64 KiB** | one header (≈200 B + 32 B per transaction) + 16 samples × (1 KiB chunk + sibling path ≈ 1.3 KiB) ≈ 24 KiB; headroom for larger blocks |
| Steady-state memory (`MEMORY_BUDGET_BYTES`) | **1 MiB** | the client retains the checkpoint, a bounded producer history (≤64 changes) and the config — nothing per block; the test asserts the estimate is identical before and after 2,000 headers |

At one block per minute that is under 100 MB/day on a Raspberry Pi at a weather
station.

## Resume

`LightClient::checkpoint()` serialises to JSON; `LightClient::resume` starts
from it. After a disconnect the client continues from the last accepted header
and re-samples nothing below it (`client_resumes_after_disconnect_without_resyncing_from_genesis`).
The binary writes the checkpoint to `--checkpoint <file>` after every batch.

## Serving side

`GET /chain/header/{height}` returns the header projection plus the commitment
this node computes over the block's canonical bytes (`da_commitment_source:
"served"`); `GET /da/chunk/{height}/{index}` returns one chunk with its
inclusion proof. Both are public read routes (`docs/security/RPC_SURFACE.md`).
