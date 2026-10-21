//! Dedicated block-producer authorized-set type (#2290, owner decision §R1).
//!
//! A **distinct privilege object** from the BFT-finality committee
//! (`consensus::validator_set::ValidatorSetChange`): it authorizes *who may
//! produce blocks*, not *who votes in finality*. It reuses #2227's **pattern**
//! (canonical set hash, `DomainSeparated` envelope, full-replacement change with
//! `epoch`/`effective_height`) but never its privilege — so a finality-committee
//! update is not silently a producer-authority update and vice versa.
//!
//! Member shape is owner-LOCKED (§R1/§R2):
//! `ProducerMember { producer_id, payout_address, public_key }` — `public_key` is
//! the rotatable signer authenticated on the block, `payout_address` is the stable
//! coinbase target (decoupled from the signing key, §R2), `producer_id` the stable
//! operator identity.
//!
//! The set-change history is on-chain consensus state
//! (`State::producer_set_history`); the authorised set at height `H` is the change
//! with the greatest `effective_height <= H`. All validation here is **fail-closed**
//! — a malformed/non-monotonic/out-of-order/duplicate/non-canonical change is a
//! rejected state transition, never a silently-normalized or dropped row.

use crate::common::types::Address;
use crate::crypto::hash::{Hash, hash_bytes};
use crate::crypto::signature::PublicKey;
use std::collections::BTreeSet;

/// One authorized block producer.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProducerMember {
    /// Stable operator identity (does not rotate with the signing key).
    pub producer_id: String,
    /// Stable coinbase target — where this producer's block reward is paid.
    /// Decoupled from `public_key` so signing-key rotation does not move payouts
    /// (§R2). Must be a canonical [`Address`] string.
    pub payout_address: String,
    /// ed25519 public key that authenticates this producer's block signature.
    pub public_key: [u8; 32],
}

/// An authenticated full-replacement change to the authorized producer set.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProducerSetChange {
    /// Generation; strictly increasing across accepted changes.
    pub epoch: u64,
    /// Height at/after which the new set is authoritative. Must be strictly
    /// greater than the height of the block that carries the change (causal rule,
    /// [`validate_candidate_change`]).
    pub effective_height: u64,
    /// The full producer committee that becomes active. Order not significant —
    /// the canonical hash sorts by `producer_id`.
    pub members: Vec<ProducerMember>,
}

impl ProducerSetChange {
    pub fn new(epoch: u64, effective_height: u64, members: Vec<ProducerMember>) -> Self {
        Self { epoch, effective_height, members }
    }

    /// The ONE canonical member order — sorted by `producer_id`. This is the
    /// single source of truth used by `producer_set_hash`, the `DomainSeparated`
    /// envelope, AND the `state_root` encoding, so member order is genuinely
    /// insignificant: two equal sets in any caller order canonicalize identically
    /// and thus hash to the same block/producer-set hash AND the same state_root
    /// (Dan review P1 — otherwise equal sets fork). Validation
    /// ([`validate_members`]) rejects duplicate `producer_id`, so the sort is a
    /// total order over a valid set.
    pub fn canonical_members(&self) -> Vec<&ProducerMember> {
        let mut sorted: Vec<&ProducerMember> = self.members.iter().collect();
        sorted.sort_by(|a, b| a.producer_id.cmp(&b.producer_id));
        sorted
    }

    /// Deterministic hash of the resulting producer set — the single identity of
    /// "which producer committee is this?" (mirrors `canonical_validator_set_hash`,
    /// distinct tag). Uses [`Self::canonical_members`] so member order is
    /// insignificant.
    ///
    /// ```text
    /// "wrfcoin.producer_set.v1|" || u64_LE(epoch) || u64_LE(effective_height)
    ///   || u64_LE(count)
    ///   || for each member (canonical order):
    ///        u64_LE(id_len) || id || u64_LE(addr_len) || payout_address || pubkey[32]
    /// ```
    pub fn producer_set_hash(&self) -> Hash {
        let sorted = self.canonical_members();

        let mut m = Vec::with_capacity(48 + sorted.len() * 96);
        m.extend_from_slice(b"wrfcoin.producer_set.v1|");
        m.extend_from_slice(&self.epoch.to_le_bytes());
        m.extend_from_slice(&self.effective_height.to_le_bytes());
        m.extend_from_slice(&(sorted.len() as u64).to_le_bytes());
        for e in &sorted {
            let id = e.producer_id.as_bytes();
            m.extend_from_slice(&(id.len() as u64).to_le_bytes());
            m.extend_from_slice(id);
            let addr = e.payout_address.as_bytes();
            m.extend_from_slice(&(addr.len() as u64).to_le_bytes());
            m.extend_from_slice(addr);
            m.extend_from_slice(&e.public_key);
        }
        hash_bytes(&m)
    }
}

