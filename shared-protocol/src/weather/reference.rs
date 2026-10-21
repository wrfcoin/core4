//! Multi-reference residual → quality-tier mapping.
//!
//! Implements the "Option C" policy described in [`super::quality_tier`]: a
//! reading is compared against independent gridded references (HRRR/GFS
//! analysis, MRMS, ERA5 — served off-chain by the `oracle-reference` service)
//! and the per-variable residuals are folded into one tier. The mapping lives
//! here, in consensus-adjacent code, so that any node can re-derive a tier
//! from public reference data; the off-chain service only ever supplies
//! reference values and residuals, never verdicts.
//!
//! The fold is a **second-worst rule**, not a worst-variable one (#3098): a
//! station that is good on most metrics must not be zeroed by a single
//! borderline metric. See [`ReferenceComparison::tier`] for the exact rule.
//!
//! Band thresholds are data (`fixtures/reference_bands.json`), not code:
//! per-source band sets let coarse references use wider bands (GFS 0.25° vs
//! HRRR 3 km), and wind bands stay generous (grid cell vs point anemometer).
//! The same JSON — plus the golden vectors in
//! `fixtures/reference_tier_vectors.json` — is consumed by the backend's
//! mirrored TypeScript implementation for parity testing.
//!
//! Gating philosophy (matches the documented fallback): **no gating without
//! reference data.** Missing sources, unknown variables, or an empty residual
//! set all resolve toward `Good`.
//!
//! # The gross-outlier guard (#3131)
//!
//! The second-worst fold drops the lone `Outlier` regardless of how far out
//! it is, so a thermometer 10 °C off earned the same `Degraded` (0.5×) as a
//! near-miss. Bands version 3 adds an optional per-variable hard limit,
//! [`ResidualBand::gross_max`]. When exactly one scored variable is `Outlier`
//! and its `|residual|` is strictly greater than that variable's `gross_max`,
//! [`ReferenceComparison::tier_with_gross_guard`] collapses the reading to
//! `Outlier`. Every other input folds exactly as [`ReferenceComparison::tier`]
//! does, and a variable with no `gross_max` is never gross.
//!
//! The guard is a reward-tier change, so callers choose the rule by height:
//! the oracle attester applies it only once
//! [`ConsensusFeature::RewardGrossOutlierGuard`](crate::crypto::chain_binding::ConsensusFeature::RewardGrossOutlierGuard)
//! is active on the chain it signs for. [`ReferenceComparison::tier`] stays
//! the pre-activation rule, unchanged.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::quality_tier::ReadingQualityTier;

/// Absolute-residual thresholds for one variable against one reference source.
///
/// `|residual| <= good_max` → `Good`; `<= marginal_max` → `Degraded`;
/// beyond that → `Outlier`.
///
/// `gross_max` (bands v3, #3131) is optional. It never changes this variable's
/// own tier. It only marks an `Outlier` whose `|residual| > gross_max` as
/// *gross*, which [`ReferenceComparison::tier_with_gross_guard`] refuses to
/// drop. Absent → the variable is never gross, so a v2 fixture (which has no
/// such key) parses and tiers exactly as before.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ResidualBand {
    pub good_max: f64,
    pub marginal_max: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    // determinism-ok: band threshold read from the embedded fixture; the tier fold runs only in the oracle attester (grep: no block production/application/replay caller), and consensus receives the attester's signed confidence verbatim
    pub gross_max: Option<f64>,
}

impl ResidualBand {
    /// Whether `residual` is a *gross* miss for this band: finite, and
    /// strictly beyond `gross_max`. `false` when no `gross_max` is configured.
    /// A non-finite residual is not gross: it points at a reference fetch or
    /// unit bug, not at this station's sensor, and keeps its #3098 handling.
    // determinism-ok: attester-side comparison against a fixture threshold, same boundary as tier_for above; never executed on a consensus path
    pub fn is_gross(self, residual: f64) -> bool {
        residual.is_finite() && self.gross_max.is_some_and(|gross| residual.abs() > gross)
    }

