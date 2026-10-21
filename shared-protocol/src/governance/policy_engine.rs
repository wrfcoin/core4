//! Deterministic policy compiler and interpreter.
//!
//! Compiles a `SanctionMatrix` into a `CompiledPolicy` that can:
//! - Evaluate any (OffenseCode, SeverityTier, ActorClass) -> SanctionOutcome
//! - Produce a stable blake3 content hash for on-chain anchoring
//! - Validate matrix completeness and sanction range invariants

use super::actor_policy::{ActorClassPolicy, ActorClassPolicyError};
use super::sanctions::{ActorClass, OffenseCode, SanctionMatrix, SanctionOutcome, SeverityTier};
use serde::{Deserialize, Serialize};

/// A compiled, deterministic policy ready for evaluation and on-chain anchoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledPolicy {
    matrix: SanctionMatrix,
    #[serde(default)]
    actor_policy: ActorClassPolicy,
    version: u32,
    compiled_at_block: Option<u64>,
}

impl CompiledPolicy {
    /// Evaluate a specific case. Returns the sanction outcome.
    /// Actor class is accepted for audit trail but does not change the outcome.
    pub fn evaluate(
        &self,
        offense: OffenseCode,
        severity: SeverityTier,
        actor_class: ActorClass,
    ) -> Option<&SanctionOutcome> {
        self.matrix.lookup(offense, severity, actor_class)
    }

    /// Evaluate a specific case from registered actor identity.
    /// Unknown or not-yet-attested actors default to Human class for audit traceability.
    pub fn evaluate_for_actor(
        &self,
        offense: OffenseCode,
        severity: SeverityTier,
        actor_id: &str,
    ) -> Option<&SanctionOutcome> {
        self.actor_policy
            .evaluate_sanction(&self.matrix, actor_id, offense, severity)
    }

    /// BLAKE3 content hash of the compiled policy.
    /// Delegates to the inner SanctionMatrix's content_hash.
    pub fn content_hash(&self) -> [u8; 32] {
        self.matrix.content_hash()
    }

    /// Hex-encoded content hash for on-chain anchoring.
    pub fn content_hash_hex(&self) -> String {
        self.matrix.content_hash_hex()
    }

    /// Policy version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Block height at which this policy was compiled (if known).
    pub fn compiled_at_block(&self) -> Option<u64> {
        self.compiled_at_block
    }

    /// Actor-class policy view used to classify actors for sanctions audit.
    pub fn actor_policy(&self) -> &ActorClassPolicy {
        &self.actor_policy
    }
}

/// Compiles a SanctionMatrix into a CompiledPolicy after validation.
pub struct PolicyCompiler;

impl PolicyCompiler {
    /// Compile from a SanctionMatrix. Validates:
    /// - All (OffenseCode, SeverityTier) pairs are populated (16x4 = 64)
    /// - All slash_bps_min <= slash_bps_max <= 10000
    pub fn compile(
        matrix: SanctionMatrix,
        block: Option<u64>,
    ) -> Result<CompiledPolicy, PolicyCompileError> {
        Self::compile_with_actor_policy(matrix, ActorClassPolicy::default(), block)
    }

    /// Compile from a matrix and actor policy model.
    pub fn compile_with_actor_policy(
        matrix: SanctionMatrix,
        actor_policy: ActorClassPolicy,
        block: Option<u64>,
    ) -> Result<CompiledPolicy, PolicyCompileError> {
        // Validate completeness and range invariants.
        for offense in OffenseCode::all() {
            for severity in SeverityTier::all() {
                match matrix.lookup(*offense, *severity, ActorClass::Human) {
                    None => {
                        return Err(PolicyCompileError::IncompleteMatrix {
                            offense: *offense,
                            severity: *severity,
                        });
                    },
                    Some(outcome) => {
                        if outcome.slash_bps_min > outcome.slash_bps_max {
                            return Err(PolicyCompileError::InvalidSlashRange {
                                offense: *offense,
                                severity: *severity,
                                min: outcome.slash_bps_min,
                                max: outcome.slash_bps_max,
                            });
                        }
                        if outcome.slash_bps_max > 10_000 {
                            return Err(PolicyCompileError::InvalidSlashRange {
                                offense: *offense,
                                severity: *severity,
                                min: outcome.slash_bps_min,
                                max: outcome.slash_bps_max,
                            });
                        }
                    },
                }
            }
        }
        actor_policy
            .assert_equal_liability(&matrix)
            .map_err(PolicyCompileError::ActorPolicy)?;

        let version = matrix.version();
        Ok(CompiledPolicy {
            matrix,
            actor_policy,
            version,
            compiled_at_block: block,
        })
    }
}

