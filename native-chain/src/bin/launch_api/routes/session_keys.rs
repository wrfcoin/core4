//! Session keys (`accounts::session_keys`): list an account's grants and
//! submit an owner-signed register / revoke.
//!
//! Spending with a session key goes through the ordinary transfer route
//! (`POST /transactions/submit` with `session_key_auth`), so it shares that
//! route's replay guard and rate limits. Nothing here signs for a user: the
//! submit route only admits an action the client already signed with the
//! account's primary key (and, for a registration, with the session key).
use actix_web::{HttpRequest, Responder, http::StatusCode, web};
use serde_json::json;

use crate::helpers::{
    enforce_legacy_submit_rate_limit, enqueue_protocol_transaction_with_nonce, envelope_error,
    envelope_success, require_role,
};
use crate::state::AppState;
use wrfcoin_native_chain::blockchain::transaction::TransactionType;
use wrfcoin_native_chain::common::types::Address;
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_shared_protocol::accounts::session_keys::{
    ANY_TARGET, SessionKeyAction, SessionKeyActionPayload, SessionKeyRecord,
    session_keys_active, verify_stateless,
};
use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;

/// Most targets echoed per record (the consensus maximum is 64).
const MAX_LISTED_TARGETS: usize = 64;

fn record_status(record: &SessionKeyRecord, height: u64) -> &'static str {
    if record.revoked {
        "revoked"
    } else if height < record.grant.valid_from {
        "not_yet_valid"
    } else if height > record.grant.valid_until {
        "expired"
    } else if record
        .grant
        .max_uses
        .is_some_and(|max| record.uses_so_far >= max)
    {
        "exhausted"
    } else {
        "active"
    }
}

/// Value still spendable in the window that `height` falls in.
fn window_remaining(record: &SessionKeyRecord, height: u64) -> Option<(u64, u128)> {
    let window = record.grant.scope.window?;
    let index = height
        .checked_sub(record.grant.valid_from)?
        .checked_div(window.blocks)?;
    let used = if index == record.window_index {
        record.window_value_used
    } else {
        0
    };
    let ends_at = record
        .grant
        .valid_from
        .saturating_add(index.saturating_add(1).saturating_mul(window.blocks))
        .saturating_sub(1);
    Some((ends_at, window.max_value.saturating_sub(used)))
}

fn record_json(session_key: &Address, record: &SessionKeyRecord, height: u64) -> serde_json::Value {
    let scope = &record.grant.scope;
    let any_target = scope.allowed_targets.contains(&ANY_TARGET);
    let targets: Vec<String> = scope
        .allowed_targets
        .iter()
        .filter(|t| **t != ANY_TARGET)
        .take(MAX_LISTED_TARGETS)
        .map(|t| t.to_string())
        .collect();
    let window = scope.window.map(|w| {
        let (ends_at, remaining) = window_remaining(record, height).unwrap_or((0, w.max_value));
        json!({
            "blocks": w.blocks,
            "max_value": w.max_value.to_string(),
            "remaining": remaining.to_string(),
            "window_ends_at_height": ends_at,
        })
    });
    json!({
        "session_key": session_key.to_string(),
        "public_key": record.grant.public_key.to_hex(),
        "registration_nonce": record.registration_nonce,
        "status": record_status(record, height),
        "allowed_methods": &scope.allowed_methods,
        "any_target": any_target,
        "allowed_targets": targets,
        "max_value_per_use": scope.max_value_per_use.to_string(),
        "max_total_value": scope.max_total_value.to_string(),
        "total_remaining": scope.max_total_value.saturating_sub(record.cumulative_value_used).to_string(),
        "window": window,
        "valid_from": record.grant.valid_from,
        "valid_until": record.grant.valid_until,
        "max_uses": record.grant.max_uses,
        "uses_so_far": record.uses_so_far,
        "cumulative_value_used": record.cumulative_value_used.to_string(),
        "revoked": record.revoked,
    })
}

/// `GET /accounts/{address}/session-keys` — the account's grants as of the
/// next block (the height a transfer submitted now would be checked at), plus
/// the registration nonce a new grant must sign.
pub async fn list_session_keys(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let Ok(owner) = path.into_inner().parse::<Address>() else {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "address must be a valid hex address",
        );
    };
    let chain = state.blockchain.read().await;
    let next_height = chain.height.saturating_add(1);
    let owner_state = chain.state.session_keys.get(&owner);
    let records: Vec<serde_json::Value> = owner_state
        .map(|s| {
            s.records
                .iter()
                .map(|(key, record)| record_json(key, record, next_height))
                .collect()
        })
        .unwrap_or_default();
    envelope_success(
        &req,
        StatusCode::OK,
        json!({
            "owner": owner.to_string(),
            "feature_active": session_keys_active(next_height),
            "next_height": next_height,
            "next_registration_nonce": owner_state.map_or(0, |s| s.next_nonce),
            "chain_id": ChainIdentity::active().chain_id,
            "session_keys": records,
        }),
    )
}

