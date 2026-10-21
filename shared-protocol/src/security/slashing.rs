use crate::blockchain::transaction::{RevealPayload, Transaction};
use crate::oracle::attestation::SlashingTrigger;
use crate::weather::validation::{DataQualityPolicy, DataQualityScore};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SlashingReason {
    InvalidReveal,
    OracleAttestationFraud,
    RepeatedLowQualityData,
    InvalidZKProof,
    DataQualityViolation,
}

impl SlashingReason {
    /// Stable snake_case label used as a Prometheus metric label value.
    pub fn as_str(&self) -> &'static str {
        match self {
            SlashingReason::InvalidReveal => "invalid_reveal",
            SlashingReason::OracleAttestationFraud => "oracle_attestation_fraud",
            SlashingReason::RepeatedLowQualityData => "repeated_low_quality_data",
            SlashingReason::InvalidZKProof => "invalid_zk_proof",
            SlashingReason::DataQualityViolation => "data_quality_violation",
        }
    }

    /// All known reason variants, for pre-initializing metric label combinations.
    pub const ALL: &'static [SlashingReason] = &[
        SlashingReason::InvalidReveal,
        SlashingReason::OracleAttestationFraud,
        SlashingReason::RepeatedLowQualityData,
        SlashingReason::InvalidZKProof,
        SlashingReason::DataQualityViolation,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingEvent {
    pub address_or_oracle_id: String,
    pub amount: u128,
    pub reason: SlashingReason,
    pub details: String,
}

/// Category of on-chain slashing evidence, as carried by a [`SlashingReportPayload`].
///
/// TODO(#1393): extend as additional durable-evidence types graduate from
/// detector-only to on-chain-applied.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SlashingEvidenceType {
    /// Validator signed two different blocks at the same height.
    DoubleSign,
    /// Validator submitted an invalid ZK proof that was later detected.
    InvalidZkProof,
    /// Repeated low-quality weather observations above the policy threshold.
    RepeatedLowQualityData,
}

/// Wire-format payload for an on-chain `SlashingReport` transaction.
///
/// This struct is the bridge between the durable evidence store
/// (`native-chain/src/consensus/evidence_store.rs`, wired by #1218) and the
/// state-machine apply path. The evidence submitter task (added in #1393)
/// reads persisted evidence, wraps each item as a `SlashingReportPayload`,
/// **signs it with the reporter's key**, and pushes it into the mempool as
/// a normal transaction. The apply handler in `blockchain::state::State`:
///
///   1. verifies `reporter_signature` against
///      [`SlashingReportPayload::canonical_signing_message`] using
///      `reporter_pubkey`;
///   2. derives the reporter address from `reporter_pubkey` and requires it
///      to have `stake_locked > 0` (i.e. the reporter is an active staker —
///      unauthenticated accounts cannot weaponise slashing);
///   3. **recomputes** the canonical penalty from
///      [`expected_penalty_for_evidence_type`] and rejects any report whose
///      `penalty_amount` does not match;
///   4. checks `evidence_hash` against the replay-protection set;
///   5. dispatches through the existing `StateTransition::Slash` variant.
///
/// Rationale (#1393 round-4 P0): without steps 1–3 the apply path was an
/// unauthenticated stake-burn primitive — any account could mint a report
/// with a fresh `evidence_hash` and burn arbitrary stake from a target.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SlashingReportPayload {
    /// Identifier (address / pubkey hash) of the validator being slashed.
    pub validator_id: String,
    /// Category of evidence (double-sign, invalid ZK proof, etc.).
    pub evidence_type: SlashingEvidenceType,
    /// Deterministic content hash of the underlying evidence record,
    /// used by `State::slashing_evidence_seen` to prevent replay.
    pub evidence_hash: [u8; 32],
    /// Height at which the violation was observed (informational and for
    /// ordering; not used in the penalty math).
    pub reported_at_height: u64,
    /// Penalty amount in base units (WRF). Computed by the submitter
    /// using [`scaled_penalty`]. The apply handler recomputes this from
    /// `evidence_type` via [`expected_penalty_for_evidence_type`] and
    /// **rejects** reports whose `penalty_amount` differs, so attacker-
    /// supplied values cannot drain extra stake.
    pub penalty_amount: u128,
    /// Reporter's public key (hex-encoded ed25519). Used by the apply
    /// handler to (a) verify `reporter_signature` and (b) derive the
    /// reporter address for the staker-check.
    pub reporter_pubkey: String,
    /// Hex-encoded ed25519 signature over
    /// [`SlashingReportPayload::canonical_signing_message`]. `None` on
    /// the wire means "unsigned" — the apply handler rejects those with
    /// `SP_SLASHING_REPORT_UNAUTHENTICATED`. Skipped during
    /// canonical-message hashing so a payload's signing message is
    /// stable across serialise/sign/serialise round-trips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reporter_signature: Option<String>,
}

