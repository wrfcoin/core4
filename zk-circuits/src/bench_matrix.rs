//! Measurement matrix for the proving-cost benchmark suite (story #2177).
//!
//! This module exists so that every measurement surface agrees on *what* is
//! being measured. It is shared by:
//!
//! - `benches/proving.rs` — criterion wall-clock timings (setup / prove / verify)
//! - `src/bin/zk-proving-report.rs` — the per-phase cost table (peak RSS, key
//!   sizes on disk, proof bytes) that produces `docs/zk/PROVING_COSTS.md`
//! - `src/budgets.rs` — the enforced verification-time and proof-size budgets
//!
//! Every case is fully deterministic: fixed witnesses, no RNG in this module.
//! The only randomness is inside [`crate::proving`] (Groth16 setup toxic waste
//! and the per-proof blinding scalars), which is `OsRng` by construction.

use ark_bn254::{Bn254, Fr};
use ark_groth16::{PreparedVerifyingKey, ProvingKey};
use ark_relations::gr1cs::{ConstraintSynthesizer, ConstraintSystem};

use crate::circuits::confidential_transfer::ConfidentialTransferCircuit;
use crate::circuits::poseidon_config::poseidon_hash;
use crate::circuits::range_proof::RangeProofCircuit;
use crate::circuits::selective_disclosure::SelectiveDisclosureCircuit;
use crate::circuits::weather_model::WeatherModelCircuit;
use crate::error::ZkError;
use crate::proving;
use crate::types::ZkProof;

/// A single (circuit, input size) point in the measurement matrix.
///
/// The variant fields are the *input size knob* for that circuit family —
/// the thing story #2177 asks us to sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CircuitKind {
    /// Range proof over `n_bits` (bit-decomposition width dominates the cost).
    RangeProof { n_bits: usize },
    /// Confidential transfer with `num_inputs` x `num_outputs` notes, each
    /// range-checked to `n_bits`.
    ConfidentialTransfer {
        num_inputs: usize,
        num_outputs: usize,
        n_bits: usize,
    },
    /// Selective disclosure over `num_fields` committed field elements.
    SelectiveDisclosure { num_fields: usize },
    /// Weather-model execution integrity over `num_samples` intermediate states.
    WeatherModel { num_samples: usize },
}

/// The matrix measured and published in `docs/zk/PROVING_COSTS.md`.
///
/// Kept deliberately small enough that a full sweep finishes in minutes, and
/// wide enough that the growth curve of each family is visible (two or more
/// input sizes per family).
pub const MATRIX: &[CircuitKind] = &[
    CircuitKind::RangeProof { n_bits: 8 },
    CircuitKind::RangeProof { n_bits: 16 },
    CircuitKind::RangeProof { n_bits: 32 },
    CircuitKind::RangeProof { n_bits: 64 },
    CircuitKind::ConfidentialTransfer {
        num_inputs: 1,
        num_outputs: 1,
        n_bits: 64,
    },
    CircuitKind::ConfidentialTransfer {
        num_inputs: 2,
        num_outputs: 2,
        n_bits: 64,
    },
    CircuitKind::ConfidentialTransfer {
        num_inputs: 4,
        num_outputs: 4,
        n_bits: 64,
    },
    CircuitKind::SelectiveDisclosure { num_fields: 1 },
    CircuitKind::SelectiveDisclosure { num_fields: 4 },
    CircuitKind::SelectiveDisclosure { num_fields: 16 },
    CircuitKind::SelectiveDisclosure { num_fields: 64 },
    CircuitKind::WeatherModel { num_samples: 1 },
    CircuitKind::WeatherModel { num_samples: 8 },
    CircuitKind::WeatherModel { num_samples: 32 },
    CircuitKind::WeatherModel { num_samples: 128 },
];

