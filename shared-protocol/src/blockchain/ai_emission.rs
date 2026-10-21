//! AI-driven emission rate adjustment based on coverage metrics.
//!
//! The [`AiEmissionController`] ingests network coverage data —
//! geographic coverage fraction, average data quality, and per-region
//! contributor counts — then computes [`EmissionAdjustment`]s that tune
//! the token emission throttle and region weights.
//!
//! Per the PRD the throttle *opens* as coverage approaches 100 %.
//! Adjustments are published as governance proposals with
//! `ai_generated: true` so that a challenge period allows human override.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/// Coverage metrics ingested by the AI emission controller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageMetrics {
    /// Global geographic coverage as a fraction in `[0.0, 1.0]`.
    pub global_coverage: f64,
    /// Average data quality score across all regions (`[0.0, 1.0]`).
    pub avg_quality: f64,
    /// Active contributors per region.
    pub contributors_per_region: HashMap<String, u64>,
    /// Total active contributors across the network.
    pub total_contributors: u64,
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// An emission adjustment proposed by the AI controller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmissionAdjustment {
    /// Proposed emission throttle in `[min_throttle, max_throttle]`.
    pub emission_throttle: f64,
    /// Proposed region weight updates.
    pub region_weights: HashMap<String, f64>,
    /// Human-readable rationale for the adjustment.
    pub rationale: String,
}

// ---------------------------------------------------------------------------
// Controller
// ---------------------------------------------------------------------------

/// AI-driven emission rate controller.
///
/// Tunes emission parameters based on network coverage metrics.
/// All tuneable knobs are public so that governance can override them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AiEmissionController {
    /// Minimum coverage to start opening throttle (default: 0.1 = 10 %).
    pub coverage_floor: f64,
    /// Coverage at which throttle is fully open (default: 1.0 = 100 %).
    pub coverage_ceiling: f64,
    /// Minimum throttle value — never fully closed (default: 0.1).
    pub min_throttle: f64,
    /// Maximum throttle value (default: 1.0).
    pub max_throttle: f64,
    /// Multiplier for underserved regions (fewer contributors => higher weight).
    pub underserved_region_boost: f64,
    /// Minimum contributors for a region to be considered "served".
    pub served_threshold: u64,
    /// Challenge period in blocks before auto-execution (default: ~48 h at 20 s blocks).
    pub challenge_period_blocks: u64,
    /// Whether the controller is paused (emergency stop).
    pub paused: bool,
}

impl Default for AiEmissionController {
    fn default() -> Self {
        Self {
            coverage_floor: 0.1,
            coverage_ceiling: 1.0,
            min_throttle: 0.1,
            max_throttle: 1.0,
            underserved_region_boost: 1.5,
            served_threshold: 5,
            challenge_period_blocks: 8640, // ~48 h at ~20 s blocks
            paused: false,
        }
    }
}

