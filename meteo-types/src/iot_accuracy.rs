//! IoT sensor accuracy verification metrics.
//!
//! Types for representing how accurate an IoT sensor's readings are compared to
//! reference values (official weather stations or consensus of nearby sensors).
//! Used in the WRFCoin reward pipeline where more accurate sensors earn higher rewards.

use serde::{Deserialize, Serialize};

use crate::DataQualityBand;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors returned by IoT accuracy metric validation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IoTAccuracyError {
    /// The MAE value is not finite or is negative.
    InvalidMae,
    /// The RMSE value is not finite or is negative.
    InvalidRmse,
    /// The bias value is not finite.
    InvalidBias,
    /// The correlation value is not finite or not in \[-1.0, 1.0\].
    InvalidCorrelation,
    /// The accuracy score is not finite or not in \[0.0, 1.0\].
    InvalidAccuracyScore,
    /// The sample count is zero.
    ZeroSampleCount,
}

impl core::fmt::Display for IoTAccuracyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidMae => write!(f, "mae must be finite and non-negative"),
            Self::InvalidRmse => write!(f, "rmse must be finite and non-negative"),
            Self::InvalidBias => write!(f, "bias must be finite"),
            Self::InvalidCorrelation => write!(f, "correlation must be finite and in [-1, 1]"),
            Self::InvalidAccuracyScore => {
                write!(f, "accuracy_score must be finite and in [0, 1]")
            }
            Self::ZeroSampleCount => write!(f, "sample count must be greater than zero"),
        }
    }
}

impl std::error::Error for IoTAccuracyError {}

// ---------------------------------------------------------------------------
// SensorReferencePair
// ---------------------------------------------------------------------------

/// A single sensor reading paired with a reference (ground truth) value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SensorReferencePair {
    /// Value reported by the IoT sensor.
    pub sensor_value: f64,
    /// Reference value from an official station or consensus.
    pub reference_value: f64,
}

impl SensorReferencePair {
    /// Create a new sensor/reference pair.
    pub fn new(sensor_value: f64, reference_value: f64) -> Self {
        Self {
            sensor_value,
            reference_value,
        }
    }

    /// Signed error (sensor minus reference). Positive means sensor reads high.
    pub fn error(&self) -> f64 {
        self.sensor_value - self.reference_value
    }

    /// Absolute error.
    pub fn abs_error(&self) -> f64 {
        (self.sensor_value - self.reference_value).abs()
    }
}

// ---------------------------------------------------------------------------
// SensorAccuracyMetrics
// ---------------------------------------------------------------------------

/// Accuracy metrics for an IoT sensor compared against reference observations.
///
/// All error metrics use the same units as the underlying measurement (e.g.,
/// degrees C for temperature sensors, hPa for pressure sensors).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorAccuracyMetrics {
    /// Mean Absolute Error. Non-negative.
    pub mae: f64,
    /// Root Mean Squared Error. Non-negative.
    pub rmse: f64,
    /// Mean signed bias (sensor - reference). Positive means sensor reads high.
    pub bias: f64,
    /// Pearson correlation coefficient. In \[-1.0, 1.0\].
    pub correlation: f64,
    /// Number of comparison pairs used to compute these metrics.
    pub sample_count: u32,
    /// Composite accuracy score in \[0.0, 1.0\] where 1.0 is perfect.
    pub accuracy_score: f64,
}

