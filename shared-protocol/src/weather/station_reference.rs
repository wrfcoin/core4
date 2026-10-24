//! Nearby-station reference pairing — the attester's reference source.
//!
//! The owner's ruling on the #2746 reference-scope question (see
//! `docs/plans/2026-08-06-attestation-reference-scope.md`): readings are
//! compared against **other local sensors**, using a reference station
//! selected by the collector's accuracy pipeline
//! (`tests/security/fetch-ambient-weather.cjs` /
//! `ambient-weather-comparison.cjs`). This module is the consensus-adjacent
//! half of that ruling: given NWS-style reference observations that the
//! attester fetched, pair a revealed reading with the nearest-in-time
//! reference and express the differences as [`ReferenceResidual`]s under the
//! `nws_station` source, which the existing #2054 band/tier machinery
//! ([`ReferenceComparison::tier`]) then consumes unchanged.
//!
//! # Pairing window vs the collector's
//!
//! The collector pairs within 300 s because for it an unpaired reading is
//! *silently* untiered ("no gating without reference data"). For the attester
//! an unpaired reading is a **visibly capped** attestation
//! ([`MechanicalPolicy`]'s unchecked-confidence cap), so a window narrower
//! than the reference station's reporting cadence would cap almost every
//! attestation: KDLH files METARs roughly hourly, while the PWS reports every
//! 5 minutes. The default window is therefore
//! [`DEFAULT_MAX_PAIR_DELTA_SECS`] (30 minutes — half the METAR cadence, so a
//! reachable NWS always yields a pair), and the `nws_station` bands in
//! `fixtures/reference_bands.json` are wider than the collector's
//! instantaneous thresholds to absorb up to 30 minutes of legitimate weather
//! drift plus the ~3 km site separation.
//!
//! # What is deliberately NOT compared
//!
//! - `rainfall_mm`: NWS hourly precipitation is frequently null and
//!   accumulation windows differ; no `nws_station` band exists for it, so
//!   even a supplied residual would be skipped.
//! - Pressure caveat: the reference pressure is NWS `barometricPressure`,
//!   which is the station's **altimeter setting** (QNH, reduced to sea level
//!   by the standard atmosphere), not station pressure. The chain stores the
//!   *submitted* `pressure_hpa`. For a station whose Ambient relative
//!   pressure carries an owner-set sea-level offset, that value is already
//!   comparable. A station that submits raw absolute pressure at elevation
//!   (the reference station’s WS-5000: ~1001 hPa at 227.7 m) reads about −27 hPa on every
//!   reading under the raw pairing, a persistent pressure outlier. That
//!   pairing is what [`pair_station_reference`] does, and it is what the
//!   attester does until [`ConsensusFeature::ReferenceSeaLevelPressure`]
//!   activates (#3285). From then, [`pair_station_reference_sea_level`] pairs
//!   the reveal's extra `pressure_sea_level_hpa` key instead, but only when
//!   the reveal's `pressure_kind` is `"station"` and the value recomputes
//!   with [`altimeter_setting_hpa`] from the raw `pressure_hpa` and the
//!   station's **registered** elevation within
//!   [`SEA_LEVEL_PRESSURE_TOLERANCE_HPA`], and only when that registered
//!   elevation lies inside the band [`DEM_BAND_BELOW_M`] /
//!   [`DEM_BAND_ABOVE_M`] around a trusted DEM ground elevation for the
//!   station (#3299): otherwise a false elevation would launder a constant
//!   barometer bias into a Verified claim. A `"relative"` `pressure_hpa` is
//!   already sea-level and is never reduced again: it pairs raw. For a
//!   station WITH a registered elevation, anything else (no claim, no known
//!   kind, a claim that does not recompute) gets no raw pressure residual
//!   (owner ruling A1), and a published, checkable `pressure_hpa` is scored
//!   as an unverified `Outlier` in the tier fold
//!   ([`PressurePairing::Unverified`]), so withholding or corrupting the claim
//!   never tiers better than an honest one. A station without a registered
//!   elevation is paired raw exactly as before. Either way a station cannot
//!   pass by asserting a number.
//!
//! [`ConsensusFeature::ReferenceSeaLevelPressure`]: crate::crypto::chain_binding::ConsensusFeature::ReferenceSeaLevelPressure
//! [`MechanicalPolicy`]: crate::oracle::mechanical::MechanicalPolicy

use serde::{Deserialize, Serialize};

use super::reference::{ReferenceComparison, ReferenceResidual};
use wrfcoin_meteo_types::WeatherObservation;

/// The band-fixture source name for nearby-station references. Present in
/// `fixtures/reference_bands.json`; the fixture addition is additive (no
/// existing threshold changed), so the bands version stays 1 — the backend's
/// vendored fixture copies remain parity-valid until synced.
pub const NWS_STATION_SOURCE: &str = "nws_station";

/// Default maximum |Δt| between a reading and its paired reference
/// observation. See the module docs for why this is 30 minutes and not the
/// collector's 300 s.
pub const DEFAULT_MAX_PAIR_DELTA_SECS: i64 = 1_800;

/// One reference observation from a nearby station, normalized to canonical
/// units. Mirrors the collector's `fetchNwsObservations` output shape
/// (`timestamp_ms` becomes unix seconds; Pa → hPa; km/h → m/s are the
/// fetcher's job, not this module's).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StationReferenceObservation {
    /// Observation time, unix seconds.
    pub timestamp: i64,
    // determinism-ok: attester-side reference data — no consensus path constructs or reads this type (grep: only the oracle_attester bin and tests); it influences only the confidence the attester SIGNS, and consensus receives those bytes verbatim
    pub temperature_c: Option<f64>,
    // determinism-ok: as temperature_c above — attester-side reference input, never consensus-executed
    pub pressure_hpa: Option<f64>,
    // determinism-ok: as temperature_c above — attester-side reference input, never consensus-executed
    pub humidity_pct: Option<f64>,
    // determinism-ok: as temperature_c above — attester-side reference input, never consensus-executed
    pub wind_speed_ms: Option<f64>,
}

/// Pair `reading` (revealed at `reading_timestamp`, unix seconds) with the
/// nearest-in-time reference observation and build the residual set.
///
/// Returns `None` — meaning "no reference comparison happened", which the
/// mechanical policy renders as a visibly capped attestation — when:
/// - `references` is empty (fetch failed or no station configured upstream),
/// - no reference observation lies within `max_pair_delta_secs`,
/// - the paired observation shares no populated variable with the reading.
///
/// Otherwise returns a comparison with one residual (observed − reference)
/// per variable populated on **both** sides, under [`NWS_STATION_SOURCE`].
/// Nearest-neighbor selection and the both-sides-present rule mirror the
/// collector's `computeReadingQualityTier` pairing exactly.
pub fn pair_station_reference(
    reading: &WeatherObservation,
    reading_timestamp: i64,
    references: &[StationReferenceObservation],
    max_pair_delta_secs: i64,
) -> Option<ReferenceComparison> {
    let nearest = references
        .iter()
        .min_by_key(|obs| (obs.timestamp - reading_timestamp).abs())?;
    if (nearest.timestamp - reading_timestamp).abs() > max_pair_delta_secs {
        return None;
    }

    let mut residuals = Vec::new();
    // determinism-ok: attester-side residual arithmetic (observed − reference); runs only in the attester process, never in block production/application/replay, and reaches the chain solely inside the signed attestation whose bytes every node parses identically
    let mut push = |variable: &str, observed: Option<f64>, reference: Option<f64>| {
        if let (Some(observed), Some(reference)) = (observed, reference) {
            residuals.push(ReferenceResidual {
                source: NWS_STATION_SOURCE.to_string(),
                variable: variable.to_string(),
                residual: observed - reference,
            });
        }
    };
    push("temperature_c", reading.temperature_c, nearest.temperature_c);
    push("pressure_hpa", reading.pressure_hpa, nearest.pressure_hpa);
    push("humidity_pct", reading.humidity_percent, nearest.humidity_pct);
    push("wind_speed_ms", reading.wind_speed_ms, nearest.wind_speed_ms);

    if residuals.is_empty() {
        return None;
    }
    Some(ReferenceComparison { residuals })
}

/// The reveal `data` key carrying a station's sea-level pressure (#3285).
/// Additive: it is not a [`WeatherObservation`] field (that struct is part of
/// the bincode checkpoint layout), so it is read from the raw reveal JSON.
pub const SEA_LEVEL_PRESSURE_KEY: &str = "pressure_sea_level_hpa";

