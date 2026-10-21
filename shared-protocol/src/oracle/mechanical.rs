//! Mechanical attestation policy — physics ranges, reference bands, freshness.
//!
//! This is the *decision half* of the oracle attester (#2464): given a
//! [`PendingObservation`] whose commit id has already been verified, decide
//! `approved` and `confidence` from checks a machine can perform without
//! judgment. Deliberately excluded is everything from #724's human-trust
//! territory: operator reputation, model-binding verdicts, station history,
//! cross-station corroboration. Those belong to a later policy layer; an
//! attestation produced here certifies only that the reading is physically
//! plausible, fresh, and (when reference residuals are supplied) consistent
//! with independent gridded references.
//!
//! # What decides `approved` vs what decides `confidence`
//!
//! The chain's quorum counts **approvals** ([`AttestationSet::approvals`]), so
//! `approved` is the payment-gating bit and is held to mechanical *facts*:
//!
//! - the reading parses as a weather observation,
//! - it passes the canonical physics validation
//!   ([`validate_weather_observation`] — the same code the chain itself uses),
//! - its timestamp is inside the freshness window.
//!
//! Reference-band tiering does **not** flip `approved`. That mirrors the
//! documented chain philosophy ([`ReadingQualityTier`]: an `Outlier` is "still
//! accepted on-chain; reward multiplier is minimal" — never hard-rejected
//! pre-mainnet). A residual outside every band therefore degrades
//! `confidence`, and the tier multiplier the chain already publishes
//! ([`ReadingQualityTier::default_reward_multiplier`]) is reused as the
//! degradation factor rather than inventing a second schedule.
//!
//! An evaluation with **no** reference comparison also does not flip
//! `approved` (an unreachable reference must not refuse honest readings),
//! but it is never a silent fallback: the confidence is capped at
//! [`UNCHECKED_CONFIDENCE_CAP`] and the cap is named in the audit trail, so a
//! physics-only attestation is distinguishable on the wire from a
//! reference-checked one (#2746 review HIGH #2 / the owner's reference-scope
//! ruling).
//!
//! [`AttestationSet::approvals`]: crate::oracle::attestation::AttestationSet::approvals

use serde::{Deserialize, Serialize};

use crate::oracle::attester::PendingObservation;
use crate::weather::quality_tier::ReadingQualityTier;
use crate::weather::reference::{ReferenceBands, ReferenceComparison};
use crate::weather::validation::{validate_weather_observation, weather_observation_from_json};

/// Default freshness window: a reading older than this at evaluation time is
/// refused. One hour matches the collector's cadence (readings ship minutes
/// after capture) with generous headroom for mempool dwell.
pub const DEFAULT_MAX_AGE_SECS: i64 = 3_600;

/// Default tolerated forward clock skew. A reading stamped further in the
/// future than this is refused — it cannot be validated against references
/// that do not exist yet, and a large future stamp is the classic replay
/// setup.
pub const DEFAULT_MAX_FUTURE_SKEW_SECS: i64 = 300;

/// The confidence floor the freshness decay converges to at the edge of the
/// window. A reading just inside `max_age_secs` is still mechanically valid,
/// so it approves — but an attester should not claim the same confidence for
/// an hour-old reading as for a live one.
// determinism-ok: exactly-representable constant (0.5 = 2^-1) in attester-side policy; no consensus path executes this module (grep: only the oracle_attester bin and tests call it) — consensus sees only the signed wire confidence, quantized to integer mu (quantize_f64) before reward arithmetic
const FRESHNESS_FLOOR: f64 = 0.5;

/// Ceiling on the confidence of an attestation produced with **no reference
/// comparison** (source unconfigured, fetch failed, or nothing paired inside
/// the window). The unavailability half of the owner's #2746 reference-scope
/// ruling: approval stays physics-based, but "this reading was never checked
/// against an independent local sensor" must be visible on the wire rather
/// than implicit — a fresh unchecked reading and a fresh reference-checked
/// one must never carry the same confidence.
// determinism-ok: exactly-representable constant (0.5 = 2^-1), same attester-side/wire-boundary reasoning as FRESHNESS_FLOOR above
pub const UNCHECKED_CONFIDENCE_CAP: f64 = 0.5;

