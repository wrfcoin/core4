// Pipeline snapshot tests for forecast + sensor accuracy.
//
// These tests pin exact numeric outputs from the scoring + reward math,
// catching integration regressions before release candidates.
//
// Run from $HOME/wrfcoin/core4:
//   cargo test -p wrfcoin-shared-protocol --test pipeline_snapshot_tests -- --nocapture

use wrfcoin_forecast_score::{
    ForecastAccuracyMetrics, ForecastArtifactManifest, ForecastVerificationReceipt,
};
use wrfcoin_meteo_types::{DataQualityBand, DataQualityPolicy, DataQualityScore};
use wrfcoin_shared_protocol::consensus::hyper_local::compose_reward_with_multipliers;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute bias = mean(predicted - observed).
fn compute_bias(observed: &[f64], predicted: &[f64]) -> f64 {
    let n = observed.len() as f64;
    predicted
        .iter()
        .zip(observed.iter())
        .map(|(p, o)| p - o)
        .sum::<f64>()
        / n
}

/// Compute skill_score = (1 - rmse / std_obs).clamp(0, 1).
fn compute_skill_score(observed: &[f64], rmse: f64) -> f64 {
    let n = observed.len() as f64;
    let mean_obs = observed.iter().sum::<f64>() / n;
    let variance = observed.iter().map(|o| (o - mean_obs).powi(2)).sum::<f64>() / n;
    let std_obs = variance.sqrt();
    if std_obs > 0.0 {
        (1.0 - rmse / std_obs).clamp(0.0, 1.0)
    } else if rmse == 0.0 {
        1.0
    } else {
        0.0
    }
}

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-10
}

// ---------------------------------------------------------------------------
// Scenario 1: Perfect Forecast Snapshot
// ---------------------------------------------------------------------------

#[test]
fn perfect_forecast_snapshot() {
    // 10 pairs where predicted == observed (temps 15.0–24.0°C, 1°C increments)
    let observed: Vec<f64> = (15..=24).map(|t| t as f64).collect();
    let predicted = observed.clone();

    let mae = ForecastAccuracyMetrics::mae_slices(&observed, &predicted).unwrap();
    let rmse = ForecastAccuracyMetrics::rmse_slices(&observed, &predicted).unwrap();
    let bias = compute_bias(&observed, &predicted);
    let skill_score = compute_skill_score(&observed, rmse);

    // Hand-computed: zero error in every pair → all metrics zero, skill perfect
    assert_eq!(mae, 0.0, "mae should be exactly 0");
    assert_eq!(rmse, 0.0, "rmse should be exactly 0");
    assert_eq!(bias, 0.0, "bias should be exactly 0");
    assert_eq!(skill_score, 1.0, "skill_score should be 1.0 (perfect)");

    let metrics = ForecastAccuracyMetrics {
        mae,
        rmse,
        bias,
        skill_score,
    };
    assert!(metrics.validate().is_ok());
}

// ---------------------------------------------------------------------------
// Scenario 2: Mediocre Forecast Snapshot
// ---------------------------------------------------------------------------

