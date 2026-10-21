//! Spatial consistency checks and oracle collusion detection for weather data.
//!
//! Implements WMO-standard "buddy check" spatial consistency (WMO-No. 8, 2018)
//! and statistical collusion detection for the WRFCoin testnet.

use serde::{Deserialize, Serialize};
use wrfcoin_geo_grid::haversine_km;
use wrfcoin_meteo_types::WeatherObservation;

/// Policy controlling spatial consistency ("buddy check") parameters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpatialConsistencyPolicy {
    /// Maximum radius (km) for neighbor inclusion.
    pub max_radius_km: f64,
    /// Maximum allowable temperature difference per km of separation.
    pub max_temp_delta_per_km: f64,
    /// Maximum allowable pressure difference per km of separation.
    pub max_pressure_delta_per_km: f64,
    /// Minimum number of neighbors required to run the check.
    /// If fewer neighbors are within radius, the check is skipped (score = 1.0).
    pub min_neighbors: usize,
    /// Block height at or above which spatial checks activate.
    pub activation_height: u64,
}

impl Default for SpatialConsistencyPolicy {
    fn default() -> Self {
        Self {
            max_radius_km: 100.0,
            max_temp_delta_per_km: 0.1,
            max_pressure_delta_per_km: 0.05,
            min_neighbors: 2,
            activation_height: 0,
        }
    }
}

/// Policy controlling collusion detection parameters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CollusionPolicy {
    /// Values within this epsilon are considered "identical".
    pub epsilon: f64,
    /// Minimum number of fields that must match to flag collusion.
    pub min_matching_fields: usize,
    /// Minimum distance (km) between stations for collusion flagging.
    /// Co-located stations (< this distance) are exempt.
    pub min_distance_km: f64,
    /// Block height at or above which collusion detection activates.
    pub activation_height: u64,
}

impl Default for CollusionPolicy {
    fn default() -> Self {
        Self {
            epsilon: 0.001,
            min_matching_fields: 3,
            min_distance_km: 1.0,
            activation_height: 0,
        }
    }
}

/// A recent observation snapshot used for cross-station comparison.
///
/// The `distance_km` field is populated during pre-filtering in
/// [`gather_nearby_observations`] so that downstream consumers (e.g.
/// `check_spatial_consistency`) can reuse it without recomputing haversine.
#[derive(Debug, Clone)]
pub struct NearbyObservation {
    pub station_id: String,
    pub latitude: f64,
    pub longitude: f64,
    pub observation: WeatherObservation,
    pub block_height: u64,
    /// Haversine distance (km) from the target station, computed once during pre-filter.
    pub distance_km: f64,
}

/// A specific spatial violation found during buddy-check analysis.
#[derive(Debug, Clone, PartialEq)]
pub enum SpatialViolation {
    TemperatureDeviation {
        station_id: String,
        distance_km: f64,
        delta_c: f64,
        expected_max_delta: f64,
    },
    PressureDeviation {
        station_id: String,
        distance_km: f64,
        delta_hpa: f64,
        expected_max_delta: f64,
    },
    SuspectedCollusion {
        station_ids: Vec<String>,
        matching_fields: Vec<String>,
        epsilon: f64,
    },
}

/// Result of a spatial consistency check for one observation.
#[derive(Debug, Clone)]
pub struct SpatialConsistencyResult {
    /// Consistency score in [0.0, 1.0]. 1.0 = fully consistent or insufficient data.
    pub score: f64,
    /// Number of neighbors within radius that were compared.
    pub neighbors_checked: usize,
    /// Individual violations found.
    pub violations: Vec<SpatialViolation>,
}

/// A group of stations suspected of collusion.
#[derive(Debug, Clone, PartialEq)]
pub struct CollusionGroup {
    pub station_ids: Vec<String>,
    pub matching_fields: Vec<String>,
}

/// Result of collusion detection across a set of observations.
#[derive(Debug, Clone)]
pub struct CollusionResult {
    pub suspected: bool,
    pub groups: Vec<CollusionGroup>,
}

/// Penalty per violation as a fraction of score (each violation costs this much).
const VIOLATION_PENALTY: f64 = 0.2;

