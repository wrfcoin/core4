#![deny(missing_docs)]
#![warn(clippy::unwrap_used)]

//! Reputation scoring engine shared across WRFCoin crates.

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const HISTORY_LIMIT: usize = 200;

/// Fixed-point scale for reputation scores.
/// Score 0.0 → 0; score 100.0 → 100 * SCORE_SCALE (= 100_000_000).
pub const SCORE_SCALE: u64 = 1_000_000;

/// Error type returned by reputation operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
pub enum ReputationError {
    /// Oracle identifier was empty.
    #[error("oracle_id is required")]
    MissingOracleId,
    /// Decay half-life must be non-zero.
    #[error("inactivity_half_life_ticks must be >= 1")]
    InvalidHalfLife,
}

/// Normalized contributor role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContributorRole {
    /// Data collector role.
    Collector,
    /// Analysis role.
    Analyzer,
    /// Data keeper role.
    Keeper,
    /// Carbon offsetter role.
    Offsetter,
    /// Oracle node role.
    Node,
    /// Development contributor role.
    Coder,
    /// Generic user role.
    User,
}

/// Reputation tier labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReputationTier {
    /// Entry tier.
    Bronze,
    /// Tier reached at score >= 25.
    Silver,
    /// Tier reached at score >= 50.
    Gold,
    /// Tier reached at score >= 75.
    Platinum,
}

/// Deterministic u64 fixed-point reputation score.
/// Value range: [0, 100 * SCORE_SCALE]. No f64 arithmetic on the consensus path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReputationScore(pub u64);

impl ReputationScore {
    /// Returns the bonus multiplier in fixed-point (SCORE_SCALE = 1.0x, 2*SCORE_SCALE = 2.0x).
    /// Formula: SCORE_SCALE + self.0 / 100
    /// Range: [1_000_000, 2_000_000] for score in [0, 100_000_000].
    pub fn bonus_multiplier_u64(&self) -> u64 {
        SCORE_SCALE + self.0 / 100
    }

    /// Score as an integer in [0, 100] (truncates fixed-point fraction).
    pub fn as_integer(&self) -> u64 {
        self.0 / SCORE_SCALE
    }
}

/// Component-level reputation values (u64 fixed-point, range [0, 100 * SCORE_SCALE]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReputationComponentScores {
    /// Data quality component.
    pub data_quality: u64,
    /// Consistency/uptime component.
    pub consistency: u64,
    /// Verification accuracy component.
    pub verification_accuracy: u64,
    /// Community contribution component.
    pub community_contribution: u64,
    /// Tenure component.
    pub tenure: u64,
}

/// Score signal payload applied during event ingestion (f64 external API, range [0, 100]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReputationSignalScores {
    /// Data quality signal score.
    pub data_quality: f64,
    /// Uptime/consistency signal score.
    pub uptime: f64,
    /// Verification signal score.
    pub verification_accuracy: f64,
    /// Community signal score.
    pub community_contribution: f64,
}

/// Historical event entry retained in the contributor record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReputationHistoryEntry {
    /// Source event type label.
    pub event_type: String,
    /// Score signals for this event.
    pub score_signals: ReputationSignalScores,
    /// Free-form reason/metadata payload.
    pub details: Option<String>,
    /// Event timestamp/tick.
    pub at: u64,
}

/// Full contributor reputation record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContributorReputationRecord {
    /// Oracle/contributor identifier.
    pub oracle_id: String,
    /// Current role.
    pub role: ContributorRole,
    /// Composite score (u64 fixed-point).
    pub composite_score: u64,
    /// Component scores.
    pub component_scores: ReputationComponentScores,
    /// Current tier.
    pub tier: ReputationTier,
    /// Reward multiplier derived from score (u64 fixed-point; SCORE_SCALE = 1.0x).
    pub bonus_multiplier: u64,
    /// Badge labels.
    pub badges: Vec<String>,
    /// Limited historical event records.
    pub history: Vec<ReputationHistoryEntry>,
    /// Last contribution timestamp/tick.
    pub last_contribution_at: Option<u64>,
    /// Last update timestamp/tick.
    pub last_updated: u64,
}

/// High-level event format expected by lane #552.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReputationEvent {
    /// Oracle/contributor identifier.
    pub oracle_id: String,
    /// Positive or negative score delta (raw units; 1 unit = SCORE_SCALE internally).
    pub delta: i32,
    /// Human-readable reason.
    pub reason: String,
    /// Event timestamp/tick.
    pub timestamp: u64,
}

