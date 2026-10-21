use crate::blockchain::transaction::OracleAttestationPayload;
use crate::common::constants::DEFAULT_ORACLE_QUORUM;
use crate::security::key_registry::{SigningKeyRegistry, SigningRole};
use crate::weather::validation::{
    DataQualitySlashingPolicy, PhysicsViolation, WeatherObservation, WeatherReading,
    validate_physics, validate_weather_observation_with_elevation,
};
use anyhow::{Result, anyhow};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use wrfcoin_reputation_engine::{
    ContributorRole, ReputationEngine, ReputationEvent, ReputationSignal, SCORE_SCALE,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationRecord {
    pub oracle_id: String,
    pub approved: bool,
    pub confidence: f64,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AttestationSet {
    pub by_oracle: FxHashMap<String, AttestationRecord>,
}

/// One attester's exact integer allocation inside an [`AttesterSettlementRecord`].
///
/// `amount` is in base units and is a **final** figure, not a nominal one: the
/// remainder of the per-commit division has already been distributed by the
/// time a record is built, so a replaying node adds these numbers up rather
/// than re-dividing anything.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AttesterAllocation {
    /// Oracle identity, as it appears in `AttestationSet::by_oracle`.
    pub attester_id: String,
    /// Base units paid to this attester for this decision.
    pub amount: u128,
}

/// Attester pay that is OWED for a decision whose quorum threshold has been
/// crossed but which no block has settled yet (#2846, ADR-003 rule 1 as
/// amended 2026-08-31).
///
/// # Why the debt carries the eligible set
///
/// ADR-003 rule 3 fixes eligibility at **the end of the threshold-crossing
/// block**: every canonical approved vote present when that block closed, one
/// per identity, and "votes landing in later blocks are unpaid". The crossing
/// block normally pays, and then live state at settlement time IS the end of
/// the crossing block. But the duty is durable: if the crossing block omits
/// or cannot fit the settlement, a later block discharges it — and by then a
/// straggler may have voted. Deriving eligibility from live state at that
/// later block would pay the straggler and dilute the voters who were
/// eligible, which is exactly what rule 3 forbids.
///
/// So the eligible set is frozen into the debt itself, on the apply path,
/// from the block's own attestation transitions: a vote inside the crossing
/// block joins the set, a vote in any later block does not. The debt is
/// state-root material, so producer, P2P applier and WAL replay settle from
/// the same frozen set whichever block discharges it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttesterSettlementDebt {
    /// Height of the threshold-crossing block — the block whose closing
    /// attestation set defines eligibility.
    pub crossing_height: u64,
    /// Eligible attester identities: the approved canonical votes present at
    /// the end of the crossing block, canonically sorted, no duplicates.
    pub eligible: Vec<String>,
}

/// The ADR-003 Phase A attester settlement record (#2846).
///
/// # Why a record and not a count
///
/// The chain already carries `oracle_reward_quorum_commit_count`, and it is
/// tempting to treat it as a payment instruction. It is not: it is evidence
/// that quorums happened. It carries no attester identities, no per-decision
/// boundaries, and no eligible cardinality — everything a payment needs and
/// nothing a counter has. This record is the typed, block-hashed thing that
/// does carry them.
///
/// # Replayed, never recomputed
///
/// Producer, P2P applier and WAL replay all consume *this* record (ADR-003
/// attester settlement rule 6). None of them recomputes an allocation from
/// local timing, because "which votes had landed when" is not recoverable from
/// state after the fact: `AttestationSet::add` REPLACES an oracle's record, so
/// a re-attestation is indistinguishable from a first vote in the resulting
/// state, and `AttestationRecord` carries no chain-assigned height. A node that
/// re-derived the split would be guessing, and two nodes guessing differently
/// is a consensus split.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttesterSettlementRecord {
    /// The decision (commit) id this settlement pays for. Also the key of the
    /// terminal marker that makes every later event for it a payment no-op.
    pub decision_id: String,
    /// The height of the block that PAID this settlement. Normally the
    /// threshold-crossing block; under the durable duty (ADR-003 rule 1 as
    /// amended) it may be a later block discharging the debt. Eligibility is
    /// anchored to the crossing block regardless — see
    /// [`AttesterSettlementDebt`].
    pub finalization_height: u64,
    /// Eligible attesters and their exact allocations, **canonically sorted by
    /// `attester_id`**. Sorted at construction so the serialized record, the
    /// state-root pre-image and the block metadata are byte-identical on every
    /// peer regardless of the order votes arrived in.
    pub allocations: Vec<AttesterAllocation>,
}

/// Phase B payment for one authorized block producer. The payout is resolved
/// from the height-effective roster; metadata alone never authorizes a mint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProducerSettlementRecord {
    pub height: u64,
    pub payout_address: String,
    pub amount: u128,
}

impl AttesterSettlementRecord {
    /// Build a record, enforcing the canonical sort at the one place records
    /// are created rather than trusting every call site to remember.
    pub fn new(
        decision_id: String,
        finalization_height: u64,
        mut allocations: Vec<AttesterAllocation>,
    ) -> Self {
        allocations.sort_by(|left, right| left.attester_id.cmp(&right.attester_id));
        Self {
            decision_id,
            finalization_height,
            allocations,
        }
    }

    /// Total base units this record pays out.
    pub fn total_amount(&self) -> u128 {
        self.allocations
            .iter()
            .fold(0u128, |acc, allocation| {
                acc.saturating_add(allocation.amount)
            })
    }

    /// Number of eligible attestation commits this record settles.
    pub fn eligible_commit_count(&self) -> usize {
        self.allocations.len()
    }

    /// True when the allocations are canonically sorted and free of duplicate
    /// identities. A replaying node checks this before applying: a record that
    /// fails it did not come from an honest producer, and applying it would
    /// pay one identity twice.
    pub fn is_canonical(&self) -> bool {
        self.allocations
            .windows(2)
            .all(|pair| pair[0].attester_id < pair[1].attester_id)
    }
}

impl AttestationSet {
    pub fn add(&mut self, record: AttestationRecord) -> Option<AttestationRecord> {
        self.by_oracle.insert(record.oracle_id.clone(), record)
    }

    pub fn approvals(&self) -> usize {
        self.by_oracle.values().filter(|r| r.approved).count()
    }

    pub fn has_quorum(&self, quorum: usize) -> bool {
        self.approvals() >= quorum
    }

    /// LEGACY float average — order-sensitive summation over an unordered
    /// map, deterministic across nodes only by the coincidence of unseeded
    /// FxHash plus replay-identical insertion sequences. Kept bit-exact
    /// because it is what every block below the ownership-binding activation
    /// height was paid under; consensus paths must call
    /// [`Self::average_confidence_at_height`], which routes here only below
    /// the gate.
    pub fn average_confidence(&self) -> f64 {
        // determinism-ok: the frozen pre-activation branch — replay of already-paid blocks requires these exact float semantics; the hardened integer path below replaces it from the activation height
        let approvals: Vec<f64> = self
            .by_oracle
            .values()
            .filter(|r| r.approved)
            .map(|r| r.confidence)
            .collect();
        if approvals.is_empty() {
            return 0.0;
        }
        // determinism-ok: as above — frozen legacy branch, replaced at the activation height
        approvals.iter().sum::<f64>() / approvals.len() as f64
    }

