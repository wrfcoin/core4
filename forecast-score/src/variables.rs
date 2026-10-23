//! Which variables a station forecast is scored on.
//!
//! Owner ruling (2026-10-19, epic #3604 Phase A): **score only the variables the
//! station reports.** A manifest declares the variables it forecasts
//! (`output_variables`, sorted and unique, already required by
//! [`crate::ForecastArtifactManifest::validate`]); the truth observation
//! reports some set of variables; skill is averaged over the intersection.
//! Consensus never inspects variable names, only the manifest digest, so this
//! is a scorer-side rule with no consensus change.

/// Sorted, de-duplicated intersection of the declared forecast variables and
/// the variables the truth observation reports. Empty means there is nothing
/// to score and no receipt should be issued.
pub fn scored_variables<'a, D, R>(declared: D, reported: R) -> Vec<String>
where
    D: IntoIterator<Item = &'a str>,
    R: IntoIterator<Item = &'a str>,
{
    let reported: std::collections::BTreeSet<&str> = reported.into_iter().collect();
    let mut out: Vec<String> = declared
        .into_iter()
        .filter(|v| reported.contains(v))
        .map(str::to_string)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Mean of per-variable skills over the scored variables, clamped to `[0, 1]`;
/// `None` when nothing was scored or the mean is not finite (`f64::clamp`
/// passes NaN through, so a NaN skill must not become a receipt).
pub fn mean_skill<I: IntoIterator<Item = f64>>(skills: I) -> Option<f64> {
    let (sum, n) = skills
        .into_iter()
        .fold((0.0_f64, 0usize), |(s, n), v| (s + v, n + 1));
    let mean = sum / n as f64;
    if n == 0 || !mean.is_finite() {
        None
    } else {
        Some(mean.clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersection_is_sorted_unique_and_ignores_unreported() {
        let declared = ["temperature_c", "pressure_hpa", "humidity_pct"];
        let reported = ["pressure_hpa", "temperature_c", "wind_speed_ms", "temperature_c"];
        assert_eq!(
            scored_variables(declared, reported),
            vec!["pressure_hpa".to_string(), "temperature_c".to_string()]
        );
    }

    #[test]
    fn nothing_in_common_scores_nothing() {
        assert!(scored_variables(["humidity_pct"], ["pressure_hpa"]).is_empty());
        assert_eq!(mean_skill(std::iter::empty()), None);
    }

    #[test]
    fn mean_skill_averages_only_scored_variables() {
        assert_eq!(mean_skill([0.2, 0.6]), Some(0.4));
        assert_eq!(mean_skill([1.5]), Some(1.0));
    }

    #[test]
    fn non_finite_skill_scores_nothing() {
        assert_eq!(mean_skill([0.5, f64::NAN]), None);
        assert_eq!(mean_skill([f64::INFINITY]), None);
    }
}
