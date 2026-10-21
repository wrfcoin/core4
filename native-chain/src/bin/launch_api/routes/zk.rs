pub mod zk_handlers {
    use actix_web::{
        HttpRequest, HttpResponse, Responder,
        http::{
            StatusCode,
            header::{HeaderValue, RETRY_AFTER},
        },
        web,
    };
    use serde::{Deserialize, Serialize};
    use tokio::sync::{OwnedSemaphorePermit, TryAcquireError};
    use wrfcoin_native_chain::zk_service;

    use crate::helpers::require_role;
    use crate::state::AppState;
    use wrfcoin_native_chain::security::api_auth::ApiRole;

    const ZK_PROVER_RETRY_AFTER_SECS: u64 = 1;

    // -----------------------------------------------------------------------
    // Request / response types
    // -----------------------------------------------------------------------

    #[derive(Debug, Deserialize)]
    pub struct RangeProofGenerateRequest {
        pub value: u64,
        /// Hex-encoded BN254 scalar field element (32 bytes). Use a CSPRNG to
        /// generate — 64-bit values are insecure for production.
        pub blinding_factor: String,
    }

    #[derive(Debug, Serialize)]
    pub struct RangeProofGenerateResponse {
        pub proof: String,
        pub commitment: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct RangeProofVerifyRequest {
        pub proof: String,
        pub commitment: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct ConfidentialTransferGenerateRequest {
        pub input_values: Vec<u64>,
        pub input_blindings: Vec<String>,
        pub output_values: Vec<u64>,
        pub output_blindings: Vec<String>,
        pub fee: u64,
    }

    #[derive(Debug, Serialize)]
    pub struct ConfidentialTransferGenerateResponse {
        pub proof: String,
        pub input_commitment: String,
        pub output_commitment: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct ConfidentialTransferVerifyRequest {
        pub proof: String,
        pub input_commitment: String,
        pub output_commitment: String,
        pub fee: u64,
        /// Which key to use for verification. Defaults to 1 (backward-compatible).
        #[serde(default = "default_one")]
        pub num_inputs: u32,
        #[serde(default = "default_one")]
        pub num_outputs: u32,
    }

    fn default_one() -> u32 { 1 }

    #[derive(Debug, Deserialize)]
    pub struct SelectiveDisclosureGenerateRequest {
        pub field_values: Vec<u64>,
    }

    #[derive(Debug, Serialize)]
    pub struct SelectiveDisclosureGenerateResponse {
        pub proof: String,
        pub commitment: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct SelectiveDisclosureVerifyRequest {
        pub proof: String,
        pub commitment: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct WeatherModelGenerateRequest {
        pub input_hash: String,
        pub output_hash: String,
        pub model_id: u64,
        pub time_secs: u64,
        pub samples: Vec<u64>,
    }

    #[derive(Debug, Serialize)]
    pub struct WeatherModelGenerateResponse {
        pub proof: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct WeatherModelVerifyRequest {
        pub proof: String,
        pub input_hash: String,
        pub output_hash: String,
        pub model_id: u64,
        pub time_secs: u64,
    }

    #[derive(Debug, Serialize)]
    pub struct VerifyResponse {
        pub valid: bool,
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn zk_error(status: StatusCode, e: impl std::fmt::Display) -> HttpResponse {
        let error_msg = e.to_string();
        if status.is_server_error() {
            log::error!("zk route error: {}", error_msg);
            HttpResponse::build(status).json(serde_json::json!({"error": "internal server error"}))
        } else {
            HttpResponse::build(status).json(serde_json::json!({"error": error_msg}))
        }
    }

    fn zk_error_with_retry_after(
        status: StatusCode,
        message: impl Into<String>,
        retry_after_secs: u64,
    ) -> HttpResponse {
        let mut response = HttpResponse::build(status).json(serde_json::json!({
            "error": message.into()
        }));
        response.headers_mut().insert(
            RETRY_AFTER,
            retry_after_header_value(&retry_after_secs.to_string()),
        );
        response
    }

    fn retry_after_header_value(retry_after: &str) -> HeaderValue {
        HeaderValue::from_str(retry_after).unwrap_or_else(|_| HeaderValue::from_static("60"))
    }

    fn zk_result<T: Serialize>(
        result: Result<Result<T, zk_service::ZkServiceError>, actix_web::error::BlockingError>,
    ) -> HttpResponse {
        match result {
            Ok(Ok(data)) => HttpResponse::Ok().json(data),
            // #2038: an unsupported circuit config is a client error (bad
            // dimensions), not a generic bad request — surface it as 422 so a
            // caller can distinguish "this circuit isn't offered" from malformed
            // proof/commitment bytes.
            Ok(Err(e @ zk_service::ZkServiceError::UnsupportedConfig(_))) => {
                zk_error(StatusCode::UNPROCESSABLE_ENTITY, e)
            },
            Ok(Err(e)) => zk_error(StatusCode::BAD_REQUEST, e),
            Err(e) => zk_error(StatusCode::INTERNAL_SERVER_ERROR, e),
        }
    }

    fn try_acquire_zk_prover_permit(
        state: &web::Data<AppState>,
    ) -> Result<OwnedSemaphorePermit, HttpResponse> {
        state.zk_prover_gate.clone().try_acquire_owned().map_err(|err| match err {
            TryAcquireError::NoPermits => zk_error_with_retry_after(
                StatusCode::TOO_MANY_REQUESTS,
                "ZK proof generation capacity exhausted; retry later.",
                ZK_PROVER_RETRY_AFTER_SECS,
            ),
            TryAcquireError::Closed => zk_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "ZK proof generation is unavailable on this node.",
            ),
        })
    }

    async fn run_gated_generation<T, F>(
        state: &web::Data<AppState>,
        work: F,
    ) -> HttpResponse
    where
        T: Serialize + Send + 'static,
        F: FnOnce() -> Result<T, zk_service::ZkServiceError> + Send + 'static,
    {
        let permit = match try_acquire_zk_prover_permit(state) {
            Ok(permit) => permit,
            Err(response) => return response,
        };
        let result = web::block(move || {
            let _permit = permit;
            work()
        })
        .await;
        zk_result(result)
    }

    // -----------------------------------------------------------------------
    // Handlers — generation endpoints require Contributor role
    // -----------------------------------------------------------------------

    pub async fn generate_range_proof(
        req: HttpRequest,
        state: web::Data<AppState>,
        request: web::Json<RangeProofGenerateRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let r = request.into_inner();
        run_gated_generation(&state, move || {
            zk_service::generate_range_proof(r.value, &r.blinding_factor)
                .map(|(proof, commitment)| RangeProofGenerateResponse { proof, commitment })
        })
        .await
    }

    pub async fn verify_range_proof(
        req: HttpRequest,
        state: web::Data<AppState>,
        request: web::Json<RangeProofVerifyRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
            return resp;
        }
        // Verification is cheap (a few pairings) and is deliberately NOT gated by
        // the expensive-*generation* prover budget: a saturated generation gate
        // must not block callers from verifying existing proofs (that would be a
        // DoS-amplification vector). This bypass is asserted by the previously
        // dark test `zk_verify_endpoint_bypasses_generation_gate` (#2385).
        let r = request.into_inner();
        let result =
            web::block(move || {
                zk_service::verify_range_proof(&r.proof, &r.commitment)
                    .map(|valid| VerifyResponse { valid })
            })
            .await;
        zk_result(result)
    }

    pub async fn generate_confidential_transfer(
        req: HttpRequest,
        state: web::Data<AppState>,
        request: web::Json<ConfidentialTransferGenerateRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let r = request.into_inner();
        run_gated_generation(&state, move || {
            let inputs = zk_service::ConfidentialTransferInputs {
                input_values: r.input_values,
                input_blindings: r.input_blindings,
                output_values: r.output_values,
                output_blindings: r.output_blindings,
                fee: r.fee,
            };
            zk_service::generate_confidential_transfer_proof(&inputs).map(
                |(proof, input_commitment, output_commitment)| {
                    ConfidentialTransferGenerateResponse {
                        proof,
                        input_commitment,
                        output_commitment,
                    }
                },
            )
        })
        .await
    }

    // Hard cap prevents on-demand key generation for arbitrarily large circuits (#1833).
    const MAX_TRANSFER_FANOUT: u32 = 16;

    pub async fn verify_confidential_transfer(
        req: HttpRequest,
        state: web::Data<AppState>,
        request: web::Json<ConfidentialTransferVerifyRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
            return resp;
        }
        // Verification is cheap (a few pairings) and is deliberately NOT gated by
        // the expensive-*generation* prover budget: a saturated generation gate
        // must not block callers from verifying existing proofs (that would be a
        // DoS-amplification vector). This bypass is asserted by the previously
        // dark test `zk_verify_endpoint_bypasses_generation_gate` (#2385).
        let r = request.into_inner();
        if r.num_inputs > MAX_TRANSFER_FANOUT || r.num_outputs > MAX_TRANSFER_FANOUT {
            return zk_error(StatusCode::UNPROCESSABLE_ENTITY, "num_inputs and num_outputs must be ≤ 16");
        }
        let result = web::block(move || {
            zk_service::verify_confidential_transfer_proof(
                &r.proof,
                &r.input_commitment,
                &r.output_commitment,
                r.fee,
                r.num_inputs as usize,
                r.num_outputs as usize,
            )
            .map(|valid| VerifyResponse { valid })
        })
        .await;
        zk_result(result)
    }

    pub async fn generate_selective_disclosure(
        req: HttpRequest,
        state: web::Data<AppState>,
        request: web::Json<SelectiveDisclosureGenerateRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let r = request.into_inner();
        run_gated_generation(&state, move || {
            zk_service::generate_selective_disclosure_proof(&r.field_values)
                .map(|(proof, commitment)| SelectiveDisclosureGenerateResponse { proof, commitment })
        })
        .await
    }

    pub async fn verify_selective_disclosure(
        req: HttpRequest,
        state: web::Data<AppState>,
        request: web::Json<SelectiveDisclosureVerifyRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
            return resp;
        }
        // Verification is cheap (a few pairings) and is deliberately NOT gated by
        // the expensive-*generation* prover budget: a saturated generation gate
        // must not block callers from verifying existing proofs (that would be a
        // DoS-amplification vector). This bypass is asserted by the previously
        // dark test `zk_verify_endpoint_bypasses_generation_gate` (#2385).
        let r = request.into_inner();
        let result = web::block(move || {
            zk_service::verify_selective_disclosure_proof(&r.proof, &r.commitment)
                .map(|valid| VerifyResponse { valid })
        })
        .await;
        zk_result(result)
    }

    pub async fn generate_weather_model_proof(
        req: HttpRequest,
        state: web::Data<AppState>,
        request: web::Json<WeatherModelGenerateRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let r = request.into_inner();
        run_gated_generation(&state, move || {
            zk_service::generate_weather_model_proof(
                &r.input_hash,
                &r.output_hash,
                r.model_id,
                r.time_secs,
                &r.samples,
            )
            .map(|proof| WeatherModelGenerateResponse { proof })
        })
        .await
    }

    pub async fn verify_weather_model_proof(
        req: HttpRequest,
        state: web::Data<AppState>,
        request: web::Json<WeatherModelVerifyRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
            return resp;
        }
        // Verification is cheap (a few pairings) and is deliberately NOT gated by
        // the expensive-*generation* prover budget: a saturated generation gate
        // must not block callers from verifying existing proofs (that would be a
        // DoS-amplification vector). This bypass is asserted by the previously
        // dark test `zk_verify_endpoint_bypasses_generation_gate` (#2385).
        let r = request.into_inner();
        let result = web::block(move || {
            zk_service::verify_weather_model_proof(
                &r.proof,
                &r.input_hash,
                &r.output_hash,
                r.model_id,
                r.time_secs,
            )
            .map(|valid| VerifyResponse { valid })
        })
        .await;
        zk_result(result)
    }

    pub async fn get_zk_stats(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
            return resp;
        }
        let blockchain = state.blockchain.read().await;
        let zk_verified_proofs = &blockchain.state.zk_verified_proofs;

        HttpResponse::Ok().json(serde_json::json!({
            "verified_proofs": zk_verified_proofs,
            "total_verified": zk_verified_proofs.values().sum::<u64>(),
        }))
    }

    pub async fn list_circuits(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
            return resp;
        }
        HttpResponse::Ok().json(serde_json::json!({
            "circuits": zk_service::available_circuits(),
        }))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn test_zk_error_sanitization_on_server_error() {
            let response = zk_error(StatusCode::INTERNAL_SERVER_ERROR, "confidential circuit failed: invalid constraint");
            let status = response.status();
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

            // Read the real response body — the previous version inspected an
            // empty `BytesMut` (and referenced an unlinked `bytes` crate), so
            // the assertion was vacuous. Extract the actual bytes instead.
            let body = actix_web::body::to_bytes(response.into_body())
                .await
                .expect("zk error body should be readable");
            let body = std::str::from_utf8(&body).unwrap_or("");
            // A 5xx must not leak internal circuit/constraint details...
            assert!(
                !body.contains("confidential circuit") && !body.contains("constraint"),
                "server error body leaked internal details: {body}"
            );
            // ...and should carry the generic sanitized message instead.
            assert!(
                body.contains("internal server error"),
                "expected sanitized message, got: {body}"
            );
        }

        #[test]
        fn test_zk_error_preserves_client_error() {
            let response = zk_error(StatusCode::BAD_REQUEST, "invalid blinding factor format");
            let status = response.status();
            assert_eq!(status, StatusCode::BAD_REQUEST);
            // Client errors (4xx) should preserve the message for validation feedback
        }

        #[test]
        fn test_zk_error_with_retry_after() {
            let response = zk_error_with_retry_after(
                StatusCode::TOO_MANY_REQUESTS,
                "ZK proof generation capacity exhausted",
                5,
            );
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
            // Verify retry-after header is present
            assert!(response.headers().contains_key("Retry-After"));
        }

        #[test]
        fn test_retry_after_header_value_falls_back_when_invalid() {
            let header = retry_after_header_value("bad\r\nheader");
            assert_eq!(header.to_str().ok(), Some("60"));
        }
    }
}
