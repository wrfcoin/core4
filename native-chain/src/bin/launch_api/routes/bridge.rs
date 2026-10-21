use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::{Deserialize, Serialize};

use crate::helpers::{envelope_error, require_role};
use crate::state::{AppState, MAX_PARTICIPANT_ID_LEN};
use wrfcoin_native_chain::blockchain::state::{AuditEventKind, AuditEventRecord, StateTransition};
use wrfcoin_native_chain::bridge::types::HaltTrigger;
use wrfcoin_native_chain::security::api_auth::ApiRole;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeHaltRequest {
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeResumeRequest {
    pub reason: String,
    pub operator: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeChallengeOpenRequest {
    pub tx_id: String,
    pub chain_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeChallengeSubmitRequest {
    pub tx_id: String,
    pub challenger_id: String,
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeChallengeResolveRequest {
    pub tx_id: String,
    pub upheld: bool,
    pub note: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeCapsUpdateRequest {
    pub per_chain_cap: u64,
    pub global_cap: u64,
    pub updated_by: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeScanStuckRequest {
    pub transactions: Vec<BridgeScanStuckEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeScanStuckEntry {
    pub tx_id: String,
    pub status: String,
    pub timestamp: i64,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const MAX_BRIDGE_REASON_LEN: usize = 512;
pub const MAX_BRIDGE_TX_ID_LEN: usize = 128;
pub const MAX_BRIDGE_SCAN_SIZE: usize = 256;

async fn persist_bridge_audit_event(
    req: &HttpRequest,
    state: &web::Data<AppState>,
    now: i64,
    kind: AuditEventKind,
    subject_id: Option<String>,
    actor_id: Option<String>,
    details: serde_json::Value,
) -> Result<(), HttpResponse> {
    let mut blockchain = state.blockchain.write().await;
    let block_height = blockchain.height;
    blockchain
        .apply_state_transition(StateTransition::RecordAuditEvent {
            event: AuditEventRecord {
                event_id: 0,
                block_height,
                timestamp: now,
                kind,
                subject_id,
                actor_id,
                details: Some(details),
            },
        })
        .map_err(|e| {
            // #2138: a storage failure's text (store, path, io detail) is a node
            // internal; it is logged under a correlation id, not sent.
            crate::helpers::envelope_redacted(
                req,
                wrfcoin_native_chain::security::error_redaction::InternalError::new(
                    wrfcoin_native_chain::security::error_redaction::ErrorKind::Storage,
                    format!("persist bridge audit event: {e}"),
                ),
            )
        })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn get_bridge_safety_status(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let manager = state.bridge_safety.read().await;
    let now = chrono::Utc::now().timestamp();
    let status = manager.get_status(now);
    HttpResponse::Ok().json(status)
}

pub async fn halt_bridge(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<BridgeHaltRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    let reason = request.reason.trim();
    if reason.is_empty() || reason.len() > MAX_BRIDGE_REASON_LEN {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("reason must be non-empty and at most {MAX_BRIDGE_REASON_LEN} characters"),
        );
    }

    let now = chrono::Utc::now().timestamp();
    let status = {
        let mut manager = state.bridge_safety.write().await;
        manager.halt(reason, HaltTrigger::ManualHalt, now);
        manager.get_status(now)
    };
    if let Err(resp) = persist_bridge_audit_event(
        &req,
        &state,
        now,
        AuditEventKind::BridgeSafetyHalt,
        None,
        None,
        serde_json::json!({ "reason": reason, "trigger": "manual_halt" }),
    )
    .await
    {
        return resp;
    }
    HttpResponse::Ok().json(status)
}

pub async fn resume_bridge(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<BridgeResumeRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    let reason = request.reason.trim();
    let operator = request.operator.trim();
    if reason.is_empty() || reason.len() > MAX_BRIDGE_REASON_LEN {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("reason must be non-empty and at most {MAX_BRIDGE_REASON_LEN} characters"),
        );
    }
    if operator.is_empty() || operator.len() > MAX_PARTICIPANT_ID_LEN {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("operator must be non-empty and at most {MAX_PARTICIPANT_ID_LEN} characters"),
        );
    }

    let now = chrono::Utc::now().timestamp();
    let status = {
        let mut manager = state.bridge_safety.write().await;
        match manager.resume(reason, operator, now) {
            Ok(()) => manager.get_status(now),
            Err(e) => return envelope_error(&req, StatusCode::BAD_REQUEST, "BAD_REQUEST", e.to_string()),
        }
    };
    if let Err(resp) = persist_bridge_audit_event(
        &req,
        &state,
        now,
        AuditEventKind::BridgeSafetyResume,
        None,
        Some(operator.to_string()),
        serde_json::json!({ "reason": reason, "operator": operator }),
    )
    .await
    {
        return resp;
    }
    HttpResponse::Ok().json(status)
}

pub async fn open_bridge_challenge(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<BridgeChallengeOpenRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    let tx_id = request.tx_id.trim();
    let chain_id = request.chain_id.trim();
    if tx_id.is_empty() || tx_id.len() > MAX_BRIDGE_TX_ID_LEN {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "tx_id is required and must be at most 128 characters",
        );
    }
    if chain_id.is_empty() || chain_id.len() > MAX_PARTICIPANT_ID_LEN {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "chain_id is required and must be at most 128 characters",
        );
    }

    let now = chrono::Utc::now().timestamp();
    let window = {
        let mut manager = state.bridge_safety.write().await;
        match manager.open_challenge(tx_id, chain_id, now) {
            Ok(()) => manager.challenge_windows.get_window(tx_id).cloned(),
            Err(e) => return envelope_error(&req, StatusCode::BAD_REQUEST, "BAD_REQUEST", e.to_string()),
        }
    };
    if let Err(resp) = persist_bridge_audit_event(
        &req,
        &state,
        now,
        AuditEventKind::BridgeChallengeOpened,
        Some(tx_id.to_string()),
        None,
        serde_json::json!({ "chain_id": chain_id }),
    )
    .await
    {
        return resp;
    }
    HttpResponse::Created().json(window)
}

pub async fn submit_bridge_challenge(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<BridgeChallengeSubmitRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }
    let tx_id = request.tx_id.trim();
    let challenger_id = request.challenger_id.trim();
    let reason = request.reason.trim();
    if tx_id.is_empty() || tx_id.len() > MAX_BRIDGE_TX_ID_LEN {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "tx_id is required and must be at most 128 characters",
        );
    }
    if challenger_id.is_empty() || challenger_id.len() > MAX_PARTICIPANT_ID_LEN {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "challenger_id is required",
        );
    }
    if reason.is_empty() || reason.len() > MAX_BRIDGE_REASON_LEN {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "reason is required and must be at most 512 characters",
        );
    }

    let mut manager = state.bridge_safety.write().await;
    let now = chrono::Utc::now().timestamp();
    match manager.submit_challenge(tx_id, challenger_id, reason, now) {
        Ok(()) => HttpResponse::Ok().json(serde_json::json!({ "accepted": true })),
        Err(e) => envelope_error(&req, StatusCode::BAD_REQUEST, "BAD_REQUEST", e.to_string()),
    }
}

