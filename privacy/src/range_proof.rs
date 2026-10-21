//! Zero-knowledge range proofs.
//!
//! Proves a committed value lies in `[0, 2^n)` without revealing it.
//!
//! # Approach
//!
//! Uses Pedersen commitment bit-decomposition with Schnorr OR-proofs
//! via [`merlin::Transcript`].  For each bit `b_i` of the committed value,
//! the prover creates a Pedersen commitment `C_i = b_i * G + r_i * H` and
//! proves `b_i in {0,1}` using a Schnorr OR-proof (sigma protocol with
//! Fiat-Shamir).  The sum of bit commitments equals the original commitment:
//! `sum(C_i * 2^i) == C`.
//!
//! This avoids the `bulletproofs` crate and its incompatible `curve25519-dalek-ng`
//! fork.

use curve25519_dalek::{
    constants::RISTRETTO_BASEPOINT_TABLE,
    ristretto::{CompressedRistretto, RistrettoPoint},
    scalar::Scalar,
};
use merlin::Transcript;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_512};

use crate::error::{PrivacyError, PrivacyResult};
use crate::pedersen::PedersenCommitment;

const DEFAULT_BITS: usize = 64;

fn h_generator() -> RistrettoPoint {
    let mut hasher = Sha3_512::new();
    hasher.update(b"WRFCOIN_PEDERSEN_H_GENERATOR");
    hasher.update(RISTRETTO_BASEPOINT_TABLE.basepoint().compress().as_bytes());
    let hash = hasher.finalize();
    let mut uniform = [0u8; 64];
    uniform.copy_from_slice(&hash);
    RistrettoPoint::from_uniform_bytes(&uniform)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeProof {
    pub n_bits: usize,
    pub bit_commitments: Vec<[u8; 32]>,
    #[serde(with = "vec_hex")]
    pub challenges: Vec<[u8; 32]>,
    #[serde(with = "vec_hex")]
    pub sub_challenges_0: Vec<[u8; 32]>,
    #[serde(with = "vec_hex")]
    pub responses_0: Vec<[u8; 32]>,
    #[serde(with = "vec_hex")]
    pub responses_1: Vec<[u8; 32]>,
}

mod vec_hex {
    use serde::{self, Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S>(data: &[[u8; 32]], serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        let hex_vec: Vec<String> = data.iter().map(hex::encode).collect();
        hex_vec.serialize(serializer)
    }
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<[u8; 32]>, D::Error>
    where D: Deserializer<'de> {
        let hex_vec: Vec<String> = Vec::deserialize(deserializer)?;
        hex_vec.iter().map(|s| {
            let bytes = hex::decode(s).map_err(|e| serde::de::Error::custom(format!("hex: {e}")))?;
            if bytes.len() != 32 { return Err(serde::de::Error::custom("expected 32 bytes")); }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            Ok(arr)
        }).collect()
    }
}

impl RangeProof {
    pub fn prove(value: u64, blinding: &Scalar, n_bits: usize) -> PrivacyResult<Self> {
        if n_bits == 0 || n_bits > 64 {
            return Err(PrivacyError::RangeProof("n_bits must be in [1, 64]".into()));
        }
        if n_bits < 64 && value >= (1u64 << n_bits) {
            return Err(PrivacyError::RangeProof("value out of range for given bit width".into()));
        }
        let h = h_generator();
        let two = Scalar::from(2u64);
        let mut bit_blindings = Vec::with_capacity(n_bits);
        let mut bit_values = Vec::with_capacity(n_bits);
        let mut accum = Scalar::ZERO;
        let mut power = Scalar::ONE;
        for i in 0..n_bits {
            bit_values.push(((value >> i) & 1) as u8);
            if i < n_bits - 1 {
                let mut rb = [0u8; 32]; OsRng.fill_bytes(&mut rb);
                let r_i = Scalar::from_bytes_mod_order(rb);
                accum += power * r_i;
                bit_blindings.push(r_i);
            } else {
                bit_blindings.push((blinding - accum) * power.invert());
            }
            power *= two;
        }
        let mut bit_commitments = Vec::with_capacity(n_bits);
        let mut transcript = Transcript::new(b"wrfcoin-range-proof-v1");
        for i in 0..n_bits {
            let c_i = if bit_values[i] == 1 {
                &Scalar::ONE * RISTRETTO_BASEPOINT_TABLE + bit_blindings[i] * h
            } else { bit_blindings[i] * h };
            let compressed = c_i.compress().to_bytes();
            bit_commitments.push(compressed);
            transcript.append_message(b"bit-commitment", &compressed);
        }
        let mut challenges = Vec::with_capacity(n_bits);
        let mut sub_challenges_0 = Vec::with_capacity(n_bits);
        let mut responses_0 = Vec::with_capacity(n_bits);
        let mut responses_1 = Vec::with_capacity(n_bits);
        let g_point = RISTRETTO_BASEPOINT_TABLE.basepoint();
        for i in 0..n_bits {
            let r_i = bit_blindings[i];
            let c_i = CompressedRistretto::from_slice(&bit_commitments[i]).unwrap().decompress().unwrap();
            if bit_values[i] == 0 {
                let mut e1b = [0u8; 32]; OsRng.fill_bytes(&mut e1b);
                let e1 = Scalar::from_bytes_mod_order(e1b);
                let mut s1b = [0u8; 32]; OsRng.fill_bytes(&mut s1b);
                let s1 = Scalar::from_bytes_mod_order(s1b);
                let r1 = s1 * h - e1 * (c_i - g_point);
                let mut kb = [0u8; 32]; OsRng.fill_bytes(&mut kb);
                let k = Scalar::from_bytes_mod_order(kb);
                let r0 = k * h;
                transcript.append_message(b"R0", &r0.compress().to_bytes());
                transcript.append_message(b"R1", &r1.compress().to_bytes());
                let mut eb = [0u8; 32];
                transcript.challenge_bytes(b"or-challenge", &mut eb);
                let e = Scalar::from_bytes_mod_order(eb);
                let e0 = e - e1;
                challenges.push(eb);
                sub_challenges_0.push(e0.to_bytes());
                responses_0.push((k + e0 * r_i).to_bytes());
                responses_1.push(s1.to_bytes());
            } else {
                let mut e0b = [0u8; 32]; OsRng.fill_bytes(&mut e0b);
                let e0 = Scalar::from_bytes_mod_order(e0b);
                let mut s0b = [0u8; 32]; OsRng.fill_bytes(&mut s0b);
                let s0 = Scalar::from_bytes_mod_order(s0b);
                let r0 = s0 * h - e0 * c_i;
                let mut kb = [0u8; 32]; OsRng.fill_bytes(&mut kb);
                let k = Scalar::from_bytes_mod_order(kb);
                let r1 = k * h;
                transcript.append_message(b"R0", &r0.compress().to_bytes());
                transcript.append_message(b"R1", &r1.compress().to_bytes());
                let mut eb = [0u8; 32];
                transcript.challenge_bytes(b"or-challenge", &mut eb);
                let e = Scalar::from_bytes_mod_order(eb);
                let e1 = e - e0;
                challenges.push(eb);
                sub_challenges_0.push(e0.to_bytes());
                responses_0.push(s0.to_bytes());
                responses_1.push((k + e1 * r_i).to_bytes());
            }
        }
        Ok(Self { n_bits, bit_commitments, challenges, sub_challenges_0, responses_0, responses_1 })
    }

    pub fn verify_structure(&self) -> PrivacyResult<bool> {
        if self.n_bits == 0 || self.n_bits > 64 { return Ok(false); }
        if self.bit_commitments.len() != self.n_bits
            || self.challenges.len() != self.n_bits
            || self.sub_challenges_0.len() != self.n_bits
            || self.responses_0.len() != self.n_bits
            || self.responses_1.len() != self.n_bits
        { return Ok(false); }
        for bytes in &self.bit_commitments {
            if CompressedRistretto::from_slice(bytes).ok().and_then(|c| c.decompress()).is_none() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn verify(&self, commitment: &PedersenCommitment) -> PrivacyResult<bool> {
        if !self.verify_structure()? { return Ok(false); }
        let h = h_generator();
        let two = Scalar::from(2u64);
        let g_point = RISTRETTO_BASEPOINT_TABLE.basepoint();
        let mut transcript = Transcript::new(b"wrfcoin-range-proof-v1");
        for bytes in &self.bit_commitments {
            transcript.append_message(b"bit-commitment", bytes);
        }
        let mut sum = RistrettoPoint::default();
        let mut power_of_two = Scalar::ONE;
        for i in 0..self.n_bits {
            let c_i = CompressedRistretto::from_slice(&self.bit_commitments[i])
                .ok().and_then(|c| c.decompress())
                .ok_or_else(|| PrivacyError::RangeProof("invalid bit commitment".into()))?;
            let e_0 = Scalar::from_bytes_mod_order(self.sub_challenges_0[i]);
            let s_0 = Scalar::from_bytes_mod_order(self.responses_0[i]);
            let s_1 = Scalar::from_bytes_mod_order(self.responses_1[i]);
            let e = Scalar::from_bytes_mod_order(self.challenges[i]);
            let e_1 = e - e_0;
            let big_r_0 = s_0 * h - e_0 * c_i;
            let big_r_1 = s_1 * h - e_1 * (c_i - g_point);
            transcript.append_message(b"R0", &big_r_0.compress().to_bytes());
            transcript.append_message(b"R1", &big_r_1.compress().to_bytes());
            let mut ep = [0u8; 32];
            transcript.challenge_bytes(b"or-challenge", &mut ep);
            if ep != self.challenges[i] { return Ok(false); }
            sum += power_of_two * c_i;
            power_of_two *= two;
        }
        Ok(sum == commitment.commitment)
    }

    pub fn prove_default(value: u64, blinding: &Scalar) -> PrivacyResult<Self> {
        Self::prove(value, blinding, DEFAULT_BITS)
    }
    pub fn to_json(&self) -> PrivacyResult<String> {
        serde_json::to_string(self).map_err(PrivacyError::from)
    }
    pub fn from_json(json: &str) -> PrivacyResult<Self> {
        serde_json::from_str(json).map_err(PrivacyError::from)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedRangeProof { pub proofs: Vec<RangeProof> }

impl AggregatedRangeProof {
    pub fn prove(values: &[u64], blindings: &[Scalar], n_bits: usize) -> PrivacyResult<Self> {
        if values.len() != blindings.len() {
            return Err(PrivacyError::RangeProof("values and blindings length mismatch".into()));
        }
        let proofs: PrivacyResult<Vec<_>> = values.iter().zip(blindings).map(|(&v,b)| RangeProof::prove(v,b,n_bits)).collect();
        Ok(Self { proofs: proofs? })
    }
    pub fn verify(&self, commitments: &[PedersenCommitment]) -> PrivacyResult<bool> {
        if self.proofs.len() != commitments.len() { return Ok(false); }
        for (p,c) in self.proofs.iter().zip(commitments) { if !p.verify(c)? { return Ok(false); } }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn prove_and_verify_small_value() {
        let (c,b) = PedersenCommitment::commit(42u64);
        let p = RangeProof::prove(42, &b, 8).unwrap();
        assert!(p.verify_structure().unwrap());
        assert!(p.verify(&c).unwrap());
    }
    #[test] fn prove_and_verify_zero() {
        let (c,b) = PedersenCommitment::commit(0u64);
        assert!(RangeProof::prove(0, &b, 8).unwrap().verify(&c).unwrap());
    }
    #[test] fn prove_and_verify_max_8bit() {
        let (c,b) = PedersenCommitment::commit(255u64);
        assert!(RangeProof::prove(255, &b, 8).unwrap().verify(&c).unwrap());
    }
    #[test] fn reject_value_out_of_range() { assert!(RangeProof::prove(256, &Scalar::from(1u64), 8).is_err()); }
    #[test] fn reject_zero_bits() { assert!(RangeProof::prove(0, &Scalar::from(1u64), 0).is_err()); }
    #[test] fn reject_too_many_bits() { assert!(RangeProof::prove(0, &Scalar::from(1u64), 65).is_err()); }
    #[test] fn wrong_commitment_fails() {
        let (_,b) = PedersenCommitment::commit(42u64);
        let p = RangeProof::prove(42, &b, 8).unwrap();
        let (w,_) = PedersenCommitment::commit(43);
        assert!(!p.verify(&w).unwrap());
    }
    #[test] fn serde_roundtrip() {
        let (_,b) = PedersenCommitment::commit(100u64);
        let p = RangeProof::prove(100, &b, 8).unwrap();
        let j = p.to_json().unwrap();
        let r = RangeProof::from_json(&j).unwrap();
        assert_eq!(r.n_bits, p.n_bits);
        assert_eq!(r.bit_commitments.len(), p.bit_commitments.len());
        assert_eq!(r.sub_challenges_0.len(), p.sub_challenges_0.len());
    }
    #[test] fn prove_default_64bit() {
        let (c,b) = PedersenCommitment::commit(1_000_000u64);
        let p = RangeProof::prove_default(1_000_000, &b).unwrap();
        assert_eq!(p.n_bits, 64);
        assert!(p.verify(&c).unwrap());
    }
    #[test] fn aggregated_proof_multiple_values() {
        let vals = [10u64, 20, 30];
        let pairs: Vec<_> = vals.iter().map(|&v| PedersenCommitment::commit(v)).collect();
        let blindings: Vec<_> = pairs.iter().map(|(_,b)| *b).collect();
        let commits: Vec<_> = pairs.into_iter().map(|(c,_)| c).collect();
        assert!(AggregatedRangeProof::prove(&vals, &blindings, 8).unwrap().verify(&commits).unwrap());
    }
    #[test] fn aggregated_proof_mismatched_lengths() {
        assert!(AggregatedRangeProof::prove(&[1,2], &[Scalar::ONE], 8).is_err());
    }
    #[test] fn tampered_sub_challenge_rejected() {
        let (c,b) = PedersenCommitment::commit(42u64);
        let mut p = RangeProof::prove(42, &b, 8).unwrap();
        p.sub_challenges_0[0][0] ^= 0xff;
        assert!(!p.verify(&c).unwrap());
    }
    #[test] fn tampered_response_rejected() {
        let (c,b) = PedersenCommitment::commit(42u64);
        let mut p = RangeProof::prove(42, &b, 8).unwrap();
        p.responses_0[0][0] ^= 0xff;
        assert!(!p.verify(&c).unwrap());
    }
    #[test] fn forged_bit_commitments_rejected() {
        let (c,b) = PedersenCommitment::commit(42u64);
        let mut p = RangeProof::prove(42, &b, 8).unwrap();
        p.bit_commitments.swap(0, 1);
        assert!(!p.verify(&c).unwrap());
    }
}
