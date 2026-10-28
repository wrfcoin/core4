//! Retrieval audits for storage keepers (#3564; design `docs/adr/2026-10-28-3564-storage-keepers.md`).
//!
//! OFF-CHAIN and behind the default-off `retrieval-audit` feature. Nothing on chain reads these types:
//! there is no transaction, state or settlement rule here. They define the four pieces an audit needs:
//!
//! 1. [`ArchiveCommitment`]: a Merkle root over fixed-size chunks of an archived object, fixed at
//!    archive time. A CID alone cannot verify one chunk without the whole object.
//! 2. [`derive_challenges`]: seed-derived, recomputable chunk challenges, so neither the keeper nor
//!    the challenger chooses which bytes are asked for.
//! 3. [`verify_response`]: one chunk plus its Merkle path against the commitment.
//! 4. [`RetrievalAuditReceipt`]: the challenger's signed verdict, a domain-separated preimage in the
//!    `NodeServiceReceipt` style (tag `wrfcoin.retrieval_audit_receipt.v1`, registered in
//!    `shared-protocol`'s domain registry).
//!
//! A PASS receipt can be rechecked by anyone holding the chunk and path. A FAIL receipt is only an
//! accusation: the design routes it through the fraud path, where the keeper can answer with the chunk.

use serde::{Deserialize, Serialize};
use wrfcoin_shared_protocol::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
use wrfcoin_shared_protocol::crypto::domain_impls::BodyBuilder;
use wrfcoin_shared_protocol::crypto::domain_signing::{
    DomainSeparated, sign_domain_for, verify_domain_for,
};
use wrfcoin_shared_protocol::crypto::hash::hash_bytes;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey, Signature};

/// Proposed audit chunk size (64 KiB; inside storage-common's 16 KiB–1 MiB chunk range).
pub const DEFAULT_CHUNK_SIZE: u32 = 64 * 1024;
const LEAF_TAG: &[u8] = b"wrfcoin.archive_chunk.leaf.v1";
const NODE_TAG: &[u8] = b"wrfcoin.archive_chunk.node.v1";
const CHALLENGE_TAG: &[u8] = b"wrfcoin.retrieval_audit.challenge.v1";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuditError {
    #[error("chunk size must be non-zero")]
    ZeroChunkSize,
    #[error("an archived object must not be empty")]
    EmptyObject,
    #[error("keeper holds no archived object to audit")]
    NothingHeld,
}

fn h(parts: &[&[u8]]) -> [u8; 32] {
    let mut buf = Vec::new();
    for part in parts {
        buf.extend_from_slice(&(part.len() as u64).to_le_bytes());
        buf.extend_from_slice(part);
    }
    *hash_bytes(&buf).as_bytes()
}

fn leaf(index: u64, chunk: &[u8]) -> [u8; 32] {
    h(&[LEAF_TAG, &index.to_le_bytes(), chunk])
}

fn node(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    h(&[NODE_TAG, left, right])
}

/// The commitment an archived object is audited against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveCommitment {
    pub dataset_cid: String,
    pub byte_len: u64,
    pub chunk_size: u32,
    pub chunk_count: u64,
    pub merkle_root: [u8; 32],
}

/// Every level of the chunk Merkle tree, leaves first. An odd node is paired with itself.
fn levels(data: &[u8], chunk_size: u32) -> Vec<Vec<[u8; 32]>> {
    let mut level: Vec<[u8; 32]> = data
        .chunks(chunk_size as usize)
        .enumerate()
        .map(|(i, c)| leaf(i as u64, c))
        .collect();
    let mut out = vec![level.clone()];
    while level.len() > 1 {
        level = level
            .chunks(2)
            .map(|pair| node(&pair[0], pair.get(1).unwrap_or(&pair[0])))
            .collect();
        out.push(level.clone());
    }
    out
}

impl ArchiveCommitment {
    /// Built by the uploader (or the keeper, for its own records) from the full object.
    pub fn build(dataset_cid: impl Into<String>, data: &[u8], chunk_size: u32) -> Result<Self, AuditError> {
        if chunk_size == 0 {
            return Err(AuditError::ZeroChunkSize);
        }
        if data.is_empty() {
            return Err(AuditError::EmptyObject);
        }
        let tree = levels(data, chunk_size);
        Ok(Self {
            dataset_cid: dataset_cid.into(),
            byte_len: data.len() as u64,
            chunk_size,
            chunk_count: tree[0].len() as u64,
            merkle_root: tree.last().expect("non-empty")[0],
        })
    }