impl SlashingReportPayload {
    /// Deterministic message bound by the reporter's signature.
    ///
    /// Covers every field that influences the apply decision *except*
    /// `reporter_signature` itself (which must not be part of its own
    /// input). Uses the domain-separated canonical signing envelope so
    /// a signature on a slashing report cannot be replayed as, e.g., a
    /// governance vote or a bridge message (#1393 round-4 P0).
    pub fn canonical_signing_message(&self) -> String {
        use crate::crypto::domain_signing::{
            DomainSigningAction, DomainSigningContext, canonical_signing_message,
            payload_digest_hex, signing_chain_id, signing_network_id,
        };
        // Digest over the same bytes everyone else sees, minus the
        // mutable signature field.
        let canonical = SlashingReportCanonical {
            validator_id: &self.validator_id,
            evidence_type: self.evidence_type,
            evidence_hash: &self.evidence_hash,
            reported_at_height: self.reported_at_height,
            penalty_amount: self.penalty_amount,
            reporter_pubkey: &self.reporter_pubkey,
        };
        let digest = payload_digest_hex(&canonical);
        let cid = signing_chain_id();
        let nid = signing_network_id();
        let context = DomainSigningContext {
            action: DomainSigningAction::SlashingReport,
            signer_id: &self.reporter_pubkey,
            timestamp: self.reported_at_height as i64,
            nonce: None,
            chain_id: &cid,
            network_id: &nid,
        };
        canonical_signing_message(&context, &digest)
    }

    /// Verify [`Self::reporter_signature`] against
    /// [`Self::canonical_signing_message`] using
    /// [`Self::reporter_pubkey`]. Returns `false` for any missing /
    /// malformed input. Callers that care about *why* verification
    /// failed should compose this with explicit field checks.
    pub fn verify_reporter_signature(&self) -> bool {
        let Some(sig_hex) = self.reporter_signature.as_deref() else {
            return false;
        };
        let Ok(public_key) = crate::crypto::signature::PublicKey::from_hex(&self.reporter_pubkey)
        else {
            return false;
        };
        let Ok(signature) = crate::crypto::signature::Signature::from_hex(sig_hex) else {
            return false;
        };
        let message = self.canonical_signing_message();
        public_key.verify(message.as_bytes(), &signature)
    }
}

/// Internal view over the signature-covered subset of a
/// [`SlashingReportPayload`]. Kept private so the canonical message
/// shape cannot drift from what `verify_reporter_signature` hashes.
#[derive(Serialize)]
struct SlashingReportCanonical<'a> {
    validator_id: &'a str,
    evidence_type: SlashingEvidenceType,
    evidence_hash: &'a [u8; 32],
    reported_at_height: u64,
    penalty_amount: u128,
    reporter_pubkey: &'a str,
}

/// Canonical base penalty for a given evidence category. The apply
/// handler re-derives this and compares against the wire-format
/// `penalty_amount`, rejecting any mismatch (#1393 round-4 P0).
pub fn base_penalty_for_evidence_type(evidence_type: SlashingEvidenceType) -> u128 {
    match evidence_type {
        SlashingEvidenceType::DoubleSign => BASE_PENALTY_DOUBLE_ATTESTATION,
        SlashingEvidenceType::InvalidZkProof => BASE_PENALTY_INVALID_ZK_PROOF,
        SlashingEvidenceType::RepeatedLowQualityData => BASE_PENALTY_DATA_QUALITY_VIOLATION,
    }
}

