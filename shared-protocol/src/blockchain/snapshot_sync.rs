use crate::crypto::domain_signing::{
    DomainSigningAction, DomainSigningContext, canonical_signing_message, payload_digest_hex,
    signing_chain_id, signing_network_id,
};
use crate::crypto::hash::hash_bytes;
use crate::crypto::signature::{PrivateKey, PublicKey, Signature};
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};

const EMPTY_SNAPSHOT_DOMAIN: &str = "wrfcoin_snapshot_empty_v1";
const LEAF_DOMAIN: &str = "wrfcoin_snapshot_leaf_v1";
const NODE_DOMAIN: &str = "wrfcoin_snapshot_node_v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotEntry {
    pub key_hex: String,
    pub value_hash: String,
    pub leaf_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateSnapshot {
    pub height: u64,
    pub state_root_hex: String,
    pub snapshot_root_hex: String,
    pub entries: Vec<SnapshotEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotProofNode {
    pub hash: String,
    pub sibling_on_left: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotInclusionProof {
    pub key_hex: String,
    pub value_hash: String,
    pub leaf_hash: String,
    pub index: usize,
    pub siblings: Vec<SnapshotProofNode>,
    pub snapshot_root_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotSyncPayload {
    pub height: u64,
    pub state_root_hex: String,
    pub snapshot_root_hex: String,
    pub entry_count: u64,
    /// Hash of block `height`, the chain tip the state belongs to (#3288 3b).
    ///
    /// Signed so an ingesting node can install the tip and accept block
    /// `height + 1` (PoP rejects any block whose `prev_block_hash` is not the
    /// local tip), and so the snapshot is bound to a block anyone can compare
    /// against peers' headers. Ingest checks the carried block `height` hashes
    /// to this value and commits to `state_root_hex`.
    pub tip_hash_hex: String,
    /// The source's BFT-finalized height when the snapshot was taken (#3288
    /// 3b). Informational and bounded (`<= height`); the ingesting node
    /// finalizes at `height` itself, because it holds no state below it.
    pub finalized_height: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedSnapshotSyncPayload {
    pub signer_id: String,
    pub timestamp: i64,
    pub nonce: String,
    pub payload: SnapshotSyncPayload,
    pub signature: String,
}

impl SignedSnapshotSyncPayload {
    pub fn canonical_signing_message(&self) -> String {
        self.canonical_signing_message_for(&crate::crypto::chain_binding::ChainIdentity::new(signing_chain_id(), signing_network_id()))
    }

    /// Consensus callers supply the recorded identity, never process settings.
    pub fn canonical_signing_message_for(&self, identity: &crate::crypto::chain_binding::ChainIdentity) -> String {
        let cid = &identity.chain_id;
        let nid = &identity.network_id;
        let context = DomainSigningContext {
            action: DomainSigningAction::SnapshotSync,
            signer_id: &self.signer_id,
            timestamp: self.timestamp,
            nonce: Some(&self.nonce),
            chain_id: &cid,
            network_id: &nid,
        };
        let payload_digest = payload_digest_hex(&self.payload);
        canonical_signing_message(&context, &payload_digest)
    }

    pub fn has_valid_signature(&self) -> bool {
        let public_key = match PublicKey::from_hex(&self.signer_id) {
            Ok(pk) => pk,
            Err(_) => return false,
        };
        let signature = match Signature::from_hex(&self.signature) {
            Ok(sig) => sig,
            Err(_) => return false,
        };
        let message = self.canonical_signing_message();
        public_key.verify(message.as_bytes(), &signature)
    }

    pub fn has_valid_signature_for(&self, identity: &crate::crypto::chain_binding::ChainIdentity) -> bool {
        let (Ok(key), Ok(sig)) = (PublicKey::from_hex(&self.signer_id), Signature::from_hex(&self.signature)) else { return false; };
        key.verify(self.canonical_signing_message_for(identity).as_bytes(), &sig)
    }

    pub fn has_valid_signature_from_trusted_signers(
        &self,
        trusted_signers: &FxHashSet<String>,
    ) -> bool {
        trusted_signers.contains(&self.signer_id) && self.has_valid_signature()
    }
}

pub fn build_state_snapshot(
    height: u64,
    state_root_hex: impl Into<String>,
    entries: &[(Vec<u8>, Vec<u8>)],
) -> StateSnapshot {
    let mut canonical = entries
        .iter()
        .map(|(key, value)| {
            let value_hash = hash_bytes(value).to_string();
            let leaf_hash = snapshot_leaf_hash(key, &value_hash);
            SnapshotEntry {
                key_hex: hex::encode(key),
                value_hash,
                leaf_hash,
            }
        })
        .collect::<Vec<_>>();
    canonical.sort_by(|a, b| a.key_hex.cmp(&b.key_hex));

    let leaf_hashes = canonical
        .iter()
        .map(|entry| entry.leaf_hash.clone())
        .collect::<Vec<_>>();

    StateSnapshot {
        height,
        state_root_hex: state_root_hex.into(),
        snapshot_root_hex: merkle_root(&leaf_hashes),
        entries: canonical,
    }
}

/// The payload to sign for `snapshot`, taken at chain tip `tip_hash_hex`
/// (the hash of block `snapshot.height`) with the source's
/// `finalized_height`.
pub fn build_snapshot_sync_payload(
    snapshot: &StateSnapshot,
    tip_hash_hex: impl Into<String>,
    finalized_height: u64,
) -> SnapshotSyncPayload {
    SnapshotSyncPayload {
        height: snapshot.height,
        state_root_hex: snapshot.state_root_hex.clone(),
        snapshot_root_hex: snapshot.snapshot_root_hex.clone(),
        entry_count: snapshot.entries.len() as u64,
        tip_hash_hex: tip_hash_hex.into(),
        finalized_height,
    }
}

pub fn sign_snapshot_sync_payload(
    payload: SnapshotSyncPayload,
    private_key: &PrivateKey,
    timestamp: i64,
    nonce: impl Into<String>,
) -> SignedSnapshotSyncPayload {
    let signer_id = PublicKey::from_private_key(private_key).to_string();
    let nonce = nonce.into();
    let unsigned = SignedSnapshotSyncPayload {
        signer_id,
        timestamp,
        nonce,
        payload,
        signature: String::new(),
    };
    let message = unsigned.canonical_signing_message();
    SignedSnapshotSyncPayload {
        signature: private_key.sign(message.as_bytes()).to_string(),
        ..unsigned
    }
}

/// Same as [`sign_snapshot_sync_payload`] but routed through a
/// [`BlockSigner`](crate::crypto::signer::BlockSigner), so the caller does
/// not need to hold raw private-key material. This is the path used by the
/// `launch_api` binary post-#1394; the env-key variant above remains for
/// tests that construct a `PrivateKey` inline.
pub fn sign_snapshot_sync_payload_with_signer(
    payload: SnapshotSyncPayload,
    signer: &dyn crate::crypto::signer::BlockSigner,
    timestamp: i64,
    nonce: impl Into<String>,
) -> anyhow::Result<SignedSnapshotSyncPayload> {
    let signer_id = signer.public_key().to_string();
    let nonce = nonce.into();
    let unsigned = SignedSnapshotSyncPayload {
        signer_id,
        timestamp,
        nonce,
        payload,
        signature: String::new(),
    };
    let message = unsigned.canonical_signing_message();
    // #2130: sign *at the height this snapshot attests to*, not heightlessly.
    // A snapshot is a claim about state at `payload.height`, so that is the
    // height at which the signing key must be authoritative. Under an active
    // rotation plan a key past its boundary is refused here rather than
    // producing a signature no verifier will accept.
    let signature = signer.sign_at_height(message.as_bytes(), unsigned.payload.height)?;
    Ok(SignedSnapshotSyncPayload {
        signature: signature.to_string(),
        ..unsigned
    })
}

pub fn build_inclusion_proof(
    snapshot: &StateSnapshot,
    key: &[u8],
) -> Option<SnapshotInclusionProof> {
    let key_hex = hex::encode(key);
    let index = snapshot
        .entries
        .iter()
        .position(|entry| entry.key_hex == key_hex)?;
    let entry = snapshot.entries.get(index)?;
    let mut siblings = Vec::new();

    let mut current = snapshot
        .entries
        .iter()
        .map(|entry| entry.leaf_hash.clone())
        .collect::<Vec<_>>();
    let mut cursor = index;

    while current.len() > 1 {
        let sibling_index = if cursor % 2 == 0 {
            (cursor + 1).min(current.len() - 1)
        } else {
            cursor - 1
        };
        siblings.push(SnapshotProofNode {
            hash: current[sibling_index].clone(),
            sibling_on_left: sibling_index < cursor,
        });

        let mut next = Vec::with_capacity(current.len().div_ceil(2));
        for pair in current.chunks(2) {
            let left = &pair[0];
            let right = pair.get(1).unwrap_or(&pair[0]);
            next.push(snapshot_node_hash(left, right));
        }

        current = next;
        cursor /= 2;
    }

    Some(SnapshotInclusionProof {
        key_hex,
        value_hash: entry.value_hash.clone(),
        leaf_hash: entry.leaf_hash.clone(),
        index,
        siblings,
        snapshot_root_hex: snapshot.snapshot_root_hex.clone(),
    })
}

pub fn verify_inclusion_proof(proof: &SnapshotInclusionProof, key: &[u8], value: &[u8]) -> bool {
    if proof.key_hex != hex::encode(key) {
        return false;
    }

    let expected_value_hash = hash_bytes(value).to_string();
    if proof.value_hash != expected_value_hash {
        return false;
    }

    let expected_leaf = snapshot_leaf_hash(key, &proof.value_hash);
    if proof.leaf_hash != expected_leaf {
        return false;
    }

    let mut rolling = expected_leaf;
    for sibling in &proof.siblings {
        rolling = if sibling.sibling_on_left {
            snapshot_node_hash(&sibling.hash, &rolling)
        } else {
            snapshot_node_hash(&rolling, &sibling.hash)
        };
    }

    rolling == proof.snapshot_root_hex
}

fn snapshot_leaf_hash(key: &[u8], value_hash: &str) -> String {
    let mut material = Vec::new();
    material.extend_from_slice(LEAF_DOMAIN.as_bytes());
    material.extend_from_slice(&(key.len() as u64).to_le_bytes());
    material.extend_from_slice(key);
    material.extend_from_slice(value_hash.as_bytes());
    hash_bytes(&material).to_string()
}

fn snapshot_node_hash(left: &str, right: &str) -> String {
    let encoded = format!("{NODE_DOMAIN}|{left}|{right}");
    hash_bytes(encoded.as_bytes()).to_string()
}

fn merkle_root(leaves: &[String]) -> String {
    if leaves.is_empty() {
        return hash_bytes(EMPTY_SNAPSHOT_DOMAIN.as_bytes()).to_string();
    }
    let mut layer = leaves.to_vec();
    while layer.len() > 1 {
        let mut next = Vec::with_capacity(layer.len().div_ceil(2));
        for pair in layer.chunks(2) {
            let left = &pair[0];
            let right = pair.get(1).unwrap_or(&pair[0]);
            next.push(snapshot_node_hash(left, right));
        }
        layer = next;
    }
    layer[0].clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    fn sample_entries() -> Vec<(Vec<u8>, Vec<u8>)> {
        vec![
            (b"station/a/temp".to_vec(), b"21.5".to_vec()),
            (b"station/b/humidity".to_vec(), b"0.41".to_vec()),
            (b"station/a/wind".to_vec(), b"3.2".to_vec()),
        ]
    }

    #[test]
    fn snapshot_root_is_deterministic_across_entry_order() {
        let entries_a = sample_entries();
        let mut entries_b = sample_entries();
        entries_b.reverse();

        let snap_a = build_state_snapshot(42, "deadbeef", &entries_a);
        let snap_b = build_state_snapshot(42, "deadbeef", &entries_b);
        assert_eq!(snap_a.snapshot_root_hex, snap_b.snapshot_root_hex);
    }

    #[test]
    fn inclusion_proof_verifies_and_tamper_fails() {
        let entries = sample_entries();
        let snapshot = build_state_snapshot(42, "abc123", &entries);

        let key = b"station/b/humidity";
        let value = b"0.41";

        let proof = build_inclusion_proof(&snapshot, key).expect("proof must exist");
        assert!(verify_inclusion_proof(&proof, key, value));

        assert!(!verify_inclusion_proof(&proof, key, b"0.49"));
    }

    #[test]
    fn inclusion_proof_missing_key_returns_none() {
        let snapshot = build_state_snapshot(42, "abc123", &sample_entries());
        assert!(build_inclusion_proof(&snapshot, b"station/c/pressure").is_none());
    }

    #[test]
    fn inclusion_proof_rejects_wrong_key_and_tampered_merkle_path() {
        let entries = sample_entries();
        let snapshot = build_state_snapshot(42, "abc123", &entries);
        let key = b"station/b/humidity";
        let value = b"0.41";

        let proof = build_inclusion_proof(&snapshot, key).expect("proof must exist");
        assert!(!verify_inclusion_proof(&proof, b"station/a/temp", value));

        let mut tampered_root = proof.clone();
        tampered_root.snapshot_root_hex = "deadbeef".to_string();
        assert!(!verify_inclusion_proof(&tampered_root, key, value));

        let mut tampered_sibling = proof;
        assert!(
            !tampered_sibling.siblings.is_empty(),
            "sample proof should have a sibling path"
        );
        tampered_sibling.siblings[0].hash = "00".repeat(32);
        assert!(!verify_inclusion_proof(&tampered_sibling, key, value));
    }

    #[test]
    fn snapshot_sync_signature_verifies() {
        let mut rng = StdRng::seed_from_u64(77);
        let sk = PrivateKey::generate(&mut rng);
        let snapshot = build_state_snapshot(77, "state-root", &sample_entries());
        let payload = build_snapshot_sync_payload(&snapshot, "ab".repeat(32), 0);

        let signed = sign_snapshot_sync_payload(payload, &sk, 1_700_000_500, "nonce-77");
        assert!(signed.has_valid_signature());

        let mut tampered = signed.clone();
        tampered.payload.entry_count += 1;
        assert!(!tampered.has_valid_signature());
    }

    #[test]
    fn snapshot_sync_signature_requires_trusted_signer_allow_list() {
        let mut rng = StdRng::seed_from_u64(88);
        let trusted_sk = PrivateKey::generate(&mut rng);
        let trusted_pk = PublicKey::from_private_key(&trusted_sk).to_string();
        let untrusted_sk = PrivateKey::generate(&mut rng);
        let snapshot = build_state_snapshot(88, "state-root", &sample_entries());
        let payload = build_snapshot_sync_payload(&snapshot, "ab".repeat(32), 0);

        let trusted_signed =
            sign_snapshot_sync_payload(payload.clone(), &trusted_sk, 1_700_000_800, "nonce-88");
        let untrusted_signed =
            sign_snapshot_sync_payload(payload, &untrusted_sk, 1_700_000_801, "nonce-89");

        let mut trusted_signers = FxHashSet::default();
        trusted_signers.insert(trusted_pk);

        assert!(trusted_signed.has_valid_signature_from_trusted_signers(&trusted_signers));
        assert!(!untrusted_signed.has_valid_signature_from_trusted_signers(&trusted_signers));
        assert!(!trusted_signed.has_valid_signature_from_trusted_signers(&FxHashSet::default()));
    }

    /// The `_with_signer` variant (used by `launch_api` post-#1394) must
    /// produce the same authenticated payload as the raw-key variant for
    /// the same inputs. Confirms the `BlockSigner`-routed path is
    /// byte-identical at the wire level.
    #[test]
    fn snapshot_sync_signer_path_matches_private_key_path() {
        use crate::crypto::signer::EnvKeySigner;
        let mut rng = StdRng::seed_from_u64(1394);
        let sk = PrivateKey::generate(&mut rng);
        let snapshot = build_state_snapshot(1394, "state-root", &sample_entries());
        let payload = build_snapshot_sync_payload(&snapshot, "ab".repeat(32), 0);
        let ts = 1_700_001_394_i64;
        let nonce = "nonce-1394";

        let via_key =
            sign_snapshot_sync_payload(payload.clone(), &sk, ts, nonce.to_string());
        let signer = EnvKeySigner::from_private_key(sk.clone(), "test");
        let via_signer = sign_snapshot_sync_payload_with_signer(
            payload,
            &signer,
            ts,
            nonce.to_string(),
        )
        .expect("signer path succeeds");

        assert_eq!(via_key.signer_id, via_signer.signer_id);
        assert_eq!(via_key.signature, via_signer.signature);
        assert!(via_signer.has_valid_signature());
    }

    #[test]
    fn verify_inclusion_rejects_truncated_siblings() {
        let entries = sample_entries();
        let snapshot = build_state_snapshot(42, "abc123", &entries);
        let key = b"station/b/humidity";
        let value = b"0.41";

        let mut proof = build_inclusion_proof(&snapshot, key).expect("proof must exist");
        assert!(verify_inclusion_proof(&proof, key, value));

        // Pop the last sibling from the path, breaking the merkle verification
        if !proof.siblings.is_empty() {
            proof.siblings.pop();
        }
        assert!(!verify_inclusion_proof(&proof, key, value));
    }

    #[test]
    fn verify_inclusion_rejects_flipped_sibling_direction() {
        let entries = sample_entries();
        let snapshot = build_state_snapshot(42, "abc123", &entries);
        let key = b"station/b/humidity";
        let value = b"0.41";

        let mut proof = build_inclusion_proof(&snapshot, key).expect("proof must exist");
        assert!(verify_inclusion_proof(&proof, key, value));
        assert!(!proof.siblings.is_empty(), "test requires proof with siblings");

        // Flip the sibling_on_left bit for the last sibling in the path
        // (which has the highest impact on the final root)
        let last_idx = proof.siblings.len() - 1;
        proof.siblings[last_idx].sibling_on_left = !proof.siblings[last_idx].sibling_on_left;
        assert!(!verify_inclusion_proof(&proof, key, value));
    }

    #[test]
    fn domain_constants_are_stable() {
        // Regression guard: ensure domain constants don't accidentally change.
        // These constants are critical for domain separation in signatures.
        assert_eq!(EMPTY_SNAPSHOT_DOMAIN, "wrfcoin_snapshot_empty_v1");
        assert_eq!(LEAF_DOMAIN, "wrfcoin_snapshot_leaf_v1");
        assert_eq!(NODE_DOMAIN, "wrfcoin_snapshot_node_v1");
    }

    // ── #1735 slice 4c: snapshot-load corruption and boundary variants ───────
    //
    // Gaps NOT covered by the existing suite:
    //  (a) Empty-entry snapshots use the `EMPTY_SNAPSHOT_DOMAIN` hash, not
    //      a zero hash or panic.
    //  (b) Single-entry snapshots produce a proof with no siblings; verify
    //      must still accept it.
    //  (c) `has_valid_signature` returns false (not panic) when `signer_id`
    //      or `signature` contains non-hex garbage — simulating a corrupted
    //      or adversarial payload.
    //  (d) `verify_inclusion_proof` rejects a proof with a tampered
    //      `leaf_hash` even when key, value, and sibling path are intact.
    //  (e) `sign_snapshot_sync_payload` binds signer_id to the public key
    //      so callers cannot forge a signed payload under a different signer_id.

    /// An empty-entry snapshot must use the `EMPTY_SNAPSHOT_DOMAIN` hash for
    /// its Merkle root, not a zero-byte hash or a panic (#1735).
    #[test]
    fn snapshot_empty_entries_uses_domain_hash() {
        let snap = build_state_snapshot(0, "genesis-root", &[]);
        let expected_root = crate::crypto::hash::hash_bytes(EMPTY_SNAPSHOT_DOMAIN.as_bytes())
            .to_string();
        assert_eq!(
            snap.snapshot_root_hex, expected_root,
            "empty-entry snapshot root must equal the domain-hash of \
             EMPTY_SNAPSHOT_DOMAIN (#1735)"
        );
        assert_eq!(snap.entries.len(), 0);
        assert_eq!(snap.height, 0);
        assert_eq!(snap.state_root_hex, "genesis-root");
    }

    /// A single-entry snapshot produces a proof with an empty sibling list
    /// (the root IS the leaf); `verify_inclusion_proof` must still accept it
    /// (#1735).
    #[test]
    fn snapshot_single_entry_proof_roundtrip() {
        let entries = vec![(b"solo-key".to_vec(), b"solo-value".to_vec())];
        let snap = build_state_snapshot(1, "solo-root", &entries);

        assert_eq!(snap.entries.len(), 1);
        // For a single leaf, the Merkle root equals the single leaf hash.
        assert_eq!(snap.snapshot_root_hex, snap.entries[0].leaf_hash);

        let proof = build_inclusion_proof(&snap, b"solo-key")
            .expect("proof must exist for the only entry");
        assert!(
            proof.siblings.is_empty(),
            "single-entry proof must have no sibling nodes"
        );
        assert!(
            verify_inclusion_proof(&proof, b"solo-key", b"solo-value"),
            "single-entry proof must verify (#1735)"
        );
    }

    /// `has_valid_signature` must return `false` — not panic — when
    /// `signer_id` cannot be decoded as a public key hex string (#1735).
    #[test]
    fn has_valid_signature_rejects_non_hex_signer_id() {
        let mut rng = StdRng::seed_from_u64(0xBAD1);
        let sk = PrivateKey::generate(&mut rng);
        let snap = build_state_snapshot(10, "root", &sample_entries());
        let payload = build_snapshot_sync_payload(&snap, "ab".repeat(32), 0);
        let mut signed = sign_snapshot_sync_payload(payload, &sk, 1_000_000, "nonce-bad-signer");

        // Corrupt the signer_id with clearly non-hex content.
        signed.signer_id = "NOT_VALID_HEX!!!".to_string();

        assert!(
            !signed.has_valid_signature(),
            "corrupted signer_id must cause has_valid_signature to return false, not panic (#1735)"
        );
    }

    /// `has_valid_signature` must return `false` — not panic — when the
    /// `signature` field is not a valid hex-encoded signature (#1735).
    #[test]
    fn has_valid_signature_rejects_non_hex_signature_field() {
        let mut rng = StdRng::seed_from_u64(0xBAD2);
        let sk = PrivateKey::generate(&mut rng);
        let snap = build_state_snapshot(20, "root", &sample_entries());
        let payload = build_snapshot_sync_payload(&snap, "ab".repeat(32), 0);
        let mut signed = sign_snapshot_sync_payload(payload, &sk, 2_000_000, "nonce-bad-sig");

        // Replace the valid hex signature with garbage.
        signed.signature = "this is not valid hex at all!!!".to_string();

        assert!(
            !signed.has_valid_signature(),
            "corrupted signature field must cause has_valid_signature to return false, not panic (#1735)"
        );
    }

    /// `verify_inclusion_proof` must reject a proof whose `leaf_hash` has been
    /// tampered with, even if the key, value, sibling path, and snapshot root
    /// are otherwise intact (#1735).
    #[test]
    fn verify_inclusion_proof_rejects_tampered_leaf_hash() {
        let snap = build_state_snapshot(42, "abc123", &sample_entries());
        let key = b"station/b/humidity";
        let value = b"0.41";

        let mut proof = build_inclusion_proof(&snap, key).expect("proof must exist");
        assert!(verify_inclusion_proof(&proof, key, value), "sanity: valid proof");

        // Replace the leaf_hash with a bogus value while keeping everything else.
        proof.leaf_hash = "00".repeat(32);

        assert!(
            !verify_inclusion_proof(&proof, key, value),
            "tampered leaf_hash must cause verify_inclusion_proof to return false (#1735)"
        );
    }

    /// After `sign_snapshot_sync_payload` the `signer_id` field must equal the
    /// hex public key derived from the private key used for signing.  If a
    /// caller tried to forge a payload by setting a different `signer_id`, the
    /// signature would not verify (#1735).
    #[test]
    fn signed_payload_signer_id_is_public_key_of_signing_key() {
        let mut rng = StdRng::seed_from_u64(0x1394);
        let sk = PrivateKey::generate(&mut rng);
        let expected_signer_id = PublicKey::from_private_key(&sk).to_string();

        let snap = build_state_snapshot(77, "root", &sample_entries());
        let payload = build_snapshot_sync_payload(&snap, "ab".repeat(32), 0);
        let signed = sign_snapshot_sync_payload(payload, &sk, 9_000_000, "nonce-signer-id");

        assert_eq!(
            signed.signer_id, expected_signer_id,
            "signer_id must be the hex public key of the signing key (#1735)"
        );
        assert!(signed.has_valid_signature());
    }

    // ── #1735 hardening: snapshot corruption detection ───────────────────────

    /// Rebuilding a snapshot after modifying any single entry's VALUE must
    /// produce a different `snapshot_root_hex`.
    ///
    /// This proves that content tampering is detectable at the root level —
    /// a receiver who rebuilds the Merkle tree from the entries will produce
    /// a root that mismatches the signed `SnapshotSyncPayload.snapshot_root_hex`,
    /// exposing the tampering without needing a full entry-by-entry audit.
    #[test]
    fn snapshot_root_changes_when_entry_value_is_modified() {
        let entries = sample_entries();
        let original = build_state_snapshot(10, "state-root-orig", &entries);

        // Modify the value of one existing entry (station/b/humidity: 0.41 → 0.99).
        let tampered_entries: Vec<(Vec<u8>, Vec<u8>)> = entries
            .into_iter()
            .map(|(k, v)| {
                if k == b"station/b/humidity" {
                    (k, b"0.99".to_vec())
                } else {
                    (k, v)
                }
            })
            .collect();

        let tampered = build_state_snapshot(10, "state-root-orig", &tampered_entries);

        assert_ne!(
            tampered.snapshot_root_hex, original.snapshot_root_hex,
            "modifying a single entry value must change snapshot_root_hex — \
             value-tampering must be detectable via root mismatch (#1735)"
        );

        // Sanity: same entries reproduce the same root.
        let reproduced = build_state_snapshot(10, "state-root-orig", &sample_entries());
        assert_eq!(
            reproduced.snapshot_root_hex, original.snapshot_root_hex,
            "identical inputs must always produce the same snapshot_root_hex"
        );
    }

    /// Signature verification must fail when `payload.snapshot_root_hex` is
    /// tampered, even if all other payload fields remain intact.
    ///
    /// This is the most security-critical field in `SnapshotSyncPayload`:
    /// an adversary who intercepts a signed sync message and swaps the
    /// `snapshot_root_hex` to point to a different (malicious) Merkle tree
    /// must be caught by `has_valid_signature()` (#1735).
    #[test]
    fn signed_snapshot_payload_rejects_tampered_snapshot_root_hex() {
        let mut rng = StdRng::seed_from_u64(0x1735_6B);
        let sk = PrivateKey::generate(&mut rng);

        let snap = build_state_snapshot(55, "state-root-55", &sample_entries());
        let payload = build_snapshot_sync_payload(&snap, "ab".repeat(32), 0);
        let signed = sign_snapshot_sync_payload(payload, &sk, 5_500_000, "nonce-6b");

        assert!(
            signed.has_valid_signature(),
            "sanity: unmodified signed payload must verify"
        );

        // Replace snapshot_root_hex with an adversarial root while keeping
        // everything else (height, state_root_hex, entry_count, signature).
        let mut tampered = signed.clone();
        tampered.payload.snapshot_root_hex = "a".repeat(64);

        assert!(
            !tampered.has_valid_signature(),
            "has_valid_signature must return false when payload.snapshot_root_hex \
             is tampered — even if height, state_root_hex, entry_count, and \
             signature are unchanged (#1735)"
        );
    }

    /// Removing an entry from a snapshot must change `snapshot_root_hex`.
    ///
    /// A partial snapshot (missing entries) must be detectable at the root
    /// level — this is the "malformed/partial snapshot is rejected" criterion
    /// from issue #1735: the root the receiver computes from fewer entries
    /// will not match the honest root embedded in the signed payload.
    #[test]
    fn snapshot_root_changes_when_entry_is_removed() {
        let full_entries = sample_entries();
        let full_snap = build_state_snapshot(7, "state-root-7", &full_entries);

        // Build a "partial" snapshot missing the last entry.
        let partial_entries: Vec<(Vec<u8>, Vec<u8>)> = full_entries
            .into_iter()
            .take(full_entries_count_minus_one())
            .collect();
        let partial_snap = build_state_snapshot(7, "state-root-7", &partial_entries);

        assert_ne!(
            partial_snap.snapshot_root_hex, full_snap.snapshot_root_hex,
            "a partial snapshot (missing entries) must produce a different \
             snapshot_root_hex than the full snapshot — partial/malformed \
             snapshots are detectable via root mismatch (#1735)"
        );
    }

    /// Returns `sample_entries().len() - 1` for use in the partial-snapshot test.
    /// Extracted so the test cannot accidentally capture the entries Vec.
    fn full_entries_count_minus_one() -> usize {
        sample_entries().len() - 1
    }
}
