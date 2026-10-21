//! Bonded service signing; no live operator automation or payment promise.
use crate::{
    helpers::{envelope_error, require_role},
    state::AppState,
};
use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::Deserialize;
use wrfcoin_native_chain::{
    node_service_archive::{InstalledNodeReceipt, endorse_installed, server_claim},
    security::api_auth::ApiRole,
};
use wrfcoin_shared_protocol::{
    consensus::{node_receipt_registry::claim_hash, node_share_contracts::BondedServiceClaim},
    crypto::chain_binding::ChainIdentity,
};

fn identity() -> Option<ChainIdentity> {
    let c =
        wrfcoin_shared_protocol::governance::schedule_extension::schedule_extension_authority()?
            .context();
    Some(ChainIdentity::new(c.chain_id, c.network_id))
}

pub async fn settlement(req: HttpRequest, state: web::Data<AppState>, epoch: web::Path<u64>) -> impl Responder {
    if let Err(response) = require_role(&req,&state,ApiRole::Reader) { return response; }
    let chain=state.blockchain.read().await;
    let Some(store)=&chain.state.node_settlement else {
        return envelope_error(&req,StatusCode::SERVICE_UNAVAILABLE,"NODE_SETTLEMENT_UNAVAILABLE","Node settlement records unavailable");
    };
    let epoch=epoch.into_inner();
    let Some(record)=store.epochs.get(&epoch) else {
        return HttpResponse::Ok().json(serde_json::json!({"epoch":epoch.to_string(),"status":"UNKNOWN",
            "scheduled_base_units":null,"reserved_base_units":null,"outstanding_base_units":null,
            "unissued_base_units":null,"payouts_base_units":null}));
    };
    use wrfcoin_shared_protocol::consensus::node_settlement::EpochStatus;
    let status=match record.status { EpochStatus::Open=>"OPEN",EpochStatus::Provisional=>"PROVISIONAL",EpochStatus::Paid=>"PAID",EpochStatus::Unissued=>"UNISSUED" };
    let outstanding=if matches!(record.status,EpochStatus::Open|EpochStatus::Provisional) {record.reserved} else {0};
    let payouts:std::collections::BTreeMap<_,_>=record.payouts.iter().map(|(id,amount)|(hex::encode(id),amount.to_string())).collect();
    let provisional:std::collections::BTreeMap<_,_>=record.provisional.iter().map(|(id,amount)|(hex::encode(id),amount.to_string())).collect();
    HttpResponse::Ok().json(serde_json::json!({"epoch":epoch.to_string(),"status":status,
        "scheduled_base_units":record.scheduled.to_string(),"reserved_base_units":record.reserved.to_string(),
        "outstanding_base_units":outstanding.to_string(),"unissued_base_units":record.unissued.to_string(),
        "maturity_height":record.maturity_height.to_string(),"payouts_base_units":payouts,"provisional_base_units":provisional}))
}
pub async fn ticket(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<InstalledNodeReceipt>,
) -> impl Responder {
    if let Err(response) = require_role(&req, &state, ApiRole::Reader) {
        return response;
    }
    let Some(signer) = state.snapshot_signing_key.as_ref() else {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_SERVER_SIGNER_UNAVAILABLE",
            "registered server signer unavailable",
        );
    };
    let Some(identity) = identity() else {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_SERVICE_INACTIVE",
            "Node service authority unavailable",
        );
    };
    // This local export check is separate from the consensus verifier.
    // determinism-ok: off-chain exported-cut availability budget, never consensus freshness.
    if !state
        .snapshot_service
        // determinism-ok: process-local served-cut retention, never consensus freshness.
        .matches_recent_receipt(&body.receipt.receipt, std::time::Instant::now())
    {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_MANIFEST_NOT_SERVED",
            "receipt does not match an exported cut",
        );
    }
    let chain = state.blockchain.read().await;
    match server_claim(
        &chain.state,
        &body,
        chain.height,
        &identity,
        signer.as_ref(),
        &*state.storage.read().await,
    )
    .and_then(|claim| Ok(hex::encode(claim.to_wire()?)))
    {
        Ok(claim_hex) => HttpResponse::Ok().json(serde_json::json!({ "claim_hex":claim_hex })),
        Err(_) => envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_TICKET_REFUSED",
            "bonded ticket refused",
        ),
    }
}
#[derive(Deserialize)]
pub struct EndorseRequest {
    pub claim_hex: String,
}
pub async fn endorse(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<EndorseRequest>,
) -> impl Responder {
    if let Err(response) = require_role(&req, &state, ApiRole::Operator) {
        return response;
    }
    let parsed = (|| {
        if body.claim_hex.len()
            > 2 * wrfcoin_shared_protocol::consensus::node_share_contracts::MAX_NODE_CLAIM_BYTES
        {
            return None;
        }
        BondedServiceClaim::from_wire(&hex::decode(&body.claim_hex).ok()?).ok()
    })();
    let Some(claim) = parsed else {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "NODE_CLAIM_BOUNDS",
            "malformed or oversized claim",
        );
    };
    let Some(signer) = state.snapshot_signing_key.as_ref() else {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_RECEIVER_SIGNER_UNAVAILABLE",
            "registered receiver signer unavailable",
        );
    };
    let Some(identity) = identity() else {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_SERVICE_INACTIVE",
            "Node service authority unavailable",
        );
    };
    let key = match BondedServiceClaim::receipt_hash(&claim.receipt) {
        Ok(key) => key,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "NODE_CLAIM_BOUNDS",
                "invalid receipt",
            );
        },
    };
    let saved = state
        .storage
        .read()
        .await
        .archived_node_receipt(&key)
        .ok()
        .flatten();
    let Some(installed) =
        saved.and_then(|bytes| serde_json::from_slice::<InstalledNodeReceipt>(&bytes).ok())
    else {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_RECEIPT_NOT_INSTALLED",
            "no durable successful-install receipt; dry runs cannot be endorsed",
        );
    };
    let chain = state.blockchain.read().await;
    let signed = endorse_installed(
        &chain.state,
        &installed,
        claim,
        chain.height,
        &identity,
        signer.as_ref(),
    );
    drop(chain);
    match signed {
        Ok(claim) => {
            let wire = match claim.to_wire() {
                Ok(wire) => wire,
                Err(_) => {
                    return envelope_error(
                        &req,
                        StatusCode::BAD_REQUEST,
                        "NODE_CLAIM_BOUNDS",
                        "claim exceeds bound",
                    );
                },
            };
            let digest = match claim_hash(&claim) {
                Ok(digest) => digest,
                Err(_) => {
                    return envelope_error(
                        &req,
                        StatusCode::BAD_REQUEST,
                        "NODE_CLAIM_BOUNDS",
                        "claim exceeds bound",
                    );
                },
            };
            if state
                .storage
                .read()
                .await
                .archive_node_receipt(&digest, &wire)
                .is_err()
            {
                return envelope_error(
                    &req,
                    StatusCode::SERVICE_UNAVAILABLE,
                    "NODE_ARCHIVE_UNAVAILABLE",
                    "signed claim could not be durably archived",
                );
            }
            HttpResponse::Ok().json(serde_json::json!({ "claim_hex":hex::encode(wire), "included":false, "receipt_id":hex::encode(digest) }))
        },
        Err(_) => envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_ENDORSEMENT_REFUSED",
            "bonded endorsement refused",
        ),
    }
}

