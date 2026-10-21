//! Validator-set changes as a first-class, signed on-chain fact (#2227).
//!
//! A validator set *does* change, and that change must be authenticated — but
//! before #2227 no Rust type existed to attach the reserved domain tag
//! `wrfcoin.validator_set_change.v1` to. The set changed only implicitly (a new
//! [`bft_finality::BftFinalityGadget`](crate) constructed from staking state),
//! so a peer had nothing signed to verify a rotation against.
//!
//! [`ValidatorSetChange`] is that missing type: it names the epoch a new
//! committee takes effect, the height it becomes authoritative, and the full
//! ordered set, and it implements
//! [`DomainSeparated`](crate::crypto::domain_signing::DomainSeparated) under the
//! reserved tag (see `crypto/domain_impls.rs`) so it can be signed and verified
//! through the same chain-bound framing as every other signed object.
//!
//! # Relationship to #2256
//!
//! [`ValidatorSetChange::validator_set_hash`] is computed the same way a finality
//! committee's identity is (`consensus_engines::selection::bft_finality::compute_validator_set_hash`):
//! the id-sorted `(id, stake_weight, public_key)` projection. A set-change
//! announces the committee that finality votes then bind themselves to (#2256),
//! so the two agree on what "this validator set" hashes to.

use crate::crypto::hash::{Hash, hash_bytes};

/// A single member of a validator set (#2227).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ValidatorSetMember {
    /// Unique validator identifier (address / public-key hash).
    pub validator_id: String,
    /// Stake weight used for supermajority calculation.
    pub stake_weight: u64,
    /// ed25519 public key that authenticates this validator's signatures.
    pub public_key: [u8; 32],
}

/// An authenticated change to the active validator set (#2227).
///
/// Makes set membership a first-class, signed, on-chain fact. The change is a
/// **full replacement** — it declares the entire new committee rather than a
/// delta — so a verifier never has to reconstruct the resulting set from a chain
/// of deltas to know what it is (and to hash it, per [`Self::validator_set_hash`]).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ValidatorSetChange {
    /// Epoch (generation) the new set takes effect at. Strictly increasing
    /// across accepted changes; an out-of-order epoch is rejected by the caller
    /// that applies the change.
    pub epoch: u64,
    /// Block height at or after which the new set is authoritative.
    pub effective_height: u64,
    /// The full validator set that becomes active. Order is not significant —
    /// the canonical preimage and [`Self::validator_set_hash`] sort by
    /// `validator_id` — but [`Self::new`] stores it sorted for a canonical value.
    pub validators: Vec<ValidatorSetMember>,
}

impl ValidatorSetChange {
    /// Construct a change, storing `validators` sorted by id so the value is
    /// canonical regardless of the caller's order.
    pub fn new(epoch: u64, effective_height: u64, mut validators: Vec<ValidatorSetMember>) -> Self {
        validators.sort_by(|a, b| a.validator_id.cmp(&b.validator_id));
        Self {
            epoch,
            effective_height,
            validators,
        }
    }

    /// Deterministic hash of the resulting ordered validator set.
    ///
    /// Delegates to [`canonical_validator_set_hash`] — the **one** definition of
    /// a validator set's identity, shared with the finality gadget (#2256) so a
    /// set-change and the votes cast under the set it installs hash to the same
    /// value byte-for-byte. `ValidatorSetMember` always carries a key, so every
    /// entry is `Some`.
    pub fn validator_set_hash(&self) -> Hash {
        canonical_validator_set_hash(self.validators.iter().map(|m| ValidatorSetEntry {
            id: &m.validator_id,
            stake_weight: m.stake_weight,
            public_key: Some(m.public_key),
        }))
    }

    /// Total stake weight of the resulting set.
    pub fn total_stake_weight(&self) -> u128 {
        self.validators
            .iter()
            .map(|m| u128::from(m.stake_weight))
            .sum()
    }
}

/// One validator's contribution to a set-identity hash (#2227/#2256).
///
/// A borrow-friendly, key-optional view so both `ValidatorSetChange`
/// (key always present) and the finality gadget's `FinalityValidator`
/// (`public_key: Option<[u8; 32]>`) feed the *same* hash function.
pub struct ValidatorSetEntry<'a> {
    /// Validator id (address / public-key hash).
    pub id: &'a str,
    /// Stake weight.
    pub stake_weight: u64,
    /// ed25519 public key, if registered.
    pub public_key: Option<[u8; 32]>,
}

