use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::{Deserialize, Serialize};

use crate::helpers::{
    build_transparent_opt_out_payload, enforce_contribution_commit_rate_limit,
    enforce_legacy_authenticated_submit_rate_limit, enforce_legacy_submit_rate_limit,
    enforce_permissionless_replay_protection, enforce_permissionless_submit_rate_limit,
    enqueue_protocol_transaction, enqueue_protocol_transaction_with_nonce, envelope_error,
    envelope_success, feature_disabled_envelope_with_hint, refuse_seen_permissionless_replay_key,
    release_permissionless_replay_key, require_forecaster_bound_to_bearer, require_role,
    submit_protocol_transaction, validate_commit_hash, validate_nonce, validate_participant_id,
    validate_station_id, validate_timestamp,
};
use crate::state::{
    AppState, DEFAULT_CONTRIBUTION_STATUS_PAGE_SIZE, MAX_CONTRIBUTION_STATUS_PAGE_SIZE,
};
use wrfcoin_native_chain::blockchain::transaction::{
    CommitPayload, ComputeReceipt, ForecastArtifactManifest, ForecastVerificationReceipt,
    RevealPayload, SensorProvenance, Transaction, TransactionType,
};
use wrfcoin_native_chain::common::types::Address;
use wrfcoin_native_chain::crypto::hash::hash_bytes;
#[cfg(feature = "p2p")]
use wrfcoin_p2p::P2pCommand;
use wrfcoin_native_chain::crypto::signature::{PublicKey, Signature};
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct LegacyTransactionSubmitRequest {
    pub from: Option<String>,
    pub to: Option<String>,
    pub amount: Option<u64>,
    #[serde(default)]
    pub transparent_opt_out: bool,
    #[serde(default)]
    pub transparent_opt_out_reason: Option<String>,
    /// Hex ed25519 public key of the `from` account, required for transparent
    /// transfers so the server never mints an unauthorized debit (#1727).
    #[serde(default)]
    pub auth_pubkey: Option<String>,
    /// Hex ed25519 signature over the canonical transparent-transfer
    /// authorization message, produced by the `from` account key (#1727).
    /// The signature must cover the required request nonce.
    #[serde(default)]
    pub auth_signature: Option<String>,
    /// Required per-request nonce for replay protection (#1846/#1882).
    ///
    /// The server records `from:nonce` in the replay-seen set
    /// and rejects any subsequent request with the same pair, preventing
    /// replay of a stolen bearer token. Public launch ingress rejects requests
    /// without a nonce so the legacy compatibility endpoint cannot bypass the
    /// shared replay guard.
    ///
    /// For transparent transfers (`transparent_opt_out=true`) the nonce is
    /// also incorporated into the `auth_signature` message (as a u64
    /// parsed from this string); clients must sign with the same nonce.
    #[serde(default)]
    pub nonce: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PermissionlessSubmitRequest {
    pub signer_public_key: String,
    pub signature: String,
    pub nonce: String,
    pub timestamp: i64,
    pub tx_type: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContributionCommitRequest {
    pub contributor: String,
    pub station_id: String,
    pub commit_hash: String,
    /// Contributor's hex-encoded ed25519 public key (#1848).
    #[serde(default)]
    pub auth_pubkey: Option<String>,
    /// Hex-encoded ed25519 signature over the canonical commit-authorization
    /// message, produced by the holder of `auth_pubkey` (#1848).
    #[serde(default)]
    pub auth_signature: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContributionRevealRequest {
    pub contributor: String,
    pub station_id: String,
    pub payload: serde_json::Value,
    pub nonce: String,
    pub commit_hash: String,
    pub timestamp: Option<i64>,
    pub data_cid: Option<String>,
    pub data_hash: Option<String>,
    pub storage_provider: Option<String>,
    pub compute_receipt: Option<ComputeReceipt>,
    pub provenance: Option<SensorProvenance>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContributionStatusResponse {
    pub commit_id: String,
    pub station_id: Option<String>,
    pub contribution_type: Option<String>,
    pub committed_at: Option<u64>,
    pub commit_expires_at: Option<u64>,
    pub reveal_expires_at: Option<u64>,
    pub revealed_at: Option<u64>,
    pub provenance_class: Option<ContributionProvenanceClass>,
    #[serde(serialize_with = "crate::routes::decimal::option_u128")]
    pub reward_amount: Option<u128>,
    pub reward_finalized: bool,
    pub reward_status: String,
    pub approvals: usize,
    pub rejections: usize,
    pub has_quorum: bool,
    pub oracle_quorum: usize,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContributionStatusListResponse {
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub items: Vec<ContributionStatusResponse>,
}

#[derive(Debug, Deserialize)]
pub struct ContributionStatusQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub status: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn submit_transaction_compat(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<LegacyTransactionSubmitRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }

    let from = body.from.as_deref().unwrap_or("").trim();
    let to = body.to.as_deref().unwrap_or("").trim();
    let amount = body.amount.unwrap_or_default();
    let transparent_opt_out = body.transparent_opt_out;
    if from.is_empty() || to.is_empty() || amount == 0 {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "from, to, and positive amount are required",
        );
    }
    if from.parse::<Address>().is_err() || to.parse::<Address>().is_err() {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "from and to must be valid hex addresses",
        );
    }

    // #1846/#1882: require a per-request nonce for replay protection.
    // Replay check runs BEFORE rate limiting so replayed requests do not
    // consume rate-limit budget (a replayed nonce is not a new request).
    let nonce_u64: u64 = match body.nonce.as_deref() {
        None => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "nonce is required",
            );
        },
        Some(raw) => {
            let trimmed = raw.trim();
            if let Err(resp) = validate_nonce(&req, trimmed) {
                return resp;
            }
            match trimmed.parse::<u64>() {
                Ok(n) => n,
                Err(_) => {
                    return envelope_error(
                        &req,
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "VALIDATION_ERROR",
                        "nonce must be a non-negative integer when provided",
                    );
                },
            }
        },
    };

    // The former default manufactured hash-shaped privacy fields and returned
    // 202 even though consensus rejects that payload as an invalid MLSAG. Refuse
    // before replay/rate-limit state changes so an unavailable feature burns no
    // caller nonce or request budget (#3235).
    if !transparent_opt_out {
        return feature_disabled_envelope_with_hint(
            &req,
            "confidential_transfer",
            "set transparent_opt_out=true with auth_pubkey/auth_signature (#1727); \
             confidential transfers are tracked in #2080; backend wallet replacement is #1165",
        );
    }

    let (tx_type, payload_value, privacy_mode) = {
        // #1727: a transparent transfer debits `from` with no per-input
        // signature, so it must carry a verifiable `from`-bound authorization.
        // The server cannot sign for the user, so the client must supply it.
        let (auth_pubkey, auth_signature) =
            match (body.auth_pubkey.as_deref(), body.auth_signature.as_deref()) {
                (Some(pk), Some(sig)) if !pk.trim().is_empty() && !sig.trim().is_empty() => {
                    (pk.trim().to_string(), sig.trim().to_string())
                },
                _ => {
                    // #3258 review R1: a failed authorization is charged to
                    // the caller's budget (see the AUTHORIZATION_INVALID arm).
                    if let Err(resp) = enforce_legacy_submit_rate_limit(&req, &state).await {
                        return resp;
                    }
                    return envelope_error(
                        &req,
                        StatusCode::BAD_REQUEST,
                        "VALIDATION_ERROR",
                        "transparent transfers require auth_pubkey and auth_signature \
                         (ed25519 over from|to|amount|nonce) from the from-account key",
                    );
                },
            };
        let payload = build_transparent_opt_out_payload(
            from,
            to,
            amount,
            body.transparent_opt_out_reason.as_deref(),
            Some(auth_pubkey),
            Some(auth_signature),
        );
        // Fail fast on a bad/forged authorization rather than enqueueing a tx
        // the mempool would reject. Bind authorization to the required caller
        // nonce so a captured request cannot be replayed on the legacy lane.
        if !payload.verify_authorization(nonce_u64) {
            // #3258 review R1: every failed authorization — each one costs an
            // ed25519 verification and an address derivation — is charged to
            // the legacy submit rate limit, so forged attempts cannot be
            // driven without bound. The limiter is keyed by the caller's
            // bearer token, never by `from`, so this spends only the caller's
            // own budget. Authenticated requests are charged to a separate
            // per-`from` budget after the replay check below, so a replay
            // costs no budget (#1846) and one relayed user's failures never
            // starve another user's sends on a shared token (#3337).
            if let Err(resp) = enforce_legacy_submit_rate_limit(&req, &state).await {
                return resp;
            }
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "AUTHORIZATION_INVALID",
                "auth_signature does not verify for auth_pubkey over from|to|amount|nonce, \
                 or auth_pubkey does not derive to from",
            );
        }
        (
            TransactionType::Transfer,
            serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
            "transparent_opt_out",
        )
    };

    // Deduplicate only after the caller has proved authorization. Recording
    // an unsigned or forged request here earlier let an attacker reserve a
    // victim's from+nonce for the full permissionless replay TTL (#3258).
    //
    // #3337 order: (1) refuse an already-observed replay read-only, before
    // any budget is charged (#1846); (2) charge the authenticated budget;
    // (3) only then reserve the key (atomic, durable); (4) hand it back if
    // the mempool refuses the transfer. A rate-limited or refused transfer
    // never entered the mempool and stays retryable at the same nonce, and a
    // 429 costs no storage write.
    let replay_key = format!("legacy:{from}:{nonce_u64}");
    if let Err(resp) = refuse_seen_permissionless_replay_key(&req, &state, &replay_key).await {
        return resp;
    }

    // #3337: authenticated transfers are charged per signed `from` (plus a
    // per-token ceiling), not to the bearer's failed-authorization budget
    // charged above (#3258 R1) — the backend relays every user with one token.
    if let Err(resp) = enforce_legacy_authenticated_submit_rate_limit(&req, &state, from).await {
        return resp;
    }

    // Atomic reservation: a concurrent duplicate that passed the read-only
    // check above is refused here, so two copies can never both enqueue.
    let reserved_at =
        match enforce_permissionless_replay_protection(&req, &state, &replay_key).await {
            Ok(reserved_at) => reserved_at,
            Err(resp) => return resp,
        };

    let accepted = match enqueue_protocol_transaction_with_nonce(
        state.clone(),
        tx_type,
        from,
        payload_value,
        nonce_u64,
    )
    .await
    {
        Ok(accepted) => accepted,
        Err(err) => {
            release_permissionless_replay_key(&state, &replay_key, reserved_at).await;
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "TRANSACTION_REJECTED",
                err,
            );
        },
    };

    envelope_success(
        &req,
        StatusCode::ACCEPTED,
        serde_json::json!({
            "accepted": true,
            "tx_id": accepted.id,
            "tx_type": accepted.tx_type,
            "from": from,
            "to": to,
            "amount": amount.to_string(),
            "privacy_mode": privacy_mode,
            "transparent_opt_out": transparent_opt_out,
            "default_privacy_mode": "feature_disabled",
            "transparent_mode_note": "supported transfers require transparent_opt_out=true with from-account authorization (#1727)",
        }),
    )
}

