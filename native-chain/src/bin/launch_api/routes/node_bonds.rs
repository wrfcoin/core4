//! Authenticated operator lifecycle admission and canonical public read models.
use crate::{
    helpers::{envelope_error, require_role},
    state::AppState,
};
use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_shared_protocol::{
    blockchain::transaction::{Transaction, TransactionType},
    consensus::node_bond_registry::NodeBondLifecycle,
};

pub async fn submit(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<Transaction>,
) -> impl Responder {
    if let Err(response) = require_role(&req, &state, ApiRole::Operator) {
        return response;
    }
    let tx = body.into_inner();
    // Refuse oversized/underpriced packets before parsing lifecycle approvals,
    // acquiring the chain lock, or entering any signature verifier.
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
            "NODE_BOND_REFUSED",
            "lifecycle admission refused",
        );
    }
    if tx.tx_type != TransactionType::NodeBondLifecycle || NodeBondLifecycle::parse(&tx).is_none() {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "NODE_BOND_PAYLOAD",
            "invalid lifecycle transaction",
        );
    }
    let chain = state.blockchain.read().await;
    let Some(registry) = chain.state.node_bond_registry.as_ref() else {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_BOND_UNAVAILABLE",
            "Node bond state unavailable",
        );
    };
    let Some(authority) =
        wrfcoin_shared_protocol::governance::schedule_extension::schedule_extension_authority()
    else {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_BOND_UNAVAILABLE",
            "Node bond authority unavailable",
        );
    };
    let c = authority.context();
    // The pool verifies the signed lifecycle exactly once against its immutable
    // authority. Require that authority to bind this canonical registry first.
    let authority_genesis: Option<[u8; 32]> = hex::decode(c.genesis_hash)
        .ok()
        .and_then(|bytes| bytes.try_into().ok());
    if authority_genesis != Some(registry.genesis_hash) {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_BOND_AUTHORIZATION",
            "lifecycle authorization refused",
        );
    }
    let height = chain.height;
    drop(chain);
    // Common mempool gate, fee policy and signatures remain authoritative.
    // Actual funded state transition, nonce execution and inclusion occur in the block.
    let id = tx.id.map(|id| id.to_hex());
    // Serialize before the tx moves into the pool; gossip only after admission.
    #[cfg(feature = "p2p")]
    let serialized = bincode::serialize(&tx).map_err(|error| error.to_string());
    if state
        .mempool
        .write()
        .await
        .add_transaction_with_signing_keys_at_height(tx, None, Some(height))
        .is_err()
    {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_BOND_REFUSED",
            "lifecycle admission refused",
        );
    }
    // Without this the tx is mined only if this node produces the next block (#3588).
    #[cfg(feature = "p2p")]
    if let Some(ref p2p) = state.p2p {
        crate::helpers::queue_transaction_gossip(
            &p2p.command_tx,
            serialized,
            &state.tx_gossip_backpressure_streak,
        );
    }
    HttpResponse::Accepted().json(serde_json::json!({"tx_id":id,"admitted":true,"included":false}))
}

fn decode_id(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    hex::decode(text).ok()?.try_into().ok()
}

pub async fn inspect(
    req: HttpRequest,
    state: web::Data<AppState>,
    id: web::Path<String>,
) -> impl Responder {
    if let Err(response) = require_role(&req, &state, ApiRole::Reader) {
        return response;
    }
    let Some(id) = decode_id(&id) else {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "NODE_BOND_ID",
            "invalid deployment identifier",
        );
    };
    let chain = state.blockchain.read().await;
    let Some(registry) = &chain.state.node_bond_registry else {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_BOND_UNAVAILABLE",
            "Node bond state unavailable",
        );
    };
    let registration = registry.registrations.get(&id);
    let escrow = registration.and_then(|r| chain.state.accounts.get(&r.binding.escrow_account));
    HttpResponse::Ok().json(serde_json::json!({"height":chain.height.to_string(),"activation_height":registry.activation_height.to_string(),
        "epoch":wrfcoin_shared_protocol::consensus::node_bond_registry::epoch_at(registry.activation_height,chain.height).map(|e|e.to_string()),
        "deployment_id":hex::encode(id),"status":if registration.is_some(){"REGISTERED"}else{"UNKNOWN"},
        "principal_base_units":registration.map(|r|r.principal_units.to_string()),
        "escrow_balance_base_units":escrow.map(|a|a.balance.0.to_string()),
        "escrow_locked_base_units":escrow.map(|a|a.stake_locked.0.to_string()),
        "escrow_spendable_base_units":escrow.map(|a|a.spendable_balance.0.to_string()),
        "jailed":chain.state.node_fraud.as_ref().map(|f|f.jailed.contains(&id)),"registration":registration}))
}

pub async fn receipt(
    req: HttpRequest,
    state: web::Data<AppState>,
    id: web::Path<String>,
) -> impl Responder {
    if let Err(response) = require_role(&req, &state, ApiRole::Reader) {
        return response;
    }
    let Some(id) = decode_id(&id) else {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "NODE_RECEIPT_ID",
            "invalid receipt identifier",
        );
    };
    let chain = state.blockchain.read().await;
    let Some(registry) = &chain.state.node_receipt_registry else {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_RECEIPT_UNAVAILABLE",
            "Node receipt state unavailable",
        );
    };
    let record = registry.accepted.values().find(|r| {
        wrfcoin_shared_protocol::consensus::node_receipt_registry::claim_hash(&r.claim).ok()
            == Some(id)
    });
    HttpResponse::Ok().json(serde_json::json!({"receipt_id":hex::encode(id),"height":chain.height.to_string(),
        "status":if record.is_some(){"INCLUDED"}else{"UNKNOWN"},"included":record.map(|_|true),"record":record}))
}
