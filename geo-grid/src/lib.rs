#![deny(missing_docs)]
#![warn(clippy::unwrap_used)]

//! Spatial cell/grid primitives for weather data indexing.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

const EARTH_RADIUS_KM: f64 = 6_371.0;
const KM_PER_DEG_LAT: f64 = 111.0;

/// Errors returned by geo-grid constructors and queries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Error)]
pub enum GeoGridError {
    /// Latitude was outside the supported range.
    #[error("latitude must be in [-90, 90], got {0}")]
    InvalidLatitude(f64),
    /// Longitude was non-finite.
    #[error("longitude must be finite, got {0}")]
    InvalidLongitude(f64),
    /// Resolution must be non-zero.
    #[error("resolution_km must be >= 1")]
    InvalidResolution,
    /// Geohash string contained an invalid character.
    #[error("invalid geohash character encountered")]
    InvalidGeohashInput,
}

/// Axis-aligned geographic bounds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    /// Minimum latitude (degrees).
    pub min_lat: f64,
    /// Maximum latitude (degrees).
    pub max_lat: f64,
    /// Minimum longitude (degrees).
    pub min_lon: f64,
    /// Maximum longitude (degrees).
    pub max_lon: f64,
}

/// Canonical quantized geographic grid cell.
///
/// Derives `Ord` so `GeoCell` can be used as a `BTreeMap` key, enabling deterministic
/// iteration order in consensus-critical paths without a manual sort step.
/// Ordering is lexicographic by `(lat_idx, lon_idx, resolution_km)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GeoCell {
    /// Quantized latitude row index.
    pub lat_idx: i32,
    /// Quantized longitude column index.
    pub lon_idx: i32,
    /// Cell edge resolution in kilometers.
    pub resolution_km: u8,
}

/// A bounded collection of grid cells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoGrid {
    /// Cells contained in this grid selection.
    pub cells: Vec<GeoCell>,
    /// Bounding box covering all returned cells.
    pub bounds: BoundingBox,
}

impl GeoCell {
    /// Quantizes a latitude/longitude point into a grid cell.
    pub fn from_lat_lon(lat: f64, lon: f64, resolution_km: u8) -> Result<Self, GeoGridError> {
        validate_lat_lon_resolution(lat, lon, resolution_km)?;
        let cell_deg = cell_size_deg(resolution_km);
        let lat_cells = lat_cell_count(resolution_km);
        let lon_cells = lon_cell_count(resolution_km);

        let clamped_lat = lat.clamp(-90.0, 90.0 - f64::EPSILON);
        let normalized_lon = normalize_lon(lon);

        let mut lat_idx = ((clamped_lat + 90.0) / cell_deg).floor() as i32;
        lat_idx = lat_idx.clamp(0, lat_cells - 1);

        let mut lon_idx = ((normalized_lon + 180.0) / cell_deg).floor() as i32;
        lon_idx = wrap_index(lon_idx, lon_cells);

        Ok(Self {
            lat_idx,
            lon_idx,
            resolution_km,
        })
    }

    /// Returns neighboring cells in the Moore neighborhood (8-way where available).
    pub fn neighbors(&self) -> Vec<GeoCell> {
        let lat_cells = lat_cell_count(self.resolution_km);
        let lon_cells = lon_cell_count(self.resolution_km);
        let mut dedup = BTreeSet::<(i32, i32)>::new();
        let mut neighbors = Vec::new();

        for d_lat in -1..=1 {
            for d_lon in -1..=1 {
                if d_lat == 0 && d_lon == 0 {
                    continue;
                }
                let next_lat = self.lat_idx + d_lat;
                if next_lat < 0 || next_lat >= lat_cells {
                    continue;
                }
                let next_lon = wrap_index(self.lon_idx + d_lon, lon_cells);
                if dedup.insert((next_lat, next_lon)) {
                    neighbors.push(GeoCell {
                        lat_idx: next_lat,
                        lon_idx: next_lon,
                        resolution_km: self.resolution_km,
                    });
                }
            }
        }

        neighbors
    }