fn build_permissionless_signing_message(
    req: &HttpRequest,
    body: &PermissionlessSubmitRequest,
) -> Result<String, HttpResponse> {
    let canonical_payload = serde_json::to_string(&body.payload).map_err(|_| {
        envelope_error(
            req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "payload must be valid JSON for canonical signing",
        )
    })?;
    Ok(format!(
        "permissionless-v1|{}|{}|{}|{}|{}",
        body.signer_public_key.trim(),
        body.nonce.trim(),
        body.timestamp,
        body.tx_type.trim().to_ascii_lowercase(),
        canonical_payload
    ))
}

pub async fn submit_permissionless_transaction(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<PermissionlessSubmitRequest>,
) -> impl Responder {
    if let Err(resp) = validate_nonce(&req, &body.nonce) {
        return resp;
    }
    if let Err(resp) = validate_timestamp(&req, body.timestamp, "timestamp") {
        return resp;
    }
    if body.tx_type.trim().to_ascii_lowercase() != "data_analysis" {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
            "tx_type must be data_analysis for permissionless lane",
        );
    }

    let signer_key = match PublicKey::from_hex(body.signer_public_key.trim()) {
        Ok(pk) => pk,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "VALIDATION_ERROR",
                "signer_public_key must be an ed25519 public key hex string",
            );
        },
    };

    let signature = match Signature::from_hex(body.signature.trim()) {
        Ok(sig) => sig,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "VALIDATION_ERROR",
                "signature must be a valid ed25519 signature hex string",
            );
        },
    };

    let signing_message = match build_permissionless_signing_message(&req, &body) {
        Ok(message) => message,
        Err(resp) => return resp,
    };
    if !signer_key.verify(signing_message.as_bytes(), &signature) {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "SIGNATURE_INVALID",
            "signature did not verify for signer_public_key and canonical payload",
        );
    }

    // The replay key is taken only AFTER the signature verifies (#3258 review
    // R3): this route needs no role, so recording it earlier let anyone who
    // knows a signer's public key pre-burn that signer's nonces with junk
    // signatures. #3337 order: (1) refuse an already-observed replay
    // read-only, before the throttle, so a verified replay spends no budget;
    // (2) throttle; (3) validate the payload; (4) only then reserve the key
    // (atomic, durable); (5) hand it back if the mempool refuses. A
    // submission that is throttled, malformed or refused never entered the
    // mempool and stays retryable at the same nonce, and a throttled retry
    // loop writes no storage.
    let replay_key = hash_bytes(format!("{}|{}", body.signer_public_key, body.nonce).as_bytes())
        .to_string();
    if let Err(resp) = refuse_seen_permissionless_replay_key(&req, &state, &replay_key).await {
        return resp;
    }

    let signer_fingerprint = hash_bytes(body.signer_public_key.as_bytes()).to_string();
    if let Err(resp) =
        enforce_permissionless_submit_rate_limit(&req, &state, &signer_fingerprint).await
    {
        return resp;
    }

    let payload_object = match body.payload.as_object() {
        Some(map) => map,
        None => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "VALIDATION_ERROR",
                "payload must be a JSON object",
            );
        },
    };

    let from = match payload_object.get("from").and_then(|value| value.as_str()) {
        Some(value) if !value.trim().is_empty() => value.trim(),
        _ => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "VALIDATION_ERROR",
                "payload.from is required",
            );
        },
    };
    let to = match payload_object.get("to").and_then(|value| value.as_str()) {
        Some(value) if !value.trim().is_empty() => value.trim(),
        _ => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "VALIDATION_ERROR",
                "payload.to is required",
            );
        },
    };
    let amount = payload_object
        .get("amount")
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    if amount == 0 {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
            "payload.amount must be a positive integer",
        );
    }
    if from.parse::<Address>().is_err() || to.parse::<Address>().is_err() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
            "payload.from and payload.to must be valid hex addresses",
        );
    }

    let permissionless_payload = serde_json::json!({
        "from": from,
        "to": to,
        "amount": amount,
        "nonce": body.nonce,
        "timestamp": body.timestamp,
        "tx_type": "data_analysis",
        "signer_public_key": body.signer_public_key,
        "admission_policy": "behavior-risk-signature-v1"
    });

    // Atomic reservation: a concurrent duplicate that passed the read-only
    // check above is refused here, so two copies can never both enqueue.
    let reserved_at =
        match enforce_permissionless_replay_protection(&req, &state, &replay_key).await {
            Ok(reserved_at) => reserved_at,
            Err(resp) => return resp,
        };

    let response = submit_protocol_transaction(
        &req,
        state.clone(),
        TransactionType::DataAnalysis,
        &signer_fingerprint,
        permissionless_payload,
    )
    .await;
    if !response.status().is_success() {
        release_permissionless_replay_key(&state, &replay_key, reserved_at).await;
        return response;
    }
    declare_no_fund_movement(response).await
}