/// Maximum |submitted − recomputed| for a submitted sea-level pressure to be
/// paired (#3285). Twice the rounding a collector can legitimately introduce
/// is far below this; an elevation wrong by more than ~8 m is not.
// determinism-ok: attester-side tolerance constant (exactly representable); consensus stores the signed confidence
pub const SEA_LEVEL_PRESSURE_TOLERANCE_HPA: f64 = 1.0;

/// How far a registered elevation may sit BELOW the station's DEM ground
/// elevation and still be trusted for the sea-level recompute (#3299 owner
/// ruling, 2026-10-24). Honest negative offsets exist: the reference
/// station's barometer-confirmed 227.7 m is 20.2 m below its USGS 3DEP DEM
/// value (247.9 m).
// determinism-ok: attester-side band constant (exactly representable); consensus stores the signed confidence
pub const DEM_BAND_BELOW_M: f64 = 50.0;

/// How far a registered elevation may sit ABOVE the DEM ground elevation
/// (#3299): the asymmetry allows for sensor height above ground.
// determinism-ok: attester-side band constant (exactly representable); consensus stores the signed confidence
pub const DEM_BAND_ABOVE_M: f64 = 60.0;

/// Whether `registered_m` is inside `[dem_m − DEM_BAND_BELOW_M, dem_m +
/// DEM_BAND_ABOVE_M]`, edges included (#3299). Non-finite input is outside.
// determinism-ok: attester-side #3299 band check; consensus stores the signed confidence, never these values
pub fn elevation_within_dem_band(registered_m: f64, dem_m: f64) -> bool {
    registered_m.is_finite()
        && dem_m.is_finite()
        && registered_m >= dem_m - DEM_BAND_BELOW_M
        && registered_m <= dem_m + DEM_BAND_ABOVE_M
}

/// The NWS altimeter setting (hPa) for a station pressure `station_hpa`
/// (hPa) measured at `elevation_m` metres:
///
/// `(p − 0.3) · (1 + (1013.25^n · 0.0065 / 288) · h / (p − 0.3)^n)^(1/n)`,
/// `n = 0.190284`.
///
/// This is the reduction behind NWS `barometricPressure` (the METAR
/// altimeter setting), which is the attester's reference value, so the
/// station side and the reference side reduce to sea level the same way. The
/// 0.3 hPa is the formula's own station-pressure offset. Golden vectors in
/// `fixtures/altimeter_setting_vectors.json` pin this function and the
/// collectors' JS port to each other.
///
/// Returns `None` for non-finite inputs or a station pressure too low to
/// reduce (≤ 0.3 hPa).
// determinism-ok: attester-side only (the #3285 recompute check and the collector mirror); consensus stores the signed confidence, never this value
pub fn altimeter_setting_hpa(station_hpa: f64, elevation_m: f64) -> Option<f64> {
    // determinism-ok: as above — attester-side f64 reduction, never consensus-executed
    const N: f64 = 0.190284;
    // determinism-ok: attester-side #3285 recompute input; consensus stores the signed confidence, never this value
    if !station_hpa.is_finite() || !elevation_m.is_finite() || station_hpa <= 0.3 {
        return None;
    }
    // determinism-ok: as above
    let p = station_hpa - 0.3;
    // determinism-ok: as above
    let k = 1013.25_f64.powf(N) * 0.0065 / 288.0;
    // determinism-ok: as above
    let base = 1.0 + k * (elevation_m / p.powf(N));
    // determinism-ok: attester-side #3285 recompute input; consensus stores the signed confidence, never this value
    if base <= 0.0 {
        return None;
    }
    // determinism-ok: as above
    let altimeter = p * base.powf(1.0 / N);
    altimeter.is_finite().then_some(altimeter)
}

/// The reveal `data` key declaring what kind of value `pressure_hpa` is
/// (#3285): [`PressureKind::Station`] or [`PressureKind::Relative`]. The
/// attester may recompute a sea-level value from elevation only for station
/// pressure; reducing an already sea-level (relative) value again would be
/// wrong by the station's whole elevation.
pub const PRESSURE_KIND_KEY: &str = "pressure_kind";

/// What the submitted `pressure_hpa` is (#3285).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureKind {
    /// Absolute pressure at the sensor (`"station"`), e.g. the reference station’s WS-5000
    /// with no Ambient offset. Reducible with the registered elevation.
    Station,
    /// Already reduced to sea level (`"relative"`), e.g. an Ambient relative
    /// pressure with an owner-set offset. Never reduced again.
    Relative,
}

impl PressureKind {
    /// The wire string the collector writes under [`PRESSURE_KIND_KEY`].
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Station => "station",
            Self::Relative => "relative",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "station" => Some(Self::Station),
            "relative" => Some(Self::Relative),
            _ => None,
        }
    }
}

/// The sea-level pressure claim a reveal carries (#3285), read from the raw
/// reveal JSON. Both fields are strict: a non-finite or non-number
/// `pressure_sea_level_hpa`, or an unknown `pressure_kind`, reads as absent —
/// never as a zero or a guessed kind.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SeaLevelPressureClaim {
    // determinism-ok: attester-side read of the raw reveal JSON; consensus hashes these bytes, it never reads this value
    pub sea_level_hpa: Option<f64>,
    pub kind: Option<PressureKind>,
}

impl SeaLevelPressureClaim {
    /// Read the claim from a reveal's `data` object.
    pub fn from_reveal(data: &serde_json::Value) -> Self {
        Self {
            sea_level_hpa: data
                .get(SEA_LEVEL_PRESSURE_KEY)
                .and_then(serde_json::Value::as_f64)
                .filter(|v| v.is_finite()),
            kind: data
                .get(PRESSURE_KIND_KEY)
                .and_then(serde_json::Value::as_str)
                .and_then(PressureKind::parse),
        }
    }
}

/// How the pressure variable was paired for one reading (#3285). Only
/// [`SeaLevelPressureOutcome::Verified`] changes the pairing; every other
/// outcome pairs the raw `pressure_hpa` exactly as [`pair_station_reference`].
#[derive(Debug, Clone, PartialEq)]
pub enum SeaLevelPressureOutcome {
    /// The feature is not active: today's raw pairing, the claim unread.
    Inactive,
    /// The reveal carries no `pressure_sea_level_hpa`.
    NotClaimed,
    /// The reveal carries a claim but no (or an unknown) `pressure_kind`, so
    /// the attester cannot tell whether a recompute from elevation is valid.
    /// Not trusted.
    NoPressureKind,
    /// The reveal carries no raw `pressure_hpa` to recompute from.
    NoRawPressure,
    /// `pressure_kind` is `relative`: `pressure_hpa` is already sea-level
    /// (an Ambient relative pressure with an owner-set offset) and the claim
    /// repeats it within tolerance. No recompute; the raw pairing pairs that
    /// same number.
    RelativeRaw,
    /// The station has no registered elevation, so the claim cannot be
    /// recomputed and is not trusted.
    NoElevation,
    /// The station has a registered elevation but the attester holds no
    /// trusted DEM ground elevation for it (#3299), so the elevation cannot
    /// be bounded and the claim is not trusted.
    NoDemElevation,
    /// The registered elevation is outside the DEM band (#3299): the
    /// recompute would trust an elevation the terrain contradicts.
    // determinism-ok: attester-side audit values, never consensus-executed
    ElevationOutsideDemBand { registered_m: f64, dem_m: f64 },
    /// The raw pressure and elevation do not reduce (non-finite input).
    Unreducible,
    /// The claim recomputes within tolerance; the sea-level value is paired.
    // determinism-ok: attester-side audit values, never consensus-executed
    Verified { claimed_hpa: f64, expected_hpa: f64 },
    /// The claim is more than [`SEA_LEVEL_PRESSURE_TOLERANCE_HPA`] from the
    /// recompute: raw pairing, and the mismatch is recorded.
    // determinism-ok: attester-side audit values, never consensus-executed
    Mismatch { claimed_hpa: f64, expected_hpa: f64 },
}