    /// Returns the center latitude/longitude of this cell.
    pub fn center(&self) -> (f64, f64) {
        let step = cell_size_deg(self.resolution_km);
        let lat = -90.0 + (f64::from(self.lat_idx) + 0.5) * step;
        let lon = -180.0 + (f64::from(self.lon_idx) + 0.5) * step;
        (lat.clamp(-90.0, 90.0), normalize_lon(lon))
    }
}

impl GeoGrid {
    /// Collect cells whose centers are within `radius_km` of `center`.
    pub fn cells_within_radius(center: GeoCell, radius_km: f64) -> Self {
        let radius = radius_km.max(0.0);
        let lat_scan = ((radius / f64::from(center.resolution_km)).ceil() as i32).max(0);
        let lat_cells = lat_cell_count(center.resolution_km);
        let lon_cells = lon_cell_count(center.resolution_km);
        let mut seen = BTreeSet::<(i32, i32)>::new();
        let mut cells = Vec::new();
        let center_point = center.center();

        for d_lat in -lat_scan..=lat_scan {
            let lat_idx = center.lat_idx + d_lat;
            if lat_idx < 0 || lat_idx >= lat_cells {
                continue;
            }
            let row_center_lat =
                -90.0 + (f64::from(lat_idx) + 0.5) * cell_size_deg(center.resolution_km);
            let lon_cell_km = (f64::from(center.resolution_km)
                * row_center_lat.to_radians().cos().abs())
            .max(0.1);
            let lon_scan = ((radius / lon_cell_km).ceil() as i32)
                .max(lat_scan)
                .min(lon_cells / 2);

            for d_lon in -lon_scan..=lon_scan {
                let lon_idx = wrap_index(center.lon_idx + d_lon, lon_cells);
                if !seen.insert((lat_idx, lon_idx)) {
                    continue;
                }
                let candidate = GeoCell {
                    lat_idx,
                    lon_idx,
                    resolution_km: center.resolution_km,
                };
                if haversine_km(center_point, candidate.center()) <= radius {
                    cells.push(candidate);
                }
            }
        }

        if cells.is_empty() {
            cells.push(center);
        }

        let bounds = bounds_for_cells(&cells);
        Self { cells, bounds }
    }
}

fn validate_lat_lon_resolution(lat: f64, lon: f64, resolution_km: u8) -> Result<(), GeoGridError> {
    if !(lat.is_finite() && (-90.0..=90.0).contains(&lat)) {
        return Err(GeoGridError::InvalidLatitude(lat));
    }
    if !lon.is_finite() {
        return Err(GeoGridError::InvalidLongitude(lon));
    }
    if resolution_km == 0 {
        return Err(GeoGridError::InvalidResolution);
    }
    Ok(())
}

fn cell_size_deg(resolution_km: u8) -> f64 {
    f64::from(resolution_km) / KM_PER_DEG_LAT
}

fn lat_cell_count(resolution_km: u8) -> i32 {
    (180.0 / cell_size_deg(resolution_km)).ceil() as i32
}

fn lon_cell_count(resolution_km: u8) -> i32 {
    (360.0 / cell_size_deg(resolution_km)).ceil() as i32
}

fn wrap_index(idx: i32, total: i32) -> i32 {
    idx.rem_euclid(total)
}

fn normalize_lon(lon: f64) -> f64 {
    let wrapped = (lon + 180.0).rem_euclid(360.0) - 180.0;
    if wrapped == 180.0 { -180.0 } else { wrapped }
}

/// Compute the great-circle distance in kilometres between two `(lat, lon)` points
/// using the haversine formula.
pub fn haversine_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat1, lon1) = (a.0.to_radians(), a.1.to_radians());
    let (lat2, lon2) = (b.0.to_radians(), b.1.to_radians());
    let d_lat = lat2 - lat1;
    let d_lon = lon2 - lon1;
    let sin_dlat = (d_lat / 2.0).sin();
    let sin_dlon = (d_lon / 2.0).sin();
    let aa = sin_dlat * sin_dlat + lat1.cos() * lat2.cos() * sin_dlon * sin_dlon;
    let c = 2.0 * aa.sqrt().atan2((1.0 - aa).sqrt());
    EARTH_RADIUS_KM * c
}