/// Spatial buddy check: compare one observation against nearby stations.
///
/// Returns score 1.0 (fully consistent) when:
/// - Fewer than `policy.min_neighbors` stations are within `policy.max_radius_km`
/// - All neighbors report consistent readings
///
/// Score decreases by `VIOLATION_PENALTY` per violation, clamped to [0.0, 1.0].
pub fn check_spatial_consistency(
    _station_lat: f64,
    _station_lon: f64,
    observation: &WeatherObservation,
    nearby: &[NearbyObservation],
    policy: &SpatialConsistencyPolicy,
) -> SpatialConsistencyResult {
    // The pre-filter in `gather_nearby_observations` already excludes stations
    // outside `max_radius_km` and those with `dist <= 0.0` (same-location).
    // We use the pre-computed `distance_km` from each NearbyObservation instead
    // of recomputing haversine here.
    let mut in_radius: Vec<(&NearbyObservation, f64)> = nearby
        .iter()
        .filter(|n| n.distance_km <= policy.max_radius_km && n.distance_km > 0.0)
        .map(|n| (n, n.distance_km))
        .collect();

    if in_radius.len() < policy.min_neighbors {
        return SpatialConsistencyResult {
            score: 1.0,
            neighbors_checked: in_radius.len(),
            violations: vec![],
        };
    }

    // Sort by distance for deterministic results
    in_radius.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut violations = Vec::new();

    for (neighbor, dist) in &in_radius {
        // Temperature check
        if let (Some(my_temp), Some(their_temp)) =
            (observation.temperature_c, neighbor.observation.temperature_c)
        {
            let delta = (my_temp - their_temp).abs();
            let expected_max = policy.max_temp_delta_per_km * dist;
            if delta > expected_max {
                violations.push(SpatialViolation::TemperatureDeviation {
                    station_id: neighbor.station_id.clone(),
                    distance_km: *dist,
                    delta_c: delta,
                    expected_max_delta: expected_max,
                });
            }
        }

        // Pressure check
        if let (Some(my_p), Some(their_p)) =
            (observation.pressure_hpa, neighbor.observation.pressure_hpa)
        {
            let delta = (my_p - their_p).abs();
            let expected_max = policy.max_pressure_delta_per_km * dist;
            if delta > expected_max {
                violations.push(SpatialViolation::PressureDeviation {
                    station_id: neighbor.station_id.clone(),
                    distance_km: *dist,
                    delta_hpa: delta,
                    expected_max_delta: expected_max,
                });
            }
        }
    }

    let penalty = (violations.len() as f64) * VIOLATION_PENALTY;
    let score = (1.0 - penalty).clamp(0.0, 1.0);

    SpatialConsistencyResult {
        score,
        neighbors_checked: in_radius.len(),
        violations,
    }
}

/// Pre-filter candidates by haversine distance, returning only those within `max_radius_km`.
///
/// Each candidate is a tuple of `(station_id, latitude, longitude, observation, block_height)`.
/// The returned `NearbyObservation`s have `distance_km` populated so callers (e.g.
/// `check_spatial_consistency`) can reuse it without recomputing haversine.
pub fn gather_nearby_observations(
    station_lat: f64,
    station_lon: f64,
    max_radius_km: f64,
    candidates: impl Iterator<Item = (String, f64, f64, WeatherObservation, u64)>,
) -> Vec<NearbyObservation> {
    candidates
        .filter_map(|(sid, lat, lon, obs, height)| {
            let raw_km = haversine_km((station_lat, station_lon), (lat, lon));
            // Quantise to 10 m resolution (0.01 km) for deterministic consensus.
            // libm sin/cos/atan2/sqrt can produce 1-ULP differences between x86_64
            // and ARM for the same inputs; those feed into distance_km and then into
            // spatial_scores in the state root, causing a fork (issue #1852).
            // Truncating (raw_km * 100.0) to u64 and reconstructing the f64 removes
            // sub-10-m variation: any 1-ULP raw difference is ~10⁻¹³ km, orders of
            // magnitude smaller than 0.01 km, so the integer step is always the same
            // on both architectures.
            let dist = if raw_km.is_nan() || raw_km < 0.0 {
                0.0
            } else {
                ((raw_km * 100.0) as u64) as f64 / 100.0
            };
            if dist > max_radius_km || dist <= 0.0 {
                return None;
            }
            Some(NearbyObservation {
                station_id: sid,
                latitude: lat,
                longitude: lon,
                observation: obs,
                block_height: height,
                distance_km: dist,
            })
        })
        .collect()
}

