use actix_web::http::header::{HeaderValue, RETRY_AFTER};
use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::helpers::{bearer_token_from_request, envelope_error, require_role};
use crate::state::{
    AppState, EXECUTION_QUOTA_MAX_TRACKED_TOKENS, EXECUTION_QUOTA_RETRY_AFTER_SECS,
    ExecutionQuotaBucket, ExecutionQuotaConfig, ExecutionQuotaDenied,
};
use wrfcoin_native_chain::security::api_auth::ApiRole;
#[cfg(feature = "p2p")]
use wrfcoin_native_chain::security::api_auth::AuthPrincipal;
#[cfg(feature = "p2p")]
use wrfcoin_execution_engine::manifest::{build_forecast_manifest, hash_input};
use wrfcoin_execution_engine::pipeline::ExecutionPipeline;
use wrfcoin_execution_engine::types::{ModelRunSpec, ResourceLimits};
#[cfg(feature = "p2p")]
use wrfcoin_p2p::P2pCommand;

/// Maximum number of completed run results to keep in memory.
const MAX_EXECUTION_RESULTS: usize = 1000;

#[cfg(feature = "p2p")]
fn forecast_node_id_from_token(
    token_store: &FxHashMap<String, AuthPrincipal>,
    token: &str,
) -> String {
    token_store
        .get(token)
        .and_then(|principal| {
            principal.identity.as_ref().and_then(|identity| {
                identity.oracle_node_id.clone().or(identity.contributor_id.clone())
            })
        })
        .or_else(|| token_store.get(token).and_then(|principal| principal.identity.as_ref().and_then(|identity| identity.wallet_address.clone())))
        .unwrap_or_else(|| anonymous_forecast_node_id(token))
}

/// Opaque fallback node id for authenticated tokens with no linked identity.
///
/// The id crosses the P2P boundary as a `node_id` and peers write node ids to
/// their debug logs, so it must not contain — or be invertible back into — the
/// bearer token (#2259). Hashing under a process-local random key keeps the id
/// stable per token within a process while making the mapping one-way even for
/// low-entropy tokens.
#[cfg(feature = "p2p")]
fn anonymous_forecast_node_id(token: &str) -> String {
    use rand::RngCore;
    use std::sync::LazyLock;

    static NODE_ID_KEY: LazyLock<[u8; 32]> = LazyLock::new(|| {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        key
    });

    let digest = blake3::keyed_hash(&NODE_ID_KEY, token.as_bytes());
    format!("anon:{}", &digest.to_hex()[..16])
}

// ---------------------------------------------------------------------------
// Per-token quota plumbing (#1244)
// ---------------------------------------------------------------------------

/// RAII guard that decrements a token's in-flight execution counter when
/// dropped.  The tokio background task that owns the execution run holds
/// this guard so the in-flight counter is released whether the job
/// succeeds, fails, or panics (drop always runs).
pub(crate) struct InFlightGuard {
    quotas: Arc<RwLock<FxHashMap<String, ExecutionQuotaBucket>>>,
    token_key: String,
}

impl std::fmt::Debug for InFlightGuard {
    // Hand-implemented: `token_key` is the caller's bearer token, so a derived
    // impl would print it in any `{:?}` of the guard (#2259).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InFlightGuard")
            .field("token_key", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        // Moving out of `self` in drop requires a take.
        let quotas = self.quotas.clone();
        let key = std::mem::take(&mut self.token_key);
        if key.is_empty() {
            return;
        }
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    let mut map = quotas.write().await;
                    if let Some(bucket) = map.get_mut(&key) {
                        bucket.release();
                    }
                });
            },
            Err(_) => {
                // No runtime (synchronous test teardown) — fall back to
                // a non-blocking try_write so drop can't deadlock.
                if let Ok(mut map) = quotas.try_write() {
                    if let Some(bucket) = map.get_mut(&key) {
                        bucket.release();
                    }
                }
            },
        }
    }
}

/// Attempt to charge a new execution run against the per-token quota.
///
/// On success returns an [`InFlightGuard`] that the caller MUST hand to the
/// background execution task so the in-flight counter is released when the
/// job finishes. On failure returns the specific [`ExecutionQuotaDenied`]
/// reason so the caller can build the correct 429 envelope.
pub(crate) async fn charge_execution_quota(
    quotas: &Arc<RwLock<FxHashMap<String, ExecutionQuotaBucket>>>,
    config: &ExecutionQuotaConfig,
    token_key: &str,
    now_ms: i64,
) -> Result<InFlightGuard, ExecutionQuotaDenied> {
    let mut map = quotas.write().await;

    // Capacity sweep: drop idle buckets before inserting a new one.
    if map.len() > EXECUTION_QUOTA_MAX_TRACKED_TOKENS {
        map.retain(|_k, b| !b.is_idle_after_prune(now_ms, config.window_ms));
    }

    let bucket = map
        .entry(token_key.to_string())
        .or_insert_with(ExecutionQuotaBucket::new);
    bucket.try_admit(
        now_ms,
        config.window_ms,
        config.max_runs_per_window,
        config.max_inflight,
    )?;

    Ok(InFlightGuard {
        quotas: quotas.clone(),
        token_key: token_key.to_string(),
    })
}

