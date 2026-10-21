use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use wrfcoin_geo_grid::{GeoCell, GeoGridError};

/// Resolution in kilometres used for the desert density grid (≈ 2° at the equator).
const DESERT_GRID_RESOLUTION_KM: u8 = 222;

/// Quantize a lat/lon pair to the desert grid cell, falling back to (0,0) on invalid input.
///
/// Using a fallback rather than panicking keeps the station counted in a real cell rather than
/// being silently dropped when a station record contains a corrupt/out-of-range coordinate.
pub fn cell_for_station(lat: f64, lon: f64) -> GeoCell {
    GeoCell::from_lat_lon(lat, lon, DESERT_GRID_RESOLUTION_KM).unwrap_or_else(
        |_: GeoGridError| GeoCell { lat_idx: 0, lon_idx: 0, resolution_km: DESERT_GRID_RESOLUTION_KM },
    )
}

/// Block-height-activated policy governing data desert reward bonuses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataDesertPolicy {
    /// Stations per grid cell below this threshold qualify as a data desert.
    pub min_station_density: u32,
    /// Reward multiplier in basis points (denominator=100).
    /// Example: 150 = +50% bonus.
    pub desert_bonus_bps: u32,
    /// Block height at which this policy activates.
    pub activation_height: u64,
}

impl Default for DataDesertPolicy {
    fn default() -> Self {
        Self {
            min_station_density: 3,
            desert_bonus_bps: 150,
            activation_height: 0,
        }
    }
}

/// Computes a per-station desert multiplier in basis points.
///
/// - `100` => no bonus
/// - `150` => +50% bonus
///
/// `station_counts` uses `BTreeMap` rather than `HashMap` to enforce deterministic
/// iteration order at the type level. This is a consensus-critical path: any map used
/// here must guarantee that two nodes with identical logical state compute identical
/// reward multipliers regardless of hash seed or insertion order.
pub fn desert_multiplier(
    station_lat: f64,
    station_lon: f64,
    station_counts: &BTreeMap<GeoCell, u32>,
    policy: &DataDesertPolicy,
    current_height: u64,
) -> u32 {
    if current_height < policy.activation_height {
        return 100;
    }

    let cell = cell_for_station(station_lat, station_lon);
    let count = station_counts.get(&cell).copied().unwrap_or(0);
    if count < policy.min_station_density {
        policy.desert_bonus_bps.max(100)
    } else {
        100
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn station_in_sparse_cell_gets_desert_bonus() {
        let policy = DataDesertPolicy::default();
        let mut counts = BTreeMap::new();
        let cell = cell_for_station(51.3, -0.1);
        counts.insert(cell, 1);

        let mult = desert_multiplier(51.3, -0.1, &counts, &policy, 100);
        assert_eq!(mult, 150);
    }

    #[test]
    fn station_in_dense_cell_gets_no_bonus() {
        let policy = DataDesertPolicy::default();
        let mut counts = BTreeMap::new();
        let cell = cell_for_station(51.3, -0.1);
        counts.insert(cell, 5);

        let mult = desert_multiplier(51.3, -0.1, &counts, &policy, 100);
        assert_eq!(mult, 100);
    }

    #[test]
    fn before_activation_returns_baseline() {
        let policy = DataDesertPolicy {
            activation_height: 1_000,
            ..DataDesertPolicy::default()
        };
        let mut counts = BTreeMap::new();
        let cell = cell_for_station(51.3, -0.1);
        counts.insert(cell, 1);

        let mult = desert_multiplier(51.3, -0.1, &counts, &policy, 999);
        assert_eq!(mult, 100);
    }

    #[test]
    fn geocell_quantization_same_cell_at_two_degree_resolution() {
        // Two points within the same 2° GeoCell (aligned from -180°) should map to the same cell.
        // GeoCell grid boundary for lon: floor((lon+180)/2). Both -0.1 and -0.5 fall in cell 89
        // (the [-2, 0) longitude band). Both 51.3 and 51.9 fall in lat cell 70 ([50, 52)).
        let a = cell_for_station(51.3, -0.1);
        let b = cell_for_station(51.9, -0.5);
        assert_eq!(a, b);
    }

    #[test]
    fn antimeridian_cells_do_not_collide() {
        let east = cell_for_station(0.0, 179.9);
        let west = cell_for_station(0.0, -179.9);
        assert_ne!(east, west);
    }
}
