// Integration test: ForecastAccuracyMetrics::from_pairs → ForecastVerificationReceipt::validate
//
// Run from $HOME/wrfcoin/core4:
//   cargo test --test forecast_integration

use wrfcoin_forecast_score::{
    ForecastAccuracyMetrics, ForecastReceiptValidationError, ForecastVerificationReceipt, Pair,
};

fn valid_manifest_digest() -> String {
    "a".repeat(64)
}

#[test]
fn from_pairs_to_receipt_validates_ok() {
    // Step 1: Build metrics from known pairs (observed temp vs predicted temp, in °C)
    let pairs = [
        Pair { predicted: 20.0, observed: 21.0 },
        Pair { predicted: 25.0, observed: 24.0 },
        Pair { predicted: 18.0, observed: 19.0 },
        Pair { predicted: 30.0, observed: 29.0 },
    ];
    let metrics = ForecastAccuracyMetrics::from_pairs(&pairs);

    // Sanity-check: mae/rmse ≥ 0, skill_score in [0,1]
    assert!(metrics.mae >= 0.0, "mae must be non-negative");
    assert!(metrics.rmse >= 0.0, "rmse must be non-negative");
    assert!(metrics.skill_score >= 0.0 && metrics.skill_score <= 1.0);

    // Step 2: Build a receipt embedding those metrics
    let receipt = ForecastVerificationReceipt {
        manifest_digest: valid_manifest_digest(),
        commit_id: "commit-abc123".to_string(),
        oracle_id: "oracle-xyz".to_string(),
        metrics,
        verified_at: 1_700_000_200,
        nonce: "nonce-42".to_string(),
        zk_proof: None,
        signature: None,
    };

    // Step 3: validate() must return Ok
    assert!(receipt.validate().is_ok(), "receipt from from_pairs should validate");

    // Step 4: mutate skill_score below 0 → validate must return Err
    let mut bad = receipt.clone();
    bad.metrics.skill_score = -0.1;
    let err = bad.validate().expect_err("negative skill_score must be rejected");
    assert!(
        matches!(
            err,
            ForecastReceiptValidationError::InvalidMetrics(_)
        ),
        "expected InvalidMetrics variant, got {:?}",
        err
    );
}

#[test]
fn from_pairs_empty_gives_all_zeros() {
    let metrics = ForecastAccuracyMetrics::from_pairs(&[]);
    assert_eq!(metrics.mae, 0.0);
    assert_eq!(metrics.rmse, 0.0);
    assert_eq!(metrics.bias, 0.0);
    assert_eq!(metrics.skill_score, 0.0);
}

#[test]
fn from_pairs_perfect_forecast_skill_is_one() {
    let pairs = [
        Pair { predicted: 10.0, observed: 10.0 },
        Pair { predicted: 20.0, observed: 20.0 },
        Pair { predicted: 30.0, observed: 30.0 },
    ];
    let metrics = ForecastAccuracyMetrics::from_pairs(&pairs);
    assert_eq!(metrics.mae, 0.0);
    assert_eq!(metrics.rmse, 0.0);
    assert_eq!(metrics.skill_score, 1.0);
}