#[test]
fn mediocre_forecast_snapshot() {
    // 10 realistic temp pairs
    let observed = [15.0, 17.0, 19.0, 21.0, 23.0, 25.0, 22.0, 18.0, 16.0, 20.0];
    let predicted = [14.0, 18.0, 20.5, 21.0, 24.0, 27.0, 23.0, 17.0, 15.0, 22.0];
    // Errors (pred - obs): -1, +1, +1.5, 0, +1, +2, +1, -1, -1, +2
    // |errors|:             1,  1,  1.5,  0,  1,  2,  1,  1,  1,  2

    // Hand-computed:
    // sum|err| = 1+1+1.5+0+1+2+1+1+1+2 = 11.5
    // mae = 11.5 / 10 = 1.15
    let expected_mae = 11.5 / 10.0;

    // sum(err^2) = 1+1+2.25+0+1+4+1+1+1+4 = 16.25
    // mse = 16.25 / 10 = 1.625
    // rmse = sqrt(1.625) ≈ 1.27475487...
    let expected_rmse = (16.25_f64 / 10.0).sqrt();

    // sum(pred-obs) = -1+1+1.5+0+1+2+1-1-1+2 = 5.5
    // bias = 5.5 / 10 = 0.55
    let expected_bias = 5.5 / 10.0;

    // mean_obs = (15+17+19+21+23+25+22+18+16+20) / 10 = 196/10 = 19.6
    // var_obs = sum((obs-19.6)^2) / 10
    //   (15-19.6)^2 = 21.16
    //   (17-19.6)^2 = 6.76
    //   (19-19.6)^2 = 0.36
    //   (21-19.6)^2 = 1.96
    //   (23-19.6)^2 = 11.56
    //   (25-19.6)^2 = 29.16
    //   (22-19.6)^2 = 5.76
    //   (18-19.6)^2 = 2.56
    //   (16-19.6)^2 = 12.96
    //   (20-19.6)^2 = 0.16
    // sum = 92.4
    // var = 92.4/10 = 9.24
    // std_obs = sqrt(9.24) ≈ 3.03974...
    let std_obs = (92.4_f64 / 10.0).sqrt();
    // skill = (1 - 1.27475487 / 3.03974).clamp(0,1) ≈ 0.58066...
    let expected_skill = (1.0 - expected_rmse / std_obs).clamp(0.0, 1.0);

    let mae = ForecastAccuracyMetrics::mae_slices(&observed, &predicted).unwrap();
    let rmse = ForecastAccuracyMetrics::rmse_slices(&observed, &predicted).unwrap();
    let bias = compute_bias(&observed, &predicted);
    let skill_score = compute_skill_score(&observed, rmse);

    assert!(approx_eq(mae, expected_mae), "mae: {mae} != {expected_mae}");
    assert!(
        approx_eq(rmse, expected_rmse),
        "rmse: {rmse} != {expected_rmse}"
    );
    assert!(
        approx_eq(bias, expected_bias),
        "bias: {bias} != {expected_bias}"
    );
    assert!(
        approx_eq(skill_score, expected_skill),
        "skill: {skill_score} != {expected_skill}"
    );

    let metrics = ForecastAccuracyMetrics {
        mae,
        rmse,
        bias,
        skill_score,
    };
    assert!(metrics.validate().is_ok());
}

// ---------------------------------------------------------------------------
// Scenario 3: Bad Forecast Snapshot
// ---------------------------------------------------------------------------

#[test]
fn bad_forecast_snapshot() {
    // 10 pairs with large systematic errors (+10 to +20°C)
    let observed = [15.0, 17.0, 19.0, 21.0, 23.0, 25.0, 22.0, 18.0, 16.0, 20.0];
    let predicted = [25.0, 32.0, 34.0, 38.0, 40.0, 45.0, 37.0, 33.0, 31.0, 36.0];
    // Errors (pred - obs): 10, 15, 15, 17, 17, 20, 15, 15, 15, 16
    // |errors|: same (all positive)

    // Hand-computed:
    // sum|err| = 10+15+15+17+17+20+15+15+15+16 = 155
    // mae = 155 / 10 = 15.5
    let expected_mae = 155.0 / 10.0;

    // sum(err^2) = 100+225+225+289+289+400+225+225+225+256 = 2459
    // rmse = sqrt(2459/10) = sqrt(245.9) ≈ 15.68117...
    let expected_rmse = (2459.0_f64 / 10.0).sqrt();

    // bias = 155/10 = 15.5
    let expected_bias = 155.0 / 10.0;

    // std_obs = sqrt(9.24) ≈ 3.03974... (same obs as scenario 2)
    // skill = (1 - 15.68117 / 3.03974).clamp(0,1) = clamp(-4.16..., 0, 1) = 0.0
    let expected_skill = 0.0;

    let mae = ForecastAccuracyMetrics::mae_slices(&observed, &predicted).unwrap();
    let rmse = ForecastAccuracyMetrics::rmse_slices(&observed, &predicted).unwrap();
    let bias = compute_bias(&observed, &predicted);
    let skill_score = compute_skill_score(&observed, rmse);

    assert!(approx_eq(mae, expected_mae), "mae: {mae} != {expected_mae}");
    assert!(
        approx_eq(rmse, expected_rmse),
        "rmse: {rmse} != {expected_rmse}"
    );
    assert!(
        approx_eq(bias, expected_bias),
        "bias: {bias} != {expected_bias}"
    );
    assert_eq!(
        skill_score, expected_skill,
        "skill must clamp to 0.0 when RMSE >> std_obs"
    );

    let metrics = ForecastAccuracyMetrics {
        mae,
        rmse,
        bias,
        skill_score,
    };
    assert!(metrics.validate().is_ok());
}