pub async fn resolve_bridge_challenge(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<BridgeChallengeResolveRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    let tx_id = request.tx_id.trim();
    let note = request.note.trim();
    if tx_id.is_empty() || tx_id.len() > MAX_BRIDGE_TX_ID_LEN {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "tx_id is required and must be at most 128 characters",
        );
    }

    let now = chrono::Utc::now().timestamp();
    let outcome = {
        let mut manager = state.bridge_safety.write().await;
        match manager.resolve_challenge(tx_id, request.upheld, note, now) {
            Ok(outcome) => outcome,
            Err(e) => return envelope_error(&req, StatusCode::BAD_REQUEST, "BAD_REQUEST", e.to_string()),
        }
    };
    if let Err(resp) = persist_bridge_audit_event(
        &req,
        &state,
        now,
        AuditEventKind::BridgeChallengeResolved,
        Some(tx_id.to_string()),
        None,
        serde_json::json!({
            "upheld": request.upheld,
            "note": note,
            "outcome": outcome,
        }),
    )
    .await
    {
        return resp;
    }
    HttpResponse::Ok().json(serde_json::json!({ "outcome": outcome }))
}

pub async fn update_bridge_caps(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<BridgeCapsUpdateRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    if request.per_chain_cap == 0 || request.global_cap == 0 {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "per_chain_cap and global_cap must be greater than zero",
        );
    }
    let updated_by = request.updated_by.trim();
    if updated_by.is_empty() || updated_by.len() > MAX_PARTICIPANT_ID_LEN {
        return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY, "UNPROCESSABLE_ENTITY", "updated_by is required");
    }

    let now = chrono::Utc::now().timestamp();
    let status = {
        let mut manager = state.bridge_safety.write().await;
        match manager
            .exposure
            .update_caps(request.per_chain_cap, request.global_cap, updated_by, now)
        {
            Ok(()) => manager.exposure.get_status(),
            Err(e) => return envelope_error(&req, StatusCode::BAD_REQUEST, "BAD_REQUEST", e.to_string()),
        }
    };
    if let Err(resp) = persist_bridge_audit_event(
        &req,
        &state,
        now,
        AuditEventKind::BridgeCapsUpdated,
        None,
        Some(updated_by.to_string()),
        serde_json::json!({
            "per_chain_cap": request.per_chain_cap,
            "global_cap": request.global_cap,
            "updated_by": updated_by,
        }),
    )
    .await
    {
        return resp;
    }
    HttpResponse::Ok().json(status)
}

pub async fn get_bridge_exposure(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let manager = state.bridge_safety.read().await;
    HttpResponse::Ok().json(manager.exposure.get_status())
}

pub async fn get_bridge_audit_log(req: HttpRequest, state: web::Data<AppState>) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let manager = state.bridge_safety.read().await;
    let log = manager.get_audit_log();
    HttpResponse::Ok().json(log)
}

pub async fn scan_bridge_stuck(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<BridgeScanStuckRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    if request.transactions.len() > MAX_BRIDGE_SCAN_SIZE {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("transactions list exceeds max size {MAX_BRIDGE_SCAN_SIZE}"),
        );
    }

    let transactions: Vec<(String, String, i64)> = request
        .transactions
        .iter()
        .map(|e| (e.tx_id.clone(), e.status.clone(), e.timestamp))
        .collect();

    let mut manager = state.bridge_safety.write().await;
    let now = chrono::Utc::now().timestamp();
    let stuck = manager.scan_stuck(&transactions, now);
    HttpResponse::Ok().json(stuck)
}