    pub fn tier_for(self, residual: f64) -> ReadingQualityTier {
        let r = residual.abs();
        if r <= self.good_max {
            ReadingQualityTier::Good
        } else if r <= self.marginal_max {
            ReadingQualityTier::Degraded
        } else {
            ReadingQualityTier::Outlier
        }
    }
}

/// Per-variable bands for a single reference source (keys are canonical
/// variable names: `temperature_c`, `pressure_hpa`, `humidity_pct`,
/// `wind_speed_ms`, `rainfall_mm`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceBands(pub BTreeMap<String, ResidualBand>);

/// Versioned band configuration: reference source → per-variable bands.
///
/// `BTreeMap` keeps iteration order deterministic (project convention for
/// anything consensus-adjacent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReferenceBands {
    /// Bumped whenever thresholds change; recorded alongside scores so
    /// shadow-mode data remains interpretable across recalibrations.
    pub version: u32,
    pub sources: BTreeMap<String, SourceBands>,
}

/// The canonical band fixture, embedded so every node ships identical
/// thresholds for a given crate version.
pub const REFERENCE_BANDS_JSON: &str = include_str!("../../fixtures/reference_bands.json");

impl ReferenceBands {
    /// The built-in thresholds. Panics only if the embedded fixture is
    /// malformed, which the test suite makes impossible to ship.
    pub fn builtin() -> Self {
        serde_json::from_str(REFERENCE_BANDS_JSON)
            .expect("embedded reference_bands.json must parse")
    }

    pub fn band(&self, source: &str, variable: &str) -> Option<ResidualBand> {
        self.sources.get(source).and_then(|s| s.0.get(variable)).copied()
    }
}

/// One residual measurement: observed value minus the reference value from a
/// named source, in the variable's canonical unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReferenceResidual {
    /// Reference source name as configured in the bands (e.g. `hrrr`, `gfs`).
    pub source: String,
    /// Canonical variable name (e.g. `temperature_c`).
    pub variable: String,
    /// Observed minus reference, canonical units. Sign is preserved for
    /// diagnostics; tiering uses the absolute value.
    pub residual: f64,
}

/// A full comparison of one reading against the available references.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ReferenceComparison {
    pub residuals: Vec<ReferenceResidual>,
}

impl ReferenceComparison {
    /// Per-variable tiers for every residual that has a configured band, in
    /// input order. Residuals with no band are skipped (no gating without
    /// reference data); non-finite residuals (NaN/inf — upstream fetch or
    /// unit bugs) tier `Outlier` for that variable rather than silently
    /// passing.
    pub fn variable_tiers(&self, bands: &ReferenceBands) -> Vec<ReadingQualityTier> {
        self.residuals
            .iter()
            .filter_map(|r| {
                let band = bands.band(&r.source, &r.variable)?;
                Some(if r.residual.is_finite() {
                    band.tier_for(r.residual)
                } else {
                    ReadingQualityTier::Outlier
                })
            })
            .collect()
    }