fn bounds_for_cells(cells: &[GeoCell]) -> BoundingBox {
    let first = cells[0];
    let step = cell_size_deg(first.resolution_km) / 2.0;
    let (first_lat, first_lon) = first.center();
    let mut min_lat = first_lat - step;
    let mut max_lat = first_lat + step;
    let mut lons = vec![first_lon];

    for cell in &cells[1..] {
        let (lat, lon) = cell.center();
        min_lat = min_lat.min(lat - step);
        max_lat = max_lat.max(lat + step);
        lons.push(lon);
    }

    let mut raw_min_lon = lons[0];
    let mut raw_max_lon = lons[0];
    for lon in &lons[1..] {
        raw_min_lon = raw_min_lon.min(*lon);
        raw_max_lon = raw_max_lon.max(*lon);
    }

    let mut shifted_lons: Vec<f64> = lons
        .iter()
        .map(|lon| if *lon < 0.0 { lon + 360.0 } else { *lon })
        .collect();
    shifted_lons.sort_by(|a, b| a.total_cmp(b));
    let shifted_min = shifted_lons[0];
    let shifted_max = shifted_lons[shifted_lons.len() - 1];

    let normal_span = raw_max_lon - raw_min_lon;
    let shifted_span = shifted_max - shifted_min;
    let (min_lon, max_lon) = if shifted_span < normal_span {
        (
            normalize_lon(shifted_min - step),
            normalize_lon(shifted_max + step),
        )
    } else {
        (
            normalize_lon(raw_min_lon - step),
            normalize_lon(raw_max_lon + step),
        )
    };

    if (max_lon - min_lon).abs() < f64::EPSILON {
        return BoundingBox {
            min_lat: min_lat.clamp(-90.0, 90.0),
            max_lat: max_lat.clamp(-90.0, 90.0),
            min_lon: -180.0,
            max_lon: 180.0,
        };
    }

    BoundingBox {
        min_lat: min_lat.clamp(-90.0, 90.0),
        max_lat: max_lat.clamp(-90.0, 90.0),
        min_lon,
        max_lon,
    }
}

/// Lambert conformal conic projection parameters for weather-radar grid alignment.
/// Projects geodetic (lat, lon) to Cartesian (x_km, y_km) with low angular distortion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LambertConformal {
    /// First standard parallel (degrees).
    pub sp1_deg: f64,
    /// Second standard parallel (degrees).
    pub sp2_deg: f64,
    /// Central meridian (degrees).
    pub lon0_deg: f64,
    /// Projection origin latitude (degrees).
    pub lat0_deg: f64,
}

impl LambertConformal {
    const R_KM: f64 = 6_371.0;

    /// Project geodetic to Cartesian (km from origin).
    pub fn project(&self, lat_deg: f64, lon_deg: f64) -> (f64, f64) {
        let (sp1, sp2, lon0, lat0) = (
            self.sp1_deg.to_radians(),
            self.sp2_deg.to_radians(),
            self.lon0_deg.to_radians(),
            self.lat0_deg.to_radians(),
        );
        let lat = lat_deg.to_radians();
        let lon = lon_deg.to_radians();
        let n = if (sp1 - sp2).abs() < 1e-10 {
            sp1.sin()
        } else {
            (sp1.cos().ln() - sp2.cos().ln())
                / ((std::f64::consts::FRAC_PI_4 + sp2 / 2.0).tan().ln()
                    - (std::f64::consts::FRAC_PI_4 + sp1 / 2.0).tan().ln())
        };
        let f =
            sp1.cos() * (std::f64::consts::FRAC_PI_4 + sp1 / 2.0).tan().powf(n) / n;
        let rho = |phi: f64| {
            Self::R_KM * f / (std::f64::consts::FRAC_PI_4 + phi / 2.0).tan().powf(n)
        };
        let rho_val = rho(lat);
        let rho0 = rho(lat0);
        let theta = n * (lon - lon0);
        (rho_val * theta.sin(), rho0 - rho_val * theta.cos())
    }

