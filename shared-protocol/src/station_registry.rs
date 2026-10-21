use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use wrfcoin_reputation_engine::{
    ContributorRole, ReputationEngine, ReputationEvent, ReputationScore,
};

const MAX_STATION_ID_LEN: usize = 128;

/// Consensus-owned classification for contribution provenance (#2460).
///
/// `Unclassified` is deliberately fail-closed: a station that cannot be
/// classified from canonical state must never inherit real-contributor reward
/// eligibility by omission.
#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum ContributionProvenanceClass {
    Real,
    Synthetic,
    #[default]
    Unclassified,
}

impl ContributionProvenanceClass {
    /// Stable discriminant used by the canonical state-root preimage.
    pub const fn state_root_discriminant(self) -> u8 {
        match self {
            Self::Real => 0,
            Self::Synthetic => 1,
            Self::Unclassified => 2,
        }
    }

    pub const fn is_reward_eligible(self) -> bool {
        matches!(self, Self::Real)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Real => "real",
            Self::Synthetic => "synthetic",
            Self::Unclassified => "unclassified",
        }
    }
}

impl std::str::FromStr for ContributionProvenanceClass {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "real" => Ok(Self::Real),
            "synthetic" => Ok(Self::Synthetic),
            "unclassified" => Ok(Self::Unclassified),
            other => Err(format!("unknown contribution provenance class '{other}'")),
        }
    }
}

/// Equivalent TypeScript interfaces for backend parity:
/// `type ContributionProvenanceClass = "real" | "synthetic" | "unclassified";`
/// `type DataQualityTier = "unverified" | "bronze" | "silver" | "gold";`
/// `interface StationLocation { latitude: number; longitude: number; elevationMeters?: number }`
/// `interface WeatherStation { stationId: string; ownerAddress: string; location: StationLocation; hardwareModel: string; calibrationDate?: string | null; isActive: boolean; reputationScore: number; dataQualityTier: DataQualityTier; provenanceClass: ContributionProvenanceClass }`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DataQualityTier {
    Unverified,
    Bronze,
    Silver,
    Gold,
}

