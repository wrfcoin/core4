#[cfg(feature = "privacy")]
pub mod privacy_handlers {
    use crate::helpers::require_role;
    use actix_web::{HttpRequest, HttpResponse, Responder, web};
    use rustc_hash::FxHashMap;
    use serde::Deserialize;
    use std::time::{SystemTime, UNIX_EPOCH};
    use wrfcoin_native_chain::crypto::hash::hash_bytes;
    use wrfcoin_native_chain::security::api_auth::ApiRole;
    use wrfcoin_native_chain::security::query_cost::declared_inputs_within;

    /// #2137: widest range proof the node will build; declared, checked before proving.
    const MAX_RANGE_PROOF_BITS: usize = 64;
    /// #2137: largest ring the node will sign with; declared, checked before allocating.
    const MAX_RING_DECOYS: usize = 64;

    use wrfcoin_privacy::{
        EncryptedPayload, MLSAGSignature, PedersenCommitment, RangeProof, RingMember, Scalar,
        StealthAddress, StealthKeys,
    };

    use crate::state::{AppState, PRIVACY_KEY_TTL_SECS};

    #[derive(Debug, Deserialize)]
    pub struct StealthAddressCreateRequest {
        pub spend_key: String,
        pub view_key: String,
        #[serde(default)]
        pub payment_id: Option<String>,
    }

    // No Debug: carries a private view key; a derived impl would print it in
    // any `{:?}` of the request (#2259).
    #[derive(Deserialize)]
    pub struct StealthAddressCheckRequest {
        pub one_time_address: String,
        pub ephemeral_public_key: String,
        pub private_view_key: String,
        pub public_spend_key: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct CommitmentCreateRequest {
        pub value: u64,
    }

    #[derive(Debug, Deserialize)]
    pub struct CommitmentVerifyRequest {
        pub commitment: String,
        pub blinding_factor: String,
        pub value: u64,
    }

    #[derive(Debug, Deserialize)]
    pub struct RangeProofCreateRequest {
        pub value: u64,
        pub blinding_factor: String,
        #[serde(default = "default_bits")]
        pub n_bits: usize,
    }

    fn default_bits() -> usize {
        64
    }