/// Mechanical validation policy. One instance per attester process; every
/// field is data, so two deployments differ only in configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MechanicalPolicy {
    /// Maximum age (seconds) of a reading at evaluation time.
    pub max_age_secs: i64,
    /// Maximum tolerated forward clock skew (seconds).
    pub max_future_skew_secs: i64,
    /// Residual band thresholds. Defaults to the embedded canonical fixture
    /// (`fixtures/reference_bands.json`) so every attester ships the same
    /// thresholds as the chain's own tier code.
    pub bands: ReferenceBands,
    /// Whether the #3131 gross-outlier guard applies
    /// ([`ReferenceComparison::tier_with_gross_guard`]). `false` is the
    /// pre-activation #3098 fold, byte-for-byte. The service sets this from
    /// the node's report that
    /// [`ConsensusFeature::RewardGrossOutlierGuard`](crate::crypto::chain_binding::ConsensusFeature::RewardGrossOutlierGuard)
    /// is active, never from its own configuration, so honest attesters switch
    /// at the scheduled height together.
    #[serde(default)]
    pub gross_outlier_guard: bool,
    /// Whether the #3285 sea-level pressure pairing applies
    /// ([`pair_station_reference_sea_level`]). `false` is today's raw
    /// `pressure_hpa` pairing, byte-for-byte. Like `gross_outlier_guard`, the
    /// service sets this from the node's report that
    /// [`ConsensusFeature::ReferenceSeaLevelPressure`](crate::crypto::chain_binding::ConsensusFeature::ReferenceSeaLevelPressure)
    /// is active, never from its own configuration.
    ///
    /// [`pair_station_reference_sea_level`]: crate::weather::station_reference::pair_station_reference_sea_level
    #[serde(default)]
    pub sea_level_pressure: bool,
}

impl Default for MechanicalPolicy {
    fn default() -> Self {
        Self {
            max_age_secs: DEFAULT_MAX_AGE_SECS,
            max_future_skew_secs: DEFAULT_MAX_FUTURE_SKEW_SECS,
            bands: ReferenceBands::builtin(),
            gross_outlier_guard: false,
            sea_level_pressure: false,
        }
    }
}

/// The outcome of mechanically evaluating one observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MechanicalVerdict {
    /// Whether the attestation should approve. See the module docs for what
    /// may and may not flip this bit.
    pub approved: bool,
    /// Confidence in `[0, 1]`, suitable for
    /// [`OracleAttestationPayload::confidence`]. `0.0` when refused.
    ///
    /// [`OracleAttestationPayload::confidence`]: crate::blockchain::transaction::OracleAttestationPayload
    // determinism-ok: mirrors the protocol wire type (Option<f64> confidence); computed from documented factors, informational to quorum
    pub confidence: f64,
    /// Worst-variable reference tier. `Good` when no comparison was supplied
    /// — but an unchecked verdict is never a silent free pass: its
    /// `confidence` is capped at [`UNCHECKED_CONFIDENCE_CAP`] and the cap is
    /// recorded in `reasons`.
    pub tier: ReadingQualityTier,
    /// Human-readable audit trail: every check that refused or degraded, in
    /// evaluation order. Empty means a clean full-confidence pass.
    pub reasons: Vec<String>,
}

impl MechanicalVerdict {
    fn refuse(reason: String) -> Self {
        Self {
            approved: false,
            // determinism-ok: exactly-representable literal in an attester-side verdict; becomes wire bytes, never consensus arithmetic
            confidence: 0.0,
            tier: ReadingQualityTier::Good,
            reasons: vec![reason],
        }
    }
}

impl MechanicalPolicy {
    /// Evaluate one observation.
    ///
    /// `comparison` carries residuals against independent references (gridded
    /// products, or a nearby station via
    /// [`pair_station_reference`]) when the deployment has a reference source
    /// configured. `None` — or a comparison that paired nothing — never
    /// refuses, but the resulting attestation is *visibly* physics-only: its
    /// confidence is capped at [`UNCHECKED_CONFIDENCE_CAP`] and the cap is
    /// named in `reasons`. `now_unix` is passed in rather than read from the
    /// clock so the function is a pure, testable decision.
    ///
    /// [`pair_station_reference`]: crate::weather::station_reference::pair_station_reference
    ///
    /// The commit id is NOT re-verified here — [`Attester::attest`] does that
    /// immediately before signing, and doing it in both places would suggest
    /// either one is optional.
    ///
    /// [`Attester::attest`]: crate::oracle::attester::Attester::attest
    pub fn evaluate(
        &self,
        observation: &PendingObservation,
        comparison: Option<&ReferenceComparison>,
        now_unix: i64,
    ) -> MechanicalVerdict {
        self.evaluate_with_unverified(observation, comparison, now_unix, &[])
    }

