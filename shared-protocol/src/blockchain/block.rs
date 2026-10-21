use crate::blockchain::transaction::Transaction;
use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion, protocol_version_at};
use crate::crypto::domain_signing::DomainSeparated;
use crate::crypto::hash::{Hash, hash_bytes};
use chrono::{DateTime, Utc};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Metadata key under which the producer commits the post-state root (#2219).
///
/// Written by `create_next_block` as the LAST metadata entry before the hash
/// is sealed; verified against the applied root by
/// `State::apply_block_finalization_checked` on every applier path. Riding the
/// metadata map means the commitment folds into the sorted-metadata region of
/// both block encodings (`calculate_hash_for` and the `DomainSeparated`
/// preimage) with no wire-format change, and survives the planned #2290
/// collapse of the two encodings unmodified.
pub const STATE_ROOT_METADATA_KEY: &str = "state_root";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub version: u32,
    pub prev_block_hash: Hash,
    pub transactions: Vec<Transaction>,
    pub difficulty: u32,
    pub height: u64,
    pub nonce: u64,
    pub timestamp: DateTime<Utc>,
    pub metadata: FxHashMap<String, String>,
    /// #2290/#2226: the block producer's ed25519 public key. Bound into the
    /// hash/preimage via `canonical_body`, so tampering moves the block's
    /// identity. All-zero when unsigned. `[u8; 32]` matches
    /// `block_producer_auth::ProducerAttestation::producer_pubkey`.
    #[serde(default)]
    pub producer_pubkey: [u8; 32],
    /// #2290/#2226: the producer's ed25519 signature over the block-producer
    /// preimage. **Excluded** from `canonical_body` (it is the output). Carried
    /// as `Vec<u8>` (serde has no fixed `[u8; 64]` impl); holds 64 bytes when
    /// signed, empty when unsigned. Converted to
    /// `ProducerAttestation::producer_sig: [u8; 64]` at the verify boundary.
    #[serde(default)]
    pub producer_sig: Vec<u8>,
    pub hash: Option<Hash>,
}

impl Block {
    pub fn new(
        version: u32,
        prev_block_hash: Hash,
        transactions: Vec<Transaction>,
        difficulty: u32,
        height: u64,
    ) -> Self {
        Self {
            version,
            prev_block_hash,
            transactions,
            difficulty,
            height,
            nonce: 0,
            timestamp: Utc::now(),
            metadata: FxHashMap::default(),
            producer_pubkey: [0u8; 32],
            producer_sig: Vec::new(),
            hash: None,
        }
    }

    /// Block hash under the protocol version in force **at this block's own
    /// height**.
    ///
    /// Always `hash_bytes(signing_preimage())` — see [`Block::calculate_hash_for`].
    /// There is one encoding; the match below chooses only whether the chain
    /// identity needs *resolving*, not how the block is serialized.
    ///
    /// #1396: the version is resolved from the height, not from a process-wide
    /// install. Previously every block hashed under whatever version the process
    /// held, so a node replaying history evaluated *old* blocks under the
    /// *current* version — invisible today only because V1 → V2 happens at a
    /// fresh genesis, where no history exists at the other version, but it would
    /// break WAL replay and reorg re-apply at the first activation on a live
    /// chain.
    ///
    /// This is why the schedule is genesis-pinned rather than state-derived:
    /// `protocol_version_at` is a pure function of height, so this stays
    /// `&self` and none of the ~138 `calculate_hash()` call sites need to change.
    /// A state-derived version would have to be threaded into every one of them.
    ///
    /// With no schedule installed this falls back to
    /// [`SigningProtocolVersion::active`] at every height, so the resolved value
    /// is unchanged from before this commit.
    pub fn calculate_hash(&self) -> Hash {
        let version = protocol_version_at(self.height);
        match version {
            // V1's frame is `tag || body`, so the identity cannot affect the
            // result. Do not resolve one: `ChainIdentity::active()` installs a
            // process-global `OnceLock`, and resolving it here would freeze the
            // identity for the whole process the first time any block is hashed —
            // observable to anything that configures `WRFCOIN_CHAIN_ID` /
            // `WRFCOIN_NETWORK_ID` afterwards.
            SigningProtocolVersion::V1 => self.calculate_hash_for(version, None),
            SigningProtocolVersion::V2 => {
                self.calculate_hash_for(version, Some(ChainIdentity::active()))
            },
        }
    }

