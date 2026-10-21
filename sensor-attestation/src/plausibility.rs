//! #2207 — calibration, location-plausibility and cross-validation
//! attestation: the process that turns a *signed* reading into a
//! *trustworthy* one.
//!
//! The hard part, stated up front: defining implausibility for weather data
//! without discarding real extreme events — which are exactly the readings a
//! weather network exists to capture. So this module is **flag-and-tier,
//! never reject**. [`CheckOutcome::Fail`] is reserved for the physically
//! impossible; everything else that looks wrong is a [`CheckOutcome::Flag`]
//! that lowers the [`crate::plausibility::AssessmentTier`] and the proposed reward weight, and the
//! reading is retained with its assessment so it can be re-scored later
//! (the #2053 authoritative re-score path).
//!
//! Checks, in the order the story names them:
//!
//! - [`crate::plausibility::physical_range`] — outside physically possible bounds, not merely
//!   unusual ones; the only source of `Fail`.
//! - [`crate::plausibility::rate_over_window`] (built on [`crate::plausibility::rate_of_change`]) —
//!   implausible change of one device's newest reading over the trailing window; a flag, or a
//!   fail past a physically impossible rate.
//! - [`crate::plausibility::neighbour_agreement`] — disagreement with nearby stations beyond the
//!   expected spatial variance. The strongest signal, and the one a lying
//!   sensor cannot fake alone; never more than a flag, because a real local
//!   extreme disagrees with its neighbours too.
//! - [`crate::plausibility::diurnal_pattern`] — a temperature sensor with no day/night cycle is
//!   indoors or broken.
//! - [`crate::plausibility::location_consistency`] — the claimed elevation against the elevation
//!   the station's own pressure implies.
//! - [`crate::plausibility::calibration_currency`] — three distinct outcomes: cryptographically
//!   verified and current, expired, or **verification unavailable**, which
//!   is never a pass (backend #931's boundary).
//!
//! Location and privacy, resolved explicitly: every function here takes the
//! **coarse** location a station discloses for verification
//! ([`crate::plausibility::CoarseLocation`], a 0.1° cell), and neighbour distances are computed
//! from those cells. Precise location is disclosed selectively elsewhere and
//! never enters an assessment. Nothing here is consensus: assessments are
//! shadow quality evidence until a feature-gated reward policy reads them.

use serde::{Deserialize, Serialize};
use wrf_primitives::Timestamp;
use wrfcoin_shared_protocol::station_registry::DataQualityTier;

/// Assessor revision recorded on every assessment so a re-score can tell
/// which rules produced the stored verdict.
pub const ASSESSOR_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum CheckKind {
    PhysicalRange,
    RateOfChange,
    NeighbourAgreement,
    DiurnalPattern,
    LocationConsistency,
    ReanalysisAgreement,
    CalibrationCurrency,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "outcome")]
pub enum CheckOutcome {
    Pass,
    /// Suspicious, retained, paid less pending confirmation.
    Flag { reason: String },
    /// Physically impossible. Retained for the record, never paid.
    Fail { reason: String },
}