    /// [`Self::evaluate`] with `unverified` variables: each is scored as an
    /// `Outlier` variable in the tier fold
    /// ([`ReferenceComparison::tier_under_with_unverified`]) and named in
    /// `reasons`. An empty slice is exactly [`Self::evaluate`].
    ///
    /// #3285 owner ruling: once `reference_sea_level_pressure` is active, a
    /// station with a registered elevation that publishes a checkable
    /// `pressure_hpa` without a trustworthy sea-level value has pressure
    /// scored unverified-Outlier, so withholding or corrupting the claim can
    /// never tier better than an honest one.
    pub fn evaluate_with_unverified(
        &self,
        observation: &PendingObservation,
        comparison: Option<&ReferenceComparison>,
        now_unix: i64,
        unverified: &[&str],
    ) -> MechanicalVerdict {
        // 1. Parse. A reading the canonical parser cannot understand cannot
        //    be validated, so it cannot be approved.
        let Some(parsed) = weather_observation_from_json(&observation.data) else {
            return MechanicalVerdict::refuse(
                "data does not parse as a weather observation".to_string(),
            );
        };

        // 2. Physics. Same validator the chain runs; hard errors refuse.
        let validation = validate_weather_observation(&parsed);
        if !validation.valid {
            return MechanicalVerdict::refuse(format!(
                "physics validation failed: {}",
                validation.errors.join("; ")
            ));
        }

        // 3. Freshness. Refusals first, then the decay factor.
        let age_secs = now_unix.saturating_sub(observation.timestamp);
        if observation.timestamp > now_unix.saturating_add(self.max_future_skew_secs) {
            return MechanicalVerdict::refuse(format!(
                "reading timestamp {} is more than {}s in the future of {}",
                observation.timestamp, self.max_future_skew_secs, now_unix
            ));
        }
        if age_secs > self.max_age_secs {
            return MechanicalVerdict::refuse(format!(
                "reading is {age_secs}s old, beyond the {}s freshness window",
                self.max_age_secs
            ));
        }

        let mut reasons = Vec::new();

        // Linear decay from 1.0 (age 0) to FRESHNESS_FLOOR (age == max_age).
        // determinism-ok: informational confidence factor, never consensus input; plain arithmetic over already-bounded values
        let freshness_factor = if age_secs <= 0 {
            // determinism-ok: attester-side decay factor of the value this process signs; consensus parses the resulting wire bytes identically on every node and quantizes them to integer mu before reward arithmetic
            1.0
        } else {
            // determinism-ok: as above — attester-side arithmetic behind the signed-wire boundary; wire_stable_confidence below pins the emitted value to its own parse image
            let fraction = age_secs as f64 / self.max_age_secs as f64;
            // determinism-ok: as above
            1.0 - (1.0 - FRESHNESS_FLOOR) * fraction
        };
        // determinism-ok: audit-trail comparison on the attester-side factor, not consensus input
        if freshness_factor < 1.0 {
            reasons.push(format!(
                "freshness: reading is {age_secs}s old (factor {freshness_factor:.3})"
            ));
        }

        // 4. Reference tier. Worst-variable fold over the supplied residuals;
        //    degrades confidence via the chain's own multiplier schedule,
        //    never flips `approved` (see module docs). A comparison that
        //    paired nothing carries the same information as no comparison, so
        //    both take the unchecked path below.
        //    From the #3131 activation, a lone Outlier beyond its variable's
        //    gross limit is not dropped by the fold.
        let checked = comparison.is_some_and(|c| !c.residuals.is_empty());
        let scored = comparison.filter(|c| !c.residuals.is_empty());
        let tier = if unverified.is_empty() {
            scored
                .map(|c| c.tier_under(&self.bands, self.gross_outlier_guard))
                .unwrap_or(ReadingQualityTier::Good)
        } else {
            let none = ReferenceComparison { residuals: Vec::new() };
            let tier = scored.unwrap_or(&none).tier_under_with_unverified(
                &self.bands,
                self.gross_outlier_guard,
                unverified.len(),
            );
            reasons.push(format!(
                "unverified variable(s) scored Outlier in the tier fold: {}",
                unverified.join(", ")
            ));
            tier
        };
        // determinism-ok: reuses the chain's published multiplier schedule as a confidence factor
        let tier_factor = tier.default_reward_multiplier();
        if tier != ReadingQualityTier::Good {
            reasons.push(format!(
                "reference comparison tier {tier} (factor {tier_factor})"
            ));
        }
        if self.gross_outlier_guard {
            if let Some(gross) = scored.and_then(|c| c.gross_outlier(&self.bands)) {
                reasons.push(format!(
                    "gross outlier guard (#3131): {}/{} residual {:+.2} is beyond its gross limit",
                    gross.source, gross.variable, gross.residual
                ));
            }
        }

        // Soft validation warnings do not refuse, but they belong in the
        // audit trail.
        for warning in &validation.warnings {
            reasons.push(format!("validation warning: {warning}"));
        }

        // determinism-ok: product of two documented factors, each in (0, 1]; informational only
        let mut confidence = (freshness_factor * tier_factor).clamp(0.0, 1.0);

        // 5. The unchecked cap. An attestation produced without any reference
        //    comparison must say so on the wire: its confidence never exceeds
        //    UNCHECKED_CONFIDENCE_CAP, and the audit trail names the cap.
        //    Approval is untouched — an NWS outage must not refuse honest
        //    readings — but "physics-only" is a visible property of the
        //    attestation, never a silent fallback (#2746 HIGH #2).
        if !checked {
            confidence = confidence.min(UNCHECKED_CONFIDENCE_CAP);
            reasons.push(format!(
                "no reference comparison available: physics-only attestation, \
                 confidence capped at {UNCHECKED_CONFIDENCE_CAP}"
            ));
        }

        let confidence = wire_stable_confidence(confidence);

        MechanicalVerdict {
            approved: true,
            confidence,
            tier,
            reasons,
        }
    }
}