    /// The residual that trips the #3131 gross-outlier guard, if any: the
    /// comparison has exactly one scored `Outlier` variable, at least two
    /// variables were scored, and that Outlier's band marks it gross
    /// ([`ResidualBand::is_gross`]). `None` in every other case, including
    /// when the single scored variable is itself the Outlier (the fold
    /// already returns `Outlier` there).
    pub fn gross_outlier<'a>(&'a self, bands: &ReferenceBands) -> Option<&'a ReferenceResidual> {
        let scored: Vec<(&ReferenceResidual, ResidualBand, ReadingQualityTier)> = self
            .residuals
            .iter()
            .filter_map(|r| {
                let band = bands.band(&r.source, &r.variable)?;
                let tier = if r.residual.is_finite() {
                    band.tier_for(r.residual)
                } else {
                    ReadingQualityTier::Outlier
                };
                Some((r, band, tier))
            })
            .collect();
        if scored.len() < 2 {
            return None;
        }
        let mut outliers = scored.iter().filter(|(_, _, t)| *t == ReadingQualityTier::Outlier);
        let (residual, band, _) = outliers.next()?;
        if outliers.next().is_some() {
            return None;
        }
        band.is_gross(residual.residual).then_some(*residual)
    }

    /// Tier under the #3131 rule: [`Self::tier`], except that a lone
    /// **gross** Outlier ([`Self::gross_outlier`]) is not dropped and the
    /// reading tiers `Outlier`.
    ///
    /// This is the post-activation rule. It differs from [`Self::tier`] only
    /// when exactly one scored variable is `Outlier`, at least two were
    /// scored, and that variable has a `gross_max` its `|residual|` exceeds.
    pub fn tier_with_gross_guard(&self, bands: &ReferenceBands) -> ReadingQualityTier {
        if self.gross_outlier(bands).is_some() {
            ReadingQualityTier::Outlier
        } else {
            self.tier(bands)
        }
    }

    /// [`Self::tier_with_gross_guard`] when `gross_guard_active`, else
    /// [`Self::tier`]. The caller derives the flag from the feature schedule.
    pub fn tier_under(&self, bands: &ReferenceBands, gross_guard_active: bool) -> ReadingQualityTier {
        if gross_guard_active {
            self.tier_with_gross_guard(bands)
        } else {
            self.tier(bands)
        }
    }

    /// Tier for this comparison under `bands` — the **second-worst rule**
    /// (#3098), folded over [`Self::variable_tiers`]:
    ///
    /// - An empty (or fully skipped) comparison is `Good` — the documented
    ///   reference-unreachable fallback.
    /// - `Outlier` only if **two or more** scored variables are `Outlier`,
    ///   or if the single scored variable is `Outlier`.
    /// - Otherwise the single worst variable is dropped when it is an
    ///   `Outlier` (any count ≥ 2) or when three or more variables were
    ///   scored, and the tier is the worst of the remaining variables.
    /// - A dropped `Outlier` still costs one tier: the result is floored at
    ///   `Degraded`, never `Good`.
    ///
    /// So one marginal barometer among good thermometer/hygrometer readings
    /// yields `Degraded` (0.5×), not `Outlier` (0.1×); a reading that is
    /// wrong on two independent variables is still `Outlier`.
    pub fn tier(&self, bands: &ReferenceBands) -> ReadingQualityTier {
        fold_variable_tiers(&self.variable_tiers(bands))
    }

    /// [`Self::tier_under`] with `unverified` extra variables scored as
    /// `Outlier` (#3285 owner ruling): a submitted variable the attester
    /// could have checked but was given no trustworthy value for is not
    /// removed from the fold (removal would hand the #3098 worst-variable drop
    /// to another variable) but counted as the worst tier an honest value
    /// could have produced. With `unverified == 0` this is exactly
    /// [`Self::tier_under`]. Under the #3131 guard, a lone real Outlier beyond
    /// its gross limit still tiers `Outlier` (it would with the extra Outlier
    /// anyway).
    pub fn tier_under_with_unverified(
        &self,
        bands: &ReferenceBands,
        gross_guard_active: bool,
        unverified: usize,
    ) -> ReadingQualityTier {
        if unverified == 0 {
            return self.tier_under(bands, gross_guard_active);
        }
        if gross_guard_active && self.gross_outlier(bands).is_some() {
            return ReadingQualityTier::Outlier;
        }
        let mut tiers = self.variable_tiers(bands);
        tiers.extend(std::iter::repeat_n(ReadingQualityTier::Outlier, unverified));
        fold_variable_tiers(&tiers)
    }
}