impl SeaLevelPressureOutcome {
    /// Why this outcome is worth an operator's attention, or `None` when it
    /// is routine (inactive, verified, a consistent relative claim, or no
    /// claim from a station with no registered elevation).
    fn concern(&self) -> Option<String> {
        match self {
            Self::Mismatch { claimed_hpa, expected_hpa } => Some(format!(
                "sea-level pressure mismatch: submitted {claimed_hpa:.2} hPa, expected \
                 {expected_hpa:.2} hPa (station: recomputed from pressure_hpa and the \
                 registered elevation; relative: pressure_hpa itself) \
                 (tolerance {SEA_LEVEL_PRESSURE_TOLERANCE_HPA} hPa)"
            )),
            Self::NoPressureKind => {
                Some("sea-level pressure not checked: reveal has no known pressure_kind".to_string())
            },
            Self::NoElevation => Some(
                "sea-level pressure not checked: station has no registered elevation".to_string(),
            ),
            Self::NoDemElevation => Some(
                "sea-level pressure not checked: no trusted DEM ground elevation for the station \
                 (#3299)"
                    .to_string(),
            ),
            Self::ElevationOutsideDemBand { registered_m, dem_m } => Some(format!(
                "sea-level pressure not trusted: registered elevation {registered_m:.1} m is outside \
                 the DEM band {:.1}..{:.1} m (DEM ground {dem_m:.1} m, -{DEM_BAND_BELOW_M}/+\
                 {DEM_BAND_ABOVE_M} m, #3299)",
                dem_m - DEM_BAND_BELOW_M,
                dem_m + DEM_BAND_ABOVE_M
            )),
            Self::Unreducible => Some(
                "sea-level pressure not checked: pressure_hpa/elevation do not reduce".to_string(),
            ),
            Self::NotClaimed => Some("no sea-level pressure claim in the reveal".to_string()),
            _ => None,
        }
    }
}

/// What the pressure variable is compared as, once the outcome is known
/// (#3285, owner ruling A1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressurePairing {
    /// The raw `pressure_hpa`, exactly as [`pair_station_reference`].
    Raw,
    /// The verified `pressure_sea_level_hpa`.
    SeaLevel,
    /// No pressure residual at all, because nothing could be checked: the
    /// reveal has no `pressure_hpa`, or the paired reference observation
    /// carries no pressure. The other variables are scored normally.
    Skipped,
    /// The reveal publishes a `pressure_hpa` the reference could have checked,
    /// but gave no trustworthy sea-level value for it (no claim, no known
    /// kind, a mismatched claim, or an unreducible input) from a station with
    /// a registered elevation. Pressure carries no residual, and the tier fold
    /// scores it as an `Outlier` variable ("unverified",
    /// [`ReferenceComparison::tier_under_with_unverified`]). Owner ruling
    /// (#3285, replacing a Degraded floor): removing the variable would hand
    /// the #3098 worst-variable drop to another one, so skipping could beat an
    /// honest claim; scoring it Outlier (the worst an honest claim can do)
    /// makes skipping never better than honesty.
    Unverified,
}

impl PressurePairing {
    /// How many variables the tier fold must score as unverified `Outlier`.
    pub const fn unverified_variables(self) -> usize {
        match self {
            Self::Unverified => 1,
            _ => 0,
        }
    }
}

/// The #3285 outcome plus the pairing it produced.
#[derive(Debug, Clone, PartialEq)]
pub struct SeaLevelPressureDecision {
    pub outcome: SeaLevelPressureOutcome,
    pub pairing: PressurePairing,
}

impl SeaLevelPressureDecision {
    /// Owner ruling A1 (2026-09-24). While the feature is active, a station
    /// with a registered elevation is compared on pressure only through a
    /// trustworthy sea-level value: a verified `station` claim, or a
    /// `relative` claim that repeats `pressure_hpa` (already sea-level, the
    /// same number). Anything else — no claim, no known `pressure_kind`, a
    /// mismatch, or an unreducible input — gets NO pressure residual rather
    /// than a raw comparison; when the reveal publishes a `pressure_hpa` the
    /// reference could have checked, the pairing then becomes
    /// [`PressurePairing::Unverified`] and the fold scores pressure `Outlier`
    /// (owner ruling replacing the Degraded floor). Raw station pressure against the sea-level
    /// reference is known to be wrong by the station's elevation, and a claim
    /// that fails its own recompute is not trustworthy either; falling back
    /// to raw on a mismatch would re-open the same two-comparison choice the
    /// ruling closes. A station with no registered elevation keeps today's
    /// raw pairing (there is nothing to recompute against). Inactive: raw.
    fn decide(outcome: SeaLevelPressureOutcome, elevation_registered: bool) -> Self {
        let pairing = match &outcome {
            SeaLevelPressureOutcome::Inactive
            | SeaLevelPressureOutcome::RelativeRaw
            | SeaLevelPressureOutcome::NoElevation => PressurePairing::Raw,
            SeaLevelPressureOutcome::Verified { .. } => PressurePairing::SeaLevel,
            SeaLevelPressureOutcome::NotClaimed
            | SeaLevelPressureOutcome::NoPressureKind
            | SeaLevelPressureOutcome::NoRawPressure
            | SeaLevelPressureOutcome::Unreducible
            | SeaLevelPressureOutcome::NoDemElevation
            | SeaLevelPressureOutcome::ElevationOutsideDemBand { .. }
            | SeaLevelPressureOutcome::Mismatch { .. } => {
                if elevation_registered {
                    PressurePairing::Skipped
                } else {
                    PressurePairing::Raw
                }
            },
        };
        Self { outcome, pairing }
    }

    /// An audit line for the verdict's `reasons`: why pressure was skipped,
    /// or why a claim could not be used and raw pressure was paired.
    pub fn audit_reason(&self) -> Option<String> {
        match self.pairing {
            PressurePairing::SeaLevel => None,
            PressurePairing::Skipped => Some(format!(
                "pressure not compared (#3285: registered elevation, no trustworthy \
                 sea-level value, nothing to check): {}",
                self.outcome.concern().unwrap_or_else(|| format!("{:?}", self.outcome))
            )),
            PressurePairing::Unverified => Some(format!(
                "pressure scored Outlier as unverified (#3285: registered elevation, no \
                 trustworthy sea-level value for a published pressure_hpa): {}",
                self.outcome.concern().unwrap_or_else(|| format!("{:?}", self.outcome))
            )),
            PressurePairing::Raw => match self.outcome {
                // No claim from a station with no elevation is today's
                // behaviour, not a concern.
                SeaLevelPressureOutcome::NotClaimed => None,
                _ => self.outcome.concern().map(|c| format!("{c}; paired raw pressure_hpa")),
            },
        }
    }
}