    /// Inverse project Cartesian (km) back to geodetic.
    pub fn unproject(&self, x_km: f64, y_km: f64) -> (f64, f64) {
        let (sp1, sp2, lon0, lat0) = (
            self.sp1_deg.to_radians(),
            self.sp2_deg.to_radians(),
            self.lon0_deg.to_radians(),
            self.lat0_deg.to_radians(),
        );
        let n = if (sp1 - sp2).abs() < 1e-10 {
            sp1.sin()
        } else {
            (sp1.cos().ln() - sp2.cos().ln())
                / ((std::f64::consts::FRAC_PI_4 + sp2 / 2.0).tan().ln()
                    - (std::f64::consts::FRAC_PI_4 + sp1 / 2.0).tan().ln())
        };
        let f =
            sp1.cos() * (std::f64::consts::FRAC_PI_4 + sp1 / 2.0).tan().powf(n) / n;
        let rho0 =
            Self::R_KM * f / (std::f64::consts::FRAC_PI_4 + lat0 / 2.0).tan().powf(n);
        let rho = (x_km.powi(2) + (rho0 - y_km).powi(2)).sqrt() * n.signum();
        let theta = (x_km / rho).atan2((rho0 - y_km) / rho);
        let lat = 2.0 * (Self::R_KM * f / rho).powf(1.0 / n).atan()
            - std::f64::consts::FRAC_PI_2;
        let lon = theta / n + lon0;
        (lat.to_degrees(), lon.to_degrees())
    }
}

const GEOHASH_CHARS: &[u8] = b"0123456789bcdefghjkmnpqrstuvwxyz";

/// Encode (lat, lon) to a geohash string of the given precision (1–12).
///
/// Returns an error if lat/lon are out of range or precision is 0.
pub fn geohash_encode(lat: f64, lon: f64, precision: usize) -> Result<String, GeoGridError> {
    if !(-90.0..=90.0).contains(&lat) {
        return Err(GeoGridError::InvalidLatitude(lat));
    }
    if !(-180.0..=180.0).contains(&lon) {
        return Err(GeoGridError::InvalidLongitude(lon));
    }
    if precision == 0 {
        return Err(GeoGridError::InvalidResolution);
    }
    let bits = precision * 5;
    let mut lat_range = (-90.0_f64, 90.0_f64);
    let mut lon_range = (-180.0_f64, 180.0_f64);
    let mut hash = String::with_capacity(precision);
    let mut acc: u8 = 0;
    let mut bit_pos = 0usize;
    let mut char_count = 0usize;
    let mut use_lon = true;
    for _ in 0..bits {
        let mid;
        if use_lon {
            mid = (lon_range.0 + lon_range.1) / 2.0;
            if lon >= mid {
                acc = (acc << 1) | 1;
                lon_range.0 = mid;
            } else {
                acc <<= 1;
                lon_range.1 = mid;
            }
        } else {
            mid = (lat_range.0 + lat_range.1) / 2.0;
            if lat >= mid {
                acc = (acc << 1) | 1;
                lat_range.0 = mid;
            } else {
                acc <<= 1;
                lat_range.1 = mid;
            }
        }
        use_lon = !use_lon;
        bit_pos += 1;
        if bit_pos == 5 {
            hash.push(GEOHASH_CHARS[acc as usize] as char);
            acc = 0;
            bit_pos = 0;
            char_count += 1;
            if char_count == precision {
                break;
            }
        }
    }
    Ok(hash)
}