/// core4#3239 (owner Option A): this route is a participation signal. Its
/// transfer-shaped payload moves no funds, so a success response says so and
/// never reads like an accepted transfer.
async fn declare_no_fund_movement(response: HttpResponse) -> HttpResponse {
    let status = response.status();
    let bytes = match actix_web::body::to_bytes(response.into_body()).await {
        Ok(bytes) => bytes,
        Err(_) => return HttpResponse::build(status).finish(),
    };
    match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(serde_json::Value::Object(mut object)) => {
            object.insert("effect".to_string(), serde_json::json!("none"));
            object.insert("moves_funds".to_string(), serde_json::json!(false));
            HttpResponse::build(status).json(serde_json::Value::Object(object))
        }
        _ => HttpResponse::build(status)
            .content_type("application/json")
            .body(bytes),
    }
}

pub async fn get_participation_policy(req: HttpRequest) -> impl Responder {
    envelope_success(
        &req,
        StatusCode::OK,
        serde_json::json!({
            "policy_id": "permissionless-participation-v1",
            "human_readable_rules": [
                "Valid cryptographic signatures are required for permissionless submissions.",
                "Permissionless submit is a participation signal: it moves no funds, and its response says effect none.",
                "Replay attempts are rejected using signer+nonce replay protection.",
                "Behavior-based throttling is enforced for spam abuse controls.",
                "Default confidential transfer submission is disabled; supported transfers require explicit transparent_opt_out with from-account authorization.",
                "No jurisdictional or geofence-based exclusion logic is allowed."
            ],
            "privacy_safety": "Only aggregate and hashed telemetry are retained for abuse controls.",
            "enforcement_scope": "Actor-neutral across humans, AI agents, and robots."
        }),
    )
}

pub async fn submit_contribution_commit(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<ContributionCommitRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }
    if let Err(resp) = validate_participant_id(&req, &request.contributor, "contributor") {
        return resp;
    }
    if let Err(resp) = validate_station_id(&req, &request.station_id, "station_id") {
        return resp;
    }
    if let Err(resp) = validate_commit_hash(&req, &request.commit_hash) {
        return resp;
    }
    // Per-contributor backpressure (#1609): cap a single collector's commit
    // rate so a retry storm cannot flood the mempool and stall block
    // production while the node still reports healthy.
    if let Err(resp) = enforce_contribution_commit_rate_limit(&req, &state, &request.contributor).await
    {
        return resp;
    }
    // #2148: Sybil caps — stations per identity and commits per station per
    // window — refused before the commit can reach the mempool.
    if let Err(resp) = super::sybil::enforce_contribution_sybil_policy(
        &req,
        &state,
        &request.contributor,
        &request.station_id,
    ) {
        return resp;
    }

    let payload = CommitPayload {
        commit_id: request.commit_hash.clone(),
        station_id: request.station_id.clone(),
        timestamp: chrono::Utc::now().timestamp(),
        contribution_type: "contribution".to_string(),
        auth_pubkey: request.auth_pubkey.clone(),
        auth_signature: request.auth_signature.clone(),
    };
    let payload_value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    let accepted = match enqueue_protocol_transaction(
        state.clone(),
        TransactionType::ContributionCommit,
        &request.contributor,
        payload_value,
    )
    .await
    {
        Ok(accepted) => accepted,
        Err(err) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "TRANSACTION_REJECTED",
                err,
            );
        },
    };

    #[cfg(feature = "p2p")]
    if let Some(p2p) = &state.p2p {
        let _ = p2p.command_tx.try_send(P2pCommand::BroadcastWeatherDataV2 {
            station_id: request.station_id.clone(),
            data: request.commit_hash.as_bytes().to_vec(),
            content_ref: None,
            // #1969: sign V2 gossip with the node-local key so peers can verify
            // the origin and reject injected weather payloads.
            station_signing_key: (*state.weather_gossip_signing_key).clone(),
        });
    }

    HttpResponse::Created().json(accepted)
}

