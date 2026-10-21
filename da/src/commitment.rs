//! Chunk commitment with inclusion proofs (#2183; the shape #2185 gates).
//!
//! A blake3 Merkle tree over the canonical chunk layout. Leaves and inner
//! nodes are domain-separated so a chunk can never be presented as a node
//! (the classic second-preimage trick against unseparated Merkle trees), and
//! the leaf binds its index so a proof for chunk `i` cannot be replayed as a
//! proof for chunk `j`. Odd levels are closed by hashing the lone node with
//! itself, which keeps the tree a pure function of the chunk list.
//!
//! Nothing here touches a block: writing `root` into block metadata is the
//! consensus change #2185 owns, behind a `ConsensusFeature`. A sampler
//! (#2186) consumes a commitment it obtained elsewhere — from a header once
//! #2185 lands, from a trusted checkpoint until then.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::layout::{CHUNK_SIZE, ChunkLayout};

/// Leaf hash domain: `blake3(DA_LEAF_DOMAIN || index_be || chunk)`.
pub const DA_LEAF_DOMAIN: &[u8] = b"wrfcoin.da.leaf.v1";
/// Inner-node hash domain: `blake3(DA_NODE_DOMAIN || left || right)`.
pub const DA_NODE_DOMAIN: &[u8] = b"wrfcoin.da.node.v1";

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DaCommitmentError {
    #[error("chunk index {index} is outside the committed layout of {total_chunks} chunks")]
    IndexOutOfRange { index: u16, total_chunks: u16 },
}

/// The commitment a sampler verifies samples against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DaCommitment {
    /// Merkle root over the chunk leaves.
    pub root: [u8; 32],
    /// Number of chunks in the committed layout.
    pub total_chunks: u16,
    /// Original payload length, so a reconstruction can drop the padding.
    pub payload_len: u64,
}

/// One chunk plus the sibling path proving it sits at `index` under a root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkInclusionProof {
    pub index: u16,
    #[serde(with = "chunk_bytes")]
    pub chunk: [u8; CHUNK_SIZE],
    /// Sibling hashes from the leaf up; `siblings[i]` pairs with the running
    /// hash at level `i`, on the side `index >> i` says.
    pub siblings: Vec<[u8; 32]>,
}

mod chunk_bytes {
    use super::CHUNK_SIZE;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(chunk: &[u8; CHUNK_SIZE], s: S) -> Result<S::Ok, S::Error> {
        chunk.as_slice().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; CHUNK_SIZE], D::Error> {
        let bytes: Vec<u8> = Vec::deserialize(d)?;
        <[u8; CHUNK_SIZE]>::try_from(bytes.as_slice())
            .map_err(|_| serde::de::Error::invalid_length(bytes.len(), &"exactly CHUNK_SIZE bytes"))
    }
}

/// Domain-separated leaf hash binding the chunk to its index.
pub fn leaf_hash(index: u16, chunk: &[u8; CHUNK_SIZE]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DA_LEAF_DOMAIN);
    hasher.update(&index.to_be_bytes());
    hasher.update(chunk);
    *hasher.finalize().as_bytes()
}

/// Domain-separated inner-node hash.
pub fn node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DA_NODE_DOMAIN);
    hasher.update(left);
    hasher.update(right);
    *hasher.finalize().as_bytes()
}

/// Every level of the tree, leaves first. Level `k+1` pairs level `k` left to
/// right, a lone trailing node paired with itself.
fn levels(leaves: Vec<[u8; 32]>) -> Vec<Vec<[u8; 32]>> {
    let mut levels = vec![leaves];
    while levels.last().map(Vec::len).unwrap_or(0) > 1 {
        let prev = levels.last().expect("at least one level");
        let mut next = Vec::with_capacity(prev.len().div_ceil(2));
        for pair in prev.chunks(2) {
            let left = &pair[0];
            let right = pair.get(1).unwrap_or(left);
            next.push(node_hash(left, right));
        }
        levels.push(next);
    }
    levels
}

impl DaCommitment {
    /// Commit to a layout.
    pub fn commit(layout: &ChunkLayout) -> Self {
        let leaves: Vec<[u8; 32]> = layout
            .chunks()
            .iter()
            .enumerate()
            .map(|(i, chunk)| leaf_hash(i as u16, chunk))
            .collect();
        let tree = levels(leaves);
        let root = tree
            .last()
            .and_then(|level| level.first())
            .copied()
            .expect("a non-empty layout has a root");
        Self {
            root,
            total_chunks: layout.total_chunks(),
            payload_len: layout.payload_len(),
        }
    }

    /// Build the inclusion proof for `index` from the full layout (the serving
    /// side; a light node only ever verifies).
    pub fn prove(
        layout: &ChunkLayout,
        index: u16,
    ) -> Result<ChunkInclusionProof, DaCommitmentError> {
        let total_chunks = layout.total_chunks();
        let chunk = *layout
            .chunk(index)
            .ok_or(DaCommitmentError::IndexOutOfRange {
                index,
                total_chunks,
            })?;
        let leaves: Vec<[u8; 32]> = layout
            .chunks()
            .iter()
            .enumerate()
            .map(|(i, c)| leaf_hash(i as u16, c))
            .collect();
        let tree = levels(leaves);
        let mut siblings = Vec::with_capacity(tree.len());
        let mut pos = usize::from(index);
        for level in &tree[..tree.len() - 1] {
            let sibling_pos = pos ^ 1;
            let sibling = level.get(sibling_pos).unwrap_or(&level[pos]);
            siblings.push(*sibling);
            pos >>= 1;
        }
        Ok(ChunkInclusionProof {
            index,
            chunk,
            siblings,
        })
    }

