//! Station coordinates at the public 0.1 degree grid.
//!
//! Node API reads give station coordinates at 0.1 degree to every caller
//! except the node operator; the Operator token gets full precision. This is
//! presentation only: chain state keeps what it stores, and nothing here is
//! part of a transaction, a preimage or the state root.
//!
//! The rounding rule is the website API's, so the site and the node agree on
//! every station: work in whole units of 1e-7 degree (the registry's
//! fixed-point form) and round to the nearest whole tenth of a degree, a tie
//! going UP (towards +infinity), as JavaScript `Math.round` does. So 0.05
//! becomes 0.1 and -0.05 becomes 0.0 (never -0.0). Rust's `f64::round` would
//! send -0.05 to -0.1 instead, which is why it is not used here.

/// 0.1 degree in units of 1e-7 degree.
pub const E7_UNITS_PER_TENTH_DEGREE: i64 = 1_000_000;

/// The whole tenth of a degree a coordinate falls in, as an integer count of
/// tenths (`46.8` is `468`). `None` for a non-finite or out-of-range value.
// determinism-ok: read-surface rounding of a registry coordinate for an API response; never consensus arithmetic
pub fn tenth_degree_cell(value: f64) -> Option<i64> {
    // determinism-ok: as above
    if !value.is_finite() || value.abs() > 360.0 {
        return None;
    }
    // determinism-ok: as above; the registry holds integer e7 values, so this recovers them exactly
    let units = (value * 1e7).round() as i64;
    Some((units + E7_UNITS_PER_TENTH_DEGREE / 2).div_euclid(E7_UNITS_PER_TENTH_DEGREE))
}

/// A coordinate in degrees on the public 0.1 degree grid.
// determinism-ok: read-surface rounding of a registry coordinate for an API response; never consensus arithmetic
pub fn tenth_degree(value: f64) -> Option<f64> {
    // determinism-ok: as above; `+ 0.0` turns a -0.0 into 0.0
    tenth_degree_cell(value).map(|tenths| tenths as f64 / 10.0 + 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // All coordinates here are made up.

    #[test]
    fn rounds_to_the_nearest_tenth() {
        assert_eq!(tenth_degree(10.123_456_7), Some(10.1));
        assert_eq!(tenth_degree(10.150_000_1), Some(10.2));
        assert_eq!(tenth_degree(-20.654_321), Some(-20.7));
        assert_eq!(tenth_degree(-20.6), Some(-20.6));
        assert_eq!(tenth_degree(0.0), Some(0.0));
    }

    #[test]
    fn a_tie_goes_up_like_the_website() {
        // Website: Math.round(units / 1e6) * 1e6.
        assert_eq!(tenth_degree(10.05), Some(10.1));
        assert_eq!(tenth_degree(-10.05), Some(-10.0));
        assert_eq!(tenth_degree(0.05), Some(0.1));
        let minus_tie = tenth_degree(-0.05).unwrap();
        assert_eq!(minus_tie, 0.0);
        assert!(minus_tie.is_sign_positive(), "never -0.0");
    }

    #[test]
    fn integer_e7_inputs_round_exactly() {
        // The registry stores latitude_e7 / 1e7; the cell must come from the integer.
        for (e7, cell) in [
            (104_999_999_i64, 105),
            (105_000_000, 105),
            (105_500_000, 106),
            (-105_500_000, -105),
        ] {
            assert_eq!(tenth_degree_cell(e7 as f64 / 1e7), Some(cell), "e7 {e7}");
        }
    }

    #[test]
    fn non_finite_is_none() {
        assert_eq!(tenth_degree(f64::NAN), None);
        assert_eq!(tenth_degree(f64::INFINITY), None);
    }
}