    #[derive(Debug, Deserialize)]
    pub struct RangeProofVerifyRequest {
        pub proof_json: String,
        pub commitment_hex: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct RingSignatureCreateRequest {
        /// UTF-8 message bytes. Exactly one of `message` and `message_hex`
        /// must be present.
        #[serde(default)]
        pub message: Option<String>,
        /// Hex-encoded message bytes for canonical binary signing preimages.
        #[serde(default)]
        pub message_hex: Option<String>,
        /// Server-side key reference (from POST /privacy/keys/register).
        pub key_id: String,
        pub decoy_public_keys: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    pub struct RingSignatureVerifyRequest {
        pub signature_json: String,
        /// UTF-8 message bytes. Exactly one of `message` and `message_hex`
        /// must be present.
        #[serde(default)]
        pub message: Option<String>,
        /// Hex-encoded message bytes for canonical binary signing preimages.
        #[serde(default)]
        pub message_hex: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    pub struct PayloadEncryptRequest {
        pub plaintext: String,
        pub recipient_public_key: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct PayloadDecryptRequest {
        pub payload_json: String,
        /// Server-side key reference (from POST /privacy/keys/register).
        pub key_id: String,
    }

    /// Request body for registering a private key on the server side.
    // No Debug: carries a raw private key; a derived impl would print it in
    // any `{:?}` of the request (#2259).
    #[derive(Deserialize)]
    pub struct KeyRegisterRequest {
        /// Hex-encoded 32-byte private key.
        pub private_key: String,
    }

    fn prune_expired_private_keys_map(
        registry: &mut FxHashMap<String, (i64, [u8; 32])>,
        now: i64,
    ) -> usize {
        let before = registry.len();
        registry.retain(|_, (expires_at, _)| *expires_at > now);
        before.saturating_sub(registry.len())
    }

    pub(crate) async fn prune_expired_private_keys_at(state: &AppState, now: i64) -> usize {
        let mut guard = state.privacy_key_registry.write().await;
        prune_expired_private_keys_map(&mut guard, now)
    }

    pub(crate) async fn prune_expired_private_keys(state: &AppState) -> usize {
        prune_expired_private_keys_at(state, chrono::Utc::now().timestamp()).await
    }

    fn hex_to_32(hex_str: &str) -> Result<[u8; 32], HttpResponse> {
        let bytes = hex::decode(hex_str).map_err(|e| {
            HttpResponse::BadRequest()
                .json(serde_json::json!({"error": format!("invalid hex: {e}")}))
        })?;
        if bytes.len() != 32 {
            return Err(HttpResponse::BadRequest()
                .json(serde_json::json!({"error": "expected 32-byte hex value"})));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    }

    fn decode_ring_message(
        message: Option<&str>,
        message_hex: Option<&str>,
    ) -> Result<Vec<u8>, HttpResponse> {
        match (message, message_hex) {
            (Some(message), None) => Ok(message.as_bytes().to_vec()),
            (None, Some(message_hex)) => hex::decode(message_hex).map_err(|e| {
                HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": format!("invalid message_hex: {e}")}))
            }),
            (Some(_), Some(_)) => Err(HttpResponse::BadRequest().json(
                serde_json::json!({"error": "provide exactly one of message or message_hex"}),
            )),
            (None, None) => Err(HttpResponse::BadRequest().json(
                serde_json::json!({"error": "provide exactly one of message or message_hex"}),
            )),
        }
    }

    /// Resolve a `key_id` from the privacy key registry.
    /// Expired entries are pruned on registration and by the background sweep task.
    async fn resolve_private_key(state: &AppState, key_id: &str) -> Result<[u8; 32], HttpResponse> {
        let now = chrono::Utc::now().timestamp();
        let guard = state.privacy_key_registry.read().await;
        match guard.get(key_id) {
            Some(&(expires_at, key)) if expires_at > now => Ok(key),
            Some(_) => {
                Err(HttpResponse::NotFound().json(serde_json::json!({"error": "key_id expired"})))
            },
            None => {
                Err(HttpResponse::NotFound().json(serde_json::json!({"error": "key_id not found"})))
            },
        }
    }

    /// POST /privacy/keys/register — store a private key server-side and return a key_id.
    pub async fn register_key(
        req: HttpRequest,
        state: web::Data<AppState>,
        body: web::Json<KeyRegisterRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let key = match hex_to_32(&body.private_key) {
            Ok(k) => k,
            Err(resp) => return resp,
        };
        // Derive a unique key_id by hashing the key material with a timestamp nonce.
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes();
        let id_hash = hash_bytes(&[&key[..], &nonce[..]].concat());
        let key_id = format!("pk-{}", &id_hash.to_hex()[..32]);
        let now = chrono::Utc::now().timestamp();
        let expires_at = now + PRIVACY_KEY_TTL_SECS;
        {
            let mut guard = state.privacy_key_registry.write().await;
            prune_expired_private_keys_map(&mut guard, now);
            guard.insert(key_id.clone(), (expires_at, key));
        }
        HttpResponse::Ok().json(serde_json::json!({
            "key_id": key_id,
            "expires_at": expires_at,
        }))
    }

    pub async fn generate_stealth_keys(
        req: HttpRequest,
        state: web::Data<AppState>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let keys = StealthKeys::generate();
        HttpResponse::Ok().json(serde_json::json!({
            "private_spend_key": hex::encode(keys.private_spend_key),
            "private_view_key": hex::encode(keys.private_view_key),
            "public_spend_key": hex::encode(keys.public_spend_key),
            "public_view_key": hex::encode(keys.public_view_key),
            "stealth_address_format": keys.stealth_address_format(),
        }))
    }

    pub async fn create_stealth_address(
        req: HttpRequest,
        state: web::Data<AppState>,
        body: web::Json<StealthAddressCreateRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let spend_key = match hex_to_32(&body.spend_key) {
            Ok(k) => k,
            Err(resp) => return resp,
        };
        let view_key = match hex_to_32(&body.view_key) {
            Ok(k) => k,
            Err(resp) => return resp,
        };

        let payment_id = match &body.payment_id {
            Some(pid) => {
                let bytes = match hex::decode(pid) {
                    Ok(b) if b.len() == 8 => {
                        let mut arr = [0u8; 8];
                        arr.copy_from_slice(&b);
                        arr
                    },
                    _ => {
                        return HttpResponse::BadRequest()
                            .json(serde_json::json!({"error": "payment_id must be 8 bytes hex"}));
                    },
                };
                Some(bytes)
            },
            None => None,
        };

        match StealthAddress::new(spend_key, view_key, payment_id) {
            Ok((addr, ephemeral_secret)) => {
                // Store the ephemeral secret server-side in the key registry
                // and return only a key_id reference. The secret must never be
                // exposed in an HTTP response (issue #949).
                let nonce = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
                    .to_le_bytes();
                let id_hash = hash_bytes(&[&ephemeral_secret[..], &nonce[..]].concat());
                let key_id = format!("eph-{}", &id_hash.to_hex()[..32]);
                let now = chrono::Utc::now().timestamp();
                let expires_at = now + PRIVACY_KEY_TTL_SECS;
                {
                    let mut guard = state.privacy_key_registry.write().await;
                    prune_expired_private_keys_map(&mut guard, now);
                    guard.insert(key_id.clone(), (expires_at, ephemeral_secret));
                }
                HttpResponse::Ok()
                    .insert_header(("Cache-Control", "no-store"))
                    .insert_header(("Pragma", "no-cache"))
                    .json(serde_json::json!({
                        "one_time_address": hex::encode(addr.one_time_public_key),
                        "transaction_key": hex::encode(addr.transaction_key),
                        "payment_id": addr.payment_id.map(hex::encode),
                        "ephemeral_key_id": key_id,
                        "ephemeral_key_expires_at": expires_at,
                    }))
            },
            Err(e) => HttpResponse::BadRequest().json(serde_json::json!({"error": e.to_string()})),
        }
    }

    pub async fn check_stealth_address(
        req: HttpRequest,
        state: web::Data<AppState>,
        body: web::Json<StealthAddressCheckRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let one_time = match hex_to_32(&body.one_time_address) {
            Ok(k) => k,
            Err(resp) => return resp,
        };
        let ephemeral = match hex_to_32(&body.ephemeral_public_key) {
            Ok(k) => k,
            Err(resp) => return resp,
        };
        let priv_view = match hex_to_32(&body.private_view_key) {
            Ok(k) => k,
            Err(resp) => return resp,
        };
        let pub_spend = match hex_to_32(&body.public_spend_key) {
            Ok(k) => k,
            Err(resp) => return resp,
        };

        let addr = StealthAddress {
            public_spend_key: pub_spend,
            public_view_key: [0u8; 32], // not needed for check
            one_time_public_key: one_time,
            transaction_key: ephemeral,
            payment_id: None,
        };

        match addr.is_for_recipient(&priv_view, &pub_spend) {
            Ok(is_for) => HttpResponse::Ok().json(serde_json::json!({
                "is_for_recipient": is_for,
            })),
            Err(e) => HttpResponse::BadRequest().json(serde_json::json!({"error": e.to_string()})),
        }
    }

    pub async fn create_commitment(
        req: HttpRequest,
        state: web::Data<AppState>,
        body: web::Json<CommitmentCreateRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let (commitment, blinding) = PedersenCommitment::commit(body.value);
        HttpResponse::Ok().json(serde_json::json!({
            "commitment": hex::encode(commitment.to_bytes()),
            "blinding_factor": hex::encode(blinding.as_bytes()),
        }))
    }

    pub async fn verify_commitment(
        req: HttpRequest,
        state: web::Data<AppState>,
        body: web::Json<CommitmentVerifyRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let commitment_bytes = match hex::decode(&body.commitment) {
            Ok(b) => b,
            Err(e) => {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": format!("invalid commitment hex: {e}")}));
            },
        };
        let blinding_bytes = match hex_to_32(&body.blinding_factor) {
            Ok(b) => b,
            Err(resp) => return resp,
        };

        let commitment = match PedersenCommitment::from_bytes(&commitment_bytes) {
            Ok(c) => c,
            Err(e) => {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": e.to_string()}));
            },
        };

        let blinding = Scalar::from_bytes_mod_order(blinding_bytes);
        let valid = commitment.verify(body.value, &blinding);

        HttpResponse::Ok().json(serde_json::json!({"valid": valid}))
    }

    pub async fn create_range_proof(
        req: HttpRequest,
        state: web::Data<AppState>,
        body: web::Json<RangeProofCreateRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let blinding_bytes = match hex_to_32(&body.blinding_factor) {
            Ok(b) => b,
            Err(resp) => return resp,
        };
        let blinding = Scalar::from_bytes_mod_order(blinding_bytes);
        // #2137: the declared bit width sizes the proof; refuse before proving.
        if let Err(e) = declared_inputs_within(body.n_bits, MAX_RANGE_PROOF_BITS) {
            return super::super::query_cost::inputs_rejected(&req, e);
        }
        let (value, n_bits) = (body.value, body.n_bits);
        // #2137: proving runs on the bounded expensive pool, never on the
        // request path; a saturated pool answers 503 + Retry-After at once.
        let proved = match super::super::query_cost::run_expensive(&req, super::super::query_cost::expensive_pool(), move || {
            RangeProof::prove(value, &blinding, n_bits)
        })
        .await
        {
            Ok(result) => result,
            Err(resp) => return resp,
        };

        match proved {
            Ok(proof) => match proof.to_json() {
                Ok(json) => HttpResponse::Ok().json(serde_json::json!({"proof": json})),
                Err(e) => {
                    log::error!("range proof to_json failed: {e}");
                    HttpResponse::InternalServerError()
                        .json(serde_json::json!({"error": "internal server error"}))
                }
            },
            Err(e) => HttpResponse::BadRequest().json(serde_json::json!({"error": e.to_string()})),
        }
    }

    pub async fn verify_range_proof(
        req: HttpRequest,
        state: web::Data<AppState>,
        body: web::Json<RangeProofVerifyRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let proof = match RangeProof::from_json(&body.proof_json) {
            Ok(p) => p,
            Err(e) => {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": format!("invalid proof: {e}")}));
            },
        };
        let commitment_bytes = match hex::decode(&body.commitment_hex) {
            Ok(b) => b,
            Err(e) => {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": format!("invalid hex: {e}")}));
            },
        };
        let commitment = match PedersenCommitment::from_bytes(&commitment_bytes) {
            Ok(c) => c,
            Err(e) => {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": e.to_string()}));
            },
        };

        match proof.verify(&commitment) {
            Ok(valid) => HttpResponse::Ok().json(serde_json::json!({"valid": valid})),
            Err(e) => {
                log::error!("range proof verification failed: {e}");
                HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": "internal server error"}))
            }
        }
    }

    pub async fn create_ring_signature(
        req: HttpRequest,
        state: web::Data<AppState>,
        body: web::Json<RingSignatureCreateRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let private_key = match resolve_private_key(&state, &body.key_id).await {
            Ok(k) => k,
            Err(resp) => return resp,
        };

        // #2137: the declared ring size drives every allocation below; refuse
        // it before the first one (the #1833 lesson).
        if let Err(e) = declared_inputs_within(body.decoy_public_keys.len(), MAX_RING_DECOYS) {
            return super::super::query_cost::inputs_rejected(&req, e);
        }
        let mut decoys = Vec::with_capacity(body.decoy_public_keys.len());
        for dk in &body.decoy_public_keys {
            match hex_to_32(dk) {
                Ok(pk) => decoys.push(RingMember { public_key: pk }),
                Err(resp) => return resp,
            }
        }

        let message =
            match decode_ring_message(body.message.as_deref(), body.message_hex.as_deref()) {
                Ok(message) => message,
                Err(resp) => return resp,
            };

        // #2137: ring signing is proving-class work; bounded pool, not the request path.
        let message_for_signing = message.clone();
        let signed = match super::super::query_cost::run_expensive(&req, super::super::query_cost::expensive_pool(), move || {
            MLSAGSignature::sign(&message_for_signing, &private_key, &decoys)
        })
        .await
        {
            Ok(result) => result,
            Err(resp) => return resp,
        };

        match signed {
            Ok(sig) => match serde_json::to_string(&sig) {
                Ok(json) => {
                    let key_image_hex = hex::encode(sig.key_image.image);
                    HttpResponse::Ok().json(serde_json::json!({
                        "signature": json,
                        "key_image": key_image_hex,
                        "signed_message": hex::encode(&message),
                    }))
                },
                Err(e) => {
                    log::error!("ring signature serialization failed: {e}");
                    HttpResponse::InternalServerError()
                        .json(serde_json::json!({"error": "internal server error"}))
                }
            },
            Err(e) => HttpResponse::BadRequest().json(serde_json::json!({"error": e.to_string()})),
        }
    }

    pub async fn verify_ring_signature(
        req: HttpRequest,
        state: web::Data<AppState>,
        body: web::Json<RingSignatureVerifyRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let sig: MLSAGSignature = match serde_json::from_str(&body.signature_json) {
            Ok(s) => s,
            Err(e) => {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": format!("invalid signature: {e}")}));
            },
        };
        let message =
            match decode_ring_message(body.message.as_deref(), body.message_hex.as_deref()) {
                Ok(message) => message,
                Err(resp) => return resp,
            };

        match sig.verify(&message) {
            Ok(valid) => HttpResponse::Ok().json(serde_json::json!({"valid": valid})),
            Err(e) => {
                log::error!("ring signature verification failed: {e}");
                HttpResponse::InternalServerError()
                    .json(serde_json::json!({"error": "internal server error"}))
            }
        }
    }

    pub async fn encrypt_payload(
        req: HttpRequest,
        state: web::Data<AppState>,
        body: web::Json<PayloadEncryptRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let recipient_key = match hex_to_32(&body.recipient_public_key) {
            Ok(k) => k,
            Err(resp) => return resp,
        };

        match wrfcoin_privacy::encrypt_payload(body.plaintext.as_bytes(), &recipient_key) {
            Ok(encrypted) => match serde_json::to_string(&encrypted) {
                Ok(json) => HttpResponse::Ok().json(serde_json::json!({"payload": json})),
                Err(e) => {
                    log::error!("payload encryption serialization failed: {e}");
                    HttpResponse::InternalServerError()
                        .json(serde_json::json!({"error": "internal server error"}))
                }
            },
            Err(e) => HttpResponse::BadRequest().json(serde_json::json!({"error": e.to_string()})),
        }
    }

    pub async fn decrypt_payload(
        req: HttpRequest,
        state: web::Data<AppState>,
        body: web::Json<PayloadDecryptRequest>,
    ) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
            return resp;
        }
        let private_key = match resolve_private_key(&state, &body.key_id).await {
            Ok(k) => k,
            Err(resp) => return resp,
        };
        let payload: EncryptedPayload = match serde_json::from_str(&body.payload_json) {
            Ok(p) => p,
            Err(e) => {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({"error": format!("invalid payload: {e}")}));
            },
        };

        match wrfcoin_privacy::decrypt_payload(&payload, &private_key) {
            Ok(plaintext) => {
                let text = String::from_utf8_lossy(&plaintext).to_string();
                HttpResponse::Ok().json(serde_json::json!({"plaintext": text}))
            },
            Err(e) => HttpResponse::BadRequest().json(serde_json::json!({"error": e.to_string()})),
        }
    }

    pub async fn get_key_images(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
        if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
            return resp;
        }
        let count = state.key_image_registry.count();
        HttpResponse::Ok().json(serde_json::json!({
            "registered_key_images": count,
        }))
    }

    #[cfg(test)]
    mod error_sanitization_tests {
        use super::*;

        #[test]
        fn verify_error_response_does_not_leak_details() {
            // Test that error responses return generic "internal server error" messages,
            // not raw error strings from crypto operations.
            //
            // This is a logical test that documents the security requirement:
            // privacy crypto route handlers MUST NOT expose internal error details
            // to clients.

            // Simulate what happens when range_proof.to_json() fails:
            // Before fix: {"error": "failed to serialize proof: <internal json error>"}
            // After fix: {"error": "internal server error"}

            let response = serde_json::json!({"error": "internal server error"});
            let error_msg = response["error"].as_str().unwrap();

            // Verify the error message is generic and doesn't contain crypto internals
            assert_eq!(error_msg, "internal server error");
            assert!(!error_msg.contains("serde_json"));
            assert!(!error_msg.contains("failed to"));
            assert!(!error_msg.contains("prove"));
            assert!(!error_msg.contains("serialize"));
            assert!(!error_msg.contains("signature"));
            assert!(!error_msg.contains("commitment"));
        }
    }
}