pub async fn submit_contribution_reveal(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<ContributionRevealRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }
    if let Err(resp) = validate_participant_id(&req, &request.contributor, "contributor") {
        return resp;
    }
    if let Err(resp) = validate_station_id(&req, &request.station_id, "station_id") {
        return resp;
    }
    if let Err(resp) = validate_commit_hash(&req, &request.commit_hash) {
        return resp;
    }
    if let Err(resp) = validate_nonce(&req, &request.nonce) {
        return resp;
    }
    if !request.payload.is_object() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "payload must be a JSON object",
        );
    }

    let reveal_timestamp = request
        .timestamp
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    if let Err(resp) = validate_timestamp(&req, reveal_timestamp, "timestamp") {
        return resp;
    }

    let reveal = RevealPayload {
        commit_id: request.commit_hash.clone(),
        station_id: request.station_id.clone(),
        timestamp: reveal_timestamp,
        contribution_type: "contribution".to_string(),
        data: request.payload.clone(),
        nonce: request.nonce.clone(),
        data_cid: request.data_cid.clone(),
        data_hash: request.data_hash.clone(),
        storage_provider: request.storage_provider.clone(),
        compute_receipt: request.compute_receipt.clone(),
        provenance: request.provenance.clone(),
        manifest: None,
        verification_receipt: None,
        quality_tier: None,
    };

    let computed = Transaction::compute_commit_id(
        &reveal.data,
        &reveal.nonce,
        &reveal.station_id,
        reveal.timestamp,
        &reveal.contribution_type,
    );
    if computed != reveal.commit_id {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "Reveal payload does not match commit_hash",
        );
    }

    let payload_value = serde_json::to_value(reveal).unwrap_or(serde_json::Value::Null);
    submit_protocol_transaction(
        &req,
        state,
        TransactionType::ContributionReveal,
        &request.contributor,
        payload_value,
    )
    .await
}

// ---------------------------------------------------------------------------
// Forecast submission (2026-09-09 forecast earning-loop prototype)
//
// `TransactionType::ForecastCommit` / `ForecastReveal` are handled end to end
// by the mempool (`legacy/pool.rs`), the transition extractor and the
// consensus reward loop — but before this there was NO HTTP surface that
// could put one in the mempool. `/transactions/contribution/reveal` hardwires
// `manifest: None, verification_receipt: None` and emits a
// `ContributionReveal`, and `/transactions/submit` is transfer-only. These
// two routes are the missing surface, deliberately kept as thin twins of the
// contribution pair so the validation, rate limiting and payee rules stay
// identical; the only differences are `contribution_type: "forecast"`, the
// forecast transaction types, and carrying the manifest and the oracle
// verification receipt through to the reveal payload.
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct ForecastCommitRequest {
    pub forecaster: String,
    pub station_id: String,
    pub commit_hash: String,
    /// Forecaster's hex-encoded ed25519 public key (#1848).
    #[serde(default)]
    pub auth_pubkey: Option<String>,
    /// Hex-encoded ed25519 signature over the canonical commit-authorization
    /// message, produced by the holder of `auth_pubkey` (#1848).
    #[serde(default)]
    pub auth_signature: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ForecastRevealRequest {
    pub forecaster: String,
    pub station_id: String,
    pub payload: serde_json::Value,
    pub nonce: String,
    pub commit_hash: String,
    pub timestamp: Option<i64>,
    /// The forecast artifact manifest. Required: a `ForecastReveal` whose
    /// manifest is absent or invalid is skipped entirely by the consensus
    /// reward loop, so accepting one would be accepting an unpayable reveal.
    pub manifest: ForecastArtifactManifest,
    /// Optional oracle scoring receipt. When present, valid, signed by an
    /// authorized oracle under the chain binding this node resolves, and
    /// bound to this manifest and commit, consensus scores the forecast at
    /// the receipt's `skill_score` instead of `BASELINE_FORECAST_SKILL`.
    #[serde(default)]
    pub verification_receipt: Option<ForecastVerificationReceipt>,
    #[serde(default)]
    pub data_cid: Option<String>,
    #[serde(default)]
    pub data_hash: Option<String>,
    #[serde(default)]
    pub storage_provider: Option<String>,
    #[serde(default)]
    pub compute_receipt: Option<ComputeReceipt>,
    #[serde(default)]
    pub provenance: Option<SensorProvenance>,
}

pub async fn submit_forecast_commit(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<ForecastCommitRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }
    if let Err(resp) = validate_participant_id(&req, &request.forecaster, "forecaster") {
        return resp;
    }
    // #3070: the bearer's wallet claim, not the request body, decides whom a
    // forecast may be submitted for.
    // Everything below uses `forecaster`, the canonical identity, never the
    // raw request string: case/whitespace variants must share one rate-limit
    // bucket and one sender (review of core4#3114).
    let forecaster = match require_forecaster_bound_to_bearer(&req, &state, &request.forecaster) {
        Ok(canonical) => canonical,
        Err(resp) => return resp,
    };
    if let Err(resp) = validate_station_id(&req, &request.station_id, "station_id") {
        return resp;
    }
    if let Err(resp) = validate_commit_hash(&req, &request.commit_hash) {
        return resp;
    }
    if let Err(resp) =
        enforce_contribution_commit_rate_limit(&req, &state, &forecaster).await
    {
        return resp;
    }

    let payload = CommitPayload {
        commit_id: request.commit_hash.clone(),
        station_id: request.station_id.clone(),
        // The stamp is frozen into the CommitPayload that is submitted, and
        // every validator then replays that recorded number instead of
        // reading its own clock. Identical construct to the contribution
        // twin above, which is frozen in the determinism baseline.
        // determinism-ok: HTTP ingress stamp, not a consensus computation
        timestamp: chrono::Utc::now().timestamp(),
        contribution_type: "forecast".to_string(),
        auth_pubkey: request.auth_pubkey.clone(),
        auth_signature: request.auth_signature.clone(),
    };
    // The bearer grants a submission role, not authority to forge a commit.
    // Reuse the canonical, chain-bound authorization used at consensus admission.
    // Below FORECAST_MANIFEST_ACTIVATION_HEIGHT this edge check is the only authorization gate; mempool enforcement is height-gated.
    if !payload.verify_authorization() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "INVALID_FORECAST_AUTHORIZATION",
            "forecast commit requires valid chain-bound auth_pubkey and auth_signature",
        );
    }
    let payload_value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    submit_protocol_transaction(
        &req,
        state,
        TransactionType::ForecastCommit,
        &forecaster,
        payload_value,
    )
    .await
}