/// Rich signal event format mirroring backend TS scoring inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReputationSignal {
    /// Oracle/contributor identifier.
    pub oracle_id: String,
    /// Contributor role.
    pub role: ContributorRole,
    /// Event type label.
    pub event_type: String,
    /// Data quality score signal (f64, range [0, 100]).
    pub data_quality: f64,
    /// Uptime score signal (f64, range [0, 100]).
    pub uptime: f64,
    /// Verification accuracy score signal (f64, range [0, 100]).
    pub verification_accuracy: f64,
    /// Community contribution score signal (f64, range [0, 100]).
    pub community_contribution: f64,
    /// Optional detail text.
    pub details: Option<String>,
    /// Event timestamp/tick. `0` is allowed.
    pub timestamp: u64,
}

/// Configurable decay options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReputationEngineOptions {
    /// Inactivity grace period before decay starts.
    pub inactivity_grace_ticks: u64,
    /// Half-life applied after grace period.
    pub inactivity_half_life_ticks: u64,
}

impl Default for ReputationEngineOptions {
    fn default() -> Self {
        Self {
            inactivity_grace_ticks: 30,
            inactivity_half_life_ticks: 30,
        }
    }
}

/// In-memory reputation engine with deterministic scoring/decay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReputationEngine {
    options: ReputationEngineOptions,
    records: FxHashMap<String, ContributorReputationRecord>,
}

impl ReputationEngine {
    /// Constructs a new engine with explicit options.
    pub fn with_options(options: ReputationEngineOptions) -> Result<Self, ReputationError> {
        if options.inactivity_half_life_ticks == 0 {
            return Err(ReputationError::InvalidHalfLife);
        }
        Ok(Self {
            options,
            records: FxHashMap::default(),
        })
    }

    /// Seeds an initial record for callers that already have a score.
    ///
    /// `score` is a raw score in \[0, 100\]; values above 100 are clamped.
    pub fn seed_score(
        &mut self,
        oracle_id: &str,
        role: ContributorRole,
        score: u64,
        timestamp: u64,
    ) -> Result<(), ReputationError> {
        validate_oracle_id(oracle_id)?;
        let seeded_u64 = score.min(100) * SCORE_SCALE;
        let mut record = self.build_default(oracle_id, role, timestamp);
        record.component_scores = ReputationComponentScores {
            data_quality: seeded_u64,
            consistency: seeded_u64,
            verification_accuracy: seeded_u64,
            community_contribution: seeded_u64,
            tenure: seeded_u64,
        };
        record.composite_score = seeded_u64;
        record.tier = compute_tier(seeded_u64);
        record.bonus_multiplier = ReputationScore(seeded_u64).bonus_multiplier_u64();
        record.badges = compute_badges(record.tier, record.component_scores, seeded_u64);
        self.records.insert(oracle_id.to_string(), record);
        Ok(())
    }

    /// Applies a rich signal event and returns the updated contributor record.
    pub fn apply_signal(
        &mut self,
        signal: ReputationSignal,
    ) -> Result<ContributorReputationRecord, ReputationError> {
        validate_oracle_id(&signal.oracle_id)?;
        let existing = self
            .records
            .get(&signal.oracle_id)
            .cloned()
            .unwrap_or_else(|| {
                self.build_default(&signal.oracle_id, signal.role, signal.timestamp)
            });
        let decayed = self.apply_decay_record(existing, signal.timestamp);

        // Clamp f64 signals to [0, 100] for history storage.
        let normalized = ReputationSignalScores {
            data_quality: signal.data_quality.clamp(0.0, 100.0),
            uptime: signal.uptime.clamp(0.0, 100.0),
            verification_accuracy: signal.verification_accuracy.clamp(0.0, 100.0),
            community_contribution: signal.community_contribution.clamp(0.0, 100.0),
        };

        // Convert clamped f64 signals to u64 fixed-point for component score update.
        let dq_u64 = f64_signal_to_u64(normalized.data_quality);
        let up_u64 = f64_signal_to_u64(normalized.uptime);
        let va_u64 = f64_signal_to_u64(normalized.verification_accuracy);
        let cc_u64 = f64_signal_to_u64(normalized.community_contribution);

        let mut updated = decayed;
        updated.role = signal.role;
        updated.component_scores = ReputationComponentScores {
            data_quality: smooth(updated.component_scores.data_quality, dq_u64),
            consistency: smooth(updated.component_scores.consistency, up_u64),
            verification_accuracy: smooth(
                updated.component_scores.verification_accuracy,
                va_u64,
            ),
            community_contribution: smooth(
                updated.component_scores.community_contribution,
                cc_u64,
            ),
            tenure: compute_tenure_score(updated.history.len() + 1),
        };

        let mut history = updated.history;
        if history.len() >= HISTORY_LIMIT {
            let start = history.len() + 1 - HISTORY_LIMIT;
            history = history.split_off(start);
        }
        history.push(ReputationHistoryEntry {
            event_type: signal.event_type,
            score_signals: normalized,
            details: signal.details,
            at: signal.timestamp,
        });
        updated.history = history;
        updated.last_contribution_at = Some(signal.timestamp);
        updated.last_updated = signal.timestamp;
        updated.composite_score = compute_composite(updated.component_scores);
        updated.tier = compute_tier(updated.composite_score);
        updated.bonus_multiplier = ReputationScore(updated.composite_score).bonus_multiplier_u64();
        updated.badges = compute_badges(
            updated.tier,
            updated.component_scores,
            updated.composite_score,
        );

        self.records
            .insert(signal.oracle_id.clone(), updated.clone());
        Ok(updated)
    }