// ---------------------------------------------------------------------------
// Scenario 4: Barometer Data Quality Evaluation Snapshot
// ---------------------------------------------------------------------------

#[test]
fn barometer_data_quality_evaluation_snapshot() {
    // Simulate barometer accuracy as quality scores, then classify via policy.
    // 10 sensors with overall quality scores from barometer readings.
    // High-quality sensors (score >= 0.90): full rewards
    // Borderline sensors (score >= 0.70): reduced rewards
    // Warning-only sensors (score >= 0.50): zero rewards
    // Low-quality sensors (score < 0.50): slashing-eligible

    let policy = DataQualityPolicy::default();
    // Verify default thresholds we rely on:
    assert_eq!(policy.high_quality_threshold, 0.90);
    assert_eq!(policy.min_acceptance_threshold, 0.70);
    assert_eq!(policy.warning_only_threshold, 0.50);

    // 10 barometer quality scores: realistic range for pressure sensors
    let quality_scores: [f64; 10] = [0.95, 0.92, 0.88, 0.85, 0.78, 0.72, 0.65, 0.55, 0.42, 0.30];
    let expected_bands = [
        DataQualityBand::High,       // 0.95 >= 0.90
        DataQualityBand::High,       // 0.92 >= 0.90
        DataQualityBand::Borderline, // 0.88 >= 0.70
        DataQualityBand::Borderline, // 0.85 >= 0.70
        DataQualityBand::Borderline, // 0.78 >= 0.70
        DataQualityBand::Borderline, // 0.72 >= 0.70
        DataQualityBand::WarningOnly, // 0.65 >= 0.50
        DataQualityBand::WarningOnly, // 0.55 >= 0.50
        DataQualityBand::Low,        // 0.42 < 0.50
        DataQualityBand::Low,        // 0.30 < 0.50
    ];

    for (i, &score_val) in quality_scores.iter().enumerate() {
        let score = DataQualityScore::try_from(score_val).unwrap();
        let eval = policy.evaluate(Some(&score));

        assert_eq!(
            eval.band, expected_bands[i],
            "sensor {i} (score={score_val}): expected {:?}, got {:?}",
            expected_bands[i], eval.band
        );

        // Verify multiplier invariants
        match eval.band {
            DataQualityBand::High => {
                assert_eq!(eval.scoring_multiplier, 1.0);
                assert_eq!(eval.reward_multiplier, 1.0);
                assert!(!eval.trigger_challenge);
                assert!(!eval.counts_toward_low_quality_streak);
            }
            DataQualityBand::Borderline => {
                // Hand-computed from defaults: borderline_scoring_multiplier = 0.70
                assert_eq!(eval.scoring_multiplier, 0.70);
                assert_eq!(eval.reward_multiplier, 0.50);
                assert!(!eval.trigger_challenge);
            }
            DataQualityBand::WarningOnly => {
                assert_eq!(eval.scoring_multiplier, 0.0);
                assert_eq!(eval.reward_multiplier, 0.0);
                assert!(eval.warning_only);
                assert!(eval.trigger_challenge);
            }
            DataQualityBand::Low => {
                assert_eq!(eval.scoring_multiplier, 0.0);
                assert_eq!(eval.reward_multiplier, 0.0);
                assert!(eval.trigger_challenge);
                assert!(eval.counts_toward_low_quality_streak);
            }
        }
    }

    // Verify counts: 2 High, 4 Borderline, 2 WarningOnly, 2 Low
    let high_count = expected_bands.iter().filter(|b| **b == DataQualityBand::High).count();
    let borderline_count = expected_bands
        .iter()
        .filter(|b| **b == DataQualityBand::Borderline)
        .count();
    let warning_count = expected_bands
        .iter()
        .filter(|b| **b == DataQualityBand::WarningOnly)
        .count();
    let low_count = expected_bands.iter().filter(|b| **b == DataQualityBand::Low).count();
    assert_eq!(high_count, 2);
    assert_eq!(borderline_count, 4);
    assert_eq!(warning_count, 2);
    assert_eq!(low_count, 2);
}

