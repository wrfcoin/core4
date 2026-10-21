//! Node-service receipts: a consumer's signed statement that a serving node
//! provided verifiable service (#2848 Phase C subject; issued today for
//! snapshot-join bootstraps).
//!
//! A signature proves control of the named consumer key and commits to the
//! claimed service; it does not prove the signer fetched or applied anything.
//! The serving API binds admission to its exported snapshot and bounds ledger
//! writes without evicting accepted evidence. Self-dealing, stake floors and
//! on-chain settlement remain policy for #2848, not properties of this type.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
use crate::crypto::domain_impls::BodyBuilder;
use crate::crypto::domain_signing::{DomainSeparated, sign_domain_for, verify_domain_for};
use crate::crypto::hash::hash_bytes;
use crate::crypto::signature::{PrivateKey, PublicKey, Signature};

/// What was served.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeService {
    /// A signed state snapshot was fetched, verified against the trusted
    /// signer set and the signed root, and applied.
    SnapshotJoin,
    /// A block range was served over block-sync (reserved for #2848).
    SyncRange,
}

impl NodeService {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SnapshotJoin => "snapshot_join",
            Self::SyncRange => "sync_range",
        }
    }
}

/// The unsigned receipt body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeServiceReceipt {
    pub service: NodeService,
    /// The serving node as the consumer addressed it (peer id, or the URL
    /// authority for HTTP service).
    pub server_id: String,
    /// The consumer's ed25519 public key — the receipt's signer.
    pub consumer_pubkey: [u8; 32],
    /// Chain height the service delivered (snapshot height / range end).
    pub chain_height: u64,
    /// What was served: the snapshot root, or the block-range descriptor hash.
    pub subject_hash: [u8; 32],
    /// Size of the service (entries, or blocks).
    pub units: u64,
    /// Unix seconds when the consumer issued the receipt.
    pub timestamp: i64,
    /// Consumer-chosen uniqueness; with `(consumer, server, subject)` it is the
    /// dedup key.
    pub nonce: [u8; 32],
}

impl DomainSeparated for NodeServiceReceipt {
    const DOMAIN_TAG: &'static [u8] = b"wrfcoin.node_service_receipt.v1";

    fn canonical_body(&self) -> Vec<u8> {
        BodyBuilder::new()
            .str_field(self.service.as_str())
            .str_field(&self.server_id)
            .bytes_field(&self.consumer_pubkey)
            .u64_field(self.chain_height)
            .bytes_field(&self.subject_hash)
            .u64_field(self.units)
            .i64_field(self.timestamp)
            .bytes_field(&self.nonce)
            .finish()
    }
}

/// A receipt plus the consumer's signature over its domain-separated preimage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedNodeServiceReceipt {
    pub receipt: NodeServiceReceipt,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReceiptError {
    #[error("consumer public key is malformed")]
    MalformedKey,
    #[error("signature is malformed or does not verify")]
    BadSignature,
    #[error(
        "receipt timestamp {timestamp} is outside the freshness window around {now} (±{window_secs}s)"
    )]
    Stale {
        timestamp: i64,
        now: i64,
        window_secs: i64,
    },
    #[error("receipt names server {got:?} but this node is {expected:?}")]
    WrongServer { got: String, expected: String },
    #[error("receipt already recorded")]
    Duplicate,
}

impl SignedNodeServiceReceipt {
    pub fn sign(
        receipt: NodeServiceReceipt,
        signer: &PrivateKey,
        identity: &ChainIdentity,
        version: SigningProtocolVersion,
    ) -> Self {
        let signature = sign_domain_for(signer, &receipt, identity, version);
        Self {
            receipt,
            signature: signature.as_bytes().to_vec(),
        }
    }

    /// Sign with any `BlockSigner` (HSM-backed in production): the signer
    /// signs the domain-separated preimage bytes directly.
    pub fn sign_with(
        receipt: NodeServiceReceipt,
        signer: &dyn crate::crypto::signer::BlockSigner,
        identity: &ChainIdentity,
        version: SigningProtocolVersion,
    ) -> anyhow::Result<Self> {
        let preimage = receipt.signing_preimage_for(identity, version);
        let signature = signer.sign(&preimage)?;
        Ok(Self {
            receipt,
            signature: signature.as_bytes().to_vec(),
        })
    }