fn quota_denied_response(req: &HttpRequest, reason: ExecutionQuotaDenied) -> HttpResponse {
    let mut response = envelope_error(
        req,
        StatusCode::TOO_MANY_REQUESTS,
        reason.code(),
        reason.message(),
    );
    response.headers_mut().insert(
        RETRY_AFTER,
        HeaderValue::from_str(&EXECUTION_QUOTA_RETRY_AFTER_SECS.to_string())
            .unwrap_or_else(|_| HeaderValue::from_static("60")),
    );
    response
}

fn now_ms_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct RunModelRequest {
    pub model_id: String,
    pub region: String,
    pub horizon_hours: u32,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct RunModelResponse {
    pub run_id: String,
    pub model_id: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct ListModelsResponse {
    pub models: Vec<String>,
}

/// POST /execution/run — submit a model run request.
///
/// Requires a valid API token with at least Contributor role.  Each token
/// is also subject to per-token quotas (#1244):
///   * a sliding-window submission rate (runs per minute), and
///   * a concurrent in-flight run cap.
/// When either cap is exceeded the endpoint returns HTTP 429 with a
/// `Retry-After` header so clients can back off deterministically.
pub async fn submit_run(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<RunModelRequest>,
) -> impl Responder {
    // 1. AuthN + AuthZ: require at least Contributor role. The quota map
    //    is keyed on the bearer token string, so a valid token must be
    //    present before we touch the quota.
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }

    let token_key = match bearer_token_from_request(&req) {
        Some(t) => t.to_string(),
        None => {
            // require_role already rejected the missing/malformed case, so
            // this path is defensive only.
            return envelope_error(
                &req,
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "Missing or malformed Authorization header",
            );
        },
    };

    // 2. Per-token quota enforcement. Reserves a slot + bumps in-flight
    //    count on success, returning a guard that MUST travel with the
    //    spawned background task so the counter is released when the
    //    Docker job finishes.
    let guard = match charge_execution_quota(
        &state.execution_token_quotas,
        &state.execution_quota_config,
        &token_key,
        now_ms_epoch(),
    )
    .await
    {
        Ok(g) => g,
        Err(reason) => {
            return quota_denied_response(&req, reason);
        },
    };

    // 3. Validate the requested model exists in the registry.
    let registry = &state.execution_registry;
    if registry.get(&request.model_id).is_none() {
        // `guard` drops here, releasing the in-flight slot we just took.
        return envelope_error(
            &req,
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            format!("unknown model_id: {}", request.model_id),
        );
    }

    // Generate a unique run ID
    let run_id = format!(
        "run-{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            & 0xFFFF_FFFF_FFFF
    );

    // Construct the run spec
    let spec = ModelRunSpec {
        run_id: run_id.clone(),
        model_id: request.model_id.clone(),
        region: request.region.clone(),
        horizon_hours: request.horizon_hours,
        resource_limits: ResourceLimits::default(),
        params: request.params.clone().unwrap_or(serde_json::json!({})),
    };

    // Spawn background execution
    let runtime = state.execution_runtime.clone();
    let results = state.execution_results.clone();
    let model_id = request.model_id.clone();
    let registry_arc = state.execution_registry.clone();
    #[cfg(feature = "p2p")]
    let p2p = state.p2p.clone();
    #[cfg(feature = "p2p")]
    let forecast_node_id = forecast_node_id_from_token(&state.token_store, &token_key);

    tokio::spawn(async move {
        // Keep the quota guard alive for the lifetime of the background
        // job. When this task ends (success, error, or panic) the guard
        // drops and the token's in-flight counter is decremented.
        let _quota_guard = guard;

        let pipeline = ExecutionPipeline::new(runtime);

        let model = match registry_arc.get(&model_id) {
            Some(m) => m,
            None => {
                log::error!("Model {model_id} disappeared from registry during execution");
                return;
            },
        };

        log::info!(
            "Starting execution: run_id={} model={}",
            spec.run_id,
            model_id
        );

        match pipeline.execute(model, &spec).await {
            Ok(result) => {
                #[cfg(feature = "p2p")]
                if let Some(client) = p2p {
                    if let Some(output) = result.output.as_ref() {
                        let manifest = build_forecast_manifest(
                            &spec,
                            output,
                            &hash_input(spec.params.to_string().as_bytes()),
                        );
                        match bincode::serialize(&manifest) {
                            Ok(manifest_bytes) => {
                                if let Err(error) = client.command_tx.try_send(
                                    P2pCommand::BroadcastForecastTaskAssignment {
                                        task_id: result.run_id.clone(),
                                        node_id: forecast_node_id.clone(),
                                        manifest_bytes,
                                    },
                                ) {
                                    log::warn!(
                                        "Failed to enqueue forecast task assignment for run {}: {error}",
                                        result.run_id
                                    );
                                }
                            },
                            Err(error) => {
                                log::warn!(
                                    "Failed to serialize forecast manifest for run {}: {error}",
                                    result.run_id
                                );
                            },
                        }
                    }
                }
                log::info!(
                    "Execution complete: run_id={} success={} time={:.1}s",
                    result.run_id,
                    result.success,
                    result.execution_time_secs
                );
                let mut map = results.write().await;
                map.insert(spec.run_id, result);
                // Evict oldest entries to prevent unbounded memory growth.
                while map.len() > MAX_EXECUTION_RESULTS {
                    if let Some(key) = map.keys().next().cloned() {
                        map.remove(&key);
                    }
                }
            },
            Err(e) => {
                log::error!("Execution failed: run_id={} error={e}", spec.run_id);
                let error_result = wrfcoin_execution_engine::types::ModelRunResult {
                    run_id: spec.run_id.clone(),
                    success: false,
                    exit_code: -1,
                    execution_time_secs: 0.0,
                    output: None,
                    error: Some(e.to_string()),
                };
                let mut map = results.write().await;
                map.insert(spec.run_id, error_result);
            },
        }
    });

    HttpResponse::Accepted().json(RunModelResponse {
        run_id,
        model_id: request.model_id.clone(),
        status: "running".to_string(),
    })
}

/// GET /execution/models — list registered models.
pub async fn list_models(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let models = state.execution_registry.list_models();
    HttpResponse::Ok().json(ListModelsResponse { models })
}

/// GET /execution/result/{run_id} — get result for a run.
pub async fn get_result(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let run_id = path.into_inner();
    let results = state.execution_results.read().await;

    match results.get(&run_id) {
        Some(result) => HttpResponse::Ok().json(result),
        None => HttpResponse::Ok().json(serde_json::json!({
            "run_id": run_id,
            "status": "running",
        })),
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ExecutionQuotaConfig;

    /// Fresh per-token quota map used by the tests below.
    fn fresh_quota_map() -> Arc<RwLock<FxHashMap<String, ExecutionQuotaBucket>>> {
        Arc::new(RwLock::new(FxHashMap::default()))
    }

    #[test]
    fn in_flight_guard_debug_redacts_token() {
        let token = "synthetic-test-bearer-token-not-a-real-secret";
        let guard = InFlightGuard {
            quotas: fresh_quota_map(),
            token_key: token.to_string(),
        };
        let rendered = format!("{guard:?}");
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains(token)); // secret-leak-ok: synthetic value; assert! prints only the expression source
    }

    #[cfg(feature = "p2p")]
    mod anonymous_node_id {
        use super::super::anonymous_forecast_node_id;

        #[test]
        fn contains_no_token_material_and_is_prefixed() {
            let token = "synthetic-low-entropy-token";
            let id = anonymous_forecast_node_id(token);
            assert!(id.starts_with("anon:"));
            assert!(!id.contains(token)); // secret-leak-ok: synthetic value; assert! prints only the expression source
            // No fragment of the token may survive either — the id must be
            // hash output, not a transformation of the input.
            assert!(!id.contains("synthetic"));
            assert_eq!(id.len(), "anon:".len() + 16);
        }

        #[test]
        fn stable_per_token_within_process() {
            let a1 = anonymous_forecast_node_id("token-a");
            let a2 = anonymous_forecast_node_id("token-a");
            assert_eq!(a1, a2);
        }

        #[test]
        fn distinct_tokens_get_distinct_ids() {
            let a = anonymous_forecast_node_id("token-a");
            let b = anonymous_forecast_node_id("token-b");
            assert_ne!(a, b);
        }
    }

    fn tight_config() -> ExecutionQuotaConfig {
        ExecutionQuotaConfig {
            window_ms: 60_000,
            max_runs_per_window: 3,
            max_inflight: 2,
        }
    }

    /// Synchronously release one in-flight slot for a token in the test
    /// map.  Production releases go through [`InFlightGuard`]'s async
    /// drop path; these bucket-level tests take a direct route so the
    /// assertions are deterministic (no runtime-task scheduling races).
    async fn release(quotas: &Arc<RwLock<FxHashMap<String, ExecutionQuotaBucket>>>, token: &str) {
        let mut map = quotas.write().await;
        if let Some(b) = map.get_mut(token) {
            b.release();
        }
    }

    /// Admit without holding an `InFlightGuard` — used to isolate rate-cap
    /// behavior from in-flight counting by releasing immediately.
    async fn admit_then_release(
        quotas: &Arc<RwLock<FxHashMap<String, ExecutionQuotaBucket>>>,
        config: &ExecutionQuotaConfig,
        token: &str,
        now: i64,
    ) -> Result<(), ExecutionQuotaDenied> {
        let g = charge_execution_quota(quotas, config, token, now).await?;
        // Skip the async drop path — release synchronously so the next
        // `charge_execution_quota` call sees the updated counter.
        std::mem::forget(g);
        release(quotas, token).await;
        Ok(())
    }

    #[tokio::test]
    async fn quota_admits_up_to_rate_limit() {
        let quotas = fresh_quota_map();
        let config = tight_config();
        let now = 1_000_000i64;

        // Three submissions within the rate cap (3/window) should succeed.
        for _ in 0..3 {
            admit_then_release(&quotas, &config, "tokA", now)
                .await
                .expect("should admit within rate");
        }

        // Fourth submission in the same window must be denied for rate.
        let err = charge_execution_quota(&quotas, &config, "tokA", now)
            .await
            .expect_err("should exceed rate");
        assert_eq!(err, ExecutionQuotaDenied::RateExceeded);
    }

    #[tokio::test]
    async fn quota_admits_up_to_inflight_cap() {
        let quotas = fresh_quota_map();
        let config = tight_config();
        let now = 2_000_000i64;

        // Hold onto both guards so in-flight sits at the cap of 2. We
        // forget them so their async-drop task does not race us.
        let g1 = charge_execution_quota(&quotas, &config, "tokB", now)
            .await
            .expect("first concurrent run allowed");
        let g2 = charge_execution_quota(&quotas, &config, "tokB", now)
            .await
            .expect("second concurrent run allowed");
        std::mem::forget(g1);
        std::mem::forget(g2);

        // Third concurrent run is denied for concurrency — this is the
        // core #1244 protection: a single token cannot pin more than
        // `max_inflight` Docker jobs even when rate has room.
        let err = charge_execution_quota(&quotas, &config, "tokB", now)
            .await
            .expect_err("should exceed concurrency cap");
        assert_eq!(err, ExecutionQuotaDenied::ConcurrencyExceeded);

        // Release one slot and confirm a new submission is admitted.
        release(&quotas, "tokB").await;
        let g3 = charge_execution_quota(&quotas, &config, "tokB", now)
            .await
            .expect("slot should free up after release");
        std::mem::forget(g3);
    }

    #[tokio::test]
    async fn quota_is_per_token() {
        let quotas = fresh_quota_map();
        let config = tight_config();
        let now = 3_000_000i64;

        // Token A exhausts its full rate budget.
        for _ in 0..3 {
            admit_then_release(&quotas, &config, "tokA", now)
                .await
                .unwrap();
        }
        let err = charge_execution_quota(&quotas, &config, "tokA", now)
            .await
            .unwrap_err();
        assert_eq!(err, ExecutionQuotaDenied::RateExceeded);

        // Token B starts from a clean slate — another token burning
        // their budget must not affect this one.
        admit_then_release(&quotas, &config, "tokB", now)
            .await
            .expect("token B is unaffected by token A");
    }

    #[tokio::test]
    async fn quota_window_resets() {
        let quotas = fresh_quota_map();
        let config = tight_config();
        let now = 4_000_000i64;

        for _ in 0..3 {
            admit_then_release(&quotas, &config, "tokC", now)
                .await
                .unwrap();
        }
        assert!(
            charge_execution_quota(&quotas, &config, "tokC", now)
                .await
                .is_err()
        );

        // After the full window elapses, the rate slot is available
        // again even though the bucket still exists.
        let future = now + config.window_ms + 1;
        admit_then_release(&quotas, &config, "tokC", future)
            .await
            .expect("rate window should reset after elapse");
    }

    #[tokio::test]
    async fn bucket_idle_sweep_drops_expired_tokens() {
        // Directly exercise the capacity-sweep eligibility check used
        // by `charge_execution_quota` when the tracked-tokens map is
        // near the hard cap.
        let mut bucket = ExecutionQuotaBucket::new();
        let now = 10_000_000i64;
        let window = 60_000i64;

        // Load a submission + in-flight, then release the in-flight.
        bucket.try_admit(now, window, 5, 5).unwrap();
        bucket.release();
        assert!(
            !bucket.is_idle_after_prune(now, window),
            "timestamp is still fresh"
        );

        // After the window, the submission timestamp ages out and the
        // bucket reports idle — eligible for eviction from the map.
        let future = now + window + 1;
        assert!(bucket.is_idle_after_prune(future, window));
    }
}