/// Decode a geohash string to (lat, lon) center point.
pub fn geohash_decode(hash: &str) -> Result<(f64, f64), GeoGridError> {
    let mut lat_range = (-90.0_f64, 90.0_f64);
    let mut lon_range = (-180.0_f64, 180.0_f64);
    let mut use_lon = true;
    for ch in hash.chars() {
        let idx = GEOHASH_CHARS
            .iter()
            .position(|&b| b == ch as u8)
            .ok_or(GeoGridError::InvalidGeohashInput)?;
        let bits = idx as u8;
        for shift in (0..5).rev() {
            let bit = (bits >> shift) & 1;
            if use_lon {
                let mid = (lon_range.0 + lon_range.1) / 2.0;
                if bit == 1 {
                    lon_range.0 = mid;
                } else {
                    lon_range.1 = mid;
                }
            } else {
                let mid = (lat_range.0 + lat_range.1) / 2.0;
                if bit == 1 {
                    lat_range.0 = mid;
                } else {
                    lat_range.1 = mid;
                }
            }
            use_lon = !use_lon;
        }
    }
    Ok((
        (lat_range.0 + lat_range.1) / 2.0,
        (lon_range.0 + lon_range.1) / 2.0,
    ))
}

/// Data density classification for a geographic cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataDensity {
    /// Fewer than 0.1 active stations per 100 km².
    Sparse,
    /// 0.1–1.0 active stations per 100 km².
    Normal,
    /// More than 1.0 active stations per 100 km².
    Dense,
}

impl DataDensity {
    /// Classify density from a station count and approximate cell area.
    pub fn classify(station_count: u32, area_km2: f64) -> Self {
        if area_km2 <= 0.0 {
            return Self::Dense;
        }
        let density = f64::from(station_count) / area_km2 * 100.0;
        if density < 0.1 {
            Self::Sparse
        } else if density <= 1.0 {
            Self::Normal
        } else {
            Self::Dense
        }
    }
}

/// A geographic grid cell annotated with its weather-data scarcity level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScarcityRegion {
    /// The grid cell this region covers.
    pub cell: GeoCell,
    /// Density classification for this cell.
    pub density: DataDensity,
    /// Number of active stations inside this cell.
    pub station_count: u32,
    /// Approximate cell area (km²).
    pub area_km2: f64,
}

impl ScarcityRegion {
    /// Build a ScarcityRegion and classify its density automatically.
    pub fn new(cell: GeoCell, station_count: u32, area_km2: f64) -> Self {
        let density = DataDensity::classify(station_count, area_km2);
        Self {
            cell,
            density,
            station_count,
            area_km2,
        }
    }

