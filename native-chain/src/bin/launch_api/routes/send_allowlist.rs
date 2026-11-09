//! Send allowlist node API (`accounts::send_allowlist`).
//!
//! - `POST /accounts/send-allowlist/submit` takes a signed
//!   `TransactionType::SendAllowlist` transaction (built offline by
//!   `wrfcoin send-allowlist ...`), admits it to the mempool and gossips it.
//!   Refused with 503 `FEATURE_NOT_ACTIVE` until `send_allowlist` is active at
//!   tip + 1. The pre-checks here are advisory: consensus (apply) is the only
//!   authority, and a stateful refusal there is an inert drop at assembly.
//! - `GET /accounts/{address}/send-allowlist` is the read model wallets use to
//!   show the list, the pending additions (so the owner notices one they did
//!   not ask for), a pending turn-off, and the nonce the next action needs.

use crate::{
    helpers::{envelope_error, envelope_success, require_role},
    state::AppState,
};
use actix_web::{HttpRequest, Responder, http::StatusCode, web};
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_shared_protocol::{
    accounts::send_allowlist::{self, SignedSendAllowlistAction},
    blockchain::transaction::{Transaction, TransactionType},
    common::types::Address,
    crypto::chain_binding::{ChainIdentity, ConsensusFeature, feature_active},
};

/// Largest accepted JSON body (the payload itself is capped far lower).
pub const MAX_SEND_ALLOWLIST_JSON_BYTES: usize = 32 * 1024;

pub async fn submit(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<Transaction>,
) -> impl Responder {
    if let Err(response) = require_role(&req, &state, ApiRole::Contributor) {
        return response;
    }
    let tx = body.into_inner();
    let tip = state.blockchain.read().await.height;
    if !feature_active(ConsensusFeature::SendAllowlist, tip.saturating_add(1)) {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "FEATURE_NOT_ACTIVE",
            "send_allowlist is not active at the next height",
        );
    }
    if state
        .mempool
        .read()
        .await
        .check_transaction_admission_cost(&tx)
        .is_err()
    {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "SEND_ALLOWLIST_REFUSED",
            "admission cost refused",
        );
    }
    if tx.tx_type != TransactionType::SendAllowlist {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "SEND_ALLOWLIST_PAYLOAD",
            "not a SendAllowlist transaction",
        );
    }
    let Some(signed) = SignedSendAllowlistAction::parse(&tx) else {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "SEND_ALLOWLIST_PAYLOAD",
            "payload does not parse, or its account is not the sender",
        );
    };
    if let Err(error) = signed.verify(&ChainIdentity::active().chain_id) {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "SEND_ALLOWLIST_AUTHORIZATION",
            error.to_string(),
        );
    }
    // Advisory dry run against the tip state, so the owner hears about a stale
    // nonce or a cooldown now instead of seeing a silent drop.
    {
        let chain = state.blockchain.read().await;
        let mut scratch = chain.state.send_allowlists.clone();
        if let Err(error) =
            send_allowlist::apply(&mut scratch, &signed, tip.saturating_add(1), &ChainIdentity::active().chain_id)
        {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "SEND_ALLOWLIST_REFUSED",
                error.to_string(),
            );
        }
    }
    let id = tx.id.map(|id| id.to_hex());
    #[cfg(feature = "p2p")]
    let serialized = bincode::serialize(&tx).map_err(|error| error.to_string());
    if let Err(error) = state
        .mempool
        .write()
        .await
        .add_transaction_with_signing_keys_at_height(tx, None, Some(tip))
    {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "MEMPOOL_REJECTED",
            error.to_string(),
        );
    }
    // Without gossip the tx is mined only if this node produces the next block.
    #[cfg(feature = "p2p")]
    if let Some(ref p2p) = state.p2p {
        crate::helpers::queue_transaction_gossip(
            &p2p.command_tx,
            serialized,
            &state.tx_gossip_backpressure_streak,
        );
    }
    envelope_success(
        &req,
        StatusCode::ACCEPTED,
        serde_json::json!({"tx_id": id, "admitted": true, "included": false}),
    )
}

pub async fn show(
    req: HttpRequest,
    state: web::Data<AppState>,
    address: web::Path<String>,
) -> impl Responder {
    if let Err(response) = require_role(&req, &state, ApiRole::Reader) {
        return response;
    }
    let Ok(account) = address.trim().parse::<Address>() else {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "INVALID_ADDRESS",
            "address must be 40 hex characters",
        );
    };
    let chain = state.blockchain.read().await;
    let next = chain.height.saturating_add(1);
    let view = send_allowlist::view(&chain.state.send_allowlists, &account, next);
    envelope_success(
        &req,
        StatusCode::OK,
        serde_json::json!({
            "feature_active_next_height": feature_active(ConsensusFeature::SendAllowlist, next),
            "observed_at_height": chain.height,
            "allowlist": view,
            "limits": {
                "min_add_delay_blocks": send_allowlist::MIN_ADD_DELAY_BLOCKS,
                "default_add_delay_blocks": send_allowlist::DEFAULT_ADD_DELAY_BLOCKS,
                "max_add_delay_blocks": send_allowlist::MAX_ADD_DELAY_BLOCKS,
                "cancel_cooldown_blocks": send_allowlist::CANCEL_COOLDOWN_BLOCKS,
                "max_allowed_addresses": send_allowlist::MAX_ALLOWED_ADDRESSES,
                "max_pending_additions": send_allowlist::MAX_PENDING_ADDITIONS,
            },
        }),
    )
}

/// Advisory refusal for `POST /transactions/submit`: a transparent transfer
/// the sender's allowlist will refuse at apply. `None` when it may proceed.
pub fn transfer_refusal(state: &wrfcoin_shared_protocol::blockchain::state::State, from: &str, to: &str, height: u64) -> Option<String> {
    if state.send_allowlists.is_empty() {
        return None;
    }
    let from = from.parse::<Address>().ok()?;
    let to = to.parse::<Address>().ok()?;
    (!send_allowlist::permits(&state.send_allowlists, &from, &to, height))
        .then(|| send_allowlist::destination_refusal(&from, &to, height))
}