/// [`pair_station_reference`] with the #3285 sea-level pressure rule.
///
/// With `sea_level_active == false` this *is* [`pair_station_reference`]
/// (outcome [`SeaLevelPressureOutcome::Inactive`]): the pre-activation
/// pairing, byte-for-byte. When active, the pressure variable pairs the
/// claim's sea-level value with the reference only if the claim declares
/// `pressure_kind: "station"`, the station's registered `elevation_m` lies in
/// the band around the attester's trusted `dem_ground_m` (#3299,
/// [`elevation_within_dem_band`]), and the value recomputes from the
/// reading's raw `pressure_hpa` and that elevation within
/// [`SEA_LEVEL_PRESSURE_TOLERANCE_HPA`]. A `relative` claim is never
/// reduced again: `pressure_hpa` is already sea-level and is paired raw.
/// Otherwise, for a station with a registered elevation, pressure is
/// **skipped** (owner ruling A1, see [`SeaLevelPressureDecision`]); for a
/// station without one, the raw pairing is kept. The decision says which and
/// why. The other variables are paired identically either way.
// determinism-ok: attester-side pairing; its f64 inputs never reach consensus except through the signed confidence
pub fn pair_station_reference_sea_level(
    reading: &WeatherObservation,
    claim: SeaLevelPressureClaim,
    // determinism-ok: attester-side #3285 recompute input; consensus stores the signed confidence, never this value
    elevation_m: Option<f64>,
    // determinism-ok: attester-side #3299 band input (trusted DEM ground elevation); consensus stores the signed confidence, never this value
    dem_ground_m: Option<f64>,
    sea_level_active: bool,
    reading_timestamp: i64,
    references: &[StationReferenceObservation],
    max_pair_delta_secs: i64,
) -> (Option<ReferenceComparison>, SeaLevelPressureDecision) {
    if !sea_level_active {
        // Pre-activation: literally today's pairing.
        return (
            pair_station_reference(reading, reading_timestamp, references, max_pair_delta_secs),
            SeaLevelPressureDecision {
                outcome: SeaLevelPressureOutcome::Inactive,
                pairing: PressurePairing::Raw,
            },
        );
    }
    // determinism-ok: attester-side #3285 presence check on the registered elevation; consensus stores the signed confidence, never this value
    let elevation_registered = elevation_m.is_some_and(f64::is_finite);
    let mut decision = SeaLevelPressureDecision::decide(
        sea_level_pressure_outcome(reading.pressure_hpa, claim, elevation_m, dem_ground_m),
        elevation_registered,
    );
    if decision.pairing == PressurePairing::Skipped && reading.pressure_hpa.is_some() {
        // Would an honest claim have been scored? Exactly when the raw pairing
        // produces a pressure residual (the nearest reference in the window
        // carries pressure). Then the published value is unverified, not
        // merely unchecked.
        let raw = pair_station_reference(reading, reading_timestamp, references, max_pair_delta_secs);
        if raw.is_some_and(|c| c.residuals.iter().any(|r| r.variable == "pressure_hpa")) {
            decision.pairing = PressurePairing::Unverified;
        }
    }
    let comparison = match (&decision.pairing, &decision.outcome) {
        (PressurePairing::SeaLevel, SeaLevelPressureOutcome::Verified { claimed_hpa, .. }) => {
            let mut sea_level_reading = reading.clone();
            sea_level_reading.pressure_hpa = Some(*claimed_hpa);
            pair_station_reference(
                &sea_level_reading,
                reading_timestamp,
                references,
                max_pair_delta_secs,
            )
        },
        (PressurePairing::Skipped | PressurePairing::Unverified, _) => {
            let mut without_pressure = reading.clone();
            without_pressure.pressure_hpa = None;
            pair_station_reference(
                &without_pressure,
                reading_timestamp,
                references,
                max_pair_delta_secs,
            )
        },
        _ => pair_station_reference(reading, reading_timestamp, references, max_pair_delta_secs),
    };
    (comparison, decision)
}

// determinism-ok: attester-side recompute check (#3285), never consensus-executed
fn sea_level_pressure_outcome(
    // determinism-ok: attester-side #3285 recompute input; consensus stores the signed confidence, never this value
    raw_hpa: Option<f64>,
    claim: SeaLevelPressureClaim,
    // determinism-ok: attester-side #3285 recompute input; consensus stores the signed confidence, never this value
    elevation_m: Option<f64>,
    // determinism-ok: attester-side #3299 band input; consensus stores the signed confidence, never this value
    dem_ground_m: Option<f64>,
) -> SeaLevelPressureOutcome {
    let Some(claimed_hpa) = claim.sea_level_hpa.filter(|v| v.is_finite()) else {
        return SeaLevelPressureOutcome::NotClaimed;
    };
    let Some(kind) = claim.kind else {
        return SeaLevelPressureOutcome::NoPressureKind;
    };
    let Some(raw_hpa) = raw_hpa.filter(|v| v.is_finite()) else {
        return SeaLevelPressureOutcome::NoRawPressure;
    };
    if kind == PressureKind::Relative {
        // Already sea-level: never reduce it again. The claim must simply
        // repeat pressure_hpa; the raw pairing pairs that same number.
        return if (claimed_hpa - raw_hpa).abs() <= SEA_LEVEL_PRESSURE_TOLERANCE_HPA {
            SeaLevelPressureOutcome::RelativeRaw
        } else {
            SeaLevelPressureOutcome::Mismatch { claimed_hpa, expected_hpa: raw_hpa }
        };
    }
    let Some(elevation_m) = elevation_m.filter(|v| v.is_finite()) else {
        return SeaLevelPressureOutcome::NoElevation;
    };
    let Some(dem_m) = dem_ground_m.filter(|v| v.is_finite()) else {
        return SeaLevelPressureOutcome::NoDemElevation;
    };
    if !elevation_within_dem_band(elevation_m, dem_m) {
        return SeaLevelPressureOutcome::ElevationOutsideDemBand { registered_m: elevation_m, dem_m };
    }
    let Some(expected_hpa) = altimeter_setting_hpa(raw_hpa, elevation_m) else {
        return SeaLevelPressureOutcome::Unreducible;
    };
    if (claimed_hpa - expected_hpa).abs() <= SEA_LEVEL_PRESSURE_TOLERANCE_HPA {
        SeaLevelPressureOutcome::Verified { claimed_hpa, expected_hpa }
    } else {
        SeaLevelPressureOutcome::Mismatch { claimed_hpa, expected_hpa }
    }
}