impl CheckOutcome {
    pub fn is_flag(&self) -> bool {
        matches!(self, Self::Flag { .. })
    }
    pub fn is_fail(&self) -> bool {
        matches!(self, Self::Fail { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlausibilityCheck {
    pub kind: CheckKind,
    pub outcome: CheckOutcome,
    /// How much this outcome should weigh, 0.0..=1.0. A flag below
    /// [`FLAG_CONFIDENCE_FLOOR`] is recorded but does not move the tier.
    pub confidence: f32,
}

impl PlausibilityCheck {
    fn new(kind: CheckKind, outcome: CheckOutcome, confidence: f32) -> Self {
        Self { kind, outcome, confidence: confidence.clamp(0.0, 1.0) }
    }
    fn pass(kind: CheckKind, confidence: f32) -> Self {
        Self::new(kind, CheckOutcome::Pass, confidence)
    }
    fn flag(kind: CheckKind, reason: impl Into<String>, confidence: f32) -> Self {
        Self::new(kind, CheckOutcome::Flag { reason: reason.into() }, confidence)
    }
    fn fail(kind: CheckKind, reason: impl Into<String>) -> Self {
        Self::new(kind, CheckOutcome::Fail { reason: reason.into() }, 1.0)
    }
}

/// A flag with less confidence than this is noted, not acted on.
pub const FLAG_CONFIDENCE_FLOOR: f32 = 0.25;

/// Tier of one reading's assessment, best first.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum AssessmentTier {
    /// A physically impossible reading. Retained, unpaid.
    Rejected,
    /// Neighbour disagreement, or several flags: paid least pending confirmation.
    Provisional,
    /// One flag.
    Reduced,
    /// Every check passed or was uninformative.
    Full,
}

impl AssessmentTier {
    /// Proposed reward weight, basis points. Shadow evidence: not read by
    /// consensus until a feature-gated policy does.
    pub fn proposed_reward_weight_bps(self) -> u32 {
        match self {
            Self::Full => 10_000,
            Self::Reduced => 7_500,
            Self::Provisional => 5_000,
            Self::Rejected => 0,
        }
    }

    /// The highest data-quality tier a reading at this assessment supports.
    pub fn quality_tier_ceiling(self) -> DataQualityTier {
        match self {
            Self::Full => DataQualityTier::Gold,
            Self::Reduced => DataQualityTier::Silver,
            Self::Provisional => DataQualityTier::Bronze,
            Self::Rejected => DataQualityTier::Unverified,
        }
    }
}

/// The stored verdict: every check that ran, the tier they imply, and when
/// and by which assessor revision. Stored alongside the reading; the reading
/// itself is never discarded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualityAssessment {
    pub checks: Vec<PlausibilityCheck>,
    pub tier: AssessmentTier,
    pub assessed_at: Timestamp,
    pub assessor_version: u32,
    /// Assessors that produced earlier verdicts for this reading, oldest
    /// first — a re-score keeps the history rather than overwriting it.
    #[serde(default)]
    pub previous_tiers: Vec<(Timestamp, AssessmentTier)>,
}

impl QualityAssessment {
    /// Derive the tier from the checks. Any `Fail` rejects; a confident
    /// neighbour flag or two confident flags make it provisional; one
    /// confident flag reduces it; otherwise full.
    pub fn assess(checks: Vec<PlausibilityCheck>, assessed_at: Timestamp) -> Self {
        let tier = tier_for(&checks);
        Self { checks, tier, assessed_at, assessor_version: ASSESSOR_VERSION, previous_tiers: Vec::new() }
    }

    /// Re-score with additional or replacement checks (later neighbour data,
    /// a verified calibration, a reanalysis), keeping the prior verdict on
    /// record. A check of a kind already present is replaced.
    pub fn rescore(&self, new_checks: Vec<PlausibilityCheck>, rescored_at: Timestamp) -> Self {
        let mut checks: Vec<PlausibilityCheck> = self
            .checks
            .iter()
            .filter(|c| !new_checks.iter().any(|n| n.kind == c.kind))
            .cloned()
            .collect();
        checks.extend(new_checks);
        let mut previous_tiers = self.previous_tiers.clone();
        previous_tiers.push((self.assessed_at, self.tier));
        Self {
            tier: tier_for(&checks),
            checks,
            assessed_at: rescored_at,
            assessor_version: ASSESSOR_VERSION,
            previous_tiers,
        }
    }

    /// Readings are retained at every tier; this is the invariant the module
    /// exists for, spelled out for callers.
    pub fn is_retained(&self) -> bool {
        true
    }

    pub fn confident_flags(&self) -> impl Iterator<Item = &PlausibilityCheck> {
        self.checks
            .iter()
            .filter(|c| c.outcome.is_flag() && c.confidence >= FLAG_CONFIDENCE_FLOOR)
    }
}

fn tier_for(checks: &[PlausibilityCheck]) -> AssessmentTier {
    if checks.iter().any(|c| c.outcome.is_fail()) {
        return AssessmentTier::Rejected;
    }
    let confident: Vec<&PlausibilityCheck> = checks
        .iter()
        .filter(|c| c.outcome.is_flag() && c.confidence >= FLAG_CONFIDENCE_FLOOR)
        .collect();
    let neighbour_flag = confident.iter().any(|c| c.kind == CheckKind::NeighbourAgreement);
    match (confident.len(), neighbour_flag) {
        (0, _) => AssessmentTier::Full,
        (1, false) => AssessmentTier::Reduced,
        _ => AssessmentTier::Provisional,
    }
}

// ---------------------------------------------------------------------------
// Variables and physical bounds
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum Variable {
    TemperatureC,
    PressureHpa,
    HumidityPct,
    WindSpeedMps,
    PrecipMmPerHour,
}

impl Variable {
    /// Physically possible bounds: records plus margin, not climatology.
    /// (Vostok −89.2 °C, Furnace Creek 56.7 °C; Agata 1083.8 hPa, Typhoon Tip
    /// 870 hPa at sea level; Barrow Island gust 113 m/s; Holt 305 mm/h.)
    pub fn physical_bounds(self) -> (f64, f64) {
        match self {
            Self::TemperatureC => (-95.0, 65.0),
            Self::PressureHpa => (300.0, 1_100.0),
            Self::HumidityPct => (0.0, 100.0),
            Self::WindSpeedMps => (0.0, 120.0),
            Self::PrecipMmPerHour => (0.0, 400.0),
        }
    }

    /// Per-hour change that is unusual (flag) and impossible (fail).
    pub fn rate_thresholds_per_hour(self) -> (f64, f64) {
        match self {
            Self::TemperatureC => (15.0, 60.0),
            Self::PressureHpa => (8.0, 60.0),
            Self::HumidityPct => (50.0, 400.0),
            Self::WindSpeedMps => (40.0, 400.0),
            Self::PrecipMmPerHour => (200.0, 2_000.0),
        }
    }

    /// Expected spatial variance between nearby stations: a base sigma plus a
    /// per-kilometre gradient.
    pub fn spatial_sigma(self, distance_km: f64) -> f64 {
        let (base, per_km) = match self {
            Self::TemperatureC => (1.5, 0.05),
            Self::PressureHpa => (1.0, 0.02),
            Self::HumidityPct => (8.0, 0.2),
            Self::WindSpeedMps => (3.0, 0.05),
            Self::PrecipMmPerHour => (5.0, 0.3),
        };
        base + per_km * distance_km.max(0.0)
    }
}

/// A reading outside physically possible bounds fails; a non-finite value
/// fails; anything else passes with full confidence.
pub fn physical_range(variable: Variable, value: f64) -> PlausibilityCheck {
    let (lo, hi) = variable.physical_bounds();
    if !value.is_finite() {
        return PlausibilityCheck::fail(CheckKind::PhysicalRange, format!("{variable:?} is not a finite number"));
    }
    if value < lo || value > hi {
        return PlausibilityCheck::fail(
            CheckKind::PhysicalRange,
            format!("{variable:?} = {value} is outside the physically possible range [{lo}, {hi}]"),
        );
    }
    PlausibilityCheck::pass(CheckKind::PhysicalRange, 1.0)
}

/// Shortest interval a delta is normalised over. Readings seconds apart carry
/// quantisation steps and gusts that, scaled to a per-hour rate, would look
/// physically impossible; the per-hour thresholds describe weather, not
/// sensor noise, so a short interval is treated as this long.
pub const RATE_MIN_INTERVAL_SECS: u64 = 15 * 60;

/// Delta between two readings of one device, normalised per hour over at least
/// [`RATE_MIN_INTERVAL_SECS`]. Past the unusual rate: a flag; past the
/// impossible rate: a fail.
///
/// **Contract.** Because of the floor, this is a *pair* check: fed only
/// consecutive readings seconds apart, a staircase of sub-threshold steps can
/// hide a physically impossible change (−14.9 hPa every 10 s is 59.6/h per
/// pair, under the 60/h fail rate, yet −89.4 hPa in 60 s). Assessing a device's
/// stream must go through [`rate_over_window`], which compares the newest
/// reading with every reading in the trailing window. Call this directly only
/// when `previous` is at least [`RATE_MIN_INTERVAL_SECS`] before `next`.
pub fn rate_of_change(variable: Variable, previous: f64, next: f64, elapsed_secs: u64) -> PlausibilityCheck {
    if elapsed_secs == 0 || !previous.is_finite() || !next.is_finite() {
        return PlausibilityCheck::flag(CheckKind::RateOfChange, "no usable interval between readings", 0.1);
    }
    let interval_secs = elapsed_secs.max(RATE_MIN_INTERVAL_SECS);
    let per_hour = (next - previous).abs() * 3_600.0 / interval_secs as f64;
    let (unusual, impossible) = variable.rate_thresholds_per_hour();
    if per_hour > impossible {
        return PlausibilityCheck::fail(
            CheckKind::RateOfChange,
            format!("{variable:?} changed {per_hour:.1} per hour, beyond the physically possible {impossible}"),
        );
    }
    if per_hour > unusual {
        let confidence = ((per_hour - unusual) / (impossible - unusual)).clamp(0.3, 1.0) as f32;
        return PlausibilityCheck::flag(
            CheckKind::RateOfChange,
            format!("{variable:?} changed {per_hour:.1} per hour, above the usual {unusual}"),
            confidence,
        );
    }
    PlausibilityCheck::pass(CheckKind::RateOfChange, 1.0)
}

/// Rate of change of the **newest** reading in `samples` (one device, one
/// variable), judged against every earlier reading in the trailing
/// [`RATE_MIN_INTERVAL_SECS`] window and against the most recent reading at
/// or beyond it. The worst outcome wins: fail, then the most confident flag,
/// then pass. A staircase of small steps is therefore judged by its net change
/// over the window, so it fails exactly as the same change in one step would,
/// while sensor noise seconds apart still normalises over the floor and passes
/// (review of core4#3118).
///
/// Non-finite readings are ignored. With no earlier reading there is nothing
/// to compare: it passes with zero confidence, like a lone station.
pub fn rate_over_window(variable: Variable, samples: &[(Timestamp, f64)]) -> PlausibilityCheck {
    let mut finite: Vec<(u64, f64)> = samples
        .iter()
        .filter(|(_, v)| v.is_finite())
        .map(|(t, v)| (t.into_inner(), *v))
        .collect();
    finite.sort_by_key(|(t, _)| *t);
    let Some(&(t_new, v_new)) = finite.last() else {
        return PlausibilityCheck::pass(CheckKind::RateOfChange, 0.0);
    };
    let earlier: Vec<(u64, f64)> = finite.iter().copied().filter(|(t, _)| *t < t_new).collect();
    let within = earlier.iter().filter(|(t, _)| t_new - t < RATE_MIN_INTERVAL_SECS);
    let baseline = earlier.iter().rev().find(|(t, _)| t_new - t >= RATE_MIN_INTERVAL_SECS);
    let mut worst: Option<PlausibilityCheck> = None;
    for &(t, v) in within.chain(baseline) {
        let check = rate_of_change(variable, v, v_new, t_new - t);
        let rank = |c: &PlausibilityCheck| match c.outcome {
            CheckOutcome::Fail { .. } => (2, c.confidence),
            CheckOutcome::Flag { .. } => (1, c.confidence),
            CheckOutcome::Pass => (0, c.confidence),
        };
        let replace = match &worst {
            None => true,
            Some(w) => rank(&check).0 > rank(w).0 || (rank(&check).0 == rank(w).0 && rank(&check).1 > rank(w).1),
        };
        if replace {
            worst = Some(check);
        }
    }
    worst.unwrap_or_else(|| PlausibilityCheck::pass(CheckKind::RateOfChange, 0.0))
}

/// A neighbour's reading at a distance computed from coarse cells.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NeighbourReading {
    pub distance_km: f64,
    pub value: f64,
}

/// Neighbours farther than this do not constrain a reading.
pub const NEIGHBOUR_MAX_DISTANCE_KM: f64 = 75.0;
/// Fewer neighbours than this is not evidence either way.
pub const NEIGHBOUR_MIN_COUNT: usize = 2;
/// Standard deviations of spatial variance beyond which a reading is flagged.
pub const NEIGHBOUR_FLAG_SIGMAS: f64 = 3.0;

/// Distance-weighted agreement with nearby stations. With fewer than
/// [`NEIGHBOUR_MIN_COUNT`] neighbours in range the check passes with zero
/// confidence — a lone station is not penalised for being alone. A
/// disagreement beyond [`NEIGHBOUR_FLAG_SIGMAS`] is a flag whose confidence
/// grows with the number of neighbours; it is never a fail, because a real
/// local extreme disagrees with its neighbours as well.
pub fn neighbour_agreement(variable: Variable, value: f64, neighbours: &[NeighbourReading]) -> PlausibilityCheck {
    let usable: Vec<&NeighbourReading> = neighbours
        .iter()
        .filter(|n| n.distance_km.is_finite() && n.distance_km <= NEIGHBOUR_MAX_DISTANCE_KM && n.value.is_finite())
        .collect();
    if usable.len() < NEIGHBOUR_MIN_COUNT || !value.is_finite() {
        return PlausibilityCheck::pass(CheckKind::NeighbourAgreement, 0.0);
    }
    let mut weight_sum = 0.0;
    let mut weighted = 0.0;
    let mut distance_sum = 0.0;
    for n in &usable {
        let w = 1.0 / (1.0 + n.distance_km);
        weight_sum += w;
        weighted += w * n.value;
        distance_sum += n.distance_km;
    }
    let consensus = weighted / weight_sum;
    let mean_distance = distance_sum / usable.len() as f64;
    let sigma = variable.spatial_sigma(mean_distance);
    let z = (value - consensus).abs() / sigma;
    let confidence = (usable.len() as f32 / 5.0).clamp(0.4, 1.0);
    if z > NEIGHBOUR_FLAG_SIGMAS {
        return PlausibilityCheck::flag(
            CheckKind::NeighbourAgreement,
            format!(
                "{variable:?} = {value} is {z:.1} sigma from the {}-neighbour consensus {consensus:.2} \
                 (mean distance {mean_distance:.1} km); recorded, paid less pending confirmation",
                usable.len()
            ),
            confidence,
        );
    }
    PlausibilityCheck::pass(CheckKind::NeighbourAgreement, confidence)
}

/// Minimum span and sample count for a diurnal verdict.
pub const DIURNAL_MIN_SPAN_SECS: u64 = 24 * 3_600;
pub const DIURNAL_MIN_SAMPLES: usize = 8;
/// A fitted day/night amplitude below this, in °C, is no cycle at all.
pub const DIURNAL_MIN_AMPLITUDE_C: f64 = 1.0;

/// Fit the first 24 h harmonic of a temperature series. A sensor with no
/// day/night cycle is indoors, shaded into uselessness, or broken: a flag
/// whose confidence grows with the span observed. Too little data is an
/// uninformative pass, not a flag.
pub fn diurnal_pattern(samples: &[(Timestamp, f64)]) -> PlausibilityCheck {
    let finite: Vec<(Timestamp, f64)> = samples.iter().copied().filter(|(_, v)| v.is_finite()).collect();
    if finite.len() < DIURNAL_MIN_SAMPLES {
        return PlausibilityCheck::pass(CheckKind::DiurnalPattern, 0.0);
    }
    let t0 = finite.iter().map(|(t, _)| t.into_inner()).min().unwrap_or(0);
    let t1 = finite.iter().map(|(t, _)| t.into_inner()).max().unwrap_or(0);
    let span = t1.saturating_sub(t0);
    if span < DIURNAL_MIN_SPAN_SECS {
        return PlausibilityCheck::pass(CheckKind::DiurnalPattern, 0.0);
    }
    let omega = std::f64::consts::TAU / 86_400.0;
    let mut normal = [[0.0f64; 3]; 3];
    let mut rhs = [0.0f64; 3];
    for (t, y) in &finite {
        let x = t.into_inner() as f64 - t0 as f64;
        let phi = [1.0, (omega * x).cos(), (omega * x).sin()];
        // Iterator zips, not indices: every row, cell and rhs entry is paired
        // with its basis term by construction, so nothing can be out of range.
        for ((row, r), pi) in normal.iter_mut().zip(rhs.iter_mut()).zip(phi.iter()) {
            *r += pi * y;
            for (cell, pj) in row.iter_mut().zip(phi.iter()) {
                *cell += pi * pj;
            }
        }
    }
    let Some(coeff) = solve3(normal, rhs) else {
        return PlausibilityCheck::pass(CheckKind::DiurnalPattern, 0.0);
    };
    let amplitude = coeff[1].hypot(coeff[2]);
    let confidence = ((span as f64 / (3.0 * 86_400.0)) as f32).clamp(0.3, 1.0);
    if amplitude < DIURNAL_MIN_AMPLITUDE_C {
        return PlausibilityCheck::flag(
            CheckKind::DiurnalPattern,
            format!(
                "temperature shows a {amplitude:.2} °C day/night amplitude over {:.1} days: \
                 indoors, fully shaded, or a stuck sensor",
                span as f64 / 86_400.0
            ),
            confidence,
        );
    }
    PlausibilityCheck::pass(CheckKind::DiurnalPattern, confidence)
}

/// Solve the 3×3 system `a · x = b` by Cramer's rule over destructured
/// arrays: no indexing, so no panic path (#3308). `None` when the system is
/// singular relative to its own scale (the normal matrix of a degenerate
/// sample set), which the caller treats as "no evidence".
fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let scale = a.iter().flatten().fold(0.0f64, |m, v| m.max(v.abs()));
    if scale == 0.0 {
        return None;
    }
    let [[a00, a01, a02], [a10, a11, a12], [a20, a21, a22]] = a;
    let (c0, c1, c2) = ([a00, a10, a20], [a01, a11, a21], [a02, a12, a22]);
    // Scalar triple product u · (v × w): the determinant of columns u, v, w.
    let det = |[u0, u1, u2]: [f64; 3], [v0, v1, v2]: [f64; 3], [w0, w1, w2]: [f64; 3]| {
        u0 * (v1 * w2 - v2 * w1) - u1 * (v0 * w2 - v2 * w0) + u2 * (v0 * w1 - v1 * w0)
    };
    let d = det(c0, c1, c2);
    if !d.is_finite() || d.abs() < 1e-9 * scale * scale * scale {
        return None;
    }
    Some([det(b, c1, c2) / d, det(c0, b, c2) / d, det(c0, c1, b) / d])
}