/// Make a confidence value survive the node's JSON parse bit-exactly.
///
/// The attestation signing digest hashes `serde_json::to_vec` of the payload
/// (`payload_digest_hex`), and the node recomputes it from the value it
/// PARSED out of the request body. `serde_json`'s default float parsing is
/// not correctly rounded (the `float_roundtrip` feature is off), so a value
/// like `0.9916666666666667` — one hour of linear freshness decay — parses
/// back one ulp different, the digests diverge, and the node rejects a
/// perfectly good signature as "verification failed". Same silent-divergence
/// class as #2713/#2744, one layer down.
///
/// Two steps: quantize to 4 decimals (so emitted confidences are also
/// human-stable), then iterate serialize→parse to a fix-point so the signed
/// value IS the node's parse image. In practice one iteration converges.
// determinism-ok: confidence is informational to quorum; this makes its wire representation stable, it never feeds consensus arithmetic
fn wire_stable_confidence(confidence: f64) -> f64 {
    // determinism-ok: this function EXISTS to remove float nondeterminism at the wire: it quantizes and then fixes the value to its own serde_json parse image, so the bytes the attester signs are bit-identical to what every node parses
    let mut value = ((confidence * 10_000.0).round() / 10_000.0).clamp(0.0, 1.0);
    for _ in 0..4 {
        // determinism-ok: serialization probe of the wire representation, attester-side
        let printed = serde_json::to_string(&value)
            .expect("a finite clamped f64 always serializes");
        // determinism-ok: deliberately mirrors the node's serde_json parse — matching it is the point
        let parsed: f64 = serde_json::from_str(&printed)
            .expect("serde_json parses its own float output");
        if parsed.to_bits() == value.to_bits() {
            break;
        }
        value = parsed;
    }
    // determinism-ok: exactly-representable clamp bounds on the attester-side result
    value.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::reference::ReferenceResidual;

    const NOW: i64 = 1_700_000_000;

    fn fresh_observation() -> PendingObservation {
        PendingObservation {
            commit_id: "unused-here".to_string(),
            station_id: "WX-TEST-1".to_string(),
            nonce: "n1".to_string(),
            timestamp: NOW - 60,
            data: serde_json::json!({
                "temperature_c": 21.5,
                "humidity_pct": 55.0,
                "pressure_hpa": 1013.2,
                "wind_mps": 3.1,
                "rainfall_mm": 0.0
            }),
            contribution_type: "contribution".to_string(),
        }
    }

    fn residual(source: &str, variable: &str, residual: f64) -> ReferenceResidual {
        ReferenceResidual {
            source: source.to_string(),
            variable: variable.to_string(),
            residual,
        }
    }

    #[test]
    fn a_fresh_plausible_reading_with_a_good_reference_approves_at_high_confidence() {
        let verdict = MechanicalPolicy::default().evaluate(
            &fresh_observation(),
            Some(&ReferenceComparison {
                residuals: vec![residual("hrrr", "temperature_c", 0.5)],
            }),
            NOW,
        );
        assert!(verdict.approved, "reasons: {:?}", verdict.reasons);
        assert!(
            verdict.confidence > 0.95,
            "a one-minute-old clean reference-checked reading should be near full \
             confidence, got {}",
            verdict.confidence
        );
        assert_eq!(verdict.tier, ReadingQualityTier::Good);
    }

    /// #2746 HIGH #2, the unavailability half of the owner's ruling: an
    /// attestation produced with NO reference comparison must be visibly
    /// different on the wire from a reference-checked one. Approval stays
    /// physics-based (an NWS outage must not refuse honest readings), but the
    /// confidence is capped and the cap is named in the audit trail —
    /// "silently falls back to physics-only" is the defect being fixed.
    #[test]
    fn an_unchecked_evaluation_is_visibly_capped_never_silently_full_confidence() {
        let policy = MechanicalPolicy::default();
        let verdict = policy.evaluate(&fresh_observation(), None, NOW);
        assert!(verdict.approved, "unavailable reference must not refuse: {:?}", verdict.reasons);
        assert!(
            (verdict.confidence - UNCHECKED_CONFIDENCE_CAP).abs() < 1e-9,
            "a fresh unchecked reading must sit exactly at the cap, got {}",
            verdict.confidence
        );
        assert!(
            verdict.reasons.iter().any(|r| r.contains("no reference comparison")),
            "the cap must be named in the audit trail, got {:?}",
            verdict.reasons
        );
    }

    /// A comparison that paired nothing is the same fact as no comparison —
    /// treating `Some(empty)` as checked would let a caller launder the cap.
    #[test]
    fn an_empty_comparison_is_unchecked_not_a_free_pass() {
        let verdict = MechanicalPolicy::default().evaluate(
            &fresh_observation(),
            Some(&ReferenceComparison { residuals: vec![] }),
            NOW,
        );
        assert!(verdict.approved);
        assert!(
            verdict.confidence <= UNCHECKED_CONFIDENCE_CAP + 1e-9,
            "empty comparison must be capped like None, got {}",
            verdict.confidence
        );
    }

    /// The cap composes with freshness decay by min, not by product — an
    /// hour-old unchecked reading is not punished twice below the floor.
    #[test]
    fn the_unchecked_cap_composes_with_freshness_as_a_ceiling() {
        let policy = MechanicalPolicy::default();
        let mut obs = fresh_observation();
        obs.timestamp = NOW - policy.max_age_secs / 2; // freshness factor 0.75
        let verdict = policy.evaluate(&obs, None, NOW);
        assert!(verdict.approved);
        assert!(
            (verdict.confidence - UNCHECKED_CONFIDENCE_CAP).abs() < 1e-9,
            "cap should be the binding constraint at half-window age, got {}",
            verdict.confidence
        );
    }

    #[test]
    fn a_physically_impossible_reading_is_refused() {
        let mut obs = fresh_observation();
        obs.data["temperature_c"] = serde_json::json!(120.0);
        let verdict = MechanicalPolicy::default().evaluate(&obs, None, NOW);
        assert!(!verdict.approved);
        assert_eq!(verdict.confidence, 0.0);
        assert!(
            verdict.reasons[0].contains("physics validation failed"),
            "got: {:?}",
            verdict.reasons
        );
    }

    #[test]
    fn unparseable_data_is_refused_not_approved_by_default() {
        let mut obs = fresh_observation();
        obs.data = serde_json::json!({"blob": "not weather"});
        let verdict = MechanicalPolicy::default().evaluate(&obs, None, NOW);
        assert!(!verdict.approved);
        assert_eq!(verdict.confidence, 0.0);
    }

    #[test]
    fn a_stale_reading_is_refused() {
        let policy = MechanicalPolicy::default();
        let mut obs = fresh_observation();
        obs.timestamp = NOW - policy.max_age_secs - 1;
        let verdict = policy.evaluate(&obs, None, NOW);
        assert!(!verdict.approved);
        assert!(verdict.reasons[0].contains("freshness window"), "{:?}", verdict.reasons);
    }

    #[test]
    fn a_future_stamped_reading_beyond_skew_is_refused() {
        let policy = MechanicalPolicy::default();
        let mut obs = fresh_observation();
        obs.timestamp = NOW + policy.max_future_skew_secs + 1;
        let verdict = policy.evaluate(&obs, None, NOW);
        assert!(!verdict.approved);
        assert!(verdict.reasons[0].contains("future"), "{:?}", verdict.reasons);
    }

    #[test]
    fn small_future_skew_inside_the_tolerance_is_accepted_at_full_freshness() {
        let policy = MechanicalPolicy::default();
        let mut obs = fresh_observation();
        obs.timestamp = NOW + 30;
        let verdict = policy.evaluate(
            &obs,
            Some(&ReferenceComparison {
                residuals: vec![residual("hrrr", "temperature_c", 0.5)],
            }),
            NOW,
        );
        assert!(verdict.approved);
        assert_eq!(verdict.confidence, 1.0);
    }

    #[test]
    fn confidence_decays_with_age_but_stays_above_the_floor() {
        let policy = MechanicalPolicy::default();
        let mut obs = fresh_observation();
        obs.timestamp = NOW - policy.max_age_secs; // exactly at the window edge
        let verdict = policy.evaluate(&obs, None, NOW);
        assert!(verdict.approved);
        // determinism-ok: exact comparison of a value constructed from the same constants
        assert!(
            (verdict.confidence - FRESHNESS_FLOOR).abs() < 1e-9,
            "edge-of-window confidence should be the floor, got {}",
            verdict.confidence
        );
    }

    #[test]
    fn a_good_band_residual_keeps_full_tier_confidence() {
        let verdict = MechanicalPolicy::default().evaluate(
            &fresh_observation(),
            Some(&ReferenceComparison {
                residuals: vec![residual("hrrr", "temperature_c", 1.0)],
            }),
            NOW,
        );
        assert!(verdict.approved);
        assert_eq!(verdict.tier, ReadingQualityTier::Good);
    }

    /// The chain philosophy pinned: an out-of-band residual degrades
    /// confidence but does not refuse — tier handling is the reward layer's
    /// job, not the approval bit's.
    #[test]
    fn an_outlier_residual_degrades_confidence_without_refusing() {
        let verdict = MechanicalPolicy::default().evaluate(
            &fresh_observation(),
            Some(&ReferenceComparison {
                // hrrr temperature marginal_max is 5.0; 9.0 is Outlier.
                residuals: vec![residual("hrrr", "temperature_c", 9.0)],
            }),
            NOW,
        );
        assert!(verdict.approved, "tier must not flip approved");
        assert_eq!(verdict.tier, ReadingQualityTier::Outlier);
        assert!(
            verdict.confidence <= 0.1 + 1e-9,
            "outlier tier should collapse confidence to the outlier multiplier, got {}",
            verdict.confidence
        );
        assert!(verdict.confidence > 0.0);
    }

    /// The #3131 reference case: 10.4 °C off on temperature, every other
    /// variable within its good band. `nws_station` temperature has
    /// `marginal_max` 6 and `gross_max` 9.
    fn gross_temperature_comparison(temperature_residual: f64) -> ReferenceComparison {
        ReferenceComparison {
            residuals: vec![
                residual("nws_station", "temperature_c", temperature_residual),
                residual("nws_station", "pressure_hpa", -0.2),
                residual("nws_station", "humidity_pct", 2.0),
                residual("nws_station", "wind_speed_ms", -0.2),
            ],
        }
    }

    #[test]
    fn before_activation_a_lone_gross_outlier_keeps_the_3098_degraded_verdict() {
        // Pre-activation is the #3098 fold, byte-for-byte: the same verdict
        // main produced before #3131 (Degraded, 0.5 × fresh, no guard reason).
        let policy = MechanicalPolicy::default();
        assert!(!policy.gross_outlier_guard, "the guard is off until the feature activates");
        let verdict = policy.evaluate(&fresh_observation(), Some(&gross_temperature_comparison(-10.4)), NOW);
        assert!(verdict.approved);
        assert_eq!(verdict.tier, ReadingQualityTier::Degraded);
        assert_eq!(verdict.confidence, 0.4958);
        assert!(!verdict.reasons.iter().any(|r| r.contains("gross")), "{verdict:?}");
    }

    #[test]
    fn after_activation_a_lone_gross_outlier_collapses_to_outlier() {
        let policy = MechanicalPolicy { gross_outlier_guard: true, ..MechanicalPolicy::default() };
        let verdict = policy.evaluate(&fresh_observation(), Some(&gross_temperature_comparison(-10.4)), NOW);
        assert!(verdict.approved, "the guard degrades, it never refuses");
        assert_eq!(verdict.tier, ReadingQualityTier::Outlier);
        assert!(verdict.confidence <= 0.1, "{verdict:?}");
        assert!(
            verdict.reasons.iter().any(|r| r.contains("gross outlier guard") && r.contains("temperature_c")),
            "the audit trail names the gross variable: {verdict:?}"
        );
    }

    #[test]
    fn after_activation_a_near_miss_lone_outlier_stays_degraded() {
        // 8.5 °C is past marginal_max (6) but not past gross_max (9).
        let policy = MechanicalPolicy { gross_outlier_guard: true, ..MechanicalPolicy::default() };
        let verdict = policy.evaluate(&fresh_observation(), Some(&gross_temperature_comparison(8.5)), NOW);
        assert_eq!(verdict.tier, ReadingQualityTier::Degraded);
        assert_eq!(verdict.confidence, 0.4958);
        assert!(!verdict.reasons.iter().any(|r| r.contains("gross")), "{verdict:?}");
    }

    #[test]
    fn after_activation_a_variable_without_a_gross_limit_is_unchanged() {
        // nws_station pressure has no gross_max (bands v3): the live Duluth
        // WS-5000 submits uncorrected station pressure, ~27 hPa below KDLH's
        // sea-level value on every reading. Even far past that, the guard
        // does not fire, and the verdict equals the pre-activation one.
        let comparison = ReferenceComparison {
            residuals: vec![
                residual("nws_station", "temperature_c", 1.2),
                residual("nws_station", "pressure_hpa", -500.0),
                residual("nws_station", "humidity_pct", 3.8),
                residual("nws_station", "wind_speed_ms", -3.9),
            ],
        };
        let before = MechanicalPolicy::default().evaluate(&fresh_observation(), Some(&comparison), NOW);
        let after = MechanicalPolicy { gross_outlier_guard: true, ..MechanicalPolicy::default() }
            .evaluate(&fresh_observation(), Some(&comparison), NOW);
        assert_eq!(before.tier, ReadingQualityTier::Degraded);
        assert_eq!(after, before);
    }

    #[test]
    fn a_marginal_residual_lands_between_the_two() {
        let verdict = MechanicalPolicy::default().evaluate(
            &fresh_observation(),
            Some(&ReferenceComparison {
                // hrrr temperature: good_max 2.0 < 3.5 <= marginal_max 5.0.
                residuals: vec![residual("hrrr", "temperature_c", 3.5)],
            }),
            NOW,
        );
        assert!(verdict.approved);
        assert_eq!(verdict.tier, ReadingQualityTier::Degraded);
        assert!(verdict.confidence < 0.6 && verdict.confidence > 0.3);
    }

    /// The regression that motivated `wire_stable_confidence`: the node
    /// recomputes the signing digest from the confidence it PARSED, and
    /// serde_json's default float parse is not correctly rounded. Every
    /// confidence this policy emits must be its own parse image, or the
    /// attestation is unverifiable on arrival.
    #[test]
    fn every_emitted_confidence_survives_the_json_wire_bit_exactly() {
        let policy = MechanicalPolicy::default();
        for age in [0, 1, 59, 60, 61, 600, 1799, 1800, 3599, 3600] {
            let mut obs = fresh_observation();
            obs.timestamp = NOW - age;
            let verdict = policy.evaluate(&obs, None, NOW);
            let printed = serde_json::to_string(&verdict.confidence).unwrap();
            let parsed: f64 = serde_json::from_str(&printed).unwrap();
            assert_eq!(
                parsed.to_bits(),
                verdict.confidence.to_bits(),
                "confidence {} (age {age}s) does not survive the JSON wire — the node \
                 would reject the attestation as a signature failure",
                verdict.confidence
            );
        }
        // The raw hour-of-decay value is the known-bad case; prove the
        // stabilizer actually had something to fix.
        let raw: f64 = 0.991_666_666_666_666_7;
        let parsed: f64 = serde_json::from_str(&serde_json::to_string(&raw).unwrap()).unwrap();
        assert_ne!(
            parsed.to_bits(),
            raw.to_bits(),
            "serde_json now round-trips this value exactly — the float_roundtrip \
             feature may have been enabled, and wire_stable_confidence can be retired"
        );
    }

    #[test]
    fn the_verdict_confidence_is_always_a_valid_attestation_confidence() {
        // Sweep ages and tiers; every approved verdict must produce a
        // confidence the node accepts ([0, 1]).
        let policy = MechanicalPolicy::default();
        for age in [0, 60, 1800, 3600] {
            for res in [None, Some(1.0), Some(3.5), Some(9.0)] {
                let mut obs = fresh_observation();
                obs.timestamp = NOW - age;
                let comparison = res.map(|r| ReferenceComparison {
                    residuals: vec![residual("hrrr", "temperature_c", r)],
                });
                let verdict = policy.evaluate(&obs, comparison.as_ref(), NOW);
                assert!(
                    (0.0..=1.0).contains(&verdict.confidence),
                    "age {age} residual {res:?} produced out-of-range confidence {}",
                    verdict.confidence
                );
            }
        }
    }

    // ---- #3285: unverified variables ---------------------------------------

    #[test]
    fn unverified_variables_are_scored_outlier_and_empty_is_exactly_evaluate_3285() {
        let policy = MechanicalPolicy::default();
        // Age 0, so the confidence is the tier multiplier alone.
        let obs = PendingObservation { timestamp: NOW, ..fresh_observation() };
        let three_good = ReferenceComparison {
            residuals: vec![
                residual("nws_station", "temperature_c", 0.4),
                residual("nws_station", "humidity_pct", 2.0),
                residual("nws_station", "wind_speed_ms", 0.5),
            ],
        };

        // Empty slice: exactly evaluate(), with and without the #3131 guard.
        for guard in [false, true] {
            let p = MechanicalPolicy { gross_outlier_guard: guard, ..policy.clone() };
            assert_eq!(
                p.evaluate_with_unverified(&obs, Some(&three_good), NOW, &[]),
                p.evaluate(&obs, Some(&three_good), NOW)
            );
        }

        // One unverified variable on three good ones: [G,G,G,Out] → Degraded.
        let v = policy.evaluate_with_unverified(&obs, Some(&three_good), NOW, &["pressure_hpa"]);
        assert_eq!(v.tier, ReadingQualityTier::Degraded);
        assert!(v.approved);
        assert_eq!(v.confidence, ReadingQualityTier::Degraded.default_reward_multiplier());
        assert!(v.reasons.iter().any(|r| r.contains("unverified") && r.contains("pressure_hpa")));
        assert_eq!(policy.evaluate(&obs, Some(&three_good), NOW).tier, ReadingQualityTier::Good);

        // Plus one real Outlier: two Outliers → Outlier (the drop is spent).
        let mut with_outlier = three_good.clone();
        with_outlier.residuals[1] = residual("nws_station", "humidity_pct", 90.0);
        assert_eq!(policy.evaluate(&obs, Some(&with_outlier), NOW).tier, ReadingQualityTier::Degraded);
        assert_eq!(
            policy.evaluate_with_unverified(&obs, Some(&with_outlier), NOW, &["pressure_hpa"]).tier,
            ReadingQualityTier::Outlier
        );
    }
}