/// **The** canonical hash of an ordered validator set — the single source of
/// truth for "which committee is this?" (#2227 + #2256).
///
/// A `ValidatorSetChange` declares a committee and the finality gadget's votes
/// bind to one; unless both hash a set to identical bytes, a set-change cannot
/// name the set the votes reference. This function is that one definition, so
/// the two cannot drift. `consensus_engines::selection::bft_finality::compute_validator_set_hash`
/// delegates here rather than re-encoding.
///
/// # Encoding (versioned by the `wrfcoin.validator_set.v1` tag)
///
/// Entries are sorted by `id`, then:
/// ```text
/// "wrfcoin.validator_set.v1|"
///   || u64_LE(count)
///   || for each entry: u64_LE(id_len) || id || u64_LE(stake_weight) || key
/// ```
/// where `key` is `0x00` when absent, or `0x01 || pubkey[32]` when present. The
/// 1-byte presence discriminant keeps `None` and any real key unambiguous. All
/// integers are little-endian, matching the rest of shared-protocol.
pub fn canonical_validator_set_hash<'a, I>(entries: I) -> Hash
where
    I: IntoIterator<Item = ValidatorSetEntry<'a>>,
{
    let mut sorted: Vec<ValidatorSetEntry<'a>> = entries.into_iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(b.id));

    let mut material = Vec::with_capacity(32 + sorted.len() * 49);
    material.extend_from_slice(b"wrfcoin.validator_set.v1|");
    material.extend_from_slice(&(sorted.len() as u64).to_le_bytes());
    for e in &sorted {
        let id = e.id.as_bytes();
        material.extend_from_slice(&(id.len() as u64).to_le_bytes());
        material.extend_from_slice(id);
        material.extend_from_slice(&e.stake_weight.to_le_bytes());
        match e.public_key {
            Some(pk) => {
                material.push(1);
                material.extend_from_slice(&pk);
            },
            None => material.push(0),
        }
    }
    hash_bytes(&material)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
    use crate::crypto::domain_signing::{DomainSeparated, sign_domain_for, verify_domain_for};
    use crate::crypto::signature::{PrivateKey, PublicKey};
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn member(id: &str, stake: u64, seed: u8) -> ValidatorSetMember {
        ValidatorSetMember {
            validator_id: id.into(),
            stake_weight: stake,
            public_key: [seed; 32],
        }
    }

    #[test]
    fn tag_is_the_reserved_one() {
        assert_eq!(
            ValidatorSetChange::DOMAIN_TAG,
            b"wrfcoin.validator_set_change.v1"
        );
    }

    #[test]
    fn canonical_body_is_order_independent() {
        let a = ValidatorSetChange::new(3, 100, vec![member("v0", 10, 1), member("v1", 20, 2)]);
        let b = ValidatorSetChange::new(3, 100, vec![member("v1", 20, 2), member("v0", 10, 1)]);
        assert_eq!(a.canonical_body(), b.canonical_body());
        assert_eq!(a.validator_set_hash(), b.validator_set_hash());
    }

    #[test]
    fn canonical_body_binds_every_field() {
        let base = ValidatorSetChange::new(3, 100, vec![member("v0", 10, 1)]);
        let diff_epoch = ValidatorSetChange::new(4, 100, vec![member("v0", 10, 1)]);
        let diff_height = ValidatorSetChange::new(3, 101, vec![member("v0", 10, 1)]);
        let diff_stake = ValidatorSetChange::new(3, 100, vec![member("v0", 11, 1)]);
        let diff_key = ValidatorSetChange::new(3, 100, vec![member("v0", 10, 2)]);
        let diff_member =
            ValidatorSetChange::new(3, 100, vec![member("v0", 10, 1), member("v1", 5, 3)]);

        for other in [diff_epoch, diff_height, diff_stake, diff_key, diff_member] {
            assert_ne!(
                base.canonical_body(),
                other.canonical_body(),
                "every field must be bound into the preimage"
            );
        }
    }

    #[test]
    fn signs_and_verifies_through_the_domain_frame() {
        let mut rng = StdRng::seed_from_u64(2227);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);
        let identity = ChainIdentity::new("wrfcoin-testnet", "testnet");
        let version = SigningProtocolVersion::V2;

        let change =
            ValidatorSetChange::new(5, 200, vec![member("v0", 10, 1), member("v1", 20, 2)]);
        let sig = sign_domain_for(&sk, &change, &identity, version);
        assert!(verify_domain_for(&pk, &change, &sig, &identity, version));

        // A tampered field breaks verification (the pubkey is bound).
        let tampered =
            ValidatorSetChange::new(6, 200, vec![member("v0", 10, 1), member("v1", 20, 2)]);
        assert!(!verify_domain_for(&pk, &tampered, &sig, &identity, version));
    }
}
