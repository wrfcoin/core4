//! The integer forecast skill rule (the "late score").
//!
//! Everything here is integer arithmetic on fixed-point values: temperature
//! in 0.01 °C, pressure in Pa. There is no floating point, every division
//! truncates toward zero, and `isqrt` is the floor square root, so every
//! platform computes the same number. The oracle job, the backend's practice
//! score and consensus use the same rule, so the number a forecaster sees is
//! the number that is paid.
//!
//! The rule is fixed by the shared vectors in
//! `forecast-score/vectors/forecast-skill-vectors.json` (schema
//! `wrfcoin-forecast-skill-vectors/v3`, vendored byte-identical from the
//! backend repository; its `rule` array is the normative text). This module
//! is the Rust implementation; its tests run every case in that file.
//!
//! What runs where:
//!
//! - **On chain:** [`late_skill`] only (rule 7), plus the range rule (rule 1).
//! - **Oracle side, off chain:** which readings count ([`truth_candidates`],
//!   [`window_candidates`]), the truth ([`nearest_reading`]), the window
//!   ([`hourly_window`], [`last_reading_before`]) and the reference
//!   ([`reference_from_window`]). Oracles sign the resulting integers.
//! - **Adapter, never on chain:** [`parse_fixed_point`], decimal text to
//!   fixed point. The chain never parses or rounds a decimal or a float.

use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

/// Skill is expressed in millionths: `1_000_000` is a perfect forecast.
pub const SKILL_PRECISION: u64 = 1_000_000;
/// Default copy tolerance in fixed-point units (0.05 °C; 5 Pa).
pub const DEFAULT_COPY_TOLERANCE_UNITS: u64 = 5;
/// Largest reference error accepted by [`late_skill`]. The shared rule puts
/// no bound on the reference; this is a sanity cap far above anything a
/// window of in-range readings can produce (at most 90_000).
pub const MAX_REFERENCE: u64 = 1_000_000;
/// Largest copy tolerance accepted.
pub const MAX_COPY_TOLERANCE: u64 = 1_000;
/// Hours in the reference window.
pub const WINDOW_HOURS: usize = 24;
/// Fewest hourly samples a window needs for a baseline.
pub const MIN_WINDOW_SAMPLES: usize = 12;
/// Fewest slot pairs the persistence error needs.
pub const MIN_PERSISTENCE_PAIRS: usize = 6;
/// A truth reading is at most this far from the target time.
pub const TRUTH_MAX_DISTANCE_SECS: i64 = 15 * 60;
/// Time allowed for a reading to land and reach quorum after the window closes.
pub const LANDING_MARGIN_SECS: i64 = 20 * 60;
/// No score before the chain's time has reached target plus this.
pub const SCORE_DELAY_SECS: i64 = TRUTH_MAX_DISTANCE_SECS + LANDING_MARGIN_SECS;
const SECS_PER_HOUR: i64 = 3_600;

/// Domain tag of [`window_hash`].
pub const WINDOW_HASH_DOMAIN: &[u8] = b"wrfcoin.forecast_late_score.window.v1";

/// A variable the rule scores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoredVariable {
    /// Air temperature, fixed point in 0.01 °C.
    TemperatureC,
    /// Station pressure, fixed point in Pa (0.01 hPa).
    PressureHpa,
}

impl ScoredVariable {
    pub const ALL: [Self; 2] = [Self::TemperatureC, Self::PressureHpa];

    /// Rule 1: the inclusive plausible range of the variable.
    pub const fn range(self) -> (i64, i64) {
        match self {
            Self::TemperatureC => (-10_000, 10_000),
            Self::PressureHpa => (30_000, 120_000),
        }
    }

    /// Whether `value` is inside the variable's range.
    pub const fn in_range(self, value: i64) -> bool {
        let (low, high) = self.range();
        value >= low && value <= high
    }
}

/// The inputs of one score (rule 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LateScoreInput {
    pub variable: ScoredVariable,
    pub forecast: i64,
    pub truth: i64,
    pub reference: u64,
    pub mean24: i64,
    pub last: i64,
    pub tolerance: u64,
}

/// Which baseline a forecast copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopiedBaseline {
    /// Within tolerance of the window mean (checked first).
    Mean24,
    /// Within tolerance of the last reading before the commit.
    Last,
}

