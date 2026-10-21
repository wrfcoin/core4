//! The header a light node verifies without transaction bodies (#2188).
//!
//! A WRFCoin block has no separate header: its hash covers every field plus
//! the list of *transaction hashes* (see `Block::canonical_body`). A light node
//! therefore needs exactly the header fields and those hashes — 32 bytes per
//! transaction instead of the bodies — to recompute the block hash, check the
//! producer signature and chain headers together. [`LightHeader`] is that
//! projection.
//!
//! **Byte parity, not a second encoding.** `canonical_body` here reproduces
//! `Block::canonical_body` field for field from the projected data; the
//! `header_hash_matches_block_hash_for_every_shape` test pins that a header
//! built from any block hashes to the block's own hash, and
//! `producer_verification_matches_block_producer_auth` pins that the producer
//! digest is the same one `block_producer_auth` signs. Nothing about how a
//! block hashes changes — this is a view, and the tests are what keep it one.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::blockchain::block::Block;
use crate::consensus::block_producer_auth::{
    BLOCK_PRODUCER_DOMAIN_TAG, ProducerAttestation, ProducerAuthError,
};
use crate::crypto::chain_binding::{
    ChainIdentity, SigningProtocolVersion, domain_preimage_bound, protocol_version_at,
};
use crate::crypto::domain_impls::BodyBuilder;
use crate::crypto::domain_signing::DomainSeparated;
use crate::crypto::hash::{Hash, hash_bytes};
use crate::crypto::signature::{PublicKey, Signature};

/// Why a header failed a local check.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LightHeaderError {
    #[error("declared hash {declared} does not match the recomputed hash {computed}")]
    HashMismatch { declared: Hash, computed: Hash },
    #[error(
        "header at height {height} names prev {prev} but the trusted header hash is {expected}"
    )]
    ChainBroken {
        height: u64,
        prev: Hash,
        expected: Hash,
    },
    #[error("header height {got} is not {expected}")]
    HeightNotNext { got: u64, expected: u64 },
    #[error("producer signature rejected: {0:?}")]
    Producer(ProducerAuthError),
}

/// Header-only projection of a [`Block`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightHeader {
    pub version: u32,
    pub prev_block_hash: Hash,
    pub height: u64,
    pub difficulty: u32,
    pub nonce: u64,
    pub timestamp: DateTime<Utc>,
    pub producer_pubkey: [u8; 32],
    pub producer_sig: Vec<u8>,
    /// `Transaction::calculate_hash()` of every transaction, in block order.
    pub tx_hashes: Vec<Hash>,
    /// Block metadata; a `BTreeMap` so the JSON form is canonical.
    pub metadata: BTreeMap<String, String>,
    /// The hash the serving side declares. Never trusted: see
    /// [`LightHeader::verify_declared_hash`].
    pub declared_hash: Hash,
}

impl LightHeader {
    /// Project a block. `declared_hash` is the block's sealed hash, or the
    /// recomputed one when the block was never sealed.
    pub fn from_block(block: &Block) -> Self {
        Self {
            version: block.version,
            prev_block_hash: block.prev_block_hash,
            height: block.height,
            difficulty: block.difficulty,
            nonce: block.nonce,
            timestamp: block.timestamp,
            producer_pubkey: block.producer_pubkey,
            producer_sig: block.producer_sig.clone(),
            tx_hashes: block
                .transactions
                .iter()
                .map(|tx| tx.calculate_hash())
                .collect(),
            metadata: block
                .metadata
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            declared_hash: block.hash.unwrap_or_else(|| block.calculate_hash()),
        }
    }

    /// Field-for-field mirror of `Block::canonical_body` over the projection.
    pub fn canonical_body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .u32_field(self.version)
            .bytes_field(self.prev_block_hash.as_bytes())
            .u64_field(self.height)
            .u32_field(self.difficulty)
            .u64_field(self.nonce)
            .i64_field(self.timestamp.timestamp_millis())
            .bytes_field(&self.producer_pubkey)
            .seq_field(self.tx_hashes.iter().map(|h| h.as_bytes().to_vec()))
            .sorted_map_field(self.metadata.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .finish()
    }

    /// The block hash under an explicit identity and version — the same
    /// bytes as `Block::calculate_hash_for(version, Some(identity))`.
    pub fn hash_for(&self, identity: &ChainIdentity, version: SigningProtocolVersion) -> Hash {
        hash_bytes(&domain_preimage_bound(
            <Block as DomainSeparated>::DOMAIN_TAG,
            identity,
            version,
            &self.canonical_body(),
        ))
    }