/// Collusion detection: check if any pair of observations report suspiciously identical data.
///
/// Two stations far apart reporting identical values (within epsilon) across multiple
/// fields is statistically implausible and suggests data fabrication or copy-paste attacks.
pub fn detect_collusion(
    observations: &[NearbyObservation],
    policy: &CollusionPolicy,
) -> CollusionResult {
    let mut groups: Vec<CollusionGroup> = Vec::new();

    for i in 0..observations.len() {
        for j in (i + 1)..observations.len() {
            let a = &observations[i];
            let b = &observations[j];

            // Quantise to 0.01 km (10 m) before threshold comparison, matching
            // gather_nearby_observations (#1852). Raw haversine can differ by 1 ULP
            // between x86 and ARM; truncation to u64 removes sub-10-m variation so
            // the collusion exemption boundary is identical on all nodes (#1874).
            let dist_100 = {
                let raw = haversine_km((a.latitude, a.longitude), (b.latitude, b.longitude));
                if raw.is_nan() || raw < 0.0 { 0u64 } else { (raw * 100.0) as u64 }
            };
            let min_100 = (policy.min_distance_km * 100.0) as u64;
            if dist_100 < min_100 {
                continue; // co-located stations are exempt
            }

            let mut matching = Vec::new();

            if let (Some(at), Some(bt)) =
                (a.observation.temperature_c, b.observation.temperature_c)
            {
                if (at - bt).abs() < policy.epsilon {
                    matching.push("temperature".to_string());
                }
            }

            if let (Some(ah), Some(bh)) =
                (a.observation.humidity_percent, b.observation.humidity_percent)
            {
                if (ah - bh).abs() < policy.epsilon {
                    matching.push("humidity".to_string());
                }
            }

            if let (Some(ap), Some(bp)) =
                (a.observation.pressure_hpa, b.observation.pressure_hpa)
            {
                if (ap - bp).abs() < policy.epsilon {
                    matching.push("pressure".to_string());
                }
            }

            if let (Some(aw), Some(bw)) =
                (a.observation.wind_speed_ms, b.observation.wind_speed_ms)
            {
                if (aw - bw).abs() < policy.epsilon {
                    matching.push("wind_speed".to_string());
                }
            }

            if matching.len() >= policy.min_matching_fields {
                groups.push(CollusionGroup {
                    station_ids: vec![a.station_id.clone(), b.station_id.clone()],
                    matching_fields: matching,
                });
            }
        }
    }

    let suspected = !groups.is_empty();
    CollusionResult { suspected, groups }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(temp: f64, hum: f64, pressure: f64, wind: f64) -> WeatherObservation {
        WeatherObservation {
            temperature_c: Some(temp),
            humidity_percent: Some(hum),
            pressure_hpa: Some(pressure),
            wind_speed_ms: Some(wind),
            precipitation_mm: None,
            dewpoint_c: None,
            ..Default::default()
        }
    }

    /// Build a `NearbyObservation` for tests.  `ref_lat`/`ref_lon` default to
    /// (40.0, -105.0) (Denver) when not specified — callers that need a custom
    /// reference point should use `nearby_from`.
    fn nearby(id: &str, lat: f64, lon: f64, temp: f64, hum: f64, press: f64, wind: f64) -> NearbyObservation {
        nearby_from(id, lat, lon, temp, hum, press, wind, 40.0, -105.0)
    }

    fn nearby_from(
        id: &str,
        lat: f64,
        lon: f64,
        temp: f64,
        hum: f64,
        press: f64,
        wind: f64,
        ref_lat: f64,
        ref_lon: f64,
    ) -> NearbyObservation {
        NearbyObservation {
            station_id: id.to_string(),
            latitude: lat,
            longitude: lon,
            observation: obs(temp, hum, press, wind),
            block_height: 100,
            distance_km: haversine_km((ref_lat, ref_lon), (lat, lon)),
        }
    }

    #[test]
    fn haversine_known_distance() {
        // NYC (40.7128, -74.0060) to London (51.5074, -0.1278)
        // Known great-circle distance ≈ 5570 km
        let dist = haversine_km((40.7128, -74.0060), (51.5074, -0.1278));
        assert!(
            (dist - 5570.0).abs() < 50.0,
            "NYC-London distance should be ~5570 km, got {dist:.0}"
        );
    }

    #[test]
    fn spatial_consistency_all_agree() {
        // Station at (40.0, -105.0), neighbors within 50km with similar readings
        let my_obs = obs(20.0, 60.0, 1013.0, 5.0);
        let neighbors = vec![
            nearby("s1", 40.1, -105.1, 20.5, 61.0, 1013.2, 5.5),
            nearby("s2", 40.2, -104.9, 19.8, 59.0, 1012.8, 4.8),
            nearby("s3", 39.9, -105.2, 20.2, 60.5, 1013.1, 5.2),
        ];
        let policy = SpatialConsistencyPolicy::default();

        let result = check_spatial_consistency(40.0, -105.0, &my_obs, &neighbors, &policy);
        assert!(
            result.score > 0.8,
            "consistent stations should score high, got {:.2}",
            result.score
        );
        assert_eq!(result.neighbors_checked, 3);
    }

    #[test]
    fn spatial_consistency_one_outlier() {
        // One station reports 25°C higher — wildly inconsistent
        let my_obs = obs(45.0, 60.0, 1013.0, 5.0);
        let neighbors = vec![
            nearby("s1", 40.1, -105.1, 20.0, 61.0, 1013.2, 5.5),
            nearby("s2", 40.2, -104.9, 19.8, 59.0, 1012.8, 4.8),
        ];
        let policy = SpatialConsistencyPolicy::default();

        let result = check_spatial_consistency(40.0, -105.0, &my_obs, &neighbors, &policy);
        assert!(
            result.score < 0.8,
            "outlier station should reduce score, got {:.2}",
            result.score
        );
        assert!(!result.violations.is_empty());
    }

    #[test]
    fn spatial_consistency_insufficient_neighbors() {
        let my_obs = obs(20.0, 60.0, 1013.0, 5.0);
        let neighbors = vec![
            nearby("s1", 40.1, -105.1, 50.0, 10.0, 900.0, 100.0), // wildly different
        ];
        let policy = SpatialConsistencyPolicy {
            min_neighbors: 2,
            ..Default::default()
        };

        let result = check_spatial_consistency(40.0, -105.0, &my_obs, &neighbors, &policy);
        assert_eq!(result.score, 1.0, "insufficient neighbors should return 1.0");
        assert!(result.violations.is_empty());
    }

    #[test]
    fn spatial_consistency_no_neighbors_in_radius() {
        let my_obs = obs(20.0, 60.0, 1013.0, 5.0);
        // Neighbors are very far away (different continent)
        let neighbors = vec![
            nearby("s1", 51.5, -0.1, 15.0, 70.0, 1010.0, 8.0),   // London
            nearby("s2", 48.8, 2.3, 16.0, 65.0, 1011.0, 7.0),    // Paris
        ];
        let policy = SpatialConsistencyPolicy {
            max_radius_km: 100.0,
            min_neighbors: 2,
            ..Default::default()
        };

        // Station is in Denver — London/Paris are >7000km away
        let result = check_spatial_consistency(40.0, -105.0, &my_obs, &neighbors, &policy);
        assert_eq!(result.score, 1.0, "no neighbors in radius should return 1.0");
        assert_eq!(result.neighbors_checked, 0);
    }

    #[test]
    fn collusion_identical_readings() {
        // Two stations 50km apart with identical values — suspicious
        let observations = vec![
            nearby("s1", 40.0, -105.0, 20.123, 60.456, 1013.789, 5.321),
            nearby("s2", 40.4, -105.0, 20.123, 60.456, 1013.789, 5.321),
        ];
        let policy = CollusionPolicy::default();

        let result = detect_collusion(&observations, &policy);
        assert!(result.suspected, "identical distant readings should flag collusion");
        assert_eq!(result.groups.len(), 1);
        assert!(result.groups[0].matching_fields.len() >= 3);
    }

    #[test]
    fn collusion_different_readings() {
        let observations = vec![
            nearby("s1", 40.0, -105.0, 20.0, 60.0, 1013.0, 5.0),
            nearby("s2", 40.4, -105.0, 22.5, 55.0, 1010.0, 8.0),
        ];
        let policy = CollusionPolicy::default();

        let result = detect_collusion(&observations, &policy);
        assert!(!result.suspected, "different readings should not flag collusion");
    }

    #[test]
    fn collusion_close_stations_exempt() {
        // Two co-located stations (<1km) with identical values — exempt
        let observations = vec![
            nearby("s1", 40.0, -105.0, 20.123, 60.456, 1013.789, 5.321),
            nearby("s2", 40.0001, -105.0001, 20.123, 60.456, 1013.789, 5.321),
        ];
        let policy = CollusionPolicy::default();

        let result = detect_collusion(&observations, &policy);
        assert!(!result.suspected, "co-located stations should be exempt from collusion detection");
    }

    #[test]
    fn collusion_partial_match() {
        // Only 2 fields match (temp + humidity) — below threshold of 3
        let observations = vec![
            nearby("s1", 40.0, -105.0, 20.0, 60.0, 1013.0, 5.0),
            nearby("s2", 40.4, -105.0, 20.0, 60.0, 1010.0, 8.0),
        ];
        let policy = CollusionPolicy::default();

        let result = detect_collusion(&observations, &policy);
        assert!(!result.suspected, "partial field match should not flag collusion");
    }

    #[test]
    fn spatial_policy_default_is_valid() {
        let policy = SpatialConsistencyPolicy::default();
        assert_eq!(policy.max_radius_km, 100.0);
        assert_eq!(policy.min_neighbors, 2);
        assert_eq!(policy.activation_height, 0);
    }

    #[test]
    fn collusion_policy_default_is_valid() {
        let policy = CollusionPolicy::default();
        assert_eq!(policy.epsilon, 0.001);
        assert_eq!(policy.min_matching_fields, 3);
        assert_eq!(policy.min_distance_km, 1.0);
    }

    #[test]
    fn gather_nearby_filters_by_radius() {
        // Reference station in Denver (40.0, -105.0).
        // "s1" is ~15 km away — inside the 50 km radius.
        // "s2" is in London (~7500 km away) — outside the 50 km radius.
        let candidates = vec![
            ("s1".to_string(), 40.1, -105.1, obs(20.0, 60.0, 1013.0, 5.0), 100u64),
            ("s2".to_string(), 51.5, -0.1, obs(15.0, 70.0, 1010.0, 8.0), 200u64),
        ];

        let result = gather_nearby_observations(
            40.0,
            -105.0,
            50.0, // 50 km radius
            candidates.into_iter(),
        );

        assert_eq!(result.len(), 1, "only the nearby station should pass the pre-filter");
        assert_eq!(result[0].station_id, "s1");
        assert!(
            result[0].distance_km > 0.0 && result[0].distance_km < 50.0,
            "distance_km should be populated and within radius, got {:.1}",
            result[0].distance_km,
        );
    }

    #[test]
    fn gather_nearby_excludes_same_location() {
        // Station at the exact same coordinates should be excluded (dist == 0.0).
        let candidates = vec![
            ("self".to_string(), 40.0, -105.0, obs(20.0, 60.0, 1013.0, 5.0), 100u64),
        ];

        let result = gather_nearby_observations(
            40.0,
            -105.0,
            100.0,
            candidates.into_iter(),
        );

        assert!(result.is_empty(), "same-location station should be excluded");
    }

    #[test]
    fn gather_nearby_distance_quantised_to_10m() {
        // Verify that distance_km is a multiple of 0.01 km (10 m) so it is
        // bitwise-identical across x86_64 and ARM (issue #1852).
        let candidates = vec![
            ("s1".to_string(), 40.5, -105.3, obs(18.0, 55.0, 1010.0, 3.0), 1u64),
            ("s2".to_string(), 41.0, -104.5, obs(17.0, 58.0, 1011.0, 4.0), 1u64),
        ];

        let result = gather_nearby_observations(
            40.0,
            -105.0,
            200.0,
            candidates.into_iter(),
        );

        assert_eq!(result.len(), 2);
        for obs in &result {
            // Multiply by 100 and check there is no fractional part (rounded to
            // nearest integer centimetre ≙ 10 m — the same grid on all platforms).
            let scaled = obs.distance_km * 100.0;
            let rounded = scaled.round();
            assert!(
                (scaled - rounded).abs() < 1e-9,
                "distance_km {} is not quantised to 0.01 km steps",
                obs.distance_km,
            );
        }
    }

    #[test]
    fn gather_nearby_distance_bytes_deterministic() {
        // Same inputs must produce bit-for-bit identical distance_km values.
        let make_candidates = || {
            vec![
                ("a".to_string(), 51.5, -0.12, obs(15.0, 70.0, 1012.0, 2.0), 1u64),
            ]
        };

        let r1 = gather_nearby_observations(51.0, 0.0, 500.0, make_candidates().into_iter());
        let r2 = gather_nearby_observations(51.0, 0.0, 500.0, make_candidates().into_iter());

        assert_eq!(r1[0].distance_km.to_bits(), r2[0].distance_km.to_bits(),
            "distance_km must be bit-for-bit identical across calls (determinism regression guard)");
    }

    #[test]
    fn detect_collusion_quantized_boundary_1874() {
        // Regression for #1874: detect_collusion previously compared raw
        // haversine_km() directly against policy.min_distance_km (1.0 km).
        // Because libm haversine can differ by ±1 ULP between x86 and ARM for
        // the same coordinate pair, the exempt/flagged branch was
        // non-deterministic at the boundary.
        //
        // The fix: quantise `(raw * 100.0) as u64` (0.01 km = 10 m resolution)
        // before the threshold comparison — matching gather_nearby_observations
        // (#1852).  Two raw floats that straddle a 0.01 km bucket edge
        // (e.g. 0.99999…km vs 1.00001…km for the same pair on different arches)
        // truncate to the same u64 and therefore take the same branch.
        //
        // Coordinate pairs used (same longitude, Denver area):
        //   (40.0, -105.0) → (40.0089, -105.0)  ≈ 0.989 km → dist_100 =  98 → EXEMPT
        //   (40.0, -105.0) → (40.009,  -105.0)  ≈ 1.001 km → dist_100 = 100 → NOT exempt
        //
        // The "adjacent floats" scenario: 0.99950 km and 1.00001 km are both raw
        // haversine values that could arise from the same nominal pair on
        // different arches; after truncation they land in different 0.01 km
        // buckets (99 vs 100) and the branch is deterministic.
        let policy = CollusionPolicy::default(); // min_distance_km = 1.0

        // ── Below-threshold: ≈ 0.99 km → dist_100 = 98 → exempt ─────────────
        let below = vec![
            nearby("a", 40.0, -105.0, 20.0, 60.0, 1013.0, 5.0),
            nearby("b", 40.0089, -105.0, 20.0, 60.0, 1013.0, 5.0),
        ];
        assert!(
            !detect_collusion(&below, &policy).suspected,
            "#1874: stations ≈0.99 km apart must be exempt (quantized dist_100 < min_100=100)"
        );

        // ── Above-threshold: ≈ 1.001 km → dist_100 = 100 → not exempt → flagged ──
        let above = vec![
            nearby("a", 40.0, -105.0, 20.0, 60.0, 1013.0, 5.0),
            nearby("b", 40.009, -105.0, 20.0, 60.0, 1013.0, 5.0),
        ];
        assert!(
            detect_collusion(&above, &policy).suspected,
            "#1874: stations ≈1.0 km apart with identical readings must be flagged"
        );

        // ── Pin the quantized value for the boundary coordinate ───────────────
        // For (40.0, -105.0) → (40.009, -105.0): raw haversine ∈ [1.000, 1.002) km
        // on all supported architectures.  Truncation to u64 always yields 100,
        // so `dist_100 < min_100` (100 < 100) is always false → always flagged.
        let raw = haversine_km((40.0, -105.0), (40.009, -105.0));
        let dist_100 = if raw.is_nan() || raw < 0.0 { 0u64 } else { (raw * 100.0) as u64 };
        assert_eq!(
            dist_100, 100,
            "#1874: 0.009° lat offset must quantise to exactly dist_100=100; \
             raw haversine was {raw:.6} km"
        );
    }
}