impl AiEmissionController {
    /// Create a controller with default parameters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Compute an emission adjustment from the given coverage metrics.
    ///
    /// Returns `None` if the controller is paused.
    pub fn compute_adjustment(&self, metrics: &CoverageMetrics) -> Option<EmissionAdjustment> {
        if self.paused {
            return None;
        }

        // --- throttle: linear ramp from floor to ceiling -----------------
        let coverage_range = (self.coverage_ceiling - self.coverage_floor).max(0.001);
        let normalized =
            ((metrics.global_coverage - self.coverage_floor) / coverage_range).clamp(0.0, 1.0);
        let throttle_range = self.max_throttle - self.min_throttle;
        let emission_throttle = self.min_throttle + normalized * throttle_range;

        // Quality gate: poor quality scales down the throttle.
        let quality_factor = metrics.avg_quality.clamp(0.0, 1.0);
        let emission_throttle =
            (emission_throttle * (0.5 + 0.5 * quality_factor)).clamp(self.min_throttle, self.max_throttle);

        // --- region weights: boost underserved regions -------------------
        let mut region_weights = HashMap::new();
        let avg_contributors = if metrics.contributors_per_region.is_empty() {
            0
        } else {
            metrics.total_contributors / metrics.contributors_per_region.len().max(1) as u64
        };

        for (region, &count) in &metrics.contributors_per_region {
            let weight = if count < self.served_threshold {
                // Underserved: full boost.
                self.underserved_region_boost
            } else if avg_contributors > 0 && count < avg_contributors {
                // Below average: partial boost.
                1.0 + (self.underserved_region_boost - 1.0)
                    * (1.0 - count as f64 / avg_contributors as f64)
            } else {
                1.0
            };
            region_weights.insert(region.clone(), weight);
        }

        let rationale = format!(
            "Coverage {:.1}% (throttle {:.2}), quality {:.1}%, {} regions, {} contributors",
            metrics.global_coverage * 100.0,
            emission_throttle,
            metrics.avg_quality * 100.0,
            metrics.contributors_per_region.len(),
            metrics.total_contributors,
        );

        Some(EmissionAdjustment {
            emission_throttle,
            region_weights,
            rationale,
        })
    }

    /// Validate that an adjustment is within safe bounds.
    pub fn validate_adjustment(&self, adj: &EmissionAdjustment) -> Result<(), String> {
        if adj.emission_throttle < self.min_throttle || adj.emission_throttle > self.max_throttle {
            return Err(format!(
                "Throttle {:.2} outside bounds [{:.2}, {:.2}]",
                adj.emission_throttle, self.min_throttle, self.max_throttle
            ));
        }
        for (region, &weight) in &adj.region_weights {
            if weight < 0.0 {
                return Err(format!(
                    "Negative weight {:.2} for region {}",
                    weight, region
                ));
            }
            let cap = self.underserved_region_boost * 2.0;
            if weight > cap {
                return Err(format!(
                    "Weight {:.2} for region {} exceeds safety cap {:.2}",
                    weight, region, cap
                ));
            }
        }
        Ok(())
    }

