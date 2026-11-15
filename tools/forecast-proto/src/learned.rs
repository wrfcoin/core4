//! Optional CPU inference. The bundled model is synthetic-trained, not for payouts.
use super::Observation;
use anyhow::{Result, bail, ensure};
use std::{collections::BTreeMap, path::Path, sync::atomic::{AtomicBool, Ordering}};
use tract_onnx::prelude::*;

pub const MODEL_ID: &str = "learned-conv1d/v1";
// Trainer order is intentionally distinct from the sorted manifest order.
const VARIABLES: [&str; 3] = ["temperature_c", "pressure_hpa", "humidity_pct"];
static FALLBACK_LOGGED: AtomicBool = AtomicBool::new(false);

fn infer(path: &Path, window: &[Observation]) -> Result<BTreeMap<String, f64>> {
    ensure!(window.len() >= 24, "learned model needs 24 hourly observations");
    let window = &window[window.len() - 24..];
    ensure!(window.windows(2).all(|w| w[1].timestamp.checked_sub(w[0].timestamp) == Some(3600)),
            "learned model needs consecutive hourly observations");
    let mut input = Vec::with_capacity(72);
    for observation in window {
        for variable in VARIABLES {
            let value = *observation.values.get(variable)
                .ok_or_else(|| anyhow::anyhow!("missing input variable {variable}"))? as f32;
            ensure!(value.is_finite(), "non-finite learned input");
            input.push(value);
        }
    }
    let graph = tract_onnx::onnx().model_for_path(path)?;
    ensure!(graph.input_outlets()?.len() == 1 && graph.output_outlets()?.len() == 1,
            "expected one model input and one output");
    let graph = graph.with_input_fact(0, f32::fact([1, 24, 3]).into())?
        .into_optimized()?.into_runnable()?;
    let tensor = Tensor::from_shape(&[1, 24, 3], &input)?;
    let outputs = graph.run(tvec!(tensor.into()))?;
    let values = outputs[0].to_plain_array_view::<f32>()?;
    ensure!(values.shape() == [1, 3], "expected learned output shape [1,3]");
    ensure!(values.iter().all(|v| v.is_finite()), "non-finite learned output");
    Ok(VARIABLES.into_iter().zip(values.iter()).map(|(name, value)| (name.to_string(), *value as f64)).collect())
}

