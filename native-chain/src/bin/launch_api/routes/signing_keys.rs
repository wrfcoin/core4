use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::{Deserialize, Serialize};

use crate::helpers::{
    envelope_error, live_network_mutation_rejected, oracle_admin_api_disabled, parse_signing_role,
    require_role, signing_role_label, validate_participant_id, validate_timestamp,
};
use crate::state::AppState;
use wrfcoin_native_chain::blockchain::state::StateTransition;
use wrfcoin_native_chain::crypto::signature::PublicKey;
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_native_chain::security::key_registry::KeyEpochRecord;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct SigningKeyEpochRegisterRequest {
    pub identity: String,
    pub role: String,
    pub epoch: u64,
    pub public_key: String,
    pub activated_at: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SigningKeyEpochRevokeRequest {
    pub identity: String,
    pub role: String,
    pub epoch: u64,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SigningKeyMutationResponse {
    pub identity: String,
    pub role: String,
    pub epoch: u64,
    pub state_root_hex: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SigningKeyIdentityEpochs {
    pub identity: String,
    pub epochs: Vec<KeyEpochRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SigningKeyRoleSnapshotResponse {
    pub role: String,
    pub identities: Vec<SigningKeyIdentityEpochs>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn register_signing_key_epoch(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<SigningKeyEpochRegisterRequest>,
) -> impl Responder {
    // #1775: out-of-band RegisterSigningKeyEpoch mutation (folds Oracle-role
    // signing keys into the consensus state_root) — gated off by default.
    if !state.oracle_admin_api_enabled {
        return oracle_admin_api_disabled(&req);
    }
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    if let Err(resp) = validate_participant_id(&req, &request.identity, "identity") {
        return resp;
    }
    let role = match parse_signing_role(&request.role) {
        Some(role) => role,
        None => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "role must be one of: oracle, calibrator, bridge_operator, governance_operator, validator",
            );
        },
    };
    if PublicKey::from_hex(request.public_key.trim()).is_err() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "public_key must be a valid ed25519 public key hex string",
        );
    }
    let activated_at = request
        .activated_at
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    if let Err(resp) = validate_timestamp(&req, activated_at, "activated_at") {
        return resp;
    }

    let mut blockchain = state.blockchain.write().await;
    if blockchain.height > 0 {
        return live_network_mutation_rejected(&req, blockchain.height);
    }
    if let Err(err) = blockchain.apply_state_transition(StateTransition::RegisterSigningKeyEpoch {
        identity: request.identity.trim().to_string(),
        role,
        epoch: request.epoch,
        public_key: request.public_key.trim().to_string(),
        activated_at,
    }) {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            format!("Failed to register signing key epoch: {err}"),
        );
    }

    HttpResponse::Ok().json(SigningKeyMutationResponse {
        identity: request.identity.trim().to_string(),
        role: signing_role_label(role).to_string(),
        epoch: request.epoch,
        state_root_hex: hex::encode(blockchain.state.state_root.0),
    })
}

pub async fn revoke_signing_key_epoch(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<SigningKeyEpochRevokeRequest>,
) -> impl Responder {
    // #1775: out-of-band RevokeSigningKeyEpoch mutation — gated off by default.
    if !state.oracle_admin_api_enabled {
        return oracle_admin_api_disabled(&req);
    }
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }
    if let Err(resp) = validate_participant_id(&req, &request.identity, "identity") {
        return resp;
    }
    let role = match parse_signing_role(&request.role) {
        Some(role) => role,
        None => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "role must be one of: oracle, calibrator, bridge_operator, governance_operator, validator",
            );
        },
    };
    let revoked_at = request
        .revoked_at
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    if let Err(resp) = validate_timestamp(&req, revoked_at, "revoked_at") {
        return resp;
    }

    let mut blockchain = state.blockchain.write().await;
    if blockchain.height > 0 {
        return live_network_mutation_rejected(&req, blockchain.height);
    }
    if let Err(err) = blockchain.apply_state_transition(StateTransition::RevokeSigningKeyEpoch {
        identity: request.identity.trim().to_string(),
        role,
        epoch: request.epoch,
        revoked_at,
    }) {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            format!("Failed to revoke signing key epoch: {err}"),
        );
    }

    HttpResponse::Ok().json(SigningKeyMutationResponse {
        identity: request.identity.trim().to_string(),
        role: signing_role_label(role).to_string(),
        epoch: request.epoch,
        state_root_hex: hex::encode(blockchain.state.state_root.0),
    })
}

pub async fn get_signing_keys_by_role(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let raw_role = path.into_inner();
    let role = match parse_signing_role(&raw_role) {
        Some(role) => role,
        None => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "role must be one of: oracle, calibrator, bridge_operator, governance_operator, validator",
            );
        },
    };

    let blockchain = state.blockchain.read().await;
    let identities = blockchain
        .state
        .signing_keys
        .snapshot_for_role(role)
        .into_iter()
        .map(|(identity, epochs)| SigningKeyIdentityEpochs { identity, epochs })
        .collect::<Vec<_>>();

    HttpResponse::Ok().json(SigningKeyRoleSnapshotResponse {
        role: signing_role_label(role).to_string(),
        identities,
    })
}