    /// Applies a delta event and returns the resulting score.
    pub fn apply_event(
        &mut self,
        event: ReputationEvent,
    ) -> Result<ReputationScore, ReputationError> {
        validate_oracle_id(&event.oracle_id)?;
        if !self.records.contains_key(&event.oracle_id) {
            self.records.insert(
                event.oracle_id.clone(),
                self.build_default(&event.oracle_id, ContributorRole::User, event.timestamp),
            );
        }
        let existing = self
            .records
            .get(&event.oracle_id)
            .cloned()
            .expect("record is inserted above");
        let mut updated = self.apply_decay_record(existing, event.timestamp);
        let current = updated.composite_score;
        let next = if event.delta >= 0 {
            current.saturating_add(event.delta as u64 * SCORE_SCALE)
        } else {
            current.saturating_sub(event.delta.unsigned_abs() as u64 * SCORE_SCALE)
        };
        let next = next.min(100 * SCORE_SCALE);

        updated.composite_score = next;
        updated.component_scores = ReputationComponentScores {
            data_quality: next,
            consistency: next,
            verification_accuracy: next,
            community_contribution: next,
            tenure: compute_tenure_score(updated.history.len() + 1),
        };
        updated.tier = compute_tier(next);
        updated.bonus_multiplier = ReputationScore(next).bonus_multiplier_u64();
        updated.badges = compute_badges(updated.tier, updated.component_scores, next);
        updated.last_contribution_at = Some(event.timestamp);
        updated.last_updated = event.timestamp;

        let mut history = updated.history;
        if history.len() >= HISTORY_LIMIT {
            history.remove(0);
        }
        // Store signal scores in human-readable [0, 100] range for history.
        let score_f64 = next as f64 / SCORE_SCALE as f64;
        history.push(ReputationHistoryEntry {
            event_type: event.reason,
            score_signals: ReputationSignalScores {
                data_quality: score_f64,
                uptime: score_f64,
                verification_accuracy: score_f64,
                community_contribution: score_f64,
            },
            details: None,
            at: event.timestamp,
        });
        updated.history = history;

        self.records.insert(event.oracle_id, updated);
        Ok(ReputationScore(next))
    }

    /// Returns the current score for an oracle if present.
    pub fn score_for(&self, oracle_id: &str) -> Option<ReputationScore> {
        self.records
            .get(oracle_id)
            .map(|record| ReputationScore(record.composite_score))
    }

    /// Returns the full record for an oracle if present.
    pub fn record_for(&self, oracle_id: &str) -> Option<&ContributorReputationRecord> {
        self.records.get(oracle_id)
    }

    /// Applies inactivity decay to every known record.
    pub fn decay_scores(&mut self, current_height: u64) {
        let keys: Vec<String> = self.records.keys().cloned().collect();
        for key in keys {
            if let Some(existing) = self.records.get(&key).cloned() {
                let decayed = self.apply_decay_record(existing, current_height);
                self.records.insert(key, decayed);
            }
        }
    }

    fn build_default(
        &self,
        oracle_id: &str,
        role: ContributorRole,
        now: u64,
    ) -> ContributorReputationRecord {
        ContributorReputationRecord {
            oracle_id: oracle_id.to_string(),
            role,
            composite_score: 0,
            component_scores: ReputationComponentScores::default(),
            tier: ReputationTier::Bronze,
            bonus_multiplier: SCORE_SCALE, // 1.0x
            badges: vec!["Bronze Contributor".to_string()],
            history: Vec::new(),
            last_contribution_at: None,
            last_updated: now,
        }
    }

