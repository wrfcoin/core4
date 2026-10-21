//! #3098 — weather reward tier: widened NWS pressure band + second-worst fold.
//!
//! Acceptance cases from the story, pinned against the built-in bands so a
//! station that is good on most metrics stops being tiered `Outlier` (0.1×)
//! by a single borderline one.

use wrfcoin_shared_protocol::weather::quality_tier::ReadingQualityTier;
use wrfcoin_shared_protocol::weather::{
    fold_variable_tiers, tier_from_residuals, ReferenceBands, ReferenceComparison,
    ReferenceResidual,
};

fn residual(source: &str, variable: &str, value: f64) -> ReferenceResidual {
    ReferenceResidual { source: source.into(), variable: variable.into(), residual: value }
}

fn nws(variable: &str, value: f64) -> ReferenceResidual {
    residual("nws_station", variable, value)
}

#[test]
fn bands_fixture_is_version_2_with_the_widened_nws_pressure_band() {
    let bands = ReferenceBands::builtin();
    // #3098 bumped the fixture to 2; #3131 bumped it to 3 by adding optional
    // `gross_max` limits only — no good/marginal threshold changed.
    assert_eq!(bands.version, 3, "#3131 bumps the fixture version");
    let p = bands.band("nws_station", "pressure_hpa").unwrap();
    assert_eq!(p.good_max, 8.0);
    assert_eq!(p.marginal_max, 15.0);
    assert_eq!(p.gross_max, None, "#3131: pressure deliberately has no gross limit");
    // Gridded bands are untouched.
    let hrrr = bands.band("hrrr", "pressure_hpa").unwrap();
    assert_eq!((hrrr.good_max, hrrr.marginal_max), (2.5, 6.0));
    let gfs = bands.band("gfs", "pressure_hpa").unwrap();
    assert_eq!((gfs.good_max, gfs.marginal_max), (3.5, 8.0));
}

/// The 2026-09-11 diagnosis against KDLH: temp residual 1.8 °C, pressure
/// residual 7.8 hPa (a backyard barometer below the airport sea-level
/// reference), wind unpaired. Was `Outlier`; now `Good` with a full reward.
#[test]
fn the_2026_09_11_station_residuals_tier_good_with_a_nonzero_reward() {
    let tier = tier_from_residuals(&[nws("temperature_c", 1.8), nws("pressure_hpa", -7.8)]);
    assert_eq!(tier, ReadingQualityTier::Good);
    assert_eq!(tier.default_reward_multiplier(), 1.0);
}

/// temp Good + pressure Degraded + wind unpaired → Degraded (was Outlier
/// under the old marginal edge, 0.1×; now 0.5×).
#[test]
fn temp_good_pressure_degraded_wind_unpaired_is_degraded() {
    let bands = ReferenceBands::builtin();
    let comparison = ReferenceComparison {
        residuals: vec![nws("temperature_c", 1.8), nws("pressure_hpa", -12.0)],
    };
    assert_eq!(
        comparison.variable_tiers(&bands),
        vec![ReadingQualityTier::Good, ReadingQualityTier::Degraded]
    );
    let tier = comparison.tier(&bands);
    assert_eq!(tier, ReadingQualityTier::Degraded);
    assert!(tier.default_reward_multiplier() > 0.1);
}

/// temp Good + pressure Outlier with ≥3 variables scored → Degraded.
#[test]
fn a_single_outlier_variable_among_three_scored_is_degraded() {
    let tier = tier_from_residuals(&[
        nws("temperature_c", 1.8),
        nws("pressure_hpa", -40.0), // Outlier
        nws("humidity_pct", 3.0),
    ]);
    assert_eq!(tier, ReadingQualityTier::Degraded);
}

#[test]
fn two_outlier_variables_still_tier_outlier() {
    let tier = tier_from_residuals(&[
        nws("temperature_c", 1.8),
        nws("pressure_hpa", -40.0), // Outlier
        nws("wind_speed_ms", 20.0), // Outlier
    ]);
    assert_eq!(tier, ReadingQualityTier::Outlier);
}

#[test]
fn a_single_variable_comparison_that_is_outlier_stays_outlier() {
    assert_eq!(
        tier_from_residuals(&[nws("pressure_hpa", -40.0)]),
        ReadingQualityTier::Outlier
    );
}

#[test]
fn a_non_finite_residual_is_still_outlier_for_its_own_variable() {
    let bands = ReferenceBands::builtin();
    let comparison = ReferenceComparison {
        residuals: vec![nws("temperature_c", f64::NAN), nws("pressure_hpa", 1.0)],
    };
    assert_eq!(
        comparison.variable_tiers(&bands),
        vec![ReadingQualityTier::Outlier, ReadingQualityTier::Good]
    );
    // Alone it is the reading's tier; beside a good variable the drop-worst
    // fold applies and the floor keeps it at Degraded.
    assert_eq!(tier_from_residuals(&[nws("temperature_c", f64::NAN)]), ReadingQualityTier::Outlier);
    assert_eq!(comparison.tier(&bands), ReadingQualityTier::Degraded);
}

#[test]
fn reward_multipliers_are_preserved() {
    assert_eq!(ReadingQualityTier::Good.default_reward_multiplier(), 1.0);
    assert_eq!(ReadingQualityTier::Degraded.default_reward_multiplier(), 0.5);
    assert_eq!(ReadingQualityTier::Outlier.default_reward_multiplier(), 0.1);
}

#[test]
fn the_fold_is_pure_over_scored_tiers() {
    use ReadingQualityTier::{Degraded as D, Good as G, Outlier as O};
    assert_eq!(fold_variable_tiers(&[]), G);
    assert_eq!(fold_variable_tiers(&[O]), O);
    assert_eq!(fold_variable_tiers(&[G, O]), D);
    assert_eq!(fold_variable_tiers(&[G, G, O]), D);
    assert_eq!(fold_variable_tiers(&[O, O]), O);
    assert_eq!(fold_variable_tiers(&[G, D]), D);
    assert_eq!(fold_variable_tiers(&[G, G, D]), G);
}
