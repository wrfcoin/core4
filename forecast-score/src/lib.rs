//! Forecast verification metrics for meteorological and probabilistic predictions.
//!
//! Provides standard verification scores used in numerical weather prediction (NWP),
//! prediction markets, and other forecasting domains.
//!
//! # Metric Categories
//!
//! - **Continuous**: [`mae`], [`rmse`], [`bias`] — for temperature, wind speed, etc.
//! - **Categorical**: [`pod`], [`far`], [`csi`], [`categorical_scores`] — for binary events (rain/no-rain)
//! - **Probabilistic**: [`brier_score`], [`crps`], [`crps_normalized`] — for probability forecasts
//! - **Composite**: [`ForecastAccuracyMetrics`], [`CategoricalScores`] — bundled results
//!
//! # Blockchain Types
//!
//! [`ForecastArtifactManifest`] and [`ForecastVerificationReceipt`] encode the
//! consensus-critical structures used by the WRFCoin Proof-of-Publishing protocol.

pub mod anti_gaming;
pub mod blockchain;
pub mod categorical;
pub mod continuous;
pub mod probabilistic;

// Re-export blockchain types at crate root (consensus-critical).
pub use blockchain::{
    CHAIN_ID_ENV, ChainBinding, ChainBindingError, ForecastArtifactManifest,
    ForecastManifestValidationError, ForecastMetricsValidationError,
    ForecastReceiptValidationError, ForecastVerificationReceipt, NETWORK_ID_ENV, validate_metrics,
};

// Re-export metric functions at crate root.
pub use anti_gaming::{ReferenceStrategy, persistence_rmse};
pub use categorical::{CategoricalScores, categorical_scores, csi, far, pod};
pub use continuous::{bias, mae, rmse};
pub use probabilistic::{
    brier_score, brier_score_checked, crps, crps_checked, crps_normalized, crps_normalized_checked,
};

use serde::{Deserialize, Serialize};

/// Errors returned by fallible scoring functions.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ScoreError {
    /// Input slices have different lengths.
    LengthMismatch { expected: usize, got: usize },
    /// An input value is outside the valid domain.
    InvalidInput(&'static str),
}

impl core::fmt::Display for ScoreError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ScoreError::LengthMismatch { expected, got } => {
                write!(f, "length mismatch: expected {expected}, got {got}")
            },
            ScoreError::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
        }
    }
}

impl std::error::Error for ScoreError {}

/// A forecast–observation pair.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pair {
    /// The predicted value from the forecast model.
    pub predicted: f64,
    /// The actual observed value.
    pub observed: f64,
}

impl Pair {
    /// Create a new forecast–observation pair.
    pub fn new(predicted: f64, observed: f64) -> Self {
        Self {
            predicted,
            observed,
        }
    }
}

/// Accuracy metrics from forecast verification.
///
/// Compatible with `ForecastAccuracyMetrics` in WRFCoin's shared-protocol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForecastAccuracyMetrics {
    /// Mean absolute error.
    pub mae: f64,
    /// Root mean squared error.
    pub rmse: f64,
    /// Mean signed error.
    pub bias: f64,
    /// Skill score normalized to the inclusive range `[0.0, 1.0]`.
    pub skill_score: f64,
}

impl ForecastAccuracyMetrics {
    /// Compute all continuous metrics from a set of pairs, with a composite skill score.
    ///
    /// The skill score is defined as `1.0 - (rmse / ref_rmse)`, clamped to `[0, 1]`,
    /// where `ref_rmse` is the **population standard deviation of the observed values**
    /// (equivalent to the RMSE of the climatological mean forecast).
    pub fn from_pairs(pairs: &[Pair]) -> Self {
        let mae_val = mae(pairs);
        let rmse_val = rmse(pairs);
        let bias_val = bias(pairs);
        let skill_score = if pairs.is_empty() {
            0.0
        } else {
            let mean_obs = pairs.iter().map(|p| p.observed).sum::<f64>() / pairs.len() as f64;
            let variance = pairs
                .iter()
                .map(|p| (p.observed - mean_obs).powi(2))
                .sum::<f64>()
                / pairs.len() as f64;
            let ref_rmse = variance.sqrt();
            if ref_rmse > 0.0 {
                (1.0 - rmse_val / ref_rmse).clamp(0.0, 1.0)
            } else if rmse_val == 0.0 {
                1.0
            } else {
                0.0
            }
        };
        Self {
            mae: mae_val,
            rmse: rmse_val,
            bias: bias_val,
            skill_score,
        }
    }

    /// Validate that all fields are finite and within expected ranges.
    ///
    /// Returns typed [`ForecastMetricsValidationError`] suitable for embedding
    /// in [`ForecastReceiptValidationError::InvalidMetrics`].
    pub fn validate(&self) -> Result<(), ForecastMetricsValidationError> {
        validate_metrics(self)
    }

    // ------------------------------------------------------------------
    // Backward-compatible static methods used by shared-protocol tests.
    // These accept raw slices instead of `&[Pair]`.
    // ------------------------------------------------------------------

    /// Computes mean absolute error from paired observation/prediction slices.
    pub fn mae_slices(observed: &[f64], predicted: &[f64]) -> Result<f64, ScoreError> {
        ensure_equal_nonempty(observed.len(), predicted.len())?;
        let pairs: Vec<Pair> = observed
            .iter()
            .zip(predicted.iter())
            .map(|(&o, &p)| Pair::new(p, o))
            .collect();
        Ok(mae(&pairs))
    }

