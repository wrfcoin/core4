//! Security-key wallets (`security_key_spend`; design
//! `docs/design/security-key-wallets.md`).
//!
//! - `GET /accounts/{address}/security-key`: public read of the account's
//!   policy (keys are public handles), the next action nonce, a pending escape
//!   and whether the feature is active at the next block.
//! - `POST /accounts/security-key/actions`: a client-signed
//!   [`SecurityKeyActionPayload`]. The node dry-runs it against its own State at
//!   tip+1 (so the caller gets the exact refusal, not a silent drop at block
//!   assembly), then enqueues a `SecurityKeyAction` transaction.
//!
//! Non-forking: the route admits only what the mempool and the governance sink
//! already understand; consensus (`security_key::apply_action`) re-checks
//! everything at the inclusion height.

use std::collections::BTreeMap;

use actix_web::{HttpRequest, Responder, http::StatusCode, web};
use serde::Serialize;
use wrfcoin_native_chain::blockchain::transaction::TransactionType;
use wrfcoin_native_chain::common::types::Address;
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_shared_protocol::accounts::security_key::{
    PendingEscape, SecurityKeyActionPayload, SecurityKeyWire, apply_action, security_key_active,
    stored_policy,
};
use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;

use crate::helpers::{
    enforce_legacy_submit_rate_limit, enqueue_protocol_transaction_with_nonce, envelope_error,
    envelope_success, require_role,
};
use crate::state::AppState;

/// Largest action body the route parses (two assertions plus a key).
pub const MAX_SECURITY_KEY_ACTION_JSON_BYTES: usize = 32 * 1024;

#[derive(Debug, Serialize)]
pub struct PendingEscapeView {
    pub initiated_at_height: u64,
    pub executable_at_height: u64,
    pub blocks_remaining: u64,
}

#[derive(Debug, Serialize)]
pub struct SecurityKeyStatus {
    pub address: String,
    pub next_height: u64,
    pub feature_active_at_next_height: bool,
    /// The nonce the next action must carry.
    pub next_nonce: u64,
    pub generation: u64,
    /// Whether a send at the next height needs a security-key assertion.
    pub required_to_spend: bool,
    pub keys: Vec<SecurityKeyWire>,
    pub escape_delay_blocks: Option<u64>,
    pub pending_escape: Option<PendingEscapeView>,
    /// A pending escape that has run its full delay: the rule has lapsed.
    pub escape_matured: bool,
}

fn escape_view(escape: &PendingEscape, next_height: u64) -> PendingEscapeView {
    PendingEscapeView {
        initiated_at_height: escape.initiated_at_height,
        executable_at_height: escape.executable_at_height,
        blocks_remaining: escape.executable_at_height.saturating_sub(next_height),
    }
}

pub async fn get_security_key_status(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    let Ok(address) = path.trim().parse::<Address>() else {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "address must be a valid hex address",
        );
    };
    let chain = state.blockchain.read().await;
    let next_height = chain.height.saturating_add(1);
    let declaration = chain
        .state
        .authorization_declarations
        .get(&address)
        .cloned()
        .unwrap_or_default();
    drop(chain);
    let policy = stored_policy(&declaration);
    let status = SecurityKeyStatus {
        address: address.to_string(),
        next_height,
        feature_active_at_next_height: security_key_active(next_height),
        next_nonce: declaration.next_nonce,
        generation: declaration.generation,
        required_to_spend: policy.is_some_and(|p| p.enforced_at(next_height)),
        keys: policy
            .map(|p| p.keys.iter().map(|k| k.to_wire()).collect())
            .unwrap_or_default(),
        escape_delay_blocks: policy.map(|p| p.escape_delay_blocks),
        pending_escape: policy
            .and_then(|p| p.pending_escape.as_ref())
            .map(|e| escape_view(e, next_height)),
        escape_matured: policy.is_some_and(|p| p.escape_matured(next_height)),
    };
    envelope_success(&req, StatusCode::OK, status)
}

pub async fn submit_security_key_action(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<SecurityKeyActionPayload>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }
    let payload = body.into_inner();
    let Ok(account) = payload.account.trim().parse::<Address>() else {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "account must be a valid hex address",
        );
    };

    // Dry-run against this node's State at the height the action would be
    // mined at. Only the one account's declaration is cloned.
    let dry_run = {
        let chain = state.blockchain.read().await;
        let next_height = chain.height.saturating_add(1);
        if !security_key_active(next_height) {
            return envelope_error(
                &req,
                StatusCode::SERVICE_UNAVAILABLE,
                "FEATURE_NOT_ACTIVE",
                format!("security_key_spend is not active at the next block height {next_height}"),
            );
        }
        let mut scratch = BTreeMap::new();
        if let Some(existing) = chain.state.authorization_declarations.get(&account) {
            scratch.insert(account, existing.clone());
        }
        let identity = ChainIdentity::active();
        apply_action(
            &mut scratch,
            &payload,
            next_height,
            &identity.chain_id,
            &identity.network_id,
        )
    };
    if let Err(error) = dry_run {
        // Each refused attempt can cost an ed25519 and two P-256
        // verifications: charge it to the caller's failed-submit budget.
        if let Err(resp) = enforce_legacy_submit_rate_limit(&req, &state).await {
            return resp;
        }
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "SECURITY_KEY_ACTION_REJECTED",
            error.to_string(),
        );
    }

    let nonce = payload.nonce;
    let value = match serde_json::to_value(&payload) {
        Ok(value) => value,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                "action payload is not serializable",
            );
        },
    };
    match enqueue_protocol_transaction_with_nonce(
        state.clone(),
        TransactionType::SecurityKeyAction,
        &account.to_string(),
        value,
        nonce,
    )
    .await
    {
        Ok(accepted) => envelope_success(
            &req,
            StatusCode::ACCEPTED,
            serde_json::json!({
                "accepted": true,
                "tx_id": accepted.id,
                "tx_type": accepted.tx_type,
                "account": account.to_string(),
                "nonce": nonce,
            }),
        ),
        Err(err) => envelope_error(&req, StatusCode::BAD_REQUEST, "TRANSACTION_REJECTED", err),
    }
}
