//! Read-only view of the paid data-scarcity rule (core4#28, ADR 2026-10-23-3563, answer A1).
//!
//! The chain pays a desert bonus per reveal through
//! `wrfcoin_shared_protocol::consensus::data_desert::desert_multiplier`, counting active
//! registered stations per ≈2° cell in `state.station_counts`. This route calls that SAME
//! function against that SAME map so a client can show exactly what is paid. It reads chain
//! state under one read lock and writes nothing: no reward, validation or state-root change.

use std::collections::BTreeMap;

use actix_web::http::StatusCode;
use actix_web::{HttpRequest, HttpResponse, Responder, web};
use serde::{Deserialize, Serialize};

use wrfcoin_geo_grid::GeoCell;
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_shared_protocol::consensus::data_desert::{
    DataDesertPolicy, cell_for_station, desert_multiplier,
};
use wrfcoin_shared_protocol::rewards::RewardConfig;

use crate::helpers::{envelope_error, require_role};
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ScarcityRuleView {
    /// Grid edge the chain counts stations on (≈ 2° at the equator).
    pub grid_km: u8,
    /// Edge of one cell in degrees (lat and lon).
    // determinism-ok: read-only API echo / request bounds; not consensus arithmetic
    pub cell_size_degrees: f64,
    /// A cell with fewer active registered stations than this pays the bonus.
    pub min_station_density: u32,
    /// Multiplier in percent for a station in such a cell (150 = ×1.5).
    pub desert_bonus_percent: u32,
    /// Multiplier in percent otherwise (always 100).
    pub baseline_percent: u32,
    /// Block height from which the bonus applies.
    pub activation_height: u64,
    /// Whether the bonus applies at `observed_at_height + 1`, the next block.
    pub active: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ScarcityCellView {
    pub lat_idx: i32,
    pub lon_idx: i32,
    // determinism-ok: read-only API echo / request bounds; not consensus arithmetic
    pub center_latitude: f64,
    // determinism-ok: read-only API echo / request bounds; not consensus arithmetic
    pub center_longitude: f64,
    /// Active registered stations counted in this cell by the chain.
    pub station_count: u32,
    /// What a station registered in this cell is paid at the next block, in percent.
    pub multiplier_percent: u32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ScarcityPointView {
    pub cell_lat_idx: i32,
    pub cell_lon_idx: i32,
    /// Stations the chain counts in the queried point's cell, including the station itself
    /// once registered.
    pub station_count: u32,
    pub multiplier_percent: u32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ScarcityResponse {
    pub rule: ScarcityRuleView,
    /// Cells with at least one active station. Any other cell has count 0 and pays
    /// `desert_bonus_percent` to a first station.
    pub cells: Vec<ScarcityCellView>,
    /// Present when `lat` and `lon` were supplied.
    pub point: Option<ScarcityPointView>,
    pub observed_at_height: u64,
    pub state_root_hex: String,
}

#[derive(Debug, Deserialize)]
pub struct ScarcityQuery {
    // determinism-ok: read-only API echo / request bounds; not consensus arithmetic
    pub lat: Option<f64>,
    // determinism-ok: read-only API echo / request bounds; not consensus arithmetic
    pub lon: Option<f64>,
}

/// The resolution the payout quantizes stations to, read from the payout's own quantizer.
fn grid_resolution_km() -> u8 {
    // determinism-ok: probe coordinate for a read-only API echo; the payout path is untouched
    cell_for_station(0.0, 0.0).resolution_km
}

/// Build the view from the exact inputs the payout reads. `height` is the block height the
/// bonus would be evaluated at.
pub fn build_scarcity_view(
    station_counts: &BTreeMap<GeoCell, u32>,
    policy: &DataDesertPolicy,
    height: u64,
    // determinism-ok: read-only API echo / request bounds; not consensus arithmetic
    point: Option<(f64, f64)>,
) -> (ScarcityRuleView, Vec<ScarcityCellView>, Option<ScarcityPointView>) {
    let resolution_km = grid_resolution_km();
    let row = |lat_idx| GeoCell { lat_idx, lon_idx: 0, resolution_km }.center().0;
    // determinism-ok: read-only API echo of the cell edge in degrees; not consensus arithmetic
    let cell_size_degrees = row(1) - row(0);
    let rule = ScarcityRuleView {
        grid_km: resolution_km,
        cell_size_degrees,
        min_station_density: policy.min_station_density,
        desert_bonus_percent: policy.desert_bonus_bps.max(100),
        baseline_percent: 100,
        activation_height: policy.activation_height,
        active: height >= policy.activation_height,
    };
    let cells = station_counts
        .iter()
        .filter(|(_, count)| **count > 0)
        .map(|(cell, count)| {
            let (lat, lon) = cell.center();
            ScarcityCellView {
                lat_idx: cell.lat_idx,
                lon_idx: cell.lon_idx,
                center_latitude: lat,
                center_longitude: lon,
                station_count: *count,
                multiplier_percent: desert_multiplier(lat, lon, station_counts, policy, height),
            }
        })
        .collect();
    let point = point.map(|(lat, lon)| {
        let cell = cell_for_station(lat, lon);
        ScarcityPointView {
            cell_lat_idx: cell.lat_idx,
            cell_lon_idx: cell.lon_idx,
            station_count: station_counts.get(&cell).copied().unwrap_or(0),
            multiplier_percent: desert_multiplier(lat, lon, station_counts, policy, height),
        }
    });
    (rule, cells, point)
}

/// `GET /network/scarcity[?lat=..&lon=..]`
pub async fn get_scarcity(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<ScarcityQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let point = match (query.lat, query.lon) {
        (None, None) => None,
        // determinism-ok: request bounds check on a read-only route; not consensus arithmetic
        (Some(lat), Some(lon))
            if lat.is_finite()
                && lon.is_finite()
                // determinism-ok: read-only API echo / request bounds; not consensus arithmetic
                && (-90.0..=90.0).contains(&lat)
                // determinism-ok: read-only API echo / request bounds; not consensus arithmetic
                && (-180.0..=180.0).contains(&lon) =>
        {
            Some((lat, lon))
        },
        _ => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "lat and lon must both be given, with lat in [-90, 90] and lon in [-180, 180]",
            );
        },
    };

    let blockchain = state.blockchain.read().await;
    // Same default policy the payout uses (`RewardConfig::default().data_desert_policy`);
    // no runtime path replaces it. The next block is the one that would pay.
    let policy = RewardConfig::default().data_desert_policy;
    let (rule, cells, point) = build_scarcity_view(
        &blockchain.state.station_counts,
        &policy,
        blockchain.height.saturating_add(1),
        point,
    );
    HttpResponse::Ok().json(ScarcityResponse {
        rule,
        cells,
        point,
        observed_at_height: blockchain.height,
        state_root_hex: hex::encode(blockchain.state.state_root.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(entries: &[(f64, f64, u32)]) -> BTreeMap<GeoCell, u32> {
        let mut map = BTreeMap::new();
        for (lat, lon, n) in entries {
            *map.entry(cell_for_station(*lat, *lon)).or_insert(0) += *n;
        }
        map
    }

    #[test]
    fn point_value_equals_the_paid_multiplier_for_every_sample() {
        let policy = RewardConfig::default().data_desert_policy;
        let sc = counts(&[(51.3, -0.1, 1), (40.0, -105.0, 3), (47.5, -94.0, 2), (35.0, 139.0, 7)]);
        for (lat, lon) in [(51.3, -0.1), (40.0, -105.0), (47.5, -94.0), (35.0, 139.0), (-33.9, 151.2), (0.0, 0.0), (89.9, 179.9)] {
            let (_, _, p) = build_scarcity_view(&sc, &policy, 100, Some((lat, lon)));
            assert_eq!(
                p.unwrap().multiplier_percent,
                desert_multiplier(lat, lon, &sc, &policy, 100),
                "({lat},{lon})"
            );
        }
    }

    #[test]
    fn two_degree_cell_not_tenth_degree() {
        let policy = DataDesertPolicy::default();
        // Three stations 0.5 deg apart share one ≈2° cell: dense (x1.0), not three desert cells.
        let sc = counts(&[(47.1, -94.1, 1), (47.6, -94.1, 1), (47.9, -94.5, 1)]);
        let (rule, cells, p) = build_scarcity_view(&sc, &policy, 100, Some((47.6, -94.1)));
        assert_eq!(rule.grid_km, 222);
        assert!((rule.cell_size_degrees - 2.0).abs() < 0.05, "{}", rule.cell_size_degrees);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].station_count, 3);
        assert_eq!(cells[0].multiplier_percent, 100);
        assert_eq!(p.unwrap().multiplier_percent, 100);
    }

    #[test]
    fn empty_cell_pays_the_desert_bonus_and_no_two_x_tier_exists() {
        let policy = DataDesertPolicy::default();
        let (rule, cells, p) = build_scarcity_view(&BTreeMap::new(), &policy, 100, Some((10.0, 10.0)));
        assert!(cells.is_empty());
        let p = p.unwrap();
        assert_eq!((p.station_count, p.multiplier_percent), (0, 150));
        assert_eq!((rule.min_station_density, rule.desert_bonus_percent, rule.baseline_percent), (3, 150, 100));
    }

    #[test]
    fn before_activation_reports_baseline_and_inactive() {
        let policy = DataDesertPolicy { activation_height: 1_000, ..DataDesertPolicy::default() };
        let sc = counts(&[(51.3, -0.1, 1)]);
        let (rule, cells, _) = build_scarcity_view(&sc, &policy, 999, None);
        assert!(!rule.active);
        assert_eq!(cells[0].multiplier_percent, 100);
    }

    #[test]
    fn threshold_boundary_two_pays_three_does_not() {
        let policy = DataDesertPolicy::default();
        let two = counts(&[(10.0, 10.0, 2)]);
        let three = counts(&[(10.0, 10.0, 3)]);
        assert_eq!(build_scarcity_view(&two, &policy, 5, None).1[0].multiplier_percent, 150);
        assert_eq!(build_scarcity_view(&three, &policy, 5, None).1[0].multiplier_percent, 100);
    }
}