/// An authenticated authorization of a [`ProducerSetChange`] (#2290 P4).
///
/// The `DomainSeparated` `ProducerSetChange` is the registered signed object (tag
/// `wrfcoin.producer_set_change.v1`); this envelope is its **named authenticated
/// container** — it carries the change plus the governance authorizer's key and
/// signature so a set change is a real, verifiable signed object that can be
/// gossiped/applied (the on-chain governance-ingestion tx path that carries this
/// is the P5 fast-follow). Signature is over the change's domain-separated
/// preimage under the active chain identity + protocol version, so it cannot be
/// replayed across chains/versions (#2123/#2232).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProducerSetChangeEnvelope {
    pub change: ProducerSetChange,
    /// ed25519 key of the governance authorizer.
    pub authorizer_pubkey: [u8; 32],
    /// Signature over `change`'s domain-separated preimage (64 bytes).
    pub signature: Vec<u8>,
}

impl ProducerSetChangeEnvelope {
    /// Authorize a change: sign its domain-separated preimage under the given
    /// identity + version.
    pub fn sign(
        change: ProducerSetChange,
        signer: &crate::crypto::signature::PrivateKey,
        identity: &crate::crypto::chain_binding::ChainIdentity,
        version: crate::crypto::chain_binding::SigningProtocolVersion,
    ) -> Self {
        let sig = crate::crypto::domain_signing::sign_domain_for(signer, &change, identity, version);
        let authorizer_pubkey: [u8; 32] = crate::crypto::signature::PublicKey::from_private_key(signer)
            .as_bytes()
            .try_into()
            .expect("ed25519 public key is 32 bytes");
        Self { change, authorizer_pubkey, signature: sig.as_bytes().to_vec() }
    }

    /// Verify the authorization, returning the authenticated authorizer key.
    /// Fail-closed: malformed key/signature or a bad signature all return `Err`.
    pub fn verify(
        &self,
        identity: &crate::crypto::chain_binding::ChainIdentity,
        version: crate::crypto::chain_binding::SigningProtocolVersion,
    ) -> Result<crate::crypto::signature::PublicKey, ProducerSetError> {
        let pk = crate::crypto::signature::PublicKey::from_bytes(&self.authorizer_pubkey)
            .map_err(|_| ProducerSetError::MalformedMemberKey)?;
        let sig = crate::crypto::signature::Signature::from_bytes(&self.signature)
            .map_err(|_| ProducerSetError::UnauthorizedChange)?;
        if crate::crypto::domain_signing::verify_domain_for(&pk, &self.change, &sig, identity, version) {
            Ok(pk)
        } else {
            Err(ProducerSetError::UnauthorizedChange)
        }
    }
}

/// Why a producer-set change/history is invalid. Every variant is a **rejected
/// state transition** — fail-closed, no silent normalization or drop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProducerSetError {
    NonMonotonicEpoch { prev: u64, got: u64 },
    NonMonotonicEffectiveHeight { prev: u64, got: u64 },
    /// `effective_height <= containing_block_height` — the causal off-by-one that
    /// forks pre-state verify vs replay.
    NotFutureDated { effective_height: u64, containing_block_height: u64 },
    EmptyMemberSet { effective_height: u64 },
    DuplicateProducerId,
    DuplicateMemberKey,
    MalformedMemberKey,
    /// `payout_address` is not a canonical [`Address`] string (§R2 invariant —
    /// no arbitrary/non-canonical strings admitted to consensus state).
    NonCanonicalPayoutAddress,
    /// A [`ProducerSetChangeEnvelope`] signature is missing, malformed, or does
    /// not verify under the declared authorizer key.
    UnauthorizedChange,
}

