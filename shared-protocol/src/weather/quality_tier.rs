use serde::{Deserialize, Serialize};

/// Collector-assigned quality tier for a weather reading submission.
///
/// Computed from a multi-reference gridded comparison (HRRR/GFS analysis,
/// MRMS, ERA5 — the "Option C hybrid policy", implemented in
/// [`super::reference`]; per-source residual bands live in
/// `fixtures/reference_bands.json`):
///   - When no reference is reachable: `Good` (no gating without reference
///     data).
///   - When every per-variable residual is within its source's "Good" band:
///     `Good`.
///   - When the folded (second-worst, #3098) residual tier hits the
///     "Marginal" band, or a single variable is an outlier beside others:
///     `Degraded` (reduced reward multiplier on-chain).
///   - When two or more variables hit the "Poor" band, or the only scored
///     variable does: `Outlier` (minimal reward multiplier on-chain, but
///     still accepted — never hard-rejected pre-mainnet).
///
/// The on-chain reward multiplier is governance-controlled; this tier is
/// the per-reading input that feeds into that computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReadingQualityTier {
    /// High-confidence reading: NWS comparison within "Good" band, or NWS
    /// unavailable.
    #[default]
    Good,
    /// Suspicious reading: NWS comparison reached "Marginal" band for at least
    /// one primary variable.
    Degraded,
    /// Persistent outlier: NWS comparison reached "Poor" band for at least one
    /// primary variable.  Still accepted on-chain; reward multiplier is minimal.
    Outlier,
}

impl ReadingQualityTier {
    /// Reward multiplier for this tier (governance can override; this is the
    /// default schedule).
    pub fn default_reward_multiplier(self) -> f64 {
        match self {
            Self::Good => 1.0,
            Self::Degraded => 0.5,
            Self::Outlier => 0.1,
        }
    }

    /// Returns the most conservative (lowest-reward) tier of `self` and `other`.
    pub fn worst(self, other: Self) -> Self {
        match (self, other) {
            (Self::Outlier, _) | (_, Self::Outlier) => Self::Outlier,
            (Self::Degraded, _) | (_, Self::Degraded) => Self::Degraded,
            _ => Self::Good,
        }
    }
}

impl std::fmt::Display for ReadingQualityTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Good => write!(f, "good"),
            Self::Degraded => write!(f, "degraded"),
            Self::Outlier => write!(f, "outlier"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_reading_tier_is_good() {
        let tier = ReadingQualityTier::Good;
        assert_eq!(tier.default_reward_multiplier(), 1.0);
        assert_eq!(tier.to_string(), "good");
    }

    #[test]
    fn outlier_reading_with_nws_reachable_is_degraded_or_outlier() {
        // A "Marginal" band result should produce Degraded
        let tier = ReadingQualityTier::Degraded;
        assert_eq!(tier.default_reward_multiplier(), 0.5);
        assert!(tier.default_reward_multiplier() < ReadingQualityTier::Good.default_reward_multiplier());
    }

    #[test]
    fn outlier_with_nws_unreachable_is_good() {
        // When NWS is unreachable the collector falls back to Good — no gating without data.
        let tier = ReadingQualityTier::default();
        assert_eq!(tier, ReadingQualityTier::Good);
    }

    #[test]
    fn worst_combines_tiers_correctly() {
        assert_eq!(ReadingQualityTier::Good.worst(ReadingQualityTier::Good), ReadingQualityTier::Good);
        assert_eq!(ReadingQualityTier::Good.worst(ReadingQualityTier::Degraded), ReadingQualityTier::Degraded);
        assert_eq!(ReadingQualityTier::Degraded.worst(ReadingQualityTier::Outlier), ReadingQualityTier::Outlier);
        assert_eq!(ReadingQualityTier::Good.worst(ReadingQualityTier::Outlier), ReadingQualityTier::Outlier);
    }

    #[test]
    fn serde_round_trip() {
        for tier in [ReadingQualityTier::Good, ReadingQualityTier::Degraded, ReadingQualityTier::Outlier] {
            let json = serde_json::to_string(&tier).unwrap();
            let back: ReadingQualityTier = serde_json::from_str(&json).unwrap();
            assert_eq!(tier, back);
        }
    }

    #[test]
    fn serde_snake_case_values() {
        assert_eq!(serde_json::to_string(&ReadingQualityTier::Good).unwrap(), "\"good\"");
        assert_eq!(serde_json::to_string(&ReadingQualityTier::Degraded).unwrap(), "\"degraded\"");
        assert_eq!(serde_json::to_string(&ReadingQualityTier::Outlier).unwrap(), "\"outlier\"");
    }
}
