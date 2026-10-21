//! Validator Module for Consensus System
//!
//! This module defines validator-related types and functionality for the consensus system.

use serde::{Deserialize, Serialize};
use rustc_hash::FxHashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// Slashing penalties in basis points (1 bp = 0.01%). Kept in integer math
// to preserve the workspace currency invariant (`WRF_UNITS_PER_COIN = 10^8`,
// all currency is u64 base units, not f64 — PR #505). See #1401 for the
// consensus-fork risk that f64 penalty math carried before this fix.
// pub(crate) so the losslessness oracle in `economics::proofs` can recompute the exact
// penalty from the spec rate through an INDEPENDENT arithmetic path and assert equality,
// rather than resting on the one-sided upper bound that a truncated or zero result walked
// through (#2253 review).
pub(crate) const PENALTY_BPS_MINOR: u64 = 100; // 100 basis points
pub(crate) const PENALTY_BPS_MAJOR: u64 = 500; // 500 basis points
pub(crate) const PENALTY_BPS_CRITICAL: u64 = 1_000; // 1,000 basis points
/// Basis-point denominator, matching `FEE_DENOMINATOR_BPS` in the AMM fee model.
pub(crate) const BPS_DENOMINATOR: u64 = 10_000;
/// Largest penalty rate any severity can select — the bound the non-inflation
/// proof in `economics::proofs` relies on.
pub(crate) const MAX_PENALTY_BPS: u64 = PENALTY_BPS_CRITICAL;

// The invariant the slash proofs rest on, machine-checked rather than asserted in prose:
// every severity's rate is <= MAX_PENALTY_BPS, and that ceiling is strictly below the
// denominator, so `stake * bps / DENOM < stake` and the `u128 -> u64` narrowing cannot
// truncate. If a future rate breaks this, the build fails here rather than a proof
// silently going vacuous.
const _: () = {
    assert!(PENALTY_BPS_MINOR <= MAX_PENALTY_BPS);
    assert!(PENALTY_BPS_MAJOR <= MAX_PENALTY_BPS);
    assert!(PENALTY_BPS_CRITICAL <= MAX_PENALTY_BPS);
    assert!(MAX_PENALTY_BPS < BPS_DENOMINATOR);
};

/// Compute the slashing penalty for a given stake and severity using integer
/// bps math. Uses a u128 intermediate so stakes approaching `u64::MAX` cannot
/// overflow the multiplication — that class of silent-truncation bug is exactly
/// what makes f64 penalty math dangerous.
///
/// **Domain note.** This function's API domain is the full `u64`, deliberately:
/// it is generic integer base-unit arithmetic and is not restricted to WRF. The
/// WRF *protocol* domain is much smaller — total supply is
/// `21_000_000 * 10^8 = 2_100_000_000_000_000` base units, which fits in 51 bits
/// (`shared_protocol::consensus::proof_of_publishing::MAX_SUPPLY`). Keep the two
/// apart: a `u64::MAX` stake is unreachable in WRF but is a legitimate input to
/// this helper for any token with higher precision or a larger cap, which is why
/// the pathological-stake test below pins `u64::MAX - 1`.
///
/// For scale, `u64::MAX / 10^8` is ~184.47 **billion** WRF. An earlier revision
/// of this comment said ~18.4M WRF — wrong by four orders of magnitude, and the
/// error had propagated into #2108's proof documentation as a justification for
/// withdrawing two harnesses. Corrected per the #2253 review.
///
/// The result is bit-exact across architectures: no floating-point
/// rounding-mode divergence, no x87 extended-precision vs SSE2 drift, no
/// `as u64` rounding surprises. Matches the AMM fee-model convention
/// (`FEE_DENOMINATOR_BPS = 10_000`) already used by `CarbonCreditAMM` and
/// `AMMRouter`.
///
/// Refs #1401, #1393.
pub(crate) fn penalty_amount_bps(stake: u64, severity: &SlashingSeverity) -> u64 {
    let penalty_bps: u64 = match severity {
        SlashingSeverity::Minor => PENALTY_BPS_MINOR,
        SlashingSeverity::Major => PENALTY_BPS_MAJOR,
        SlashingSeverity::Critical => PENALTY_BPS_CRITICAL,
    };
    // The precondition that makes the `as u64` narrowing below lossless: with
    // `penalty_bps <= MAX_PENALTY_BPS < BPS_DENOMINATOR`, the quotient is `< stake`, so it
    // always fits u64. Checked here (debug builds) at the point the cast happens, not only
    // in prose.
    debug_assert!(penalty_bps <= MAX_PENALTY_BPS);
    // u128 intermediate prevents overflow near u64::MAX.
    ((stake as u128).saturating_mul(penalty_bps as u128) / BPS_DENOMINATOR as u128) as u64
}