pub async fn submit_forecast_reveal(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<ForecastRevealRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }
    if let Err(resp) = validate_participant_id(&req, &request.forecaster, "forecaster") {
        return resp;
    }
    // #3070: the bearer's wallet claim, not the request body, decides whom a
    // forecast may be submitted for.
    // Everything below uses `forecaster`, the canonical identity, never the
    // raw request string: case/whitespace variants must share one rate-limit
    // bucket and one sender (review of core4#3114).
    let forecaster = match require_forecaster_bound_to_bearer(&req, &state, &request.forecaster) {
        Ok(canonical) => canonical,
        Err(resp) => return resp,
    };
    if let Err(resp) = validate_station_id(&req, &request.station_id, "station_id") {
        return resp;
    }
    if let Err(resp) = validate_commit_hash(&req, &request.commit_hash) {
        return resp;
    }
    if let Err(resp) = validate_nonce(&req, &request.nonce) {
        return resp;
    }
    if !request.payload.is_object() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "payload must be a JSON object",
        );
    }
    // Reject an unpayable reveal at the edge rather than letting it occupy a
    // block: consensus skips any ForecastReveal whose manifest does not
    // validate.
    if let Err(err) = request.manifest.validate() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("manifest is invalid: {err}"),
        );
    }
    // Refuse unusable receipts before they occupy the mempool. This read-only
    // check invokes the same verifier as state replay, without applying a slash.
    // Peer-submitted transactions still undergo the unchanged consensus checks.
    if let Some(receipt) = request.verification_receipt.as_ref() {
        if let Err(err) = receipt.validate() {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                format!("verification_receipt is invalid: {err}"),
            );
        }
        if receipt.nonce.trim().is_empty() {
            return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY,
                "INVALID_FORECAST_RECEIPT", "verification_receipt nonce must not be blank");
        }
        if receipt.commit_id != request.commit_hash
            || receipt.manifest_digest != request.manifest.manifest_digest()
        {
            return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY,
                "INVALID_FORECAST_RECEIPT", "verification_receipt must bind this commit_hash and manifest");
        }
        if !receipt.has_valid_signature() {
            return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY,
                "INVALID_FORECAST_RECEIPT",
                "verification_receipt requires a valid signature under this node's configured chain binding");
        }
        let chain = state.blockchain.read().await;
        // #3070: is_registered is oracle_set().contains, the exact set copied by
        // PopConsensusAdapter::create_block into PoP's is_authorized_oracle.
        // Receipt validation here may refuse more than the reward loop, never less.
        // Below FORECAST_MANIFEST_ACTIVATION_HEIGHT the edge is the only gate; mempool enforcement is height-gated.
        if let Some(trigger) = chain.state.oracle_registry().verify_receipt_with_slashing(
            receipt, &request.manifest, &*chain.state.zk_verifier,
        ) {
            return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY,
                "INVALID_FORECAST_RECEIPT", format!("verification_receipt refused: {trigger:?}"));
        }
    }

    let reveal_timestamp = request
        .timestamp
        // Used only when the client omits `timestamp`. validate_timestamp
        // bounds it below and it must still reproduce the client's
        // commit_id, so this read cannot change what consensus replays.
        // Identical construct to the contribution twin above, frozen in
        // the determinism baseline.
        // determinism-ok: HTTP ingress default, not a consensus computation
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    if let Err(resp) = validate_timestamp(&req, reveal_timestamp, "timestamp") {
        return resp;
    }

    let reveal = RevealPayload {
        commit_id: request.commit_hash.clone(),
        station_id: request.station_id.clone(),
        timestamp: reveal_timestamp,
        contribution_type: "forecast".to_string(),
        data: request.payload.clone(),
        nonce: request.nonce.clone(),
        data_cid: request.data_cid.clone(),
        data_hash: request.data_hash.clone(),
        storage_provider: request.storage_provider.clone(),
        compute_receipt: request.compute_receipt.clone(),
        provenance: request.provenance.clone(),
        manifest: Some(request.manifest.clone()),
        verification_receipt: request.verification_receipt.clone(),
        quality_tier: None,
    };

    let computed = Transaction::compute_commit_id(
        &reveal.data,
        &reveal.nonce,
        &reveal.station_id,
        reveal.timestamp,
        &reveal.contribution_type,
    );
    if computed != reveal.commit_id {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "Reveal payload does not match commit_hash",
        );
    }

    let payload_value = serde_json::to_value(reveal).unwrap_or(serde_json::Value::Null);
    submit_protocol_transaction(
        &req,
        state,
        TransactionType::ForecastReveal,
        &forecaster,
        payload_value,
    )
    .await
}

pub fn contribution_status(
    state: &wrfcoin_native_chain::blockchain::state::State,
    commit_id: &str,
) -> ContributionStatusResponse {
    let commit = state.pending_commits.get(commit_id);
    let reveal = state.pending_reveals.get(commit_id);
    let reward = state.contribution_reward_records.get(commit_id);
    let attestation = state.attestations.get(commit_id);

    let approvals = attestation.map(|s| s.approvals()).unwrap_or(0);
    let rejections = attestation
        .map(|s| s.by_oracle.values().filter(|r| !r.approved).count())
        .unwrap_or(0);
    let has_quorum = attestation
        .map(|s| s.has_quorum(state.oracle_quorum))
        .unwrap_or(false);
    let reward_finalized = state.finalized_contribution_rewards.contains(commit_id);

    let status = if reveal.is_some() && has_quorum {
        "revealed_attested"
    } else if reveal.is_some() {
        "revealed_pending_attestation"
    } else if commit.is_some() {
        "committed_pending_reveal"
    } else if attestation.is_some() {
        "attested_without_pending_records"
    } else if reward.is_some() && reward_finalized {
        "reward_finalized"
    } else if reward.is_some() {
        "reward_pending_quorum"
    } else {
        "not_found"
    };
    let reward_status = match reward {
        None => "not_evaluated",
        Some(_) if !reward_finalized => "pending_quorum",
        Some(record) if !record.provenance_class.is_reward_eligible() => "excluded_non_real",
        Some(record) if record.amount.0 == 0 => "finalized_zero",
        Some(_) => "paid",
    };

    ContributionStatusResponse {
        commit_id: commit_id.to_string(),
        station_id: commit
            .map(|c| c.station_id.clone())
            .or_else(|| reveal.map(|r| r.station_id.clone()))
            .or_else(|| reward.map(|r| r.station_id.clone())),
        contribution_type: commit.map(|c| c.contribution_type.clone()).or_else(|| {
            reward.map(|record| {
                if record.reason.starts_with("forecast:") {
                    "forecast".to_string()
                } else {
                    "contribution".to_string()
                }
            })
        }),
        committed_at: commit.map(|c| c.committed_at),
        commit_expires_at: commit.map(|c| c.commit_expires_at),
        reveal_expires_at: commit.map(|c| c.reveal_expires_at),
        revealed_at: reveal.map(|r| r.revealed_at),
        provenance_class: commit
            .map(|c| c.provenance_class)
            .or_else(|| reveal.map(|r| r.provenance_class))
            .or_else(|| reward.map(|r| r.provenance_class)),
        reward_amount: reward.map(|r| r.amount.0),
        reward_finalized,
        reward_status: reward_status.to_string(),
        approvals,
        rejections,
        has_quorum,
        oracle_quorum: state.oracle_quorum,
        status: status.to_string(),
    }
}