    fn apply_decay_record(
        &self,
        mut record: ContributorReputationRecord,
        now: u64,
    ) -> ContributorReputationRecord {
        let Some(last_contribution) = record.last_contribution_at else {
            record.last_updated = now;
            return record;
        };

        let inactive = now.saturating_sub(last_contribution);
        if inactive <= self.options.inactivity_grace_ticks {
            record.last_updated = now;
            return record;
        }

        let components = record.component_scores;
        record.component_scores = ReputationComponentScores {
            data_quality: decay_score_u64(
                components.data_quality,
                inactive,
                self.options.inactivity_grace_ticks,
                self.options.inactivity_half_life_ticks,
            ),
            consistency: decay_score_u64(
                components.consistency,
                inactive,
                self.options.inactivity_grace_ticks,
                self.options.inactivity_half_life_ticks,
            ),
            verification_accuracy: decay_score_u64(
                components.verification_accuracy,
                inactive,
                self.options.inactivity_grace_ticks,
                self.options.inactivity_half_life_ticks,
            ),
            community_contribution: decay_score_u64(
                components.community_contribution,
                inactive,
                self.options.inactivity_grace_ticks,
                self.options.inactivity_half_life_ticks,
            ),
            tenure: components.tenure,
        };
        record.composite_score = compute_composite(record.component_scores);
        record.tier = compute_tier(record.composite_score);
        record.bonus_multiplier = ReputationScore(record.composite_score).bonus_multiplier_u64();
        record.badges =
            compute_badges(record.tier, record.component_scores, record.composite_score);
        record.last_updated = now;
        record
    }
}

fn validate_oracle_id(oracle_id: &str) -> Result<(), ReputationError> {
    if oracle_id.trim().is_empty() {
        return Err(ReputationError::MissingOracleId);
    }
    Ok(())
}

/// Convert a f64 signal in [0, 100] to u64 fixed-point [0, 100*SCORE_SCALE].
fn f64_signal_to_u64(v: f64) -> u64 {
    let clamped = v.clamp(0.0, 100.0);
    (clamped * SCORE_SCALE as f64).round() as u64
}

/// Exponential moving average: 70% previous + 30% incoming (pure integer arithmetic).
fn smooth(previous: u64, incoming: u64) -> u64 {
    previous * 7 / 10 + incoming * 3 / 10
}

/// Tenure score from event count: 365+ events → full score; linear otherwise.
fn compute_tenure_score(event_count: usize) -> u64 {
    if event_count >= 365 {
        return 100 * SCORE_SCALE;
    }
    (event_count as u64 * 100 * SCORE_SCALE / 365).min(100 * SCORE_SCALE)
}

/// Weighted composite: data_quality 30%, consistency 25%, verification_accuracy 25%,
/// community_contribution 10%, tenure 10%.
fn compute_composite(c: ReputationComponentScores) -> u64 {
    (c.data_quality * 30
        + c.consistency * 25
        + c.verification_accuracy * 25
        + c.community_contribution * 10
        + c.tenure * 10)
        / 100
}

/// Map a u64 fixed-point score to a reputation tier.
pub fn compute_tier(score: u64) -> ReputationTier {
    if score >= 75 * SCORE_SCALE {
        ReputationTier::Platinum
    } else if score >= 50 * SCORE_SCALE {
        ReputationTier::Gold
    } else if score >= 25 * SCORE_SCALE {
        ReputationTier::Silver
    } else {
        ReputationTier::Bronze
    }
}

/// Integer approximation of exponential decay using halving.
///
/// After `grace` ticks of inactivity, score is halved every `half_life` ticks.
/// Fractional halvings use linear interpolation between consecutive powers of two.
fn decay_score_u64(score: u64, ticks_inactive: u64, grace: u64, half_life: u64) -> u64 {
    if ticks_inactive <= grace {
        return score;
    }
    let excess = ticks_inactive - grace;
    let halvings = excess / half_life;
    let remainder = excess % half_life;
    let after_halvings = if halvings >= 64 { 0 } else { score >> halvings };
    // Fractional halving: linear interpolation between halvings.
    // factor = (2*half_life - remainder) / (2*half_life)
    let num = 2 * half_life - remainder;
    let den = 2 * half_life;
    after_halvings * num / den
}