    /// Signature check only: the consumer key named in the receipt signed it.
    pub fn verify_signature(
        &self,
        identity: &ChainIdentity,
        version: SigningProtocolVersion,
    ) -> Result<PublicKey, ReceiptError> {
        let key = PublicKey::from_bytes(&self.receipt.consumer_pubkey)
            .map_err(|_| ReceiptError::MalformedKey)?;
        let signature =
            Signature::from_bytes(&self.signature).map_err(|_| ReceiptError::BadSignature)?;
        if verify_domain_for(&key, &self.receipt, &signature, identity, version) {
            Ok(key)
        } else {
            Err(ReceiptError::BadSignature)
        }
    }

    /// Full acceptance check on the serving side: signature, freshness
    /// (`|timestamp − now| ≤ window`), and that the receipt names this server.
    pub fn verify(
        &self,
        identity: &ChainIdentity,
        version: SigningProtocolVersion,
        expected_server_id: &str,
        now: i64,
        freshness_window_secs: i64,
    ) -> Result<PublicKey, ReceiptError> {
        if self.receipt.server_id != expected_server_id {
            return Err(ReceiptError::WrongServer {
                got: self.receipt.server_id.clone(),
                expected: expected_server_id.to_string(),
            });
        }
        let skew = (self.receipt.timestamp - now).abs();
        if skew > freshness_window_secs {
            return Err(ReceiptError::Stale {
                timestamp: self.receipt.timestamp,
                now,
                window_secs: freshness_window_secs,
            });
        }
        self.verify_signature(identity, version)
    }

    /// Dedup key: one receipt per `(consumer, server, subject)`.
    pub fn dedup_key(&self) -> [u8; 32] {
        let mut m = Vec::with_capacity(32 + self.receipt.server_id.len() + 32);
        m.extend_from_slice(&self.receipt.consumer_pubkey);
        m.extend_from_slice(self.receipt.server_id.as_bytes());
        m.extend_from_slice(&self.receipt.subject_hash);
        *hash_bytes(&m).as_bytes()
    }
}

