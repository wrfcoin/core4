//! Data-availability sampling (#2186).
//!
//! The protocol: a node chooses a small subset of chunk indices, asks several
//! **distinct** peers for those chunks with inclusion proofs against the
//! commitment it already trusts, and accepts availability only when every
//! requested sample came back valid.
//!
//! # The one detail that matters
//!
//! [`choose_indices`] derives the indices from a **node-local secret** plus the
//! height and the commitment root — never from the height alone. Height-only
//! derivation is the same for every node, so a withholding producer could
//! publish exactly the chunks that will be asked for and hide the rest; a
//! sampler with a predictable pattern is theatre. With a per-node secret the
//! serving side learns which chunks this node wants only when the request
//! arrives, and different nodes ask for different chunks at the same height.
//!
//! # Outcomes are three, not two
//!
//! * [`SampleOutcome::Available`] — every sample returned a valid proof.
//! * [`SampleOutcome::Unavailable`] — for at least one index, at least
//!   `distinct_peers_required` distinct peers each answered "I do not have it"
//!   (or served an invalid proof, which is penalised and counts the same
//!   way). That corroboration is what turns a missing chunk into evidence of
//!   withholding rather than one flaky peer.
//! * [`SampleOutcome::Inconclusive`] — the sampler could not reach a verdict:
//!   too few distinct peers, a timeout, a transport failure. **A timeout is
//!   never evidence of withholding**; conflating the two would let an
//!   adversary induce false accusations by degrading the network.
//!
//! The wire is abstracted behind [`SampleSource`] so the same protocol runs
//! over libp2p request-response, HTTP, or an in-memory test double.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

use crate::commitment::{ChunkInclusionProof, DaCommitment};

/// Minimum node-secret length accepted by [`choose_indices`]: 16 bytes of
/// entropy, below which the "unpredictable" claim is not one.
pub const NODE_SECRET_MIN_LEN: usize = 16;

/// Domain for the index-derivation key.
const INDEX_KEY_DOMAIN: &str = "wrfcoin.da.sample_index_key.v1";

/// Sampler parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SamplingConfig {
    /// Distinct chunk indices to request per block.
    pub samples_per_block: u16,
    /// The detection probability this node wants; recorded so an operator can
    /// compare it with [`crate::confidence::detection_probability`] for the
    /// layout actually sampled. Not enforced here — a sampler cannot sample
    /// more chunks than exist.
    pub confidence_target: f64,
    /// Per-request deadline; a request past it is a timeout, hence
    /// `Inconclusive`, never `Unavailable`.
    pub request_timeout: Duration,
    /// Distinct peers every index is spread over, and the corroboration
    /// needed before a missing chunk counts as withheld.
    pub distinct_peers_required: u8,
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            samples_per_block: 16,
            confidence_target: 0.99,
            request_timeout: Duration::from_secs(5),
            distinct_peers_required: 2,
        }
    }
}

/// The subset of chunks this node asks for at `height`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleRequest {
    pub height: u64,
    pub indices: Vec<u16>,
}

/// A peer's answer for one index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkResponse {
    pub proof: ChunkInclusionProof,
}

/// Why a fetch produced no chunk.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SampleFetchError {
    /// The peer explicitly reported it does not hold the chunk. Evidence
    /// (once corroborated), because the peer chose to say so.
    #[error("peer does not have the chunk")]
    Missing,
    /// No answer inside the deadline. Not evidence.
    #[error("request timed out")]
    Timeout,
    /// Connection-level failure. Not evidence.
    #[error("transport failure: {0}")]
    Transport(String),
    /// The peer refused to serve (rate limit, policy). Not evidence.
    #[error("peer refused: {0}")]
    Refused(String),
}

/// What the sampler concluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SampleOutcome {
    Available { sampled: u16 },
    Unavailable { missing: Vec<u16> },
    Inconclusive { reason: &'static str },
}

/// Why a peer is penalised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplePenalty {
    /// The proof did not verify against the commitment: the peer served
    /// bytes that are not the committed chunk.
    InvalidProof,
    /// The peer answered a different index than the one requested.
    WrongIndex,
}

/// Errors from [`choose_indices`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SamplingError {
    #[error("node secret must be at least {NODE_SECRET_MIN_LEN} bytes, got {0}")] // secret-leak-ok: interpolates NODE_SECRET_MIN_LEN and the usize length, never the secret bytes
    SecretTooShort(usize),
    #[error("cannot sample a layout with zero chunks")]
    NoChunks,
    #[error("samples_per_block must be at least 1")]
    NoSamples,
}