    /// Block hash under an explicit protocol version and chain identity.
    ///
    /// # #2226 §3.2: ONE block encoding
    ///
    /// The block hash **is** the domain-separated preimage:
    /// `hash_bytes(signing_preimage())`, on **every** path and at every protocol
    /// version. There is deliberately no second serializer.
    ///
    /// The pre-#2123 raw little-endian serializer used to live here as the V1
    /// branch. It is **removed**, because two encodings of the same block are
    /// exactly the drift §3.2 exists to eliminate: "one block encoding,
    /// length-framed and domain-tagged, instead of two that can drift" — a field
    /// added to one and not the other is a silent divergence between what a block
    /// *is* and what its signature *covers*. Keeping it as dead-but-callable code
    /// preserved that hazard for no benefit: the #2034 cut starts a **fresh
    /// genesis at V2**, so nothing post-cut ever hashes under the legacy bytes.
    ///
    /// Consequences that follow for free from having one encoding:
    /// - `producer_pubkey` is in the hash **and** the signature by construction
    ///   (`canonical_body` folds it in) — the attribution swap is closed.
    /// - the frame's `chain_id` / `network_id` **subsume #2066's** 4-byte network
    ///   magic, so testnet and mainnet blocks are distinct with no separate
    ///   `NETWORK_MAGIC` constant to define and keep in step.
    ///
    /// `identity` is only observable at V2 (the V1 frame is `tag || body`);
    /// passing `None` substitutes the empty identity, which is correct only for
    /// tests deliberately probing that case — the live [`Block::calculate_hash`]
    /// always passes `Some`.
    pub fn calculate_hash_for(
        &self,
        protocol_version: SigningProtocolVersion,
        identity: Option<&ChainIdentity>,
    ) -> Hash {
        let fallback;
        let identity = match identity {
            Some(id) => id,
            None => {
                fallback = ChainIdentity::new("", "");
                &fallback
            },
        };
        hash_bytes(&self.signing_preimage_for(identity, protocol_version))
    }

    pub fn set_hash(&mut self, hash: Hash) {
        self.hash = Some(hash);
    }

    /// #2219: whether this block's metadata carries a well-formed post-state-root
    /// commitment (64 hex chars under [`STATE_ROOT_METADATA_KEY`]).
    ///
    /// Shape only — whether the committed root matches the applied state is
    /// `State::apply_block_finalization_checked`'s job, because only the
    /// applier that reached a root can compare against one.
    pub fn has_well_formed_state_root_commitment(&self) -> bool {
        self.metadata
            .get(STATE_ROOT_METADATA_KEY)
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    }
}

#[cfg(test)]
mod tests {
    use super::Block;
    use crate::blockchain::transaction::Transaction;
    use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
    use crate::crypto::hash::Hash;
    use chrono::Utc;

    fn testnet() -> ChainIdentity {
        ChainIdentity::new("wrfcoin-testnet", "testnet")
    }

    fn mainnet() -> ChainIdentity {
        ChainIdentity::new("wrfcoin-main", "mainnet")
    }

    /// Two blocks with byte-identical contents.
    fn identical_pair() -> (Block, Block) {
        let timestamp = Utc::now();
        let mut a = Block::new(3, Hash::default(), Vec::new(), 1, 7);
        let mut b = Block::new(3, Hash::default(), Vec::new(), 1, 7);
        a.timestamp = timestamp;
        b.timestamp = timestamp;
        (a, b)
    }

    // ---- #2226: the V2 hash-collapse and producer-key binding ----