/// Hash a snapshot root string into the receipt's `subject_hash`.
pub fn snapshot_subject_hash(snapshot_root_hex: &str) -> [u8; 32] {
    *hash_bytes(snapshot_root_hex.trim().to_ascii_lowercase().as_bytes()).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn consumer(seed: u64) -> PrivateKey {
        PrivateKey::generate(&mut StdRng::seed_from_u64(seed))
    }

    fn receipt(sk: &PrivateKey, server: &str, ts: i64) -> NodeServiceReceipt {
        NodeServiceReceipt {
            service: NodeService::SnapshotJoin,
            server_id: server.to_string(),
            consumer_pubkey: PublicKey::from_private_key(sk)
                .as_bytes()
                .try_into()
                .unwrap(),
            chain_height: 14_799,
            subject_hash: snapshot_subject_hash("ABCD"),
            units: 12_345,
            timestamp: ts,
            nonce: [9u8; 32],
        }
    }

    fn identity() -> ChainIdentity {
        ChainIdentity::new("wrfcoin-test", "testnet")
    }

    #[test]
    fn a_consumer_signed_receipt_verifies_and_dedups() {
        let sk = consumer(1);
        let signed = SignedNodeServiceReceipt::sign(
            receipt(&sk, "seed-a", 1_000),
            &sk,
            &identity(),
            SigningProtocolVersion::V2,
        );
        let key = signed
            .verify(
                &identity(),
                SigningProtocolVersion::V2,
                "seed-a",
                1_010,
                300,
            )
            .unwrap();
        assert_eq!(key.as_bytes(), signed.receipt.consumer_pubkey);
        // Same consumer, server and subject with a different nonce/timestamp: same dedup key.
        let again = SignedNodeServiceReceipt::sign(
            NodeServiceReceipt {
                nonce: [1u8; 32],
                timestamp: 1_050,
                ..receipt(&sk, "seed-a", 1_000)
            },
            &sk,
            &identity(),
            SigningProtocolVersion::V2,
        );
        assert_eq!(again.dedup_key(), signed.dedup_key());
        // A different subject is a different receipt.
        let other = SignedNodeServiceReceipt::sign(
            NodeServiceReceipt {
                subject_hash: snapshot_subject_hash("EF01"),
                ..receipt(&sk, "seed-a", 1_000)
            },
            &sk,
            &identity(),
            SigningProtocolVersion::V2,
        );
        assert_ne!(other.dedup_key(), signed.dedup_key());
        assert_eq!(
            snapshot_subject_hash("abcd"),
            snapshot_subject_hash(" ABCD "),
            "root case/whitespace-insensitive"
        );
    }

    #[test]
    fn forged_stale_and_misaddressed_receipts_are_refused() {
        let sk = consumer(2);
        let mut signed = SignedNodeServiceReceipt::sign(
            receipt(&sk, "seed-a", 1_000),
            &sk,
            &identity(),
            SigningProtocolVersion::V2,
        );
        // Wrong server.
        assert!(matches!(
            // secret-leak-ok: test; seed-a/seed-b are public server IDs and SigningProtocolVersion is a public enum name.
            signed.verify(
                &identity(),
                SigningProtocolVersion::V2,
                "seed-b",
                1_000,
                300
            ),
            Err(ReceiptError::WrongServer { .. })
        ));
        // Stale (either direction).
        assert!(matches!(
            // secret-leak-ok: test; seed-a/seed-b are public server IDs and SigningProtocolVersion is a public enum name.
            signed.verify(
                &identity(),
                SigningProtocolVersion::V2,
                "seed-a",
                5_000,
                300
            ),
            Err(ReceiptError::Stale { .. })
        ));
        assert!(matches!(
            // secret-leak-ok: test; seed-a/seed-b are public server IDs and SigningProtocolVersion is a public enum name.
            signed.verify(&identity(), SigningProtocolVersion::V2, "seed-a", 100, 300),
            Err(ReceiptError::Stale { .. })
        ));
        // A receipt the server minted itself with a different key: the named consumer key did not sign it.
        let server_sk = consumer(3);
        let forged = SignedNodeServiceReceipt::sign(
            receipt(&sk, "seed-a", 1_000),
            &server_sk,
            &identity(),
            SigningProtocolVersion::V2,
        );
        assert_eq!(
            // secret-leak-ok: test; seed-a/seed-b are public server IDs and SigningProtocolVersion is a public enum name.
            forged.verify(
                &identity(),
                SigningProtocolVersion::V2,
                "seed-a",
                1_000,
                300
            ),
            Err(ReceiptError::BadSignature)
        );
        // Tampered units after signing.
        signed.receipt.units += 1;
        assert_eq!(
            // secret-leak-ok: test; seed-a/seed-b are public server IDs and SigningProtocolVersion is a public enum name.
            signed.verify(
                &identity(),
                SigningProtocolVersion::V2,
                "seed-a",
                1_000,
                300
            ),
            Err(ReceiptError::BadSignature)
        );
        // Chain-bound: another identity does not verify.
        let signed = SignedNodeServiceReceipt::sign(
            receipt(&sk, "seed-a", 1_000),
            &sk,
            &identity(),
            SigningProtocolVersion::V2,
        );
        assert_eq!(
            // secret-leak-ok: test; seed-a/seed-b are public server IDs and SigningProtocolVersion is a public enum name.
            signed.verify(
                &ChainIdentity::new("wrfcoin-main", "mainnet"),
                SigningProtocolVersion::V2,
                "seed-a",
                1_000,
                300
            ),
            Err(ReceiptError::BadSignature)
        );
    }

    #[test]
    fn canonical_body_binds_every_field() {
        let sk = consumer(4);
        let base = receipt(&sk, "seed-a", 1_000);
        let variants = vec![
            NodeServiceReceipt {
                service: NodeService::SyncRange,
                ..base.clone()
            },
            NodeServiceReceipt {
                server_id: "seed-b".into(),
                ..base.clone()
            },
            NodeServiceReceipt {
                chain_height: 1,
                ..base.clone()
            },
            NodeServiceReceipt {
                subject_hash: [0u8; 32],
                ..base.clone()
            },
            NodeServiceReceipt {
                units: 0,
                ..base.clone()
            },
            NodeServiceReceipt {
                timestamp: 1,
                ..base.clone()
            },
            NodeServiceReceipt {
                nonce: [0u8; 32],
                ..base.clone()
            },
        ];
        for v in variants {
            assert_ne!(v.canonical_body(), base.canonical_body());
        }
    }
}