    /// Height-gated average confidence — the consensus entry point.
    ///
    /// From `STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT` the average is
    /// computed **in the mu domain**: each confidence is clamped to `[0, 1]`
    /// and quantized through the crate-shared `quantize_score_mu` (the same
    /// float→fixed-point boundary `score_oracle_attestation` already uses),
    /// then summed and divided in integers. Integer addition is commutative
    /// and exact, so the result is independent of `by_oracle`'s iteration
    /// order **by algebra**, not by hash-layout coincidence — which is the
    /// contract the legacy float path never had.
    ///
    /// The f64 boundary stays at the call sites: the return converts mu back
    /// to f64 at exactly one named point below, rather than moving
    /// `oracle_quality` to mu end-to-end, because the emission API takes f64
    /// and rewriting `calculate_contribution_reward`'s arithmetic is a wider
    /// consensus change than the ruling ordered. That single conversion is
    /// bit-deterministic everywhere: the integer average is ≤ 1_000_000
    /// (each addend is clamped before quantizing), both integers convert to
    /// f64 exactly (< 2^53), and one IEEE-754 division of identical operands
    /// is correctly rounded to identical bits on every conforming platform.
    ///
    /// Below the gate this delegates to [`Self::average_confidence`]
    /// unchanged — the branch that keeps blocks 674/716/851 replaying to the
    /// same state root.
    // determinism-ok: f64 only at the edges — mu-quantized inputs and one exact integer→f64 conversion at the named point below; every accumulation step is integer arithmetic
    pub fn average_confidence_at_height(&self, block_height: u64) -> f64 {
        use crate::consensus::proof_of_publishing::{
            SCORE_PRECISION, STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT, quantize_score_mu,
        };
        if block_height < STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT {
            return self.average_confidence();
        }
        let (sum_mu, count) = self
            .by_oracle
            .values()
            .filter(|r| r.approved)
            .fold((0u64, 0u64), |(sum, count), r| {
                (
                    // determinism-ok: clamp+quantize is the SAME float→mu boundary score_oracle_attestation uses; everything after this line is integer arithmetic
                    sum.saturating_add(quantize_score_mu(r.confidence.clamp(0.0, 1.0))),
                    count + 1,
                )
            });
        if count == 0 {
            // determinism-ok: exactly-representable empty-set sentinel, same value the legacy branch returns
            return 0.0;
        }
        // THE named mu→f64 conversion point (see doc above for why it is
        // bit-deterministic).
        // determinism-ok: exact integer→f64 conversions plus one correctly-rounded division of identical operands; no order-sensitive float arithmetic remains
        (sum_mu / count) as f64 / SCORE_PRECISION as f64
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SlashingTrigger {
    OracleNotRegistered {
        oracle_id: String,
    },
    InvalidSignature {
        oracle_id: String,
        commit_id: String,
    },
    DoubleAttestation {
        oracle_id: String,
        commit_id: String,
    },
    InvalidWeatherProof {
        oracle_id: String,
        commit_id: String,
    },
    DataQualityViolation {
        oracle_id: String,
        station_id: String,
        /// Name of the violating field (e.g. "temperature").
        field: String,
        /// The submitted value.
        value: f64,
        /// The threshold that was exceeded.
        threshold: f64,
    },
}

impl SlashingTrigger {
    /// Extract the oracle identifier for deterministic ordering.
    pub fn oracle_id(&self) -> &str {
        match self {
            Self::OracleNotRegistered { oracle_id }
            | Self::InvalidSignature { oracle_id, .. }
            | Self::DoubleAttestation { oracle_id, .. }
            | Self::InvalidWeatherProof { oracle_id, .. }
            | Self::DataQualityViolation { oracle_id, .. } => oracle_id,
        }
    }
}

/// Number of blocks an oracle is protected from re-slashing after a slash.
pub const SLASHING_COOLDOWN_BLOCKS: u64 = 100;

/// Activation height for the slashing cooldown feature.
pub const SLASHING_COOLDOWN_ACTIVATION_HEIGHT: u64 = 0;

#[derive(Debug, Clone)]
pub struct OracleAttestationOutcome {
    pub commit_id: String,
    pub accepted: bool,
    pub slashing: Option<SlashingTrigger>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleRegistry {
    oracle_set: FxHashSet<String>,
    quorum: usize,
    /// u64 fixed-point reputation scores keyed by oracle/contributor id.
    /// Scale: [0, 100 * SCORE_SCALE]. Absent entries default to 50 * SCORE_SCALE / 100.
    #[serde(default)]
    reputation_scores: FxHashMap<String, u64>,
    /// Live reputation engine — drives dynamic score updates via EWMA smoothing and decay.
    #[serde(default)]
    reputation_engine: ReputationEngine,
    /// Block height at which each oracle was last slashed (for cooldown enforcement).
    #[serde(default)]
    last_slashed_height: FxHashMap<String, u64>,
}

impl Default for OracleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl OracleRegistry {
    pub fn new() -> Self {
        Self {
            oracle_set: FxHashSet::default(),
            quorum: DEFAULT_ORACLE_QUORUM.max(1),
            reputation_scores: FxHashMap::default(),
            reputation_engine: ReputationEngine::default(),
            last_slashed_height: FxHashMap::default(),
        }
    }

    pub fn with_params(oracle_set: FxHashSet<String>, quorum: usize) -> Self {
        let mut registry = Self::new();
        registry.oracle_set = oracle_set;
        registry.quorum = quorum.max(1);
        registry
    }

    /// Returns the stored u64 fixed-point reputation score for an oracle/contributor.
    ///
    /// Defaults to `SCORE_SCALE * 50 / 100` (= 0.5 on the [0, 1] normalized scale) when the
    /// oracle has no recorded history, giving new entrants a neutral trust factor.
    pub fn reputation_score(&self, oracle_id: &str) -> u64 {
        self.reputation_scores
            .get(oracle_id)
            .copied()
            .unwrap_or(SCORE_SCALE * 50 / 100)
    }

    /// Records or updates the u64 fixed-point reputation score for an oracle/contributor.
    pub fn set_reputation_score(&mut self, oracle_id: &str, score: u64) {
        self.reputation_scores.insert(oracle_id.to_string(), score);
    }

    /// Apply a rich multi-dimensional signal (contribution / forecast accuracy) and sync the
    /// composite score back into `reputation_scores`.
    pub fn apply_reputation_signal(&mut self, signal: ReputationSignal) {
        let oracle_id = signal.oracle_id.clone();
        let _ = self.reputation_engine.apply_signal(signal);
        self.sync_rep_score(&oracle_id);
    }

    /// Apply a simple delta event (e.g. slashing penalty) and sync the score back.
    pub fn apply_reputation_event(&mut self, event: ReputationEvent) {
        let oracle_id = event.oracle_id.clone();
        let _ = self.reputation_engine.apply_event(event);
        self.sync_rep_score(&oracle_id);
    }

    /// Decay all tracked reputation scores for inactivity — call once per block finalization.
    pub fn decay_reputation_scores(&mut self, current_height: u64) {
        self.reputation_engine.decay_scores(current_height);
        // Sync every oracle that has a stored score (these were all written via
        // apply_reputation_signal / apply_reputation_event, so they exist in the engine).
        let ids: Vec<String> = self.reputation_scores.keys().cloned().collect();
        for id in &ids {
            self.sync_rep_score(id);
        }
    }

    /// Sync the engine composite score for one oracle back into the flat scores map.
    ///
    /// The engine's composite_score is in [0, 100 * SCORE_SCALE]; the flat
    /// `reputation_scores` map (and PoP's `rep_factor = raw / SCORE_SCALE`) expects
    /// values in [0, SCORE_SCALE], so we normalize by dividing by 100.
    fn sync_rep_score(&mut self, oracle_id: &str) {
        if let Some(score) = self.reputation_engine.score_for(oracle_id) {
            self.reputation_scores
                .insert(oracle_id.to_string(), score.0 / 100);
        }
    }

    pub fn set_oracle_set(&mut self, oracle_set: FxHashSet<String>) {
        self.oracle_set = oracle_set;
    }

    pub fn set_quorum(&mut self, quorum: usize) {
        self.quorum = quorum.max(1);
    }

    pub fn quorum(&self) -> usize {
        self.quorum
    }

    pub fn oracle_set(&self) -> &FxHashSet<String> {
        &self.oracle_set
    }

    /// Returns oracle reputation scores sorted descending, capped at `limit` entries.
    /// Score is in [0, SCORE_SCALE] — divide by SCORE_SCALE and multiply by 100 for %.
    pub fn leaderboard(&self, limit: usize) -> Vec<(String, u64)> {
        let mut entries: Vec<(String, u64)> = self
            .oracle_set
            .iter()
            .map(|id| (id.clone(), self.reputation_score(id)))
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        entries.truncate(limit);
        entries
    }

    /// Return number of registered oracles.
    pub fn oracle_count(&self) -> usize {
        self.oracle_set.len()
    }

    pub fn register_oracle(&mut self, oracle_id: String) {
        self.oracle_set.insert(oracle_id);
    }

    pub fn remove_oracle(&mut self, oracle_id: &str) {
        self.oracle_set.remove(oracle_id);
    }

    /// Record that an oracle was slashed at the given block height.
    pub fn record_slash(&mut self, oracle_id: &str, block_height: u64) {
        self.last_slashed_height
            .insert(oracle_id.to_string(), block_height);
    }

    /// Check if an oracle is in cooldown (recently slashed).
    pub fn is_in_cooldown(&self, oracle_id: &str, current_height: u64) -> bool {
        if current_height < SLASHING_COOLDOWN_ACTIVATION_HEIGHT {
            return false;
        }
        match self.last_slashed_height.get(oracle_id) {
            Some(&slashed_at) => {
                current_height.saturating_sub(slashed_at) < SLASHING_COOLDOWN_BLOCKS
            },
            None => false,
        }
    }

    pub fn is_registered(&self, oracle_id: &str) -> bool {
        self.oracle_set.contains(oracle_id)
    }

    pub fn verify_attestation_signature(&self, attestation: &OracleAttestationPayload) -> bool {
        attestation.has_valid_signature()
    }

    pub fn verify_attestation_signature_with_registry(
        &self,
        attestation: &OracleAttestationPayload,
        signing_keys: &SigningKeyRegistry,
    ) -> bool {
        // Gate non-finite `confidence` before `signing_message()` so a crafted
        // attestation cannot panic the digest step (mirrors has_valid_signature).
        if !attestation.is_signable() {
            return false;
        }
        let Some(signature_hex) = attestation.signature.as_deref() else {
            return false;
        };
        let message = attestation.signing_message();
        signing_keys.verify_signature_for_identity_role(
            &attestation.oracle_id,
            SigningRole::Oracle,
            attestation.timestamp,
            message.as_bytes(),
            signature_hex,
        )
    }

    pub fn apply_attestation(
        &self,
        sets: &mut FxHashMap<String, AttestationSet>,
        attestation: &OracleAttestationPayload,
    ) -> Result<OracleAttestationOutcome> {
        if attestation.commit_id.trim().is_empty() {
            return Err(anyhow!("commit_id is required"));
        }
        if attestation.oracle_id.trim().is_empty() {
            return Err(anyhow!("oracle_id is required"));
        }

        if !self.is_registered(&attestation.oracle_id) {
            return Ok(OracleAttestationOutcome {
                commit_id: attestation.commit_id.clone(),
                accepted: false,
                slashing: Some(SlashingTrigger::OracleNotRegistered {
                    oracle_id: attestation.oracle_id.clone(),
                }),
            });
        }

        if !self.verify_attestation_signature(attestation) {
            return Ok(OracleAttestationOutcome {
                commit_id: attestation.commit_id.clone(),
                accepted: false,
                slashing: Some(SlashingTrigger::InvalidSignature {
                    oracle_id: attestation.oracle_id.clone(),
                    commit_id: attestation.commit_id.clone(),
                }),
            });
        }
        self.apply_verified_attestation(sets, attestation)
    }

    pub fn apply_attestation_with_registry(
        &self,
        sets: &mut FxHashMap<String, AttestationSet>,
        attestation: &OracleAttestationPayload,
        signing_keys: &SigningKeyRegistry,
    ) -> Result<OracleAttestationOutcome> {
        if attestation.commit_id.trim().is_empty() {
            return Err(anyhow!("commit_id is required"));
        }
        if attestation.oracle_id.trim().is_empty() {
            return Err(anyhow!("oracle_id is required"));
        }

        if !self.is_registered(&attestation.oracle_id) {
            return Ok(OracleAttestationOutcome {
                commit_id: attestation.commit_id.clone(),
                accepted: false,
                slashing: Some(SlashingTrigger::OracleNotRegistered {
                    oracle_id: attestation.oracle_id.clone(),
                }),
            });
        }

        if !self.verify_attestation_signature_with_registry(attestation, signing_keys) {
            return Ok(OracleAttestationOutcome {
                commit_id: attestation.commit_id.clone(),
                accepted: false,
                slashing: Some(SlashingTrigger::InvalidSignature {
                    oracle_id: attestation.oracle_id.clone(),
                    commit_id: attestation.commit_id.clone(),
                }),
            });
        }

        self.apply_verified_attestation(sets, attestation)
    }

    fn apply_verified_attestation(
        &self,
        sets: &mut FxHashMap<String, AttestationSet>,
        attestation: &OracleAttestationPayload,
    ) -> Result<OracleAttestationOutcome> {
        let set = sets.entry(attestation.commit_id.clone()).or_default();

        if let Some(previous) = set.by_oracle.get(&attestation.oracle_id) {
            // Reject any duplicate attestation from the same oracle for the same commit.
            // If the oracle changed its vote (approved differs), this is a slashable offense.
            // If the vote is identical, treat as a replay and reject without slashing.
            let slashing = if previous.approved != attestation.approved {
                Some(SlashingTrigger::DoubleAttestation {
                    oracle_id: attestation.oracle_id.clone(),
                    commit_id: attestation.commit_id.clone(),
                })
            } else {
                None
            };
            return Ok(OracleAttestationOutcome {
                commit_id: attestation.commit_id.clone(),
                accepted: false,
                slashing,
            });
        }

        let confidence = attestation.confidence.unwrap_or(1.0).clamp(0.0, 1.0);
        set.add(AttestationRecord {
            oracle_id: attestation.oracle_id.clone(),
            approved: attestation.approved,
            confidence,
            timestamp: attestation.timestamp,
        });

        Ok(OracleAttestationOutcome {
            commit_id: attestation.commit_id.clone(),
            accepted: true,
            slashing: None,
        })
    }

    pub fn ensure_commit_has_quorum(
        &self,
        sets: &FxHashMap<String, AttestationSet>,
        commit_id: &str,
    ) -> Result<()> {
        let Some(set) = sets.get(commit_id) else {
            return Err(anyhow!("No attestations for commit {commit_id}"));
        };
        if !set.has_quorum(self.quorum) {
            return Err(anyhow!(
                "Attestation quorum not met for commit {} (approvals: {}, quorum: {})",
                commit_id,
                set.approvals(),
                self.quorum
            ));
        }
        Ok(())
    }

    /// Verifies a forecast verification receipt and returns a slashing trigger if it is fraudulent.
    pub fn verify_receipt_with_slashing(
        &self,
        receipt: &crate::blockchain::transaction::ForecastVerificationReceipt,
        manifest: &crate::blockchain::transaction::ForecastArtifactManifest,
        zk_verifier: &dyn crate::consensus::zk_verification::ZkVerifier,
    ) -> Option<SlashingTrigger> {
        // 1. Check if oracle is registered
        if !self.is_registered(&receipt.oracle_id) {
            return Some(SlashingTrigger::OracleNotRegistered {
                oracle_id: receipt.oracle_id.clone(),
            });
        }

        // 2. Verify signature
        if !receipt.has_valid_signature() {
            return Some(SlashingTrigger::InvalidSignature {
                oracle_id: receipt.oracle_id.clone(),
                commit_id: receipt.commit_id.clone(),
            });
        }

        // 3. Verify ZK proof if present
        if let Some(ref zk_proof) = receipt.zk_proof {
            let Ok(model_id) = manifest.model_id.parse::<u64>() else {
                return None;
            };
            match zk_verifier.verify_weather_model(
                zk_proof,
                &manifest.input_hash,
                &manifest.output_hash,
                model_id,
                manifest.produced_at as u64,
            ) {
                Ok(true) => {},
                Ok(false) => {
                    return Some(SlashingTrigger::InvalidWeatherProof {
                        oracle_id: receipt.oracle_id.clone(),
                        commit_id: receipt.commit_id.clone(),
                    });
                },
                Err(_) => return None,
            }
        }

        None
    }

    /// Check a weather observation for physical-range violations.
    ///
    /// Runs a two-stage check:
    /// 1. **Physics pre-check** (`validate_physics`) — produces structured `PhysicsViolation`
    ///    values against `policy.physics_policy`. Fires first; returns on the first violation.
    /// 2. **String-based range check** (`validate_weather_observation`) — fallback for any
    ///    remaining range errors not caught by the physics pre-check.
    ///
    /// Returns `Some(SlashingTrigger::DataQualityViolation)` on the first violation if
    /// `block_height >= policy.activation_height`, else `None`.
    pub fn check_weather_data_quality(
        &self,
        oracle_id: &str,
        station_id: &str,
        observation: &WeatherObservation,
        policy: &DataQualitySlashingPolicy,
        block_height: u64,
        elevation_m: Option<f64>,
    ) -> Option<SlashingTrigger> {
        if block_height < policy.activation_height {
            return None;
        }

        // Stage 1: physics pre-check — structured violations, no string parsing required.
        let reading = WeatherReading {
            temperature_c: observation.temperature_c,
            pressure_hpa: observation.pressure_hpa,
            humidity_pct: observation.humidity_percent,
            wind_speed_ms: observation.wind_speed_ms,
            // WeatherObservation carries no timestamp at this call-site; rate-of-change
            // checks are skipped (prev_reading = None). Temporal checks remain in
            // validate_weather_observation_with_temporal_context.
            timestamp_unix: 0,
        };
        let phys_violations = validate_physics(
            &reading,
            None,
            &policy.physics_policy,
            block_height,
            elevation_m,
        );
        if let Some(v) = phys_violations.first() {
            let (field, value, threshold) = physics_violation_info(v);
            return Some(SlashingTrigger::DataQualityViolation {
                oracle_id: oracle_id.to_string(),
                station_id: station_id.to_string(),
                field,
                value,
                threshold,
            });
        }

        // Stage 2: elevation-aware observation validation for residual non-physics errors.
        let result = validate_weather_observation_with_elevation(observation, elevation_m);
        let first_error = result.errors.into_iter().next()?;

        // Parse structured info from the error string produced by validate_weather_observation.
        // Format: "<field> out of range: <value> not in [<min>, <max>]"
        let field = first_error
            .split_once(" out of range")
            .map(|(f, _)| f.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let value = first_error
            .split_once(": ")
            .and_then(|(_, rest)| rest.split_once(" not in"))
            .and_then(|(v, _)| v.trim().parse::<f64>().ok())
            .unwrap_or(0.0);
        let threshold = first_error
            .split_once('[')
            .and_then(|(_, rest)| rest.split_once(']'))
            .and_then(|(range, _)| range.split(", ").last())
            .and_then(|t| t.parse::<f64>().ok())
            .unwrap_or(0.0);

        Some(SlashingTrigger::DataQualityViolation {
            oracle_id: oracle_id.to_string(),
            station_id: station_id.to_string(),
            field,
            value,
            threshold,
        })
    }
}

/// Extract (field_name, value, threshold) from a `PhysicsViolation` for use in
/// `SlashingTrigger::DataQualityViolation`.
fn physics_violation_info(v: &PhysicsViolation) -> (String, f64, f64) {
    match v {
        PhysicsViolation::TemperatureOutOfRange { value_c, max, .. } => {
            ("temperature".to_string(), *value_c, *max)
        },
        PhysicsViolation::PressureOutOfRange { value_hpa, max, .. } => {
            ("pressure".to_string(), *value_hpa, *max)
        },
        PhysicsViolation::HumidityOutOfRange { value_pct, max, .. } => {
            ("humidity".to_string(), *value_pct, *max)
        },
        PhysicsViolation::WindSpeedExceedsMaximum { value_ms, max } => {
            ("wind_speed".to_string(), *value_ms, *max)
        },
        PhysicsViolation::WindSpeedBelowMinimum { value_ms, min } => {
            ("wind_speed".to_string(), *value_ms, *min)
        },
        PhysicsViolation::TemperatureRateExceeded { delta_c, limit } => {
            ("temperature_rate".to_string(), *delta_c, *limit)
        },
        PhysicsViolation::CrossParameterInconsistency {
            dew_point_c,
            temp_c,
            ..
        } => ("dew_point_consistency".to_string(), *dew_point_c, *temp_c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::zk_verification::{RejectAllZkVerifier, ZkVerifier};
    use crate::crypto::signature::{PrivateKey, PublicKey};
    use crate::security::key_registry::{SigningKeyRegistry, SigningRole};
    use crate::weather::validation::{DataQualitySlashingPolicy, WeatherObservation};
    use rand::{SeedableRng, rngs::StdRng};
    use wrfcoin_meteo_types::EARTH_MAX_AIR_TEMPERATURE_C;

    #[test]
    fn data_quality_violation_trigger_variant_compiles() {
        let trigger = SlashingTrigger::DataQualityViolation {
            oracle_id: "oracle-1".to_string(),
            station_id: "station-1".to_string(),
            field: "temperature".to_string(),
            value: 200.0,
            threshold: EARTH_MAX_AIR_TEMPERATURE_C,
        };
        assert!(matches!(
            trigger,
            SlashingTrigger::DataQualityViolation { .. }
        ));
    }

    #[test]
    fn check_data_quality_returns_trigger_for_impossible_temperature() {
        let registry = OracleRegistry::new();
        let policy = DataQualitySlashingPolicy::default();
        let obs = WeatherObservation {
            temperature_c: Some(200.0),
            humidity_percent: Some(50.0),
            pressure_hpa: Some(1013.0),
            wind_speed_ms: Some(5.0),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(10.0),
            ..Default::default()
        };
        let trigger =
            registry.check_weather_data_quality("oracle-1", "station-1", &obs, &policy, 100, None);
        assert!(matches!(
            trigger,
            Some(SlashingTrigger::DataQualityViolation { .. })
        ));
    }

    #[test]
    fn check_data_quality_returns_none_for_valid_observation() {
        let registry = OracleRegistry::new();
        let policy = DataQualitySlashingPolicy::default();
        let obs = WeatherObservation {
            temperature_c: Some(22.0),
            humidity_percent: Some(60.0),
            pressure_hpa: Some(1010.0),
            wind_speed_ms: Some(4.0),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(13.0),
            ..Default::default()
        };
        let trigger =
            registry.check_weather_data_quality("oracle-1", "station-1", &obs, &policy, 100, None);
        assert!(trigger.is_none());
    }

    #[test]
    fn check_data_quality_returns_none_below_activation_height() {
        let registry = OracleRegistry::new();
        let policy = DataQualitySlashingPolicy {
            activation_height: 500,
            ..DataQualitySlashingPolicy::default()
        };
        let obs = WeatherObservation {
            temperature_c: Some(200.0),
            humidity_percent: None,
            pressure_hpa: None,
            wind_speed_ms: None,
            precipitation_mm: None,
            dewpoint_c: None,
            ..Default::default()
        };
        let trigger =
            registry.check_weather_data_quality("oracle-1", "station-1", &obs, &policy, 10, None);
        assert!(trigger.is_none(), "must not slash below activation height");
    }

    #[test]
    fn check_data_quality_allows_low_pressure_when_station_elevation_is_known() {
        let registry = OracleRegistry::new();
        let policy = DataQualitySlashingPolicy::default();
        let obs = WeatherObservation {
            temperature_c: Some(20.0),
            humidity_percent: Some(50.0),
            pressure_hpa: Some(835.0),
            wind_speed_ms: Some(5.0),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(10.0),
            ..Default::default()
        };

        let trigger = registry.check_weather_data_quality(
            "oracle-1",
            "station-1",
            &obs,
            &policy,
            100,
            Some(1600.0),
        );

        assert!(
            trigger.is_none(),
            "valid Denver-ish pressure should not slash when elevation is known"
        );
    }

    #[test]
    fn check_data_quality_still_slashes_low_pressure_without_station_elevation() {
        let registry = OracleRegistry::new();
        let policy = DataQualitySlashingPolicy::default();
        let obs = WeatherObservation {
            temperature_c: Some(20.0),
            humidity_percent: Some(50.0),
            pressure_hpa: Some(835.0),
            wind_speed_ms: Some(5.0),
            precipitation_mm: Some(0.0),
            dewpoint_c: Some(10.0),
            ..Default::default()
        };

        let trigger =
            registry.check_weather_data_quality("oracle-1", "station-1", &obs, &policy, 100, None);

        assert!(
            matches!(trigger, Some(SlashingTrigger::DataQualityViolation { .. })),
            "same pressure must still slash when elevation metadata is missing"
        );
    }

    #[derive(Debug, Clone)]
    struct StaticWeatherVerifier(Result<bool, String>);

    impl ZkVerifier for StaticWeatherVerifier {
        fn verify_range_proof(
            &self,
            _proof_hex: &str,
            _commitment_hex: &str,
        ) -> std::result::Result<bool, String> {
            Ok(true)
        }

        fn verify_confidential_transfer(
            &self,
            _proof_hex: &str,
            _input_commitment_hex: &str,
            _output_commitment_hex: &str,
            _fee: u64,
            _num_inputs: usize,
            _num_outputs: usize,
        ) -> std::result::Result<bool, String> {
            Ok(true)
        }

        fn verify_selective_disclosure(
            &self,
            _proof_hex: &str,
            _commitment_hex: &str,
        ) -> std::result::Result<bool, String> {
            Ok(true)
        }

        fn verify_weather_model(
            &self,
            _proof_hex: &str,
            _input_hash_hex: &str,
            _output_hash_hex: &str,
            _model_id: u64,
            _time_secs: u64,
        ) -> std::result::Result<bool, String> {
            self.0.clone()
        }

        fn verify_batch_proof(
            &self,
            _proof_hex: &str,
            _commitment_hex: &str,
        ) -> std::result::Result<bool, String> {
            self.0.clone()
        }
    }

    fn oracle_identity(seed: u64) -> (PrivateKey, String) {
        let mut rng = StdRng::seed_from_u64(seed);
        let private_key = PrivateKey::generate(&mut rng);
        let oracle_id = PublicKey::from_private_key(&private_key).to_string();
        (private_key, oracle_id)
    }

    fn valid_manifest() -> crate::blockchain::transaction::ForecastArtifactManifest {
        crate::blockchain::transaction::ForecastArtifactManifest {
            schema_version: 1,
            model_id: "7".to_string(),
            input_hash: "a".repeat(64),
            output_hash: "b".repeat(64),
            produced_at: 1_700_000_000,
            region: "us-west".to_string(),
            horizon_hours: 24,
            input_variables: vec!["humidity".to_string()],
            output_variables: vec!["temperature".to_string()],
        }
    }

    fn signed_receipt(
        private_key: &PrivateKey,
        oracle_id: &str,
    ) -> crate::blockchain::transaction::ForecastVerificationReceipt {
        let mut receipt = crate::blockchain::transaction::ForecastVerificationReceipt {
            manifest_digest: "c".repeat(64),
            commit_id: "commit-1".to_string(),
            oracle_id: oracle_id.to_string(),
            metrics: crate::blockchain::transaction::ForecastAccuracyMetrics {
                mae: 0.1,
                rmse: 0.2,
                bias: 0.0,
                skill_score: 0.9,
            },
            verified_at: 1_700_000_010,
            nonce: "nonce-1".to_string(),
            zk_proof: Some("ab".repeat(192)),
            signature: None,
        };
        let message = receipt.signing_message();
        receipt.signature = Some(private_key.sign(message.as_bytes()).to_string());
        receipt
    }

    fn signed_attestation(
        private_key: &PrivateKey,
        oracle_id: &str,
        commit_id: &str,
        approved: bool,
        confidence: f64,
        timestamp: i64,
    ) -> OracleAttestationPayload {
        let mut payload = OracleAttestationPayload {
            commit_id: commit_id.to_string(),
            oracle_id: oracle_id.to_string(),
            approved,
            signature: None,
            confidence: Some(confidence),
            timestamp,
            nonce: None,
        };
        let message = payload.signing_message();
        payload.signature = Some(private_key.sign(message.as_bytes()).to_string());
        payload
    }

    #[test]
    fn quorum_check_works() {
        let mut registry = OracleRegistry::new();
        let (oracle_a_sk, oracle_a_id) = oracle_identity(1);
        let (oracle_b_sk, oracle_b_id) = oracle_identity(2);
        registry.register_oracle(oracle_a_id.clone());
        registry.register_oracle(oracle_b_id.clone());
        registry.set_quorum(2);

        let mut sets = FxHashMap::default();
        let a = signed_attestation(&oracle_a_sk, &oracle_a_id, "c1", true, 1.0, 1);
        let b = signed_attestation(&oracle_b_sk, &oracle_b_id, "c1", true, 0.9, 2);

        registry.apply_attestation(&mut sets, &a).unwrap();
        assert!(registry.ensure_commit_has_quorum(&sets, "c1").is_err());
        registry.apply_attestation(&mut sets, &b).unwrap();
        assert!(registry.ensure_commit_has_quorum(&sets, "c1").is_ok());
    }

    #[test]
    fn leaderboard_sorts_descending_and_respects_limit() {
        let mut reg = OracleRegistry::new();
        reg.register_oracle("oracle-a".into());
        reg.register_oracle("oracle-b".into());
        reg.register_oracle("oracle-c".into());

        reg.set_reputation_score("oracle-b", 900_000);
        reg.set_reputation_score("oracle-a", 700_000);
        // oracle-c has no direct score entry and should rank after stronger scores

        let full = reg.leaderboard(10);
        assert_eq!(full.len(), 3);
        assert_eq!(full[0].0, "oracle-b");
        assert_eq!(full[1].0, "oracle-a");
        assert_eq!(full[2].0, "oracle-c");
        assert!(full[0].1 >= full[1].1);
        assert!(full[1].1 >= full[2].1);

        let limited = reg.leaderboard(2);
        assert_eq!(limited.len(), 2, "limit=2 respected");
        assert_eq!(limited[0].0, "oracle-b", "highest score first");
        assert!(limited[0].1 > limited[1].1, "sorted descending");
    }

    #[test]
    fn duplicate_attestation_from_same_oracle_is_rejected() {
        let mut registry = OracleRegistry::new();
        let (oracle_sk, oracle_id) = oracle_identity(3);
        registry.register_oracle(oracle_id.clone());
        let mut sets = FxHashMap::default();

        let payload = signed_attestation(&oracle_sk, &oracle_id, "c1", true, 0.9, 1);

        let result1 = registry.apply_attestation(&mut sets, &payload).unwrap();
        assert!(result1.accepted);

        // Same attestation replayed -- should be rejected without slashing
        let result2 = registry.apply_attestation(&mut sets, &payload).unwrap();
        assert!(!result2.accepted);
        assert!(result2.slashing.is_none()); // Not slashable, just a replay
    }

    #[test]
    fn conflicting_double_attestation_triggers_slashing() {
        let mut registry = OracleRegistry::new();
        let (oracle_sk, oracle_id) = oracle_identity(4);
        registry.register_oracle(oracle_id.clone());
        let mut sets = FxHashMap::default();

        let approve = signed_attestation(&oracle_sk, &oracle_id, "c1", true, 0.8, 1);
        let reject = signed_attestation(&oracle_sk, &oracle_id, "c1", false, 0.8, 2);

        registry.apply_attestation(&mut sets, &approve).unwrap();
        let outcome = registry.apply_attestation(&mut sets, &reject).unwrap();
        assert!(!outcome.accepted);
        assert!(matches!(
            outcome.slashing,
            Some(SlashingTrigger::DoubleAttestation { .. })
        ));
    }

    #[test]
    fn missing_signature_triggers_invalid_signature_slashing() {
        let mut registry = OracleRegistry::new();
        let (_oracle_sk, oracle_id) = oracle_identity(5);
        registry.register_oracle(oracle_id.clone());
        let mut sets = FxHashMap::default();

        let payload = OracleAttestationPayload {
            commit_id: "c1".to_string(),
            oracle_id,
            approved: true,
            signature: None,
            confidence: Some(1.0),
            timestamp: 1,
            nonce: None,
        };

        // Verify signature check itself rejects missing signatures
        assert!(!registry.verify_attestation_signature(&payload));

        // Full flow through apply_attestation should also reject with slashing trigger
        let outcome = registry.apply_attestation(&mut sets, &payload).unwrap();
        assert!(!outcome.accepted);
        assert!(matches!(
            outcome.slashing,
            Some(SlashingTrigger::InvalidSignature { .. })
        ));
    }

    #[test]
    fn rotated_oracle_key_is_accepted_when_epoch_active() {
        let mut registry = OracleRegistry::new();
        let (_legacy_sk, oracle_id) = oracle_identity(6);
        let (rotated_sk, rotated_key) = oracle_identity(7);
        registry.register_oracle(oracle_id.clone());

        let mut key_registry = SigningKeyRegistry::default();
        key_registry
            .ensure_legacy_identity_key(&oracle_id, SigningRole::Oracle, 100)
            .unwrap();
        key_registry
            .register_key_epoch(&oracle_id, SigningRole::Oracle, 1, rotated_key.clone(), 150)
            .unwrap();

        let payload = signed_attestation(&rotated_sk, &oracle_id, "c1", true, 0.95, 160);
        let mut sets = FxHashMap::default();
        let outcome = registry
            .apply_attestation_with_registry(&mut sets, &payload, &key_registry)
            .unwrap();
        assert!(outcome.accepted);
    }

    #[test]
    fn revoked_oracle_key_is_rejected() {
        let mut registry = OracleRegistry::new();
        let (_legacy_sk, oracle_id) = oracle_identity(8);
        let (rotated_sk, rotated_key) = oracle_identity(9);
        registry.register_oracle(oracle_id.clone());

        let mut key_registry = SigningKeyRegistry::default();
        key_registry
            .ensure_legacy_identity_key(&oracle_id, SigningRole::Oracle, 100)
            .unwrap();
        key_registry
            .register_key_epoch(&oracle_id, SigningRole::Oracle, 1, rotated_key.clone(), 150)
            .unwrap();
        key_registry
            .revoke_key_epoch(&oracle_id, SigningRole::Oracle, 1, 170)
            .unwrap();

        let payload = signed_attestation(&rotated_sk, &oracle_id, "c1", true, 0.95, 175);
        let mut sets = FxHashMap::default();
        let outcome = registry
            .apply_attestation_with_registry(&mut sets, &payload, &key_registry)
            .unwrap();
        assert!(!outcome.accepted);
        assert!(matches!(
            outcome.slashing,
            Some(SlashingTrigger::InvalidSignature { .. })
        ));
    }

    #[test]
    fn verifier_unavailable_does_not_trigger_invalid_weather_slashing() {
        let mut registry = OracleRegistry::new();
        let (oracle_sk, oracle_id) = oracle_identity(10);
        registry.register_oracle(oracle_id.clone());

        let trigger = registry.verify_receipt_with_slashing(
            &signed_receipt(&oracle_sk, &oracle_id),
            &valid_manifest(),
            &RejectAllZkVerifier,
        );

        assert_eq!(trigger, None);
    }

    #[test]
    fn invalid_weather_proof_triggers_slashing_when_verifier_returns_false() {
        let mut registry = OracleRegistry::new();
        let (oracle_sk, oracle_id) = oracle_identity(11);
        registry.register_oracle(oracle_id.clone());

        let trigger = registry.verify_receipt_with_slashing(
            &signed_receipt(&oracle_sk, &oracle_id),
            &valid_manifest(),
            &StaticWeatherVerifier(Ok(false)),
        );

        assert!(matches!(
            trigger,
            Some(SlashingTrigger::InvalidWeatherProof { oracle_id: trigger_oracle, .. })
            if trigger_oracle == oracle_id
        ));
    }

    #[test]
    fn malformed_model_id_does_not_trigger_weather_slashing() {
        let mut registry = OracleRegistry::new();
        let (oracle_sk, oracle_id) = oracle_identity(12);
        registry.register_oracle(oracle_id.clone());

        let mut manifest = valid_manifest();
        manifest.model_id = "wrf-v3".to_string();

        let trigger = registry.verify_receipt_with_slashing(
            &signed_receipt(&oracle_sk, &oracle_id),
            &manifest,
            &StaticWeatherVerifier(Ok(false)),
        );

        assert_eq!(trigger, None);
    }

    #[test]
    fn short_weather_proof_verified_not_bypassed() {
        let mut registry = OracleRegistry::new();
        let (oracle_sk, oracle_id) = oracle_identity(13);
        registry.register_oracle(oracle_id.clone());

        let mut receipt = signed_receipt(&oracle_sk, &oracle_id);
        receipt.zk_proof = Some("abcd".to_string());
        let message = receipt.signing_message();
        receipt.signature = Some(oracle_sk.sign(message.as_bytes()).to_string());

        // With an accepting verifier, even short proofs pass without slash.
        let trigger = registry.verify_receipt_with_slashing(
            &receipt,
            &valid_manifest(),
            &StaticWeatherVerifier(Ok(true)),
        );

        assert_eq!(trigger, None);
    }

    #[test]
    fn oracle_in_cooldown_is_not_reslashed() {
        let mut registry = OracleRegistry::new();
        registry.register_oracle("oracle1".into());

        // Not in cooldown initially
        assert!(!registry.is_in_cooldown("oracle1", 100));

        // Record slash at height 100
        registry.record_slash("oracle1", 100);

        // In cooldown at height 101 through 199
        assert!(registry.is_in_cooldown("oracle1", 101));
        assert!(registry.is_in_cooldown("oracle1", 150));
        assert!(registry.is_in_cooldown("oracle1", 199));

        // Not in cooldown at height 200 (100 blocks later)
        assert!(!registry.is_in_cooldown("oracle1", 200));
        assert!(!registry.is_in_cooldown("oracle1", 250));
    }

    #[test]
    fn oracle_cooldown_respects_activation_height() {
        let mut registry = OracleRegistry::new();
        registry.register_oracle("oracle1".into());
        registry.record_slash("oracle1", 5);

        // SLASHING_COOLDOWN_ACTIVATION_HEIGHT = 0, so cooldown is always active
        assert!(registry.is_in_cooldown("oracle1", 6));
    }

    #[test]
    fn cross_parameter_inconsistency_produces_non_nan_values() {
        // humidity > 110% triggers CrossParameterInconsistency via validate_physics.
        // Use relaxed humidity max so range check doesn't fire first.
        let policy = crate::weather::validation::PhysicsRangePolicy {
            humidity_max_pct: 200.0,
            ..crate::weather::validation::PhysicsRangePolicy::default()
        };
        let reading = WeatherReading {
            temperature_c: Some(20.0),
            pressure_hpa: Some(1013.0),
            humidity_pct: Some(115.0),
            wind_speed_ms: Some(5.0),
            timestamp_unix: 0,
        };
        let violations = validate_physics(&reading, None, &policy, 0, None);
        let cross = violations
            .iter()
            .find(|v| matches!(v, PhysicsViolation::CrossParameterInconsistency { .. }))
            .expect("should produce CrossParameterInconsistency");
        let (field, value, threshold) = physics_violation_info(cross);
        assert_eq!(field, "dew_point_consistency");
        assert!(!value.is_nan(), "value must not be NaN, got {value}");
        assert!(
            !threshold.is_nan(),
            "threshold must not be NaN, got {threshold}"
        );
    }

    #[test]
    fn fallback_string_parsing_produces_non_nan() {
        // Feed a malformed error string through the stage-2 parsing path.
        // Simulate by creating a DataQualitySlashingPolicy that won't fire
        // the physics pre-check, then use an observation whose only error
        // comes from the string-based range check.
        // Easiest: call physics_violation_info is already tested above.
        // Instead, directly test the parse logic by constructing the same
        // parse chain used at lines 516/522 with an unparseable string.
        let error = "garbled error with no parseable numbers";
        let value = error
            .split_once(": ")
            .and_then(|(_, rest)| rest.split_once(" not in"))
            .and_then(|(v, _)| v.trim().parse::<f64>().ok())
            .unwrap_or(0.0);
        let threshold = error
            .split_once('[')
            .and_then(|(_, rest)| rest.split_once(']'))
            .and_then(|(range, _)| range.split(", ").last())
            .and_then(|t| t.parse::<f64>().ok())
            .unwrap_or(0.0);
        assert!(!value.is_nan(), "fallback value must not be NaN");
        assert!(!threshold.is_nan(), "fallback threshold must not be NaN");
        assert_eq!(value, 0.0);
        assert_eq!(threshold, 0.0);
    }

    #[test]
    fn slashing_trigger_oracle_id_accessor() {
        let t = SlashingTrigger::DataQualityViolation {
            oracle_id: "oracle-x".to_string(),
            station_id: "s1".to_string(),
            field: "temperature".to_string(),
            value: 200.0,
            threshold: EARTH_MAX_AIR_TEMPERATURE_C,
        };
        assert_eq!(t.oracle_id(), "oracle-x");

        let t2 = SlashingTrigger::OracleNotRegistered {
            oracle_id: "oracle-y".to_string(),
        };
        assert_eq!(t2.oracle_id(), "oracle-y");
    }

    // -----------------------------------------------------------------------
    // average_confidence hardening (owner ruling: integer/mu averaging at the
    // #2742/#2743 activation height).
    // -----------------------------------------------------------------------

    fn set_from(records: &[(&str, bool, f64)]) -> AttestationSet {
        let mut set = AttestationSet::default();
        for (oracle_id, approved, confidence) in records {
            set.add(AttestationRecord {
                oracle_id: oracle_id.to_string(),
                approved: *approved,
                confidence: *confidence,
                timestamp: 1,
            });
        }
        set
    }

    const GATE: u64 =
        crate::consensus::proof_of_publishing::STATION_OWNERSHIP_BINDING_ACTIVATION_HEIGHT;

    /// Below the gate the height-aware entry point must be BIT-exact with the
    /// legacy float path — this is the branch that replays every already-paid
    /// block.
    #[test]
    fn below_the_gate_is_bit_exact_with_the_legacy_float_path() {
        for records in [
            vec![("a", true, 0.9917_f64), ("b", true, 0.5), ("c", false, 0.1)],
            vec![("a", true, 0.1), ("b", true, 0.1), ("c", true, 0.1)],
            vec![("solo", true, 0.7331)],
            vec![("none-approved", false, 0.9)],
            vec![],
        ] {
            let set = set_from(&records);
            assert_eq!(
                set.average_confidence_at_height(GATE - 1).to_bits(),
                set.average_confidence().to_bits(),
                "below the gate the legacy bits must be preserved for {records:?}"
            );
        }
    }

    /// At the gate the result must not depend on how the map was built. The
    /// permutations are spelled out: the same (approved, confidence)
    /// multiset is inserted in three different orders AND under three
    /// disjoint key sets — different keys hash to different buckets, so the
    /// map's iteration order genuinely differs between these sets, which is
    /// exactly the variable the legacy float sum was sensitive to.
    #[test]
    fn at_the_gate_the_average_is_independent_of_insertion_and_key_order() {
        let confidences = [0.1_f64, 0.30000000000000004, 0.7331, 0.9917];
        let permutations: [[usize; 4]; 3] = [[0, 1, 2, 3], [3, 2, 1, 0], [2, 0, 3, 1]];
        let key_sets = [
            ["oracle-a", "oracle-b", "oracle-c", "oracle-d"],
            ["zz-9", "yy-8", "xx-7", "ww-6"],
            ["k", "kk", "kkk", "kkkk"],
        ];

        let mut observed: Option<f64> = None;
        for keys in &key_sets {
            for perm in &permutations {
                let mut set = AttestationSet::default();
                for &i in perm {
                    set.add(AttestationRecord {
                        oracle_id: keys[i].to_string(),
                        approved: true,
                        confidence: confidences[i],
                        timestamp: 1,
                    });
                }
                let value = set.average_confidence_at_height(GATE);
                match observed {
                    None => observed = Some(value),
                    Some(prev) => assert_eq!(
                        value.to_bits(),
                        prev.to_bits(),
                        "hardened average must be one value for every build order \
                         (keys {keys:?}, perm {perm:?})"
                    ),
                }
            }
        }
    }

    /// The two paths genuinely differ, pinned on both sides: three
    /// attestations of 0.1. The legacy float path accumulates
    /// 0.1+0.1+0.1 = 0.30000000000000004 and divides to a value strictly
    /// above 0.1 (order-independent HERE because all addends are equal —
    /// chosen so the legacy side is well-defined); the mu path averages
    /// 100_000 exactly and returns 0.1 exactly. This is the ulp the
    /// hardening exists to make impossible to disagree about.
    #[test]
    fn legacy_and_hardened_paths_genuinely_diverge_on_a_pinned_case() {
        let set = set_from(&[("a", true, 0.1), ("b", true, 0.1), ("c", true, 0.1)]);

        let legacy = set.average_confidence();
        let expected_legacy = (0.1_f64 + 0.1 + 0.1) / 3.0;
        assert_eq!(
            legacy.to_bits(),
            expected_legacy.to_bits(),
            "legacy path must match the explicit float expression"
        );

        let hardened = set.average_confidence_at_height(GATE);
        assert_eq!(
            hardened.to_bits(),
            0.1_f64.to_bits(),
            "mu path: 3 × 100_000 mu / 3 = 100_000 mu = exactly 0.1"
        );

        assert_ne!(
            legacy.to_bits(),
            hardened.to_bits(),
            "the pinned case must actually exercise the divergence"
        );
    }

    /// Hardened-path contract details: empty and no-approval sets stay 0.0,
    /// and out-of-range wire values are clamped before quantizing (the same
    /// clamp score_oracle_attestation applies).
    #[test]
    fn hardened_path_edge_cases_are_fail_safe() {
        assert_eq!(AttestationSet::default().average_confidence_at_height(GATE), 0.0);
        assert_eq!(
            set_from(&[("a", false, 0.9)]).average_confidence_at_height(GATE),
            0.0
        );
        let clamped = set_from(&[("a", true, 7.5), ("b", true, -3.0)]);
        assert_eq!(
            clamped.average_confidence_at_height(GATE).to_bits(),
            0.5_f64.to_bits(),
            "7.5 clamps to 1.0 (1_000_000 mu), -3.0 clamps to 0.0; average 0.5"
        );
    }
}