impl Default for DataQualityTier {
    fn default() -> Self {
        Self::Unverified
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StationLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_meters: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeatherStation {
    pub station_id: String,
    pub owner_address: String,
    pub location: StationLocation,
    pub hardware_model: String,
    pub calibration_date: Option<String>,
    pub is_active: bool,
    pub reputation_score: u64,
    pub data_quality_tier: DataQualityTier,
    /// Canonical reward/reputation classification. Missing serialized values
    /// become `Unclassified`, never `Real`.
    #[serde(default)]
    pub provenance_class: ContributionProvenanceClass,
    /// Hex-encoded 32-byte compressed Ristretto point for ring signature participation.
    #[serde(default)]
    pub ring_public_key: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StationRegistry {
    stations: FxHashMap<String, WeatherStation>,
    reputation_engine: ReputationEngine,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StationRegistryError {
    #[error("station id is invalid: {0}")]
    InvalidStationId(String),
    #[error("station already registered: {0}")]
    AlreadyRegistered(String),
}

impl StationRegistry {
    pub fn register(&mut self, station: WeatherStation) -> Result<(), StationRegistryError> {
        validate_station_id(&station.station_id)?;
        if self.stations.contains_key(&station.station_id) {
            return Err(StationRegistryError::AlreadyRegistered(
                station.station_id.clone(),
            ));
        }
        let station_id = station.station_id.clone();
        let seeded_score = station.reputation_score;
        self.stations.insert(station.station_id.clone(), station);
        let _ =
            self.reputation_engine
                .seed_score(&station_id, ContributorRole::User, seeded_score, 0);
        Ok(())
    }

    /// Insert or replace a station record, returning the previous record if one
    /// existed (#2537).
    ///
    /// [`Self::register`] refuses duplicates with `AlreadyRegistered`. That is
    /// the right behaviour for a direct caller, but it is the wrong behaviour
    /// on a consensus path: a re-registration (an operator correcting
    /// coordinates, or governance re-classifying a station) would return `Err`,
    /// fail block application, and stall the chain. This is the deterministic
    /// upsert the `ApplyStationRegistration` transition uses instead.
    ///
    /// Reputation is seeded only on first insert, so a re-registration cannot
    /// reset a station's earned standing.
    pub fn upsert(
        &mut self,
        station: WeatherStation,
    ) -> Result<Option<WeatherStation>, StationRegistryError> {
        validate_station_id(&station.station_id)?;
        let station_id = station.station_id.clone();
        let seeded_score = station.reputation_score;
        let previous = self.stations.insert(station_id.clone(), station);
        if previous.is_none() {
            let _ =
                self.reputation_engine
                    .seed_score(&station_id, ContributorRole::User, seeded_score, 0);
        }
        Ok(previous)
    }

    pub fn get(&self, station_id: &str) -> Option<&WeatherStation> {
        if validate_station_id(station_id).is_err() {
            return None;
        }
        self.stations.get(station_id)
    }

    pub fn deactivate(&mut self, station_id: &str) -> bool {
        if validate_station_id(station_id).is_err() {
            return false;
        }
        if let Some(station) = self.stations.get_mut(station_id) {
            let was_active = station.is_active;
            station.is_active = false;
            return was_active;
        }
        false
    }

    /// Update the reputation for a station using the provided block timestamp.
    /// Use `update_reputation_at` directly if you already have the block timestamp.
    pub fn update_reputation(
        &mut self,
        station_id: &str,
        delta: i32,
        block_timestamp: u64,
    ) -> Option<u64> {
        self.update_reputation_at(station_id, delta, block_timestamp)
    }

    pub fn update_reputation_at(
        &mut self,
        station_id: &str,
        delta: i32,
        timestamp: u64,
    ) -> Option<u64> {
        if validate_station_id(station_id).is_err() {
            return None;
        }
        if !self.stations.contains_key(station_id) {
            return None;
        }

        if self.reputation_engine.record_for(station_id).is_none() {
            if let Some(station) = self.stations.get(station_id) {
                let _ = self.reputation_engine.seed_score(
                    station_id,
                    ContributorRole::User,
                    station.reputation_score,
                    0,
                );
            }
        }

        let ReputationScore(updated) = self
            .reputation_engine
            .apply_event(ReputationEvent {
                oracle_id: station_id.to_string(),
                delta,
                reason: "station_registry_update".to_string(),
                timestamp,
            })
            .ok()?;

        let station = self.stations.get_mut(station_id)?;
        station.reputation_score = updated;
        Some(updated)
    }

    pub fn entries(&self) -> impl Iterator<Item = (&String, &WeatherStation)> {
        self.stations.iter()
    }

    /// Number of registered stations, O(1). Read-only; not part of any
    /// serialized form or state root.
    pub fn len(&self) -> usize {
        self.stations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stations.is_empty()
    }
}

/// Validate a canonical station identifier without consulting registry state.
///
/// API surfaces use this same validator so malformed identifiers remain
/// distinguishable from well-formed identifiers that are not registered.
pub fn validate_station_id(station_id: &str) -> Result<(), StationRegistryError> {
    if station_id.trim().is_empty() {
        return Err(StationRegistryError::InvalidStationId(
            "station_id cannot be empty".to_string(),
        ));
    }
    if station_id.trim() != station_id {
        return Err(StationRegistryError::InvalidStationId(
            "station_id cannot have leading or trailing whitespace".to_string(),
        ));
    }
    if station_id.chars().any(|c| c.is_whitespace()) {
        return Err(StationRegistryError::InvalidStationId(
            "station_id cannot contain whitespace".to_string(),
        ));
    }
    if station_id.len() > MAX_STATION_ID_LEN {
        return Err(StationRegistryError::InvalidStationId(format!(
            "station_id exceeds max length ({MAX_STATION_ID_LEN})"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_station() -> WeatherStation {
        WeatherStation {
            station_id: "station-1".to_string(),
            owner_address: "wrf1owneraddress".to_string(),
            location: StationLocation {
                latitude: 40.7128,
                longitude: -74.006,
                elevation_meters: Some(12.0),
            },
            hardware_model: "BME680".to_string(),
            calibration_date: Some("2026-03-01".to_string()),
            is_active: true,
            reputation_score: 10,
            data_quality_tier: DataQualityTier::Bronze,
            provenance_class: ContributionProvenanceClass::Real,
            ring_public_key: None,
        }
    }

    #[test]
    fn register_and_get_station() {
        let mut registry = StationRegistry::default();
        let station = sample_station();
        assert!(registry.register(station.clone()).is_ok());

        let loaded = registry.get("station-1").expect("station should exist");
        assert_eq!(loaded.station_id, station.station_id);
        assert_eq!(loaded.owner_address, station.owner_address);
    }

    #[test]
    fn register_rejects_duplicate_station_id() {
        let mut registry = StationRegistry::default();
        let station = sample_station();
        assert!(registry.register(station.clone()).is_ok());
        let err = registry
            .register(station)
            .expect_err("duplicate station id should fail");
        assert_eq!(
            err,
            StationRegistryError::AlreadyRegistered("station-1".to_string())
        );
    }

    #[test]
    fn register_rejects_invalid_station_id() {
        let mut registry = StationRegistry::default();
        let mut station = sample_station();
        station.station_id = "  ".to_string();
        let err = registry
            .register(station)
            .expect_err("empty station id should fail");
        assert!(matches!(err, StationRegistryError::InvalidStationId(_)));
    }

    #[test]
    fn deactivate_station_once() {
        let mut registry = StationRegistry::default();
        assert!(registry.register(sample_station()).is_ok());

        assert!(registry.deactivate("station-1"));
        assert!(!registry.deactivate("station-1"));
        assert!(
            !registry
                .get("station-1")
                .expect("station should still exist")
                .is_active
        );
    }

    #[test]
    fn update_reputation_with_saturating_math() {
        use wrfcoin_reputation_engine::SCORE_SCALE;
        let mut registry = StationRegistry::default();
        assert!(registry.register(sample_station()).is_ok());

        // Station seeded at score 10; delta +5 → 15 (fixed-point)
        assert_eq!(
            registry.update_reputation("station-1", 5, 0),
            Some(15 * SCORE_SCALE)
        );
        // delta -20 → saturates to 0
        assert_eq!(registry.update_reputation("station-1", -20, 0), Some(0));
        assert_eq!(registry.update_reputation("missing-station", 3, 0), None);
    }

    #[test]
    fn update_reputation_applies_decay_with_nonzero_ticks() {
        use wrfcoin_reputation_engine::SCORE_SCALE;
        let mut registry = StationRegistry::default();
        assert!(registry.register(sample_station()).is_ok());

        // Station seeded at 10; delta +50 → 60 (fixed-point)
        assert_eq!(
            registry.update_reputation_at("station-1", 50, 1),
            Some(60 * SCORE_SCALE)
        );
        // After 89 ticks past grace (119 - 30), 2 halvings + fractional: ~7M
        let decayed = registry
            .update_reputation_at("station-1", 0, 120)
            .expect("decayed score");
        // Score should be significantly reduced but still in valid range
        assert!(decayed < 60 * SCORE_SCALE, "score should have decayed");
        assert!(decayed > 0, "score should not be zero");
    }

    #[test]
    fn serde_round_trip_preserves_reputation_state() {
        use wrfcoin_reputation_engine::SCORE_SCALE;
        let mut registry = StationRegistry::default();
        assert!(registry.register(sample_station()).is_ok());
        assert_eq!(
            registry.update_reputation_at("station-1", 40, 10),
            Some(50 * SCORE_SCALE)
        );

        let encoded = serde_json::to_string(&registry).expect("serialize registry");
        let mut decoded: StationRegistry =
            serde_json::from_str(&encoded).expect("deserialize registry");

        // If reputation state is preserved, decay from tick=10 should apply.
        let after_decay = decoded
            .update_reputation_at("station-1", 0, 100)
            .expect("decayed score");
        assert!(after_decay < 50 * SCORE_SCALE, "score should have decayed");
        assert!(after_decay > 0, "score should not be zero");
    }

    #[test]
    fn station_registry_uses_block_timestamp() {
        use wrfcoin_reputation_engine::SCORE_SCALE;
        let mut registry = StationRegistry::default();
        assert!(registry.register(sample_station()).is_ok());

        // Two calls with the same block_timestamp produce the same result (deterministic).
        let ts_a = 1_000_000_u64;
        let score_a = registry.update_reputation("station-1", 10, ts_a);

        // Reset registry, replay with same timestamp — must match.
        let mut registry2 = StationRegistry::default();
        assert!(registry2.register(sample_station()).is_ok());
        let score_b = registry2.update_reputation("station-1", 10, ts_a);

        assert_eq!(score_a, score_b, "same block_timestamp must yield same score");
        assert!(score_a.unwrap() > 0, "score must be positive");

        // A different block_timestamp (simulating later block) may differ due to decay.
        let ts_b = ts_a + 200;
        let score_c = registry.update_reputation("station-1", 0, ts_b);
        assert!(score_c.is_some(), "update with later timestamp should succeed");
        // The score may decay over time; we verify no wall-clock dependency.
        let _ = score_c; // The exact value is decay-engine-dependent.
        let _ = SCORE_SCALE;
    }
}