pub fn select_forecast(
    requested: &str,
    path: Option<&Path>,
    window: &[Observation],
    persistence: BTreeMap<String, f64>,
) -> (&'static str, BTreeMap<String, f64>) {
    if requested == MODEL_ID {
        // Model loading is optional. Even an unexpected runtime-library panic
        // must not prevent the persistence forecast from being produced.
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match path {
                Some(path) => infer(path, window),
                None => bail!("no ONNX path configured"),
            }
        }));
        let reason = match attempt {
            Ok(Ok(values)) => return (MODEL_ID, values),
            Ok(Err(error)) => error.to_string(),
            Err(_) => "ONNX runtime panicked".to_string(),
        };
        if !FALLBACK_LOGGED.swap(true, Ordering::Relaxed) {
            eprintln!("learned model unavailable ({reason}); using {}", super::MODEL_ID);
        }
    }
    (super::MODEL_ID, persistence)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Vec<Observation>, Vec<f64>) {
        let report: serde_json::Value = serde_json::from_str(include_str!("../models/placeholder-export.json")).unwrap();
        let window = report["input"][0].as_array().unwrap().iter().enumerate().map(|(i, row)| Observation {
            timestamp: 1_700_000_000 + i as i64 * 3600,
            values: VARIABLES.iter().enumerate().map(|(j, name)| (name.to_string(), row[j].as_f64().unwrap())).collect(),
        }).collect();
        let expected = report["expected_output"][0].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        (window, expected)
    }

    fn model_path() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models/synthetic-placeholder.onnx"))
    }

    /// The observed-data model's parity record: `(window, expected_output, tolerance)`.
    /// Written by `gpu/train_forecast.py --observations ... --export-onnx`; the tolerance
    /// is the record's own `rust_parity_tolerance_abs`.
    fn observed_fixture() -> (Vec<Observation>, Vec<f64>, f64) {
        let report: serde_json::Value = serde_json::from_str(include_str!("../models/learned-conv1d-v1-export.json")).unwrap();
        let window = report["input"][0].as_array().unwrap().iter().enumerate().map(|(i, row)| Observation {
            timestamp: 1_700_000_000 + i as i64 * 3600,
            values: VARIABLES.iter().enumerate().map(|(j, name)| (name.to_string(), row[j].as_f64().unwrap())).collect(),
        }).collect();
        let expected = report["expected_output"][0].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let tolerance = report["rust_parity_tolerance_abs"].as_f64().unwrap();
        (window, expected, tolerance)
    }

    fn observed_model_path() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/models/learned-conv1d-v1.onnx"))
    }

    #[test]
    fn observed_data_onnx_matches_cpu_export() {
        // Same contract as the placeholder: raw units in, raw units out, one
        // CPU prediction from the exporting process reproduced by tract here.
        let (window, expected, tolerance) = observed_fixture();
        let (id, values) = select_forecast(MODEL_ID, Some(observed_model_path()), &window, BTreeMap::new());
        assert_eq!(id, MODEL_ID, "test must execute ONNX, not fallback");
        assert_eq!(values.len(), 3);
        for (name, expected) in VARIABLES.iter().zip(expected) {
            assert!(values[*name].is_finite());
            assert!((values[*name] - expected).abs() < tolerance, "{name}: {} vs {expected} (tol {tolerance})", values[*name]);
        }
    }

    #[test]
    fn observed_data_onnx_is_level_invariant() {
        // The graph centres each window on its own mean, so a constant offset
        // on the pressure input (station pressure vs sea-level pressure) moves
        // the pressure forecast by exactly that offset and nothing else.
        let (window, _, tolerance) = observed_fixture();
        let shifted: Vec<Observation> = window.iter().map(|o| {
            let mut values = o.values.clone();
            *values.get_mut("pressure_hpa").unwrap() -= 50.0;
            Observation { timestamp: o.timestamp, values }
        }).collect();
        let base = select_forecast(MODEL_ID, Some(observed_model_path()), &window, BTreeMap::new());
        let moved = select_forecast(MODEL_ID, Some(observed_model_path()), &shifted, BTreeMap::new());
        assert_eq!((base.0, moved.0), (MODEL_ID, MODEL_ID));
        assert!((base.1["pressure_hpa"] - 50.0 - moved.1["pressure_hpa"]).abs() < tolerance);
        for name in ["temperature_c", "humidity_pct"] {
            assert!((base.1[name] - moved.1[name]).abs() < tolerance, "{name} must not depend on the pressure level");
        }
    }

    #[test]
    fn onnx_fixed_window_matches_cpu_export() {
        let (window, expected) = fixture();
        let (id, values) = select_forecast(MODEL_ID, Some(model_path()), &window, BTreeMap::new());
        assert_eq!(id, MODEL_ID, "test must execute ONNX, not fallback");
        assert_eq!(values.len(), 3);
        for (name, expected) in VARIABLES.iter().zip(expected) {
            assert!(values[*name].is_finite());
            assert!((values[*name] - expected).abs() < 0.005, "{name}: {} vs {expected}", values[*name]);
        }
    }

    #[test]
    fn absent_path_preserves_persistence_and_identity() {
        let (window, _) = fixture();
        let fallback = BTreeMap::from([("temperature_c".to_string(), 12.0)]);
        for path in [None, Some(model_path().join("missing")).as_deref()] {
            let (id, values) = select_forecast(MODEL_ID, path, &window, fallback.clone());
            assert_eq!(id, super::super::MODEL_ID);
            assert_eq!(values, fallback);
        }
    }

    #[test]
    fn bad_graph_and_irregular_window_fall_back() {
        let (mut window, _) = fixture();
        let invalid = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
        assert_eq!(select_forecast(MODEL_ID, Some(invalid), &window, BTreeMap::new()).0, super::super::MODEL_ID);
        window[1].timestamp += 1;
        assert_eq!(select_forecast(MODEL_ID, Some(model_path()), &window, BTreeMap::new()).0, super::super::MODEL_ID);
    }

    #[test]
    fn persistence_selection_does_not_need_a_model_or_window() {
        let fallback = BTreeMap::from([("temperature_c".to_string(), 12.0)]);
        assert_eq!(select_forecast(super::super::MODEL_ID, None, &[], fallback.clone()), (super::super::MODEL_ID, fallback));
    }
}