    /// Expected byte length of chunk `index` (the last chunk may be short).
    pub fn chunk_len(&self, index: u64) -> Option<u64> {
        if index >= self.chunk_count {
            return None;
        }
        let start = index * self.chunk_size as u64;
        Some((self.byte_len - start).min(self.chunk_size as u64))
    }
}

/// A keeper's answer: the chunk bytes and the sibling hashes from the leaf to the root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditResponse {
    pub chunk_index: u64,
    pub chunk: Vec<u8>,
    pub path: Vec<[u8; 32]>,
}

/// What an honest keeper holding the full object returns for a challenge.
pub fn respond(data: &[u8], chunk_size: u32, chunk_index: u64) -> Option<AuditResponse> {
    let tree = levels(data, chunk_size);
    let chunk = data.chunks(chunk_size as usize).nth(chunk_index as usize)?.to_vec();
    let mut path = Vec::with_capacity(tree.len());
    let mut i = chunk_index as usize;
    for level in &tree[..tree.len() - 1] {
        let sibling = i ^ 1;
        path.push(*level.get(sibling).unwrap_or(&level[i]));
        i /= 2;
    }
    Some(AuditResponse { chunk_index, chunk, path })
}

/// One audit: which object a keeper must answer for, and which chunk of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditChallenge {
    pub epoch: u64,
    pub seed: [u8; 32],
    pub keeper_id: String,
    pub ordinal: u32,
    pub dataset_cid: String,
    pub merkle_root: [u8; 32],
    pub chunk_index: u64,
}

fn draw(seed: &[u8; 32], keeper_id: &str, epoch: u64, ordinal: u32, what: &[u8], modulus: u64) -> u64 {
    let d = h(&[CHALLENGE_TAG, seed, keeper_id.as_bytes(), &epoch.to_le_bytes(), &ordinal.to_le_bytes(), what]);
    u64::from_le_bytes(d[..8].try_into().expect("8 bytes")) % modulus
}

/// Blocks whose hashes feed one audit seed (proposed; see the ADR's seed section).
pub const SEED_SPAN_BLOCKS: usize = 8;
const SEED_TAG: &[u8] = b"wrfcoin.retrieval_audit.seed.v1";

/// The audit seed: a hash over the hashes of `SEED_SPAN_BLOCKS` consecutive blocks that all come
/// AFTER the keeper's holdings were committed. A single block hash could be ground by that block's
/// producer; to steer this seed a producer would have to control every block in the span. `None`
/// unless exactly `SEED_SPAN_BLOCKS` hashes are given, in height order.
pub fn combine_seed(block_hashes_in_height_order: &[[u8; 32]]) -> Option<[u8; 32]> {
    if block_hashes_in_height_order.len() != SEED_SPAN_BLOCKS {
        return None;
    }
    let mut parts: Vec<&[u8]> = vec![SEED_TAG];
    parts.extend(block_hashes_in_height_order.iter().map(|b| b.as_slice()));
    Some(h(&parts))
}

/// `count` challenges for one keeper in one epoch. Deterministic in (seed, keeper, epoch, holdings):
/// anyone can recompute them, nobody can choose them. Holdings are canonicalised (sorted by CID and
/// root, duplicates removed) so the order a keeper lists them in cannot steer the draw.
pub fn derive_challenges(
    seed: [u8; 32],
    epoch: u64,
    keeper_id: &str,
    holdings: &[ArchiveCommitment],
    count: u32,
) -> Result<Vec<AuditChallenge>, AuditError> {
    let mut held: Vec<&ArchiveCommitment> = holdings.iter().collect();
    held.sort_by(|a, b| (&a.dataset_cid, a.merkle_root).cmp(&(&b.dataset_cid, b.merkle_root)));
    held.dedup_by(|a, b| a.dataset_cid == b.dataset_cid && a.merkle_root == b.merkle_root);
    if held.is_empty() {
        return Err(AuditError::NothingHeld);
    }
    Ok((0..count)
        .map(|ordinal| {
            let object = held[draw(&seed, keeper_id, epoch, ordinal, b"object", held.len() as u64) as usize];
            AuditChallenge {
                epoch,
                seed,
                keeper_id: keeper_id.to_string(),
                ordinal,
                dataset_cid: object.dataset_cid.clone(),
                merkle_root: object.merkle_root,
                chunk_index: draw(&seed, keeper_id, epoch, ordinal, b"chunk", object.chunk_count),
            }
        })
        .collect())
}

