//! Scorer-side baseline helpers ported from forecast-score b2c357466.
//!
//! The forecast prototype uses these under the unscheduled forecast_skill_baseline
//! feature. Floating-point arithmetic remains off-chain only; consensus continues
//! consuming the existing authorized, signed metrics and never recomputes a baseline.

use serde::{Deserialize, Serialize};

use crate::{ForecastAccuracyMetrics, Pair, ScoreError};

/// Reference forecast against which skill is measured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReferenceStrategy {
    /// Preserve the existing sample-standard-deviation scoring byte for byte.
    SampleStdDev,
    /// Independent climatological error in the same units as the pairs.
    FixedClimatology { reference_rmse: f64 }, // determinism-ok: inert off-chain scorer API
    /// Time-ordered persistence. The first pair seeds the previous observation;
    /// skill compares both forecasts on pairs[1..], avoiding unequal windows.
    Persistence,
}

/// Persistence error over adjacent observations, requiring at least two samples.
pub fn persistence_rmse(observations: &[f64]) -> Result<f64, ScoreError> {
    // determinism-ok: inert off-chain scorer API
    if observations.len() < 2 {
        return Err(ScoreError::InvalidInput(
            "persistence needs two observations",
        ));
    }
    if observations.iter().any(|v| !v.is_finite()) {
        return Err(ScoreError::InvalidInput("observations must be finite"));
    }
    let squared_error: f64 = observations.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum(); // determinism-ok: inert off-chain scorer arithmetic
    let error = (squared_error / (observations.len() - 1) as f64).sqrt(); // determinism-ok: inert off-chain scorer arithmetic
    if !error.is_finite() {
        return Err(ScoreError::InvalidInput("persistence error overflow"));
    }
    Ok(error)
}