pub async fn get_contribution_statuses(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<ContributionStatusQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let limit = query
        .limit
        .unwrap_or(DEFAULT_CONTRIBUTION_STATUS_PAGE_SIZE)
        .min(MAX_CONTRIBUTION_STATUS_PAGE_SIZE)
        .max(1);
    let offset = query.offset.unwrap_or(0);

    let blockchain = state.blockchain.read().await;
    let state_ref = &blockchain.state;

    let mut commit_ids: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    commit_ids.extend(state_ref.pending_commits.keys().cloned());
    commit_ids.extend(state_ref.pending_reveals.keys().cloned());
    commit_ids.extend(state_ref.attestations.keys().cloned());
    commit_ids.extend(state_ref.contribution_reward_records.keys().cloned());

    let mut all_items: Vec<ContributionStatusResponse> = commit_ids
        .iter()
        .map(|id| contribution_status(state_ref, id))
        .collect();

    // Stable deterministic order: descending by timestamp, then ascending by commit_id as
    // tie-breaker so identical timestamps yield the same page across requests.
    all_items.sort_by(|a, b| {
        let a_ts = a.revealed_at.or(a.committed_at).unwrap_or(0);
        let b_ts = b.revealed_at.or(b.committed_at).unwrap_or(0);
        b_ts.cmp(&a_ts).then_with(|| a.commit_id.cmp(&b.commit_id))
    });

    let filtered: Vec<ContributionStatusResponse> = if let Some(ref status_filter) = query.status {
        all_items
            .into_iter()
            .filter(|item| &item.status == status_filter)
            .collect()
    } else {
        all_items
    };

    let total = filtered.len();
    let items: Vec<ContributionStatusResponse> =
        filtered.into_iter().skip(offset).take(limit).collect();

    HttpResponse::Ok().json(ContributionStatusListResponse {
        total,
        offset,
        limit,
        items,
    })
}

// ---------------------------------------------------------------------------
// #2456 — evidence-backed reward progress: one read model per commit
// ---------------------------------------------------------------------------
//
// The onboarding flow must never infer "earning" from registration. This route
// assembles, for ONE commit id, every leg of the evidence chain the story names
// and says which leg is missing. It is fail-closed: `rewarded` is true only when
// a non-zero, reward-eligible, finalized `contribution:<commit_id>` record
// exists, the block that carried it is found and is at or below the BFT
// finalized height, and the payee account's earned total covers the amount.
// Anything short of that is reported as exactly what it is — a pending quorum,
// a zero decision, an unfinalized block, a block outside the search window —
// never as success. Chain identity travels with the answer so a consumer can
// bind the evidence to `network_id` / `chain_id` / `genesis_hash` and archive,
// rather than count, evidence from a reset chain.

/// Blocks below the tip the block search covers: the in-memory window first,
/// then the durable archive. `WRFCOIN_REWARD_EVIDENCE_SCAN_BLOCKS` overrides
/// the default up to the ceiling.
pub const DEFAULT_REWARD_EVIDENCE_SCAN_BLOCKS: u64 = 4_096;
pub const MAX_REWARD_EVIDENCE_SCAN_BLOCKS: u64 = 65_536;
pub const REWARD_EVIDENCE_SCAN_BLOCKS_ENV: &str = "WRFCOIN_REWARD_EVIDENCE_SCAN_BLOCKS";
/// Archive reads are chunked and byte-bounded so one request cannot pull the
/// whole history into memory (same discipline as the block-sync serve path).
const REWARD_EVIDENCE_ARCHIVE_CHUNK_BLOCKS: u64 = 256;
const REWARD_EVIDENCE_ARCHIVE_CHUNK_BYTES: u64 = 8 << 20;
/// Block-metadata key the producer writes its applied reward records under.
const REWARD_ENTRIES_METADATA_KEY: &str = "reward_entries";

pub fn reward_evidence_scan_blocks() -> u64 {
    // determinism-ok: node-local read-API search window for GET /transactions/contributions/{id}/reward-evidence (capped by MAX_REWARD_EVIDENCE_SCAN_BLOCKS); it bounds a read-locked block scan and never reaches state or consensus
    std::env::var(REWARD_EVIDENCE_SCAN_BLOCKS_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_REWARD_EVIDENCE_SCAN_BLOCKS)
        .min(MAX_REWARD_EVIDENCE_SCAN_BLOCKS)
}