/// The location a station discloses for verification: a 0.1° cell, roughly
/// 11 km. Built from a precise position by rounding, and the only form of
/// location this module ever sees.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoarseLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_m: Option<f64>,
}

impl CoarseLocation {
    pub const CELL_DEGREES: f64 = 0.1;

    /// Round a precise position to its cell; the precise input is dropped.
    pub fn from_precise(latitude: f64, longitude: f64, elevation_m: Option<f64>) -> Self {
        // Round via the integer tenth so the cell is the nearest double to
        // the decimal it names (46.8, not 46.800000000000004).
        let cell = |v: f64| (v / Self::CELL_DEGREES).round() / (1.0 / Self::CELL_DEGREES);
        Self {
            latitude: cell(latitude.clamp(-90.0, 90.0)),
            longitude: cell(longitude.clamp(-180.0, 180.0)),
            elevation_m: elevation_m.filter(|e| e.is_finite()).map(|e| (e / 10.0).round() * 10.0),
        }
    }

    /// Great-circle distance between two cells, kilometres.
    pub fn distance_km(&self, other: &Self) -> f64 {
        let to_rad = |d: f64| d.to_radians();
        let (lat1, lat2) = (to_rad(self.latitude), to_rad(other.latitude));
        let dlat = lat2 - lat1;
        let dlon = to_rad(other.longitude - self.longitude);
        let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
        2.0 * 6_371.0 * a.sqrt().asin()
    }
}