    /// Computes root mean squared error from paired observation/prediction slices.
    pub fn rmse_slices(observed: &[f64], predicted: &[f64]) -> Result<f64, ScoreError> {
        ensure_equal_nonempty(observed.len(), predicted.len())?;
        let pairs: Vec<Pair> = observed
            .iter()
            .zip(predicted.iter())
            .map(|(&o, &p)| Pair::new(p, o))
            .collect();
        Ok(rmse(&pairs))
    }

    /// Computes the Brier score from paired observation/prediction slices.
    pub fn brier_score_slices(observed: &[bool], predicted: &[f64]) -> Result<f64, ScoreError> {
        ensure_equal_nonempty(observed.len(), predicted.len())?;
        let outcomes: Vec<f64> = observed
            .iter()
            .map(|&b| if b { 1.0 } else { 0.0 })
            .collect();
        brier_score_checked(predicted, &outcomes)
    }
}

/// Validates input slices are non-empty and equal length.
fn ensure_equal_nonempty(lhs: usize, rhs: usize) -> Result<(), ScoreError> {
    if lhs == 0 || rhs == 0 {
        return Err(ScoreError::InvalidInput("input slices must not be empty"));
    }
    if lhs != rhs {
        return Err(ScoreError::LengthMismatch {
            expected: lhs,
            got: rhs,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(data: &[(f64, f64)]) -> Vec<Pair> {
        data.iter().map(|&(p, o)| Pair::new(p, o)).collect()
    }

    #[test]
    fn from_pairs_empty() {
        let metrics = ForecastAccuracyMetrics::from_pairs(&[]);
        assert_eq!(metrics.mae, 0.0);
        assert_eq!(metrics.rmse, 0.0);
        assert_eq!(metrics.bias, 0.0);
        assert_eq!(metrics.skill_score, 0.0);
    }

    #[test]
    fn from_pairs_perfect_forecast() {
        let metrics = ForecastAccuracyMetrics::from_pairs(&pairs(&[
            (10.0, 10.0),
            (20.0, 20.0),
            (30.0, 30.0),
        ]));
        assert_eq!(metrics.mae, 0.0);
        assert_eq!(metrics.rmse, 0.0);
        assert_eq!(metrics.bias, 0.0);
        assert_eq!(metrics.skill_score, 1.0);
    }

    #[test]
    fn from_pairs_identical_observations_nonzero_error() {
        let metrics = ForecastAccuracyMetrics::from_pairs(&pairs(&[(5.0, 5.0), (6.0, 5.0)]));
        assert_eq!(metrics.skill_score, 0.0);
    }

    #[test]
    fn from_pairs_identical_obs_perfect_forecast() {
        let metrics =
            ForecastAccuracyMetrics::from_pairs(&pairs(&[(5.0, 5.0), (5.0, 5.0), (5.0, 5.0)]));
        assert_eq!(metrics.rmse, 0.0);
        assert_eq!(metrics.skill_score, 1.0);
    }

    #[test]
    fn validate_accepts_valid_metrics() {
        let metrics = ForecastAccuracyMetrics {
            mae: 0.1,
            rmse: 0.2,
            bias: -0.1,
            skill_score: 0.7,
        };
        assert!(metrics.validate().is_ok());
    }

    #[test]
    fn validate_rejects_negative_mae() {
        let metrics = ForecastAccuracyMetrics {
            mae: -0.1,
            rmse: 0.2,
            bias: 0.0,
            skill_score: 0.7,
        };
        assert!(matches!(
            metrics.validate(),
            Err(ForecastMetricsValidationError::InvalidMae)
        ));
    }

    #[test]
    fn score_error_display() {
        let e = ScoreError::LengthMismatch {
            expected: 3,
            got: 2,
        };
        assert_eq!(e.to_string(), "length mismatch: expected 3, got 2");
        let e2 = ScoreError::InvalidInput("bad value");
        assert_eq!(e2.to_string(), "invalid input: bad value");
    }

    #[test]
    fn compat_mae_slices() {
        let result = ForecastAccuracyMetrics::mae_slices(&[1.0, 2.0], &[1.0, 2.0]).unwrap();
        assert_eq!(result, 0.0);
    }

    #[test]
    fn compat_rmse_slices() {
        let result = ForecastAccuracyMetrics::rmse_slices(&[1.0, 2.0], &[2.0, 4.0]).unwrap();
        assert!((result - (2.5_f64).sqrt()).abs() < 1e-12);
    }

    #[test]
    fn compat_brier_score_slices() {
        let result =
            ForecastAccuracyMetrics::brier_score_slices(&[true, false], &[0.8, 0.2]).unwrap();
        assert!((result - 0.04).abs() < 1e-12);
    }
}

#[cfg(test)]
mod wire_layout_fixtures;

#[cfg(test)]
mod baseline_port_tests {
    use super::*;

    #[test]
    fn persistence_copy_earns_zero_baseline_skill() {
        let pairs = [
            Pair::new(0.0, 1.0),
            Pair::new(1.0, 2.0),
            Pair::new(2.0, 3.0),
            Pair::new(3.0, 4.0),
        ];
        assert!(ForecastAccuracyMetrics::from_pairs(&pairs).skill_score > 0.0);
        let metrics = ForecastAccuracyMetrics::from_pairs_with_reference(
            &pairs,
            ReferenceStrategy::Persistence,
        )
        .unwrap();
        assert_eq!(metrics.skill_score, 0.0);
    }
}