/// Validate one change's members: non-empty, well-formed keys, canonical payout
/// addresses, and no duplicate `producer_id`/`public_key` (§R1/§R2 invariants).
fn validate_members(c: &ProducerSetChange) -> Result<(), ProducerSetError> {
    if c.members.is_empty() {
        return Err(ProducerSetError::EmptyMemberSet { effective_height: c.effective_height });
    }
    let mut seen_key = BTreeSet::new();
    let mut seen_id = BTreeSet::new();
    for m in &c.members {
        if PublicKey::from_bytes(&m.public_key).is_err() {
            return Err(ProducerSetError::MalformedMemberKey);
        }
        // Canonical payout address: parses AND round-trips to itself (no
        // non-canonical/upper-case/short forms into consensus state).
        match m.payout_address.parse::<Address>() {
            Ok(addr) if addr.to_string() == m.payout_address => {},
            _ => return Err(ProducerSetError::NonCanonicalPayoutAddress),
        }
        if !seen_key.insert(m.public_key) {
            return Err(ProducerSetError::DuplicateMemberKey);
        }
        if !seen_id.insert(m.producer_id.clone()) {
            return Err(ProducerSetError::DuplicateProducerId);
        }
    }
    Ok(())
}

/// Validate a producer-set history **in the given application order** (do NOT
/// sort first — an out-of-order log must FAIL, not be normalized to validity).
/// Strictly-increasing epoch + effective_height, plus each change's members.
pub fn validate_producer_set_history(changes: &[ProducerSetChange]) -> Result<(), ProducerSetError> {
    let mut prev: Option<&ProducerSetChange> = None;
    for c in changes {
        if let Some(p) = prev {
            if c.epoch <= p.epoch {
                return Err(ProducerSetError::NonMonotonicEpoch { prev: p.epoch, got: c.epoch });
            }
            if c.effective_height <= p.effective_height {
                return Err(ProducerSetError::NonMonotonicEffectiveHeight {
                    prev: p.effective_height,
                    got: c.effective_height,
                });
            }
        }
        validate_members(c)?;
        prev = Some(c);
    }
    Ok(())
}

