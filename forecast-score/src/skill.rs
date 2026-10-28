//! Receipt scoring for the oracle-operated forecast scorer job (epic #3604, A1/A6).
//!
//! Consensus mints a revealed forecast at whatever `skill_score` an authorized,
//! signed receipt carries; it never recomputes skill. Which skill definition a
//! receipt carries is therefore an OWNER choice, not a consensus fact, and this
//! module implements both candidates without choosing between them:
//!
//! - [`SkillDefinition::RmseSigma`]: the crate's `1 − RMSE / σ`, where the
//!   reference error is the population standard deviation of the station's own
//!   observations over the input window (the climatological spread), applied
//!   through [`ForecastAccuracyMetrics::from_pairs_with_reference`] with
//!   [`ReferenceStrategy::FixedClimatology`].
//! - [`SkillDefinition::MaeClim`]: the prototype's (`tools/forecast-proto`,
//!   `forecast-kit score`) `1 − |e| / |climatology − truth|`, where climatology
//!   is the window mean.
//!
//! Per variable skill is clamped to `[0, 1]` and averaged over the variables the
//! forecast declares AND the truth reports ([`crate::scored_variables`]).
//!
//! Floating-point arithmetic here is off-chain only (the scorer job); nothing in
//! consensus calls it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{ForecastAccuracyMetrics, Pair, ReferenceStrategy};

const EPS: f64 = f64::EPSILON; // determinism-ok: off-chain scorer arithmetic

/// The skill definition a receipt carries. There is deliberately no `Default`:
/// the scorer job refuses to sign until one is configured explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillDefinition {
    /// `1 − |e| / σ_window` (the crate's `1 − RMSE/σ`, one pair per variable).
    RmseSigma,
    /// `1 − |e| / |climatology − truth|` (the prototype receipt rule).
    MaeClim,
}

impl SkillDefinition {
    pub const ALL: [SkillDefinition; 2] = [Self::RmseSigma, Self::MaeClim];

    pub const fn name(self) -> &'static str {
        match self {
            Self::RmseSigma => "rmse_sigma",
            Self::MaeClim => "mae_clim",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.name() == name.trim())
    }
}

/// One variable of one forecast, with the scorer's own truth and baselines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariableInput {
    /// The forecaster's value.
    pub forecast: f64,
    /// The truth reading at `valid_at`.
    pub truth: f64,
    /// The station's observations over the input window, time-ordered, last =
    /// most recent before issue. The scorer builds this from its own truth
    /// store, never from a file the forecaster supplies.
    pub window: Vec<f64>,
}

