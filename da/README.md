# wrfcoin-da — data availability (epic #2183)

Canonical chunk layout, blake3 chunk commitments with inclusion proofs, and the
data-availability sampling protocol (#2186). Additive: nothing here writes into
a block. Committing the chunk root into block metadata is #2185's consensus
change and ships behind a `ConsensusFeature`; erasure coding is #2184.

## The confidence a sample gives

With `n = total_chunks` chunks of which `k = data_chunks` suffice to
reconstruct, a withholding producer must hide at least `w = n − k + 1` chunks.
A node drawing `s` distinct chunks uniformly misses every hidden chunk with
probability `C(n−w, s) / C(n, s)`; the detection probability is one minus that
(`confidence::detection_probability`). Availability is therefore always stated
as a confidence, never as a fact — and the claim is only worth anything if the
serving side cannot predict which chunks will be requested, which is why
`sampling::choose_indices` draws from a node-local secret.

| layout | `w` | `s = 1` | `s = 8` | `s = 16` | `s = 32` | samples for 0.99 |
|---|---|---|---|---|---|---|
| uncoded, n = 64 (today, pre-#2184) | 1 | 0.016 | 0.125 | 0.250 | 0.500 | 64 (every chunk) |
| uncoded, n = 1024 | 1 | 0.001 | 0.008 | 0.016 | 0.031 | 1014 |
| half-rate RS, n = 256, k = 128 (#2184 target) | 129 | 0.504 | 0.996 | > 0.99998 | ≈ 1 | 7 |
| half-rate RS, n = 1024, k = 512 | 513 | 0.501 | 0.996 | > 0.99998 | ≈ 1 | 7 |

Read the first two rows plainly: **without erasure coding, sampling only proves
that the sampled chunks exist.** A producer can hide one chunk and a 16-sample
node will notice a quarter of the time. That is the honest state of the crate
until #2184 lands; the sampler already computes the right number for either
case, and `SamplingConfig::confidence_target` records what the operator wanted
so it can be compared with what the layout delivers.

## Three outcomes, not two

* `Available { sampled }` — every requested chunk came back with a valid proof.
* `Unavailable { missing }` — for some chunk, at least `distinct_peers_required`
  distinct peers each said they do not have it (or served bytes that do not
  verify, which is penalised and counted the same way).
* `Inconclusive { reason }` — too few distinct peers, a timeout, a transport
  failure, a refusal. **A timeout is never evidence of withholding**: treating
  it as such would let an adversary manufacture accusations by degrading the
  network.

## Wiring

`SampleSource` is the wire (libp2p request-response, HTTP, or a test double);
`PeerPenalty` is the scoring sink. The p2p transport for sample requests and
the light-node consumer are #2188.