/// The wire, abstracted: fetch one chunk (with its proof) from one peer.
#[async_trait]
pub trait SampleSource: Send + Sync {
    /// Opaque peer identifier (a libp2p `PeerId` string in production).
    async fn fetch(
        &self,
        peer: &str,
        height: u64,
        index: u16,
    ) -> Result<ChunkResponse, SampleFetchError>;
}

/// Penalty sink: production routes this into `p2p`'s peer scoring; tests
/// record it.
pub trait PeerPenalty: Send + Sync {
    fn penalize(&self, peer: &str, reason: SamplePenalty);
}

/// A penalty sink that drops every report (for callers without scoring).
pub struct NoPenalty;

impl PeerPenalty for NoPenalty {
    fn penalize(&self, _peer: &str, _reason: SamplePenalty) {}
}

/// Choose the distinct chunk indices to sample at `height` under `commitment`.
///
/// The index stream is `blake3::keyed_hash(key, height || root || counter)`
/// with `key = blake3(INDEX_KEY_DOMAIN || node_secret)`: a serving peer that
/// does not know `node_secret` cannot predict the sequence, two nodes with
/// different secrets draw different sequences, and one node draws the same
/// sequence twice for the same block (so a retry asks for the same chunks —
/// switching chunks on retry would let a peer that refused the first set
/// hope for an easier second one). Each 32-byte output yields sixteen
/// candidate `u16`s; candidates are taken modulo `total_chunks` with
/// rejection of the biased tail so every index is equally likely.
///
/// # `node_secret`
///
/// Must be **fresh random bytes** (at least [`NODE_SECRET_MIN_LEN`], e.g. from
/// `OsRng`) generated and kept locally by this node. Never derive it from
/// anything public (the libp2p `PeerId`, the node's public key, its address,
/// the chain id): a secret a serving peer can compute makes the draw
/// predictable again and the sampling is theatre. The derived key and the
/// hasher state that held it are zeroized before this function returns.
pub fn choose_indices(
    height: u64,
    node_secret: &[u8],
    commitment: &DaCommitment,
    cfg: &SamplingConfig,
) -> Result<Vec<u16>, SamplingError> {
    if node_secret.len() < NODE_SECRET_MIN_LEN {
        return Err(SamplingError::SecretTooShort(node_secret.len()));
    }
    let total = commitment.total_chunks;
    if total == 0 {
        return Err(SamplingError::NoChunks);
    }
    if cfg.samples_per_block == 0 {
        return Err(SamplingError::NoSamples);
    }
    let wanted = usize::from(cfg.samples_per_block.min(total));

    // The derived key predicts every draw, so it is as sensitive as the
    // secret: keep it in a `Zeroizing` buffer and wipe every hasher that saw
    // the secret or the key.
    let key: Zeroizing<[u8; 32]> = {
        let mut hasher = blake3::Hasher::new();
        hasher.update(INDEX_KEY_DOMAIN.as_bytes());
        hasher.update(node_secret);
        let mut digest = hasher.finalize();
        hasher.zeroize();
        let key = Zeroizing::new(*digest.as_bytes());
        digest.zeroize();
        key
    };
    // Largest multiple of `total` that fits in u16 space: candidates at or
    // above it are rejected so `candidate % total` is unbiased.
    let total_u32 = u32::from(total);
    let reject_from = (65_536u32 / total_u32) * total_u32;

    let mut chosen: Vec<u16> = Vec::with_capacity(wanted);
    let mut seen: BTreeSet<u16> = BTreeSet::new();
    let mut counter: u64 = 0;
    // `wanted <= total`, so termination is guaranteed: every index has a
    // positive probability per draw and the loop stops at `wanted` distinct.
    while chosen.len() < wanted {
        let mut hasher = blake3::Hasher::new_keyed(&key);
        hasher.update(&height.to_be_bytes());
        hasher.update(&commitment.root);
        hasher.update(&counter.to_be_bytes());
        let block = hasher.finalize();
        hasher.zeroize();
        counter = counter.wrapping_add(1);
        for pair in block.as_bytes().chunks(2) {
            let candidate = u32::from(u16::from_be_bytes([pair[0], pair[1]]));
            if candidate >= reject_from {
                continue;
            }
            let index = (candidate % total_u32) as u16;
            if seen.insert(index) {
                chosen.push(index);
                if chosen.len() == wanted {
                    break;
                }
            }
        }
    }
    Ok(chosen)
}