    /// #2226 RED test: the V2 block hash IS `hash_bytes(signing_preimage())`.
    /// Reverting the collapse in `calculate_hash_for` (back to the ad-hoc
    /// network-magic material) breaks this equality and this test goes RED.
    #[test]
    fn v2_block_hash_equals_domain_separated_preimage_2226() {
        use crate::crypto::domain_signing::DomainSeparated;
        use crate::crypto::hash::hash_bytes;
        let (mut a, _) = identical_pair();
        a.producer_pubkey = [9u8; 32];
        let id = testnet();
        assert_eq!(
            a.calculate_hash_for(SigningProtocolVersion::V2, Some(&id)),
            hash_bytes(&a.signing_preimage_for(&id, SigningProtocolVersion::V2)),
            "the V2 block hash must be hash_bytes(signing_preimage) (#2226 collapse)"
        );
    }

    /// #2226 §3.2 — **the** anti-drift guard: the block hash IS
    /// `hash_bytes(signing_preimage)` at *every* protocol version, so exactly one
    /// block encoding exists.
    ///
    /// This is the test that goes RED if a second serializer is reintroduced
    /// (e.g. restoring the pre-#2123 raw little-endian V1 branch). Its siblings
    /// provably cannot, which is why this one exists:
    ///
    /// - `v1_block_hash_ignores_chain_identity` stays GREEN under a restore — the
    ///   legacy raw-LE encoding also ignores the chain identity, so
    ///   identity-independence is not evidence of the collapse.
    /// - `v2_block_hash_equals_domain_separated_preimage_2226` only covers V2,
    ///   and V1 is exactly the branch a reintroduction would put back.
    ///
    /// Only asserting the equality itself, on every version, is load-bearing.
    #[test]
    fn block_hash_is_the_domain_separated_preimage_at_every_version_2226() {
        use crate::crypto::domain_signing::DomainSeparated;
        use crate::crypto::hash::hash_bytes;

        // Exhaustiveness: adding a protocol version stops compiling here until it
        // is also added to `VERSIONS`, so a new version cannot skip this guard.
        fn _every_version_is_covered(v: SigningProtocolVersion) {
            match v {
                SigningProtocolVersion::V1 | SigningProtocolVersion::V2 => {},
            }
        }
        const VERSIONS: &[SigningProtocolVersion] =
            &[SigningProtocolVersion::V1, SigningProtocolVersion::V2];

        // Content-bearing block: producer key, nonce and metadata all differ from
        // their defaults, so two encodings could not coincide by emptiness.
        let (mut block, _) = identical_pair();
        block.producer_pubkey = [7u8; 32];
        block.nonce = 99;
        block
            .metadata
            .insert("publishing_score".to_string(), "12345".to_string());

        let id = testnet();
        let empty = ChainIdentity::new("", "");
        for &version in VERSIONS {
            assert_eq!(
                block.calculate_hash_for(version, Some(&id)),
                hash_bytes(&block.signing_preimage_for(&id, version)),
                "{version:?}: block hash must be hash_bytes(signing_preimage) — one \
                 block encoding (#2226 §3.2). A second serializer has been reintroduced."
            );
            // The no-identity path substitutes the empty identity; it must not be
            // a different encoding.
            assert_eq!(
                block.calculate_hash_for(version, None),
                hash_bytes(&block.signing_preimage_for(&empty, version)),
                "{version:?}: the None-identity path must also be \
                 hash_bytes(signing_preimage)"
            );
        }
    }

    /// #2226/#2290 RED test: `producer_pubkey` is bound into the V2 block hash,
    /// while `producer_sig` is NOT (it is the output). Two blocks identical
    /// except for `producer_pubkey` must hash differently — removing the
    /// `.bytes_field(&self.producer_pubkey)` from `canonical_body` makes them
    /// equal (RED). A block differing only in `producer_sig` must hash the same.
    #[test]
    fn v2_block_hash_binds_producer_pubkey_but_not_signature_2226() {
        let (mut a, mut b) = identical_pair();
        a.producer_pubkey = [1u8; 32];
        b.producer_pubkey = [2u8; 32];
        let id = testnet();
        assert_ne!(
            a.calculate_hash_for(SigningProtocolVersion::V2, Some(&id)),
            b.calculate_hash_for(SigningProtocolVersion::V2, Some(&id)),
            "producer_pubkey must be bound into the V2 block hash (#2290)"
        );

        let mut c = a.clone();
        c.producer_sig = vec![7u8; 64];
        assert_eq!(
            a.calculate_hash_for(SigningProtocolVersion::V2, Some(&id)),
            c.calculate_hash_for(SigningProtocolVersion::V2, Some(&id)),
            "producer_sig is the output and must NOT affect the block hash"
        );
    }