/// The executable transition seam: validate a candidate change about to be
/// applied by a block at `containing_block_height`, against the applied history.
/// Produces [`ProducerSetError::NotFutureDated`] on the causal off-by-one.
pub fn validate_candidate_change(
    previous_history: &[ProducerSetChange],
    candidate: &ProducerSetChange,
    containing_block_height: u64,
) -> Result<(), ProducerSetError> {
    if candidate.effective_height <= containing_block_height {
        return Err(ProducerSetError::NotFutureDated {
            effective_height: candidate.effective_height,
            containing_block_height,
        });
    }
    if let Some(last) = previous_history.last() {
        if candidate.epoch <= last.epoch {
            return Err(ProducerSetError::NonMonotonicEpoch { prev: last.epoch, got: candidate.epoch });
        }
        if candidate.effective_height <= last.effective_height {
            return Err(ProducerSetError::NonMonotonicEffectiveHeight {
                prev: last.effective_height,
                got: candidate.effective_height,
            });
        }
    }
    validate_members(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signature::PrivateKey;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn member(seed: u64, id: &str) -> ProducerMember {
        let mut rng = StdRng::seed_from_u64(seed);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);
        let key: [u8; 32] = pk.as_bytes().try_into().unwrap();
        // payout_address is the coinbase target in canonical `Address` form
        // (0x-hex, what the mining route's `parse::<Address>()` accepts) — NOT
        // `pk.to_address()` (a distinct `wrf`-prefixed form). Decoupled from the
        // signing key per §R2.
        let addr = Address(key[0..20].try_into().unwrap());
        ProducerMember {
            producer_id: id.to_string(),
            payout_address: addr.to_string(),
            public_key: key,
        }
    }

    #[test]
    fn canonical_hash_is_order_independent_over_members() {
        let a = member(1, "a");
        let b = member(2, "b");
        let h1 = ProducerSetChange::new(0, 0, vec![a.clone(), b.clone()]).producer_set_hash();
        let h2 = ProducerSetChange::new(0, 0, vec![b, a]).producer_set_hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn history_validation_rejects_out_of_order_and_bad_members() {
        let a = member(3, "a");
        let b = member(4, "b");
        // out-of-order (epoch/height decreasing) must FAIL in given order
        let ooo = [
            ProducerSetChange::new(1, 100, vec![b.clone()]),
            ProducerSetChange::new(0, 0, vec![a.clone()]),
        ];
        assert!(matches!(
            validate_producer_set_history(&ooo),
            Err(ProducerSetError::NonMonotonicEpoch { .. })
                | Err(ProducerSetError::NonMonotonicEffectiveHeight { .. })
        ));
        // empty
        assert!(matches!(
            validate_producer_set_history(&[ProducerSetChange::new(0, 0, vec![])]),
            Err(ProducerSetError::EmptyMemberSet { .. })
        ));
        // duplicate producer_id
        let mut dup_id = member(5, "same");
        let mut dup_id2 = member(6, "same");
        dup_id.producer_id = "same".into();
        dup_id2.producer_id = "same".into();
        assert_eq!(
            validate_producer_set_history(&[ProducerSetChange::new(0, 0, vec![dup_id, dup_id2])]),
            Err(ProducerSetError::DuplicateProducerId)
        );
        // malformed key
        let bad = ProducerMember { producer_id: "x".into(), payout_address: member(7, "x").payout_address, public_key: [0x02u8; 32] };
        assert_eq!(
            validate_producer_set_history(&[ProducerSetChange::new(0, 0, vec![bad])]),
            Err(ProducerSetError::MalformedMemberKey)
        );
        // non-canonical payout address
        let mut noncanon = member(8, "x");
        noncanon.payout_address = "not-an-address".into();
        assert_eq!(
            validate_producer_set_history(&[ProducerSetChange::new(0, 0, vec![noncanon])]),
            Err(ProducerSetError::NonCanonicalPayoutAddress)
        );
    }

    #[test]
    fn envelope_sign_verify_round_trips_and_rejects_tampering() {
        use crate::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
        use crate::crypto::signature::PrivateKey;
        use rand::SeedableRng;
        let id = ChainIdentity::new("wrfcoin-testnet", "testnet");
        let mut rng = StdRng::seed_from_u64(77);
        let sk = PrivateKey::generate(&mut rng);
        let pk = PublicKey::from_private_key(&sk);

        let change = ProducerSetChange::new(1, 10, vec![member(50, "a")]);
        let env = ProducerSetChangeEnvelope::sign(change.clone(), &sk, &id, SigningProtocolVersion::V2);
        assert_eq!(env.verify(&id, SigningProtocolVersion::V2), Ok(pk)); // secret-leak-ok: verifier returns only the public key

        // Tamper the change → signature no longer verifies.
        let mut tampered = env.clone();
        tampered.change = ProducerSetChange::new(2, 20, vec![member(51, "b")]);
        assert_eq!( // secret-leak-ok: verifier returns only a public key or public error variant
            tampered.verify(&id, SigningProtocolVersion::V2),
            Err(ProducerSetError::UnauthorizedChange)
        );
        // Wrong version → no cross-version replay.
        assert!(env.verify(&id, SigningProtocolVersion::V1).is_err()); // secret-leak-ok: boolean assertion over a public protocol version
    }

    #[test]
    fn candidate_causal_rule_k_h_boundaries() {
        let a = member(9, "a");
        let prev = vec![ProducerSetChange::new(0, 0, vec![a.clone()])];
        let k = 10u64;
        assert_eq!(
            validate_candidate_change(&prev, &ProducerSetChange::new(1, k, vec![a.clone()]), k),
            Err(ProducerSetError::NotFutureDated { effective_height: k, containing_block_height: k })
        );
        assert!(matches!(
            validate_candidate_change(&prev, &ProducerSetChange::new(1, k - 1, vec![a.clone()]), k),
            Err(ProducerSetError::NotFutureDated { .. })
        ));
        assert_eq!(
            validate_candidate_change(&prev, &ProducerSetChange::new(1, k + 1, vec![a]), k),
            Ok(())
        );
    }
}