/// Why an audit passed or failed. A missing response is judged by the caller's deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditVerdict {
    Pass,
    WrongChunkIndex,
    WrongChunkLength,
    BadPath,
    NoResponse,
}

impl AuditVerdict {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::WrongChunkIndex => "wrong_chunk_index",
            Self::WrongChunkLength => "wrong_chunk_length",
            Self::BadPath => "bad_path",
            Self::NoResponse => "no_response",
        }
    }
}

/// Verify a keeper's response to `challenge` against the commitment it was drawn from.
pub fn verify_response(
    commitment: &ArchiveCommitment,
    challenge: &AuditChallenge,
    response: Option<&AuditResponse>,
) -> AuditVerdict {
    let Some(response) = response else {
        return AuditVerdict::NoResponse;
    };
    if response.chunk_index != challenge.chunk_index
        || commitment.merkle_root != challenge.merkle_root
        || commitment.dataset_cid != challenge.dataset_cid
    {
        return AuditVerdict::WrongChunkIndex;
    }
    if Some(response.chunk.len() as u64) != commitment.chunk_len(challenge.chunk_index) {
        return AuditVerdict::WrongChunkLength;
    }
    let depth = levels_depth(commitment.chunk_count);
    if response.path.len() != depth {
        return AuditVerdict::BadPath;
    }
    let mut acc = leaf(challenge.chunk_index, &response.chunk);
    let mut i = challenge.chunk_index;
    for sibling in &response.path {
        acc = if i % 2 == 0 { node(&acc, sibling) } else { node(sibling, &acc) };
        i /= 2;
    }
    if acc == commitment.merkle_root {
        AuditVerdict::Pass
    } else {
        AuditVerdict::BadPath
    }
}

fn levels_depth(mut n: u64) -> usize {
    let mut depth = 0;
    while n > 1 {
        n = n.div_ceil(2);
        depth += 1;
    }
    depth
}

/// The challenger's signed verdict on one audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalAuditReceipt {
    pub challenge: AuditChallenge,
    pub verdict: AuditVerdict,
    /// SHA3-256 of the returned chunk (zero for no response), so a PASS names the bytes it saw.
    pub chunk_digest: [u8; 32],
    /// The challenger's ed25519 public key (the receipt's signer).
    pub challenger_pubkey: [u8; 32],
    /// Unix seconds when the challenger issued the receipt.
    pub timestamp: i64,
}

impl DomainSeparated for RetrievalAuditReceipt {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.retrieval_audit_receipt.v1";