    // ---- #2066 / #2123: network magic in the block-hash preimage ----

    #[test]
    fn mainnet_and_testnet_block_hashes_differ_for_identical_contents() {
        // The #2066 property. Without network magic the two networks' blocks
        // are cryptographically indistinguishable.
        let (a, b) = identical_pair();
        assert_eq!(
            a.calculate_hash_for(SigningProtocolVersion::V1, Some(&testnet())),
            b.calculate_hash_for(SigningProtocolVersion::V1, Some(&mainnet())),
            "V1 has no network magic — this is the defect #2066 describes"
        );
        assert_ne!(
            a.calculate_hash_for(SigningProtocolVersion::V2, Some(&testnet())),
            b.calculate_hash_for(SigningProtocolVersion::V2, Some(&mainnet())),
            "V2 must make identical blocks hash differently per network"
        );
    }

    #[test]
    fn v1_block_hash_ignores_chain_identity() {
        // Renamed from `v1_block_hash_is_unchanged_by_this_story` (#2226).
        //
        // Its original claim — "merging #2123 must not move a single existing
        // block hash" — is no longer true and can no longer be asserted: #2226
        // removed the second, raw little-endian block serializer so that
        // `calculate_hash` derives from `signing_preimage` on EVERY path (§3.2,
        // one block encoding). V1 hashes therefore did move. That is intended and
        // safe because the #2034 cut starts a fresh genesis at V2, so no chain
        // continues under the legacy bytes; the genesis pin is re-derived at
        // cutover (`GENESIS_CUT_PLAN.md` §5).
        //
        // What remains true, and is what this test now guards, is the frame
        // property: at V1 the preimage is `tag || body`, so the chain identity is
        // not observable in a V1 hash.
        let (a, b) = identical_pair();
        assert_eq!(
            a.calculate_hash_for(SigningProtocolVersion::V1, Some(&testnet())),
            b.calculate_hash_for(SigningProtocolVersion::V1, None)
        );
        assert_eq!(
            a.calculate_hash_for(SigningProtocolVersion::V1, None),
            b.calculate_hash()
        );
    }

    #[test]
    fn v2_block_hash_differs_from_v1_on_the_same_network() {
        let (a, b) = identical_pair();
        assert_ne!(
            a.calculate_hash_for(SigningProtocolVersion::V1, Some(&mainnet())),
            b.calculate_hash_for(SigningProtocolVersion::V2, Some(&mainnet())),
            "the version must be inside the hash, or a v1 and v2 block collide"
        );
    }

    #[test]
    fn v2_block_hash_is_deterministic_for_one_network() {
        let (a, b) = identical_pair();
        assert_eq!(
            a.calculate_hash_for(SigningProtocolVersion::V2, Some(&testnet())),
            b.calculate_hash_for(SigningProtocolVersion::V2, Some(&testnet()))
        );
    }

    #[test]
    fn calculate_hash_is_independent_of_metadata_insertion_order() {
        let timestamp = Utc::now();

        let mut first = Block::new(3, Hash::default(), Vec::new(), 1, 1);
        first.timestamp = timestamp;
        first
            .metadata
            .insert("publishing_score".to_string(), "0.75".to_string());
        first
            .metadata
            .insert("reward_total".to_string(), "42".to_string());
        first
            .metadata
            .insert("role_keeper_count".to_string(), "1".to_string());

        let mut second = Block::new(3, Hash::default(), Vec::new(), 1, 1);
        second.timestamp = timestamp;
        second
            .metadata
            .insert("role_keeper_count".to_string(), "1".to_string());
        second
            .metadata
            .insert("reward_total".to_string(), "42".to_string());
        second
            .metadata
            .insert("publishing_score".to_string(), "0.75".to_string());

        assert_eq!(first.calculate_hash(), second.calculate_hash());
    }

