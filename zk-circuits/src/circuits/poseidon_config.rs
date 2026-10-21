//! Poseidon hash configuration for BN254 Fr.
//!
//! Provides a deterministic Poseidon sponge parameterization suitable for
//! use inside R1CS circuits (Groth16 over BN254).

use ark_bn254::Fr;
use ark_crypto_primitives::sponge::{
    poseidon::{find_poseidon_ark_and_mds, PoseidonConfig, PoseidonSponge},
    CryptographicSponge,
};
/// Standard Poseidon parameters for BN254 Fr.
///
/// - **rate** = 2 (absorb 2 field elements per permutation)
/// - **capacity** = 1 (state width = 3)
/// - **full rounds** = 8
/// - **partial rounds** = 57 (128-bit security for 254-bit field)
/// - **alpha** = 5 (S-box exponent x^5)
const RATE: usize = 2;
const FULL_ROUNDS: usize = 8;
const PARTIAL_ROUNDS: usize = 57;
const ALPHA: u64 = 5;

/// Build the canonical Poseidon configuration for BN254 Fr.
///
/// Round constants (ARK) and MDS matrix are generated deterministically
/// via the Poseidon Grain LFSR, matching the reference implementation.
pub fn poseidon_config_bn254() -> PoseidonConfig<Fr> {
    let (ark, mds) = find_poseidon_ark_and_mds::<Fr>(
        254,                    // prime field bit size
        RATE,                   // rate
        FULL_ROUNDS as u64,     // full rounds
        PARTIAL_ROUNDS as u64,  // partial rounds
        0,                      // skip_matrices
    );
    PoseidonConfig::new(FULL_ROUNDS, PARTIAL_ROUNDS, ALPHA, mds, ark, RATE, 1)
}

/// Compute a Poseidon hash of field elements (native, outside R1CS).
///
/// Absorbs all elements into a Poseidon sponge and squeezes one field element.
pub fn poseidon_hash(fields: &[Fr]) -> Fr {
    let config = poseidon_config_bn254();
    let mut sponge = PoseidonSponge::new(&config);
    for f in fields {
        sponge.absorb(f);
    }
    let out: Vec<Fr> = sponge.squeeze_field_elements(1);
    out[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poseidon_hash_deterministic() {
        let a = vec![Fr::from(1u64), Fr::from(2u64), Fr::from(3u64)];
        let h1 = poseidon_hash(&a);
        let h2 = poseidon_hash(&a);
        assert_eq!(h1, h2, "Poseidon hash must be deterministic");
    }

    #[test]
    fn poseidon_hash_differs_on_different_input() {
        let a = vec![Fr::from(1u64), Fr::from(2u64)];
        let b = vec![Fr::from(1u64), Fr::from(3u64)];
        let ha = poseidon_hash(&a);
        let hb = poseidon_hash(&b);
        assert_ne!(ha, hb, "Different inputs must produce different hashes");
    }
}