/// The #3098 second-worst fold over already-scored per-variable tiers. Pure
/// and order-independent; exposed so the reward layer and the backend mirror
/// can pin the exact truth table.
pub fn fold_variable_tiers(tiers: &[ReadingQualityTier]) -> ReadingQualityTier {
    let scored = tiers.len();
    if scored == 0 {
        return ReadingQualityTier::Good;
    }
    let outliers = tiers.iter().filter(|t| **t == ReadingQualityTier::Outlier).count();
    if outliers >= 2 || (outliers == 1 && scored == 1) {
        return ReadingQualityTier::Outlier;
    }
    // From here: at most one Outlier, and if there is one, scored >= 2.
    let mut sorted = tiers.to_vec();
    sorted.sort_by_key(|t| tier_rank(*t));
    let drop_worst = outliers == 1 || scored >= 3;
    let kept = if drop_worst { &sorted[..scored - 1] } else { &sorted[..] };
    let base = kept
        .iter()
        .copied()
        .fold(ReadingQualityTier::Good, ReadingQualityTier::worst);
    if outliers == 1 {
        base.worst(ReadingQualityTier::Degraded)
    } else {
        base
    }
}

fn tier_rank(tier: ReadingQualityTier) -> u8 {
    match tier {
        ReadingQualityTier::Good => 0,
        ReadingQualityTier::Degraded => 1,
        ReadingQualityTier::Outlier => 2,
    }
}