/// Oracle-count input used by the on-chain re-derivation of slashing
/// penalties. Must match the value the submitter passes to
/// [`scaled_penalty`] (currently `1`, a conservative lower bound). If
/// either side changes, every in-flight report's penalty re-derivation
/// will mismatch and the apply handler will reject with
/// `SP_SLASHING_REPORT_PENALTY_MISMATCH`, so the two constants are pinned
/// together.
pub const SLASHING_PENALTY_ORACLE_COUNT: usize = 1;

/// Canonical expected penalty for a given evidence category. Both the
/// submitter (off-chain) and the apply handler (on-chain) compute the
/// wire penalty through this helper, so a mismatch at the apply site
/// is unambiguous evidence of tampering.
pub fn expected_penalty_for_evidence_type(evidence_type: SlashingEvidenceType) -> u128 {
    scaled_penalty(
        base_penalty_for_evidence_type(evidence_type),
        SLASHING_PENALTY_ORACLE_COUNT,
    )
}

/// Maximum number of slashing events applied in a single block.
/// Prevents mass-slashing during correlated weather events (hurricanes, polar vortices).
pub const MAX_SLASHING_EVENTS_PER_BLOCK: usize = 100;

/// Activation height for the per-block slashing circuit breaker.
pub const SLASHING_CIRCUIT_BREAKER_ACTIVATION_HEIGHT: u64 = 0;

/// Activation height for scaled (network-size-aware) slashing penalties.
pub const SCALED_SLASHING_ACTIVATION_HEIGHT: u64 = 0;
/// Minimum penalty after scaling (prevents negligible penalties at large network sizes).
pub const SCALED_SLASHING_MIN_PENALTY: u128 = 1;
/// Maximum penalty after scaling (caps outlier base penalties).
pub const SCALED_SLASHING_MAX_PENALTY: u128 = 100;
/// Fixed-point precision for integer sqrt to avoid floating-point non-determinism.
const SCALED_SLASHING_PRECISION: u128 = 1_000_000;

pub const BASE_PENALTY_ORACLE_NOT_REGISTERED: u128 = 5;
pub const BASE_PENALTY_INVALID_SIGNATURE: u128 = 20;
pub const BASE_PENALTY_DOUBLE_ATTESTATION: u128 = 25;
pub const BASE_PENALTY_INVALID_ZK_PROOF: u128 = 50;
pub const BASE_PENALTY_DATA_QUALITY_VIOLATION: u128 = 30;

/// Tracks slashing within a single block and enforces the cap.
#[derive(Debug, Clone)]
pub struct SlashingCircuitBreaker {
    cap: usize,
    events_this_block: usize,
    skipped_this_block: usize,
}

impl Default for SlashingCircuitBreaker {
    fn default() -> Self {
        Self {
            cap: MAX_SLASHING_EVENTS_PER_BLOCK,
            events_this_block: 0,
            skipped_this_block: 0,
        }
    }
}

impl SlashingCircuitBreaker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a circuit breaker with a custom cap (useful in tests to exercise drop logic
    /// without depending on the global `MAX_SLASHING_EVENTS_PER_BLOCK` constant).
    pub fn with_cap(cap: usize) -> Self {
        Self {
            cap,
            events_this_block: 0,
            skipped_this_block: 0,
        }
    }

    /// Returns true if the cap has been reached and this slash should be skipped.
    pub fn would_exceed_cap(&self) -> bool {
        self.events_this_block >= self.cap
    }

    /// Record that a slashing event was applied.
    pub fn record_slash(&mut self) {
        self.events_this_block += 1;
    }

    /// Record that a slashing event was skipped due to the cap.
    pub fn record_skipped(&mut self) {
        self.skipped_this_block += 1;
    }

    /// Reset for a new block.
    pub fn reset(&mut self) {
        self.events_this_block = 0;
        self.skipped_this_block = 0;
    }

    pub fn events_applied(&self) -> usize {
        self.events_this_block
    }

    pub fn events_skipped(&self) -> usize {
        self.skipped_this_block
    }
}

