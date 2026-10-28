//! `forecast-score-batch`: score a batch of revealed forecasts and apply the
//! per-cell/lead cap, for the oracle scorer job (`tools/forecast-kit/scorer_job.py`).
//!
//! Reads one JSON request on stdin, writes one JSON response on stdout. It signs
//! nothing and reads no network or environment: the job owns the gates.
//!
//! Request:
//! ```json
//! {"skill_definition": "rmse_sigma" | "mae_clim",
//!  "cap_per_cell_lead": 3, "baseline": 0.1,
//!  "forecasts": [{"commit_id": "..", "forecaster": "0x..", "cell": "station-id",
//!                 "horizon_hours": 6, "valid_at": 1700000000,
//!                 "variables": {"temperature_c": {"forecast": 1.0, "truth": 1.2,
//!                                                 "window": [..]}}}]}
//! ```
//! `skill_definition` is REQUIRED; there is no default.

use std::collections::BTreeMap;
use std::io::Read;

use serde::{Deserialize, Serialize};
use wrfcoin_forecast_score::skill::{
    CapCandidate, CapDecision, ScoredForecast, SkillDefinition, VariableInput,
    score_forecast, select_capped,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    skill_definition: SkillDefinition,
    cap_per_cell_lead: usize,
    baseline: f64, // determinism-ok: off-chain scorer input
    forecasts: Vec<ForecastIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ForecastIn {
    commit_id: String,
    forecaster: String,
    cell: String,
    horizon_hours: u32,
    valid_at: i64,
    variables: BTreeMap<String, VariableInput>,
}

#[derive(Serialize)]
struct ForecastOut {
    commit_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    score: Option<ScoredForecast>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    decision: Option<CapDecision>,
}

fn main() {
    let mut raw = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut raw) {
        eprintln!("forecast-score-batch: cannot read stdin: {e}");
        std::process::exit(2);
    }
    let request: Request = match serde_json::from_str(&raw) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("forecast-score-batch: invalid request: {e}");
            std::process::exit(2);
        },
    };
    let mut outs = Vec::with_capacity(request.forecasts.len());
    let mut candidates = Vec::new();
    for f in &request.forecasts {
        match score_forecast(&f.variables, request.skill_definition) {
            Ok(score) => {
                candidates.push(CapCandidate {
                    commit_id: f.commit_id.clone(),
                    forecaster: f.forecaster.clone(),
                    cell: f.cell.clone(),
                    horizon_hours: f.horizon_hours,
                    valid_at: f.valid_at,
                    skill: score.metrics.skill_score,
                });
                outs.push(ForecastOut { commit_id: f.commit_id.clone(), score: Some(score), error: None, decision: None });
            },
            Err(e) => outs.push(ForecastOut {
                commit_id: f.commit_id.clone(),
                score: None,
                error: Some(e.to_string()),
                decision: None,
            }),
        }
    }
    let decisions: BTreeMap<String, CapDecision> =
        select_capped(&candidates, request.cap_per_cell_lead, request.baseline)
            .into_iter()
            .map(|d| (d.commit_id.clone(), d))
            .collect();
    for out in &mut outs {
        out.decision = decisions.get(&out.commit_id).cloned();
    }
    let response = serde_json::json!({
        "skill_definition": request.skill_definition,
        "forecasts": outs,
    });
    println!("{}", serde_json::to_string_pretty(&response).expect("serializable"));
}