// ---------------------------------------------------------------------------
// Scenario 5: Receipt + Manifest Serialization Roundtrip
// ---------------------------------------------------------------------------

#[test]
fn receipt_serialization_roundtrip() {
    // Use mediocre metrics from Scenario 2
    let metrics = ForecastAccuracyMetrics {
        mae: 1.15,
        rmse: (16.25_f64 / 10.0).sqrt(),
        bias: 0.55,
        skill_score: (1.0 - (16.25_f64 / 10.0).sqrt() / (92.4_f64 / 10.0).sqrt()).clamp(0.0, 1.0),
    };
    assert!(metrics.validate().is_ok());

    let manifest = ForecastArtifactManifest {
        schema_version: 1,
        model_id: "wrf-gfs-v3".to_string(),
        input_hash: "a".repeat(64),
        output_hash: "b".repeat(64),
        produced_at: 1_700_000_000,
        region: "us-east".to_string(),
        horizon_hours: 48,
        input_variables: vec!["humidity".to_string(), "pressure".to_string()],
        output_variables: vec!["precipitation".to_string(), "temperature".to_string()],
    };
    assert!(manifest.validate().is_ok());

    let receipt = ForecastVerificationReceipt {
        manifest_digest: manifest.manifest_digest(),
        commit_id: "commit-abc123".to_string(),
        oracle_id: "oracle-weather-east".to_string(),
        metrics: metrics.clone(),
        verified_at: 1_700_003_600,
        nonce: "nonce-12345".to_string(),
        zk_proof: None,
        signature: None,
    };
    assert!(receipt.validate().is_ok());

    // Serialize both to JSON
    let manifest_json = serde_json::to_string_pretty(&manifest).unwrap();
    let receipt_json = serde_json::to_string_pretty(&receipt).unwrap();

    // Deserialize back and assert structural equality.
    // f64 fields may lose a ULP during JSON roundtrip, so compare them with tolerance.
    let manifest_back: ForecastArtifactManifest =
        serde_json::from_str(&manifest_json).unwrap();
    let receipt_back: ForecastVerificationReceipt =
        serde_json::from_str(&receipt_json).unwrap();

    assert_eq!(manifest, manifest_back);

    assert_eq!(receipt_back.manifest_digest, receipt.manifest_digest);
    assert_eq!(receipt_back.commit_id, receipt.commit_id);
    assert_eq!(receipt_back.oracle_id, receipt.oracle_id);
    assert_eq!(receipt_back.verified_at, receipt.verified_at);
    assert_eq!(receipt_back.nonce, receipt.nonce);
    assert_eq!(receipt_back.zk_proof, receipt.zk_proof);
    assert_eq!(receipt_back.signature, receipt.signature);
    // Metrics: compare with f64 tolerance (JSON roundtrip may lose a ULP)
    assert!(approx_eq(receipt_back.metrics.mae, receipt.metrics.mae));
    assert!(approx_eq(receipt_back.metrics.rmse, receipt.metrics.rmse));
    assert!(approx_eq(receipt_back.metrics.bias, receipt.metrics.bias));
    assert!(approx_eq(receipt_back.metrics.skill_score, receipt.metrics.skill_score));

    // Pin key JSON field names (not full snapshot — avoids brittle ordering)
    assert!(manifest_json.contains("\"schema_version\""));
    assert!(manifest_json.contains("\"model_id\""));
    assert!(manifest_json.contains("\"input_hash\""));
    assert!(manifest_json.contains("\"output_hash\""));
    assert!(manifest_json.contains("\"horizon_hours\""));
    assert!(manifest_json.contains("\"input_variables\""));
    assert!(manifest_json.contains("\"output_variables\""));

    assert!(receipt_json.contains("\"manifest_digest\""));
    assert!(receipt_json.contains("\"commit_id\""));
    assert!(receipt_json.contains("\"oracle_id\""));
    assert!(receipt_json.contains("\"metrics\""));
    assert!(receipt_json.contains("\"verified_at\""));
    assert!(receipt_json.contains("\"nonce\""));
    assert!(receipt_json.contains("\"mae\""));
    assert!(receipt_json.contains("\"skill_score\""));

    // #2922: this pinned the OPPOSITE of what it should have. `zk_proof`/
    // `signature` carried `skip_serializing_if`, which is safe under JSON —
    // it just omits the key — but the same attribute made this receipt
    // undecodable over bincode, the format it actually crosses a peer
    // boundary with (`native-chain/src/bin/launch_api/p2p_events.rs` via
    // `bincode::deserialize`): omitting a field on serialize left a
    // positional decoder expecting a byte that was never written. Fixed by
    // dropping `skip_serializing_if`, which is JSON-neutral (`None` now
    // serializes as `null` instead of an absent key; either deserializes
    // back to `None`, asserted above) and bincode-correct (the `Option` tag
    // byte is always written). So the key is now always PRESENT in JSON.
    assert!(receipt_json.contains("\"zk_proof\""));
    assert!(receipt_json.contains("\"signature\""));
}