    /// Reward multiplier suggested by data desert policy: 1.0 (Dense), 1.5 (Normal), 2.5 (Sparse).
    pub fn reward_multiplier(&self) -> f64 {
        match self.density {
            DataDensity::Dense => 1.0,
            DataDensity::Normal => 1.5,
            DataDensity::Sparse => 2.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_quantization_round_trips() {
        let samples = [
            (0.0, 0.0),
            (45.1234, -122.8765),
            (-33.865, 151.2094),
            (89.999, 179.999),
        ];

        for (lat, lon) in samples {
            let cell = GeoCell::from_lat_lon(lat, lon, 10).expect("cell should quantize");
            let center = cell.center();
            let round_trip =
                GeoCell::from_lat_lon(center.0, center.1, 10).expect("center should quantize");
            assert_eq!(cell, round_trip);
        }
    }

    #[test]
    fn neighbor_counts_at_poles_and_equator() {
        let equator = GeoCell::from_lat_lon(0.0, 0.0, 10).expect("equator cell");
        let north_pole = GeoCell::from_lat_lon(89.9999, 0.0, 10).expect("north pole cell");
        let south_pole = GeoCell::from_lat_lon(-89.9999, 0.0, 10).expect("south pole cell");

        assert_eq!(equator.neighbors().len(), 8);
        assert!(north_pole.neighbors().len() <= 5);
        assert!(south_pole.neighbors().len() <= 5);
    }

    #[test]
    fn radius_query_includes_center_cell() {
        let center = GeoCell::from_lat_lon(37.7749, -122.4194, 10).expect("center");
        let grid = GeoGrid::cells_within_radius(center, 0.0);
        assert_eq!(grid.cells.len(), 1);
        assert_eq!(grid.cells[0], center);
    }

    #[test]
    fn radius_query_with_nonzero_radius_returns_multiple_cells() {
        let center = GeoCell::from_lat_lon(37.7749, -122.4194, 10).expect("center");
        let grid = GeoGrid::cells_within_radius(center, 30.0);
        assert!(grid.cells.len() > 1);
        assert!(grid.cells.contains(&center));
    }

    #[test]
    fn anti_meridian_bounds_are_not_degenerate() {
        let left = GeoCell::from_lat_lon(0.0, 179.9, 10).expect("left");
        let right = GeoCell::from_lat_lon(0.0, -179.9, 10).expect("right");
        let grid = GeoGrid {
            cells: vec![left, right],
            bounds: bounds_for_cells(&[left, right]),
        };

        assert_ne!(grid.bounds.min_lon, grid.bounds.max_lon);
        assert!(grid.bounds.min_lon > grid.bounds.max_lon);
    }

    #[test]
    fn high_latitude_radius_expands_longitude_scan() {
        let center = GeoCell::from_lat_lon(89.9, 0.0, 10).expect("center");
        let grid = GeoGrid::cells_within_radius(center, 10.0);
        assert!(grid.cells.len() > 8);
    }

    #[test]
    fn grid_serde_round_trip() {
        let center = GeoCell::from_lat_lon(37.7749, -122.4194, 10).expect("center");
        let grid = GeoGrid::cells_within_radius(center, 25.0);
        let encoded = serde_json::to_string(&grid).expect("serialize");
        let decoded: GeoGrid = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(grid.cells, decoded.cells);
        assert!((grid.bounds.min_lat - decoded.bounds.min_lat).abs() < 1e-9);
        assert!((grid.bounds.max_lat - decoded.bounds.max_lat).abs() < 1e-9);
        assert!((grid.bounds.min_lon - decoded.bounds.min_lon).abs() < 1e-9);
        assert!((grid.bounds.max_lon - decoded.bounds.max_lon).abs() < 1e-9);
    }

    #[test]
    fn lambert_project_unproject_roundtrip() {
        let proj = LambertConformal {
            sp1_deg: 25.0,
            sp2_deg: 60.0,
            lon0_deg: -95.0,
            lat0_deg: 23.0,
        };
        let (x, y) = proj.project(40.0, -105.0);
        let (lat2, lon2) = proj.unproject(x, y);
        assert!((lat2 - 40.0).abs() < 0.001, "lat roundtrip");
        assert!((lon2 - -105.0).abs() < 0.001, "lon roundtrip");
    }

    #[test]
    fn geohash_encode_decode_roundtrip() {
        let (lat, lon) = (51.5074, -0.1278);
        let h = geohash_encode(lat, lon, 7).unwrap();
        assert_eq!(h.len(), 7);
        let (dlat, dlon) = geohash_decode(&h).unwrap();
        assert!((dlat - lat).abs() < 0.01, "decoded lat close");
        assert!((dlon - lon).abs() < 0.01, "decoded lon close");
    }

    #[test]
    fn scarcity_region_density_classification() {
        let cell = GeoCell::from_lat_lon(40.0, -105.0, 50).unwrap();
        let sparse = ScarcityRegion::new(cell, 0, 10_000.0);
        assert_eq!(sparse.density, DataDensity::Sparse);
        assert_eq!(sparse.reward_multiplier(), 2.5);

        let dense = ScarcityRegion::new(cell, 500, 1_000.0);
        assert_eq!(dense.density, DataDensity::Dense);
        assert_eq!(dense.reward_multiplier(), 1.0);
    }
}