    // ---- #2219: post-state-root commitment ----

    #[test]
    fn state_root_commitment_shape_check() {
        let mut block = Block::new(3, Hash::default(), Vec::new(), 1, 1);
        assert!(
            !block.has_well_formed_state_root_commitment(),
            "no commitment key"
        );

        block
            .metadata
            .insert(super::STATE_ROOT_METADATA_KEY.to_string(), "ab".repeat(31));
        assert!(
            !block.has_well_formed_state_root_commitment(),
            "62 hex chars is not a 32-byte root"
        );

        block.metadata.insert(
            super::STATE_ROOT_METADATA_KEY.to_string(),
            format!("{}zz", "ab".repeat(31)),
        );
        assert!(
            !block.has_well_formed_state_root_commitment(),
            "non-hex payload"
        );

        block
            .metadata
            .insert(super::STATE_ROOT_METADATA_KEY.to_string(), "ab".repeat(32));
        assert!(block.has_well_formed_state_root_commitment());
    }

    /// Tampering the commitment moves the block's identity: the sorted-metadata
    /// fold already covers the key, so a relayed block whose `state_root` entry
    /// was altered no longer matches its stored hash. This is the property that
    /// lets the P2P apply path catch tampering with its existing hash
    /// recomputation, before the applier-side equality check even runs.
    #[test]
    fn tampering_state_root_commitment_changes_block_hash() {
        let timestamp = Utc::now();
        let mut block = Block::new(3, Hash::default(), Vec::new(), 1, 1);
        block.timestamp = timestamp;
        block
            .metadata
            .insert(super::STATE_ROOT_METADATA_KEY.to_string(), "ab".repeat(32));
        let sealed = block.calculate_hash_for(SigningProtocolVersion::V2, Some(&testnet()));

        block
            .metadata
            .insert(super::STATE_ROOT_METADATA_KEY.to_string(), "ba".repeat(32));
        assert_ne!(
            sealed,
            block.calculate_hash_for(SigningProtocolVersion::V2, Some(&testnet())),
        );
    }

    #[test]
    fn calculate_hash_survives_bincode_round_trip() {
        let timestamp = Utc::now();
        let mut block = Block::new(3, Hash::default(), Vec::new(), 1, 1);
        block.timestamp = timestamp;
        block.metadata.insert(
            "useful_work_liveness_fallback".to_string(),
            "empty_liveness".to_string(),
        );
        block.transactions.push(Transaction::new_coinbase(
            "0x0000000000000000000000000000000000000001",
            50,
            1,
        ));

        let expected = block.calculate_hash();
        let bytes = bincode::serialize(&block).expect("block should serialize");
        let round_tripped: Block = bincode::deserialize(&bytes).expect("block should deserialize");

        assert_eq!(round_tripped.calculate_hash(), expected);
    }

    #[test]
    fn calculate_hash_deterministic_fixture() {
        use chrono::TimeZone;

        let timestamp = chrono::Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
        let mut block = Block::new(3, Hash::default(), Vec::new(), 1, 42);
        block.timestamp = timestamp;
        block.nonce = 7;
        block
            .metadata
            .insert("publishing_score".to_string(), "500000".to_string());

        let hash_a = block.calculate_hash();
        let hash_b = block.calculate_hash();
        assert_eq!(
            hash_a, hash_b,
            "same block must always produce the same hash"
        );

        // Changing any field must produce a different hash.
        let mut block_different_nonce = block.clone();
        block_different_nonce.nonce = 8;
        assert_ne!(
            block.calculate_hash(),
            block_different_nonce.calculate_hash(),
            "different nonce must produce different hash"
        );
    }
}