/// The result of one score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LateScore {
    /// Skill in millionths, `0..=SKILL_PRECISION`.
    pub skill: u64,
    /// `|forecast − truth|` in fixed-point units.
    pub err: u64,
    /// Set when the forecast copied a baseline (skill is then 0).
    pub copied: Option<CopiedBaseline>,
}

impl LateScore {
    /// Whether the forecast beat the baseline.
    pub const fn beat_baseline(&self) -> bool {
        self.skill > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LateScoreError {
    #[error("forecast, truth, mean24 or last is outside the variable's range")]
    ValueOutOfRange,
    #[error("reference is above MAX_REFERENCE")]
    ReferenceOutOfRange,
    #[error("copy tolerance is above MAX_COPY_TOLERANCE")]
    ToleranceOutOfRange,
    #[error("a window has exactly 24 hourly slots")]
    WindowLength,
    #[error("lag must be between 1 and 24 hours")]
    Lag,
    #[error("arithmetic overflow")]
    Overflow,
}

/// Rule 7 arithmetic alone, with no range rule:
/// `err = |forecast − truth|`; a copy or a zero reference scores 0; otherwise
/// `max(0, 1_000_000 − 1_000_000·err/reference)`.
///
/// `abs_diff` on `i64` cannot overflow (the result is a `u64`), and the
/// product is formed in `u128` with checked operations, so every `i64` input
/// is safe. Under the range rule `1_000_000·err ≤ 2.4·10^11`.
pub fn skill_arithmetic(
    forecast: i64,
    truth: i64,
    reference: u64,
    mean24: i64,
    last: i64,
    tolerance: u64,
) -> Result<LateScore, LateScoreError> {
    let err = forecast.abs_diff(truth);
    let copied = if forecast.abs_diff(mean24) <= tolerance {
        Some(CopiedBaseline::Mean24)
    } else if forecast.abs_diff(last) <= tolerance {
        Some(CopiedBaseline::Last)
    } else {
        None
    };
    if copied.is_some() || reference == 0 || err >= reference {
        return Ok(LateScore {
            skill: 0,
            err,
            copied,
        });
    }
    // err < reference here, so the quotient is < SKILL_PRECISION.
    let lost = u128::from(SKILL_PRECISION)
        .checked_mul(u128::from(err))
        .and_then(|n| n.checked_div(u128::from(reference)))
        .ok_or(LateScoreError::Overflow)?;
    let lost = u64::try_from(lost).map_err(|_| LateScoreError::Overflow)?;
    let skill = SKILL_PRECISION
        .checked_sub(lost)
        .ok_or(LateScoreError::Overflow)?;
    Ok(LateScore { skill, err, copied })
}

/// Score one forecast against one observation: the range rule (rule 1), then
/// [`skill_arithmetic`] (rule 7). This is the function consensus calls. An
/// out-of-range input is an error, never a clamped value (fail closed).
pub fn late_skill(input: &LateScoreInput) -> Result<LateScore, LateScoreError> {
    let variable = input.variable;
    if !(variable.in_range(input.forecast)
        && variable.in_range(input.truth)
        && variable.in_range(input.mean24)
        && variable.in_range(input.last))
    {
        return Err(LateScoreError::ValueOutOfRange);
    }
    if input.reference > MAX_REFERENCE {
        return Err(LateScoreError::ReferenceOutOfRange);
    }
    if input.tolerance > MAX_COPY_TOLERANCE {
        return Err(LateScoreError::ToleranceOutOfRange);
    }
    skill_arithmetic(
        input.forecast,
        input.truth,
        input.reference,
        input.mean24,
        input.last,
        input.tolerance,
    )
}

/// Which spread [`reference_from_window`] chose as the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    ClimatologySpread,
    PersistenceSpread,
}

/// Rule 6: the numbers derived from a station's hourly window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowReference {
    /// Slots that hold a sample.
    pub samples: usize,
    /// `sum / samples`, truncated toward zero.
    pub mean24: i64,
    /// Population standard deviation, floored.
    pub spread: u64,
    /// Slot pairs `(i, i + lag)` where both hold a sample.
    pub pairs: usize,
    /// Lagged-persistence RMSE, floored; `None` below [`MIN_PERSISTENCE_PAIRS`].
    pub persistence: Option<u64>,
    /// The persistence error when it exists and is smaller than the spread,
    /// else the spread.
    pub reference: u64,
    pub kind: ReferenceKind,
}