/// Errors that can occur during policy compilation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PolicyCompileError {
    #[error("SP_POLICY_INCOMPLETE_MATRIX: missing entry for ({offense:?}, {severity:?})")]
    IncompleteMatrix {
        offense: OffenseCode,
        severity: SeverityTier,
    },
    #[error(
        "SP_POLICY_INVALID_SLASH_RANGE: for ({offense:?}, {severity:?}), \
         min ({min}) > max ({max}) or max > 10000"
    )]
    InvalidSlashRange {
        offense: OffenseCode,
        severity: SeverityTier,
        min: u16,
        max: u16,
    },
    #[error("SP_POLICY_PARSE_ERROR: {0}")]
    ParseError(String),
    #[error("SP_POLICY_ACTOR_POLICY_ERROR: {0}")]
    ActorPolicy(ActorClassPolicyError),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::governance::actor_policy::ActorClassPolicyTransition;

    #[test]
    fn test_compile_v1_succeeds() {
        let matrix = SanctionMatrix::v1();
        let policy = PolicyCompiler::compile(matrix, None);
        assert!(policy.is_ok(), "v1 matrix should compile without error");

        let policy = policy.unwrap();
        assert_eq!(policy.version(), 1);
        assert_eq!(policy.compiled_at_block(), None);
    }

    #[test]
    fn test_compiled_hash_is_deterministic() {
        let policy_a = PolicyCompiler::compile(SanctionMatrix::v1(), Some(42)).expect("compile a");
        let policy_b = PolicyCompiler::compile(SanctionMatrix::v1(), Some(99)).expect("compile b");

        assert_eq!(
            policy_a.content_hash(),
            policy_b.content_hash(),
            "Content hash must be deterministic regardless of block height"
        );
        assert_eq!(
            policy_a.content_hash_hex(),
            policy_b.content_hash_hex(),
            "Hex-encoded hashes must also match"
        );
    }

    #[test]
    fn test_evaluate_matches_direct_lookup() {
        let matrix = SanctionMatrix::v1();
        let policy = PolicyCompiler::compile(matrix.clone(), None).expect("compile");

        for offense in OffenseCode::all() {
            for severity in SeverityTier::all() {
                for actor_class in ActorClass::all() {
                    let direct = matrix.lookup(*offense, *severity, *actor_class);
                    let compiled = policy.evaluate(*offense, *severity, *actor_class);
                    assert_eq!(
                        direct, compiled,
                        "CompiledPolicy.evaluate must match SanctionMatrix.lookup \
                         for ({:?}, {:?}, {:?})",
                        offense, severity, actor_class,
                    );
                }
            }
        }
    }

    #[test]
    fn test_evaluate_for_actor_uses_actor_policy_registry() {
        let matrix = SanctionMatrix::v1();
        let mut actor_policy = ActorClassPolicy::default();
        actor_policy
            .apply_transition(
                ActorClassPolicyTransition::Register {
                    actor_id: "agent-42".to_string(),
                    actor_class: ActorClass::AiAgent,
                    evidence_hash: None,
                },
                100,
            )
            .expect("register");
        actor_policy
            .apply_transition(
                ActorClassPolicyTransition::Attest {
                    actor_id: "agent-42".to_string(),
                    attester_id: "auditor-a".to_string(),
                    evidence_hash: None,
                },
                101,
            )
            .expect("attest");

        let policy = PolicyCompiler::compile_with_actor_policy(matrix.clone(), actor_policy, None)
            .expect("compile with actor policy");

        let via_actor = policy
            .evaluate_for_actor(
                OffenseCode::DataFabrication,
                SeverityTier::Critical,
                "agent-42",
            )
            .expect("actor evaluation should resolve");
        let direct = matrix
            .lookup(
                OffenseCode::DataFabrication,
                SeverityTier::Critical,
                ActorClass::AiAgent,
            )
            .expect("direct lookup should resolve");
        assert_eq!(via_actor, direct);
    }

    #[test]
    fn test_reject_incomplete_matrix() {
        let mut matrix = SanctionMatrix::v1();
        matrix.remove_entry(OffenseCode::DataFabrication, SeverityTier::Critical);

        let result = PolicyCompiler::compile(matrix, None);
        assert!(result.is_err(), "Incomplete matrix should be rejected");

        let err = result.unwrap_err();
        assert_eq!(
            err,
            PolicyCompileError::IncompleteMatrix {
                offense: OffenseCode::DataFabrication,
                severity: SeverityTier::Critical,
            }
        );
        assert!(err.to_string().contains("SP_POLICY_INCOMPLETE_MATRIX"));
    }

    #[test]
    fn test_reject_invalid_slash_range() {
        let mut matrix = SanctionMatrix::v1();
        // Set min > max to trigger validation failure.
        matrix.mutate_entry(
            OffenseCode::ConsensusDoubleSign,
            SeverityTier::Minor,
            |outcome| {
                outcome.slash_bps_min = 500;
                outcome.slash_bps_max = 100;
            },
        );

        let result = PolicyCompiler::compile(matrix, None);
        assert!(result.is_err(), "Invalid slash range should be rejected");

        let err = result.unwrap_err();
        assert_eq!(
            err,
            PolicyCompileError::InvalidSlashRange {
                offense: OffenseCode::ConsensusDoubleSign,
                severity: SeverityTier::Minor,
                min: 500,
                max: 100,
            }
        );
        assert!(err.to_string().contains("SP_POLICY_INVALID_SLASH_RANGE"));
    }
}