// ---------------------------------------------------------------------------
// Scenario 6: Reward Multiplier Composition
// ---------------------------------------------------------------------------

#[test]
fn reward_multiplier_composition() {
    // Test 1: desert_mult=150 (1.5x), hyper_local_mult=12_000 (1.2x)
    // Hand-computed: 1_000_000 * 150 / 100 = 1_500_000
    //               1_500_000 * 12_000 / 10_000 = 1_800_000
    let result = compose_reward_with_multipliers(1_000_000, 150, 12_000);
    assert_eq!(result, 1_800_000, "1.5x desert * 1.2x hyper-local");

    // Test 2: no bonus (identity multipliers)
    // desert_mult=100 (1x), hyper_local_mult=10_000 (1x)
    // Hand-computed: 1_000_000 * 100 / 100 = 1_000_000
    //               1_000_000 * 10_000 / 10_000 = 1_000_000
    let result = compose_reward_with_multipliers(1_000_000, 100, 10_000);
    assert_eq!(result, 1_000_000, "identity multipliers yield base reward");

    // Test 3: double desert bonus, no hyper-local
    // desert_mult=200 (2x), hyper_local_mult=10_000 (1x)
    // Hand-computed: 500_000 * 200 / 100 = 1_000_000
    //               1_000_000 * 10_000 / 10_000 = 1_000_000
    let result = compose_reward_with_multipliers(500_000, 200, 10_000);
    assert_eq!(result, 1_000_000, "2x desert doubles the base reward");

    // Test 4: no desert bonus, 1.5x hyper-local
    // desert_mult=100 (1x), hyper_local_mult=15_000 (1.5x)
    // Hand-computed: 2_000_000 * 100 / 100 = 2_000_000
    //               2_000_000 * 15_000 / 10_000 = 3_000_000
    let result = compose_reward_with_multipliers(2_000_000, 100, 15_000);
    assert_eq!(result, 3_000_000, "1.5x hyper-local only");

    // Test 5: skill_score scales emission into effective reward
    // emission = 10_000_000, skill_score = 0.75, oracle_quality = 0.90
    // effective_reward = floor(10_000_000 * 0.75 * 0.90) = floor(6_750_000) = 6_750_000
    // Then compose: desert_mult=100, hyper_local_mult=10_000 → same 6_750_000
    let emission: u128 = 10_000_000;
    let skill_score = 0.75;
    let oracle_quality = 0.90;
    let effective_reward = (emission as f64 * skill_score * oracle_quality) as u128;
    assert_eq!(effective_reward, 6_750_000);

    let final_reward = compose_reward_with_multipliers(effective_reward, 100, 10_000);
    assert_eq!(
        final_reward, 6_750_000,
        "skill-scaled reward with identity multipliers"
    );

    // Test 6: compose with skill-scaled reward and both multipliers
    // 6_750_000 * 150 / 100 = 10_125_000
    // 10_125_000 * 12_000 / 10_000 = 12_150_000
    let final_reward = compose_reward_with_multipliers(effective_reward, 150, 12_000);
    assert_eq!(
        final_reward, 12_150_000,
        "skill-scaled reward with 1.5x desert + 1.2x hyper-local"
    );
}