impl CircuitKind {
    /// Circuit family name, stable across input sizes.
    pub fn family(&self) -> &'static str {
        match self {
            CircuitKind::RangeProof { .. } => "range-proof",
            CircuitKind::ConfidentialTransfer { .. } => "confidential-transfer",
            CircuitKind::SelectiveDisclosure { .. } => "selective-disclosure",
            CircuitKind::WeatherModel { .. } => "weather-model",
        }
    }

    /// Human-readable input size for the results table.
    pub fn input_size(&self) -> String {
        match self {
            CircuitKind::RangeProof { n_bits } => format!("{n_bits} bits"),
            CircuitKind::ConfidentialTransfer {
                num_inputs,
                num_outputs,
                n_bits,
            } => format!("{num_inputs}in x {num_outputs}out @ {n_bits} bits"),
            CircuitKind::SelectiveDisclosure { num_fields } => format!("{num_fields} fields"),
            CircuitKind::WeatherModel { num_samples } => format!("{num_samples} samples"),
        }
    }

    /// Stable identifier, round-trips through [`CircuitKind::from_id`].
    pub fn id(&self) -> String {
        match self {
            CircuitKind::RangeProof { n_bits } => format!("range-proof/{n_bits}"),
            CircuitKind::ConfidentialTransfer {
                num_inputs,
                num_outputs,
                n_bits,
            } => format!("confidential-transfer/{num_inputs}x{num_outputs}x{n_bits}"),
            CircuitKind::SelectiveDisclosure { num_fields } => {
                format!("selective-disclosure/{num_fields}")
            },
            CircuitKind::WeatherModel { num_samples } => format!("weather-model/{num_samples}"),
        }
    }

    /// Parse an id produced by [`CircuitKind::id`].
    pub fn from_id(id: &str) -> Option<Self> {
        let (family, params) = id.split_once('/')?;
        let nums: Option<Vec<usize>> = params.split('x').map(|p| p.parse().ok()).collect();
        let nums = nums?;
        match (family, nums.as_slice()) {
            ("range-proof", [n_bits]) => Some(CircuitKind::RangeProof { n_bits: *n_bits }),
            ("confidential-transfer", [num_inputs, num_outputs, n_bits]) => {
                Some(CircuitKind::ConfidentialTransfer {
                    num_inputs: *num_inputs,
                    num_outputs: *num_outputs,
                    n_bits: *n_bits,
                })
            },
            ("selective-disclosure", [num_fields]) => Some(CircuitKind::SelectiveDisclosure {
                num_fields: *num_fields,
            }),
            ("weather-model", [num_samples]) => Some(CircuitKind::WeatherModel {
                num_samples: *num_samples,
            }),
            _ => None,
        }
    }

    /// Number of Groth16 public inputs, excluding the implicit `1`.
    ///
    /// This is the term that makes verification *not* constant-time in input
    /// size: `prepare_inputs` runs a multi-scalar multiplication of this length
    /// over `vk.gamma_abc_g1`.
    pub fn num_public_inputs(&self) -> usize {
        match self {
            CircuitKind::RangeProof { .. } => 1,
            CircuitKind::ConfidentialTransfer { .. } => 3,
            CircuitKind::SelectiveDisclosure { .. } => 1,
            CircuitKind::WeatherModel { .. } => 4,
        }
    }

    /// R1CS constraint count — the structural size that drives setup and
    /// proving cost. Synthesizes the witnessed circuit, so it also acts as a
    /// cheap satisfiability check on the fixtures below.
    pub fn num_constraints(&self) -> Result<usize, ZkError> {
        let cs = ConstraintSystem::<Fr>::new_ref();
        self.with_witness(|circuit| circuit.generate_constraints(cs.clone()))
            .map_err(|e| ZkError::InvalidInput(format!("constraint synthesis failed: {e}")))?;
        Ok(cs.num_constraints())
    }

    /// Run Groth16 setup for this case.
    ///
    /// Setup uses the *blank* circuit (no witness), which is the only correct
    /// input: a key derived from a witnessed circuit would leak the witness
    /// through the QAP.
    pub fn generate_keys(
        &self,
    ) -> Result<(ProvingKey<Bn254>, PreparedVerifyingKey<Bn254>), ZkError> {
        match *self {
            CircuitKind::RangeProof { n_bits } => {
                proving::generate_keys(RangeProofCircuit::blank(n_bits))
            },
            CircuitKind::ConfidentialTransfer {
                num_inputs,
                num_outputs,
                n_bits,
            } => proving::generate_keys(ConfidentialTransferCircuit::blank(
                num_inputs,
                num_outputs,
                n_bits,
            )),
            CircuitKind::SelectiveDisclosure { num_fields } => {
                proving::generate_keys(SelectiveDisclosureCircuit::blank(num_fields))
            },
            CircuitKind::WeatherModel { num_samples } => {
                proving::generate_keys(WeatherModelCircuit::blank(num_samples))
            },
        }
    }

    /// Produce one proof for this case using the fixed witness.
    pub fn prove(&self, pk: &ProvingKey<Bn254>) -> Result<ZkProof, ZkError> {
        let id = self.id();
        match *self {
            CircuitKind::RangeProof { .. } => {
                let (value, blinding, n_bits) = range_witness(self);
                proving::prove(
                    RangeProofCircuit::new(
                        RangeProofCircuit::compute_commitment(value, blinding),
                        Fr::from(value),
                        blinding,
                        n_bits,
                    ),
                    pk,
                    &id,
                )
            },
            CircuitKind::ConfidentialTransfer { n_bits, .. } => {
                let w = transfer_witness(self);
                proving::prove(
                    ConfidentialTransferCircuit::new(
                        ConfidentialTransferCircuit::compute_commitment(
                            &w.input_values,
                            &w.input_blindings,
                        ),
                        ConfidentialTransferCircuit::compute_commitment(
                            &w.output_values,
                            &w.output_blindings,
                        ),
                        w.fee,
                        w.input_values.clone(),
                        w.input_blindings.clone(),
                        w.output_values.clone(),
                        w.output_blindings.clone(),
                        n_bits,
                    ),
                    pk,
                    &id,
                )
            },
            CircuitKind::SelectiveDisclosure { .. } => {
                let fields = disclosure_witness(self);
                proving::prove(
                    SelectiveDisclosureCircuit::new(
                        SelectiveDisclosureCircuit::compute_commitment(&fields),
                        fields,
                    ),
                    pk,
                    &id,
                )
            },
            CircuitKind::WeatherModel { .. } => {
                let w = weather_witness(self);
                proving::prove(
                    WeatherModelCircuit::new(
                        w.input_hash,
                        w.output_hash,
                        w.model_id,
                        w.execution_time_secs,
                        w.state_samples.clone(),
                    ),
                    pk,
                    &id,
                )
            },
        }
    }

    /// Public inputs matching the fixed witness, in Groth16 allocation order.
    pub fn public_inputs(&self) -> Vec<Fr> {
        match *self {
            CircuitKind::RangeProof { .. } => {
                let (value, blinding, _) = range_witness(self);
                vec![RangeProofCircuit::compute_commitment(value, blinding)]
            },
            CircuitKind::ConfidentialTransfer { .. } => {
                let w = transfer_witness(self);
                vec![
                    ConfidentialTransferCircuit::compute_commitment(
                        &w.input_values,
                        &w.input_blindings,
                    ),
                    ConfidentialTransferCircuit::compute_commitment(
                        &w.output_values,
                        &w.output_blindings,
                    ),
                    w.fee,
                ]
            },
            CircuitKind::SelectiveDisclosure { .. } => {
                let fields = disclosure_witness(self);
                vec![SelectiveDisclosureCircuit::compute_commitment(&fields)]
            },
            CircuitKind::WeatherModel { .. } => {
                let w = weather_witness(self);
                vec![
                    w.input_hash,
                    w.output_hash,
                    w.model_id,
                    w.execution_time_secs,
                ]
            },
        }
    }

    /// Call `f` with the fully witnessed circuit for this case.
    ///
    /// Used by [`CircuitKind::num_constraints`]; the closure is generic over
    /// the concrete circuit type so each family keeps its own `impl`.
    fn with_witness<R>(&self, f: impl FnOnce(WitnessedCircuit) -> R) -> R {
        match *self {
            CircuitKind::RangeProof { .. } => {
                let (value, blinding, n_bits) = range_witness(self);
                f(WitnessedCircuit::Range(RangeProofCircuit::new(
                    RangeProofCircuit::compute_commitment(value, blinding),
                    Fr::from(value),
                    blinding,
                    n_bits,
                )))
            },
            CircuitKind::ConfidentialTransfer { n_bits, .. } => {
                let w = transfer_witness(self);
                f(WitnessedCircuit::Transfer(Box::new(
                    ConfidentialTransferCircuit::new(
                        ConfidentialTransferCircuit::compute_commitment(
                            &w.input_values,
                            &w.input_blindings,
                        ),
                        ConfidentialTransferCircuit::compute_commitment(
                            &w.output_values,
                            &w.output_blindings,
                        ),
                        w.fee,
                        w.input_values,
                        w.input_blindings,
                        w.output_values,
                        w.output_blindings,
                        n_bits,
                    ),
                )))
            },
            CircuitKind::SelectiveDisclosure { .. } => {
                let fields = disclosure_witness(self);
                f(WitnessedCircuit::Disclosure(
                    SelectiveDisclosureCircuit::new(
                        SelectiveDisclosureCircuit::compute_commitment(&fields),
                        fields,
                    ),
                ))
            },
            CircuitKind::WeatherModel { .. } => {
                let w = weather_witness(self);
                f(WitnessedCircuit::Weather(WeatherModelCircuit::new(
                    w.input_hash,
                    w.output_hash,
                    w.model_id,
                    w.execution_time_secs,
                    w.state_samples,
                )))
            },
        }
    }
}