/// Floor of the square root of `n` (Newton's method on integers).
pub fn isqrt_u128(n: u128) -> u128 {
    if n < 2 {
        return n;
    }
    // Start at a power of two that is ≥ sqrt(n), then descend monotonically.
    let bits = 128 - n.leading_zeros();
    let mut x: u128 = 1u128 << bits.div_ceil(2);
    loop {
        // x ≥ 1 always: the sequence decreases to floor(sqrt(n)) ≥ 1.
        let y = (x + n / x) >> 1;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// Rule 6. `window` is the 24 hourly slots, oldest first; `lag` is the lead in
/// hours. `Ok(None)` when the window has fewer than [`MIN_WINDOW_SAMPLES`]
/// samples: no baseline, the forecast is unscored.
///
/// - `spread = isqrt((n·Σx² − (Σx)²) / n²)`
/// - `persistence = isqrt(Σ(x[i+lag] − x[i])² / pairs)` when `pairs ≥ 6`
///
/// Widths: the shared rule shows every term fits `i64` under the range rule
/// (`n·Σx² < 8.3·10^12`). This uses `i128`/`u128` with checked operations
/// anyway, so any `i64` window is safe.
pub fn reference_from_window(
    window: &[Option<i64>],
    lag: usize,
) -> Result<Option<WindowReference>, LateScoreError> {
    if window.len() != WINDOW_HOURS {
        return Err(LateScoreError::WindowLength);
    }
    if !(1..=WINDOW_HOURS).contains(&lag) {
        return Err(LateScoreError::Lag);
    }
    let samples = window.iter().flatten().count();
    if samples < MIN_WINDOW_SAMPLES {
        return Ok(None);
    }
    let n = samples as i128;
    let mut sum: i128 = 0;
    let mut sum_sq: i128 = 0;
    for value in window.iter().flatten() {
        let value = i128::from(*value);
        sum = sum.checked_add(value).ok_or(LateScoreError::Overflow)?;
        sum_sq = value
            .checked_mul(value)
            .and_then(|sq| sum_sq.checked_add(sq))
            .ok_or(LateScoreError::Overflow)?;
    }
    // Cauchy–Schwarz: n·Σx² ≥ (Σx)², so the numerator is never negative.
    let numerator = n
        .checked_mul(sum_sq)
        .and_then(|a| sum.checked_mul(sum).and_then(|b| a.checked_sub(b)))
        .ok_or(LateScoreError::Overflow)?;
    let numerator = u128::try_from(numerator).map_err(|_| LateScoreError::Overflow)?;
    let spread = isqrt_u128(numerator / (n * n) as u128);

    let mut pairs = 0usize;
    let mut squared: u128 = 0;
    for i in 0..WINDOW_HOURS.saturating_sub(lag) {
        if let (Some(earlier), Some(later)) = (window[i], window[i + lag]) {
            let difference = u128::from(later.abs_diff(earlier));
            squared = difference
                .checked_mul(difference)
                .and_then(|sq| squared.checked_add(sq))
                .ok_or(LateScoreError::Overflow)?;
            pairs += 1;
        }
    }
    let persistence = (pairs >= MIN_PERSISTENCE_PAIRS).then(|| isqrt_u128(squared / pairs as u128));
    let (reference, kind) = match persistence {
        Some(p) if p < spread => (p, ReferenceKind::PersistenceSpread),
        _ => (spread, ReferenceKind::ClimatologySpread),
    };
    let narrow = |value: u128| u64::try_from(value).map_err(|_| LateScoreError::Overflow);
    Ok(Some(WindowReference {
        samples,
        // Rust's integer division truncates toward zero, as the rule requires.
        mean24: i64::try_from(sum / n).map_err(|_| LateScoreError::Overflow)?,
        spread: narrow(spread)?,
        pairs,
        persistence: persistence.map(narrow).transpose()?,
        reference: narrow(reference)?,
        kind,
    }))
}

/// One station reading as the rule sees it. `id` is the reading's commit id;
/// ties compare it in byte order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reading {
    /// Unix seconds, as claimed by the submitter.
    pub observed_at: i64,
    pub value: i64,
    pub id: String,
}

/// A station reading as the chain holds it: when it was observed and the
/// block it landed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainSample {
    pub id: String,
    pub observed_at: i64,
    pub value: i64,
    /// Height and time of the block the reading landed in.
    pub height: u64,
    pub block_time: i64,
    /// Quorum as of the closing block: the first block whose time is at or
    /// after target + [`SCORE_DELAY_SECS`], at the threshold in force there.
    pub has_quorum: bool,
    /// The reading's provenance class; `None` when none is recorded.
    pub provenance: Option<String>,
}