/// `POST /session-keys/submit` — admit a client-signed
/// [`SessionKeyActionPayload`] (`Register` or `Revoke`) as a
/// `SessionKeyAction` transaction. Refused while the feature is not active at
/// the next height, and when the action's state-free checks fail (the same
/// `verify_stateless` the mempool and block validation run).
pub async fn submit_session_key_action(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<SessionKeyActionPayload>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }
    let payload = body.into_inner();
    let (owner, nonce) = match &payload.action {
        SessionKeyAction::Register {
            owner,
            registration_nonce,
            ..
        } => (*owner, *registration_nonce),
        SessionKeyAction::Revoke { owner, .. } => (*owner, 0),
        SessionKeyAction::Use { .. } => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                "only register and revoke can be submitted; spend with POST /transactions/submit and session_key_auth",
            );
        },
    };
    let next_height = state.blockchain.read().await.height.saturating_add(1);
    if !session_keys_active(next_height) {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "FEATURE_NOT_ACTIVE",
            format!("session_keys is not active at the next block height {next_height}"),
        );
    }
    let chain_id = &ChainIdentity::active().chain_id;
    if let Err(err) = verify_stateless(&payload.action, chain_id) {
        // Each failed attempt costs signature verification: charge the
        // caller's budget, like a failed transfer authorization (#3258 R1).
        if let Err(resp) = enforce_legacy_submit_rate_limit(&req, &state).await {
            return resp;
        }
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "AUTHORIZATION_INVALID",
            format!("session key action does not verify: {err}"),
        );
    }
    let payload_value = match serde_json::to_value(&payload) {
        Ok(value) => value,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "could not encode the action",
            );
        },
    };
    match enqueue_protocol_transaction_with_nonce(
        state.clone(),
        TransactionType::SessionKeyAction,
        &owner.to_string(),
        payload_value,
        nonce,
    )
    .await
    {
        Ok(accepted) => envelope_success(
            &req,
            StatusCode::ACCEPTED,
            json!({
                "accepted": true,
                "tx_id": accepted.id,
                "tx_type": accepted.tx_type,
                "owner": owner.to_string(),
            }),
        ),
        Err(err) => envelope_error(&req, StatusCode::BAD_REQUEST, "TRANSACTION_REJECTED", err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::accounts::session_keys::{
        MethodId, SessionKeyGrant, SessionScope, SpendWindow,
    };
    use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

    fn record(window: Option<SpendWindow>) -> SessionKeyRecord {
        let sk = PrivateKey::generate(&mut rand::thread_rng());
        SessionKeyRecord {
            owner: Address([1; 20]),
            registration_nonce: 0,
            grant: SessionKeyGrant {
                public_key: PublicKey::from_private_key(&sk),
                scope: SessionScope {
                    allowed_methods: vec![MethodId("transfer".into())],
                    allowed_targets: vec![ANY_TARGET],
                    max_value_per_use: 10,
                    max_total_value: 100,
                    window,
                },
                valid_from: 100,
                valid_until: 10_000,
                max_uses: None,
            },
            uses_so_far: 0,
            cumulative_value_used: 0,
            revoked: false,
            window_index: 0,
            window_value_used: 7,
        }
    }

    #[test]
    fn window_remaining_resets_in_a_later_window() {
        let r = record(Some(SpendWindow { blocks: 50, max_value: 10 }));
        assert_eq!(window_remaining(&r, 120), Some((149, 3)));
        assert_eq!(window_remaining(&r, 150), Some((199, 10)));
        assert_eq!(window_remaining(&r, 99), None, "before valid_from");
        assert_eq!(window_remaining(&record(None), 120), None);
    }

    #[test]
    fn status_reflects_height_and_revocation() {
        let mut r = record(None);
        assert_eq!(record_status(&r, 99), "not_yet_valid");
        assert_eq!(record_status(&r, 100), "active");
        assert_eq!(record_status(&r, 10_001), "expired");
        r.revoked = true;
        assert_eq!(record_status(&r, 100), "revoked");
    }

    #[test]
    fn listing_hides_the_any_target_sentinel() {
        let r = record(None);
        let v = record_json(&Address([2; 20]), &r, 120);
        assert_eq!(v["any_target"], true);
        assert_eq!(v["allowed_targets"].as_array().map(Vec::len), Some(0));
        assert_eq!(v["total_remaining"], "100");
    }
}