/// Type-erased witnessed circuit, so `num_constraints` can be written once.
enum WitnessedCircuit {
    Range(RangeProofCircuit),
    // Boxed: `ConfidentialTransferCircuit` is much larger than the others and
    // would otherwise set the size of every variant.
    Transfer(Box<ConfidentialTransferCircuit>),
    Disclosure(SelectiveDisclosureCircuit),
    Weather(WeatherModelCircuit),
}

impl ConstraintSynthesizer<Fr> for WitnessedCircuit {
    fn generate_constraints(
        self,
        cs: ark_relations::gr1cs::ConstraintSystemRef<Fr>,
    ) -> Result<(), ark_relations::gr1cs::SynthesisError> {
        match self {
            WitnessedCircuit::Range(c) => c.generate_constraints(cs),
            WitnessedCircuit::Transfer(c) => c.generate_constraints(cs),
            WitnessedCircuit::Disclosure(c) => c.generate_constraints(cs),
            WitnessedCircuit::Weather(c) => c.generate_constraints(cs),
        }
    }
}

// ---------------------------------------------------------------------------
// Fixed witnesses. Deterministic by construction — no RNG, no clock, no map
// iteration. Every helper is total for any matrix parameter.
// ---------------------------------------------------------------------------

/// `(value, blinding, n_bits)` — value saturates the range so the bit
/// decomposition exercises every constraint.
fn range_witness(kind: &CircuitKind) -> (u64, Fr, usize) {
    let n_bits = match kind {
        CircuitKind::RangeProof { n_bits } => *n_bits,
        _ => unreachable!("range_witness called on {kind:?}"),
    };
    let value = if n_bits >= 64 {
        u64::MAX
    } else {
        (1u64 << n_bits) - 1
    };
    (value, Fr::from(0x9e37_79b9_7f4a_7c15_u64), n_bits)
}