/// Great-circle distance in kilometres from a registered station location to
/// a reference point `(latitude, longitude)` in degrees (#3240).
///
/// This is the radius check behind the attester's per-station reference
/// selection (`WRFCOIN_ATTESTER_MAX_REFERENCE_KM`): the distance is computed
/// locally from the station's registry coordinates and the reference
/// station's own coordinates, never inferred from how a remote service
/// ordered its candidate list. It delegates to the workspace's single
/// haversine implementation ([`wrfcoin_geo_grid::haversine_km`], mean Earth
/// radius 6 371 km) so there is one formula, not two that can drift.
///
/// Service-side only: nothing on-chain calls this, and the result never
/// reaches a signing preimage, a transaction or the state root. The
/// `nws_station` bands were calibrated for ~3 km separation (see the module
/// docs), which is why any radius larger than that is an owner reward-policy
/// choice rather than a default here.
// determinism-ok: service-side reference-selection distance (attester only); never consensus-executed
pub fn great_circle_km(
    station: &crate::station_registry::StationLocation,
    // determinism-ok: service-side reference coordinates; never consensus-executed
    latitude: f64,
    // determinism-ok: as above
    longitude: f64,
    // determinism-ok: as above
) -> f64 {
    wrfcoin_geo_grid::haversine_km((station.latitude, station.longitude), (latitude, longitude))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::quality_tier::ReadingQualityTier;
    use crate::weather::reference::ReferenceBands;

    const NOW: i64 = 1_700_000_000;

    fn reading(temperature_c: f64) -> WeatherObservation {
        WeatherObservation {
            temperature_c: Some(temperature_c),
            humidity_percent: Some(55.0),
            pressure_hpa: Some(1013.2),
            wind_speed_ms: Some(3.1),
            precipitation_mm: Some(0.0),
            dewpoint_c: None,
            radio_metadata: None,
        }
    }

    fn reference(timestamp: i64, temperature_c: f64) -> StationReferenceObservation {
        StationReferenceObservation {
            timestamp,
            temperature_c: Some(temperature_c),
            pressure_hpa: Some(1013.0),
            humidity_pct: Some(52.0),
            wind_speed_ms: Some(4.0),
        }
    }

    #[test]
    fn the_bands_fixture_has_nws_station_entries() {
        // The pairing below is only meaningful if the tier machinery can see
        // these residuals. An `nws_station` residual with no band would be
        // silently skipped (→ Good), which is the exact silence this feature
        // removes.
        let bands = ReferenceBands::builtin();
        for variable in ["temperature_c", "pressure_hpa", "humidity_pct", "wind_speed_ms"] {
            assert!(
                bands.band(NWS_STATION_SOURCE, variable).is_some(),
                "fixtures/reference_bands.json is missing nws_station/{variable}"
            );
        }
    }

    #[test]
    fn nearest_reference_within_window_pairs_all_shared_variables() {
        let refs =
            vec![reference(NOW - 3_000, 0.0), reference(NOW - 600, 21.0), reference(NOW + 900, 30.0)];
        let comparison = pair_station_reference(&reading(21.5), NOW, &refs, 1_800)
            .expect("a reference 600s away must pair inside a 1800s window");
        assert_eq!(comparison.residuals.len(), 4);
        let temp = comparison
            .residuals
            .iter()
            .find(|r| r.variable == "temperature_c")
            .expect("temperature residual");
        assert_eq!(temp.source, NWS_STATION_SOURCE);
        // Nearest is the −600s observation at 21.0, NOT the +900s one.
        assert!((temp.residual - 0.5).abs() < 1e-9, "residual {}", temp.residual);
    }

    #[test]
    fn no_reference_inside_the_window_is_none_not_a_fake_good() {
        let refs = vec![reference(NOW - 2_000, 21.0)];
        assert_eq!(pair_station_reference(&reading(21.5), NOW, &refs, 1_800), None);
        assert_eq!(pair_station_reference(&reading(21.5), NOW, &[], 1_800), None);
    }

    #[test]
    fn a_reference_with_no_shared_variables_is_none() {
        let refs = vec![StationReferenceObservation {
            timestamp: NOW,
            temperature_c: None,
            pressure_hpa: None,
            humidity_pct: None,
            wind_speed_ms: None,
        }];
        assert_eq!(pair_station_reference(&reading(21.5), NOW, &refs, 1_800), None);
    }

    #[test]
    fn missing_variables_on_either_side_are_skipped_not_zeroed() {
        let mut obs = reading(21.5);
        obs.wind_speed_ms = None;
        let mut nws = reference(NOW, 21.0);
        nws.humidity_pct = None;
        let comparison = pair_station_reference(&obs, NOW, &[nws], 1_800).unwrap();
        let vars: Vec<&str> =
            comparison.residuals.iter().map(|r| r.variable.as_str()).collect();
        assert_eq!(vars, ["temperature_c", "pressure_hpa"]);
    }

    /// The acceptance case from the round-2 brief: a physically-plausible but
    /// WRONG reading (19.6 °C submitted while the airport reports 30 °C) must
    /// be materially down-weighted. 10.4 °C is far beyond the nws_station
    /// marginal band, so temperature's own tier is Outlier. Under the #3098
    /// second-worst rule a single Outlier among three agreeing variables
    /// costs one tier (Degraded, multiplier 0.5), not the 0.1 collapse — a
    /// station wrong on two independent variables still tiers Outlier (see
    /// `a_reading_wrong_on_two_variables_still_tiers_outlier`).
    #[test]
    fn a_plausible_but_wrong_reading_is_degraded_against_the_local_reference() {
        let refs = vec![reference(NOW - 300, 30.0)];
        let mut obs = reading(19.6);
        // Keep the other variables agreeing so temperature alone decides.
        obs.humidity_percent = Some(52.0);
        obs.pressure_hpa = Some(1013.0);
        obs.wind_speed_ms = Some(4.0);
        let comparison = pair_station_reference(&obs, NOW, &refs, 1_800).unwrap();
        let bands = ReferenceBands::builtin();
        assert_eq!(
            comparison.variable_tiers(&bands).iter().filter(|t| **t == ReadingQualityTier::Outlier).count(),
            1,
            "temperature alone must be the Outlier variable"
        );
        assert_eq!(comparison.tier(&bands), ReadingQualityTier::Degraded);
    }

    #[test]
    fn a_reading_wrong_on_two_variables_still_tiers_outlier() {
        let refs = vec![reference(NOW - 300, 30.0)];
        let mut obs = reading(19.6);
        obs.humidity_percent = Some(52.0);
        obs.pressure_hpa = Some(1013.0 + 40.0); // 40 hPa off: Outlier on nws_station too
        obs.wind_speed_ms = Some(4.0);
        let comparison = pair_station_reference(&obs, NOW, &refs, 1_800).unwrap();
        assert_eq!(comparison.tier(&ReferenceBands::builtin()), ReadingQualityTier::Outlier);
    }

    #[test]
    fn an_honest_reading_tiers_good_against_the_same_reference() {
        let refs = vec![reference(NOW - 300, 30.0)];
        let mut obs = reading(29.4);
        obs.humidity_percent = Some(52.0);
        obs.pressure_hpa = Some(1013.0);
        obs.wind_speed_ms = Some(4.0);
        let comparison = pair_station_reference(&obs, NOW, &refs, 1_800).unwrap();
        assert_eq!(comparison.tier(&ReferenceBands::builtin()), ReadingQualityTier::Good);
    }

    // ---- #3285: sea-level pressure pairing -------------------------------

    /// the reference station’s replayed pair (09-24): raw WS-5000 station pressure 1001.2 hPa
    /// at 227.7 m against KDLH's 1027.8 hPa altimeter setting,
    /// with the other variables agreeing.
    const JON_RAW_HPA: f64 = 1001.2;
    const JON_ELEVATION_M: f64 = 227.7;
    /// USGS 3DEP DEM ground elevation at the reference station's site: its
    /// true elevation is 20.2 m below it (#3299).
    const JON_DEM_M: f64 = 247.9;
    const KDLH_ALTIMETER_HPA: f64 = 1027.8;

    fn jon_reading() -> WeatherObservation {
        WeatherObservation {
            temperature_c: Some(10.2),
            humidity_percent: Some(97.0),
            pressure_hpa: Some(JON_RAW_HPA),
            wind_speed_ms: Some(1.0),
            precipitation_mm: Some(0.0),
            dewpoint_c: None,
            radio_metadata: None,
        }
    }

    fn kdlh() -> Vec<StationReferenceObservation> {
        vec![StationReferenceObservation {
            timestamp: NOW - 300,
            temperature_c: Some(10.0),
            pressure_hpa: Some(KDLH_ALTIMETER_HPA),
            humidity_pct: Some(93.0),
            wind_speed_ms: Some(2.1),
        }]
    }

    fn jon_claim() -> f64 {
        altimeter_setting_hpa(JON_RAW_HPA, JON_ELEVATION_M).unwrap()
    }

    fn pressure_residual(comparison: &ReferenceComparison) -> f64 {
        comparison.residuals.iter().find(|r| r.variable == "pressure_hpa").unwrap().residual
    }

    #[test]
    fn altimeter_setting_matches_the_golden_vectors() {
        const VECTORS_JSON: &str = include_str!("../../fixtures/altimeter_setting_vectors.json");
        let doc: serde_json::Value = serde_json::from_str(VECTORS_JSON).unwrap();
        let tolerance = doc["tolerance_hpa"].as_f64().unwrap();
        let vectors = doc["vectors"].as_array().unwrap();
        assert!(vectors.len() >= 5, "the fixture must carry the golden set");
        for v in vectors {
            let got = altimeter_setting_hpa(
                v["station_pressure_hpa"].as_f64().unwrap(),
                v["elevation_m"].as_f64().unwrap(),
            )
            .unwrap();
            let want = v["altimeter_setting_hpa"].as_f64().unwrap();
            assert!((got - want).abs() <= tolerance, "{}: got {got}, want {want}", v["name"]);
        }
    }

    #[test]
    fn altimeter_setting_refuses_inputs_that_do_not_reduce() {
        assert_eq!(altimeter_setting_hpa(f64::NAN, 100.0), None);
        assert_eq!(altimeter_setting_hpa(1000.0, f64::INFINITY), None);
        assert_eq!(altimeter_setting_hpa(0.3, 100.0), None);
        assert_eq!(altimeter_setting_hpa(-5.0, 100.0), None);
    }

    fn station(sea_level_hpa: f64) -> SeaLevelPressureClaim {
        SeaLevelPressureClaim { sea_level_hpa: Some(sea_level_hpa), kind: Some(PressureKind::Station) }
    }

    fn relative(sea_level_hpa: f64) -> SeaLevelPressureClaim {
        SeaLevelPressureClaim { sea_level_hpa: Some(sea_level_hpa), kind: Some(PressureKind::Relative) }
    }

    #[test]
    fn the_claim_is_read_strictly_from_the_reveal() {
        let read = |v: serde_json::Value| SeaLevelPressureClaim::from_reveal(&v);
        assert_eq!(
            read(serde_json::json!({"pressure_sea_level_hpa": 1028.3, "pressure_kind": "station"})),
            station(1028.3)
        );
        assert_eq!(
            read(serde_json::json!({"pressure_sea_level_hpa": 1020.1, "pressure_kind": "relative"})),
            relative(1020.1)
        );
        assert_eq!(read(serde_json::json!({"pressure_hpa": 1001.2})), SeaLevelPressureClaim::default());
        let not_a_number = read(serde_json::json!({"pressure_sea_level_hpa": "1028.3"}));
        assert_eq!(not_a_number.sea_level_hpa, None);
        assert_eq!(read(serde_json::json!({"pressure_sea_level_hpa": null})).sea_level_hpa, None);
        // An unknown kind is no kind, never a guess.
        assert_eq!(read(serde_json::json!({"pressure_kind": "sea_level"})).kind, None);
        assert_eq!(read(serde_json::json!({"pressure_kind": 1})).kind, None);
        assert_eq!(PressureKind::Station.as_str(), "station");
        assert_eq!(PressureKind::Relative.as_str(), "relative");
    }

    fn run(
        reading: &WeatherObservation,
        claim: SeaLevelPressureClaim,
        elevation: Option<f64>,
    ) -> (Option<ReferenceComparison>, SeaLevelPressureDecision) {
        pair_station_reference_sea_level(reading, claim, elevation, Some(JON_DEM_M), true, NOW, &kdlh(), 1_800)
    }

    fn has_pressure(comparison: &Option<ReferenceComparison>) -> bool {
        comparison
            .as_ref()
            .is_some_and(|c| c.residuals.iter().any(|r| r.variable == "pressure_hpa"))
    }

    fn without_pressure(reading: &WeatherObservation) -> Option<ReferenceComparison> {
        let mut r = reading.clone();
        r.pressure_hpa = None;
        pair_station_reference(&r, NOW, &kdlh(), 1_800)
    }

    /// Feature inactive ⇒ identical to today, whatever the reveal claims,
    /// with or without a registered elevation.
    #[test]
    fn inactive_is_todays_pairing_byte_for_byte() {
        let today = pair_station_reference(&jon_reading(), NOW, &kdlh(), 1_800);
        for (claim, elevation) in [
            (station(jon_claim()), Some(JON_ELEVATION_M)),
            (station(9999.0), Some(JON_ELEVATION_M)),
            (relative(JON_RAW_HPA), None),
            (SeaLevelPressureClaim::default(), None),
            (SeaLevelPressureClaim::default(), Some(JON_ELEVATION_M)),
        ] {
            // #3299: whatever DEM the attester holds (none, in band, or one
            // that puts the registered elevation far out of band).
            for dem in [None, Some(JON_DEM_M), Some(JON_DEM_M + 500.0)] {
                let (comparison, decision) = pair_station_reference_sea_level(
                    &jon_reading(),
                    claim,
                    elevation,
                    dem,
                    false,
                    NOW,
                    &kdlh(),
                    1_800,
                );
                assert_eq!(comparison, today);
                assert_eq!(decision.outcome, SeaLevelPressureOutcome::Inactive);
                assert_eq!(decision.pairing, PressurePairing::Raw);
                assert_eq!(decision.audit_reason(), None);
            }
        }
        // Today Jon is a pressure Outlier and so tiers Degraded (#3285 body).
        let today = today.unwrap();
        assert!((pressure_residual(&today) - (JON_RAW_HPA - KDLH_ALTIMETER_HPA)).abs() < 1e-9);
        assert_eq!(today.tier(&ReferenceBands::builtin()), ReadingQualityTier::Degraded);
    }

    /// Active + a station-kind claim that recomputes ⇒ the sea-level value is
    /// paired and Jon tiers Good; the other residuals are unchanged.
    #[test]
    fn active_with_a_verified_claim_pairs_sea_level_pressure() {
        let (comparison, decision) = run(&jon_reading(), station(jon_claim()), Some(JON_ELEVATION_M));
        let comparison = comparison.unwrap();
        assert!(matches!(decision.outcome, SeaLevelPressureOutcome::Verified { .. }), "{decision:?}");
        assert_eq!(decision.pairing, PressurePairing::SeaLevel);
        assert_eq!(decision.audit_reason(), None);
        assert!((pressure_residual(&comparison) - (jon_claim() - KDLH_ALTIMETER_HPA)).abs() < 1e-9);
        assert!(pressure_residual(&comparison).abs() < 1.0);
        assert_eq!(comparison.tier(&ReferenceBands::builtin()), ReadingQualityTier::Good);

        let today = pair_station_reference(&jon_reading(), NOW, &kdlh(), 1_800).unwrap();
        let others = |c: &ReferenceComparison| -> Vec<ReferenceResidual> {
            c.residuals.iter().filter(|r| r.variable != "pressure_hpa").cloned().collect()
        };
        assert_eq!(others(&comparison), others(&today));
    }

    /// A claim a collector rounded to 0.1 hPa still verifies.
    #[test]
    fn a_rounded_claim_within_tolerance_verifies() {
        let rounded = (jon_claim() * 10.0).round() / 10.0 + 0.5;
        let (_, decision) = run(&jon_reading(), station(rounded), Some(JON_ELEVATION_M));
        assert!(matches!(decision.outcome, SeaLevelPressureOutcome::Verified { .. }), "{decision:?}");
    }

    /// Owner rulings: active + a registered elevation + NO claim ⇒ no raw
    /// pressure residual (not a raw fallback), the other residuals exactly as
    /// before, and pressure marked Unverified — the fold scores it Outlier, so
    /// the reference station’s healthy reading is Degraded ([G,Out,G,G]), not Good.
    #[test]
    fn active_with_elevation_and_no_claim_scores_pressure_unverified() {
        let (comparison, decision) =
            run(&jon_reading(), SeaLevelPressureClaim::default(), Some(JON_ELEVATION_M));
        assert_eq!(decision.outcome, SeaLevelPressureOutcome::NotClaimed);
        assert_eq!(decision.pairing, PressurePairing::Unverified);
        assert!(!has_pressure(&comparison), "{comparison:?}");
        assert_eq!(comparison, without_pressure(&jon_reading()));
        assert_eq!(comparison.as_ref().unwrap().residuals.len(), 3, "temperature, humidity, wind");
        assert_eq!(scored_tier(&comparison, &decision, false), ReadingQualityTier::Degraded);
        assert!(decision.audit_reason().unwrap().contains("scored Outlier as unverified"));

        // A claim with no known kind is the same case.
        let no_kind = SeaLevelPressureClaim { sea_level_hpa: Some(jon_claim()), kind: None };
        let (comparison, decision) = run(&jon_reading(), no_kind, Some(JON_ELEVATION_M));
        assert_eq!(decision.outcome, SeaLevelPressureOutcome::NoPressureKind);
        assert_eq!(decision.pairing, PressurePairing::Unverified);
        assert!(!has_pressure(&comparison));
        assert!(decision.audit_reason().unwrap().contains("pressure_kind"));

        // Nothing to check (the reference has no pressure) ⇒ merely Skipped,
        // not Unverified: an honest claim would not have been scored either.
        let mut no_ref_pressure = kdlh();
        no_ref_pressure[0].pressure_hpa = None;
        let (comparison, decision) = pair_station_reference_sea_level(
            &jon_reading(),
            SeaLevelPressureClaim::default(),
            Some(JON_ELEVATION_M),
            Some(JON_DEM_M),
            true,
            NOW,
            &no_ref_pressure,
            1_800,
        );
        assert_eq!(decision.pairing, PressurePairing::Skipped);
        assert!(!has_pressure(&comparison));
        assert!(decision.audit_reason().unwrap().contains("pressure not compared"));
    }

    /// A mismatch from a station with a registered elevation is skipped too
    /// (the conservative A1 choice: neither the untrustworthy claim nor the
    /// known-wrong raw value is compared). Asserting KDLH's own value does
    /// not pass.
    #[test]
    fn active_with_a_mismatched_claim_scores_pressure_unverified_and_records_it() {
        for fabricated in [KDLH_ALTIMETER_HPA, jon_claim() + 1.01, jon_claim() - 3.0] {
            let (comparison, decision) =
                run(&jon_reading(), station(fabricated), Some(JON_ELEVATION_M + 20.0));
            // 20 m of wrong elevation moves the recompute ~2.4 hPa: the
            // correctly reduced claim no longer matches either.
            assert!(matches!(decision.outcome, SeaLevelPressureOutcome::Mismatch { .. }), "{decision:?}");
            assert_eq!(decision.pairing, PressurePairing::Unverified);
            assert!(!has_pressure(&comparison));
            let reason = decision.audit_reason().unwrap();
            assert!(reason.contains("mismatch") && reason.contains("unverified"), "{reason}");
        }
        // 1027.8 vs the 1028.29 recompute is inside 1 hPa: honest rounding
        // noise, not fabrication, so this one verifies.
        let (comparison, decision) =
            run(&jon_reading(), station(KDLH_ALTIMETER_HPA), Some(JON_ELEVATION_M));
        assert!(matches!(decision.outcome, SeaLevelPressureOutcome::Verified { .. }), "{decision:?}");
        assert!(has_pressure(&comparison));
    }

    /// Active, NO registered elevation ⇒ today's raw pairing, whatever the
    /// claim says (there is nothing to recompute against).
    #[test]
    fn active_without_elevation_pairs_raw() {
        let today = pair_station_reference(&jon_reading(), NOW, &kdlh(), 1_800);

        let (comparison, decision) = run(&jon_reading(), SeaLevelPressureClaim::default(), None);
        assert_eq!(comparison, today);
        assert_eq!(decision.outcome, SeaLevelPressureOutcome::NotClaimed);
        assert_eq!(decision.pairing, PressurePairing::Raw);
        assert_eq!(decision.audit_reason(), None, "no claim + no elevation is routine");

        let (comparison, decision) = run(&jon_reading(), station(jon_claim()), None);
        assert_eq!(comparison, today);
        assert_eq!(decision.outcome, SeaLevelPressureOutcome::NoElevation);
        assert_eq!(decision.pairing, PressurePairing::Raw);
        assert!(decision.audit_reason().unwrap().contains("paired raw"));

        let no_kind = SeaLevelPressureClaim { sea_level_hpa: Some(jon_claim()), kind: None };
        let (comparison, decision) = run(&jon_reading(), no_kind, None);
        assert_eq!(comparison, today);
        assert_eq!(decision.pairing, PressurePairing::Raw);

        let mut no_raw = jon_reading();
        no_raw.pressure_hpa = None;
        let (comparison, decision) = run(&no_raw, station(jon_claim()), Some(JON_ELEVATION_M));
        assert_eq!(decision.outcome, SeaLevelPressureOutcome::NoRawPressure);
        assert!(!has_pressure(&comparison));
    }

    /// Steven's collector submits Ambient's relative (owner-offset) pressure
    /// as `pressure_hpa`, with `pressure_kind: "relative"` and the same value
    /// as the claim. It is never reduced from elevation (that would add his
    /// whole elevation a second time): it pairs raw — the same number — with
    /// no audit noise, whatever his registered elevation.
    #[test]
    fn a_relative_claim_is_never_reduced_again() {
        let mut steven = jon_reading();
        steven.pressure_hpa = Some(1020.1);
        let today = pair_station_reference(&steven, NOW, &kdlh(), 1_800);
        for elevation in [None, Some(242.5)] {
            let (comparison, decision) = run(&steven, relative(1020.1), elevation);
            assert_eq!(comparison, today);
            assert_eq!(decision.outcome, SeaLevelPressureOutcome::RelativeRaw);
            assert_eq!(decision.pairing, PressurePairing::Raw);
            assert_eq!(decision.audit_reason(), None);
        }
        // A relative claim that does not repeat pressure_hpa is a mismatch,
        // even when it equals what a (wrong) elevation reduction would give;
        // with a registered elevation it is scored unverified.
        let double_reduced = altimeter_setting_hpa(1020.1, 242.5).unwrap();
        let (comparison, decision) = run(&steven, relative(double_reduced), Some(242.5));
        assert_eq!(
            decision.outcome,
            SeaLevelPressureOutcome::Mismatch { claimed_hpa: double_reduced, expected_hpa: 1020.1 }
        );
        assert_eq!(decision.pairing, PressurePairing::Unverified);
        assert!(!has_pressure(&comparison));
        // The same relative value mislabelled "station" does not verify.
        let (_, decision) = run(&steven, station(1020.1), Some(242.5));
        assert!(matches!(decision.outcome, SeaLevelPressureOutcome::Mismatch { .. }), "{decision:?}");
    }

    /// The reading's tier as the attester folds it: residuals plus one
    /// Outlier per unverified variable.
    fn scored_tier(
        comparison: &Option<ReferenceComparison>,
        decision: &SeaLevelPressureDecision,
        gross_guard: bool,
    ) -> ReadingQualityTier {
        let none = ReferenceComparison { residuals: Vec::new() };
        comparison.as_ref().unwrap_or(&none).tier_under_with_unverified(
            &ReferenceBands::builtin(),
            gross_guard,
            decision.pairing.unverified_variables(),
        )
    }

    fn rank(tier: ReadingQualityTier) -> u8 {
        match tier {
            ReadingQualityTier::Good => 0,
            ReadingQualityTier::Degraded => 1,
            ReadingQualityTier::Outlier => 2,
        }
    }

    /// review-trio's Delta review 3 probe (review-3285-core4.md), encoded:
    /// Jon at 227.7 m with a biased barometer and/or hygrometer, active. The
    /// honest claim is the altimeter value correctly recomputed from the
    /// BIASED raw pressure. For every row, every way of withholding or
    /// corrupting the claim (no claim, no kind, +50 hPa garbage, a claim off
    /// by 3 hPa) must tier no better than honesty — with and without the
    /// #3131 gross guard — and the rows the review computed by hand must
    /// land exactly.
    #[test]
    fn withholding_or_corrupting_the_claim_never_beats_honesty_probe_rows() {
        use ReadingQualityTier::{Degraded, Good, Outlier};
        // (barometer bias hPa, hygrometer offset %, honest tier, withheld tier)
        let rows: [(f64, f64, ReadingQualityTier, ReadingQualityTier); 6] = [
            (0.0, 0.0, Good, Degraded),       // healthy: [G,Out,G,G] when withheld
            (-3.0, 0.0, Good, Degraded),
            (-27.0, 0.0, Degraded, Degraded), // [G,Out,G,G] either way
            (-27.0, -20.0, Degraded, Degraded), // [G,Out,Deg,G]
            (-27.0, -35.0, Outlier, Outlier), // two Outliers
            // Hygrometer alone: honest [G,G,Out,G] → Degraded; withholding adds
            // the unverified pressure as a second Outlier → Outlier (strictly
            // worse than honesty; under the old skip rule these were equal).
            (0.0, -35.0, Degraded, Outlier),
        ];
        for (bias, hyg, honest_want, withheld_want) in rows {
            let mut reading = jon_reading();
            reading.pressure_hpa = Some(JON_RAW_HPA + bias);
            reading.humidity_percent = Some(97.0 + hyg);
            let honest_claim =
                station(altimeter_setting_hpa(JON_RAW_HPA + bias, JON_ELEVATION_M).unwrap());
            let withheld = [
                SeaLevelPressureClaim::default(),
                SeaLevelPressureClaim { sea_level_hpa: honest_claim.sea_level_hpa, kind: None },
                station(honest_claim.sea_level_hpa.unwrap() + 50.0),
                station(honest_claim.sea_level_hpa.unwrap() - 3.0),
            ];
            for gross_guard in [false, true] {
                let (hc, hd) = run(&reading, honest_claim, Some(JON_ELEVATION_M));
                assert_eq!(hd.pairing, PressurePairing::SeaLevel, "row {bias}/{hyg}");
                let honest = scored_tier(&hc, &hd, gross_guard);
                assert_eq!(honest, honest_want, "honest, row {bias}/{hyg}, guard {gross_guard}");
                for claim in withheld {
                    let (c, d) = run(&reading, claim, Some(JON_ELEVATION_M));
                    assert_eq!(d.pairing, PressurePairing::Unverified, "{claim:?}");
                    let got = scored_tier(&c, &d, gross_guard);
                    assert!(
                        rank(got) >= rank(honest),
                        "withholding beat honesty: row {bias}/{hyg} guard {gross_guard} \
                         claim {claim:?}: {got:?} vs honest {honest:?}"
                    );
                    assert_eq!(got, withheld_want, "row {bias}/{hyg} claim {claim:?}");
                }
            }
        }
    }

    /// A deliberately wrong claim (+50 hPa) is a Mismatch and behaves exactly
    /// as unverified-Outlier: the same tier as sending no claim at all.
    #[test]
    fn a_deliberately_wrong_claim_is_unverified_outlier() {
        let (c, d) = run(&jon_reading(), station(jon_claim() + 50.0), Some(JON_ELEVATION_M));
        assert!(matches!(d.outcome, SeaLevelPressureOutcome::Mismatch { .. }), "{d:?}");
        assert_eq!(d.pairing, PressurePairing::Unverified);
        assert!(!has_pressure(&c));
        let (nc, nd) = run(&jon_reading(), SeaLevelPressureClaim::default(), Some(JON_ELEVATION_M));
        assert_eq!(scored_tier(&c, &d, false), scored_tier(&nc, &nd, false));
        assert_eq!(scored_tier(&c, &d, false), ReadingQualityTier::Degraded);
    }

    /// With no unverified variables the new fold is exactly the old one.
    #[test]
    fn zero_unverified_is_exactly_tier_under() {
        let bands = ReferenceBands::builtin();
        let comparison = pair_station_reference(&jon_reading(), NOW, &kdlh(), 1_800).unwrap();
        for guard in [false, true] {
            assert_eq!(
                comparison.tier_under_with_unverified(&bands, guard, 0),
                comparison.tier_under(&bands, guard)
            );
        }
    }

    /// review-trio Delta review 4 F1, at the pairing layer: on a reading that
    /// tiers GOOD (pressure agrees with KDLH), the three non-Unverified cases
    /// must fold with zero unverified variables and stay Good — (a) inactive,
    /// (b) active + a consistent relative claim, (c) active + no registered
    /// elevation. Leaking the unverified Outlier into any of them turns Good
    /// into Degraded.
    #[test]
    fn f1_unverified_scoring_never_leaks_into_raw_pairings() {
        let mut good = jon_reading();
        good.pressure_hpa = Some(1027.5);
        let today = pair_station_reference(&good, NOW, &kdlh(), 1_800);
        assert_eq!(today.as_ref().unwrap().tier(&ReferenceBands::builtin()), ReadingQualityTier::Good);

        let cases = [
            // (a) inactive, with an elevation and no claim
            pair_station_reference_sea_level(
                &good,
                SeaLevelPressureClaim::default(),
                Some(JON_ELEVATION_M),
                Some(JON_DEM_M),
                false,
                NOW,
                &kdlh(),
                1_800,
            ),
            // (b) active, consistent relative claim, with an elevation
            run(&good, relative(1027.5), Some(242.5)),
            // (c) active, no registered elevation, no claim
            run(&good, SeaLevelPressureClaim::default(), None),
        ];
        for (comparison, decision) in cases {
            assert_eq!(decision.pairing, PressurePairing::Raw, "{decision:?}");
            assert_eq!(decision.pairing.unverified_variables(), 0, "{decision:?}");
            assert_eq!(comparison, today);
            for guard in [false, true] {
                assert_eq!(
                    scored_tier(&comparison, &decision, guard),
                    ReadingQualityTier::Good,
                    "{decision:?} guard {guard}"
                );
            }
        }
    }

    fn at(latitude: f64, longitude: f64) -> crate::station_registry::StationLocation {
        crate::station_registry::StationLocation { latitude, longitude, elevation_meters: None }
    }

    /// #3240: known-pair values for the radius check, against closed-form
    /// results for a 6 371 km sphere.
    #[test]
    fn great_circle_km_known_pairs_3240() {
        let close = |got: f64, want: f64, tol: f64| {
            assert!((got - want).abs() <= tol, "got {got} km, want {want} ± {tol} km");
        };
        // Same point.
        close(great_circle_km(&at(46.7867, -92.1005), 46.7867, -92.1005), 0.0, 1e-9);
        // One degree of longitude on the equator: R·π/180.
        close(great_circle_km(&at(0.0, 0.0), 0.0, 1.0), 111.194_926_644_558_7, 1e-6);
        // Pole to pole: R·π.
        close(great_circle_km(&at(90.0, 0.0), -90.0, 0.0), 20_015.086_796_020_57, 1e-6);
        // The antimeridian is the short way round, not 359°.
        close(great_circle_km(&at(0.0, 179.5), 0.0, -179.5), 111.194_926_644_559, 1e-6);
        // Duluth test site → KDLH-area point (~8.4 km) and → KMSP-area point
        // (~231 km): the two scales the attester's radius has to separate.
        close(great_circle_km(&at(46.7867, -92.1005), 46.8369, -92.1833), 8.418, 1e-3);
        close(great_circle_km(&at(46.8369, -92.1833), 44.8848, -93.2223), 231.489, 1e-3);
    }

    /// #3240: symmetric, and elevation plays no part (a surface distance).
    #[test]
    fn great_circle_km_is_symmetric_and_ignores_elevation_3240() {
        let a = at(41.95, -77.10);
        let mut a_high = a.clone();
        a_high.elevation_meters = Some(2_500.0);
        let there = great_circle_km(&a, 46.84, -92.18);
        let back = great_circle_km(&at(46.84, -92.18), 41.95, -77.10);
        assert_eq!(there.to_bits(), back.to_bits());
        assert_eq!(there.to_bits(), great_circle_km(&a_high, 46.84, -92.18).to_bits());
    }

    /// #3299 owner ruling: the band is DEM −50 m / +60 m, edges included.
    /// The reference station's honest 227.7 m (20.2 m below the DEM) is inside.
    #[test]
    fn dem_band_edges_are_minus_50_and_plus_60_3299() {
        assert!(elevation_within_dem_band(JON_DEM_M - DEM_BAND_BELOW_M, JON_DEM_M));
        assert!(elevation_within_dem_band(JON_DEM_M + DEM_BAND_ABOVE_M, JON_DEM_M));
        assert!(!elevation_within_dem_band(JON_DEM_M - DEM_BAND_BELOW_M - 0.1, JON_DEM_M));
        assert!(!elevation_within_dem_band(JON_DEM_M + DEM_BAND_ABOVE_M + 0.1, JON_DEM_M));
        assert!(elevation_within_dem_band(JON_ELEVATION_M, JON_DEM_M));
        assert!(!elevation_within_dem_band(f64::NAN, JON_DEM_M));
        assert!(!elevation_within_dem_band(JON_ELEVATION_M, f64::INFINITY));
    }

    /// Active pairing at and just past each band edge: an honest claim at an
    /// in-band elevation is Verified; one 0.1 m past the edge is not trusted
    /// and, with a published pressure the reference can check, Unverified.
    #[test]
    fn active_pairing_respects_the_dem_band_edges_3299() {
        for (elevation, inside) in [
            (JON_DEM_M - DEM_BAND_BELOW_M, true),
            (JON_DEM_M + DEM_BAND_ABOVE_M, true),
            (JON_DEM_M - DEM_BAND_BELOW_M - 0.1, false),
            (JON_DEM_M + DEM_BAND_ABOVE_M + 0.1, false),
        ] {
            let claim = altimeter_setting_hpa(JON_RAW_HPA, elevation).unwrap();
            let (_, decision) = run(&jon_reading(), station(claim), Some(elevation));
            if inside {
                assert!(matches!(decision.outcome, SeaLevelPressureOutcome::Verified { .. }), "{elevation}: {decision:?}");
                assert_eq!(decision.pairing, PressurePairing::SeaLevel);
            } else {
                assert!(
                    matches!(decision.outcome, SeaLevelPressureOutcome::ElevationOutsideDemBand { .. }),
                    "{elevation}: {decision:?}"
                );
                assert_eq!(decision.pairing, PressurePairing::Unverified);
                assert!(decision.audit_reason().unwrap().contains("outside the DEM band"));
            }
        }
    }

    /// No trusted DEM for a station WITH a registered elevation: the claim is
    /// not trusted (fail closed). A relative claim never uses the elevation,
    /// and a station with no registered elevation pairs raw, as before.
    #[test]
    fn missing_dem_fails_closed_only_where_the_elevation_is_used_3299() {
        let (_, decision) = pair_station_reference_sea_level(
            &jon_reading(),
            station(jon_claim()),
            Some(JON_ELEVATION_M),
            None,
            true,
            NOW,
            &kdlh(),
            1_800,
        );
        assert_eq!(decision.outcome, SeaLevelPressureOutcome::NoDemElevation);
        assert_eq!(decision.pairing, PressurePairing::Unverified);

        let mut relative_station = jon_reading();
        relative_station.pressure_hpa = Some(1020.1);
        let (_, decision) = pair_station_reference_sea_level(
            &relative_station,
            relative(1020.1),
            Some(242.5),
            None,
            true,
            NOW,
            &kdlh(),
            1_800,
        );
        assert_eq!(decision.outcome, SeaLevelPressureOutcome::RelativeRaw);

        let (_, decision) = pair_station_reference_sea_level(
            &jon_reading(),
            station(jon_claim()),
            None,
            None,
            true,
            NOW,
            &kdlh(),
            1_800,
        );
        assert_eq!(decision.outcome, SeaLevelPressureOutcome::NoElevation);
        assert_eq!(decision.pairing, PressurePairing::Raw);
    }
}