impl ChainSample {
    fn reading(&self) -> Reading {
        Reading {
            observed_at: self.observed_at,
            value: self.value,
            id: self.id.clone(),
        }
    }
}

/// Rule 3, the part that does not depend on where the reading landed: in
/// range, at quorum, and not of class `synthetic` (no class counts).
pub fn counts_as_reading(variable: ScoredVariable, sample: &ChainSample) -> bool {
    sample.has_quorum
        && sample.provenance.as_deref() != Some("synthetic")
        && variable.in_range(sample.value)
}

/// Rule 3b: the readings the truth may be picked from, in input order: landed
/// in a block whose time is in `[target − 15 min, target + 35 min)`.
pub fn truth_candidates(
    samples: &[ChainSample],
    variable: ScoredVariable,
    target: i64,
) -> Vec<Reading> {
    samples
        .iter()
        .filter(|s| {
            counts_as_reading(variable, s)
                && s.block_time >= target.saturating_sub(TRUTH_MAX_DISTANCE_SECS)
                && s.block_time < target.saturating_add(SCORE_DELAY_SECS)
        })
        .map(ChainSample::reading)
        .collect()
}

/// Rule 3c: the readings the window and `last` are built from, in input
/// order: landed below the commit height, in a block no older than 24 h
/// before the commit. A reading that landed at or after the commit never
/// counts, whatever observation time it claims.
pub fn window_candidates(
    samples: &[ChainSample],
    variable: ScoredVariable,
    commit_time: i64,
    commit_height: u64,
) -> Vec<Reading> {
    let start = commit_time.saturating_sub(WINDOW_HOURS as i64 * SECS_PER_HOUR);
    samples
        .iter()
        .filter(|s| {
            counts_as_reading(variable, s) && s.block_time >= start && s.height < commit_height
        })
        .map(ChainSample::reading)
        .collect()
}

/// Rule 4: the reading nearest `target`, at most 15 minutes away. Ties go to
/// the earlier reading, then the smaller id.
pub fn nearest_reading(readings: &[Reading], target: i64) -> Option<&Reading> {
    readings
        .iter()
        .filter(|r| r.observed_at.abs_diff(target) <= TRUTH_MAX_DISTANCE_SECS as u64)
        .min_by(|a, b| {
            a.observed_at
                .abs_diff(target)
                .cmp(&b.observed_at.abs_diff(target))
                .then(a.observed_at.cmp(&b.observed_at))
                .then(a.id.as_bytes().cmp(b.id.as_bytes()))
        })
}

/// Rule 5: the hourly samples of the 24 hours before `anchor` (the commit
/// time). Slot `k` covers `[anchor − (24 − k) h, anchor − (23 − k) h)` and
/// holds `sum / count` of its readings (truncated toward zero), or `None`.
pub fn hourly_window(
    readings: &[Reading],
    anchor: i64,
) -> Result<[Option<i64>; WINDOW_HOURS], LateScoreError> {
    let start = anchor
        .checked_sub(WINDOW_HOURS as i64 * SECS_PER_HOUR)
        .ok_or(LateScoreError::Overflow)?;
    let mut sums = [(0i128, 0i128); WINDOW_HOURS];
    for reading in readings {
        if reading.observed_at < start || reading.observed_at >= anchor {
            continue;
        }
        // 0 ≤ observed_at − start < 24 h, so the slot is 0..=23.
        let slot = ((reading.observed_at - start) / SECS_PER_HOUR) as usize;
        sums[slot].0 += i128::from(reading.value);
        sums[slot].1 += 1;
    }
    let mut window = [None; WINDOW_HOURS];
    for (slot, (sum, count)) in sums.into_iter().enumerate() {
        if count > 0 {
            window[slot] = Some(i64::try_from(sum / count).map_err(|_| LateScoreError::Overflow)?);
        }
    }
    Ok(window)
}

