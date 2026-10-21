// PWS accuracy integration tests for the Ambient Weather pipeline.
//
// Uses the reference station’s WS-5000 (Duluth, MN) realistic readings paired against
// plausible NWS KDLH hourly observations to verify that the meteo-types
// SensorAccuracyMetrics pipeline produces correct bands and scores.
//
// Run from $HOME/wrfcoin/core4:
//   cargo test -p wrfcoin-shared-protocol --test pws_accuracy_integration -- --nocapture

use wrfcoin_meteo_types::{
    DataQualityBand, IoTAccuracyError, SensorAccuracyBand, SensorAccuracyMetrics,
    SensorReferencePair,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn make_pairs(sensor: &[f64], reference: &[f64]) -> Vec<SensorReferencePair> {
    sensor
        .iter()
        .zip(reference.iter())
        .map(|(&s, &r)| SensorReferencePair::new(s, r))
        .collect()
}

// ---------------------------------------------------------------------------
// Test 1: Temperature accuracy from realistic Jon WS-5000 data
// ---------------------------------------------------------------------------

#[test]
fn pws_temperature_accuracy_from_realistic_data() {
    // the reference station’s WS-5000 readings (°C) over 12 hours (diurnal cycle, hourly)
    let pws_temps = [2.1, 1.8, 1.5, 1.2, 3.5, 6.8, 9.2, 11.5, 12.8, 12.1, 9.6, 6.2];

    // NWS KDLH hourly observations (rounded to nearest 0.5–1.0°C as typical for METAR)
    let nws_temps = [2.0, 1.5, 1.0, 1.0, 3.0, 7.0, 9.0, 12.0, 13.0, 12.0, 10.0, 6.0];

    let pairs = make_pairs(&pws_temps, &nws_temps);
    let metrics = SensorAccuracyMetrics::from_pairs(&pairs).unwrap();

    assert!(metrics.validate().is_ok());
    assert_eq!(metrics.sample_count, 12);

    // MAE should be in a reasonable range for a well-sited PWS vs NWS
    assert!(metrics.mae > 0.05, "MAE too low: {}", metrics.mae);
    assert!(metrics.mae < 1.0, "MAE too high: {}", metrics.mae);

    // Band should be Good or Excellent for a quality station with full diurnal range
    let band = metrics.band();
    assert!(
        band == SensorAccuracyBand::Good || band == SensorAccuracyBand::Excellent,
        "expected Good or Excellent, got {:?} (accuracy_score={})",
        band,
        metrics.accuracy_score,
    );
}

// ---------------------------------------------------------------------------
// Test 2: Pressure accuracy — WS-5000 barometers are very precise
// ---------------------------------------------------------------------------

#[test]
fn pws_pressure_accuracy_excellent_band() {
    // Frontal passage scenario — pressure drops 11 hPa over 12 hours (strong system)
    // PWS readings are slightly offset from NWS (typical for consumer barometer)
    let pws_pressure = [
        988.2, 987.1, 986.1, 985.2, 984.1, 983.2, 982.1, 981.1, 980.2, 979.1, 978.2, 977.1,
    ];

    // NWS KDLH hourly pressure (hPa) — steady decline
    let nws_pressure = [
        988.0, 987.0, 986.0, 985.0, 984.0, 983.0, 982.0, 981.0, 980.0, 979.0, 978.0, 977.0,
    ];

    let pairs = make_pairs(&pws_pressure, &nws_pressure);
    let metrics = SensorAccuracyMetrics::from_pairs(&pairs).unwrap();

    assert!(metrics.validate().is_ok());
    assert_eq!(metrics.sample_count, 12);

    // Barometric pressure errors should be very small (< 0.5 hPa MAE)
    assert!(metrics.mae < 0.5, "pressure MAE too high: {}", metrics.mae);
    assert!(metrics.rmse < 0.5, "pressure RMSE too high: {}", metrics.rmse);

    // Band should be Excellent or Good — WS-5000 barometers track very well
    let band = metrics.band();
    assert!(
        band == SensorAccuracyBand::Excellent || band == SensorAccuracyBand::Good,
        "expected Excellent or Good for pressure, got {:?} (accuracy_score={})",
        band,
        metrics.accuracy_score,
    );
}

// ---------------------------------------------------------------------------
// Test 3: Empty pairs returns ZeroSampleCount error
// ---------------------------------------------------------------------------

#[test]
fn pws_accuracy_empty_pairs_returns_error() {
    let result = SensorAccuracyMetrics::from_pairs(&[]);
    assert_eq!(result, Err(IoTAccuracyError::ZeroSampleCount));
}

// ---------------------------------------------------------------------------
// Test 4: Humidity with constant bias — verify bias and correlation
// ---------------------------------------------------------------------------

#[test]
fn pws_humidity_accuracy_with_constant_bias() {
    // the reference station’s humidity readings (%)
    let pws_humidity: Vec<f64> = vec![56.0, 56.0, 58.0, 57.0, 55.0, 56.0, 55.0, 57.0, 56.0, 56.0, 55.0, 55.0];

    // NWS reference with -3% systematic offset (NWS sensor reads lower)
    let nws_humidity: Vec<f64> = vec![53.0, 53.0, 55.0, 54.0, 52.0, 53.0, 52.0, 54.0, 53.0, 53.0, 52.0, 52.0];

    let pairs = make_pairs(&pws_humidity, &nws_humidity);
    let metrics = SensorAccuracyMetrics::from_pairs(&pairs).unwrap();

    assert!(metrics.validate().is_ok());
    assert_eq!(metrics.sample_count, 12);

    // Bias should be exactly +3.0 (constant offset)
    assert!(
        approx_eq(metrics.bias, 3.0),
        "expected bias ≈ 3.0, got {}",
        metrics.bias,
    );

    // MAE should equal the absolute bias for a constant offset
    assert!(
        approx_eq(metrics.mae, 3.0),
        "expected MAE ≈ 3.0, got {}",
        metrics.mae,
    );

    // Correlation should be 1.0 (perfect linear relationship with offset)
    assert!(
        approx_eq(metrics.correlation, 1.0),
        "expected correlation ≈ 1.0, got {}",
        metrics.correlation,
    );
}

// ---------------------------------------------------------------------------
// Test 5: Band boundaries map to correct quality bands
// ---------------------------------------------------------------------------

#[test]
fn sensor_accuracy_band_maps_to_quality_band() {
    // Excellent: accuracy_score >= 0.90
    assert_eq!(SensorAccuracyBand::from_score(0.95), SensorAccuracyBand::Excellent);
    assert_eq!(
        SensorAccuracyBand::from_score(0.95).to_quality_band(),
        DataQualityBand::High,
    );

    // Good: 0.75 <= accuracy_score < 0.90
    assert_eq!(SensorAccuracyBand::from_score(0.80), SensorAccuracyBand::Good);
    assert_eq!(
        SensorAccuracyBand::from_score(0.80).to_quality_band(),
        DataQualityBand::Borderline,
    );

    // Marginal: 0.50 <= accuracy_score < 0.75
    assert_eq!(SensorAccuracyBand::from_score(0.60), SensorAccuracyBand::Marginal);
    assert_eq!(
        SensorAccuracyBand::from_score(0.60).to_quality_band(),
        DataQualityBand::WarningOnly,
    );

    // Poor: accuracy_score < 0.50
    assert_eq!(SensorAccuracyBand::from_score(0.30), SensorAccuracyBand::Poor);
    assert_eq!(
        SensorAccuracyBand::from_score(0.30).to_quality_band(),
        DataQualityBand::Low,
    );

    // Exact boundary values
    assert_eq!(SensorAccuracyBand::from_score(0.90), SensorAccuracyBand::Excellent);
    assert_eq!(SensorAccuracyBand::from_score(0.75), SensorAccuracyBand::Good);
    assert_eq!(SensorAccuracyBand::from_score(0.50), SensorAccuracyBand::Marginal);
}

// ---------------------------------------------------------------------------
// Test 6: Single pair edge case — zero variance in reference
// ---------------------------------------------------------------------------

#[test]
fn pws_accuracy_single_pair_edge_case() {
    // Single pair: std_ref = 0, so skill-score depends on rmse
    let pairs = [SensorReferencePair::new(5.2, 5.0)];
    let metrics = SensorAccuracyMetrics::from_pairs(&pairs).unwrap();

    assert!(metrics.validate().is_ok());
    assert_eq!(metrics.sample_count, 1);

    // Single pair with non-zero error → std_ref = 0, rmse > 0 → accuracy_score = 0.0
    assert!(
        approx_eq(metrics.accuracy_score, 0.0),
        "expected accuracy_score = 0.0 for single pair with error, got {}",
        metrics.accuracy_score,
    );
    assert_eq!(metrics.band(), SensorAccuracyBand::Poor);

    // Single pair with zero error → rmse = 0, std_ref = 0 → accuracy_score = 1.0
    let perfect = [SensorReferencePair::new(5.0, 5.0)];
    let m2 = SensorAccuracyMetrics::from_pairs(&perfect).unwrap();
    assert!(
        approx_eq(m2.accuracy_score, 1.0),
        "expected accuracy_score = 1.0 for perfect single pair, got {}",
        m2.accuracy_score,
    );
    assert_eq!(m2.band(), SensorAccuracyBand::Excellent);
}