    /// Pause the controller (emergency stop).
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Resume the controller.
    pub fn resume(&mut self) {
        self.paused = false;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_metrics(
        coverage: f64,
        quality: f64,
        regions: Vec<(&str, u64)>,
    ) -> CoverageMetrics {
        let total: u64 = regions.iter().map(|(_, c)| c).sum();
        CoverageMetrics {
            global_coverage: coverage,
            avg_quality: quality,
            contributors_per_region: regions
                .into_iter()
                .map(|(r, c)| (r.to_string(), c))
                .collect(),
            total_contributors: total,
        }
    }

    #[test]
    fn test_paused_returns_none() {
        let mut ctrl = AiEmissionController::new();
        ctrl.pause();
        let metrics = make_metrics(0.5, 0.9, vec![("us-east", 10)]);
        assert!(ctrl.compute_adjustment(&metrics).is_none());
    }

    #[test]
    fn test_zero_coverage_gives_min_throttle() {
        let ctrl = AiEmissionController::new();
        let metrics = make_metrics(0.0, 1.0, vec![("us-east", 10)]);
        let adj = ctrl.compute_adjustment(&metrics).unwrap();
        // coverage 0.0 < floor 0.1 => normalized = 0 => throttle = min
        // quality 1.0 => factor = 1.0 => scaled = min * 1.0 = min
        assert!(
            (adj.emission_throttle - ctrl.min_throttle).abs() < 1e-9,
            "Expected min throttle {}, got {}",
            ctrl.min_throttle,
            adj.emission_throttle
        );
    }

    #[test]
    fn test_full_coverage_gives_max_throttle() {
        let ctrl = AiEmissionController::new();
        let metrics = make_metrics(1.0, 1.0, vec![("us-east", 10)]);
        let adj = ctrl.compute_adjustment(&metrics).unwrap();
        // coverage 1.0 => normalized = 1.0 => throttle = max
        // quality 1.0 => factor = 1.0 => scaled = max * 1.0 = max
        assert!(
            (adj.emission_throttle - ctrl.max_throttle).abs() < 1e-9,
            "Expected max throttle {}, got {}",
            ctrl.max_throttle,
            adj.emission_throttle
        );
    }

    #[test]
    fn test_half_coverage() {
        let ctrl = AiEmissionController::new();
        // coverage = 0.55, midpoint of [0.1, 1.0] range
        let metrics = make_metrics(0.55, 1.0, vec![("us-east", 10)]);
        let adj = ctrl.compute_adjustment(&metrics).unwrap();
        // normalized = (0.55 - 0.1) / 0.9 = 0.5
        // throttle = 0.1 + 0.5 * 0.9 = 0.55
        // quality 1.0 => factor 1.0 => no change
        assert!(
            (adj.emission_throttle - 0.55).abs() < 1e-9,
            "Expected ~0.55, got {}",
            adj.emission_throttle
        );
    }

    #[test]
    fn test_underserved_regions_boosted() {
        let ctrl = AiEmissionController::new();
        let metrics = make_metrics(
            0.5,
            1.0,
            vec![("us-east", 20), ("africa-west", 2), ("eu-central", 15)],
        );
        let adj = ctrl.compute_adjustment(&metrics).unwrap();
        // africa-west has 2 contributors < served_threshold (5) => full boost
        let africa_weight = adj.region_weights.get("africa-west").unwrap();
        assert!(
            (*africa_weight - ctrl.underserved_region_boost).abs() < 1e-9,
            "Expected underserved boost {}, got {}",
            ctrl.underserved_region_boost,
            africa_weight
        );
        // us-east has 20 contributors >= average => weight 1.0
        let us_weight = adj.region_weights.get("us-east").unwrap();
        assert!(
            (*us_weight - 1.0).abs() < 1e-9,
            "Expected 1.0, got {}",
            us_weight
        );
    }

    #[test]
    fn test_quality_gates_throttle() {
        let ctrl = AiEmissionController::new();
        // Full coverage but low quality
        let metrics = make_metrics(1.0, 0.0, vec![("us-east", 10)]);
        let adj = ctrl.compute_adjustment(&metrics).unwrap();
        // throttle_raw = 1.0, quality factor = 0.0 => scaled = 1.0 * (0.5 + 0.0) = 0.5
        assert!(
            (adj.emission_throttle - 0.5).abs() < 1e-9,
            "Expected 0.5, got {}",
            adj.emission_throttle
        );
    }

    #[test]
    fn test_validate_rejects_negative_weight() {
        let ctrl = AiEmissionController::new();
        let adj = EmissionAdjustment {
            emission_throttle: 0.5,
            region_weights: [("bad-region".to_string(), -0.5)].into_iter().collect(),
            rationale: "test".to_string(),
        };
        assert!(ctrl.validate_adjustment(&adj).is_err());
    }

    #[test]
    fn test_validate_rejects_excessive_weight() {
        let ctrl = AiEmissionController::new();
        let cap = ctrl.underserved_region_boost * 2.0;
        let adj = EmissionAdjustment {
            emission_throttle: 0.5,
            region_weights: [("over-region".to_string(), cap + 1.0)].into_iter().collect(),
            rationale: "test".to_string(),
        };
        assert!(ctrl.validate_adjustment(&adj).is_err());
    }

    #[test]
    fn test_validate_rejects_out_of_bounds_throttle() {
        let ctrl = AiEmissionController::new();
        let adj = EmissionAdjustment {
            emission_throttle: 1.5, // above max
            region_weights: HashMap::new(),
            rationale: "test".to_string(),
        };
        assert!(ctrl.validate_adjustment(&adj).is_err());

        let adj_low = EmissionAdjustment {
            emission_throttle: 0.01, // below min
            region_weights: HashMap::new(),
            rationale: "test".to_string(),
        };
        assert!(ctrl.validate_adjustment(&adj_low).is_err());
    }
}