/// Convenience: second-worst-rule tier for a batch of residuals under the
/// built-in bands.
pub fn tier_from_residuals(residuals: &[ReferenceResidual]) -> ReadingQualityTier {
    ReferenceComparison { residuals: residuals.to_vec() }.tier(&ReferenceBands::builtin())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS_JSON: &str = include_str!("../../fixtures/reference_tier_vectors.json");

    #[derive(Debug, Deserialize)]
    struct GoldenVector {
        name: String,
        residuals: Vec<ReferenceResidual>,
        /// The pre-activation (#3098 second-worst) tier.
        expected_tier: ReadingQualityTier,
        /// The post-activation (#3131 gross guard) tier. Absent means the
        /// guard does not change this vector.
        #[serde(default)]
        expected_tier_gross_guard: Option<ReadingQualityTier>,
    }

    #[derive(Debug, Deserialize)]
    struct GoldenFile {
        bands_version: u32,
        vectors: Vec<GoldenVector>,
    }

    fn residual(source: &str, variable: &str, value: f64) -> ReferenceResidual {
        ReferenceResidual { source: source.into(), variable: variable.into(), residual: value }
    }

    #[test]
    fn builtin_bands_parse_and_are_sane() {
        let bands = ReferenceBands::builtin();
        assert!(bands.version >= 1);
        for (source, vars) in &bands.sources {
            assert!(!vars.0.is_empty(), "source {source} has no bands");
            for (variable, band) in &vars.0 {
                assert!(
                    band.good_max > 0.0 && band.marginal_max > band.good_max,
                    "bands for {source}/{variable} must satisfy 0 < good_max < marginal_max"
                );
                if let Some(gross) = band.gross_max {
                    assert!(
                        gross.is_finite() && gross > band.marginal_max,
                        "bands for {source}/{variable}: gross_max must be finite and \
                         strictly above marginal_max, or the guard would fire inside \
                         the Degraded band"
                    );
                }
            }
        }
        // Coarser grid ⇒ wider bands: GFS must be at least as forgiving as HRRR.
        let hrrr = bands.band("hrrr", "temperature_c").unwrap();
        let gfs = bands.band("gfs", "temperature_c").unwrap();
        assert!(gfs.good_max >= hrrr.good_max);
    }

    #[test]
    fn empty_comparison_is_good() {
        // The reference-unreachable fallback: no gating without data.
        assert_eq!(tier_from_residuals(&[]), ReadingQualityTier::Good);
    }

    #[test]
    fn unknown_source_or_variable_is_skipped() {
        let tier = tier_from_residuals(&[
            residual("not_a_source", "temperature_c", 99.0),
            residual("hrrr", "not_a_variable", 99.0),
        ]);
        assert_eq!(tier, ReadingQualityTier::Good);
    }

    #[test]
    fn a_single_outlier_beside_a_good_variable_is_degraded_not_outlier() {
        // #3098: one wrong variable no longer zeroes the reading. It still
        // costs a tier (Degraded, 0.5×) — the Outlier floor.
        let bands = ReferenceBands::builtin();
        let t = bands.band("hrrr", "temperature_c").unwrap();
        let comparison = ReferenceComparison {
            residuals: vec![
                residual("hrrr", "temperature_c", t.good_max / 2.0), // Good
                residual("hrrr", "pressure_hpa", -1e9),              // Outlier
            ],
        };
        assert_eq!(comparison.tier(&bands), ReadingQualityTier::Degraded);
    }

    #[test]
    fn two_outlier_variables_are_still_outlier() {
        let bands = ReferenceBands::builtin();
        let comparison = ReferenceComparison {
            residuals: vec![
                residual("hrrr", "temperature_c", 0.1), // Good
                residual("hrrr", "pressure_hpa", -1e9), // Outlier
                residual("hrrr", "wind_speed_ms", 1e9), // Outlier
            ],
        };
        assert_eq!(comparison.tier(&bands), ReadingQualityTier::Outlier);
    }

    #[test]
    fn fold_truth_table() {
        use ReadingQualityTier::{Degraded as D, Good as G, Outlier as O};
        // (input, expected)
        let cases: &[(&[ReadingQualityTier], ReadingQualityTier)] = &[
            (&[], G),
            (&[G], G),
            (&[D], D),
            (&[O], O),                 // the only scored variable is Outlier
            (&[G, D], D),              // two scored, no outlier: worst of all
            (&[D, D], D),
            (&[G, O], D),              // single outlier among 2: dropped, floored at Degraded
            (&[D, O], D),
            (&[O, O], O),              // two outliers
            (&[G, G, G], G),
            (&[G, G, D], G),           // >=3 scored: the single worst is dropped
            (&[G, D, D], D),
            (&[G, G, O], D),           // dropped outlier still costs one tier
            (&[G, D, O], D),
            (&[D, D, O], D),
            (&[G, O, O], O),
            (&[O, O, O], O),
            (&[G, G, G, O], D),
            (&[G, G, O, O], O),
        ];
        for (input, expected) in cases {
            assert_eq!(fold_variable_tiers(input), *expected, "fold of {input:?}");
            // Order independence.
            let mut reversed = input.to_vec();
            reversed.reverse();
            assert_eq!(fold_variable_tiers(&reversed), *expected, "fold of {reversed:?}");
        }
    }

    #[test]
    fn sign_of_residual_is_irrelevant() {
        let bands = ReferenceBands::builtin();
        let t = bands.band("hrrr", "temperature_c").unwrap();
        for r in [t.marginal_max - 0.01, -(t.marginal_max - 0.01)] {
            assert_eq!(
                ReferenceComparison { residuals: vec![residual("hrrr", "temperature_c", r)] }
                    .tier(&bands),
                ReadingQualityTier::Degraded
            );
        }
    }

    #[test]
    fn band_edges_are_inclusive() {
        let band = ResidualBand { good_max: 1.0, marginal_max: 3.0, gross_max: None };
        assert_eq!(band.tier_for(1.0), ReadingQualityTier::Good);
        assert_eq!(band.tier_for(3.0), ReadingQualityTier::Degraded);
        assert_eq!(band.tier_for(3.000001), ReadingQualityTier::Outlier);
    }

    #[test]
    fn non_finite_residual_is_outlier() {
        assert_eq!(
            tier_from_residuals(&[residual("hrrr", "temperature_c", f64::NAN)]),
            ReadingQualityTier::Outlier
        );
        assert_eq!(
            tier_from_residuals(&[residual("hrrr", "temperature_c", f64::INFINITY)]),
            ReadingQualityTier::Outlier
        );
        // The variable's own tier stays Outlier; the drop-worst fold applies
        // to it like any other Outlier (#3098).
        let bands = ReferenceBands::builtin();
        let comparison = ReferenceComparison {
            residuals: vec![
                residual("hrrr", "temperature_c", f64::NAN),
                residual("hrrr", "pressure_hpa", 0.5),
            ],
        };
        assert_eq!(
            comparison.variable_tiers(&bands),
            vec![ReadingQualityTier::Outlier, ReadingQualityTier::Good]
        );
        assert_eq!(comparison.tier(&bands), ReadingQualityTier::Degraded);
    }

    #[test]
    fn golden_vectors_match() {
        // Shared with the backend's TS mirror — keep in sync with
        // fixtures/reference_tier_vectors.json, never edit expectations here.
        let golden: GoldenFile = serde_json::from_str(VECTORS_JSON).unwrap();
        let bands = ReferenceBands::builtin();
        assert_eq!(golden.bands_version, bands.version, "vectors were built for another bands version");
        for v in &golden.vectors {
            let comparison = ReferenceComparison { residuals: v.residuals.clone() };
            assert_eq!(comparison.tier(&bands), v.expected_tier, "golden vector '{}' diverged", v.name);
            assert_eq!(
                comparison.tier_with_gross_guard(&bands),
                v.expected_tier_gross_guard.unwrap_or(v.expected_tier),
                "golden vector '{}' diverged under the #3131 gross guard",
                v.name
            );
        }
    }

    #[test]
    fn gross_guard_collapses_only_a_lone_gross_outlier() {
        let bands = ReferenceBands::builtin();
        let t = bands.band("nws_station", "temperature_c").unwrap();
        let gross = t.gross_max.expect("nws_station temperature has a gross limit");
        let with_temp = |r: f64| ReferenceComparison {
            residuals: vec![
                residual("nws_station", "temperature_c", r),
                residual("nws_station", "humidity_pct", 2.0),
            ],
        };
        // Exactly at the limit: not gross (strict >), the #3098 fold applies.
        assert_eq!(with_temp(gross).tier_with_gross_guard(&bands), ReadingQualityTier::Degraded);
        assert_eq!(with_temp(-gross).tier_with_gross_guard(&bands), ReadingQualityTier::Degraded);
        // Just past it, either sign: Outlier.
        assert_eq!(with_temp(gross + 0.01).tier_with_gross_guard(&bands), ReadingQualityTier::Outlier);
        assert_eq!(with_temp(-(gross + 0.01)).tier_with_gross_guard(&bands), ReadingQualityTier::Outlier);
        // Pre-activation rule is untouched either way.
        assert_eq!(with_temp(gross + 0.01).tier(&bands), ReadingQualityTier::Degraded);
    }

    #[test]
    fn gross_guard_ignores_non_finite_and_single_scored_residuals() {
        let bands = ReferenceBands::builtin();
        // A NaN residual is Outlier for its variable but never gross.
        let nan = ReferenceComparison {
            residuals: vec![
                residual("nws_station", "temperature_c", f64::NAN),
                residual("nws_station", "humidity_pct", 2.0),
            ],
        };
        assert_eq!(nan.gross_outlier(&bands), None);
        assert_eq!(nan.tier_with_gross_guard(&bands), ReadingQualityTier::Degraded);
        // One scored variable: already Outlier by the fold, guard not needed.
        let single = ReferenceComparison { residuals: vec![residual("nws_station", "temperature_c", 50.0)] };
        assert_eq!(single.gross_outlier(&bands), None);
        assert_eq!(single.tier_with_gross_guard(&bands), ReadingQualityTier::Outlier);
    }

    #[test]
    fn v2_band_json_without_gross_max_still_parses() {
        let v2 = r#"{"version":2,"sources":{"hrrr":{"temperature_c":{"good_max":2.0,"marginal_max":5.0}}}}"#;
        let bands: ReferenceBands = serde_json::from_str(v2).unwrap();
        assert_eq!(bands.band("hrrr", "temperature_c").unwrap().gross_max, None);
        // And a v3 band with no gross_max re-serialises without the key.
        let json = serde_json::to_string(&bands).unwrap();
        assert!(!json.contains("gross_max"), "{json}");
    }
}