struct TransferWitness {
    input_values: Vec<Fr>,
    input_blindings: Vec<Fr>,
    output_values: Vec<Fr>,
    output_blindings: Vec<Fr>,
    fee: Fr,
}

/// Balanced transfer: `sum(inputs) == sum(outputs) + fee` and
/// `sum(input_blindings) == sum(output_blindings)`, with **every** value —
/// inputs and outputs alike — inside the `n_bits` range check.
///
/// The per-note amount is derived from `n_bits` rather than fixed, so the
/// helper stays total as the range width shrinks. A fixed amount silently
/// overflows the range check at small `n_bits`, which surfaces as an opaque
/// `cs.is_satisfied()` assertion deep inside the ark prover.
fn transfer_witness(kind: &CircuitKind) -> TransferWitness {
    let (num_inputs, num_outputs, n_bits) = match kind {
        CircuitKind::ConfidentialTransfer {
            num_inputs,
            num_outputs,
            n_bits,
        } => (*num_inputs, *num_outputs, *n_bits),
        _ => unreachable!("transfer_witness called on {kind:?}"),
    };

    let cap: u64 = if n_bits >= 64 {
        u64::MAX
    } else {
        (1u64 << n_bits) - 1
    };

    // Halving against the wider side of the split guarantees both
    // `sum(inputs) <= cap/2` and every individual output `<= cap/2`, whatever
    // the input/output ratio.
    let widest = num_inputs.max(num_outputs) as u64;
    let unit = (cap / (2 * widest)).max(1);

    // The fee is a single public field element; its magnitude does not change
    // any constraint count. Keeping it at 1 keeps the witness satisfiable all
    // the way down to a 4-bit range.
    const FEE: u64 = 1;

    let input_values: Vec<Fr> = (0..num_inputs).map(|_| Fr::from(unit)).collect();
    let input_blindings: Vec<Fr> = (0..num_inputs)
        .map(|i| Fr::from((i as u64 + 1) * 1_000))
        .collect();

    // Split the remainder as evenly as possible; the last output absorbs the
    // rounding so the balance constraint holds exactly.
    let total_in = unit * num_inputs as u64;
    let total_out = total_in - FEE;
    let per_output = total_out / num_outputs as u64;
    let mut output_values: Vec<Fr> = (0..num_outputs).map(|_| Fr::from(per_output)).collect();
    let remainder = total_out - per_output * num_outputs as u64;
    let last = num_outputs - 1;
    output_values[last] = Fr::from(per_output + remainder);

    // Blindings are conserved but not range-checked, so plain field arithmetic
    // is sufficient here.
    let blinding_sum: Fr = input_blindings.iter().sum();
    let mut output_blindings = vec![blinding_sum - Fr::from(num_outputs as u64 - 1)];
    output_blindings.extend((1..num_outputs).map(|_| Fr::from(1u64)));

    TransferWitness {
        input_values,
        input_blindings,
        output_values,
        output_blindings,
        fee: Fr::from(FEE),
    }
}

