use super::*;
use skill::SkillPolicy;
use std::collections::BTreeMap;

fn selected_score(
    forecast: &ForecastOutput,
    truth: &Observation,
    climatology: &BTreeMap<String, f64>,
) -> ScoreReport {
    let mut previous = observation(4.0);
    previous.timestamp = forecast.issued_at;
    skill::score(
        forecast,
        truth,
        climatology,
        &previous,
        SkillPolicy::Persistence,
    )
    .unwrap()
}

fn observation(value: f64) -> Observation {
    Observation {
        timestamp: 1_700_021_600,
        values: FORECAST_VARIABLES
            .iter()
            .map(|v| (v.to_string(), value))
            .collect(),
    }
}

fn forecast(value: f64) -> ForecastOutput {
    ForecastOutput {
        model_id: MODEL_ID.to_string(),
        issued_at: 1_700_000_000,
        valid_at: 1_700_021_600,
        horizon_hours: HORIZON_HOURS,
        station_id: "synthetic-station".to_string(),
        values: observation(value).values,
    }
}

#[test]
fn persistence_copy_adds_no_skill() {
    // Legacy window mean=0, last observation=4, truth=6: a persistence
    // copy earns 2/3 under the window mean but no skill over persistence.
    let climatology = observation(0.0).values;
    let report = selected_score(&forecast(4.0), &observation(6.0), &climatology);
    assert_eq!(report.metrics.skill_score, 0.0);
}

#[test]
fn perfect_forecast_tied_with_persistence_adds_no_skill() {
    let climatology = observation(4.0).values;
    let report = selected_score(&forecast(4.0), &observation(4.0), &climatology);
    assert_eq!(report.metrics.skill_score, 0.0);
}

#[test]
fn genuine_improvement_known_answer() {
    let climatology = observation(0.0).values;
    let report = selected_score(&forecast(5.0), &observation(6.0), &climatology);
    assert_eq!(report.metrics.skill_score, 0.5);
}

#[test]
fn off_mode_keeps_legacy_report_bytes_even_with_an_unusable_seed() {
    let f = forecast(5.0);
    let truth = observation(6.0);
    let climate = observation(0.0).values;
    let seed = Observation {
        timestamp: truth.timestamp,
        values: BTreeMap::new(),
    };
    assert_eq!(
        serde_json::to_vec(
            &skill::score(&f, &truth, &climate, &seed, SkillPolicy::Legacy).unwrap()
        )
        .unwrap(),
        serde_json::to_vec(&score(&f, &truth, &climate)).unwrap(),
    );
}

#[test]
fn feature_view_uses_exact_names_and_chain_identity() {
    let mut view = serde_json::json!({"height":99,"chain_id":"777","network_id":"testnet"});
    assert_eq!(
        SkillPolicy::from_pending(&view, "777", "testnet").unwrap(),
        SkillPolicy::Legacy
    );
    for name in [
        "FORECAST_SKILL_BASELINE",
        "forecast_skill_baseline_extra",
        " forecast_skill_baseline",
    ] {
        view["active_features"] = serde_json::json!([name]);
        assert_eq!(
            SkillPolicy::from_pending(&view, "777", "testnet").unwrap(),
            SkillPolicy::Legacy
        );
    }
    view["active_features"] = serde_json::json!(["forecast_skill_baseline"]);
    assert_eq!(
        SkillPolicy::from_pending(&view, "777", "testnet").unwrap(),
        SkillPolicy::Persistence
    );
    assert!(SkillPolicy::from_pending(&view, "another-chain", "testnet").is_err());
    view["height"] = serde_json::json!(-1);
    assert!(SkillPolicy::from_pending(&view, "777", "testnet").is_err());
}

#[test]
fn feature_activation_view_switches_only_at_the_synthetic_height() {
    use wrfcoin_shared_protocol::crypto::chain_binding::{ConsensusFeature, FeatureSchedule};
    let feature = ConsensusFeature::ForecastSkillBaseline;
    let schedule = FeatureSchedule::from_entries(vec![(feature.name().to_string(), 100)]).unwrap();
    for height in [99, 100, 101] {
        let active: Vec<_> = ConsensusFeature::ALL
            .iter()
            .filter(|f| schedule.active(**f, height))
            .map(|f| f.name())
            .collect();
        let view = serde_json::json!({"height":height-1,"chain_id":"777","network_id":"testnet","active_features":active});
        assert_eq!(
            SkillPolicy::from_pending(&view, "777", "testnet").unwrap(),
            if height < 100 {
                SkillPolicy::Legacy
            } else {
                SkillPolicy::Persistence
            }
        );
    }
}

#[test]
fn active_mode_retains_continuous_metrics_and_does_not_mix_units() {
    let mut f = forecast(5.0);
    let mut truth = observation(6.0);
    let mut seed = observation(4.0);
    seed.timestamp = f.issued_at;
    f.values.insert("pressure_hpa".into(), 500.0);
    truth.values.insert("pressure_hpa".into(), 600.0);
    seed.values.insert("pressure_hpa".into(), 400.0);
    let climate = observation(0.0).values;
    let legacy = score(&f, &truth, &climate);
    let active = skill::score(&f, &truth, &climate, &seed, SkillPolicy::Persistence).unwrap();
    assert_eq!(
        (active.metrics.mae, active.metrics.rmse, active.metrics.bias),
        (legacy.metrics.mae, legacy.metrics.rmse, legacy.metrics.bias)
    );
    assert_eq!(active.metrics.skill_score, 0.5);
}

#[test]
fn active_mode_refuses_missing_nonfinite_overflow_and_unordered_seed() {
    let f = forecast(5.0);
    let truth = observation(6.0);
    let climate = observation(0.0).values;
    let mut seed = observation(4.0);
    seed.timestamp = f.issued_at;
    let evaluate = |f: &ForecastOutput, t: &Observation, s: &Observation| {
        skill::score(f, t, &climate, s, SkillPolicy::Persistence)
    };
    let mut invalid = seed.clone();
    invalid.values.clear();
    assert!(evaluate(&f, &truth, &invalid).is_err());
    invalid = seed.clone();
    invalid.timestamp = truth.timestamp;
    assert!(evaluate(&f, &truth, &invalid).is_err());
    for value in [f64::NAN, f64::INFINITY, f64::MAX] {
        assert!(evaluate(&forecast(value), &truth, &seed).is_err());
        assert!(evaluate(&f, &observation(value), &seed).is_err());
    }
}

#[test]
fn active_mode_refuses_aggregate_overflow_from_finite_variable_errors() {
    let f = forecast(0.0);
    let truth = observation(1e154);
    let climate = observation(0.0).values;
    let mut seed = observation(0.0);
    seed.timestamp = f.issued_at;
    let legacy = score(&f, &truth, &climate);
    assert!(legacy.per_variable.values().all(|value| {
        value.abs_error.is_finite() && (value.abs_error * value.abs_error).is_finite()
    }));
    assert!(legacy.metrics.rmse.is_infinite());
    assert!(skill::score(&f, &truth, &climate, &seed, SkillPolicy::Persistence).is_err());
}