/// Run one sampling round for `height` against `commitment`.
///
/// Every chosen index is requested from `distinct_peers_required` distinct
/// peers (round-robin over `peers`, so with more peers than required the
/// load spreads). A verdict per index:
///
/// * valid proof from any peer → satisfied;
/// * otherwise, if at least `distinct_peers_required` distinct peers answered
///   `Missing` (or served an invalid proof) → **withheld**;
/// * otherwise → unresolved (some answer was a timeout/transport/refusal).
///
/// Any withheld index ⇒ `Unavailable { missing }`; else any unresolved index
/// ⇒ `Inconclusive`; else `Available`.
pub async fn sample<S: SampleSource + ?Sized, P: PeerPenalty + ?Sized>(
    height: u64,
    commitment: &DaCommitment,
    indices: &[u16],
    cfg: &SamplingConfig,
    peers: &[String],
    source: &S,
    penalty: &P,
) -> SampleOutcome {
    let required = usize::from(cfg.distinct_peers_required.max(1));
    let distinct: BTreeSet<&String> = peers.iter().collect();
    if distinct.len() < required {
        return SampleOutcome::Inconclusive {
            reason: "fewer distinct peers than distinct_peers_required",
        };
    }
    if indices.is_empty() {
        return SampleOutcome::Inconclusive {
            reason: "no indices to sample",
        };
    }
    let peer_ring: Vec<&String> = distinct.into_iter().collect();

    // Per index: which distinct peers said "missing"/served garbage, whether
    // any answer was non-evidential.
    let mut satisfied: BTreeSet<u16> = BTreeSet::new();
    let mut missing_votes: BTreeMap<u16, BTreeSet<String>> = BTreeMap::new();
    let mut unresolved: BTreeSet<u16> = BTreeSet::new();

    for (slot, &index) in indices.iter().enumerate() {
        for k in 0..required {
            let peer = peer_ring[(slot + k) % peer_ring.len()];
            let answer =
                tokio::time::timeout(cfg.request_timeout, source.fetch(peer, height, index)).await;
            match answer {
                Ok(Ok(response)) => {
                    if response.proof.index != index {
                        penalty.penalize(peer, SamplePenalty::WrongIndex);
                        missing_votes.entry(index).or_default().insert(peer.clone());
                    } else if commitment.verify(&response.proof) {
                        satisfied.insert(index);
                        break;
                    } else {
                        penalty.penalize(peer, SamplePenalty::InvalidProof);
                        missing_votes.entry(index).or_default().insert(peer.clone());
                    }
                },
                Ok(Err(SampleFetchError::Missing)) => {
                    missing_votes.entry(index).or_default().insert(peer.clone());
                },
                Ok(Err(SampleFetchError::Timeout))
                | Ok(Err(SampleFetchError::Transport(_)))
                | Ok(Err(SampleFetchError::Refused(_)))
                | Err(_) => {
                    unresolved.insert(index);
                },
            }
        }
    }

    let withheld: Vec<u16> = indices
        .iter()
        .copied()
        .filter(|index| !satisfied.contains(index))
        .filter(|index| {
            missing_votes
                .get(index)
                .map(|peers| peers.len() >= required)
                .unwrap_or(false)
        })
        .collect();
    if !withheld.is_empty() {
        return SampleOutcome::Unavailable { missing: withheld };
    }
    let any_unresolved = indices.iter().any(|index| !satisfied.contains(index));
    if any_unresolved {
        let _ = unresolved;
        return SampleOutcome::Inconclusive {
            reason: "a sample was not served inside the deadline or was refused",
        };
    }
    SampleOutcome::Available {
        sampled: indices.len() as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::ChunkLayout;

    fn commitment(chunks: u16) -> DaCommitment {
        DaCommitment {
            root: [0xab; 32],
            total_chunks: chunks,
            payload_len: u64::from(chunks) * 1024,
        }
    }

    #[test]
    fn indices_are_distinct_in_range_and_deterministic_per_secret() {
        let cfg = SamplingConfig {
            samples_per_block: 20,
            ..Default::default()
        };
        let c = commitment(500);
        let a = choose_indices(42, b"node-secret-aaaaaaaaaaaaaaaa", &c, &cfg).unwrap();
        let b = choose_indices(42, b"node-secret-aaaaaaaaaaaaaaaa", &c, &cfg).unwrap();
        assert_eq!(a, b, "one node re-asks the same chunks for the same block");
        assert_eq!(a.len(), 20);
        let set: BTreeSet<u16> = a.iter().copied().collect();
        assert_eq!(set.len(), 20, "indices are distinct");
        assert!(a.iter().all(|i| *i < 500));
    }

    #[test]
    fn indices_differ_across_nodes_heights_and_roots() {
        let cfg = SamplingConfig {
            samples_per_block: 8,
            ..Default::default()
        };
        let c = commitment(1000);
        let a = choose_indices(7, b"secret-of-node-a-0000000", &c, &cfg).unwrap();
        let b = choose_indices(7, b"secret-of-node-b-0000000", &c, &cfg).unwrap();
        assert_ne!(a, b, "two nodes must not ask for the same chunks");
        let a_next = choose_indices(8, b"secret-of-node-a-0000000", &c, &cfg).unwrap();
        assert_ne!(
            a, a_next,
            "the same node asks for different chunks at the next height"
        );
        let mut other_root = c;
        other_root.root[3] ^= 1;
        let a_other = choose_indices(7, b"secret-of-node-a-0000000", &other_root, &cfg).unwrap();
        assert_ne!(a, a_other, "the commitment root is bound into the draw");
    }

    #[test]
    fn a_serving_peer_cannot_predict_the_draw() {
        // Over many secrets at one height the first index covers the chunk
        // range broadly: a height-only derivation would give one value.
        let cfg = SamplingConfig {
            samples_per_block: 1,
            ..Default::default()
        };
        let c = commitment(64);
        let mut firsts = BTreeSet::new();
        for i in 0..400u32 {
            let secret = format!("secret-{i:0>24}");
            let idx = choose_indices(11, secret.as_bytes(), &c, &cfg).unwrap()[0];
            firsts.insert(idx);
        }
        assert!(
            firsts.len() > 48,
            "400 nodes should cover most of 64 chunks, covered {}",
            firsts.len()
        );
    }

    #[test]
    fn draw_is_unbiased_across_a_non_power_of_two_layout() {
        // 3 chunks: with rejection sampling each index gets ~1/3 of draws.
        let cfg = SamplingConfig {
            samples_per_block: 1,
            ..Default::default()
        };
        let c = commitment(3);
        let mut counts = [0u32; 3];
        for i in 0..3000u32 {
            let secret = format!("bias-probe-{i:0>24}");
            let idx = choose_indices(1, secret.as_bytes(), &c, &cfg).unwrap()[0];
            counts[usize::from(idx)] += 1;
        }
        for c in counts {
            assert!((800..=1200).contains(&c), "skewed draw: {counts:?}");
        }
    }

    #[test]
    fn choose_indices_refuses_weak_or_empty_inputs() {
        let cfg = SamplingConfig::default();
        assert_eq!(
            // secret-leak-ok: test; the trigger is the variant name `SecretTooShort` and a usize length
            choose_indices(1, b"short", &commitment(10), &cfg),
            Err(SamplingError::SecretTooShort(5))
        );
        assert_eq!(
            choose_indices(1, b"0123456789abcdef", &commitment(0), &cfg),
            Err(SamplingError::NoChunks)
        );
        let zero = SamplingConfig {
            samples_per_block: 0,
            ..Default::default()
        };
        assert_eq!(
            choose_indices(1, b"0123456789abcdef", &commitment(10), &zero),
            Err(SamplingError::NoSamples)
        );
        // More samples than chunks: capped at the layout, all distinct.
        let big = SamplingConfig {
            samples_per_block: 50,
            ..Default::default()
        };
        let got = choose_indices(1, b"0123456789abcdef", &commitment(5), &big).unwrap();
        assert_eq!(got.len(), 5);
    }

    #[test]
    fn layout_backed_commitment_verifies_real_proofs_end_to_end() {
        let payload: Vec<u8> = (0..(1024 * 9 + 3)).map(|i| (i % 199) as u8).collect();
        let layout = ChunkLayout::for_payload(&payload).unwrap();
        let c = DaCommitment::commit(&layout);
        let cfg = SamplingConfig {
            samples_per_block: 4,
            ..Default::default()
        };
        for index in choose_indices(3, b"proof-check-fixture-", &c, &cfg).unwrap() {
            let proof = DaCommitment::prove(&layout, index).unwrap();
            assert!(c.verify(&proof));
        }
    }
}