fn disclosure_witness(kind: &CircuitKind) -> Vec<Fr> {
    let num_fields = match kind {
        CircuitKind::SelectiveDisclosure { num_fields } => *num_fields,
        _ => unreachable!("disclosure_witness called on {kind:?}"),
    };
    (0..num_fields)
        .map(|i| Fr::from(i as u64 * 7 + 1))
        .collect()
}

struct WeatherWitness {
    input_hash: Fr,
    output_hash: Fr,
    model_id: Fr,
    execution_time_secs: Fr,
    state_samples: Vec<Fr>,
}

fn weather_witness(kind: &CircuitKind) -> WeatherWitness {
    let num_samples = match kind {
        CircuitKind::WeatherModel { num_samples } => *num_samples,
        _ => unreachable!("weather_witness called on {kind:?}"),
    };
    let input_hash = Fr::from(10u64);
    let model_id = Fr::from(7u64);
    // The circuit enforces `execution_time_secs != 0`.
    let execution_time_secs = Fr::from(15u64);
    let state_samples: Vec<Fr> = (0..num_samples).map(|i| Fr::from(i as u64 + 3)).collect();

    // Must mirror the in-circuit absorb order: samples, then model_id, then
    // input_hash.
    let mut preimage = state_samples.clone();
    preimage.push(model_id);
    preimage.push(input_hash);
    let output_hash = poseidon_hash(&preimage);

    WeatherWitness {
        input_hash,
        output_hash,
        model_id,
        execution_time_secs,
        state_samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for kind in MATRIX {
            let id = kind.id();
            assert_eq!(
                CircuitKind::from_id(&id),
                Some(*kind),
                "id {id} must round-trip"
            );
        }
    }

    #[test]
    fn from_id_rejects_garbage() {
        for bad in [
            "",
            "range-proof",
            "range-proof/",
            "range-proof/abc",
            "no-such-circuit/8",
            "confidential-transfer/2x2",
        ] {
            assert_eq!(CircuitKind::from_id(bad), None, "{bad} must not parse");
        }
    }

    #[test]
    fn matrix_ids_are_unique() {
        let mut ids: Vec<String> = MATRIX.iter().map(|k| k.id()).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "matrix contains duplicate cases");
    }

    /// Every fixture must actually satisfy its circuit. `num_constraints`
    /// synthesizes the witnessed circuit, so a broken fixture shows up here
    /// rather than as a confusing proving failure hours into a sweep.
    #[test]
    fn every_matrix_case_synthesizes() {
        for kind in MATRIX {
            let n = kind
                .num_constraints()
                .unwrap_or_else(|e| panic!("{}: {e}", kind.id()));
            assert!(n > 0, "{} produced zero constraints", kind.id());
        }
    }

    /// The witness must be *satisfying*, not merely synthesizable — a circuit
    /// with unsatisfied constraints still reports a constraint count.
    #[test]
    fn every_matrix_case_is_satisfied() {
        for kind in MATRIX {
            let cs = ConstraintSystem::<Fr>::new_ref();
            kind.with_witness(|c| c.generate_constraints(cs.clone()))
                .unwrap_or_else(|e| panic!("{}: synthesis failed: {e}", kind.id()));
            assert!(
                cs.is_satisfied().unwrap_or(false),
                "{} witness does not satisfy its constraints",
                kind.id()
            );
        }
    }

    /// The witness helpers must stay total *outside* the published matrix too
    /// — `budgets.rs` deliberately uses small off-matrix cases to keep the
    /// `--lib` run fast, and other stories in epic #2176 will add their own.
    ///
    /// This sweep exists because a fixed per-note amount in `transfer_witness`
    /// overflowed the range check at small `n_bits`, and the only symptom was
    /// an opaque `cs.is_satisfied()` assertion inside the ark prover.
    #[test]
    fn witness_helpers_are_total_across_the_parameter_grid() {
        let mut cases = Vec::new();
        for n_bits in [4usize, 8, 16, 32, 64] {
            cases.push(CircuitKind::RangeProof { n_bits });
            for num_inputs in [1usize, 2, 3] {
                for num_outputs in [1usize, 2, 3] {
                    cases.push(CircuitKind::ConfidentialTransfer {
                        num_inputs,
                        num_outputs,
                        n_bits,
                    });
                }
            }
        }
        for n in [1usize, 2, 5, 17] {
            cases.push(CircuitKind::SelectiveDisclosure { num_fields: n });
            cases.push(CircuitKind::WeatherModel { num_samples: n });
        }

        for kind in cases {
            let cs = ConstraintSystem::<Fr>::new_ref();
            kind.with_witness(|c| c.generate_constraints(cs.clone()))
                .unwrap_or_else(|e| panic!("{}: synthesis failed: {e}", kind.id()));
            assert!(
                cs.is_satisfied().unwrap_or(false),
                "{} witness does not satisfy its constraints",
                kind.id()
            );
        }
    }

    #[test]
    fn public_input_counts_match_declaration() {
        for kind in MATRIX {
            assert_eq!(
                kind.public_inputs().len(),
                kind.num_public_inputs(),
                "{} public input count mismatch",
                kind.id()
            );
        }
    }
}