fn compute_badges(
    tier: ReputationTier,
    component_scores: ReputationComponentScores,
    composite_score: u64,
) -> Vec<String> {
    let mut badges = Vec::new();
    badges.push(format!("{tier:?} Contributor"));

    if component_scores.data_quality >= 90 * SCORE_SCALE {
        badges.push("Data Quality Guardian".to_string());
    }
    if component_scores.verification_accuracy >= 90 * SCORE_SCALE {
        badges.push("Verification Sentinel".to_string());
    }
    if component_scores.consistency >= 85 * SCORE_SCALE {
        badges.push("Consistency Anchor".to_string());
    }
    if composite_score >= 95 * SCORE_SCALE {
        badges.push("Network Vanguard".to_string());
    }
    badges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_accumulates_with_signal_events() {
        let mut engine = ReputationEngine::default();
        let signal = ReputationSignal {
            oracle_id: "oracle-1".to_string(),
            role: ContributorRole::Collector,
            event_type: "weather_submission".to_string(),
            data_quality: 90.0,
            uptime: 80.0,
            verification_accuracy: 85.0,
            community_contribution: 75.0,
            details: None,
            timestamp: 10,
        };
        let first = engine.apply_signal(signal.clone()).expect("first update");
        let second = engine
            .apply_signal(ReputationSignal {
                timestamp: 11,
                ..signal
            })
            .expect("second update");

        assert!(second.composite_score >= first.composite_score);
        assert_eq!(
            engine.score_for("oracle-1"),
            Some(ReputationScore(second.composite_score))
        );
    }

    #[test]
    fn decay_reduces_scores_after_grace_period() {
        let mut engine = ReputationEngine::with_options(ReputationEngineOptions {
            inactivity_grace_ticks: 10,
            inactivity_half_life_ticks: 10,
        })
        .expect("options");

        engine
            .apply_signal(ReputationSignal {
                oracle_id: "oracle-2".to_string(),
                role: ContributorRole::Analyzer,
                event_type: "verification".to_string(),
                data_quality: 100.0,
                uptime: 100.0,
                verification_accuracy: 100.0,
                community_contribution: 100.0,
                details: None,
                timestamp: 1,
            })
            .expect("seed score");

        let before = engine.score_for("oracle-2").expect("score");
        engine.decay_scores(40);
        let after = engine.score_for("oracle-2").expect("decayed score");
        assert!(after.0 < before.0);
    }

    #[test]
    fn event_floor_and_ceiling_bounds() {
        let mut engine = ReputationEngine::default();
        engine
            .seed_score("oracle-3", ContributorRole::User, 50_u64, 0)
            .expect("seed");

        let down = engine
            .apply_event(ReputationEvent {
                oracle_id: "oracle-3".to_string(),
                delta: -500,
                reason: "penalty".to_string(),
                timestamp: 1,
            })
            .expect("apply negative");
        assert_eq!(down.0, 0);

        let up = engine
            .apply_event(ReputationEvent {
                oracle_id: "oracle-3".to_string(),
                delta: i32::MAX,
                reason: "reward".to_string(),
                timestamp: 2,
            })
            .expect("apply positive");
        assert_eq!(up.0, 100 * SCORE_SCALE);
    }

    #[test]
    fn bonus_multiplier_bounds() {
        assert_eq!(ReputationScore(0).bonus_multiplier_u64(), SCORE_SCALE); // 1.0x
        assert_eq!(
            ReputationScore(100 * SCORE_SCALE).bonus_multiplier_u64(),
            2 * SCORE_SCALE
        ); // 2.0x
        assert_eq!(
            ReputationScore(50 * SCORE_SCALE).bonus_multiplier_u64(),
            SCORE_SCALE + 500_000
        ); // 1.5x
    }

    #[test]
    fn composite_score_deterministic() {
        let c = ReputationComponentScores {
            data_quality: 80 * SCORE_SCALE,
            consistency: 70 * SCORE_SCALE,
            verification_accuracy: 90 * SCORE_SCALE,
            community_contribution: 60 * SCORE_SCALE,
            tenure: 50 * SCORE_SCALE,
        };
        // expected = (80*30 + 70*25 + 90*25 + 60*10 + 50*10) / 100
        //          = (2400 + 1750 + 2250 + 600 + 500) / 100 = 7500 / 100 = 75
        let score = compute_composite(c);
        assert_eq!(score, 75 * SCORE_SCALE);
        assert_eq!(compute_tier(score), ReputationTier::Platinum);
    }
}
