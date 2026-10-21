# Determinism baseline triage — full appendix (all 726 findings)

> Generated for #2244 from `scripts/determinism_baseline.json` as of PR #2245 branch
> (`feat/2105-determinism-lint` @ `ebd7de33c`), cross-checked against a live
> `check_determinism.py --report` run on `main`: 798 raw line hits collapse to exactly
> the 726 frozen fingerprints (0 drift, 0 new). Line numbers are from the live run on
> `main` at the time of triage; the baseline itself is line-independent
> (`path|class|text` fingerprints). See `BASELINE_TRIAGE.md` for the analysis.

Bucket key: **CC** consensus-critical · **ND** needs-decision · **DL** display/logging/metrics · **TO** test-only · **UW** unwired library code


## `shared-protocol/src/consensus/proof_of_publishing.rs` — 44 (CC×38, ND×2, DL×1, TO×3)

- `shared-protocol/src/consensus/proof_of_publishing.rs:73` **CC** [float-type] `const BASELINE_FORECAST_SKILL: f64 = 0.1;`
- `shared-protocol/src/consensus/proof_of_publishing.rs:73` **CC** [float-literal] `const BASELINE_FORECAST_SKILL: f64 = 0.1;`
- `shared-protocol/src/consensus/proof_of_publishing.rs:85` **CC** [wall-clock] `chrono::Utc::now()`
- `shared-protocol/src/consensus/proof_of_publishing.rs:254` **ND** [env-var] `let raw = std::env::var("WRFCOIN_STATE_PRUNING_RETENTION_BLOCKS").ok();`
- `shared-protocol/src/consensus/proof_of_publishing.rs:255` **ND** [env-var] `let raw_ack = std::env::var(ALLOW_NONDEFAULT_RETENTION_ENV).ok();`
- `shared-protocol/src/consensus/proof_of_publishing.rs:987` **CC** [float-type] `percentages: std::collections::BTreeMap<PublishingRole, f64>,`
- `shared-protocol/src/consensus/proof_of_publishing.rs:996` **CC** [float-type] `pub fn set_region_weights(&mut self, weights: FxHashMap<String, f64>) {`
- `shared-protocol/src/consensus/proof_of_publishing.rs:1001` **CC** [float-type] `pub fn set_accuracy_multiplier(&mut self, multiplier: f64) {`
- `shared-protocol/src/consensus/proof_of_publishing.rs:1007` **CC** [float-type] `pub fn set_reputation_multiplier(&mut self, multiplier: f64) {`
- `shared-protocol/src/consensus/proof_of_publishing.rs:2005` **CC** [float-literal] `Self::quantize_f64(attestation.confidence.unwrap_or(1.0).clamp(0.0, 1.0));`
- `shared-protocol/src/consensus/proof_of_publishing.rs:2180` **CC** [float-type] `let contribution_score = contribution_score_mu as f64 / SCORE_PRECISION as f64;`
- `shared-protocol/src/consensus/proof_of_publishing.rs:2185` **CC** [float-literal] `.unwrap_or(1.0);`
- `shared-protocol/src/consensus/proof_of_publishing.rs:2200` **CC** [float-literal] `.clamp(0.0, 1.0);`
- `shared-protocol/src/consensus/proof_of_publishing.rs:2206` **CC** [float-type] `let rep_factor = (raw_rep as f64 / SCORE_SCALE as f64).clamp(0.1, 2.0);`
- `shared-protocol/src/consensus/proof_of_publishing.rs:2206` **CC** [float-literal] `let rep_factor = (raw_rep as f64 / SCORE_SCALE as f64).clamp(0.1, 2.0);`
- `shared-protocol/src/consensus/proof_of_publishing.rs:2497` **CC** [float-literal] `(receipt.metrics.skill_score.clamp(0.0, 1.0) * 10_000.0) as u32,`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3178` **CC** [float-type] `Self::quantize_f64((nft.computation_complexity as f64).min(10.0) / 10.0);`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3178` **CC** [float-literal] `Self::quantize_f64((nft.computation_complexity as f64).min(10.0) / 10.0);`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3235` **CC** [float-literal] `let accuracy = Self::quantize_f64(*metrics.get("accuracy").unwrap_or(&0.5));`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3236` **CC** [float-literal] `let complexity = Self::quantize_f64(*metrics.get("complexity").unwrap_or(&0.5));`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3237` **CC** [float-literal] `let usefulness = Self::quantize_f64(*metrics.get("usefulness").unwrap_or(&0.5));`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3248` **CC** [float-literal] `let acc_raw = *metrics.get("accuracy").unwrap_or(&0.5);`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3274` **CC** [float-literal] `let quality = Self::quantize_f64(*metrics.get("quality").unwrap_or(&0.5));`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3275` **CC** [float-literal] `let volume = Self::quantize_f64(*metrics.get("volume").unwrap_or(&0.5));`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3276` **CC** [float-literal] `let uniqueness = Self::quantize_f64(*metrics.get("uniqueness").unwrap_or(&0.5));`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3300` **CC** [float-type] `metrics: &FxHashMap<String, f64>,`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3308` **CC** [float-literal] `Self::quantize_f64(metrics.get("assignment_utility").copied().unwrap_or(0.0));`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3318` **CC** [float-literal] `.unwrap_or(0.0)`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3319` **CC** [float-literal] `.clamp(0.0, 1.0),`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3364` **CC** [float-type] `fn multidomain_weight_for_tx(&self, tx: &Transaction) -> Option<f64> {`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3374` **CC** [float-literal] `Some(policy.soil_weight.max(0.0))`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3376` **CC** [float-literal] `Some(0.0)`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3381` **CC** [float-literal] `Some(policy.ocean_weight.max(0.0))`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3388` **CC** [float-literal] `Some(policy.hydrology_weight.max(0.0))`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3565` **CC** [float-type] `fn extract_metrics(&self, tx: &Transaction) -> Option<FxHashMap<String, f64>> {`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3580` **CC** [float-literal] `let amount = Self::quantize_f64(*metrics.get("amount").unwrap_or(&0.5));`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3581` **CC** [float-literal] `let verification = Self::quantize_f64(*metrics.get("verification").unwrap_or(&0.5));`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3582` **CC** [float-literal] `let impact = Self::quantize_f64(*metrics.get("impact").unwrap_or(&0.5));`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3595` **CC** [float-type] `fn parse_metrics(&self, data: &str) -> Result<FxHashMap<String, f64>> {`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3596` **CC** [float-type] `let metrics: FxHashMap<String, f64> =`
- `shared-protocol/src/consensus/proof_of_publishing.rs:3820` **DL** [float-type] `let ratio_display = elapsed_secs as f64 / expected_secs as f64;`
- `shared-protocol/src/consensus/proof_of_publishing.rs:8677` **TO** [float-literal] `serde_json::json!({ "quality": 0.20 }),`
- `shared-protocol/src/consensus/proof_of_publishing.rs:8760` **TO** [float-literal] `serde_json::json!({ "temperature": 21.0, "humidity": 0.42 }),`
- `shared-protocol/src/consensus/proof_of_publishing.rs:8768` **TO** [float-literal] `serde_json::json!({ "temperature": 22.0, "humidity": 0.40 }),`

## `consensus-engines/src/factory.rs` — 3 (CC×3)

- `consensus-engines/src/factory.rs:212` **CC** [float-literal] `serde_json::Value::Number(serde_json::Number::from_f64(0.6).unwrap()),`
- `consensus-engines/src/factory.rs:310` **CC** [float-literal] `if !(0.0..=1.0).contains(&score) {`
- `consensus-engines/src/factory.rs:312` **CC** [float-literal] `"PoP min_publication_score must be between 0.0 and 1.0".to_string(),`

## `shared-protocol/src/consensus/data_desert.rs` — 3 (CC×3)

- `shared-protocol/src/consensus/data_desert.rs:12` **CC** [float-type] `pub fn cell_for_station(lat: f64, lon: f64) -> GeoCell {`
- `shared-protocol/src/consensus/data_desert.rs:50` **CC** [float-type] `station_lat: f64,`
- `shared-protocol/src/consensus/data_desert.rs:51` **CC** [float-type] `station_lon: f64,`

## `consensus-engines/src/algorithms/pop_adapter.rs` — 2 (CC×2)

- `consensus-engines/src/algorithms/pop_adapter.rs:341` **CC** [float-type] `if !scaled.is_finite() \|\| scaled < 0.0 \|\| scaled > u64::MAX as f64 {`
- `consensus-engines/src/algorithms/pop_adapter.rs:341` **CC** [float-literal] `if !scaled.is_finite() \|\| scaled < 0.0 \|\| scaled > u64::MAX as f64 {`

## `shared-protocol/src/consensus/forecast_verification.rs` — 2 (CC×2)

- `shared-protocol/src/consensus/forecast_verification.rs:12` **CC** [float-type] `pub skill_score: f64,`
- `shared-protocol/src/consensus/forecast_verification.rs:47` **CC** [float-literal] `skill_score: 0.0,`

## `consensus-engines/src/ffi.rs` — 5 (ND×5)

- `consensus-engines/src/ffi.rs:71` **ND** [float-literal] `avg_block_time: 10.0,`
- `consensus-engines/src/ffi.rs:72` **ND** [float-literal] `tx_throughput: 0.0,`
- `consensus-engines/src/ffi.rs:73` **ND** [float-literal] `network_latency_ms: 0.0,`
- `consensus-engines/src/ffi.rs:74` **ND** [float-literal] `consensus_participation: 0.0,`
- `consensus-engines/src/ffi.rs:329` **ND** [float-literal] `commission_rate: 0.0,`

## `consensus-engines/src/engine.rs` — 3 (DL×3)

- `consensus-engines/src/engine.rs:60` **DL** [float-type] `avg_block_time: config.target_block_time as f64,`
- `consensus-engines/src/engine.rs:61` **DL** [float-literal] `tx_throughput: 0.0,`
- `consensus-engines/src/engine.rs:65` **DL** [float-literal] `network_latency_ms: 100.0,`

## `consensus-engines/src/traits.rs` — 3 (DL×3)

- `consensus-engines/src/traits.rs:105` **DL** [float-type] `pub avg_block_time: f64,`
- `consensus-engines/src/traits.rs:108` **DL** [float-type] `pub tx_throughput: f64,`
- `consensus-engines/src/traits.rs:120` **DL** [float-type] `pub network_latency_ms: f64,`

## `consensus-engines/src/coordination/adaptive_fault_tolerance.rs` — 78 (UW×78)

- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:29` **UW** [wall-clock] `SystemTime::now()`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:46` **UW** [float-type] `density: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:176` **UW** [float-type] `pub topology_adaptation_threshold: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:192` **UW** [float-literal] `topology_adaptation_threshold: 0.2,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:202` **UW** [float-type] `pub health_score: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:203` **UW** [float-type] `pub response_time: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:204` **UW** [float-type] `pub error_rate: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:205` **UW** [float-type] `pub cpu_usage: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:206` **UW** [float-type] `pub memory_usage: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:207` **UW** [float-type] `pub connectivity_score: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:217` **UW** [float-type] `pub avg_response_time: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:218` **UW** [float-type] `pub throughput: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:219` **UW** [float-type] `pub recovery_time: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:220` **UW** [float-type] `pub resource_efficiency: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:221` **UW** [float-type] `pub network_overhead: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:222` **UW** [float-type] `pub finality_time: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:230` **UW** [float-literal] `density: 0.7,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:234` **UW** [float-literal] `avg_response_time: 0.0,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:235` **UW** [float-literal] `throughput: 0.0,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:236` **UW** [float-literal] `recovery_time: 0.0,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:237` **UW** [float-literal] `resource_efficiency: 1.0,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:238` **UW** [float-literal] `network_overhead: 0.0,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:239` **UW** [float-literal] `finality_time: 0.0,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:276` **UW** [float-type] `selection_criteria: FxHashMap<String, f64>,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:327` **UW** [float-type] `pub progress: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:396` **UW** [float-literal] `density: 0.8,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:465` **UW** [float-type] `let byzantine_ratio = detected_byzantine_nodes.len() as f64 / total_nodes as f64;`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:535` **UW** [wall-clock] `let start_time = Instant::now();`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:637` **UW** [float-literal] `performance_improvement: 0.0,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:654` **UW** [float-literal] `.filter(\|health\| health.health_score > 0.7)`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:715` **UW** [float-type] `health.is_reachable = rand::random::<f64>() > 0.1;`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:715` **UW** [float-literal] `health.is_reachable = rand::random::<f64>() > 0.1;`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:715` **UW** [unseeded-rng] `health.is_reachable = rand::random::<f64>() > 0.1;`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:716` **UW** [float-type] `health.response_time = 50.0 + rand::random::<f64>() * 100.0;`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:716` **UW** [float-literal] `health.response_time = 50.0 + rand::random::<f64>() * 100.0;`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:716` **UW** [unseeded-rng] `health.response_time = 50.0 + rand::random::<f64>() * 100.0;`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:718` **UW** [float-type] `0.8 + rand::random::<f64>() * 0.2`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:718` **UW** [float-literal] `0.8 + rand::random::<f64>() * 0.2`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:718` **UW** [unseeded-rng] `0.8 + rand::random::<f64>() * 0.2`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:720` **UW** [float-literal] `0.0`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:740` **UW** [float-type] `if rand::random::<f64>() < 0.01 {`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:740` **UW** [float-literal] `if rand::random::<f64>() < 0.01 {`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:740` **UW** [unseeded-rng] `if rand::random::<f64>() < 0.01 {`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:808` **UW** [float-type] `) -> f64 {`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:810` **UW** [float-literal] `return 0.0;`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:814` **UW** [float-type] `node_health.values().map(\|h\| h.health_score).sum::<f64>() / node_health.len() as f64;`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:816` **UW** [float-literal] `let partition_penalty = if active_partitions > 0 { 0.2 } else { 0.0 };`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:817` **UW** [float-type] `let failure_penalty = (active_failures as f64) * 0.1;`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:817` **UW** [float-literal] `let failure_penalty = (active_failures as f64) * 0.1;`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:819` **UW** [float-literal] `(avg_health - partition_penalty - failure_penalty).clamp(0.0, 1.0)`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:879` **UW** [float-literal] `node_health.health_score = 0.0;`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1034` **UW** [float-type] `) -> ConsensusResult<f64> {`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1036` **UW** [float-literal] `DegradationLevel::Full => 0.0,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1037` **UW** [float-literal] `DegradationLevel::Minor => 0.05,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1038` **UW** [float-literal] `DegradationLevel::Moderate => 0.15,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1039` **UW** [float-literal] `DegradationLevel::Severe => 0.35,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1040` **UW** [float-literal] `DegradationLevel::Emergency => 0.60,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1053` **UW** [float-literal] `resource_usage: 0.7,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1054` **UW** [float-literal] `efficiency_score: 0.85,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1062` **UW** [float-literal] `.filter(\|h\| h.health_score > 0.5 && h.error_rate < 0.1)`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1068` **UW** [float-type] `active_nodes as f64 / node_health.len() as f64`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1073` **UW** [float-literal] `partition_risk: if active_nodes < 4 { 0.6 } else { 0.2 },`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1074` **UW** [float-literal] `byzantine_risk: if active_nodes >= 4 { 0.1 } else { 0.8 },`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1084` **UW** [float-literal] `if conditions.partition_risk > 0.5 {`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1086` **UW** [float-literal] `density: 0.9,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1090` **UW** [float-literal] `} else if performance.efficiency_score < 0.7 {`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1108` **UW** [float-literal] `Ok(current != optimal && performance.efficiency_score < 0.8)`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1126` **UW** [float-literal] `performance_improvement: 0.15,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1168` **UW** [float-type] `pub estimated_performance_impact: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1176` **UW** [float-type] `pub performance_improvement: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1190` **UW** [float-type] `pub overall_resilience_score: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1196` **UW** [float-type] `pub latency: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1197` **UW** [float-type] `pub block_time: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1198` **UW** [float-type] `pub resource_usage: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1199` **UW** [float-type] `pub efficiency_score: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1205` **UW** [float-type] `pub partition_risk: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1206` **UW** [float-type] `pub byzantine_risk: f64,`
- `consensus-engines/src/coordination/adaptive_fault_tolerance.rs:1207` **UW** [float-type] `pub stability_score: f64,`

## `consensus-engines/src/security/manager.rs` — 77 (UW×77)

- `consensus-engines/src/security/manager.rs:35` **UW** [wall-clock] `SystemTime::now()`
- `consensus-engines/src/security/manager.rs:63` **UW** [float-type] `estimated_impact: f64,`
- `consensus-engines/src/security/manager.rs:68` **UW** [float-type] `frequency_anomaly: f64,`
- `consensus-engines/src/security/manager.rs:83` **UW** [float-type] `network_isolation_level: f64,`
- `consensus-engines/src/security/manager.rs:87` **UW** [float-type] `performance_impact: f64,`
- `consensus-engines/src/security/manager.rs:173` **UW** [float-type] `pub max_byzantine_percentage: f64,`
- `consensus-engines/src/security/manager.rs:177` **UW** [float-type] `pub threat_sensitivity: f64,`
- `consensus-engines/src/security/manager.rs:205` **UW** [float-literal] `max_byzantine_percentage: 0.33, // 33% Byzantine tolerance`
- `consensus-engines/src/security/manager.rs:207` **UW** [float-literal] `threat_sensitivity: 0.7,`
- `consensus-engines/src/security/manager.rs:311` **UW** [float-type] `frequency_score: f64,`
- `consensus-engines/src/security/manager.rs:320` **UW** [float-type] `threshold_values: FxHashMap<String, f64>,`
- `consensus-engines/src/security/manager.rs:328` **UW** [float-type] `anomaly_thresholds: FxHashMap<String, f64>,`
- `consensus-engines/src/security/manager.rs:336` **UW** [float-type] `average_response_time: f64,`
- `consensus-engines/src/security/manager.rs:338` **UW** [float-type] `trust_score: f64,`
- `consensus-engines/src/security/manager.rs:349` **UW** [float-type] `severity: f64,`
- `consensus-engines/src/security/manager.rs:365` **UW** [float-type] `consensus_participation_rate: f64,`
- `consensus-engines/src/security/manager.rs:366` **UW** [float-type] `average_block_time: f64,`
- `consensus-engines/src/security/manager.rs:367` **UW** [float-type] `network_congestion: f64,`
- `consensus-engines/src/security/manager.rs:368` **UW** [float-type] `byzantine_tolerance_ratio: f64,`
- `consensus-engines/src/security/manager.rs:375` **UW** [float-type] `latency_ms: f64,`
- `consensus-engines/src/security/manager.rs:376` **UW** [float-type] `packet_loss_rate: f64,`
- `consensus-engines/src/security/manager.rs:377` **UW** [float-type] `connection_stability: f64,`
- `consensus-engines/src/security/manager.rs:452` **UW** [unseeded-rng] `let mut rng = rand::thread_rng();`
- `consensus-engines/src/security/manager.rs:613` **UW** [wall-clock] `timestamp: SystemTime::now()`
- `consensus-engines/src/security/manager.rs:765` **UW** [wall-clock] `let current_time = SystemTime::now()`
- `consensus-engines/src/security/manager.rs:782` **UW** [float-type] `let avg_interval = intervals.iter().sum::<u64>() as f64 / intervals.len() as f64;`
- `consensus-engines/src/security/manager.rs:784` **UW** [float-literal] `if avg_interval < 300.0 {`
- `consensus-engines/src/security/manager.rs:810` **UW** [float-type] `frequency_score: recent_patterns.len() as f64,`
- `consensus-engines/src/security/manager.rs:869` **UW** [wall-clock] `let timestamp = SystemTime::now()`
- `consensus-engines/src/security/manager.rs:918` **UW** [float-literal] `integrity_score: 0.0,`
- `consensus-engines/src/security/manager.rs:938` **UW** [float-literal] `let mut score = 0.0;`
- `consensus-engines/src/security/manager.rs:940` **UW** [float-literal] `score += 0.3;`
- `consensus-engines/src/security/manager.rs:946` **UW** [float-literal] `score += 0.2;`
- `consensus-engines/src/security/manager.rs:969` **UW** [float-literal] `let threat_level = if result.integrity_score > 0.8 {`
- `consensus-engines/src/security/manager.rs:971` **UW** [float-literal] `} else if result.integrity_score > 0.6 {`
- `consensus-engines/src/security/manager.rs:1021` **UW** [wall-clock] `last_updated: SystemTime::now()`
- `consensus-engines/src/security/manager.rs:1065` **UW** [float-literal] `if reputation.score < 0.6 {`
- `consensus-engines/src/security/manager.rs:1152` **UW** [float-literal] `if reputation.score < 0.3 {`
- `consensus-engines/src/security/manager.rs:1183` **UW** [wall-clock] `audit_timestamp: SystemTime::now()`
- `consensus-engines/src/security/manager.rs:1197` **UW** [float-literal] `methodology_version: "1.0.0".to_string(),`
- `consensus-engines/src/security/manager.rs:1310` **UW** [float-type] `let approval_percentage = approval_votes as f64 / total_votes as f64;`
- `consensus-engines/src/security/manager.rs:1311` **UW** [float-literal] `if approval_percentage < 0.5 {`
- `consensus-engines/src/security/manager.rs:1364` **UW** [float-type] `network_health.malicious_nodes as f64 / network_health.total_nodes as f64`
- `consensus-engines/src/security/manager.rs:1366` **UW** [float-literal] `0.0`
- `consensus-engines/src/security/manager.rs:1373` **UW** [float-literal] `} else if byzantine_ratio < self.config.max_byzantine_percentage * 1.5 {`
- `consensus-engines/src/security/manager.rs:1376` **UW** [float-literal] `byzantine_ratio * 100.0`
- `consensus-engines/src/security/manager.rs:1421` **UW** [float-literal] `let participation_ok = health_metrics.consensus_participation_rate > 0.6;`
- `consensus-engines/src/security/manager.rs:1422` **UW** [float-literal] `let block_time_ok = health_metrics.average_block_time < 60.0;`
- `consensus-engines/src/security/manager.rs:1423` **UW** [float-literal] `let congestion_ok = health_metrics.network_congestion < 0.8;`
- `consensus-engines/src/security/manager.rs:1424` **UW** [float-literal] `let byzantine_ok = health_metrics.byzantine_tolerance_ratio > 0.33;`
- `consensus-engines/src/security/manager.rs:1429` **UW** [float-type] `.count() as f64`
- `consensus-engines/src/security/manager.rs:1430` **UW** [float-literal] `/ 4.0;`
- `consensus-engines/src/security/manager.rs:1432` **UW** [float-literal] `let status = if health_score > 0.8 {`
- `consensus-engines/src/security/manager.rs:1434` **UW** [float-literal] `} else if health_score > 0.6 {`
- `consensus-engines/src/security/manager.rs:1437` **UW** [float-literal] `health_score * 100.0`
- `consensus-engines/src/security/manager.rs:1440` **UW** [float-literal] `CheckpointStatus::Failed(format!("Poor network health: {:.1}%", health_score * 100.0))`
- `consensus-engines/src/security/manager.rs:1630` **UW** [float-type] `async fn calculate_overall_security_score(&self) -> f64 {`
- `consensus-engines/src/security/manager.rs:1635` **UW** [float-literal] `return 0.8;`
- `consensus-engines/src/security/manager.rs:1644` **UW** [float-type] `let threat_ratio = threat_events as f64 / total_events as f64;`
- `consensus-engines/src/security/manager.rs:1646` **UW** [float-literal] `(1.0 - threat_ratio).clamp(0.0, 1.0)`
- `consensus-engines/src/security/manager.rs:1897` **UW** [float-type] `successful_transitions as f64 / recent_count as f64`
- `consensus-engines/src/security/manager.rs:1899` **UW** [float-literal] `1.0`
- `consensus-engines/src/security/manager.rs:1906` **UW** [float-literal] `average_validation_time: 150.0,`
- `consensus-engines/src/security/manager.rs:1928` **UW** [float-literal] `mitigation_effectiveness: 0.95,`
- `consensus-engines/src/security/manager.rs:1953` **UW** [float-literal] `overall_compliance_score: 0.96,`
- `consensus-engines/src/security/manager.rs:1979` **UW** [float-type] `pub success_rate: f64,`
- `consensus-engines/src/security/manager.rs:1980` **UW** [float-type] `pub average_validation_time: f64,`
- `consensus-engines/src/security/manager.rs:1989` **UW** [float-type] `pub mitigation_effectiveness: f64,`
- `consensus-engines/src/security/manager.rs:2001` **UW** [float-type] `pub overall_compliance_score: f64,`
- `consensus-engines/src/security/manager.rs:2020` **UW** [float-type] `pub confidence_score: f64,`
- `consensus-engines/src/security/manager.rs:2029` **UW** [float-literal] `confidence_score: 0.8,`
- `consensus-engines/src/security/manager.rs:2038` **UW** [float-type] `pub integrity_score: f64,`
- `consensus-engines/src/security/manager.rs:2052` **UW** [float-type] `pub security_score: f64,`
- `consensus-engines/src/security/manager.rs:2085` **UW** [float-literal] `consensus_participation_rate: 1.0,`
- `consensus-engines/src/security/manager.rs:2086` **UW** [float-literal] `average_block_time: 10.0,`
- `consensus-engines/src/security/manager.rs:2087` **UW** [float-literal] `network_congestion: 0.1,`
- `consensus-engines/src/security/manager.rs:2088` **UW** [float-literal] `byzantine_tolerance_ratio: 1.0,`

## `consensus-engines/src/coordination/consensus_builder.rs` — 71 (UW×71)

- `consensus-engines/src/coordination/consensus_builder.rs:29` **UW** [wall-clock] `SystemTime::now()`
- `consensus-engines/src/coordination/consensus_builder.rs:62` **UW** [float-type] `Stake { max_power_ratio: f64 },`
- `consensus-engines/src/coordination/consensus_builder.rs:64` **UW** [float-type] `Reputation { reputation_weight: f64 },`
- `consensus-engines/src/coordination/consensus_builder.rs:67` **UW** [float-type] `stake_weight: f64,`
- `consensus-engines/src/coordination/consensus_builder.rs:68` **UW** [float-type] `reputation_weight: f64,`
- `consensus-engines/src/coordination/consensus_builder.rs:73` **UW** [float-type] `expertise_weight: f64,`
- `consensus-engines/src/coordination/consensus_builder.rs:81` **UW** [float-type] `Fixed { percentage: f64 },`
- `consensus-engines/src/coordination/consensus_builder.rs:84` **UW** [float-type] `base_percentage: f64,`
- `consensus-engines/src/coordination/consensus_builder.rs:85` **UW** [float-type] `participation_factor: f64,`
- `consensus-engines/src/coordination/consensus_builder.rs:88` **UW** [float-type] `StakeWeighted { min_stake_percentage: f64 },`
- `consensus-engines/src/coordination/consensus_builder.rs:91` **UW** [float-type] `min_percentage: f64,`
- `consensus-engines/src/coordination/consensus_builder.rs:92` **UW** [float-type] `max_percentage: f64,`
- `consensus-engines/src/coordination/consensus_builder.rs:98` **UW** [float-literal] `QuorumType::Fixed { percentage: 0.67 }`
- `consensus-engines/src/coordination/consensus_builder.rs:112` **UW** [float-type] `criteria_weights: FxHashMap<String, f64>,`
- `consensus-engines/src/coordination/consensus_builder.rs:148` **UW** [float-literal] `stake_weight: 0.7,`
- `consensus-engines/src/coordination/consensus_builder.rs:149` **UW** [float-literal] `reputation_weight: 0.3,`
- `consensus-engines/src/coordination/consensus_builder.rs:152` **UW** [float-literal] `base_percentage: 0.51,`
- `consensus-engines/src/coordination/consensus_builder.rs:153` **UW** [float-literal] `participation_factor: 0.1,`
- `consensus-engines/src/coordination/consensus_builder.rs:158` **UW** [float-literal] `weights.insert("technical_merit".to_string(), 0.4);`
- `consensus-engines/src/coordination/consensus_builder.rs:159` **UW** [float-literal] `weights.insert("community_support".to_string(), 0.3);`
- `consensus-engines/src/coordination/consensus_builder.rs:160` **UW** [float-literal] `weights.insert("risk_assessment".to_string(), 0.3);`
- `consensus-engines/src/coordination/consensus_builder.rs:193` **UW** [float-type] `pub conviction_factor: f64,`
- `consensus-engines/src/coordination/consensus_builder.rs:195` **UW** [float-type] `pub max_conviction: f64,`
- `consensus-engines/src/coordination/consensus_builder.rs:205` **UW** [float-literal] `conviction_factor: 2.0,`
- `consensus-engines/src/coordination/consensus_builder.rs:206` **UW** [float-literal] `max_conviction: 6.0,`
- `consensus-engines/src/coordination/consensus_builder.rs:224` **UW** [float-type] `pub voting_power: f64,`
- `consensus-engines/src/coordination/consensus_builder.rs:252` **UW** [float-type] `pub quorum_requirement: f64,`
- `consensus-engines/src/coordination/consensus_builder.rs:257` **UW** [float-type] `pub vote_tallies: FxHashMap<String, f64>, // Choice -> Power`
- `consensus-engines/src/coordination/consensus_builder.rs:279` **UW** [float-type] `pub conviction_multiplier: f64,`
- `consensus-engines/src/coordination/consensus_builder.rs:292` **UW** [float-type] `final_tally: FxHashMap<String, f64>,`
- `consensus-engines/src/coordination/consensus_builder.rs:306` **UW** [float-type] `pub criteria_scores: FxHashMap<String, FxHashMap<String, f64>>, // Proposal -> (Criteria -> Score)`
- `consensus-engines/src/coordination/consensus_builder.rs:346` **UW** [float-type] `pub consensus_requirements: FxHashMap<String, f64>, // Algorithm -> Required percentage`
- `consensus-engines/src/coordination/consensus_builder.rs:385` **UW** [float-type] `voter_reputations: Arc<RwLock<FxHashMap<String, f64>>>,`
- `consensus-engines/src/coordination/consensus_builder.rs:432` **UW** [float-type] `custom_quorum: Option<f64>,`
- `consensus-engines/src/coordination/consensus_builder.rs:585` **UW** [float-type] `let influence = (*credits as f64).sqrt(); // Square root for quadratic voting`
- `consensus-engines/src/coordination/consensus_builder.rs:586` **UW** [float-literal] `*session.vote_tallies.entry(choice.clone()).or_insert(0.0) += influence;`
- `consensus-engines/src/coordination/consensus_builder.rs:599` **UW** [float-type] `voting_power: total_credits_used as f64,`
- `consensus-engines/src/coordination/consensus_builder.rs:672` **UW** [float-type] `let effective_power = (stake_amount as f64) * conviction_multiplier;`
- `consensus-engines/src/coordination/consensus_builder.rs:688` **UW** [float-literal] `*session.vote_tallies.entry(choice_key).or_insert(0.0) += effective_power;`
- `consensus-engines/src/coordination/consensus_builder.rs:798` **UW** [float-literal] `Some(0.67), // 67% consensus required`
- `consensus-engines/src/coordination/consensus_builder.rs:803` **UW** [float-literal] `consensus_requirements.insert(algorithm.clone(), 0.67);`
- `consensus-engines/src/coordination/consensus_builder.rs:910` **UW** [float-type] `let total_participants = session.participants.len() as f64;`
- `consensus-engines/src/coordination/consensus_builder.rs:911` **UW** [float-type] `let total_eligible_voters = self.get_total_eligible_voters().await? as f64;`
- `consensus-engines/src/coordination/consensus_builder.rs:1000` **UW** [unseeded-rng] `let signing_key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());`
- `consensus-engines/src/coordination/consensus_builder.rs:1010` **UW** [float-type] `async fn calculate_dynamic_quorum(&self) -> ConsensusResult<f64> {`
- `consensus-engines/src/coordination/consensus_builder.rs:1020` **UW** [float-literal] `Ok(adjusted_quorum.clamp(0.33, 0.95)) // Cap between 33% and 95%`
- `consensus-engines/src/coordination/consensus_builder.rs:1033` **UW** [float-literal] `Ok((*min_percentage + *max_percentage) / 2.0)`
- `consensus-engines/src/coordination/consensus_builder.rs:1043` **UW** [float-type] `) -> std::pin::Pin<Box<dyn std::future::Future<Output = ConsensusResult<f64>> + 'a>> {`
- `consensus-engines/src/coordination/consensus_builder.rs:1046` **UW** [float-literal] `VotingPowerMethod::Equal => Ok(1.0),`
- `consensus-engines/src/coordination/consensus_builder.rs:1048` **UW** [float-type] `let stake = self.get_voter_stake(voter_id).await? as f64;`
- `consensus-engines/src/coordination/consensus_builder.rs:1049` **UW** [float-type] `let total_stake = self.get_total_stake().await? as f64;`
- `consensus-engines/src/coordination/consensus_builder.rs:1050` **UW** [float-literal] `let stake_ratio = if total_stake > 0.0 {`
- `consensus-engines/src/coordination/consensus_builder.rs:1053` **UW** [float-literal] `0.0`
- `consensus-engines/src/coordination/consensus_builder.rs:1069` **UW** [float-literal] `max_power_ratio: 1.0,`
- `consensus-engines/src/coordination/consensus_builder.rs:1077` **UW** [float-literal] `reputation_weight: 1.0,`
- `consensus-engines/src/coordination/consensus_builder.rs:1098` **UW** [float-literal] `if reputation < 0.1 {`
- `consensus-engines/src/coordination/consensus_builder.rs:1189` **UW** [float-literal] `*session.vote_tallies.entry(choice_key).or_insert(0.0) += vote.voting_power;`
- `consensus-engines/src/coordination/consensus_builder.rs:1195` **UW** [float-type] `async fn calculate_conviction_multiplier(&self, lock_duration: u64) -> f64 {`
- `consensus-engines/src/coordination/consensus_builder.rs:1197` **UW** [float-type] `let time_ratio = lock_duration as f64 / config.full_conviction_time as f64;`
- `consensus-engines/src/coordination/consensus_builder.rs:1198` **UW** [float-literal] `let multiplier = 1.0 + (config.conviction_factor * time_ratio.ln_1p());`
- `consensus-engines/src/coordination/consensus_builder.rs:1251` **UW** [float-type] `async fn get_voter_reputation(&self, voter_id: &str) -> ConsensusResult<f64> {`
- `consensus-engines/src/coordination/consensus_builder.rs:1253` **UW** [float-literal] `Ok(reputations.get(voter_id).copied().unwrap_or(0.5))`
- `consensus-engines/src/coordination/consensus_builder.rs:1268` **UW** [float-literal] `Ok(reputations.iter().filter(\|(_, &rep)\| rep >= 0.1).count())`
- `consensus-engines/src/coordination/consensus_builder.rs:1271` **UW** [float-type] `async fn get_current_participation_rate(&self) -> ConsensusResult<f64> {`
- `consensus-engines/src/coordination/consensus_builder.rs:1273` **UW** [float-literal] `Ok(0.7) // 70% participation rate`
- `consensus-engines/src/coordination/consensus_builder.rs:1287` **UW** [float-type] `let score: f64 = rand::thread_rng().gen_range(0.0..1.0);`
- `consensus-engines/src/coordination/consensus_builder.rs:1287` **UW** [float-literal] `let score: f64 = rand::thread_rng().gen_range(0.0..1.0);`
- `consensus-engines/src/coordination/consensus_builder.rs:1287` **UW** [unseeded-rng] `let score: f64 = rand::thread_rng().gen_range(0.0..1.0);`
- `consensus-engines/src/coordination/consensus_builder.rs:1297` **UW** [float-type] `let total: f64 = scores`
- `consensus-engines/src/coordination/consensus_builder.rs:1300` **UW** [float-literal] `let weight = criteria_weights.get(criterion).unwrap_or(&1.0);`
- `consensus-engines/src/coordination/consensus_builder.rs:1368` **UW** [float-type] `) -> ConsensusResult<(Option<String>, FxHashMap<String, f64>)> {`

## `consensus-engines/src/security/coordinator.rs` — 66 (UW×66)

- `consensus-engines/src/security/coordinator.rs:129` **UW** [float-type] `pub risk_score: f64,`
- `consensus-engines/src/security/coordinator.rs:140` **UW** [float-type] `pub network_health: f64,`
- `consensus-engines/src/security/coordinator.rs:146` **UW** [float-type] `pub participation_rate: f64,`
- `consensus-engines/src/security/coordinator.rs:148` **UW** [float-type] `pub average_latency: f64,`
- `consensus-engines/src/security/coordinator.rs:316` **UW** [wall-clock] `start_time: std::time::Instant::now(),`
- `consensus-engines/src/security/coordinator.rs:349` **UW** [float-literal] `if integrity_result.integrity_score < 0.8 {`
- `consensus-engines/src/security/coordinator.rs:430` **UW** [wall-clock] `timestamp: SystemTime::now()`
- `consensus-engines/src/security/coordinator.rs:503` **UW** [float-literal] `if reputation.score < 0.5 {`
- `consensus-engines/src/security/coordinator.rs:518` **UW** [float-literal] `if reputation.score >= 0.3 {`
- `consensus-engines/src/security/coordinator.rs:576` **UW** [wall-clock] `let start_time = std::time::Instant::now();`
- `consensus-engines/src/security/coordinator.rs:601` **UW** [float-literal] `frequency_anomaly: 1.0,`
- `consensus-engines/src/security/coordinator.rs:604` **UW** [float-literal] `confidence_score: 0.9,`
- `consensus-engines/src/security/coordinator.rs:611` **UW** [float-literal] `network_health: 0.3,`
- `consensus-engines/src/security/coordinator.rs:614` **UW** [float-literal] `participation_rate: 0.0,`
- `consensus-engines/src/security/coordinator.rs:615` **UW** [float-literal] `average_latency: 0.0,`
- `consensus-engines/src/security/coordinator.rs:623` **UW** [float-literal] `risk_score: 0.9,`
- `consensus-engines/src/security/coordinator.rs:652` **UW** [float-literal] `ValidationStatus::Validated => risk_score < 0.7,`
- `consensus-engines/src/security/coordinator.rs:829` **UW** [float-type] `(network_health.total_nodes - network_health.malicious_nodes) as f64`
- `consensus-engines/src/security/coordinator.rs:830` **UW** [float-type] `/ network_health.total_nodes as f64`
- `consensus-engines/src/security/coordinator.rs:832` **UW** [float-literal] `0.0`
- `consensus-engines/src/security/coordinator.rs:834` **UW** [float-literal] `average_latency: 50.0,  // Would be measured from actual network`
- `consensus-engines/src/security/coordinator.rs:844` **UW** [float-type] `) -> f64 {`
- `consensus-engines/src/security/coordinator.rs:845` **UW** [float-literal] `let mut risk_score = 0.0;`
- `consensus-engines/src/security/coordinator.rs:849` **UW** [float-literal] `SecurityThreatLevel::Green => risk_score += 0.1,`
- `consensus-engines/src/security/coordinator.rs:850` **UW** [float-literal] `SecurityThreatLevel::Yellow => risk_score += 0.3,`
- `consensus-engines/src/security/coordinator.rs:851` **UW** [float-literal] `SecurityThreatLevel::Orange => risk_score += 0.6,`
- `consensus-engines/src/security/coordinator.rs:852` **UW** [float-literal] `SecurityThreatLevel::Red => risk_score += 0.8,`
- `consensus-engines/src/security/coordinator.rs:853` **UW** [float-literal] `SecurityThreatLevel::Critical => risk_score += 1.0,`
- `consensus-engines/src/security/coordinator.rs:857` **UW** [float-literal] `risk_score += (1.0 - state_assessment.network_health) * 0.5;`
- `consensus-engines/src/security/coordinator.rs:861` **UW** [float-literal] `risk_score += 0.3;`
- `consensus-engines/src/security/coordinator.rs:866` **UW** [float-type] `risk_score += (state_assessment.malicious_nodes as f64 / 100.0).min(0.3);`
- `consensus-engines/src/security/coordinator.rs:866` **UW** [float-literal] `risk_score += (state_assessment.malicious_nodes as f64 / 100.0).min(0.3);`
- `consensus-engines/src/security/coordinator.rs:869` **UW** [float-literal] `risk_score.min(1.0)`
- `consensus-engines/src/security/coordinator.rs:1024` **UW** [float-literal] `integrity_result.integrity_score *= 0.7; // Reduce integrity score`
- `consensus-engines/src/security/coordinator.rs:1031` **UW** [float-literal] `if integrity_result.integrity_score < 0.9 {`
- `consensus-engines/src/security/coordinator.rs:1178` **UW** [float-literal] `network_stability: 0.95, // Would calculate from actual metrics`
- `consensus-engines/src/security/coordinator.rs:1179` **UW** [float-literal] `security_posture: 0.96,  // From security manager`
- `consensus-engines/src/security/coordinator.rs:1181` **UW** [float-literal] `consensus_participation: 0.89, // From network metrics`
- `consensus-engines/src/security/coordinator.rs:1182` **UW** [float-literal] `validator_readiness: 0.92,     // From validator status`
- `consensus-engines/src/security/coordinator.rs:1183` **UW** [float-literal] `overall_readiness_score: 0.93,`
- `consensus-engines/src/security/coordinator.rs:1193` **UW** [float-type] `1.0 - (network_health.malicious_nodes as f64 / network_health.total_nodes as f64)`
- `consensus-engines/src/security/coordinator.rs:1193` **UW** [float-literal] `1.0 - (network_health.malicious_nodes as f64 / network_health.total_nodes as f64)`
- `consensus-engines/src/security/coordinator.rs:1195` **UW** [float-literal] `1.0`
- `consensus-engines/src/security/coordinator.rs:1199` **UW** [float-literal] `(0.33`
- `consensus-engines/src/security/coordinator.rs:1200` **UW** [float-type] `- (network_health.malicious_nodes as f64 / network_health.total_nodes as f64))`
- `consensus-engines/src/security/coordinator.rs:1201` **UW** [float-literal] `.max(0.0)`
- `consensus-engines/src/security/coordinator.rs:1203` **UW** [float-literal] `0.33`
- `consensus-engines/src/security/coordinator.rs:1212` **UW** [float-literal] `threat_mitigation_effectiveness: 0.95,`
- `consensus-engines/src/security/coordinator.rs:1224` **UW** [float-literal] `/ 2.0;`
- `consensus-engines/src/security/coordinator.rs:1226` **UW** [float-literal] `if overall_security_score > 0.9 {`
- `consensus-engines/src/security/coordinator.rs:1231` **UW** [float-literal] `} else if overall_security_score > 0.7 {`
- `consensus-engines/src/security/coordinator.rs:1257` **UW** [float-literal] `0.67`
- `consensus-engines/src/security/coordinator.rs:1259` **UW** [float-literal] `0.75`
- `consensus-engines/src/security/coordinator.rs:1261` **UW** [float-literal] `required_governance_quorum: 0.51,`
- `consensus-engines/src/security/coordinator.rs:1262` **UW** [float-literal] `required_security_clearance: if byzantine_assessment.security_margin > 0.1 {`
- `consensus-engines/src/security/coordinator.rs:1292` **UW** [float-type] `pub network_stability: f64,`
- `consensus-engines/src/security/coordinator.rs:1293` **UW** [float-type] `pub security_posture: f64,`
- `consensus-engines/src/security/coordinator.rs:1295` **UW** [float-type] `pub consensus_participation: f64,`
- `consensus-engines/src/security/coordinator.rs:1296` **UW** [float-type] `pub validator_readiness: f64,`
- `consensus-engines/src/security/coordinator.rs:1297` **UW** [float-type] `pub overall_readiness_score: f64,`
- `consensus-engines/src/security/coordinator.rs:1306` **UW** [float-type] `pub fault_tolerance_ratio: f64,`
- `consensus-engines/src/security/coordinator.rs:1307` **UW** [float-type] `pub security_margin: f64,`
- `consensus-engines/src/security/coordinator.rs:1308` **UW** [float-type] `pub threat_mitigation_effectiveness: f64,`
- `consensus-engines/src/security/coordinator.rs:1315` **UW** [float-type] `confidence: f64,`
- `consensus-engines/src/security/coordinator.rs:1331` **UW** [float-type] `pub required_validator_consensus: f64,`
- `consensus-engines/src/security/coordinator.rs:1332` **UW** [float-type] `pub required_governance_quorum: f64,`

## `consensus-engines/src/coordination/byzantine_coordinator.rs` — 61 (UW×61)

- `consensus-engines/src/coordination/byzantine_coordinator.rs:25` **UW** [wall-clock] `SystemTime::now()`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:35` **UW** [float-type] `pub max_byzantine_ratio: f64,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:53` **UW** [float-literal] `max_byzantine_ratio: 0.33,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:68` **UW** [float-type] `pub min_approval_percentage: f64,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:70` **UW** [float-type] `pub min_quorum_percentage: f64,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:88` **UW** [float-literal] `min_approval_percentage: 0.67,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:89` **UW** [float-literal] `min_quorum_percentage: 0.51,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:110` **UW** [float-type] `pub attack_detection_sensitivity: f64,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:120` **UW** [float-type] `pub detection_sensitivity: f64,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:137` **UW** [float-literal] `attack_detection_sensitivity: 0.8,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:142` **UW** [float-literal] `detection_sensitivity: 0.8,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:165` **UW** [float-type] `pub approval_threshold: f64,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:283` **UW** [float-type] `pub confidence: f64,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:325` **UW** [float-type] `pub participation_rate: f64,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:327` **UW** [float-type] `pub avg_message_latency: f64,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:329` **UW** [float-type] `pub message_success_rate: f64,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:352` **UW** [float-type] `node_reputations: Arc<RwLock<FxHashMap<String, f64>>>,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:381` **UW** [float-literal] `participation_rate: 1.0,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:382` **UW** [float-literal] `avg_message_latency: 50.0,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:383` **UW** [float-literal] `message_success_rate: 1.0,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:496` **UW** [float-literal] `confidence: 0.0,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:505` **UW** [float-literal] `detection_result.confidence = 0.9;`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:517` **UW** [float-literal] `detection_result.confidence = 0.8;`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:529` **UW** [float-literal] `detection_result.confidence = 0.7;`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:539` **UW** [float-literal] `self.update_node_reputation(node_id, -0.3).await;`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:553` **UW** [float-literal] `self.update_node_reputation(node_id, 0.01).await;`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:709` **UW** [float-literal] `network_health_score: 1.0,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:729` **UW** [float-literal] `confidence_score: 0.95,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:757` **UW** [float-type] `byzantine_percentage: f64,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:763` **UW** [float-literal] `if byzantine_percentage > 33.0 {`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:768` **UW** [float-type] `((node_count as f64 * byzantine_percentage / 100.0).ceil() as u32).min(node_count / 3);`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:768` **UW** [float-literal] `((node_count as f64 * byzantine_percentage / 100.0).ceil() as u32).min(node_count / 3);`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:776` **UW** [float-literal] `network_health.participation_rate = 1.0;`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:785` **UW** [float-type] `pub async fn update_node_reputation(&self, node_id: &str, delta: f64) {`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:787` **UW** [float-literal] `let current = reputations.get(node_id).unwrap_or(&0.5);`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:788` **UW** [float-literal] `let new_reputation = (current + delta).clamp(0.0, 1.0);`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:795` **UW** [unseeded-rng] `let signing_key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:826` **UW** [float-literal] `reps.values().filter(\|&&score\| score < 0.3).count() as u32;`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:829` **UW** [float-literal] `.filter(\|&&score\| (0.3..0.6).contains(&score))`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:836` **UW** [float-type] `(health_metrics.total_nodes - health_metrics.malicious_nodes) as f64`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:837` **UW** [float-type] `/ health_metrics.total_nodes as f64`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:839` **UW** [float-literal] `1.0`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:842` **UW** [wall-clock] `health_metrics.last_check = SystemTime::now()`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:882` **UW** [float-literal] `let reputation = reputations.get(proposer).unwrap_or(&0.5);`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:884` **UW** [float-literal] `if *reputation < 0.7 {`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:917` **UW** [float-literal] `let reputation = reputations.get(voter_id).unwrap_or(&0.5);`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:919` **UW** [float-literal] `if *reputation < 0.3 {`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:966` **UW** [float-type] `let approval_rate = proposal.votes_for as f64 / total_votes as f64;`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:976` **UW** [float-literal] `approval_rate * 100.0`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:986` **UW** [float-literal] `reason: format!("Insufficient approval: {:.1}%", approval_rate * 100.0),`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:1037` **UW** [float-type] `let mean_interval = intervals.iter().sum::<u64>() as f64 / intervals.len() as f64;`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:1041` **UW** [float-type] `let diff = interval as f64 - mean_interval;`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:1044` **UW** [float-type] `.sum::<f64>()`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:1045` **UW** [float-type] `/ intervals.len() as f64;`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:1047` **UW** [float-literal] `Ok(variance < 1.0)`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:1115` **UW** [float-literal] `self.update_node_reputation(node_id, -0.5).await;`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:1157` **UW** [float-literal] `let reputation = reputations.get(leader).unwrap_or(&1.0);`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:1159` **UW** [float-literal] `if *reputation > 0.8 && evidence.len() < 2 {`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:1177` **UW** [float-literal] `.filter(\|(_, &score)\| score > 0.5)`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:1214` **UW** [float-type] `pub confidence_score: f64,`
- `consensus-engines/src/coordination/byzantine_coordinator.rs:1225` **UW** [float-type] `pub network_health_score: f64,`

## `consensus-engines/src/coordination/dynamic_engine.rs` — 60 (UW×60)

- `consensus-engines/src/coordination/dynamic_engine.rs:32` **UW** [float-type] `pub max_block_time: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:34` **UW** [float-type] `pub min_throughput: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:36` **UW** [float-type] `pub max_latency: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:38` **UW** [float-type] `pub min_participation: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:40` **UW** [float-type] `pub max_hash_rate_variance: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:46` **UW** [float-literal] `max_block_time: 30.0,`
- `consensus-engines/src/coordination/dynamic_engine.rs:47` **UW** [float-literal] `min_throughput: 100.0,`
- `consensus-engines/src/coordination/dynamic_engine.rs:48` **UW** [float-literal] `max_latency: 1000.0,`
- `consensus-engines/src/coordination/dynamic_engine.rs:49` **UW** [float-literal] `min_participation: 0.67,`
- `consensus-engines/src/coordination/dynamic_engine.rs:50` **UW** [float-literal] `max_hash_rate_variance: 0.3,`
- `consensus-engines/src/coordination/dynamic_engine.rs:59` **UW** [float-type] `pub min_approval_percentage: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:63` **UW** [float-type] `pub min_quorum: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:71` **UW** [float-literal] `min_approval_percentage: 0.67,`
- `consensus-engines/src/coordination/dynamic_engine.rs:73` **UW** [float-literal] `min_quorum: 0.51,`
- `consensus-engines/src/coordination/dynamic_engine.rs:138` **UW** [float-type] `pub criteria_weights: FxHashMap<String, f64>,`
- `consensus-engines/src/coordination/dynamic_engine.rs:148` **UW** [float-literal] `criteria_weights.insert("reputation".to_string(), 0.4);`
- `consensus-engines/src/coordination/dynamic_engine.rs:149` **UW** [float-literal] `criteria_weights.insert("stake".to_string(), 0.3);`
- `consensus-engines/src/coordination/dynamic_engine.rs:150` **UW** [float-literal] `criteria_weights.insert("performance".to_string(), 0.2);`
- `consensus-engines/src/coordination/dynamic_engine.rs:151` **UW** [float-literal] `criteria_weights.insert("availability".to_string(), 0.1);`
- `consensus-engines/src/coordination/dynamic_engine.rs:176` **UW** [float-type] `pub learning_rate: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:185` **UW** [float-literal] `learning_rate: 0.1,`
- `consensus-engines/src/coordination/dynamic_engine.rs:196` **UW** [float-type] `pub avg_block_time: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:198` **UW** [float-type] `pub tx_throughput: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:200` **UW** [float-type] `pub network_latency: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:202` **UW** [float-type] `pub resource_efficiency: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:204` **UW** [float-type] `pub security_score: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:215` **UW** [float-type] `approval_percentage: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:221` **UW** [float-type] `metric_values: FxHashMap<String, f64>,`
- `consensus-engines/src/coordination/dynamic_engine.rs:233` **UW** [float-type] `efficiency_improvement: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:263` **UW** [float-type] `pub integrity_score: Option<f64>,`
- `consensus-engines/src/coordination/dynamic_engine.rs:270` **UW** [float-type] `pub overall_score: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:274` **UW** [float-type] `pub participation_rate: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:276` **UW** [float-type] `pub avg_latency: f64,`
- `consensus-engines/src/coordination/dynamic_engine.rs:325` **UW** [float-literal] `avg_block_time: 10.0,`
- `consensus-engines/src/coordination/dynamic_engine.rs:326` **UW** [float-literal] `tx_throughput: 100.0,`
- `consensus-engines/src/coordination/dynamic_engine.rs:330` **UW** [float-literal] `network_latency_ms: 100.0,`
- `consensus-engines/src/coordination/dynamic_engine.rs:394` **UW** [wall-clock] `let start_time = Instant::now();`
- `consensus-engines/src/coordination/dynamic_engine.rs:395` **UW** [wall-clock] `let timestamp = SystemTime::now()`
- `consensus-engines/src/coordination/dynamic_engine.rs:514` **UW** [float-literal] `let mut health_score = 1.0;`
- `consensus-engines/src/coordination/dynamic_engine.rs:519` **UW** [float-literal] `health_score *= 0.8;`
- `consensus-engines/src/coordination/dynamic_engine.rs:525` **UW** [float-literal] `health_score *= 0.7;`
- `consensus-engines/src/coordination/dynamic_engine.rs:531` **UW** [float-literal] `health_score *= 0.6;`
- `consensus-engines/src/coordination/dynamic_engine.rs:536` **UW** [float-literal] `1.0`
- `consensus-engines/src/coordination/dynamic_engine.rs:538` **UW** [float-literal] `0.0`
- `consensus-engines/src/coordination/dynamic_engine.rs:573` **UW** [wall-clock] `let expiry = SystemTime::now()`
- `consensus-engines/src/coordination/dynamic_engine.rs:688` **UW** [wall-clock] `let current_time = SystemTime::now()`
- `consensus-engines/src/coordination/dynamic_engine.rs:729` **UW** [float-literal] `resource_efficiency: 0.8,`
- `consensus-engines/src/coordination/dynamic_engine.rs:730` **UW** [float-literal] `security_score: 0.9,`
- `consensus-engines/src/coordination/dynamic_engine.rs:740` **UW** [float-type] `) -> ConsensusResult<f64> {`
- `consensus-engines/src/coordination/dynamic_engine.rs:741` **UW** [float-literal] `let mut score = 0.0;`
- `consensus-engines/src/coordination/dynamic_engine.rs:749` **UW** [float-literal] `_ => 0.5,`
- `consensus-engines/src/coordination/dynamic_engine.rs:755` **UW** [float-literal] `Ok(score.clamp(0.0, 1.0))`
- `consensus-engines/src/coordination/dynamic_engine.rs:758` **UW** [float-type] `async fn get_candidate_reputation(&self, _candidate: &str) -> f64 {`
- `consensus-engines/src/coordination/dynamic_engine.rs:759` **UW** [float-literal] `0.8`
- `consensus-engines/src/coordination/dynamic_engine.rs:762` **UW** [float-type] `async fn get_candidate_stake(&self, _candidate: &str) -> f64 {`
- `consensus-engines/src/coordination/dynamic_engine.rs:763` **UW** [float-literal] `0.7`
- `consensus-engines/src/coordination/dynamic_engine.rs:766` **UW** [float-type] `async fn get_candidate_performance(&self, _candidate: &str) -> f64 {`
- `consensus-engines/src/coordination/dynamic_engine.rs:767` **UW** [float-literal] `0.85`
- `consensus-engines/src/coordination/dynamic_engine.rs:770` **UW** [float-type] `async fn get_candidate_availability(&self, _candidate: &str) -> f64 {`
- `consensus-engines/src/coordination/dynamic_engine.rs:771` **UW** [float-literal] `0.9`

## `consensus-engines/src/selection/finality.rs` — 29 (UW×29)

- `consensus-engines/src/selection/finality.rs:21` **UW** [float-type] `pub const DEFAULT_ADVERSARY_THRESHOLD: f64 = 0.33;`
- `consensus-engines/src/selection/finality.rs:21` **UW** [float-literal] `pub const DEFAULT_ADVERSARY_THRESHOLD: f64 = 0.33;`
- `consensus-engines/src/selection/finality.rs:126` **UW** [float-type] `pub adversary_threshold: f64,`
- `consensus-engines/src/selection/finality.rs:130` **UW** [float-type] `pub base_confidence_per_depth: f64,`
- `consensus-engines/src/selection/finality.rs:138` **UW** [float-literal] `base_confidence_per_depth: 0.15,`
- `consensus-engines/src/selection/finality.rs:163` **UW** [float-type] `pub fn calculate(&self, depth: u64, adversary_fraction: Option<f64>) -> f64 {`
- `consensus-engines/src/selection/finality.rs:165` **UW** [float-literal] `return 0.0;`
- `consensus-engines/src/selection/finality.rs:171` **UW** [float-literal] `let a = a.clamp(0.0, 0.5);`
- `consensus-engines/src/selection/finality.rs:176` **UW** [float-literal] `let q = if a >= 0.5 {`
- `consensus-engines/src/selection/finality.rs:180` **UW** [float-literal] `a / (1.0 - a)`
- `consensus-engines/src/selection/finality.rs:184` **UW** [float-literal] `(1.0 - reorg_probability).clamp(0.0, 1.0)`
- `consensus-engines/src/selection/finality.rs:191` **UW** [float-type] `pub fn calculate_with_weight(&self, depth: u64, cumulative_weight: u64) -> f64 {`
- `consensus-engines/src/selection/finality.rs:196` **UW** [float-type] `(cumulative_weight as f64 / self.params.total_network_weight as f64).min(1.0)`
- `consensus-engines/src/selection/finality.rs:196` **UW** [float-literal] `(cumulative_weight as f64 / self.params.total_network_weight as f64).min(1.0)`
- `consensus-engines/src/selection/finality.rs:198` **UW** [float-literal] `0.0`
- `consensus-engines/src/selection/finality.rs:202` **UW** [float-literal] `let combined = base_probability * 0.7 + weight_ratio * 0.3;`
- `consensus-engines/src/selection/finality.rs:203` **UW** [float-literal] `combined.clamp(0.0, 1.0)`
- `consensus-engines/src/selection/finality.rs:217` **UW** [float-type] `pub slashing_fraction: f64,`
- `consensus-engines/src/selection/finality.rs:224` **UW** [float-type] `pub fn new(total_staked: u64, slashing_fraction: f64, min_colluding_validators: usize) -> Self {`
- `consensus-engines/src/selection/finality.rs:243` **UW** [float-type] `let slashable = (per_validator_stake as f64 * self.slashing_fraction) as u64;`
- `consensus-engines/src/selection/finality.rs:251` **UW** [float-type] `pub fn economic_finality_score(&self, block_value_at_risk: u64) -> f64 {`
- `consensus-engines/src/selection/finality.rs:254` **UW** [float-literal] `return 1.0;`
- `consensus-engines/src/selection/finality.rs:258` **UW** [float-type] `let ratio = cost as f64 / block_value_at_risk as f64;`
- `consensus-engines/src/selection/finality.rs:259` **UW** [float-literal] `ratio.clamp(0.0, 1.0)`
- `consensus-engines/src/selection/finality.rs:546` **UW** [float-type] `pub fn finality_probability(&self, block_height: u64) -> f64 {`
- `consensus-engines/src/selection/finality.rs:563` **UW** [float-type] `pub fn comprehensive_finality(&self, block_height: u64, value_at_risk: u64) -> f64 {`
- `consensus-engines/src/selection/finality.rs:622` **UW** [float-type] `slashing_fraction: f64,`
- `consensus-engines/src/selection/finality.rs:644` **UW** [float-type] `pub fn query_finality(&self, block_height: u64) -> f64 {`
- `consensus-engines/src/selection/finality.rs:649` **UW** [float-type] `pub fn query_comprehensive_finality(&self, block_height: u64, value_at_risk: u64) -> f64 {`

## `consensus-engines/src/security/attack_prevention.rs` — 26 (UW×26)

- `consensus-engines/src/security/attack_prevention.rs:37` **UW** [float-type] `pub max_stake_concentration: f64,`
- `consensus-engines/src/security/attack_prevention.rs:69` **UW** [float-type] `pub max_malicious_threshold: f64,`
- `consensus-engines/src/security/attack_prevention.rs:82` **UW** [float-literal] `max_stake_concentration: 0.10, // No single entity > 10% (PRD requirement)`
- `consensus-engines/src/security/attack_prevention.rs:98` **UW** [float-literal] `max_malicious_threshold: 0.33, // 33% Byzantine tolerance`
- `consensus-engines/src/security/attack_prevention.rs:109` **UW** [float-type] `pub score: f64,`
- `consensus-engines/src/security/attack_prevention.rs:121` **UW** [float-type] `pub connection_diversity: f64,`
- `consensus-engines/src/security/attack_prevention.rs:128` **UW** [float-literal] `score: 0.5, // Start with neutral reputation`
- `consensus-engines/src/security/attack_prevention.rs:131` **UW** [wall-clock] `last_activity: SystemTime::now()`
- `consensus-engines/src/security/attack_prevention.rs:137` **UW** [float-literal] `connection_diversity: 0.0,`
- `consensus-engines/src/security/attack_prevention.rs:261` **UW** [float-literal] `if reputation.score < 0.1 {`
- `consensus-engines/src/security/attack_prevention.rs:289` **UW** [float-literal] `if reputation.score < 0.2 {`
- `consensus-engines/src/security/attack_prevention.rs:309` **UW** [wall-clock] `let now = SystemTime::now()`
- `consensus-engines/src/security/attack_prevention.rs:408` **UW** [float-type] `let stake_percentage = node_stake as f64 / total_stake as f64;`
- `consensus-engines/src/security/attack_prevention.rs:421` **UW** [float-type] `let pool_percentage = pool_stake as f64 / total_stake as f64;`
- `consensus-engines/src/security/attack_prevention.rs:469` **UW** [float-literal] `reputation.score = (reputation.score * 0.95 + 0.05).min(1.0);`
- `consensus-engines/src/security/attack_prevention.rs:473` **UW** [float-literal] `reputation.score = (reputation.score * 0.9 - 0.1).max(0.0);`
- `consensus-engines/src/security/attack_prevention.rs:477` **UW** [float-literal] `reputation.score = (reputation.score * 0.98 - 0.02).max(0.0);`
- `consensus-engines/src/security/attack_prevention.rs:480` **UW** [float-literal] `reputation.score = (reputation.score * 0.8 - 0.2).max(0.0);`
- `consensus-engines/src/security/attack_prevention.rs:484` **UW** [float-literal] `reputation.score = 0.0;`
- `consensus-engines/src/security/attack_prevention.rs:490` **UW** [float-literal] `if reputation.score < 0.05 \|\| reputation.penalties > 20 {`
- `consensus-engines/src/security/attack_prevention.rs:571` **UW** [wall-clock] `timestamp: SystemTime::now()`
- `consensus-engines/src/security/attack_prevention.rs:600` **UW** [float-type] `reputations.values().map(\|r\| r.score).sum::<f64>() / total_nodes as f64`
- `consensus-engines/src/security/attack_prevention.rs:602` **UW** [float-literal] `0.0`
- `consensus-engines/src/security/attack_prevention.rs:608` **UW** [float-type] `max_stake as f64 / total_stake as f64`
- `consensus-engines/src/security/attack_prevention.rs:665` **UW** [float-type] `pub avg_reputation: f64,`
- `consensus-engines/src/security/attack_prevention.rs:667` **UW** [float-type] `pub stake_concentration: f64,`

## `consensus-engines/src/security/correlation_penalties.rs` — 24 (UW×24)

- `consensus-engines/src/security/correlation_penalties.rs:10` **UW** [float-type] `pub const MIN_PENALTY_FRACTION: f64 = 0.001;`
- `consensus-engines/src/security/correlation_penalties.rs:10` **UW** [float-literal] `pub const MIN_PENALTY_FRACTION: f64 = 0.001;`
- `consensus-engines/src/security/correlation_penalties.rs:13` **UW** [float-type] `pub const MAX_PENALTY_FRACTION: f64 = 1.0;`
- `consensus-engines/src/security/correlation_penalties.rs:13` **UW** [float-literal] `pub const MAX_PENALTY_FRACTION: f64 = 1.0;`
- `consensus-engines/src/security/correlation_penalties.rs:19` **UW** [float-type] `pub const DEFAULT_WHISTLEBLOWER_REWARD_FRACTION: f64 = 0.05;`
- `consensus-engines/src/security/correlation_penalties.rs:19` **UW** [float-literal] `pub const DEFAULT_WHISTLEBLOWER_REWARD_FRACTION: f64 = 0.05;`
- `consensus-engines/src/security/correlation_penalties.rs:36` **UW** [float-type] `fn base_penalty_fraction(&self) -> f64 {`
- `consensus-engines/src/security/correlation_penalties.rs:38` **UW** [float-literal] `SlashingViolationType::DoubleSign => 0.05,   // 5%`
- `consensus-engines/src/security/correlation_penalties.rs:39` **UW** [float-literal] `SlashingViolationType::Surround => 0.05,     // 5%`
- `consensus-engines/src/security/correlation_penalties.rs:40` **UW** [float-literal] `SlashingViolationType::Downtime => 0.01,     // 1%`
- `consensus-engines/src/security/correlation_penalties.rs:41` **UW** [float-literal] `SlashingViolationType::InvalidBlock => 0.03, // 3%`
- `consensus-engines/src/security/correlation_penalties.rs:83` **UW** [float-type] `pub correlation_multiplier: f64,`
- `consensus-engines/src/security/correlation_penalties.rs:87` **UW** [float-type] `pub penalty_fraction: f64,`
- `consensus-engines/src/security/correlation_penalties.rs:100` **UW** [float-type] `pub whistleblower_reward_fraction: f64,`
- `consensus-engines/src/security/correlation_penalties.rs:102` **UW** [float-type] `pub min_penalty_fraction: f64,`
- `consensus-engines/src/security/correlation_penalties.rs:104` **UW** [float-type] `pub max_penalty_fraction: f64,`
- `consensus-engines/src/security/correlation_penalties.rs:129` **UW** [float-type] `fn apply_fraction_to_stake(stake: u64, fraction: f64) -> u64 {`
- `consensus-engines/src/security/correlation_penalties.rs:130` **UW** [float-literal] `if !fraction.is_finite() \|\| fraction <= 0.0 {`
- `consensus-engines/src/security/correlation_penalties.rs:133` **UW** [float-literal] `if fraction >= 1.0 {`
- `consensus-engines/src/security/correlation_penalties.rs:137` **UW** [float-type] `(stake as f64 * fraction) as u64`
- `consensus-engines/src/security/correlation_penalties.rs:194` **UW** [float-type] `let fraction_slashed = total_slashed_stake as f64 / total_network_stake as f64;`
- `consensus-engines/src/security/correlation_penalties.rs:199` **UW** [float-literal] `let quadratic = fraction_slashed * fraction_slashed * 3.0;`
- `consensus-engines/src/security/correlation_penalties.rs:201` **UW** [float-literal] `quadratic.max(1.0)`
- `consensus-engines/src/security/correlation_penalties.rs:203` **UW** [float-literal] `1.0`

## `consensus-engines/src/security/bridge.rs` — 22 (UW×22)

- `consensus-engines/src/security/bridge.rs:145` **UW** [float-type] `pub network_latency: f64,`
- `consensus-engines/src/security/bridge.rs:147` **UW** [float-type] `pub resource_utilization: f64,`
- `consensus-engines/src/security/bridge.rs:149` **UW** [float-type] `pub threat_detection_accuracy: f64,`
- `consensus-engines/src/security/bridge.rs:166` **UW** [float-type] `pub network_health_score: f64,`
- `consensus-engines/src/security/bridge.rs:207` **UW** [wall-clock] `last_scan_timestamp: SystemTime::now()`
- `consensus-engines/src/security/bridge.rs:212` **UW** [float-literal] `network_health_score: 1.0,`
- `consensus-engines/src/security/bridge.rs:238` **UW** [wall-clock] `let start_time = Instant::now();`
- `consensus-engines/src/security/bridge.rs:245` **UW** [wall-clock] `let security_start = Instant::now();`
- `consensus-engines/src/security/bridge.rs:304` **UW** [wall-clock] `let consensus_start = Instant::now();`
- `consensus-engines/src/security/bridge.rs:311` **UW** [wall-clock] `timestamp: SystemTime::now()`
- `consensus-engines/src/security/bridge.rs:520` **UW** [float-literal] `confidence_score: 0.9,`
- `consensus-engines/src/security/bridge.rs:653` **UW** [float-literal] `/ 2.0,`
- `consensus-engines/src/security/bridge.rs:706` **UW** [wall-clock] `state.last_scan_timestamp = SystemTime::now()`
- `consensus-engines/src/security/bridge.rs:773` **UW** [float-literal] `confidence_score: 0.95,`
- `consensus-engines/src/security/bridge.rs:924` **UW** [float-literal] `network_latency: 50.0, // Would be measured from actual network`
- `consensus-engines/src/security/bridge.rs:925` **UW** [float-literal] `resource_utilization: 0.7,`
- `consensus-engines/src/security/bridge.rs:926` **UW** [float-literal] `threat_detection_accuracy: 0.95,`
- `consensus-engines/src/security/bridge.rs:968` **UW** [float-literal] `network_latency: 50.0,`
- `consensus-engines/src/security/bridge.rs:969` **UW** [float-literal] `resource_utilization: 0.3,`
- `consensus-engines/src/security/bridge.rs:1027` **UW** [float-literal] `let success_rate = if result.success { 1.0 } else { 0.0 };`
- `consensus-engines/src/security/bridge.rs:1028` **UW** [float-literal] `state.network_health_score = (state.network_health_score * 0.9) + (success_rate * 0.1);`
- `consensus-engines/src/security/bridge.rs:1052` **UW** [float-type] `pub integrity_score: Option<f64>,`

## `consensus-engines/src/coordination/gossip.rs` — 17 (UW×17)

- `consensus-engines/src/coordination/gossip.rs:40` **UW** [wall-clock] `timestamp: chrono::Utc::now().timestamp() as u64,`
- `consensus-engines/src/coordination/gossip.rs:113` **UW** [float-type] `pub push_probability: f64,`
- `consensus-engines/src/coordination/gossip.rs:116` **UW** [float-type] `pub pull_probability: f64,`
- `consensus-engines/src/coordination/gossip.rs:140` **UW** [float-literal] `push_probability: 0.8,`
- `consensus-engines/src/coordination/gossip.rs:141` **UW** [float-literal] `pull_probability: 0.2,`
- `consensus-engines/src/coordination/gossip.rs:190` **UW** [wall-clock] `last_update: chrono::Utc::now().timestamp() as u64,`
- `consensus-engines/src/coordination/gossip.rs:198` **UW** [wall-clock] `self.last_update = chrono::Utc::now().timestamp() as u64;`
- `consensus-engines/src/coordination/gossip.rs:274` **UW** [wall-clock] `created_at: chrono::Utc::now().timestamp() as u64,`
- `consensus-engines/src/coordination/gossip.rs:285` **UW** [wall-clock] `let now = chrono::Utc::now().timestamp() as u64;`
- `consensus-engines/src/coordination/gossip.rs:371` **UW** [float-type] `pub failure_score: f64,`
- `consensus-engines/src/coordination/gossip.rs:415` **UW** [float-type] `pub avg_convergence_time: f64,`
- `consensus-engines/src/coordination/gossip.rs:502` **UW** [float-literal] `avg_convergence_time: 0.0,`
- `consensus-engines/src/coordination/gossip.rs:511` **UW** [wall-clock] `let last_bandwidth_refill = Arc::new(Mutex::new(Instant::now()));`
- `consensus-engines/src/coordination/gossip.rs:553` **UW** [wall-clock] `last_seen: chrono::Utc::now().timestamp() as u64,`
- `consensus-engines/src/coordination/gossip.rs:554` **UW** [wall-clock] `joined_at: chrono::Utc::now().timestamp() as u64,`
- `consensus-engines/src/coordination/gossip.rs:555` **UW** [float-literal] `failure_score: 0.0,`
- `consensus-engines/src/coordination/gossip.rs:558` **UW** [float-literal] `protocol_version: "1.0.0".to_string(),`

## `consensus-engines/src/security/audit.rs` — 16 (UW×16)

- `consensus-engines/src/security/audit.rs:121` **UW** [float-type] `pub confidence: f64,`
- `consensus-engines/src/security/audit.rs:127` **UW** [float-type] `pub cvss_score: Option<f64>,`
- `consensus-engines/src/security/audit.rs:165` **UW** [float-type] `pub malicious_node_percentage: f64,`
- `consensus-engines/src/security/audit.rs:177` **UW** [float-literal] `malicious_node_percentage: 0.33, // 33% malicious for Byzantine testing`
- `consensus-engines/src/security/audit.rs:276` **UW** [wall-clock] `let start_time = Instant::now();`
- `consensus-engines/src/security/audit.rs:277` **UW** [wall-clock] `let audit_timestamp = SystemTime::now()`
- `consensus-engines/src/security/audit.rs:341` **UW** [float-literal] `methodology_version: "1.0.0".to_string(),`
- `consensus-engines/src/security/audit.rs:379` **UW** [float-literal] `confidence: 0.8,`
- `consensus-engines/src/security/audit.rs:389` **UW** [float-literal] `cvss_score: Some(7.5),`
- `consensus-engines/src/security/audit.rs:439` **UW** [float-literal] `confidence: 0.9,`
- `consensus-engines/src/security/audit.rs:444` **UW** [float-literal] `cvss_score: Some(8.5),`
- `consensus-engines/src/security/audit.rs:489` **UW** [float-literal] `confidence: 0.7,`
- `consensus-engines/src/security/audit.rs:494` **UW** [float-literal] `cvss_score: Some(5.5),`
- `consensus-engines/src/security/audit.rs:551` **UW** [float-literal] `cvss_score: Some(6.0),`
- `consensus-engines/src/security/audit.rs:592` **UW** [float-literal] `confidence: 0.6,`
- `consensus-engines/src/security/audit.rs:597` **UW** [float-literal] `cvss_score: Some(4.5),`

## `consensus-engines/src/algorithms/raft.rs` — 15 (UW×15)

- `consensus-engines/src/algorithms/raft.rs:205` **UW** [float-type] `pub reputation: f64,`
- `consensus-engines/src/algorithms/raft.rs:235` **UW** [float-type] `pub avg_replication_latency: f64,`
- `consensus-engines/src/algorithms/raft.rs:237` **UW** [float-type] `pub heartbeat_success_rate: f64,`
- `consensus-engines/src/algorithms/raft.rs:241` **UW** [float-type] `pub avg_leadership_tenure: f64,`
- `consensus-engines/src/algorithms/raft.rs:250` **UW** [float-literal] `avg_replication_latency: 0.0,`
- `consensus-engines/src/algorithms/raft.rs:251` **UW** [float-literal] `heartbeat_success_rate: 1.0,`
- `consensus-engines/src/algorithms/raft.rs:253` **UW** [float-literal] `avg_leadership_tenure: 0.0,`
- `consensus-engines/src/algorithms/raft.rs:341` **UW** [wall-clock] `last_heartbeat: SystemTime::now()`
- `consensus-engines/src/algorithms/raft.rs:346` **UW** [float-literal] `reputation: 1.0,`
- `consensus-engines/src/algorithms/raft.rs:347` **UW** [wall-clock] `joined_at: SystemTime::now()`
- `consensus-engines/src/algorithms/raft.rs:371` **UW** [wall-clock] `last_heartbeat: Arc::new(RwLock::new(Instant::now())),`
- `consensus-engines/src/algorithms/raft.rs:417` **UW** [wall-clock] `let timestamp = SystemTime::now()`
- `consensus-engines/src/algorithms/raft.rs:828` **UW** [wall-clock] `created_at: SystemTime::now()`
- `consensus-engines/src/algorithms/raft.rs:934` **UW** [unseeded-rng] `let mut rng = rand::thread_rng();`
- `consensus-engines/src/algorithms/raft.rs:939` **UW** [wall-clock] `*self.last_heartbeat.write().unwrap() = Instant::now();`

## `consensus-engines/src/types.rs` — 15 (UW×15)

- `consensus-engines/src/types.rs:295` **UW** [float-type] `pub quality_score: f64,`
- `consensus-engines/src/types.rs:354` **UW** [float-type] `pub score: f64,`
- `consensus-engines/src/types.rs:383` **UW** [float-type] `pub quality: f64,`
- `consensus-engines/src/types.rs:386` **UW** [float-type] `pub uniqueness: f64,`
- `consensus-engines/src/types.rs:389` **UW** [float-type] `pub timeliness: f64,`
- `consensus-engines/src/types.rs:392` **UW** [float-type] `pub verification: f64,`
- `consensus-engines/src/types.rs:395` **UW** [float-type] `pub reputation: f64,`
- `consensus-engines/src/types.rs:398` **UW** [float-type] `pub total_score: f64,`
- `consensus-engines/src/types.rs:425` **UW** [float-literal] `quality: 0.3,`
- `consensus-engines/src/types.rs:426` **UW** [float-literal] `uniqueness: 0.25,`
- `consensus-engines/src/types.rs:427` **UW** [float-literal] `timeliness: 0.15,`
- `consensus-engines/src/types.rs:428` **UW** [float-literal] `verification: 0.2,`
- `consensus-engines/src/types.rs:429` **UW** [float-literal] `reputation: 0.1,`
- `consensus-engines/src/types.rs:466` **UW** [float-type] `pub efficiency: f64,`
- `consensus-engines/src/types.rs:469` **UW** [float-type] `pub power_consumption: Option<f64>,`

## `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs` — 12 (UW×12)

- `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs:197` **UW** [wall-clock] `created_at: std::time::Instant::now(),`
- `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs:440` **UW** [float-type] `let latency = slot.created_at.elapsed().as_millis() as f64;`
- `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs:441` **UW** [float-type] `let total_latency = metrics.avg_commit_latency_ms * metrics.blocks_committed as f64;`
- `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs:443` **UW** [float-type] `(total_latency + latency) / (metrics.blocks_committed + 1) as f64;`
- `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs:611` **UW** [float-type] `fast_path_threshold: f64,`
- `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs:625` **UW** [float-literal] `fast_path_threshold: 0.9, // 90% of votes needed for fast path`
- `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs:641` **UW** [float-type] `let success_rate = self.fast_path_successes as f64 / total as f64;`
- `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs:642` **UW** [float-literal] `success_rate > 0.5 // Continue with fast path if >50% success`
- `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs:654` **UW** [float-literal] `let alpha = 0.1;`
- `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs:655` **UW** [float-type] `let current_ms = self.current_delay_estimate.as_millis() as f64;`
- `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs:656` **UW** [float-type] `let actual_ms = actual_delay.as_millis() as f64;`
- `consensus-engines/src/algorithms/hotstuff/pipelined_voting.rs:657` **UW** [float-literal] `let new_estimate = (1.0 - alpha) * current_ms + alpha * actual_ms;`

## `shared-protocol/src/consensus/task_orchestrator.rs` — 12 (UW×12)

- `shared-protocol/src/consensus/task_orchestrator.rs:16` **UW** [float-type] `pub same_region_bonus: f64,`
- `shared-protocol/src/consensus/task_orchestrator.rs:18` **UW** [float-type] `pub distance_weight: f64,`
- `shared-protocol/src/consensus/task_orchestrator.rs:27` **UW** [float-literal] `same_region_bonus: 2.0,`
- `shared-protocol/src/consensus/task_orchestrator.rs:28` **UW** [float-literal] `distance_weight: 0.5,`
- `shared-protocol/src/consensus/task_orchestrator.rs:70` **UW** [float-type] `pub observation_density: Option<f64>,`
- `shared-protocol/src/consensus/task_orchestrator.rs:203` **UW** [float-type] `let mut best: Option<(String, String, f64)> = None;`
- `shared-protocol/src/consensus/task_orchestrator.rs:331` **UW** [float-literal] `if policy.same_region_bonus < 0.0 {`
- `shared-protocol/src/consensus/task_orchestrator.rs:336` **UW** [float-literal] `if policy.distance_weight < 0.0 {`
- `shared-protocol/src/consensus/task_orchestrator.rs:382` **UW** [float-type] `fn compute_assignment_weight(&self, task: &ForecastTask, node: &ComputeNode) -> f64 {`
- `shared-protocol/src/consensus/task_orchestrator.rs:383` **UW** [float-literal] `let mut weight = 1.0;`
- `shared-protocol/src/consensus/task_orchestrator.rs:413` **UW** [float-type] `weight *= self.data_desert_policy.desert_bonus_bps as f64 / 100.0;`
- `shared-protocol/src/consensus/task_orchestrator.rs:413` **UW** [float-literal] `weight *= self.data_desert_policy.desert_bonus_bps as f64 / 100.0;`

## `consensus-engines/src/economics/smart_contract.rs` — 11 (UW×11)

- `consensus-engines/src/economics/smart_contract.rs:80` **UW** [float-type] `pub reputation: f64,`
- `consensus-engines/src/economics/smart_contract.rs:88` **UW** [float-type] `pub commission_rate: f64,`
- `consensus-engines/src/economics/smart_contract.rs:108` **UW** [wall-clock] `let now = SystemTime::now()`
- `consensus-engines/src/economics/smart_contract.rs:215` **UW** [float-type] `pub avg_block_time: f64,`
- `consensus-engines/src/economics/smart_contract.rs:217` **UW** [float-type] `pub tx_throughput: f64,`
- `consensus-engines/src/economics/smart_contract.rs:219` **UW** [float-type] `pub network_latency_ms: f64,`
- `consensus-engines/src/economics/smart_contract.rs:221` **UW** [float-type] `pub consensus_participation: f64,`
- `consensus-engines/src/economics/smart_contract.rs:266` **UW** [float-type] `pub accuracy_score: f64,`
- `consensus-engines/src/economics/smart_contract.rs:277` **UW** [float-type] `pub cpu_usage: f64,`
- `consensus-engines/src/economics/smart_contract.rs:302` **UW** [float-type] `pub training_accuracy: f64,`
- `consensus-engines/src/economics/smart_contract.rs:304` **UW** [float-type] `pub validation_accuracy: f64,`

## `consensus-engines/src/validator.rs` — 11 (UW×11)

- `consensus-engines/src/validator.rs:13` **UW** [float-literal] `const PENALTY_BPS_MINOR: u64 = 100; // 1.00%`
- `consensus-engines/src/validator.rs:14` **UW** [float-literal] `const PENALTY_BPS_MAJOR: u64 = 500; // 5.00%`
- `consensus-engines/src/validator.rs:15` **UW** [float-literal] `const PENALTY_BPS_CRITICAL: u64 = 1_000; // 10.00%`
- `consensus-engines/src/validator.rs:86` **UW** [float-type] `pub validation_accuracy: f64,`
- `consensus-engines/src/validator.rs:88` **UW** [float-type] `pub uptime: f64,`
- `consensus-engines/src/validator.rs:98` **UW** [float-literal] `validation_accuracy: 1.0,`
- `consensus-engines/src/validator.rs:99` **UW** [float-literal] `uptime: 1.0,`
- `consensus-engines/src/validator.rs:145` **UW** [wall-clock] `let now = SystemTime::now()`
- `consensus-engines/src/validator.rs:171` **UW** [float-type] `pub slashing_rate: f64,`
- `consensus-engines/src/validator.rs:179` **UW** [float-literal] `slashing_rate: 0.05,`
- `consensus-engines/src/validator.rs:266` **UW** [wall-clock] `timestamp: SystemTime::now()`

## `consensus-engines/src/coordination/byzantine.rs` — 10 (UW×10)

- `consensus-engines/src/coordination/byzantine.rs:95` **UW** [float-type] `pub partition_tolerance: f64,`
- `consensus-engines/src/coordination/byzantine.rs:97` **UW** [float-type] `pub consensus_efficiency: f64,`
- `consensus-engines/src/coordination/byzantine.rs:131` **UW** [float-type] `byzantine_threshold: f64,`
- `consensus-engines/src/coordination/byzantine.rs:134` **UW** [float-type] `let max_byzantine_nodes = ((total_nodes as f64) * byzantine_threshold).floor() as u32;`
- `consensus-engines/src/coordination/byzantine.rs:286` **UW** [float-type] `score: f64,`
- `consensus-engines/src/coordination/byzantine.rs:292` **UW** [float-literal] `if !(0.0..=100.0).contains(&score) {`
- `consensus-engines/src/coordination/byzantine.rs:300` **UW** [float-literal] `let consensus_threshold = 50.0;`
- `consensus-engines/src/coordination/byzantine.rs:316` **UW** [float-literal] `partition_tolerance: 1.0,`
- `consensus-engines/src/coordination/byzantine.rs:317` **UW** [float-literal] `consensus_efficiency: 1.0,`
- `consensus-engines/src/coordination/byzantine.rs:341` **UW** [float-type] `(self.network_health.honest_nodes as f64) / (self.consensus.config.total_nodes as f64);`

## `consensus-engines/src/coordination/hard_fork.rs` — 9 (UW×9)

- `consensus-engines/src/coordination/hard_fork.rs:13` **UW** [float-type] `pub const DEFAULT_SIGNALING_THRESHOLD: f64 = 0.75;`
- `consensus-engines/src/coordination/hard_fork.rs:13` **UW** [float-literal] `pub const DEFAULT_SIGNALING_THRESHOLD: f64 = 0.75;`
- `consensus-engines/src/coordination/hard_fork.rs:159` **UW** [float-type] `threshold: f64,`
- `consensus-engines/src/coordination/hard_fork.rs:174` **UW** [float-type] `pub fn new(fork_name: String, threshold: f64, signaling_period: u64, start_block: u64) -> Self {`
- `consensus-engines/src/coordination/hard_fork.rs:206` **UW** [float-type] `pub fn signaling_percentage(&self) -> f64 {`
- `consensus-engines/src/coordination/hard_fork.rs:208` **UW** [float-literal] `return 0.0;`
- `consensus-engines/src/coordination/hard_fork.rs:210` **UW** [float-type] `self.signaling_blocks as f64 / self.total_blocks as f64`
- `consensus-engines/src/coordination/hard_fork.rs:424` **UW** [float-type] `signaling_threshold: f64,`
- `consensus-engines/src/coordination/hard_fork.rs:431` **UW** [float-type] `pub fn new(local_version: String, signaling_threshold: f64, signaling_period: u64) -> Self {`

## `consensus-engines/src/algorithms/pow.rs` — 6 (UW×6)

- `consensus-engines/src/algorithms/pow.rs:107` **UW** [float-type] `let actual_span = (last - first).max(1) as f64;`
- `consensus-engines/src/algorithms/pow.rs:113` **UW** [float-type] `* self.target_block_time.max(1)) as f64;`
- `consensus-engines/src/algorithms/pow.rs:114` **UW** [float-literal] `let adjustment = (expected_span / actual_span).clamp(0.25, 4.0);`
- `consensus-engines/src/algorithms/pow.rs:115` **UW** [float-type] `((self.difficulty as f64) * adjustment).round().max(1.0) as u64`
- `consensus-engines/src/algorithms/pow.rs:115` **UW** [float-literal] `((self.difficulty as f64) * adjustment).round().max(1.0) as u64`
- `consensus-engines/src/algorithms/pow.rs:119` **UW** [wall-clock] `let start = std::time::Instant::now();`

## `consensus-engines/src/algorithms/pos.rs` — 3 (UW×3)

- `consensus-engines/src/algorithms/pos.rs:201` **UW** [float-literal] `reputation: 1.0,`
- `consensus-engines/src/algorithms/pos.rs:379` **UW** [wall-clock] `detected_at: SystemTime::now()`
- `consensus-engines/src/algorithms/pos.rs:589` **UW** [wall-clock] `std::time::SystemTime::now()`

## `consensus-engines/src/algorithms/hotstuff/types.rs` — 2 (UW×2)

- `consensus-engines/src/algorithms/hotstuff/types.rs:99` **UW** [wall-clock] `let timestamp = SystemTime::now()`
- `consensus-engines/src/algorithms/hotstuff/types.rs:531` **UW** [float-type] `pub avg_commit_latency_ms: f64,`

## `consensus-engines/src/coordination/proposer_builder_separation.rs` — 2 (UW×2)

- `consensus-engines/src/coordination/proposer_builder_separation.rs:133` **UW** [float-type] `pub reliability_score: f64,`
- `consensus-engines/src/coordination/proposer_builder_separation.rs:190` **UW** [float-literal] `reliability_score: 1.0,`

## `consensus-engines/src/algorithms/dag.rs` — 1 (UW×1)

- `consensus-engines/src/algorithms/dag.rs:668` **UW** [wall-clock] `let timestamp = std::time::SystemTime::now()`

## `consensus-engines/src/algorithms/dpos.rs` — 1 (UW×1)

- `consensus-engines/src/algorithms/dpos.rs:565` **UW** [wall-clock] `std::time::SystemTime::now()`

## `consensus-engines/src/algorithms/hotstuff/crypto.rs` — 1 (UW×1)

- `consensus-engines/src/algorithms/hotstuff/crypto.rs:71` **UW** [unseeded-rng] `let signing_key = SigningKey::generate(&mut rand::thread_rng());`

## `consensus-engines/src/algorithms/hotstuff/mod.rs` — 1 (UW×1)

- `consensus-engines/src/algorithms/hotstuff/mod.rs:683` **UW** [wall-clock] `std::time::SystemTime::now()`

## `consensus-engines/src/economics/eip1559.rs` — 1 (UW×1)

- `consensus-engines/src/economics/eip1559.rs:89` **UW** [float-literal] `min_base_fee: 1_000_000,       // 0.001 Gwei`

## `consensus-engines/src/selection/vrf.rs` — 1 (UW×1)

- `consensus-engines/src/selection/vrf.rs:129` **UW** [unseeded-rng] `rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut seed);`