pub async fn submit(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<wrfcoin_shared_protocol::blockchain::transaction::Transaction>,
) -> impl Responder {
    if let Err(response) = require_role(&req, &state, ApiRole::Operator) {
        return response;
    }
    let tx = body.into_inner();
    let Some(payload) =
        wrfcoin_shared_protocol::consensus::node_receipt_registry::NodeReceiptSubmission::parse(
            &tx,
        )
    else {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "NODE_SUBMISSION_BOUNDS",
            "malformed or oversized Node receipt submission",
        );
    };
    let chain = state.blockchain.read().await;
    let Some(identity) = identity() else {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_SERVICE_INACTIVE",
            "Node service inactive",
        );
    };
    let Some(registry) = chain.state.node_bond_registry.as_ref() else {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_SERVICE_INACTIVE",
            "Node service inactive",
        );
    };
    if payload
        .verify_authorization(&identity, &registry.genesis_hash)
        .is_err()
    {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_SUBMISSION_AUTHORIZATION",
            "owner authorization invalid",
        );
    }
    let Some(carrying_height) = chain.height.checked_add(1) else {
        return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY, "NODE_SUBMISSION_REFUSED", "no valid carrying height");
    };
    // A licensed owner must submit authentic claims before consuming durable
    // journal capacity. Inclusion and nonce execution remain the applier's job.
    for claim in &payload.batch.claims {
        let licensed = registry.registrations.get(&claim.ticket.server_id).is_some_and(|r|
            r.binding.owner_account == payload.authorization.sender
                && r.principal_units == wrfcoin_shared_protocol::consensus::node_bond_registry::NODE_BOND_UNITS);
        if !licensed || wrfcoin_shared_protocol::consensus::node_receipt_registry::verify_claim(&chain.state, claim, carrying_height, &identity).is_err() {
            return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY, "NODE_SUBMISSION_NOT_BONDED", "submission does not match funded registered service");
        }
    }
    let height = chain.height;
    drop(chain);
    // The ordinary common fee policy and bounded mempool remain authoritative.
    let id = tx.id.map(|hash| hash.to_hex());
    // Archive the exact retryable signed transaction independently of inclusion.
    let bytes = match serde_json::to_vec(&tx) {
        Ok(bytes) => bytes,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::SERVICE_UNAVAILABLE,
                "NODE_ARCHIVE_UNAVAILABLE",
                "submission archive unavailable",
            );
        },
    };
    let key = *wrfcoin_shared_protocol::crypto::hash::hash_bytes(&bytes).as_bytes();
    if state
        .storage
        .read()
        .await
        .archive_node_receipt(&key, &bytes)
        .is_err()
    {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "NODE_ARCHIVE_UNAVAILABLE",
            "submission archive failed; no admission attempted",
        );
    }
    // Serialize before admission; gossip only after the pool accepts it.
    #[cfg(feature = "p2p")]
    let serialized = bincode::serialize(&tx).map_err(|error| error.to_string());
    if state
        .mempool
        .write()
        .await
        .add_transaction_with_signing_keys_at_height(tx.clone(), None, Some(height))
        .is_err()
    {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "NODE_SUBMISSION_REFUSED",
            "Node submission refused by common admission policy",
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

/// Bounded experimental ingestion for the signed state plus required block
/// window. The ordinary snapshot endpoint retains its existing JSON budget.
pub const MAX_NODE_BONDED_SNAPSHOT_INGEST_BYTES: usize = 2 * 1024 * 1024;
/// Receiver-only JSON budget for `/network/snapshot-sync/receiver-ingest` (#3586).
/// Real testnet state exports as ~372 MiB of JSON (every block is state), so the
/// 2 MiB cap refuses ingest before any receipt can be signed. This larger budget
/// is applied only to the isolated loopback `NodeRole::ReceiptConsumer` process,
/// marked by [`ReceiptConsumerIngestLimit`]; every other role keeps the 2 MiB cap.
/// A process-local HTTP limit: no consensus or state-root input.
pub const MAX_NODE_RECEIVER_SNAPSHOT_INGEST_BYTES: usize = 1024 * 1024 * 1024;

/// App-data marker registered by `main` only when the process role is
/// `NodeRole::ReceiptConsumer`; it carries the JSON limit for receiver-ingest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiptConsumerIngestLimit(pub usize);
pub async fn ingest(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<super::snapshot::SnapshotSyncIngestRequest>,
) -> HttpResponse {
    if let Err(response) = require_role(&req, &state, ApiRole::Operator) {
        return response;
    }
    // A fresh receiver has no effective overlay yet. Read the activation
    // marker from the bounded incoming snapshot, then let the ordinary path
    // verify trusted signature, roots, canonical chain window and atomic install.
    // No local flag, guessed height or fabricated marker authorizes service.
    let decoded = body.entries.iter().map(|e| Some((hex::decode(&e.key_hex).ok()?, hex::decode(&e.value_hex).ok()?))).collect::<Option<Vec<_>>>();
    let imported = decoded.and_then(|entries| super::snapshot::verify_and_apply_snapshot(wrfcoin_shared_protocol::blockchain::state::State::new(), &body.signed_payload.payload, &entries).ok());
    let active = imported.as_ref().and_then(|s| s.state.node_bond_registry.as_ref())
        .is_some_and(|r| r.activation_height <= body.signed_payload.payload.height && r.block_height == body.signed_payload.payload.height);
    if !active {
        return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY, "NODE_SERVICE_INACTIVE", "incoming snapshot has no valid activated Node marker");
    }
    super::snapshot::ingest_snapshot_sync_payload(req.clone(), state, body)
        .await.respond_to(&req).map_into_boxed_body()
}