/// Elevation disagreement, metres, beyond which the claimed location is flagged.
pub const LOCATION_ELEVATION_TOLERANCE_M: f64 = 300.0;

/// The station's own pressure implies an elevation (hypsometric equation
/// against the regional sea-level pressure); a claimed elevation far from it
/// is inconsistent with the claimed location. With no claimed elevation or no
/// regional reference the check is uninformative.
pub fn location_consistency(
    location: &CoarseLocation,
    station_pressure_hpa: f64,
    regional_sea_level_pressure_hpa: Option<f64>,
    temperature_c: f64,
) -> PlausibilityCheck {
    let (Some(claimed), Some(sea_level)) = (location.elevation_m, regional_sea_level_pressure_hpa) else {
        return PlausibilityCheck::pass(CheckKind::LocationConsistency, 0.0);
    };
    if !(station_pressure_hpa > 0.0 && sea_level > 0.0 && temperature_c.is_finite()) {
        return PlausibilityCheck::pass(CheckKind::LocationConsistency, 0.0);
    }
    let temperature_k = temperature_c + 273.15;
    // h = (R·T/g)·ln(p0/p): 29.27 m/K is R/g for dry air.
    let implied = 29.27 * temperature_k * (sea_level / station_pressure_hpa).ln();
    let delta = (implied - claimed).abs();
    if delta > LOCATION_ELEVATION_TOLERANCE_M {
        let confidence = ((delta - LOCATION_ELEVATION_TOLERANCE_M) / 1_000.0 + 0.4).clamp(0.4, 1.0) as f32;
        return PlausibilityCheck::flag(
            CheckKind::LocationConsistency,
            format!(
                "claimed elevation {claimed:.0} m but the station's pressure implies {implied:.0} m \
                 against a regional sea-level pressure of {sea_level:.1} hPa"
            ),
            confidence,
        );
    }
    PlausibilityCheck::pass(CheckKind::LocationConsistency, 0.8)
}