    /// The block hash under the version in force at this height and the
    /// process's active chain identity — the same resolution as
    /// `Block::calculate_hash`.
    pub fn hash(&self) -> Hash {
        let version = protocol_version_at(self.height);
        match version {
            SigningProtocolVersion::V1 => self.hash_for(&ChainIdentity::new("", ""), version),
            SigningProtocolVersion::V2 => self.hash_for(ChainIdentity::active(), version),
        }
    }

    /// The declared hash must be the recomputed one; a serving node cannot
    /// name a header something it is not.
    pub fn verify_declared_hash(&self) -> Result<Hash, LightHeaderError> {
        let computed = self.hash();
        if computed != self.declared_hash {
            return Err(LightHeaderError::HashMismatch {
                declared: self.declared_hash,
                computed,
            });
        }
        Ok(computed)
    }

    /// The producer attestation the header carries.
    pub fn producer_attestation(&self) -> ProducerAttestation {
        let producer_sig = <[u8; 64]>::try_from(self.producer_sig.as_slice()).unwrap_or([0u8; 64]);
        ProducerAttestation {
            producer_pubkey: self.producer_pubkey,
            producer_sig,
        }
    }

    /// Verify the producer signature exactly as `block_producer_auth::verify_block_producer`
    /// does — over the digest `hash(domain_preimage_bound(BLOCK_PRODUCER_DOMAIN_TAG, …))`,
    /// which for a block is its own hash — and return the producer key.
    pub fn verify_producer(
        &self,
        identity: &ChainIdentity,
        version: SigningProtocolVersion,
    ) -> Result<PublicKey, ProducerAuthError> {
        let attestation = self.producer_attestation();
        if attestation.is_unsigned() {
            return Err(ProducerAuthError::Unsigned);
        }
        let pubkey = PublicKey::from_bytes(&attestation.producer_pubkey)
            .map_err(|_| ProducerAuthError::MalformedKey)?;
        let signature = Signature::from_bytes(&attestation.producer_sig)
            .map_err(|_| ProducerAuthError::BadSignature)?;
        let digest = hash_bytes(&domain_preimage_bound(
            BLOCK_PRODUCER_DOMAIN_TAG,
            identity,
            version,
            &self.canonical_body(),
        ));
        if pubkey.verify(digest.as_bytes(), &signature) {
            Ok(pubkey)
        } else {
            Err(ProducerAuthError::BadSignature)
        }
    }

    /// Chain check: this header must be exactly the next height and name the
    /// trusted header's hash as its parent.
    pub fn verify_chains_onto(
        &self,
        trusted_height: u64,
        trusted_hash: Hash,
    ) -> Result<(), LightHeaderError> {
        let expected = trusted_height.saturating_add(1);
        if self.height != expected {
            return Err(LightHeaderError::HeightNotNext {
                got: self.height,
                expected,
            });
        }
        if self.prev_block_hash != trusted_hash {
            return Err(LightHeaderError::ChainBroken {
                height: self.height,
                prev: self.prev_block_hash,
                expected: trusted_hash,
            });
        }
        Ok(())
    }

    /// Wire size of the projection (what a light node downloads per block).
    pub fn wire_len_estimate(&self) -> usize {
        // fixed fields + 32 bytes per tx hash + metadata text + signature
        4 + 32
            + 8
            + 4
            + 8
            + 8
            + 32
            + self.producer_sig.len()
            + self.tx_hashes.len() * 32
            + self
                .metadata
                .iter()
                .map(|(k, v)| k.len() + v.len())
                .sum::<usize>()
            + 32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::transaction::{Transaction, TransactionInput, TransactionOutput};
    use crate::consensus::block_producer_auth::{sign_block_producer, verify_block_producer_on};
    use crate::crypto::hash::hash_string;
    use crate::crypto::signature::PrivateKey;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn tx(tag: &str) -> Transaction {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_string(&format!("in-{tag}")),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 7,
                address: format!("0x{}", "ab".repeat(20)),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        );
        tx.data = Some(format!("data-{tag}"));
        tx.set_id();
        tx
    }

    fn shapes() -> Vec<Block> {
        let mut out = Vec::new();
        for (n_tx, with_meta, height) in [
            (0, false, 1),
            (1, true, 2),
            (3, true, 11_000),
            (5, false, u64::MAX / 2),
        ] {
            let txs: Vec<Transaction> = (0..n_tx).map(|i| tx(&format!("{height}-{i}"))).collect();
            let mut block = Block::new(3, hash_string(&format!("prev-{height}")), txs, 9, height);
            block.nonce = height.wrapping_mul(3).wrapping_add(1);
            if with_meta {
                block.metadata.insert("state_root".into(), "ab".repeat(32));
                block.metadata.insert("consensus_type".into(), "pop".into());
            }
            let hash = block.calculate_hash();
            block.set_hash(hash);
            out.push(block);
        }
        out
    }