    fn canonical_body(&self) -> Vec<u8> {
        let c = &self.challenge;
        BodyBuilder::new()
            .u64_field(c.epoch)
            .bytes_field(&c.seed)
            .str_field(&c.keeper_id)
            .u32_field(c.ordinal)
            .str_field(&c.dataset_cid)
            .bytes_field(&c.merkle_root)
            .u64_field(c.chunk_index)
            .str_field(self.verdict.as_str())
            .bytes_field(&self.chunk_digest)
            .bytes_field(&self.challenger_pubkey)
            .i64_field(self.timestamp)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedRetrievalAuditReceipt {
    pub receipt: RetrievalAuditReceipt,
    pub signature: Vec<u8>,
}

/// Run one audit end to end: verify, then sign the verdict. The receipt the network would carry.
pub fn audit_and_sign(
    commitment: &ArchiveCommitment,
    challenge: &AuditChallenge,
    response: Option<&AuditResponse>,
    challenger: &PrivateKey,
    timestamp: i64,
    identity: &ChainIdentity,
) -> SignedRetrievalAuditReceipt {
    let verdict = verify_response(commitment, challenge, response);
    let receipt = RetrievalAuditReceipt {
        challenge: challenge.clone(),
        verdict,
        chunk_digest: response.map_or([0; 32], |r| *hash_bytes(&r.chunk).as_bytes()),
        challenger_pubkey: PublicKey::from_private_key(challenger).as_bytes().try_into().expect("ed25519 public key is 32 bytes"),
        timestamp,
    };
    let signature = sign_domain_for(challenger, &receipt, identity, SigningProtocolVersion::V2);
    SignedRetrievalAuditReceipt { receipt, signature: signature.as_bytes().to_vec() }
}

impl SignedRetrievalAuditReceipt {
    /// Signature check only: the challenger key named in the receipt signed it, on this chain.
    pub fn verify_signature(&self, identity: &ChainIdentity) -> bool {
        let (Ok(key), Ok(sig)) = (
            PublicKey::from_bytes(&self.receipt.challenger_pubkey),
            Signature::from_bytes(&self.signature),
        ) else {
            return false;
        };
        verify_domain_for(&key, &self.receipt, &sig, identity, SigningProtocolVersion::V2)
    }

    /// Anyone can recheck a PASS from the chunk and path: the receipt is only as good as this.
    pub fn recheck_pass(&self, commitment: &ArchiveCommitment, response: &AuditResponse) -> bool {
        self.receipt.verdict == AuditVerdict::Pass
            && *hash_bytes(&response.chunk).as_bytes() == self.receipt.chunk_digest
            && verify_response(commitment, &self.receipt.challenge, Some(response)) == AuditVerdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn object(len: usize, salt: u8) -> Vec<u8> {
        (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(salt)).collect()
    }

    fn identity() -> ChainIdentity {
        ChainIdentity::new("fixture", "fixture")
    }

    #[test]
    fn domain_tag_matches_the_registry() {
        let entry = wrfcoin_shared_protocol::crypto::domain_registry::entry(
            wrfcoin_shared_protocol::crypto::domain_registry::SignedObjectKind::RetrievalAuditReceipt,
        )
        .expect("registered");
        assert_eq!(entry.tag, RetrievalAuditReceipt::DOMAIN_TAG);
    }

    #[test]
    fn every_chunk_of_honest_objects_passes() {
        for (len, chunk) in [(1, 64), (64, 64), (65, 64), (1000, 64), (64 * 7 + 3, 64), (10_000, 1000)] {
            let data = object(len, 3);
            let c = ArchiveCommitment::build("cid-x", &data, chunk).unwrap();
            assert_eq!(c.chunk_count, (len as u64).div_ceil(chunk as u64));
            for i in 0..c.chunk_count {
                let ch = AuditChallenge {
                    epoch: 1, seed: [0; 32], keeper_id: "k".into(), ordinal: 0,
                    dataset_cid: "cid-x".into(), merkle_root: c.merkle_root, chunk_index: i,
                };
                let r = respond(&data, chunk, i).unwrap();
                assert_eq!(verify_response(&c, &ch, Some(&r)), AuditVerdict::Pass, "len {len} chunk {i}");
            }
        }
    }

    #[test]
    fn tampered_or_wrong_responses_fail_with_a_reason() {
        let data = object(64 * 9 + 10, 1);
        let c = ArchiveCommitment::build("cid-y", &data, 64).unwrap();
        let ch = AuditChallenge {
            epoch: 2, seed: [7; 32], keeper_id: "k".into(), ordinal: 0,
            dataset_cid: "cid-y".into(), merkle_root: c.merkle_root, chunk_index: 4,
        };
        let honest = respond(&data, 64, 4).unwrap();
        assert_eq!(verify_response(&c, &ch, None), AuditVerdict::NoResponse);
        let mut flipped = honest.clone();
        flipped.chunk[0] ^= 1;
        assert_eq!(verify_response(&c, &ch, Some(&flipped)), AuditVerdict::BadPath);
        let other = respond(&data, 64, 5).unwrap();
        assert_eq!(verify_response(&c, &ch, Some(&other)), AuditVerdict::WrongChunkIndex);
        let mut relabelled = other.clone();
        relabelled.chunk_index = 4; // right index, wrong bytes
        assert_eq!(verify_response(&c, &ch, Some(&relabelled)), AuditVerdict::BadPath);
        let mut short = honest.clone();
        short.chunk.pop();
        assert_eq!(verify_response(&c, &ch, Some(&short)), AuditVerdict::WrongChunkLength);
        let mut bad_path = honest.clone();
        bad_path.path[1][0] ^= 1;
        assert_eq!(verify_response(&c, &ch, Some(&bad_path)), AuditVerdict::BadPath);
        let mut truncated = honest;
        truncated.path.pop();
        assert_eq!(verify_response(&c, &ch, Some(&truncated)), AuditVerdict::BadPath);
        // A keeper that kept a different object of the same CID name cannot pass.
        let impostor = object(64 * 9 + 10, 2);
        assert_eq!(verify_response(&c, &ch, Some(&respond(&impostor, 64, 4).unwrap())), AuditVerdict::BadPath);
    }

    #[test]
    fn challenges_are_deterministic_canonical_and_seed_dependent() {
        let a = ArchiveCommitment::build("cid-a", &object(5000, 1), 64).unwrap();
        let b = ArchiveCommitment::build("cid-b", &object(9000, 2), 64).unwrap();
        let held = vec![a.clone(), b.clone()];
        let x = derive_challenges([1; 32], 10, "keeper-1", &held, 16).unwrap();
        assert_eq!(x, derive_challenges([1; 32], 10, "keeper-1", &[b.clone(), a.clone(), a.clone()], 16).unwrap());
        assert_ne!(x, derive_challenges([2; 32], 10, "keeper-1", &held, 16).unwrap());
        assert_ne!(x, derive_challenges([1; 32], 11, "keeper-1", &held, 16).unwrap());
        assert_ne!(x, derive_challenges([1; 32], 10, "keeper-2", &held, 16).unwrap());
        assert!(x.iter().any(|c| c.dataset_cid == "cid-a") && x.iter().any(|c| c.dataset_cid == "cid-b"));
        for c in &x {
            let commit = if c.dataset_cid == "cid-a" { &a } else { &b };
            assert!(c.chunk_index < commit.chunk_count);
        }
        assert_eq!(derive_challenges([1; 32], 10, "k", &[], 4), Err(AuditError::NothingHeld));
    }

    #[test]
    fn a_keeper_missing_half_the_object_is_caught() {
        let data = object(64 * 64, 5);
        let c = ArchiveCommitment::build("cid-h", &data, 64).unwrap();
        let mut caught = 0;
        for epoch in 0..200u64 {
            let seed = h(&[b"epoch-seed", &epoch.to_le_bytes()]);
            let challenges = derive_challenges(seed, epoch, "lazy-keeper", std::slice::from_ref(&c), 4).unwrap();
            // The lazy keeper kept only the first half of the chunks.
            let failed = challenges.iter().any(|ch| {
                let r = (ch.chunk_index < 32).then(|| respond(&data, 64, ch.chunk_index).unwrap());
                verify_response(&c, ch, r.as_ref()) != AuditVerdict::Pass
            });
            caught += failed as u32;
        }
        // 1 - 0.5^4 ≈ 93.75 % of epochs; allow sampling slack.
        assert!(caught >= 170, "caught in {caught}/200 epochs");
    }

    #[test]
    fn seed_needs_the_whole_span_and_every_block_moves_it() {
        let span: Vec<[u8; 32]> = (0..SEED_SPAN_BLOCKS as u8).map(|i| [i; 32]).collect();
        let seed = combine_seed(&span).unwrap();
        assert_eq!(combine_seed(&span[1..]), None);
        for i in 0..SEED_SPAN_BLOCKS {
            let mut other = span.clone();
            other[i][0] ^= 1;
            assert_ne!(combine_seed(&other).unwrap(), seed, "block {i} must influence the seed");
        }
        let mut reordered = span.clone();
        reordered.swap(0, 1);
        assert_ne!(combine_seed(&reordered).unwrap(), seed);
    }

    #[test]
    fn receipts_are_signed_chain_bound_and_recheckable() {
        let data = object(3000, 9);
        let c = ArchiveCommitment::build("cid-r", &data, 256).unwrap();
        let ch = derive_challenges([4; 32], 3, "keeper-r", std::slice::from_ref(&c), 1).unwrap().remove(0);
        let key = PrivateKey::generate(&mut StdRng::seed_from_u64(77));
        let r = respond(&data, 256, ch.chunk_index).unwrap();
        let pass = audit_and_sign(&c, &ch, Some(&r), &key, 1_700_000_000, &identity());
        assert_eq!(pass.receipt.verdict, AuditVerdict::Pass);
        assert!(pass.verify_signature(&identity()));
        assert!(!pass.verify_signature(&ChainIdentity::new("other", "fixture")), "chain-bound");
        assert!(pass.recheck_pass(&c, &r));
        let mut forged = pass.clone();
        forged.receipt.challenge.chunk_index ^= 1;
        assert!(!forged.verify_signature(&identity()), "any field change breaks the signature");

        let fail = audit_and_sign(&c, &ch, None, &key, 1_700_000_000, &identity());
        assert_eq!(fail.receipt.verdict, AuditVerdict::NoResponse);
        assert_eq!(fail.receipt.chunk_digest, [0; 32]);
        assert!(fail.verify_signature(&identity()));
        assert!(!fail.recheck_pass(&c, &r), "a fail receipt never rechecks as a pass");
    }
}