/// Rule 5: the newest reading in `[anchor − 24 h, anchor)`; ties: the
/// smaller id.
pub fn last_reading_before(readings: &[Reading], anchor: i64) -> Option<&Reading> {
    let start = anchor.checked_sub(WINDOW_HOURS as i64 * SECS_PER_HOUR)?;
    readings
        .iter()
        .filter(|r| r.observed_at >= start && r.observed_at < anchor)
        .min_by(|a, b| {
            b.observed_at
                .cmp(&a.observed_at)
                .then(a.id.as_bytes().cmp(b.id.as_bytes()))
        })
}

/// ADAPTER, NOT PART OF THE RULE AND NEVER ON CHAIN. Decimal text to fixed
/// point with two places, rounding half away from zero. Accepts exactly
/// `-?[0-9]{1,15}(\.[0-9]{1,20})?`; anything else (exponent, spaces, a bare
/// dot, a plus sign) is `None`.
pub fn parse_fixed_point(text: &str) -> Option<i64> {
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, fraction) = match body.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (body, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || whole.len() > 15 {
        return None;
    }
    let fraction = match fraction {
        Some(fraction) if digits(fraction) && fraction.len() <= 20 => fraction.as_bytes(),
        Some(_) => return None,
        None => &[],
    };
    let digit = |index: usize| i64::from(fraction.get(index).map_or(0, |b| b - b'0'));
    // At most 15 integer digits and two kept places: below 10^17, inside i64.
    let mut magnitude = whole.parse::<i64>().ok()?.checked_mul(100)?;
    magnitude = magnitude.checked_add(digit(0) * 10 + digit(1))?;
    if digit(2) >= 5 {
        magnitude = magnitude.checked_add(1)?;
    }
    Some(if negative { -magnitude } else { magnitude })
}