    #[test]
    fn header_hash_matches_block_hash_for_every_shape() {
        let identity = ChainIdentity::new("wrfcoin-test", "testnet");
        for block in shapes() {
            let header = LightHeader::from_block(&block);
            assert_eq!(header.tx_hashes.len(), block.transactions.len());
            for version in [SigningProtocolVersion::V1, SigningProtocolVersion::V2] {
                assert_eq!(
                    header.hash_for(&identity, version),
                    block.calculate_hash_for(version, Some(&identity)),
                    "height {} v{version:?}",
                    block.height
                );
            }
            // Process-resolved hash equals the block's own resolution.
            assert_eq!(header.hash(), block.calculate_hash());
            assert_eq!(header.declared_hash, block.hash.unwrap());
            assert_eq!(header.verify_declared_hash().unwrap(), block.hash.unwrap());
            // The canonical body is byte-identical to the block's.
            assert_eq!(
                header.canonical_body(),
                <Block as DomainSeparated>::canonical_body(&block)
            );
        }
    }

    #[test]
    fn a_declared_hash_that_is_not_the_recomputed_one_is_refused() {
        let block = shapes().remove(1);
        let mut header = LightHeader::from_block(&block);
        header.declared_hash = hash_string("something else");
        assert!(matches!(
            header.verify_declared_hash(),
            Err(LightHeaderError::HashMismatch { .. })
        ));
        // Tampering any projected field moves the recomputed hash.
        let mut tampered = LightHeader::from_block(&block);
        tampered.tx_hashes[0] = hash_string("swapped");
        assert_ne!(tampered.hash(), block.calculate_hash());
        let mut tampered = LightHeader::from_block(&block);
        tampered
            .metadata
            .insert("state_root".into(), "cd".repeat(32));
        assert_ne!(tampered.hash(), block.calculate_hash());
    }

    #[test]
    fn producer_verification_matches_block_producer_auth() {
        let identity = ChainIdentity::new("wrfcoin-test", "testnet");
        let version = SigningProtocolVersion::V2;
        let mut rng = StdRng::seed_from_u64(2188);
        let signer = PrivateKey::generate(&mut rng);
        for mut block in shapes() {
            // Attest exactly as the producer path does: pubkey into the block
            // first (it is in the hash), then sign the digest, then seal.
            block.producer_pubkey = PublicKey::from_private_key(&signer)
                .as_bytes()
                .try_into()
                .unwrap();
            let attestation = sign_block_producer(&block, &signer, &identity, version);
            block.producer_sig = attestation.producer_sig.to_vec();
            let sealed = block.calculate_hash_for(version, Some(&identity));
            block.set_hash(sealed);

            let header = LightHeader::from_block(&block);
            let via_block =
                verify_block_producer_on(&block, &identity, version).expect("block verifies");
            let via_header = header
                .verify_producer(&identity, version)
                .expect("header verifies");
            assert_eq!(via_block.as_bytes(), via_header.as_bytes());

            // A forged signature and an unsigned header are both refused.
            let mut forged = header.clone();
            forged.producer_sig[5] ^= 0x01;
            assert_eq!(
                forged.verify_producer(&identity, version).unwrap_err(),
                ProducerAuthError::BadSignature
            );
            let mut unsigned = header.clone();
            unsigned.producer_sig = Vec::new();
            assert_eq!(
                unsigned.verify_producer(&identity, version).unwrap_err(),
                ProducerAuthError::Unsigned
            );
            // Wrong identity: the digest binds the chain, so it fails.
            let other = ChainIdentity::new("wrfcoin-other", "testnet");
            assert!(header.verify_producer(&other, version).is_err());
        }
    }

    #[test]
    fn chain_check_requires_next_height_and_parent_hash() {
        let blocks = shapes();
        let a = LightHeader::from_block(&blocks[0]);
        let mut next = blocks[1].clone();
        next.prev_block_hash = a.declared_hash;
        next.height = a.height + 1;
        let h = next.calculate_hash();
        next.set_hash(h);
        let b = LightHeader::from_block(&next);
        assert!(b.verify_chains_onto(a.height, a.declared_hash).is_ok());
        assert!(matches!(
            b.verify_chains_onto(a.height + 1, a.declared_hash),
            Err(LightHeaderError::HeightNotNext { .. })
        ));
        assert!(matches!(
            b.verify_chains_onto(a.height, hash_string("not the parent")),
            Err(LightHeaderError::ChainBroken { .. })
        ));
    }

    #[test]
    fn projection_round_trips_through_serde() {
        let block = shapes().remove(2);
        let header = LightHeader::from_block(&block);
        let json = serde_json::to_string(&header).unwrap();
        let back: LightHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(back, header);
        assert_eq!(back.hash(), block.calculate_hash());
        assert!(
            header.wire_len_estimate() < 2_048,
            "{} bytes",
            header.wire_len_estimate()
        );
    }
}