impl ForecastAccuracyMetrics {
    /// Checked variant of the standalone baseline scorer. Continuous metrics
    /// retain their legacy full-window definition; only skill uses a baseline.
    ///
    /// Unlike the standalone implementation, malformed/overflowing inputs and
    /// insufficient persistence history are errors. A perfect forecast tied
    /// with a zero-error persistence baseline earns zero skill, since it adds
    /// no information over persistence. Callers provide time-ordered samples.
    pub fn from_pairs_with_reference(
        pairs: &[Pair],
        reference: ReferenceStrategy,
    ) -> Result<Self, ScoreError> {
        if pairs
            .iter()
            .any(|p| !p.predicted.is_finite() || !p.observed.is_finite())
        {
            return Err(ScoreError::InvalidInput("pairs must be finite"));
        }
        let mut metrics = Self::from_pairs(pairs);
        metrics
            .validate()
            .map_err(|_| ScoreError::InvalidInput("metrics overflow"))?;
        let (forecast_error, reference_error) = match reference {
            ReferenceStrategy::SampleStdDev => return Ok(metrics),
            ReferenceStrategy::FixedClimatology { reference_rmse } => {
                if !reference_rmse.is_finite() || reference_rmse <= 0.0 {
                    return Err(ScoreError::InvalidInput(
                        "climatological RMSE must be positive and finite",
                    ));
                }
                (metrics.rmse, reference_rmse)
            },
            ReferenceStrategy::Persistence => {
                let observations: Vec<_> = pairs.iter().map(|p| p.observed).collect();
                let baseline = persistence_rmse(&observations)?;
                (crate::rmse(&pairs[1..]), baseline)
            },
        };
        if !forecast_error.is_finite() {
            return Err(ScoreError::InvalidInput("forecast error overflow"));
        }
        metrics.skill_score = if pairs.is_empty() || reference_error == 0.0 {
            0.0
        } else {
            (1.0 - forecast_error / reference_error).clamp(0.0, 1.0)
        };
        Ok(metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(pairs: &[Pair], reference: ReferenceStrategy) -> ForecastAccuracyMetrics {
        ForecastAccuracyMetrics::from_pairs_with_reference(pairs, reference).unwrap()
    }

    #[test]
    fn legacy_reference_keeps_json_and_bincode_bytes() {
        for pairs in [
            vec![],
            vec![Pair::new(1.0, 1.0)],
            vec![Pair::new(1.0, 0.0), Pair::new(2.0, 3.0)],
        ] {
            let legacy = ForecastAccuracyMetrics::from_pairs(&pairs);
            let port = score(&pairs, ReferenceStrategy::SampleStdDev);
            assert_eq!(
                serde_json::to_vec(&legacy).unwrap(),
                serde_json::to_vec(&port).unwrap()
            );
            assert_eq!(
                bincode::serialize(&legacy).unwrap(),
                bincode::serialize(&port).unwrap()
            );
        }
    }

    #[test]
    fn upstream_climatology_known_answer_and_perfect_forecast() {
        let pairs = [Pair::new(0.5, 0.0), Pair::new(0.5, 1.0)];
        let metrics = score(
            &pairs,
            ReferenceStrategy::FixedClimatology {
                reference_rmse: 1.0,
            },
        );
        assert_eq!(metrics.skill_score, 0.5);
        assert_eq!(metrics.rmse, 0.5);
        assert_eq!(
            score(
                &[Pair::new(1.0, 1.0)],
                ReferenceStrategy::FixedClimatology {
                    reference_rmse: 2.0
                }
            )
            .skill_score,
            1.0
        );
        assert_eq!(
            score(
                &[],
                ReferenceStrategy::FixedClimatology {
                    reference_rmse: 2.0
                }
            )
            .skill_score,
            0.0
        );
    }

    #[test]
    fn persistence_known_answers() {
        assert_eq!(persistence_rmse(&[10.0, 11.0, 12.0, 13.0]).unwrap(), 1.0);
        assert_eq!(persistence_rmse(&[0.0, 3.0]).unwrap(), 3.0);
        for observations in [
            vec![],
            vec![1.0],
            vec![f64::NAN, 1.0],
            vec![1.0, f64::INFINITY],
            vec![-f64::MAX, f64::MAX],
        ] {
            assert!(persistence_rmse(&observations).is_err());
        }
    }

    #[test]
    fn persistence_uses_the_same_window_as_the_forecast() {
        // The seed forecast is irrelevant to skill. Subsequent predictions
        // exactly copy the previous observation on an uneven series.
        let pairs = [
            Pair::new(99.0, 0.0),
            Pair::new(0.0, 4.0),
            Pair::new(4.0, 5.0),
            Pair::new(5.0, 9.0),
        ];
        assert_eq!(
            score(&pairs, ReferenceStrategy::Persistence).skill_score,
            0.0
        );
        let perfect = [
            Pair::new(99.0, 0.0),
            Pair::new(4.0, 4.0),
            Pair::new(5.0, 5.0),
            Pair::new(9.0, 9.0),
        ];
        assert_eq!(
            score(&perfect, ReferenceStrategy::Persistence).skill_score,
            1.0
        );
    }

    #[test]
    fn tied_zero_error_persistence_adds_no_skill() {
        let pairs = [Pair::new(5.0, 5.0), Pair::new(5.0, 5.0)];
        assert_eq!(
            score(&pairs, ReferenceStrategy::Persistence).skill_score,
            0.0
        );
        assert_eq!(ForecastAccuracyMetrics::from_pairs(&pairs).skill_score, 1.0);
    }

    #[test]
    fn reference_skill_is_clamped_and_continuous_metrics_are_legacy() {
        let pairs = [Pair::new(0.0, 10.0), Pair::new(0.0, 10.0)];
        let baseline = score(
            &pairs,
            ReferenceStrategy::FixedClimatology {
                reference_rmse: 1.0,
            },
        );
        let legacy = ForecastAccuracyMetrics::from_pairs(&pairs);
        assert_eq!(baseline.skill_score, 0.0);
        assert_eq!(
            (baseline.mae, baseline.rmse, baseline.bias),
            (legacy.mae, legacy.rmse, legacy.bias)
        );
    }

    #[test]
    fn rejects_invalid_baselines_history_pairs_and_overflow() {
        let pairs = [Pair::new(1.0, 1.0)];
        for error in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            assert!(
                ForecastAccuracyMetrics::from_pairs_with_reference(
                    &pairs,
                    ReferenceStrategy::FixedClimatology {
                        reference_rmse: error
                    }
                )
                .is_err()
            );
        }
        assert!(
            ForecastAccuracyMetrics::from_pairs_with_reference(
                &pairs,
                ReferenceStrategy::Persistence
            )
            .is_err()
        );
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            let pairs = [Pair::new(value, 0.0), Pair::new(0.0, 1.0)];
            assert!(
                ForecastAccuracyMetrics::from_pairs_with_reference(
                    &pairs,
                    ReferenceStrategy::SampleStdDev
                )
                .is_err()
            );
        }
    }
}
