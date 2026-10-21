//! Scorer-side feature selection; no node recomputes these floating metrics.
use super::{FORECAST_VARIABLES, ForecastOutput, Observation, ScoreReport};
use anyhow::{Result, anyhow, ensure};
use std::collections::BTreeMap;
use wrfcoin_forecast_score::{ForecastAccuracyMetrics, Pair, anti_gaming::ReferenceStrategy};
use wrfcoin_shared_protocol::crypto::chain_binding::ConsensusFeature;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillPolicy {
    Legacy,
    Persistence,
}

impl SkillPolicy {
    /// Match the node's height+1 feature view, including applied extensions.
    /// Older nodes omit active_features and retain legacy scoring. Malformed
    /// lists or another chain's view stop before any transaction is published.
    pub fn from_pending(view: &serde_json::Value, chain: &str, network: &str) -> Result<Self> {
        ensure!(
            view["chain_id"].as_str() == Some(chain)
                && view["network_id"].as_str() == Some(network),
            "scorer feature view chain binding mismatch"
        );
        ensure!(
            view["height"].as_u64().is_some(),
            "scorer feature view has no height"
        );
        let Some(features) = view.get("active_features") else {
            return Ok(Self::Legacy);
        };
        let features = features
            .as_array()
            .ok_or_else(|| anyhow!("invalid active_features list"))?;
        let mut active = false;
        for feature in features {
            let name = feature
                .as_str()
                .ok_or_else(|| anyhow!("invalid active_features name"))?;
            active |= name == ConsensusFeature::ForecastSkillBaseline.name();
        }
        Ok(if active {
            Self::Persistence
        } else {
            Self::Legacy
        })
    }

    /// Strategy metadata is bound by the existing signed manifest digest.
    /// Reusing model_id avoids changing the manifest's positional wire layout.
    pub fn manifest_model_id(self, model: &str) -> String {
        match self {
            Self::Legacy => model.to_string(),
            Self::Persistence => format!("{model};skill=persistence-rmse/v1"),
        }
    }
}

pub fn score(
    forecast: &ForecastOutput,
    truth: &Observation,
    climatology: &BTreeMap<String, f64>,
    previous: &Observation,
    policy: SkillPolicy,
) -> Result<ScoreReport> {
    if policy == SkillPolicy::Legacy {
        return Ok(super::score(forecast, truth, climatology));
    }
    ensure!(
        previous.timestamp < truth.timestamp,
        "persistence seed must precede truth"
    );
    let mut report = super::score(forecast, truth, climatology);
    let mut skills = Vec::new();
    // Never mix pressure, humidity and temperature units in one RMSE. Each
    // variable compares the forecast and last input observation at the SAME
    // truth target. The seed pair is excluded by the ported persistence helper.
    for variable in FORECAST_VARIABLES {
        let get = |values: &BTreeMap<String, f64>| {
            values
                .get(variable)
                .copied()
                .ok_or_else(|| anyhow!("missing scoring variable {variable}"))
        };
        let seed = get(&previous.values)?;
        let pairs = [
            Pair::new(seed, seed),
            Pair::new(get(&forecast.values)?, get(&truth.values)?),
        ];
        let metrics = ForecastAccuracyMetrics::from_pairs_with_reference(
            &pairs,
            ReferenceStrategy::Persistence,
        )
        .map_err(|error| anyhow!("invalid persistence score: {error}"))?;
        report
            .per_variable
            .get_mut(variable)
            .expect("legacy scorer covers all variables")
            .skill = metrics.skill_score;
        skills.push(metrics.skill_score);
    }
    report.metrics.skill_score = skills.iter().sum::<f64>() / skills.len() as f64;
    // Finite variable errors can still overflow the aggregate sum of squares.
    // Refuse before main publishes the commit, rather than emitting null metrics.
    ensure!(
        [
            report.metrics.mae,
            report.metrics.rmse,
            report.metrics.bias,
            report.metrics.skill_score,
        ]
        .into_iter()
        .all(f64::is_finite),
        "invalid persistence score: nonfinite aggregate metric"
    );
    Ok(report)
}