/// Hash that binds an observation attestation to the exact window used:
/// `SHA3-256(domain ‖ u32_be(24) ‖ per slot: 0x00, or 0x01 ‖ i64_be(sample))`.
pub fn window_hash(window: &[Option<i64>]) -> Result<[u8; 32], LateScoreError> {
    if window.len() != WINDOW_HOURS {
        return Err(LateScoreError::WindowLength);
    }
    let mut hasher = Sha3_256::new();
    hasher.update(WINDOW_HASH_DOMAIN);
    hasher.update((WINDOW_HOURS as u32).to_be_bytes());
    for slot in window {
        match slot {
            Some(sample) => {
                hasher.update([1u8]);
                hasher.update(sample.to_be_bytes());
            },
            None => hasher.update([0u8]),
        }
    }
    Ok(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// Vendored byte-identical from wrfcoin/backend commit
    /// 6b9af4b42cdc1c7b919003c16290e68db0c4615b,
    /// `contract-fixtures/forecast-skill-vectors.json`. Never edit it here.
    const VECTORS: &str = include_str!("../vectors/forecast-skill-vectors.json");
    const VECTORS_SHA256: &str = "447efda9512f8e339f0c122349676c455c6b58db605a9df0cf089930f9b49bc7";

    fn vectors() -> Value {
        serde_json::from_str(VECTORS).expect("vectors parse")
    }

    fn cases<'a>(doc: &'a Value, section: &str, expected: usize) -> &'a Vec<Value> {
        let cases = doc[section]
            .as_array()
            .unwrap_or_else(|| panic!("section {section}"));
        assert_eq!(cases.len(), expected, "section {section} changed size");
        cases
    }

    fn int(value: &Value) -> i64 {
        value
            .as_i64()
            .unwrap_or_else(|| panic!("integer expected, got {value}"))
    }

    fn variable(value: &Value) -> ScoredVariable {
        serde_json::from_value(value.clone()).expect("variable")
    }

    fn readings(value: &Value) -> Vec<Reading> {
        serde_json::from_value(value.clone()).expect("readings")
    }

    fn window(value: &Value) -> Vec<Option<i64>> {
        serde_json::from_value(value.clone()).expect("window")
    }

    #[test]
    fn the_vendored_file_is_the_pinned_one() {
        use sha2::{Digest as _, Sha256};
        assert_eq!(
            hex::encode(Sha256::digest(VECTORS.as_bytes())),
            VECTORS_SHA256
        );
        let doc = vectors();
        assert_eq!(doc["schema"], "wrfcoin-forecast-skill-vectors/v3");
        assert_eq!(doc["precision"], SKILL_PRECISION);
        assert_eq!(doc["default_tol"], DEFAULT_COPY_TOLERANCE_UNITS);
        assert_eq!(doc["min_window_samples"], MIN_WINDOW_SAMPLES);
        assert_eq!(doc["min_persistence_pairs"], MIN_PERSISTENCE_PAIRS);
        assert_eq!(doc["truth_max_distance_s"], TRUTH_MAX_DISTANCE_SECS);
        assert_eq!(doc["landing_margin_s"], LANDING_MARGIN_SECS);
        assert_eq!(doc["score_delay_s"], SCORE_DELAY_SECS);
        assert_eq!(doc["rule"].as_array().map(Vec::len), Some(15));
    }

    #[test]
    fn range_section() {
        let doc = vectors();
        for case in cases(&doc, "range", 11) {
            assert_eq!(
                variable(&case["variable"]).in_range(int(&case["value"])),
                case["expect"].as_bool().unwrap(),
                "{case}"
            );
        }
    }

    /// ON-CHAIN MATH. Every case goes through `late_skill`, the function
    /// consensus calls, range rule included.
    #[test]
    fn skill_section() {
        let doc = vectors();
        for case in cases(&doc, "skill", 47) {
            let got = late_skill(&LateScoreInput {
                variable: variable(&case["variable"]),
                forecast: int(&case["forecast"]),
                truth: int(&case["truth"]),
                reference: case["reference"].as_u64().unwrap(),
                mean24: int(&case["mean24"]),
                last: int(&case["last"]),
                tolerance: case["tol"].as_u64().unwrap(),
            })
            .unwrap_or_else(|e| panic!("{e}: {case}"));
            let expect = &case["expect"];
            assert_eq!(got.err, expect["err"].as_u64().unwrap(), "{case}");
            assert_eq!(
                got.copied.is_some(),
                expect["copy"].as_bool().unwrap(),
                "{case}"
            );
            assert_eq!(got.skill, expect["skill"].as_u64().unwrap(), "{case}");
            assert_eq!(
                got.beat_baseline(),
                expect["beat_baseline"].as_bool().unwrap(),
                "{case}"
            );
        }
    }

    /// Values near 2^31 are outside the range rule, so `late_skill` refuses
    /// them; the arithmetic alone must still be exact.
    #[test]
    fn skill_wide_arithmetic_section() {
        let doc = vectors();
        for case in cases(&doc, "skill_wide_arithmetic", 2) {
            let args = (
                int(&case["forecast"]),
                int(&case["truth"]),
                case["reference"].as_u64().unwrap(),
                int(&case["mean24"]),
                int(&case["last"]),
                case["tol"].as_u64().unwrap(),
            );
            let got = skill_arithmetic(args.0, args.1, args.2, args.3, args.4, args.5).unwrap();
            let expect = &case["expect"];
            assert_eq!(got.err, expect["err"].as_u64().unwrap(), "{case}");
            assert_eq!(
                got.copied.is_some(),
                expect["copy"].as_bool().unwrap(),
                "{case}"
            );
            assert_eq!(got.skill, expect["skill"].as_u64().unwrap(), "{case}");
            for variable in ScoredVariable::ALL {
                assert_eq!(
                    late_skill(&LateScoreInput {
                        variable,
                        forecast: args.0,
                        truth: args.1,
                        reference: args.2,
                        mean24: args.3,
                        last: args.4,
                        tolerance: args.5,
                    }),
                    Err(LateScoreError::ValueOutOfRange)
                );
            }
        }
    }

    #[test]
    fn reference_section() {
        let doc = vectors();
        for case in cases(&doc, "reference", 33) {
            let got = reference_from_window(
                &window(&case["window"]),
                case["lag"].as_u64().unwrap() as usize,
            )
            .unwrap_or_else(|e| panic!("{e}: {case}"));
            let expect: Option<WindowReference> =
                serde_json::from_value(case["expect"].clone()).expect("expect");
            assert_eq!(got, expect, "{case}");
        }
    }

    #[test]
    fn hourly_window_section() {
        let doc = vectors();
        for case in cases(&doc, "hourly_window", 7) {
            let got = hourly_window(&readings(&case["readings"]), int(&case["anchor"])).unwrap();
            assert_eq!(got.to_vec(), window(&case["expect"]), "{case}");
        }
    }

    #[test]
    fn nearest_reading_section() {
        let doc = vectors();
        for case in cases(&doc, "nearest_reading", 8) {
            let all = readings(&case["readings"]);
            let expect: Option<Reading> =
                serde_json::from_value(case["expect"].clone()).expect("expect");
            assert_eq!(
                nearest_reading(&all, int(&case["target"])),
                expect.as_ref(),
                "{case}"
            );
        }
    }

    #[test]
    fn last_reading_before_section() {
        let doc = vectors();
        for case in cases(&doc, "last_reading_before", 5) {
            let all = readings(&case["readings"]);
            let expect: Option<Reading> =
                serde_json::from_value(case["expect"].clone()).expect("expect");
            assert_eq!(
                last_reading_before(&all, int(&case["anchor"])),
                expect.as_ref(),
                "{case}"
            );
        }
    }

    /// Block time is carried per reading in the vectors; no test block is
    /// built from a height.
    #[test]
    fn rule3_scenarios_section() {
        let doc = vectors();
        for case in cases(&doc, "rule3_scenarios", 10) {
            let samples: Vec<ChainSample> =
                serde_json::from_value(case["readings"].clone()).expect("samples");
            let variable = variable(&case["variable"]);
            let (commit_time, target) = (int(&case["commit_time"]), int(&case["target"]));
            assert_eq!(
                target,
                commit_time + int(&case["lead_hours"]) * SECS_PER_HOUR,
                "rule 2: {case}"
            );
            let ids =
                |readings: &[Reading]| readings.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
            let strings = |value: &Value| -> Vec<String> {
                serde_json::from_value(value.clone()).expect("ids")
            };
            let expect = &case["expect"];

            let truth = truth_candidates(&samples, variable, target);
            assert_eq!(ids(&truth), strings(&expect["truth_candidates"]), "{case}");
            assert_eq!(
                nearest_reading(&truth, target).map(|r| r.id.as_str()),
                expect["truth"].as_str(),
                "{case}"
            );
            let in_window = window_candidates(
                &samples,
                variable,
                commit_time,
                case["commit_height"].as_u64().unwrap(),
            );
            assert_eq!(
                ids(&in_window),
                strings(&expect["window_candidates"]),
                "{case}"
            );
            assert_eq!(
                last_reading_before(&in_window, commit_time).map(|r| r.id.as_str()),
                expect["last"].as_str(),
                "{case}"
            );
            assert_eq!(
                hourly_window(&in_window, commit_time).unwrap().to_vec(),
                window(&expect["window"]),
                "{case}"
            );
        }
    }

    /// `fixed_point` is the backend's adapter, not part of the rule and not
    /// on chain. It is covered here so oracle tooling in Rust converts the
    /// same way; consensus never calls `parse_fixed_point`.
    #[test]
    fn fixed_point_section_adapter_only() {
        let doc = vectors();
        for case in cases(&doc, "fixed_point", 37) {
            assert_eq!(
                parse_fixed_point(case["text"].as_str().unwrap()),
                case["expect"].as_i64(),
                "{case}"
            );
        }
    }

    #[test]
    fn every_section_of_the_file_is_run() {
        let doc = vectors();
        let mut arrays: Vec<&str> = doc
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, v)| v.is_array())
            .map(|(k, _)| k.as_str())
            .collect();
        arrays.sort_unstable();
        assert_eq!(
            arrays,
            [
                "fixed_point",
                "hourly_window",
                "last_reading_before",
                "nearest_reading",
                "range",
                "reference",
                "rule",
                "rule3_scenarios",
                "skill",
                "skill_wide_arithmetic",
            ],
            "a new section needs a test"
        );
    }

    #[test]
    fn late_skill_refuses_out_of_range_inputs() {
        let base = LateScoreInput {
            variable: ScoredVariable::TemperatureC,
            forecast: 2_000,
            truth: 2_100,
            reference: 300,
            mean24: 1_500,
            last: 1_700,
            tolerance: 5,
        };
        assert!(late_skill(&base).is_ok());
        for bad in [
            LateScoreInput {
                forecast: 10_001,
                ..base
            },
            LateScoreInput {
                truth: -10_001,
                ..base
            },
            LateScoreInput {
                mean24: i64::MIN,
                ..base
            },
            LateScoreInput {
                last: i64::MAX,
                ..base
            },
        ] {
            assert_eq!(late_skill(&bad), Err(LateScoreError::ValueOutOfRange));
        }
        assert_eq!(
            late_skill(&LateScoreInput {
                reference: MAX_REFERENCE + 1,
                ..base
            }),
            Err(LateScoreError::ReferenceOutOfRange)
        );
        assert!(
            late_skill(&LateScoreInput {
                reference: MAX_REFERENCE,
                ..base
            })
            .is_ok()
        );
        assert_eq!(
            late_skill(&LateScoreInput {
                tolerance: MAX_COPY_TOLERANCE + 1,
                ..base
            }),
            Err(LateScoreError::ToleranceOutOfRange)
        );
        // A temperature is not a pressure.
        assert_eq!(
            late_skill(&LateScoreInput {
                variable: ScoredVariable::PressureHpa,
                ..base
            }),
            Err(LateScoreError::ValueOutOfRange)
        );
    }

    #[test]
    fn extreme_i64_inputs_never_overflow() {
        for (forecast, truth) in [
            (i64::MIN, i64::MAX),
            (i64::MAX, i64::MIN),
            (i64::MAX, i64::MAX),
        ] {
            let got = skill_arithmetic(forecast, truth, u64::MAX, 0, 0, 0).unwrap();
            assert!(got.skill <= SKILL_PRECISION);
        }
        let mut window = [Some(i64::MAX); WINDOW_HOURS];
        for slot in window.iter_mut().step_by(2) {
            *slot = Some(i64::MIN);
        }
        // Checked 128-bit arithmetic: an answer or a clean error, never a panic.
        let _ = reference_from_window(&window, 1);
        let in_range = [Some(120_000i64), Some(30_000)].repeat(12);
        let reference = reference_from_window(&in_range, 1).unwrap().unwrap();
        assert_eq!(
            (reference.spread, reference.persistence),
            (45_000, Some(90_000))
        );
        assert_eq!(reference.reference, 45_000);
    }

    #[test]
    fn skill_never_rises_as_the_error_grows() {
        for reference in [1u64, 2, 3, 7, 100, 19_997, 20_000] {
            let mut previous = SKILL_PRECISION;
            for err in 0..=40i64 {
                let score = skill_arithmetic(err, 0, reference, -9_000, -9_000, 0).unwrap();
                assert!(score.skill <= previous, "ref {reference} err {err}");
                previous = score.skill;
            }
        }
    }

    #[test]
    fn isqrt_is_the_floor_root() {
        for n in 0u128..2_000 {
            let r = isqrt_u128(n);
            assert!(r * r <= n && (r + 1) * (r + 1) > n, "n {n}");
        }
        for n in [
            u128::from(u64::MAX),
            u128::MAX,
            (1u128 << 100) - 1,
            1u128 << 100,
        ] {
            let r = isqrt_u128(n);
            assert!(r.checked_mul(r).is_some_and(|sq| sq <= n), "n {n}");
            assert!((r + 1).checked_mul(r + 1).is_none_or(|sq| sq > n), "n {n}");
        }
    }

    #[test]
    fn window_hash_separates_empty_slots_from_zero() {
        let empty = [None; WINDOW_HOURS];
        let mut zero = empty;
        zero[0] = Some(0);
        let mut moved = empty;
        moved[1] = Some(0);
        let hashes = [
            window_hash(&empty).unwrap(),
            window_hash(&zero).unwrap(),
            window_hash(&moved).unwrap(),
        ];
        assert!(hashes[0] != hashes[1] && hashes[1] != hashes[2] && hashes[0] != hashes[2]);
        assert_eq!(window_hash(&empty[..23]), Err(LateScoreError::WindowLength));
    }
}