impl SensorAccuracyMetrics {
    /// Compute accuracy metrics from a slice of sensor/reference pairs.
    ///
    /// Returns [`IoTAccuracyError::ZeroSampleCount`] if `pairs` is empty.
    pub fn from_pairs(pairs: &[SensorReferencePair]) -> Result<Self, IoTAccuracyError> {
        if pairs.is_empty() {
            return Err(IoTAccuracyError::ZeroSampleCount);
        }

        let n = pairs.len() as f64;

        // Means
        let sum_abs_err: f64 = pairs.iter().map(|p| p.abs_error()).sum();
        let sum_sq_err: f64 = pairs.iter().map(|p| p.error() * p.error()).sum();
        let sum_err: f64 = pairs.iter().map(|p| p.error()).sum();

        let mae = sum_abs_err / n;
        let rmse = (sum_sq_err / n).sqrt();
        let bias = sum_err / n;

        // Reference statistics for correlation and accuracy_score
        let mean_ref: f64 = pairs.iter().map(|p| p.reference_value).sum::<f64>() / n;
        let mean_sen: f64 = pairs.iter().map(|p| p.sensor_value).sum::<f64>() / n;

        let var_ref: f64 =
            pairs.iter().map(|p| (p.reference_value - mean_ref).powi(2)).sum::<f64>() / n;
        let var_sen: f64 =
            pairs.iter().map(|p| (p.sensor_value - mean_sen).powi(2)).sum::<f64>() / n;
        let cov: f64 = pairs
            .iter()
            .map(|p| (p.sensor_value - mean_sen) * (p.reference_value - mean_ref))
            .sum::<f64>()
            / n;

        let std_ref = var_ref.sqrt();
        let std_sen = var_sen.sqrt();

        // Pearson correlation — 0.0 for degenerate (zero variance) cases
        let correlation = if std_ref > 0.0 && std_sen > 0.0 {
            (cov / (std_ref * std_sen)).clamp(-1.0, 1.0)
        } else {
            0.0
        };

        // Accuracy score: skill-score formula (same as forecast-score crate)
        let accuracy_score = if std_ref > 0.0 {
            (1.0 - rmse / std_ref).clamp(0.0, 1.0)
        } else if rmse == 0.0 {
            1.0
        } else {
            0.0
        };

        Ok(Self {
            mae,
            rmse,
            bias,
            correlation,
            sample_count: pairs.len() as u32,
            accuracy_score,
        })
    }

    /// Validate that all fields are finite and within expected ranges.
    #[must_use = "validation errors must be handled"]
    pub fn validate(&self) -> Result<(), IoTAccuracyError> {
        if !self.mae.is_finite() || self.mae < 0.0 {
            return Err(IoTAccuracyError::InvalidMae);
        }
        if !self.rmse.is_finite() || self.rmse < 0.0 {
            return Err(IoTAccuracyError::InvalidRmse);
        }
        if !self.bias.is_finite() {
            return Err(IoTAccuracyError::InvalidBias);
        }
        if !self.correlation.is_finite() || !(-1.0..=1.0).contains(&self.correlation) {
            return Err(IoTAccuracyError::InvalidCorrelation);
        }
        if !self.accuracy_score.is_finite() || !(0.0..=1.0).contains(&self.accuracy_score) {
            return Err(IoTAccuracyError::InvalidAccuracyScore);
        }
        if self.sample_count == 0 {
            return Err(IoTAccuracyError::ZeroSampleCount);
        }
        Ok(())
    }

    /// Classify this accuracy into a [`SensorAccuracyBand`].
    pub fn band(&self) -> SensorAccuracyBand {
        SensorAccuracyBand::from_score(self.accuracy_score)
    }

    /// Map this accuracy to the equivalent [`DataQualityBand`].
    pub fn quality_band(&self) -> DataQualityBand {
        self.band().to_quality_band()
    }
}

// ---------------------------------------------------------------------------
// SensorAccuracyBand
// ---------------------------------------------------------------------------

/// Classification of sensor accuracy for reward calculation.
///
/// Thresholds: Excellent >= 0.90, Good >= 0.75, Marginal >= 0.50, Poor < 0.50.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SensorAccuracyBand {
    /// Excellent accuracy — full reward eligibility.
    Excellent,
    /// Good accuracy — standard reward eligibility.
    Good,
    /// Marginal accuracy — reduced rewards, flagged for calibration.
    Marginal,
    /// Poor accuracy — no rewards, may trigger slashing.
    Poor,
}

impl SensorAccuracyBand {
    /// Classify an accuracy score into a band.
    pub fn from_score(accuracy_score: f64) -> Self {
        if accuracy_score >= 0.90 {
            Self::Excellent
        } else if accuracy_score >= 0.75 {
            Self::Good
        } else if accuracy_score >= 0.50 {
            Self::Marginal
        } else {
            Self::Poor
        }
    }

    /// Convert to the corresponding [`DataQualityBand`].
    pub fn to_quality_band(self) -> DataQualityBand {
        match self {
            Self::Excellent => DataQualityBand::High,
            Self::Good => DataQualityBand::Borderline,
            Self::Marginal => DataQualityBand::WarningOnly,
            Self::Poor => DataQualityBand::Low,
        }
    }
}

impl core::fmt::Display for SensorAccuracyBand {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Excellent => write!(f, "Excellent"),
            Self::Good => write!(f, "Good"),
            Self::Marginal => write!(f, "Marginal"),
            Self::Poor => write!(f, "Poor"),
        }
    }
}