/// Validator status enumeration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidatorStatus {
    /// Active validator
    Active,
    /// Jailed validator (temporarily disabled)
    Jailed,
    /// Inactive validator
    Inactive,
    /// Slashed validator
    Slashed,
}

/// Types of violations that can occur
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViolationType {
    /// Double signing violation
    DoubleSign,
    /// Consensus failure
    ConsensusFailure,
    /// Invalid block production
    InvalidBlock,
    /// Network misbehavior
    NetworkMisbehavior,
    /// Extended downtime
    ExtendedDowntime,
    /// Protocol violation
    ProtocolViolation,
}

/// Severity levels for slashing
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashingSeverity {
    /// Minor violation
    Minor,
    /// Major violation
    Major,
    /// Critical violation
    Critical,
}

/// Performance metrics for validators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Validation accuracy
    pub validation_accuracy: f64,
    /// Uptime percentage
    pub uptime: f64,
    /// Number of successful validations
    pub successful_validations: u64,
    /// Number of failed validations
    pub failed_validations: u64,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            validation_accuracy: 1.0,
            uptime: 1.0,
            successful_validations: 0,
            failed_validations: 0,
        }
    }
}

/// Slashing event record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingEvent {
    /// Type of violation
    pub violation_type: ViolationType,
    /// Severity of the violation
    pub severity: SlashingSeverity,
    /// Penalty amount
    pub penalty_amount: u64,
    /// Timestamp when the event occurred
    pub timestamp: u64,
    /// Evidence or description
    pub evidence: String,
}

/// Basic validator information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Validator {
    /// Validator identifier
    pub id: String,
    /// Public key for validation
    pub public_key: String,
    /// Current stake amount
    pub stake: u64,
    /// Validator status
    pub status: ValidatorStatus,
    /// Performance metrics
    pub performance_metrics: PerformanceMetrics,
    /// Slashing history
    pub slashing_history: Vec<SlashingEvent>,
    /// Join timestamp
    pub joined_at: u64,
    /// Last active timestamp
    pub last_active: u64,
}

impl Validator {
    /// Create a new validator
    pub fn new(id: String, public_key: String, stake: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            id,
            public_key,
            stake,
            status: ValidatorStatus::Active,
            performance_metrics: PerformanceMetrics::default(),
            slashing_history: Vec::new(),
            joined_at: now,
            last_active: now,
        }
    }
}

/// Consensus parameters for validators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusParams {
    /// Minimum stake required to be a validator
    pub min_stake: u64,
    /// Maximum number of validators
    pub max_validators: u32,
    /// Slashing rate for violations
    pub slashing_rate: f64,
}

impl Default for ConsensusParams {
    fn default() -> Self {
        Self {
            min_stake: 1000,
            max_validators: 100,
            slashing_rate: 0.05,
        }
    }
}

/// Validator management system
#[derive(Debug)]
pub struct ValidatorManager {
    /// Registered validators
    validators: FxHashMap<String, Validator>,
    /// Consensus parameters
    params: ConsensusParams,
}

impl ValidatorManager {
    /// Create a new validator manager
    pub fn new(params: ConsensusParams) -> Self {
        Self {
            validators: FxHashMap::default(),
            params,
        }
    }

    /// Register a new validator
    pub fn register_validator(&mut self, validator: Validator) -> Result<(), String> {
        if validator.stake < self.params.min_stake {
            return Err("Insufficient stake".to_string());
        }

        if self.validators.len() >= self.params.max_validators as usize {
            return Err("Maximum validators reached".to_string());
        }

        self.validators.insert(validator.id.clone(), validator);
        Ok(())
    }

    /// Get a validator by ID
    pub fn get_validator(&self, id: &str) -> Option<&Validator> {
        self.validators.get(id)
    }

    /// Get a mutable reference to a validator
    pub fn get_validator_mut(&mut self, id: &str) -> Option<&mut Validator> {
        self.validators.get_mut(id)
    }

    /// Remove a validator
    pub fn remove_validator(&mut self, id: &str) -> bool {
        self.validators.remove(id).is_some()
    }