/// Integer square root (Newton's method). Consensus-deterministic — no floats.
fn isqrt_u128(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Scales a base penalty by `1 / sqrt(oracle_count)`, clamped to
/// `[SCALED_SLASHING_MIN_PENALTY, SCALED_SLASHING_MAX_PENALTY]`.
///
/// Uses fixed-point arithmetic (`SCALED_SLASHING_PRECISION`) so results are
/// deterministic across all nodes (no floating-point).
pub fn scaled_penalty(base: u128, oracle_count: usize) -> u128 {
    if oracle_count <= 1 {
        return base.clamp(SCALED_SLASHING_MIN_PENALTY, SCALED_SLASHING_MAX_PENALTY);
    }
    let n = oracle_count as u128;
    let p = SCALED_SLASHING_PRECISION;
    let numerator = base.saturating_mul(p);
    let denominator = isqrt_u128(n.saturating_mul(p).saturating_mul(p));
    if denominator == 0 {
        return base.clamp(SCALED_SLASHING_MIN_PENALTY, SCALED_SLASHING_MAX_PENALTY);
    }
    (numerator / denominator).clamp(SCALED_SLASHING_MIN_PENALTY, SCALED_SLASHING_MAX_PENALTY)
}

#[derive(Debug, Default, Clone)]
pub struct SlashingEngine {
    low_quality_counters: FxHashMap<String, u32>,
}

impl SlashingEngine {
    pub fn evaluate_reveal(
        &mut self,
        participant: &str,
        reveal: &RevealPayload,
        tx: &Transaction,
    ) -> Option<SlashingEvent> {
        if !tx.has_valid_reveal_commit() {
            return Some(SlashingEvent {
                address_or_oracle_id: participant.to_string(),
                amount: 10,
                reason: SlashingReason::InvalidReveal,
                details: format!("commit mismatch for {}", reveal.commit_id),
            });
        }
        None
    }

    pub fn evaluate_oracle_trigger(
        &self,
        trigger: &SlashingTrigger,
        oracle_count: usize,
        block_height: u64,
    ) -> Option<SlashingEvent> {
        match trigger {
            SlashingTrigger::OracleNotRegistered { oracle_id } => {
                let amount = if block_height >= SCALED_SLASHING_ACTIVATION_HEIGHT {
                    scaled_penalty(BASE_PENALTY_ORACLE_NOT_REGISTERED, oracle_count)
                } else {
                    BASE_PENALTY_ORACLE_NOT_REGISTERED
                };
                Some(SlashingEvent {
                    address_or_oracle_id: oracle_id.clone(),
                    amount,
                    reason: SlashingReason::OracleAttestationFraud,
                    details: "oracle not in oracle_set".to_string(),
                })
            },
            SlashingTrigger::InvalidSignature {
                oracle_id,
                commit_id,
            } => {
                let amount = if block_height >= SCALED_SLASHING_ACTIVATION_HEIGHT {
                    scaled_penalty(BASE_PENALTY_INVALID_SIGNATURE, oracle_count)
                } else {
                    BASE_PENALTY_INVALID_SIGNATURE
                };
                Some(SlashingEvent {
                    address_or_oracle_id: oracle_id.clone(),
                    amount,
                    reason: SlashingReason::OracleAttestationFraud,
                    details: format!("invalid signature on commit {commit_id}"),
                })
            },
            SlashingTrigger::DoubleAttestation {
                oracle_id,
                commit_id,
            } => {
                let amount = if block_height >= SCALED_SLASHING_ACTIVATION_HEIGHT {
                    scaled_penalty(BASE_PENALTY_DOUBLE_ATTESTATION, oracle_count)
                } else {
                    BASE_PENALTY_DOUBLE_ATTESTATION
                };
                Some(SlashingEvent {
                    address_or_oracle_id: oracle_id.clone(),
                    amount,
                    reason: SlashingReason::OracleAttestationFraud,
                    details: format!("double attestation on commit {commit_id}"),
                })
            },
            SlashingTrigger::InvalidWeatherProof {
                oracle_id,
                commit_id,
            } => {
                let amount = if block_height >= SCALED_SLASHING_ACTIVATION_HEIGHT {
                    scaled_penalty(BASE_PENALTY_INVALID_ZK_PROOF, oracle_count)
                } else {
                    BASE_PENALTY_INVALID_ZK_PROOF
                };
                Some(SlashingEvent {
                    address_or_oracle_id: oracle_id.clone(),
                    amount,
                    reason: SlashingReason::InvalidZKProof,
                    details: format!("invalid ZK weather proof for commit {commit_id}"),
                })
            },
            SlashingTrigger::DataQualityViolation {
                oracle_id,
                station_id,
                field,
                value,
                threshold,
            } => {
                let amount = if block_height >= SCALED_SLASHING_ACTIVATION_HEIGHT {
                    scaled_penalty(BASE_PENALTY_DATA_QUALITY_VIOLATION, oracle_count)
                } else {
                    BASE_PENALTY_DATA_QUALITY_VIOLATION
                };
                Some(SlashingEvent {
                    address_or_oracle_id: oracle_id.clone(),
                    amount,
                    reason: SlashingReason::DataQualityViolation,
                    details: format!(
                        "physical range violation: {field}={value:.2} exceeds threshold {threshold:.2} \
                         (station {station_id})"
                    ),
                })
            },
        }
    }

    pub fn evaluate_quality(
        &mut self,
        participant: &str,
        quality_score: Option<&DataQualityScore>,
        policy: &DataQualityPolicy,
    ) -> Option<SlashingEvent> {
        let quality_eval = policy.evaluate(quality_score);
        if !quality_eval.counts_toward_low_quality_streak {
            self.low_quality_counters.remove(participant);
            return None;
        }

        let counter = self
            .low_quality_counters
            .entry(participant.to_string())
            .or_insert(0);
        *counter += 1;
        if *counter < policy.low_quality_slash_streak {
            return None;
        }

        Some(SlashingEvent {
            address_or_oracle_id: participant.to_string(),
            amount: policy.low_quality_slash_amount,
            reason: SlashingReason::RepeatedLowQualityData,
            details: format!(
                "{counter} consecutive low-quality submissions (threshold {})",
                policy.low_quality_slash_streak
            ),
        })
    }

    /// Prune stale low-quality counters for participants not in the current oracle set.
    /// Call once per block to prevent unbounded map growth at large network sizes (#841).
    ///
    /// The caller supplies a predicate so the engine does not need to know
    /// which concrete container type the active-participant set lives in
    /// (`State::oracle_set` is `FxHashSet<String>`, tests often use
    /// `std::collections::HashSet<String>`).
    pub fn prune_counters<F>(&mut self, is_active: F)
    where
        F: Fn(&str) -> bool,
    {
        self.low_quality_counters
            .retain(|participant, _| is_active(participant.as_str()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::attestation::SlashingTrigger;
    use wrfcoin_meteo_types::{EARTH_MAX_AIR_TEMPERATURE_C, EARTH_MAX_PRESSURE_HPA};

    #[test]
    fn circuit_breaker_caps_slashing_events() {
        let mut cb = SlashingCircuitBreaker::new();
        assert!(!cb.would_exceed_cap());
        for _ in 0..MAX_SLASHING_EVENTS_PER_BLOCK {
            assert!(!cb.would_exceed_cap());
            cb.record_slash();
        }
        assert!(cb.would_exceed_cap());
        cb.record_skipped();
        assert_eq!(cb.events_applied(), MAX_SLASHING_EVENTS_PER_BLOCK);
        assert_eq!(cb.events_skipped(), 1);
    }

    #[test]
    fn circuit_breaker_resets_between_blocks() {
        let mut cb = SlashingCircuitBreaker::new();
        for _ in 0..MAX_SLASHING_EVENTS_PER_BLOCK {
            cb.record_slash();
        }
        assert!(cb.would_exceed_cap());
        cb.reset();
        assert!(!cb.would_exceed_cap());
        assert_eq!(cb.events_applied(), 0);
        assert_eq!(cb.events_skipped(), 0);
    }

    #[test]
    fn evaluate_oracle_trigger_data_quality_violation_returns_event() {
        let engine = SlashingEngine::default();
        let trigger = SlashingTrigger::DataQualityViolation {
            oracle_id: "oracle-abc".to_string(),
            station_id: "station-1".to_string(),
            field: "temperature".to_string(),
            value: 200.0,
            threshold: EARTH_MAX_AIR_TEMPERATURE_C,
        };
        // oracle_count=1 → scaled_penalty(30, 1) = 30.clamp(1, 100) = 30
        let event = engine
            .evaluate_oracle_trigger(&trigger, 1, 0)
            .expect("DataQualityViolation must produce a SlashingEvent");
        assert_eq!(event.reason, SlashingReason::DataQualityViolation);
        assert_eq!(event.address_or_oracle_id, "oracle-abc");
        assert_eq!(event.amount, 30);
    }

    #[test]
    fn scaled_penalty_at_12_oracles() {
        // 30 / sqrt(12) ≈ 8.66 → integer: 8
        let p = scaled_penalty(30, 12);
        assert!((8..=9).contains(&p), "expected 8..=9, got {p}");
    }

    #[test]
    fn scaled_penalty_at_100_oracles() {
        // 30 / sqrt(100) = 3.0
        assert_eq!(scaled_penalty(30, 100), 3);
    }

    #[test]
    fn scaled_penalty_at_1000_oracles() {
        // 30 / sqrt(1000) ≈ 0.95 → clamped to min=1
        assert_eq!(scaled_penalty(30, 1000), 1);
    }

    #[test]
    fn scaled_penalty_at_12000_oracles() {
        // 30 / sqrt(12000) ≈ 0.27 → clamped to min=1
        assert_eq!(scaled_penalty(30, 12000), 1);
    }

    #[test]
    fn scaled_penalty_clamps_to_max() {
        // 200 with 1 oracle → 200.clamp(1, 100) = 100
        assert_eq!(scaled_penalty(200, 1), 100);
    }

    #[test]
    fn scaled_penalty_zero_oracles_returns_clamped_base() {
        // oracle_count=0 → treated as ≤1 → base.clamp(1, 100) = 30
        assert_eq!(scaled_penalty(30, 0), 30);
    }

    #[test]
    fn scaled_penalty_per_offense_differentiation() {
        // Different base penalties produce different scaled results at the same oracle_count
        let count = 12;
        let p5 = scaled_penalty(5, count);
        let p20 = scaled_penalty(20, count);
        let p50 = scaled_penalty(50, count);
        assert!(
            p5 < p20,
            "5-base ({p5}) should be less than 20-base ({p20})"
        );
        assert!(
            p20 < p50,
            "20-base ({p20}) should be less than 50-base ({p50})"
        );
    }

    #[test]
    fn circuit_breaker_deterministic_ordering() {
        // Use a local cap of 3 so the test exercises the drop logic regardless of what
        // MAX_SLASHING_EVENTS_PER_BLOCK is set to globally.
        const LOCAL_CAP: usize = 3;

        // 5 triggers with out-of-order oracle_ids.
        let oracle_ids = ["e", "c", "a", "d", "b"];
        let make_triggers = |ids: &[&str]| -> Vec<(SlashingTrigger, u64)> {
            ids.iter()
                .map(|id| {
                    (
                        SlashingTrigger::DataQualityViolation {
                            oracle_id: id.to_string(),
                            station_id: "s1".to_string(),
                            field: "temperature".to_string(),
                            value: 200.0,
                            threshold: EARTH_MAX_AIR_TEMPERATURE_C,
                        },
                        100,
                    )
                })
                .collect()
        };

        let flush = |triggers: Vec<(SlashingTrigger, u64)>| -> Vec<String> {
            let mut sorted = triggers;
            sorted.sort_by(|(a, _), (b, _)| a.oracle_id().cmp(b.oracle_id()));
            let mut cb = SlashingCircuitBreaker::with_cap(LOCAL_CAP);
            let mut slashed = Vec::new();
            for (trigger, _height) in sorted {
                if cb.would_exceed_cap() {
                    cb.record_skipped();
                    continue;
                }
                slashed.push(trigger.oracle_id().to_string());
                cb.record_slash();
            }
            slashed
        };

        // With cap=3 and sorted input ["a","b","c","d","e"], only the first 3 are slashed.
        let result_forward = flush(make_triggers(&oracle_ids));
        assert_eq!(
            result_forward,
            vec!["a", "b", "c"],
            "cap=3, sorted → first 3 alphabetically"
        );

        // Reversed input must produce the same result (order-independence).
        let reversed: Vec<&str> = oracle_ids.iter().rev().copied().collect();
        let result_reversed = flush(make_triggers(&reversed));
        assert_eq!(
            result_reversed, result_forward,
            "order-independent: reversed input → same result"
        );
    }

    // ----- Persistent streak tests (#1408) ---------------------------------
    //
    // Before the #1408 fix, `State::apply_slashing_trigger_inner` constructed
    // `SlashingEngine::default()` on every call, so the internal
    // `low_quality_counters` map was always empty and `evaluate_quality`
    // never reached the slash-threshold branch. These tests pin the
    // persistence contract at the engine level: given a single long-lived
    // `SlashingEngine`, streak state must accumulate across calls and
    // `prune_counters` must bound the map by the active-participant set.

    fn low_quality_score() -> DataQualityScore {
        // overall well below warning_only_threshold=0.50 so `evaluate()` returns Low.
        DataQualityScore {
            overall: 0.10,
            range_score: 0.10,
            consistency_score: 0.10,
        }
    }

    fn high_quality_score() -> DataQualityScore {
        DataQualityScore {
            overall: 0.95,
            range_score: 0.95,
            consistency_score: 0.95,
        }
    }

    #[test]
    fn evaluate_quality_persists_streak_across_calls() {
        // Default policy: low_quality_slash_streak = 3, low_quality_slash_amount = 15.
        let policy = DataQualityPolicy::default();
        let mut engine = SlashingEngine::default();
        let score = low_quality_score();

        // First two consecutive low-quality submissions must not slash — the
        // streak threshold is 3. Pre-fix, each call would see an empty
        // counter map and the third call would still return None.
        assert!(
            engine.evaluate_quality("oracle-a", Some(&score), &policy).is_none(),
            "offense #1 should not slash (streak=1 < threshold=3)"
        );
        assert!(
            engine.evaluate_quality("oracle-a", Some(&score), &policy).is_none(),
            "offense #2 should not slash (streak=2 < threshold=3)"
        );

        // Third consecutive low-quality submission must fire. This is the
        // specific bug #1408 is about — a fresh-per-call engine never gets here.
        let event = engine
            .evaluate_quality("oracle-a", Some(&score), &policy)
            .expect("offense #3 must fire at streak threshold");
        assert_eq!(event.reason, SlashingReason::RepeatedLowQualityData);
        assert_eq!(event.address_or_oracle_id, "oracle-a");
        assert_eq!(event.amount, policy.low_quality_slash_amount);
    }

    #[test]
    fn evaluate_quality_tracks_streaks_per_participant_independently() {
        let policy = DataQualityPolicy::default();
        let mut engine = SlashingEngine::default();
        let score = low_quality_score();

        // Build oracle-a up to its 2nd offense while oracle-b only has one,
        // then drive oracle-a over the threshold. oracle-b must stay under.
        assert!(engine.evaluate_quality("oracle-a", Some(&score), &policy).is_none());
        assert!(engine.evaluate_quality("oracle-b", Some(&score), &policy).is_none());
        assert!(engine.evaluate_quality("oracle-a", Some(&score), &policy).is_none());

        // oracle-a's 3rd consecutive offense must fire.
        let event = engine
            .evaluate_quality("oracle-a", Some(&score), &policy)
            .expect("oracle-a fires on its 3rd consecutive offense");
        assert_eq!(event.address_or_oracle_id, "oracle-a");

        // oracle-b only has 1 recorded offense — the next one must not fire.
        assert!(
            engine.evaluate_quality("oracle-b", Some(&score), &policy).is_none(),
            "oracle-b is still at streak=2; must not fire on offense #2 (threshold=3)"
        );
    }

    #[test]
    fn evaluate_quality_resets_streak_on_non_low_submission() {
        let policy = DataQualityPolicy::default();
        let mut engine = SlashingEngine::default();
        let low = low_quality_score();
        let high = high_quality_score();

        // Build up to just-under-threshold, then submit a good observation.
        engine.evaluate_quality("oracle-x", Some(&low), &policy);
        engine.evaluate_quality("oracle-x", Some(&low), &policy);
        assert!(
            engine.evaluate_quality("oracle-x", Some(&high), &policy).is_none(),
            "good submission returns no event and resets the streak"
        );

        // Two fresh low-quality offenses must not fire — the counter is back to 0.
        assert!(
            engine.evaluate_quality("oracle-x", Some(&low), &policy).is_none(),
            "post-reset offense #1 must not fire"
        );
        assert!(
            engine.evaluate_quality("oracle-x", Some(&low), &policy).is_none(),
            "post-reset offense #2 must not fire (threshold=3)"
        );
        assert!(
            engine.evaluate_quality("oracle-x", Some(&low), &policy).is_some(),
            "post-reset offense #3 must fire"
        );
    }

    #[test]
    fn prune_counters_retains_only_active_participants() {
        let policy = DataQualityPolicy::default();
        let mut engine = SlashingEngine::default();
        let score = low_quality_score();

        engine.evaluate_quality("oracle-active", Some(&score), &policy);
        engine.evaluate_quality("oracle-retired", Some(&score), &policy);
        assert_eq!(engine.low_quality_counters.len(), 2);

        // Only oracle-active is in the active set.
        let active: std::collections::HashSet<String> = ["oracle-active".to_string()]
            .into_iter()
            .collect();
        engine.prune_counters(|p| active.contains(p));

        assert_eq!(engine.low_quality_counters.len(), 1);
        assert!(engine.low_quality_counters.contains_key("oracle-active"));
        assert!(!engine.low_quality_counters.contains_key("oracle-retired"));
    }

    #[test]
    fn prune_counters_accepts_any_predicate_shape() {
        // Regression test for the signature change: `prune_counters` no
        // longer requires a `std::collections::HashSet<String>` — it takes
        // any `Fn(&str) -> bool`. This keeps `State::oracle_set`
        // (`FxHashSet<String>`) callable without an allocation.
        let policy = DataQualityPolicy::default();
        let mut engine = SlashingEngine::default();
        engine.evaluate_quality("keep", Some(&low_quality_score()), &policy);
        engine.evaluate_quality("drop", Some(&low_quality_score()), &policy);

        engine.prune_counters(|p| p == "keep");

        assert!(engine.low_quality_counters.contains_key("keep"));
        assert!(!engine.low_quality_counters.contains_key("drop"));
    }

    #[test]
    fn evaluate_trigger_uses_scaled_penalty() {
        let engine = SlashingEngine::default();
        let trigger = SlashingTrigger::DataQualityViolation {
            oracle_id: "oracle-xyz".to_string(),
            station_id: "station-99".to_string(),
            field: "pressure".to_string(),
            value: 1200.0,
            threshold: EARTH_MAX_PRESSURE_HPA,
        };
        let event = engine
            .evaluate_oracle_trigger(&trigger, 12, 0)
            .expect("should produce event");
        // With 12 oracles, penalty should be scaled down from the flat 30
        assert_ne!(
            event.amount, 30,
            "penalty should differ from flat 30 at 12 oracles"
        );
        assert_eq!(
            event.amount,
            scaled_penalty(BASE_PENALTY_DATA_QUALITY_VIOLATION, 12)
        );
    }
}