/// What is known about a station's calibration. Three distinct states, and
/// only the first can raise a calibration-derived tier.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum CalibrationEvidence {
    /// A calibration artifact whose signature was cryptographically verified.
    Verified { calibrated_at: Timestamp, valid_for_secs: u64 },
    /// An artifact exists but its signature could not be verified (no signer
    /// key, no certificate chain). Never a pass.
    VerificationUnavailable { reason: String },
    Absent,
}

/// Calibration is a pass only when verified and current; expired,
/// unverifiable and absent calibration are flags of increasing weight.
pub fn calibration_currency(evidence: &CalibrationEvidence, now: Timestamp) -> PlausibilityCheck {
    match evidence {
        CalibrationEvidence::Verified { calibrated_at, valid_for_secs } => {
            let expires_at = calibrated_at.into_inner().saturating_add(*valid_for_secs);
            let now = now.into_inner();
            if now <= expires_at {
                PlausibilityCheck::pass(CheckKind::CalibrationCurrency, 1.0)
            } else {
                PlausibilityCheck::flag(
                    CheckKind::CalibrationCurrency,
                    format!("calibration expired {} s ago", now.saturating_sub(expires_at)),
                    0.6,
                )
            }
        },
        CalibrationEvidence::VerificationUnavailable { reason } => PlausibilityCheck::flag(
            CheckKind::CalibrationCurrency,
            format!("calibration verification unavailable ({reason}); unavailable is not a pass"),
            0.5,
        ),
        CalibrationEvidence::Absent => {
            PlausibilityCheck::flag(CheckKind::CalibrationCurrency, "no calibration attestation", 0.4)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u64 = 1_700_000_000;
    fn ts(offset: u64) -> Timestamp {
        Timestamp::new(BASE + offset)
    }

    fn diurnal_series(mean: f64, amplitude: f64, days: u64, step_secs: u64) -> Vec<(Timestamp, f64)> {
        (0..(days * 86_400 / step_secs))
            .map(|i| {
                let t = i * step_secs;
                let phase = std::f64::consts::TAU * t as f64 / 86_400.0 - 1.9;
                (ts(t), mean + amplitude * phase.sin() + 0.1 * ((i % 3) as f64 - 1.0))
            })
            .collect()
    }

    #[test]
    fn a_physically_impossible_reading_fails_and_is_the_only_way_to_reject() {
        let check = physical_range(Variable::TemperatureC, 71.0);
        assert!(check.outcome.is_fail(), "{check:?}");
        assert!(physical_range(Variable::HumidityPct, 100.0).outcome == CheckOutcome::Pass);
        assert!(physical_range(Variable::PressureHpa, f64::NAN).outcome.is_fail());
        let assessment = QualityAssessment::assess(vec![check, physical_range(Variable::PressureHpa, 1013.0)], ts(0));
        assert_eq!(assessment.tier, AssessmentTier::Rejected);
        assert_eq!(assessment.tier.proposed_reward_weight_bps(), 0);
        assert!(assessment.is_retained(), "rejected readings are retained with their verdict");
    }

    /// Pins the #3308 solver change (review of core4#3311, F1). Once-daily
    /// samples within a minute or two of the same time of day have an almost
    /// constant diurnal phase, so the cos/sin basis columns are nearly collinear
    /// with the constant term and the normal matrix is near-singular.
    /// - The determinant-relative threshold of the Cramer solver reports "no
    ///   evidence" (pass, confidence 0.0).
    /// - The old per-pivot elimination accepted these systems and fitted an
    ///   arbitrary (often absurd) amplitude, so it PASSED them at span-based
    ///   confidence 1.0. Measured on this series: old Pass/1.0, new Pass/0.0.
    /// - A flat but correctly flaggable series can land here too (the review
    ///   found 3 such lost flags). Flagging needs a real diurnal-coverage check,
    ///   and the semantics must be revisited before this crate gets a consumer
    ///   (#2207).
    #[test]
    fn near_same_time_daily_samples_are_no_evidence_under_the_determinant_threshold() {
        for amp in [0.05, 0.4, 5.0] {
            // 10 days at 13:00, each within 0-80 s of it.
            let series: Vec<(Timestamp, f64)> = (0..10u64)
                .map(|d| {
                    let at = d * 86_400 + 13 * 3_600 + (d * 7 % 5) * 20;
                    (ts(at), 20.0 + amp * ((d * 3 % 4) as f64 - 1.5))
                })
                .collect();
            let check = diurnal_pattern(&series);
            assert_eq!(
                (check.outcome.clone(), check.confidence),
                (CheckOutcome::Pass, 0.0),
                "amp {amp}: {check:?}"
            );
        }
        // Exactly the same second every day is exactly singular for both solvers.
        let exact: Vec<(Timestamp, f64)> = (0..10u64).map(|d| (ts(d * 86_400 + 13 * 3_600), 20.0)).collect();
        let check = diurnal_pattern(&exact);
        assert_eq!((check.outcome, check.confidence), (CheckOutcome::Pass, 0.0));
    }

    #[test]
    fn solve3_recovers_a_known_solution_and_refuses_a_singular_system() {
        // x = [2, -1, 0.5] under a non-symmetric, well-conditioned matrix.
        let a = [[4.0, 1.0, -2.0], [1.0, 3.0, 0.5], [-2.0, 0.5, 5.0]];
        let x = [2.0, -1.0, 0.5];
        let b = [
            a[0][0] * x[0] + a[0][1] * x[1] + a[0][2] * x[2],
            a[1][0] * x[0] + a[1][1] * x[1] + a[1][2] * x[2],
            a[2][0] * x[0] + a[2][1] * x[1] + a[2][2] * x[2],
        ];
        let got = solve3(a, b).expect("non-singular");
        for (g, want) in got.iter().zip(x.iter()) {
            assert!((g - want).abs() < 1e-12, "{got:?}");
        }
        // Two equal rows: singular. All zeros: no scale.
        assert_eq!(solve3([[1.0, 2.0, 3.0], [1.0, 2.0, 3.0], [0.0, 1.0, 1.0]], [1.0, 1.0, 1.0]), None);
        assert_eq!(solve3([[0.0; 3]; 3], [1.0, 2.0, 3.0]), None);
    }

    #[test]
    fn a_staircase_of_sub_threshold_steps_fails_on_its_net_change() {
        // Review probe (core4#3118): pressure falls 14.9 hPa every 10 s for 6
        // readings, 1013 -> 923.6 hPa in 60 s. Each consecutive pair is under
        // the impossible rate after the floor (59.6/h), which is why the stream
        // must be judged by `rate_over_window`, not pair by pair.
        let stair: Vec<(Timestamp, f64)> =
            (0..=6u64).map(|i| (ts(i * 10), 1013.0 - 14.9 * i as f64)).collect();
        for pair in stair.windows(2) {
            let step = rate_of_change(Variable::PressureHpa, pair[0].1, pair[1].1, 10);
            assert!(!step.outcome.is_fail(), "each step alone is sub-threshold: {step:?}");
        }
        let windowed = rate_over_window(Variable::PressureHpa, &stair);
        assert!(windowed.outcome.is_fail(), "{windowed:?}");
        assert_eq!(QualityAssessment::assess(vec![windowed], ts(60)).tier, AssessmentTier::Rejected);
        // The same net change as one pair fails the same way.
        assert!(rate_of_change(Variable::PressureHpa, 1013.0, 1013.0 - 14.9 * 6.0, 60).outcome.is_fail());
    }

    #[test]
    fn the_window_keeps_noise_non_fail_and_real_weather_passing() {
        // The four noise probes, as two-reading streams.
        for (variable, previous, next, secs) in [
            (Variable::TemperatureC, 20.0, 20.5, 10),
            (Variable::PressureHpa, 1013.0, 1013.2, 10),
            (Variable::TemperatureC, 20.0, 21.0, 30),
            (Variable::HumidityPct, 50.0, 52.0, 1),
        ] {
            let check = rate_over_window(variable, &[(ts(0), previous), (ts(secs), next)]);
            assert!(!check.outcome.is_fail(), "{variable:?} {previous}->{next} in {secs}s: {check:?}");
        }
        // Noise that wanders up and down every 10 s never accumulates.
        let jitter: Vec<(Timestamp, f64)> =
            (0..180u64).map(|i| (ts(i * 10), 1013.0 + if i % 2 == 0 { 0.0 } else { 0.2 })).collect();
        assert_eq!(rate_over_window(Variable::PressureHpa, &jitter).outcome, CheckOutcome::Pass);
        // A strong but real pressure fall (3 hPa/h) sampled every minute for 2 h.
        let front: Vec<(Timestamp, f64)> =
            (0..=120u64).map(|i| (ts(i * 60), 1013.0 - 0.05 * i as f64)).collect();
        assert_eq!(rate_over_window(Variable::PressureHpa, &front).outcome, CheckOutcome::Pass);
        // No earlier reading: nothing to compare, no evidence either way.
        let lone = rate_over_window(Variable::PressureHpa, &[(ts(0), 1013.0)]);
        assert_eq!((lone.outcome.clone(), lone.confidence), (CheckOutcome::Pass, 0.0));
        // A 19.5 °C jump in 20 s after a quiet 1000 s still fails.
        let late = rate_over_window(Variable::TemperatureC, &[(ts(0), 10.0), (ts(1_000), 10.5), (ts(1_020), 30.0)]);
        assert!(late.outcome.is_fail(), "{late:?}");
    }

    #[test]
    fn sensor_noise_seconds_apart_is_never_physically_impossible() {
        // Review probes (core4#3118): ordinary quantisation steps and gusts
        // between closely spaced readings. Scaled per hour over the raw
        // interval these read 72–180/h and failed; over the floor they pass.
        for (variable, previous, next, secs) in [
            (Variable::TemperatureC, 20.0, 20.5, 10),
            (Variable::PressureHpa, 1013.0, 1013.2, 10),
            (Variable::TemperatureC, 20.0, 21.0, 30),
            (Variable::HumidityPct, 50.0, 52.0, 1),
        ] {
            let check = rate_of_change(variable, previous, next, secs);
            assert!(!check.outcome.is_fail(), "{variable:?} {previous}->{next} in {secs}s: {check:?}");
            let assessment = QualityAssessment::assess(vec![check], ts(0));
            assert_ne!(assessment.tier, AssessmentTier::Rejected, "{variable:?} {previous}->{next} in {secs}s");
        }
        // A genuinely impossible step still fails however short the interval.
        assert!(rate_of_change(Variable::TemperatureC, 20.0, 40.0, 10).outcome.is_fail());
        // At or beyond the floor the raw interval is used unchanged.
        assert!(rate_of_change(Variable::TemperatureC, 0.0, 16.0, RATE_MIN_INTERVAL_SECS).outcome.is_fail());
    }

    #[test]
    fn an_extreme_but_possible_event_is_flagged_never_failed_and_retained() {
        // 47 °C in a 30 °C neighbourhood: a real heat dome or a bad sensor —
        // either way recorded, paid less, and re-scorable.
        let neighbours = [
            NeighbourReading { distance_km: 8.0, value: 30.5 },
            NeighbourReading { distance_km: 15.0, value: 29.8 },
            NeighbourReading { distance_km: 22.0, value: 31.0 },
        ];
        let range = physical_range(Variable::TemperatureC, 47.0);
        assert_eq!(range.outcome, CheckOutcome::Pass);
        let agreement = neighbour_agreement(Variable::TemperatureC, 47.0, &neighbours);
        assert!(agreement.outcome.is_flag(), "{agreement:?}");
        assert!(agreement.confidence >= 0.4);
        let jump = rate_of_change(Variable::TemperatureC, 31.0, 47.0, 3_600);
        assert!(jump.outcome.is_flag() && !jump.outcome.is_fail(), "{jump:?}");
        let assessment = QualityAssessment::assess(vec![range, agreement, jump], ts(0));
        assert_eq!(assessment.tier, AssessmentTier::Provisional);
        assert!(assessment.is_retained());
        assert!(assessment.tier.proposed_reward_weight_bps() > 0, "paid less, not nothing");
        // Only a physically impossible rate fails.
        assert!(rate_of_change(Variable::TemperatureC, 0.0, 70.0, 600).outcome.is_fail());
    }

    #[test]
    fn an_indoor_sensor_is_detected_by_its_missing_diurnal_cycle() {
        let outdoors = diurnal_pattern(&diurnal_series(20.0, 6.0, 3, 1_800));
        assert_eq!(outdoors.outcome, CheckOutcome::Pass, "{outdoors:?}");
        let indoors = diurnal_pattern(&diurnal_series(21.0, 0.2, 3, 1_800));
        assert!(indoors.outcome.is_flag(), "{indoors:?}");
        assert!(indoors.confidence >= 0.9, "three days of flat readings is a confident verdict");
        let too_short = diurnal_pattern(&diurnal_series(21.0, 0.2, 1, 7_200)[..6]);
        assert_eq!((too_short.outcome, too_short.confidence), (CheckOutcome::Pass, 0.0), "no verdict without data");
        let assessment = QualityAssessment::assess(vec![indoors], ts(0));
        assert_eq!(assessment.tier, AssessmentTier::Reduced);
    }

    #[test]
    fn neighbour_disagreement_lowers_the_tier_and_a_lone_station_is_not_penalised() {
        let alone = neighbour_agreement(Variable::PressureHpa, 1013.0, &[NeighbourReading { distance_km: 10.0, value: 1000.0 }]);
        assert_eq!((alone.outcome, alone.confidence), (CheckOutcome::Pass, 0.0));
        let far = neighbour_agreement(
            Variable::PressureHpa,
            1013.0,
            &[NeighbourReading { distance_km: 300.0, value: 990.0 }, NeighbourReading { distance_km: 400.0, value: 991.0 }],
        );
        assert_eq!(far.confidence, 0.0, "out-of-range neighbours are ignored");
        let agree = neighbour_agreement(
            Variable::PressureHpa,
            1013.0,
            &[NeighbourReading { distance_km: 5.0, value: 1012.4 }, NeighbourReading { distance_km: 20.0, value: 1013.9 }],
        );
        assert_eq!(agree.outcome, CheckOutcome::Pass);
        let disagree = neighbour_agreement(
            Variable::PressureHpa,
            1013.0,
            &[NeighbourReading { distance_km: 5.0, value: 998.0 }, NeighbourReading { distance_km: 20.0, value: 997.0 }],
        );
        assert!(disagree.outcome.is_flag(), "{disagree:?}");
        assert_eq!(QualityAssessment::assess(vec![disagree], ts(0)).tier, AssessmentTier::Provisional);
    }

    #[test]
    fn calibration_has_three_outcomes_and_unavailable_never_passes() {
        let verified = CalibrationEvidence::Verified { calibrated_at: ts(0), valid_for_secs: 365 * 86_400 };
        assert_eq!(calibration_currency(&verified, ts(86_400)).outcome, CheckOutcome::Pass);
        let expired = calibration_currency(&verified, ts(400 * 86_400));
        assert!(expired.outcome.is_flag(), "{expired:?}");
        let unavailable = calibration_currency(
            &CalibrationEvidence::VerificationUnavailable { reason: "CAL_SIGNATURE_VERIFICATION_UNAVAILABLE".into() },
            ts(0),
        );
        assert!(unavailable.outcome.is_flag(), "{unavailable:?}");
        assert!(matches!(&unavailable.outcome, CheckOutcome::Flag { reason } if reason.contains("not a pass")));
        let absent = calibration_currency(&CalibrationEvidence::Absent, ts(0));
        assert!(absent.outcome.is_flag());
        assert_eq!(QualityAssessment::assess(vec![expired], ts(0)).tier, AssessmentTier::Reduced);
        assert_eq!(QualityAssessment::assess(vec![unavailable], ts(0)).tier, AssessmentTier::Reduced);
    }

    #[test]
    fn a_claimed_elevation_the_pressure_contradicts_is_flagged() {
        // ~1,500 m station pressure with a sea-level claim.
        let claimed_sea_level = CoarseLocation::from_precise(46.78671, -92.10053, Some(3.0));
        assert_eq!((claimed_sea_level.latitude, claimed_sea_level.longitude), (46.8, -92.1), "coarsened");
        let check = location_consistency(&claimed_sea_level, 845.0, Some(1013.0), 15.0);
        assert!(check.outcome.is_flag(), "{check:?}");
        let honest = CoarseLocation::from_precise(46.78671, -92.10053, Some(1_520.0));
        assert_eq!(location_consistency(&honest, 845.0, Some(1013.0), 15.0).outcome, CheckOutcome::Pass);
        let unknown = location_consistency(&CoarseLocation::from_precise(46.8, -92.1, None), 845.0, Some(1013.0), 15.0);
        assert_eq!((unknown.outcome, unknown.confidence), (CheckOutcome::Pass, 0.0));
        let a = CoarseLocation::from_precise(46.8, -92.1, None);
        let b = CoarseLocation::from_precise(46.9, -92.1, None);
        assert!((a.distance_km(&b) - 11.1).abs() < 0.3, "{}", a.distance_km(&b));
    }

    #[test]
    fn assessments_are_stored_and_rescorable_without_losing_history() {
        let first = QualityAssessment::assess(
            vec![
                physical_range(Variable::TemperatureC, 12.0),
                calibration_currency(&CalibrationEvidence::Absent, ts(0)),
            ],
            ts(0),
        );
        assert_eq!(first.tier, AssessmentTier::Reduced);
        let json = serde_json::to_string(&first).unwrap();
        let stored: QualityAssessment = serde_json::from_str(&json).unwrap();
        assert_eq!(stored, first);
        // Later, the calibration artifact verifies: the calibration check is
        // replaced, the earlier verdict is kept on record, the tier rises.
        let verified = CalibrationEvidence::Verified { calibrated_at: ts(0), valid_for_secs: 86_400 * 30 };
        let rescored = stored.rescore(vec![calibration_currency(&verified, ts(3_600))], ts(3_600));
        assert_eq!(rescored.tier, AssessmentTier::Full);
        assert_eq!(rescored.checks.len(), 2);
        assert_eq!(rescored.previous_tiers, vec![(ts(0), AssessmentTier::Reduced)]);
        assert_eq!(rescored.assessor_version, ASSESSOR_VERSION);
        // Low-confidence flags are recorded but do not move the tier.
        let noted = QualityAssessment::assess(vec![rate_of_change(Variable::HumidityPct, 50.0, 51.0, 0)], ts(0));
        assert_eq!(noted.tier, AssessmentTier::Full);
        assert_eq!(noted.confident_flags().count(), 0);
        assert_eq!(noted.checks.len(), 1);
    }

    #[test]
    fn tiers_order_reward_weight_and_quality_ceiling() {
        let rank = |t: DataQualityTier| match t {
            DataQualityTier::Unverified => 0,
            DataQualityTier::Bronze => 1,
            DataQualityTier::Silver => 2,
            DataQualityTier::Gold => 3,
        };
        let tiers = [AssessmentTier::Rejected, AssessmentTier::Provisional, AssessmentTier::Reduced, AssessmentTier::Full];
        for pair in tiers.windows(2) {
            assert!(pair[0] < pair[1]);
            assert!(pair[0].proposed_reward_weight_bps() < pair[1].proposed_reward_weight_bps());
            assert!(rank(pair[0].quality_tier_ceiling()) < rank(pair[1].quality_tier_ceiling()));
        }
    }
}