#[derive(Debug, Clone, Serialize)]
pub struct ChainIdentityEvidence {
    /// From the genesis block when it is available, else the node's declared
    /// identity; `None` when neither is known — never a placeholder.
    pub network_id: Option<String>,
    pub chain_id: Option<String>,
    pub genesis_hash: Option<String>,
    pub height: u64,
    pub finalized_height: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RewardRecordEvidence {
    pub address: String,
    #[serde(serialize_with = "crate::routes::decimal::u128")]
    pub amount: u128,
    pub reason: String,
    pub station_id: String,
    pub provenance_class: ContributionProvenanceClass,
    pub reward_eligible: bool,
    /// The decision reached quorum and the account was credited.
    pub finalized: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RewardBlockEvidence {
    pub height: u64,
    pub hash: Option<String>,
    pub timestamp: i64,
    /// `height <= finalized_height`.
    pub finalized: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockSearchEvidence {
    pub scanned_from: u64,
    pub scanned_to: u64,
    pub archive_consulted: bool,
    /// Every height in `[scanned_from, scanned_to]` was inspected. When false,
    /// a missing block is "unknown", not "absent".
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountEvidence {
    pub address: String,
    #[serde(serialize_with = "crate::routes::decimal::u128")]
    pub balance: u128,
    #[serde(serialize_with = "crate::routes::decimal::u128")]
    pub earned_balance_total: u128,
    pub credit_covers_reward: bool,
}

#[derive(Debug, Serialize)]
pub struct RewardEvidenceResponse {
    pub commit_id: String,
    pub chain: ChainIdentityEvidence,
    pub contribution: ContributionStatusResponse,
    pub reward: Option<RewardRecordEvidence>,
    pub block: Option<RewardBlockEvidence>,
    pub block_search: BlockSearchEvidence,
    pub account: Option<AccountEvidence>,
    /// True only when every leg above is proven. Never inferred.
    pub rewarded: bool,
    /// The first unproven leg, when `rewarded` is false.
    pub rewarded_blocker: Option<&'static str>,
}

fn block_carries_reward_for(
    block: &wrfcoin_native_chain::blockchain::block::Block,
    commit_id: &str,
) -> bool {
    use wrfcoin_native_chain::blockchain::state::ContributionRewardRecord;
    block
        .metadata
        .get(REWARD_ENTRIES_METADATA_KEY)
        .and_then(|raw| serde_json::from_str::<Vec<ContributionRewardRecord>>(raw).ok())
        .map(|records| records.iter().any(|r| r.commit_id() == Some(commit_id)))
        .unwrap_or(false)
}

fn reward_block_evidence(
    block: &wrfcoin_native_chain::blockchain::block::Block,
    finalized_height: u64,
) -> RewardBlockEvidence {
    RewardBlockEvidence {
        height: block.height,
        hash: block.hash.as_ref().map(|h| h.to_string()),
        timestamp: block.timestamp.timestamp(),
        finalized: block.height <= finalized_height,
    }
}

/// Decide `rewarded` from the assembled legs; returns the first blocker.
pub fn reward_evidence_verdict(
    reward: Option<&RewardRecordEvidence>,
    block: Option<&RewardBlockEvidence>,
    search_complete: bool,
    account: Option<&AccountEvidence>,
) -> Result<(), &'static str> {
    let Some(reward) = reward else { return Err("no_reward_record") };
    if !reward.reward_eligible {
        return Err("provenance_not_reward_eligible");
    }
    if reward.amount == 0 {
        return Err("zero_amount");
    }
    if !reward.finalized {
        return Err("reward_not_finalized");
    }
    let Some(block) = block else {
        return Err(if search_complete { "block_not_found_in_scan_window" } else { "block_scan_incomplete" });
    };
    if !block.finalized {
        return Err("block_not_finalized");
    }
    let Some(account) = account else { return Err("account_missing") };
    if !account.credit_covers_reward {
        return Err("account_credit_short");
    }
    Ok(())
}

pub async fn get_contribution_reward_evidence(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let commit_id = path.into_inner();
    let scan_blocks = reward_evidence_scan_blocks();

    // Everything state-derived under one read lock; the archive scan (if any)
    // runs after the lock is released.
    let (contribution, reward, account, chain, mut block, scanned_to, scan_floor, memory_floor) = {
        let blockchain = state.blockchain.read().await;
        let st = &blockchain.state;
        let contribution = contribution_status(st, &commit_id);
        if contribution.status == "not_found" {
            return envelope_error(
                &req,
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                "no commit, reveal, attestation or reward record for this commit id",
            );
        }
        let finalized = st.finalized_contribution_rewards.contains(&commit_id);
        let record = st.contribution_reward_records.get(&commit_id);
        let reward = record.map(|r| RewardRecordEvidence {
            address: r.address.to_string(),
            amount: r.amount.0,
            reason: r.reason.clone(),
            station_id: r.station_id.clone(),
            provenance_class: r.provenance_class,
            reward_eligible: r.provenance_class.is_reward_eligible(),
            finalized,
        });
        let account = record.and_then(|r| {
            st.get_account(&r.address).map(|a| AccountEvidence {
                address: r.address.to_string(),
                balance: a.balance.0,
                earned_balance_total: a.earned_balance_total.0,
                credit_covers_reward: finalized && a.earned_balance_total.0 >= r.amount.0,
            })
        });

        let tip = blockchain.height;
        let finalized_height = blockchain.finalized_height();
        let genesis = blockchain.block_at_height(1);
        let chain = ChainIdentityEvidence {
            network_id: genesis
                .and_then(|b| b.metadata.get(wrfcoin_shared_protocol::crypto::chain_binding::GENESIS_NETWORK_ID_KEY).cloned())
                // determinism-ok: identity FALLBACK for the reward-evidence API response only (genesis metadata is preferred); the handler is read-only and this value is serialized to the caller, never hashed, signed or applied to state
                .or_else(|| std::env::var("WRFCOIN_NETWORK_ID").ok().filter(|v| !v.trim().is_empty())),
            chain_id: st
                .genesis_emission_chain_id()
                .map(str::to_string)
                // determinism-ok: identity FALLBACK for the reward-evidence API response only (the state's genesis emission chain id is preferred); read-only handler, never hashed, signed or applied to state
                .or_else(|| std::env::var("WRFCOIN_CHAIN_ID").ok().filter(|v| !v.trim().is_empty())),
            genesis_hash: state.genesis_hash.get(),
            height: tip,
            finalized_height,
        };

        // In-memory window, newest first; the first gap marks the window floor.
        let scan_floor = tip.saturating_sub(scan_blocks).max(1);
        let mut block = None;
        let mut memory_floor = tip.saturating_add(1);
        if tip >= 1 {
            for h in (scan_floor..=tip).rev() {
                let Some(candidate) = blockchain.block_at_height(h) else { break };
                memory_floor = h;
                if block_carries_reward_for(candidate, &commit_id) {
                    block = Some(reward_block_evidence(candidate, finalized_height));
                    break;
                }
            }
        }
        (contribution, reward, account, chain, block, tip, scan_floor, memory_floor)
    };

    // Durable archive for the part of the window the in-memory chain no longer
    // holds: chunked, byte-bounded, newest chunk first, off the async lock.
    let mut archive_consulted = false;
    let mut complete = true;
    if block.is_none() && memory_floor > scan_floor && scanned_to >= 1 {
        archive_consulted = true;
        let storage = state.storage.read().await.clone();
        let mut hi = memory_floor - 1;
        let finalized_height = chain.finalized_height;
        let wanted = commit_id.clone();
        while hi >= scan_floor && block.is_none() {
            let lo = hi.saturating_sub(REWARD_EVIDENCE_ARCHIVE_CHUNK_BLOCKS - 1).max(scan_floor);
            let handle = storage.clone();
            let chunk = tokio::task::spawn_blocking(move || {
                handle.load_blocks_range_bounded(lo, hi, REWARD_EVIDENCE_ARCHIVE_CHUNK_BYTES)
            })
            .await;
            let Ok(Ok((blocks, _))) = chunk else {
                complete = false;
                break;
            };
            // A short chunk means the byte budget stopped it early or the
            // archive has gaps; either way the range was not fully inspected.
            if blocks.len() as u64 != hi - lo + 1 {
                complete = false;
            }
            if let Some(found) = blocks.iter().rev().find(|b| block_carries_reward_for(b, &wanted)) {
                block = Some(reward_block_evidence(found, finalized_height));
            }
            if lo == scan_floor || lo == 0 {
                break;
            }
            hi = lo - 1;
        }
    }
    let block_search = BlockSearchEvidence {
        scanned_from: scan_floor.min(scanned_to.max(1)),
        scanned_to,
        archive_consulted,
        complete,
    };

    let verdict = reward_evidence_verdict(reward.as_ref(), block.as_ref(), block_search.complete, account.as_ref());
    HttpResponse::Ok().json(RewardEvidenceResponse {
        commit_id,
        chain,
        contribution,
        reward,
        block,
        block_search,
        account,
        rewarded: verdict.is_ok(),
        rewarded_blocker: verdict.err(),
    })
}

pub async fn get_commit_status(state: web::Data<AppState>, path: web::Path<String>) -> impl Responder {
    let commit_id = path.into_inner();
    let blockchain = state.blockchain.read().await;
    let status = contribution_status(&blockchain.state, &commit_id);

    if status.status == "not_found" {
        return HttpResponse::NotFound().json(status);
    }
    HttpResponse::Ok().json(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_native_chain::blockchain::state::{
        ContributionRewardRecord, State, StateTransition,
    };
    use wrfcoin_native_chain::common::types::{Address, Balance};

    #[test]
    fn durable_reward_status_survives_commit_and_reveal_pruning() {
        let commit_id = "status-pruned-synthetic";
        let mut state = State::new();
        state
            .apply_transition(StateTransition::ApplyContributionRewardDecision {
                record: ContributionRewardRecord {
                    address: Address([0x24; 20]),
                    amount: Balance::new(0),
                    reason: format!("contribution:{commit_id}"),
                    station_id: "station-status-synthetic".to_string(),
                    provenance_class: ContributionProvenanceClass::Synthetic,
                },
                current_block: 0,
            })
            .unwrap();
        assert!(state.pending_commits.is_empty());
        assert!(state.pending_reveals.is_empty());

        let status = contribution_status(&state, commit_id);
        assert_eq!(status.status, "reward_finalized");
        assert_eq!(
            status.station_id.as_deref(),
            Some("station-status-synthetic")
        );
        assert_eq!(
            status.provenance_class,
            Some(ContributionProvenanceClass::Synthetic)
        );
        assert_eq!(status.reward_amount, Some(0));
        assert!(status.reward_finalized);
        assert_eq!(status.reward_status, "excluded_non_real");
    }
}

/// `after` is the last fully scanned height; `offset` resumes within after+1.
/// Copy both returned cursor fields. Work is bounded even for sparse stations.
#[derive(Debug, Deserialize)]
pub struct ObservationQuery {
    pub after: Option<u64>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

pub async fn get_station_observations(
    req: HttpRequest,
    state: web::Data<AppState>,
    station: web::Path<String>,
    query: web::Query<ObservationQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) { return resp; }
    if let Err(resp) = validate_station_id(&req, &station, "station_id") { return resp; }
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let mut after = query.after.unwrap_or(0);
    let mut offset = query.offset.unwrap_or(0);
    let chain = state.blockchain.read().await;
    let mut items = Vec::new();
    let mut scanned = 0usize;
    let mut heights = 0usize;
    while after < chain.height && items.len() < limit && scanned < 1000 && heights < 64 {
        let height = after + 1;
        let block = match chain.block_at_height(height) {
            Some(block) => block.clone(),
            None => {
                let storage = state.storage.read().await;
                match storage.load_blocks_range(height, height) {
                    Ok(mut blocks) if blocks.len() == 1 => blocks.remove(0),
                    _ => return envelope_error(&req, StatusCode::SERVICE_UNAVAILABLE,
                        "OBSERVATION_HISTORY_UNAVAILABLE", "requested block history is unavailable"),
                }
            },
        };
        if offset > block.transactions.len() {
            return envelope_error(&req, StatusCode::BAD_REQUEST,
                "INVALID_CURSOR", "offset exceeds the next block's transaction count");
        }
        heights += 1;
        while offset < block.transactions.len() && items.len() < limit && scanned < 1000 {
            let Some(tx) = block.transactions.get(offset) else { break; };
            offset += 1;
            scanned += 1;
            if tx.tx_type != TransactionType::ContributionReveal { continue; }
            let Some(reveal) = tx.parse_reveal_payload() else { continue; };
            if reveal.station_id != *station || reveal.contribution_type != "contribution" { continue; }
            let Ok(Some(parsed)) = wrfcoin_shared_protocol::weather::schema::parse_weather_payload(&reveal.data)
                else { continue; };
            let observation = parsed.observation;
            // Explicit numeric allowlist: never echo arbitrary payload keys,
            // provenance blobs, radio/MAC metadata, owners or account balances.
            let values = serde_json::json!({
                "temperature_c": observation.temperature_c,
                "humidity_percent": observation.humidity_percent,
                "pressure_hpa": observation.pressure_hpa,
                "wind_speed_ms": observation.wind_speed_ms,
                "precipitation_mm": observation.precipitation_mm,
                "dewpoint_c": observation.dewpoint_c,
            });
            let provenance = chain.state.pending_reveals.get(&reveal.commit_id)
                .map(|record| record.provenance_class)
                .or_else(|| chain.state.contribution_reward_records.get(&reveal.commit_id)
                    .map(|record| record.provenance_class));
            let attestation = chain.state.attestations.get(&reveal.commit_id).map(|summary| {
                serde_json::json!({"approvals": summary.approvals(),
                    "has_quorum": summary.has_quorum(chain.state.oracle_quorum)})
            });
            items.push(serde_json::json!({
                "commit_id": reveal.commit_id, "station_id": reveal.station_id,
                "height": height, "observed_at": reveal.timestamp, "values": values,
                "provenance_class": provenance, "attestation": attestation,
            }));
        }
        if offset == block.transactions.len() { after = height; offset = 0; }
    }
    envelope_success(&req, StatusCode::OK, serde_json::json!({
        "items": items, "limit": limit, "next_after": after, "next_offset": offset,
        "has_more": after < chain.height, "chain_height": chain.height,
        "state_tip": chain.tip.to_string(), "scanned_transactions": scanned,
    }))
}