/// Why a variable scored what it scored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariableScore {
    pub variable: String,
    pub skill: f64,
    /// The reference error the forecast error was divided by.
    pub reference_error: f64,
    /// Which reference that error came from (`climatology_spread`, `climatology`).
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoredForecast {
    pub definition: SkillDefinition,
    pub scored_variables: Vec<String>,
    pub per_variable: Vec<VariableScore>,
    /// The metrics `wrfcoin forecast sign-receipt --metrics-file` takes.
    pub metrics: ForecastAccuracyMetrics,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SkillError {
    #[error("nothing to score: no declared variable has a truth reading")]
    NothingToScore,
    #[error("variable {0}: window needs at least two finite observations")]
    ShortWindow(String),
    #[error("variable {0}: non-finite forecast, truth or reference value")]
    NonFinite(String),
    #[error("metrics failed validation")]
    InvalidMetrics,
}

fn mean(xs: &[f64]) -> f64 {
    xs.iter().sum::<f64>() / xs.len() as f64 // determinism-ok: off-chain scorer arithmetic
}

fn population_sigma(xs: &[f64]) -> f64 {
    let m = mean(xs);
    (xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / xs.len() as f64).sqrt() // determinism-ok: off-chain scorer arithmetic
}

/// `clamp(1 − error / reference, 0, 1)`; a zero reference scores 1 only for a
/// zero error (the prototype's and the crate's shared degenerate case).
fn ratio_skill(error: f64, reference: f64) -> f64 {
    if reference <= EPS {
        if error <= EPS { 1.0 } else { 0.0 }
    } else {
        (1.0 - error / reference).clamp(0.0, 1.0)
    }
}

fn score_variable(
    name: &str,
    input: &VariableInput,
    definition: SkillDefinition,
) -> Result<VariableScore, SkillError> {
    let finite = input.forecast.is_finite()
        && input.truth.is_finite()
        && input.window.iter().all(|v| v.is_finite());
    if !finite {
        return Err(SkillError::NonFinite(name.to_string()));
    }
    if input.window.len() < 2 {
        return Err(SkillError::ShortWindow(name.to_string()));
    }
    let (f, t) = (input.forecast, input.truth);
    let error = (f - t).abs();
    let climatology = mean(&input.window);
    let (reference, reference_error) = match definition {
        SkillDefinition::RmseSigma => ("climatology_spread".to_string(), population_sigma(&input.window)),
        SkillDefinition::MaeClim => ("climatology".to_string(), (climatology - t).abs()),
    };
    Ok(VariableScore {
        variable: name.to_string(),
        skill: spread_or_ratio(definition, f, t, error, reference_error)?,
        reference_error,
        reference,
    })
}

/// `RmseSigma` goes through the crate's own reference scorer so the receipt
/// carries exactly `from_pairs_with_reference(FixedClimatology)`; `MaeClim` is
/// the prototype's ratio.
fn spread_or_ratio(
    definition: SkillDefinition,
    forecast: f64,
    truth: f64,
    error: f64,
    reference_error: f64,
) -> Result<f64, SkillError> {
    match definition {
        SkillDefinition::RmseSigma if reference_error > EPS => {
            ForecastAccuracyMetrics::from_pairs_with_reference(
                &[Pair::new(forecast, truth)],
                ReferenceStrategy::FixedClimatology {
                    reference_rmse: reference_error,
                },
            )
            .map(|m| m.skill_score)
            .map_err(|_| SkillError::InvalidMetrics)
        },
        _ => Ok(ratio_skill(error, reference_error)),
    }
}

/// Score one forecast. `variables` maps declared variable → input; a declared
/// variable without a truth reading must simply be absent.
pub fn score_forecast(
    variables: &BTreeMap<String, VariableInput>,
    definition: SkillDefinition,
) -> Result<ScoredForecast, SkillError> {
    if variables.is_empty() {
        return Err(SkillError::NothingToScore);
    }
    let mut per_variable = Vec::with_capacity(variables.len());
    let mut pairs = Vec::with_capacity(variables.len());
    for (name, input) in variables {
        per_variable.push(score_variable(name, input, definition)?);
        pairs.push(Pair::new(input.forecast, input.truth));
    }
    let skill = crate::mean_skill(per_variable.iter().map(|v| v.skill)).ok_or(SkillError::InvalidMetrics)?;
    let metrics = ForecastAccuracyMetrics {
        mae: crate::mae(&pairs),
        rmse: crate::rmse(&pairs),
        bias: crate::bias(&pairs),
        skill_score: skill,
    };
    metrics.validate().map_err(|_| SkillError::InvalidMetrics)?;
    Ok(ScoredForecast {
        definition,
        scored_variables: variables.keys().cloned().collect(),
        per_variable,
        metrics,
    })
}

// ---------------------------------------------------------------------------
// Cap per cell and lead time (owner ruling 2026-10-24, Phase B Q4)
// ---------------------------------------------------------------------------

/// Width of a lead window in hours (the quota ADR's proposed six-hour windows).
pub const LEAD_WINDOW_HOURS: u32 = 6;

/// A scored forecast competing for a receipt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapCandidate {
    pub commit_id: String,
    pub forecaster: String,
    /// The forecast cell. Today a station forecast's cell is its `station_id`
    /// (`ForecastArtifactManifest.region`); no canonical geo cell is ratified.
    pub cell: String,
    pub horizon_hours: u32,
    pub valid_at: i64,
    pub skill: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CapKey {
    pub cell: String,
    /// `ceil(horizon_hours / 6)`: leads 1–6 → 1, 7–12 → 2, …
    pub lead_window: u32,
    /// UTC day of `valid_at` (`valid_at.div_euclid(86_400)`).
    pub day: i64,
}

impl CapKey {
    pub fn of(c: &CapCandidate) -> Self {
        Self {
            cell: c.cell.clone(),
            lead_window: c.horizon_hours.div_ceil(LEAD_WINDOW_HOURS).max(1),
            day: c.valid_at.div_euclid(86_400),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapDecision {
    pub commit_id: String,
    pub selected: bool,
    /// `selected` | `below_baseline` | `forecaster_already_selected` | `cell_lead_cap`
    pub reason: String,
}

/// Choose which scored forecasts get a receipt.
///
/// Per `(cell, lead window, UTC day)`: at most one receipt per forecaster (its
/// best), then at most `cap_per_cell_lead` receipts, best skill first, ties by
/// `commit_id`. A forecast whose skill does not exceed `baseline` gets no
/// receipt: consensus already pays an unreceipted reveal at the baseline, so a
/// receipt below it would only lower the forecaster's own credit. Deterministic
/// for any input order.
pub fn select_capped(
    candidates: &[CapCandidate],
    cap_per_cell_lead: usize,
    baseline: f64,
) -> Vec<CapDecision> {
    let mut groups: BTreeMap<CapKey, Vec<&CapCandidate>> = BTreeMap::new();
    for c in candidates {
        groups.entry(CapKey::of(c)).or_default().push(c);
    }
    let mut decisions: BTreeMap<String, CapDecision> = BTreeMap::new();
    for (_, mut group) in groups {
        group.sort_by(|a, b| b.skill.total_cmp(&a.skill).then_with(|| a.commit_id.cmp(&b.commit_id)));
        let mut forecasters = std::collections::BTreeSet::new();
        let mut taken = 0usize;
        for c in group {
            let reason = if c.skill <= baseline {
                "below_baseline"
            } else if !forecasters.insert(c.forecaster.as_str()) {
                "forecaster_already_selected"
            } else if taken >= cap_per_cell_lead {
                "cell_lead_cap"
            } else {
                taken += 1;
                "selected"
            };
            decisions.insert(
                c.commit_id.clone(),
                CapDecision {
                    commit_id: c.commit_id.clone(),
                    selected: reason == "selected",
                    reason: reason.to_string(),
                },
            );
        }
    }
    decisions.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(forecast: f64, truth: f64, window: &[f64]) -> VariableInput {
        VariableInput {
            forecast,
            truth,
            window: window.to_vec(),
        }
    }

    fn one(name: &str, v: VariableInput) -> BTreeMap<String, VariableInput> {
        BTreeMap::from([(name.to_string(), v)])
    }

    const WINDOW: [f64; 4] = [10.0, 12.0, 14.0, 12.0]; // mean 12, σ = √2, last 12

    #[test]
    fn names_round_trip_and_there_is_no_default() {
        for d in SkillDefinition::ALL {
            assert_eq!(SkillDefinition::parse(d.name()), Some(d));
            assert_eq!(serde_json::to_string(&d).unwrap(), format!("\"{}\"", d.name()));
        }
        assert_eq!(SkillDefinition::parse(""), None);
        assert_eq!(SkillDefinition::parse("rmse"), None);
    }

    #[test]
    fn rmse_sigma_is_the_crate_fixed_climatology_rule() {
        let s = score_forecast(&one("t", var(13.0, 13.5, &WINDOW)), SkillDefinition::RmseSigma).unwrap();
        let expected = 1.0 - 0.5 / 2f64.sqrt();
        assert!((s.metrics.skill_score - expected).abs() < 1e-12, "{}", s.metrics.skill_score);
        let crate_rule = ForecastAccuracyMetrics::from_pairs_with_reference(
            &[Pair::new(13.0, 13.5)],
            ReferenceStrategy::FixedClimatology { reference_rmse: 2f64.sqrt() },
        )
        .unwrap();
        assert_eq!(s.metrics.skill_score, crate_rule.skill_score);
    }

    #[test]
    fn mae_clim_is_the_prototype_rule() {
        // climatology 12, truth 14: clim error 2; forecast error 0.5 → 0.75
        let s = score_forecast(&one("t", var(13.5, 14.0, &WINDOW)), SkillDefinition::MaeClim).unwrap();
        assert!((s.metrics.skill_score - 0.75).abs() < 1e-12);
        // truth == climatology: any error scores 0, a perfect forecast 1
        let s = score_forecast(&one("t", var(12.3, 12.0, &WINDOW)), SkillDefinition::MaeClim).unwrap();
        assert_eq!(s.metrics.skill_score, 0.0);
        let s = score_forecast(&one("t", var(12.0, 12.0, &WINDOW)), SkillDefinition::MaeClim).unwrap();
        assert_eq!(s.metrics.skill_score, 1.0);
    }

    #[test]
    fn climatology_dump_scores_zero_under_mae_clim_but_not_rmse_sigma() {
        // Forecasting exactly the window mean: the prototype rule gives 0, the
        // spread rule gives credit whenever truth lands inside one σ.
        let dump = var(12.0, 12.8, &WINDOW);
        let mae = score_forecast(&one("t", dump.clone()), SkillDefinition::MaeClim).unwrap();
        let rmse = score_forecast(&one("t", dump), SkillDefinition::RmseSigma).unwrap();
        assert_eq!(mae.metrics.skill_score, 0.0);
        assert!(rmse.metrics.skill_score > 0.4, "{}", rmse.metrics.skill_score);
    }

    #[test]
    fn mean_over_scored_variables_and_errors() {
        let mut vars = one("a", var(13.5, 14.0, &WINDOW));
        vars.insert("b".into(), var(12.0, 14.0, &WINDOW));
        let s = score_forecast(&vars, SkillDefinition::MaeClim).unwrap();
        assert_eq!(s.scored_variables, vec!["a", "b"]);
        assert!((s.metrics.skill_score - 0.375).abs() < 1e-12);
        assert!((s.metrics.mae - 1.25).abs() < 1e-12);
        assert_eq!(score_forecast(&BTreeMap::new(), SkillDefinition::MaeClim), Err(SkillError::NothingToScore));
        assert!(matches!(
            score_forecast(&one("a", var(1.0, 1.0, &[1.0])), SkillDefinition::RmseSigma),
            Err(SkillError::ShortWindow(_))
        ));
        assert!(matches!(
            score_forecast(&one("a", var(f64::NAN, 1.0, &WINDOW)), SkillDefinition::RmseSigma),
            Err(SkillError::NonFinite(_))
        ));
    }

    fn cand(id: &str, who: &str, cell: &str, horizon: u32, valid_at: i64, skill: f64) -> CapCandidate {
        CapCandidate {
            commit_id: id.into(),
            forecaster: who.into(),
            cell: cell.into(),
            horizon_hours: horizon,
            valid_at,
            skill,
        }
    }

    #[test]
    fn cap_per_cell_lead_day_is_deterministic() {
        let day = 1_700_006_400; // a UTC midnight
        let cs = vec![
            cand("c1", "w1", "s1", 6, day + 3600, 0.9),
            cand("c2", "w1", "s1", 6, day + 7200, 0.8), // same forecaster, same group
            cand("c3", "w2", "s1", 6, day + 3600, 0.7),
            cand("c4", "w3", "s1", 6, day + 3600, 0.6), // over cap 2
            cand("c5", "w3", "s1", 12, day + 3600, 0.6), // other lead window
            cand("c6", "w4", "s1", 6, day + 86_400, 0.6), // other day
            cand("c7", "w5", "s2", 6, day + 3600, 0.05), // below baseline
        ];
        let reasons = |cs: &[CapCandidate]| {
            select_capped(cs, 2, 0.1)
                .into_iter()
                .map(|d| (d.commit_id, d.reason))
                .collect::<Vec<_>>()
        };
        let got = reasons(&cs);
        assert_eq!(
            got,
            vec![
                ("c1".into(), "selected".into()),
                ("c2".into(), "forecaster_already_selected".into()),
                ("c3".into(), "selected".into()),
                ("c4".into(), "cell_lead_cap".into()),
                ("c5".into(), "selected".into()),
                ("c6".into(), "selected".into()),
                ("c7".into(), "below_baseline".into()),
            ]
        );
        let mut reversed = cs.clone();
        reversed.reverse();
        assert_eq!(reasons(&reversed), got);
        assert_eq!(CapKey::of(&cand("x", "w", "s", 7, 0, 0.5)).lead_window, 2);
    }
}