    /// Verify that `proof` places its chunk at `proof.index` under this root.
    /// The index range is checked first so an out-of-range proof can never
    /// hash its way to the root by accident of tree shape.
    pub fn verify(&self, proof: &ChunkInclusionProof) -> bool {
        if proof.index >= self.total_chunks {
            return false;
        }
        let expected_depth = tree_depth(self.total_chunks);
        if proof.siblings.len() != expected_depth {
            return false;
        }
        let mut running = leaf_hash(proof.index, &proof.chunk);
        let mut pos = usize::from(proof.index);
        for sibling in &proof.siblings {
            running = if pos & 1 == 0 {
                node_hash(&running, sibling)
            } else {
                node_hash(sibling, &running)
            };
            pos >>= 1;
        }
        running == self.root
    }
}

/// Number of sibling levels a proof carries for a tree of `total_chunks`
/// leaves: `ceil(log2(total_chunks))`, and 0 for a single-chunk payload.
pub fn tree_depth(total_chunks: u16) -> usize {
    let mut depth = 0;
    let mut width = usize::from(total_chunks);
    while width > 1 {
        width = width.div_ceil(2);
        depth += 1;
    }
    depth
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(n_chunks: usize, tail: usize) -> Vec<u8> {
        (0..(CHUNK_SIZE * n_chunks + tail))
            .map(|i| (i * 7 % 253) as u8)
            .collect()
    }

    #[test]
    fn every_chunk_proves_under_the_root_and_nothing_else_does() {
        for (n, tail) in [(0, 1), (0, 100), (1, 0), (1, 3), (2, 0), (4, 9), (6, 500)] {
            let layout = ChunkLayout::for_payload(&payload(n, tail)).unwrap();
            let commitment = DaCommitment::commit(&layout);
            assert_eq!(commitment.total_chunks, layout.total_chunks());
            for index in 0..layout.total_chunks() {
                let proof = DaCommitment::prove(&layout, index).unwrap();
                assert_eq!(proof.siblings.len(), tree_depth(layout.total_chunks()));
                assert!(
                    commitment.verify(&proof),
                    "chunk {index} of {} must verify",
                    layout.total_chunks()
                );

                // Tampered chunk: fails.
                let mut bad = proof.clone();
                bad.chunk[0] ^= 0x01;
                assert!(!commitment.verify(&bad));
                // Replayed at another index: fails (the leaf binds the index).
                if layout.total_chunks() > 1 {
                    let mut moved = proof.clone();
                    moved.index = (index + 1) % layout.total_chunks();
                    assert!(!commitment.verify(&moved));
                }
                // Wrong root: fails.
                let mut other = commitment;
                other.root[0] ^= 0xff;
                assert!(!other.verify(&proof));
            }
            // Out of range index is refused before hashing.
            let mut oob = DaCommitment::prove(&layout, 0).unwrap();
            oob.index = layout.total_chunks();
            assert!(!commitment.verify(&oob));
            assert_eq!(
                DaCommitment::prove(&layout, layout.total_chunks()),
                Err(DaCommitmentError::IndexOutOfRange {
                    index: layout.total_chunks(),
                    total_chunks: layout.total_chunks()
                })
            );
        }
    }

    #[test]
    fn commitment_is_deterministic_and_content_sensitive() {
        let a = DaCommitment::commit(&ChunkLayout::for_payload(&payload(3, 5)).unwrap());
        let b = DaCommitment::commit(&ChunkLayout::for_payload(&payload(3, 5)).unwrap());
        assert_eq!(a, b);
        let mut bytes = payload(3, 5);
        bytes[CHUNK_SIZE + 1] ^= 1;
        let c = DaCommitment::commit(&ChunkLayout::for_payload(&bytes).unwrap());
        assert_ne!(a.root, c.root);
        // Padding is not payload: two payloads differing only by trailing
        // length commit differently (payload_len is part of the commitment).
        let d = DaCommitment::commit(&ChunkLayout::for_payload(&payload(3, 6)).unwrap());
        assert_ne!(a, d);
    }

    #[test]
    fn leaf_and_node_domains_are_separated() {
        // A leaf hash can never equal a node hash over the same 32+32 bytes
        // because the domain prefixes differ; pin the prefixes so a refactor
        // cannot silently merge them.
        assert_ne!(DA_LEAF_DOMAIN, DA_NODE_DOMAIN);
        let chunk = [0u8; CHUNK_SIZE];
        let leaf = leaf_hash(0, &chunk);
        let node = node_hash(&[0u8; 32], &[0u8; 32]);
        assert_ne!(leaf, node);
    }

    #[test]
    fn tree_depth_matches_ceil_log2() {
        assert_eq!(tree_depth(1), 0);
        assert_eq!(tree_depth(2), 1);
        assert_eq!(tree_depth(3), 2);
        assert_eq!(tree_depth(4), 2);
        assert_eq!(tree_depth(5), 3);
        assert_eq!(tree_depth(1024), 10);
        assert_eq!(tree_depth(u16::MAX), 16);
    }

    #[test]
    fn proof_round_trips_through_serde() {
        let layout = ChunkLayout::for_payload(&payload(2, 40)).unwrap();
        let proof = DaCommitment::prove(&layout, 2).unwrap();
        let json = serde_json::to_string(&proof).unwrap();
        let back: ChunkInclusionProof = serde_json::from_str(&json).unwrap();
        assert_eq!(back, proof);
    }
}