    /// Get all active validators
    pub fn get_active_validators(&self) -> Vec<&Validator> {
        self.validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Active)
            .collect()
    }

    /// Apply slashing to a validator
    pub fn slash_validator(
        &mut self,
        id: &str,
        violation: ViolationType,
        severity: SlashingSeverity,
        evidence: String,
    ) -> Result<(), String> {
        let validator = self.get_validator_mut(id).ok_or("Validator not found")?;

        // #1401: integer bps math (see `penalty_amount_bps`). f64 arithmetic
        // on u64 stake here would be a latent consensus-fork bug the moment
        // this function becomes reachable from a state transition (see #1393
        // for the tx-wiring gap that keeps it dead for now).
        let penalty_amount = penalty_amount_bps(validator.stake, &severity);
        validator.stake = validator.stake.saturating_sub(penalty_amount);

        // Jail validator for major/critical violations
        let should_jail = matches!(
            severity,
            SlashingSeverity::Major | SlashingSeverity::Critical
        );

        let slashing_event = SlashingEvent {
            violation_type: violation,
            severity,
            penalty_amount,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            evidence,
        };

        validator.slashing_history.push(slashing_event);

        if should_jail {
            validator.status = ValidatorStatus::Jailed;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn small_params() -> ConsensusParams {
        ConsensusParams {
            min_stake: 1_000,
            max_validators: 5,
            slashing_rate: 0.05,
        }
    }

    fn make_validator(id: &str, stake: u64) -> Validator {
        Validator::new(id.to_string(), format!("pubkey-{id}"), stake)
    }

    /// Happy path: validator with sufficient stake is registered.
    #[test]
    fn test_register_validator_ok() {
        let mut mgr = ValidatorManager::new(small_params());
        assert!(mgr.register_validator(make_validator("v1", 2_000)).is_ok());
        assert!(mgr.get_validator("v1").is_some());
        assert_eq!(mgr.get_active_validators().len(), 1);
    }

    /// Edge case: validator with stake below minimum is rejected.
    #[test]
    fn test_register_validator_insufficient_stake() {
        let mut mgr = ValidatorManager::new(small_params());
        let err = mgr
            .register_validator(make_validator("v1", 500)) // below min_stake 1000
            .unwrap_err();
        assert_eq!(err, "Insufficient stake");
    }

    /// Error path: registering beyond max_validators is rejected.
    #[test]
    fn test_register_validator_max_capacity() {
        let mut mgr = ValidatorManager::new(ConsensusParams {
            min_stake: 100,
            max_validators: 2,
            slashing_rate: 0.05,
        });
        mgr.register_validator(make_validator("v1", 1_000)).unwrap();
        mgr.register_validator(make_validator("v2", 1_000)).unwrap();

        let err = mgr
            .register_validator(make_validator("v3", 1_000))
            .unwrap_err();
        assert_eq!(err, "Maximum validators reached");
    }

    /// Happy path: minor slash reduces stake; validator stays Active.
    #[test]
    fn test_slash_minor_no_jail() {
        let mut mgr = ValidatorManager::new(small_params());
        mgr.register_validator(make_validator("v1", 10_000)).unwrap();

        mgr.slash_validator(
            "v1",
            ViolationType::NetworkMisbehavior,
            SlashingSeverity::Minor,
            "evidence".to_string(),
        )
        .unwrap();

        let v = mgr.get_validator("v1").unwrap();
        assert_eq!(v.stake, 9_900); // 1% of 10_000 = 100 deducted
        assert_eq!(v.status, ValidatorStatus::Active);
        assert_eq!(v.slashing_history.len(), 1);
    }

    /// Error path: major slash jails the validator.
    #[test]
    fn test_slash_major_jails() {
        let mut mgr = ValidatorManager::new(small_params());
        mgr.register_validator(make_validator("v1", 10_000)).unwrap();

        mgr.slash_validator(
            "v1",
            ViolationType::DoubleSign,
            SlashingSeverity::Major,
            "double-sign".to_string(),
        )
        .unwrap();

        let v = mgr.get_validator("v1").unwrap();
        assert_eq!(v.stake, 9_500); // 5% of 10_000 = 500 deducted
        assert_eq!(v.status, ValidatorStatus::Jailed);
    }

    /// Critical slash jails with 10% penalty.
    #[test]
    fn test_slash_critical_jails_with_max_penalty() {
        let mut mgr = ValidatorManager::new(small_params());
        mgr.register_validator(make_validator("v1", 10_000)).unwrap();

        mgr.slash_validator(
            "v1",
            ViolationType::ProtocolViolation,
            SlashingSeverity::Critical,
            "crit".to_string(),
        )
        .unwrap();

        let v = mgr.get_validator("v1").unwrap();
        assert_eq!(v.stake, 9_000); // 10% of 10_000 = 1_000 deducted
        assert_eq!(v.status, ValidatorStatus::Jailed);
    }

    /// get_active_validators excludes jailed validators.
    #[test]
    fn test_get_active_validators_excludes_jailed() {
        let mut mgr = ValidatorManager::new(ConsensusParams {
            min_stake: 100,
            max_validators: 10,
            slashing_rate: 0.05,
        });
        mgr.register_validator(make_validator("v1", 1_000)).unwrap();
        mgr.register_validator(make_validator("v2", 1_000)).unwrap();
        mgr.register_validator(make_validator("v3", 1_000)).unwrap();

        // Jail v2 via major slash
        mgr.slash_validator("v2", ViolationType::DoubleSign, SlashingSeverity::Major, "".to_string())
            .unwrap();

        let active = mgr.get_active_validators();
        assert_eq!(active.len(), 2);
        assert!(active.iter().all(|v| v.id != "v2"));
    }

    /// remove_validator is idempotent — second removal returns false.
    #[test]
    fn test_remove_validator_idempotent() {
        let mut mgr = ValidatorManager::new(small_params());
        mgr.register_validator(make_validator("v1", 2_000)).unwrap();

        assert!(mgr.remove_validator("v1"));
        assert!(mgr.get_validator("v1").is_none());
        assert!(!mgr.remove_validator("v1")); // already gone
    }

    // ── #1401: penalty math is bit-exact integer arithmetic ────────────────
    //
    // The old f64 penalty path lost precision well before the u64 ceiling and
    // was architecture-dependent. These tests pin the new integer bps math
    // to concrete values so a future refactor back to f64 fails loudly.

    #[test]
    fn penalty_amount_bps_matches_documented_rates() {
        // Canonical 10_000-stake case — same values the happy-path tests
        // above already assume, just computed through the new function.
        assert_eq!(penalty_amount_bps(10_000, &SlashingSeverity::Minor), 100);
        assert_eq!(penalty_amount_bps(10_000, &SlashingSeverity::Major), 500);
        assert_eq!(penalty_amount_bps(10_000, &SlashingSeverity::Critical), 1_000);
    }

    #[test]
    fn penalty_amount_bps_is_bit_exact_at_pathological_stake() {
        // A stake near u64::MAX would be where f64 penalty math silently
        // truncated the low bits, differently across x86_64 vs ARM64 in
        // principle. The integer path is bit-exact — pin the exact expected
        // value so a regression back to f64 is a test failure, not a
        // consensus-fork incident.
        let stake = u64::MAX - 1; // 18_446_744_073_709_551_614

        // 1% of (u64::MAX - 1):
        //   (u64::MAX - 1) * 100 / 10_000 = (u64::MAX - 1) / 100
        //   = 184_467_440_737_095_516 (integer division, no rounding drift)
        let expected_minor: u64 = (stake as u128 * 100 / 10_000) as u64;
        assert_eq!(penalty_amount_bps(stake, &SlashingSeverity::Minor), expected_minor);
        assert_eq!(expected_minor, 184_467_440_737_095_516);

        // 10% of (u64::MAX - 1):
        let expected_crit: u64 = (stake as u128 * 1_000 / 10_000) as u64;
        assert_eq!(penalty_amount_bps(stake, &SlashingSeverity::Critical), expected_crit);
        assert_eq!(expected_crit, 1_844_674_407_370_955_161);
    }

    #[test]
    fn penalty_amount_bps_u128_intermediate_prevents_overflow_at_u64_max() {
        // The u64 multiplication `u64::MAX * 1_000` would overflow; u128
        // intermediate makes it safe. Without the u128 lift this test would
        // panic in debug and silently wrap in release.
        let stake = u64::MAX;
        let result = penalty_amount_bps(stake, &SlashingSeverity::Critical);
        // (u64::MAX as u128) * 1000 / 10000 fits in u64 because the
        // ratio 1000/10000 = 1/10 scales u64::MAX down to ~1.8e18.
        assert_eq!(result, (u64::MAX as u128 * 1_000 / 10_000) as u64);
    }

    #[test]
    fn penalty_amount_bps_zero_stake_yields_zero_penalty() {
        // Defensive: slashing a zero-stake validator (e.g. after a previous
        // total slash) must not underflow or produce nonsense.
        assert_eq!(penalty_amount_bps(0, &SlashingSeverity::Minor), 0);
        assert_eq!(penalty_amount_bps(0, &SlashingSeverity::Major), 0);
        assert_eq!(penalty_amount_bps(0, &SlashingSeverity::Critical), 0);
    }
}
