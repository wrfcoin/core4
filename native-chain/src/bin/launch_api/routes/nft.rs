use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::Deserialize;

use crate::helpers::{envelope_error, envelope_success, live_network_mutation_rejected, require_role, validate_nft_id};
use crate::state::AppState;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::blockchain::state::{ComputationalNFT, StateTransition};
use wrfcoin_native_chain::common::types::{Address, Balance, NFTId, NFTMetadata};
use wrfcoin_native_chain::nft::execution::{WeatherModelMintInput, build_weather_model_output_nft};
use wrfcoin_native_chain::nft::verification::VerificationStatus;
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_native_chain::weather::validation::WeatherObservation;

fn nft_response(nft: &ComputationalNFT) -> serde_json::Value {
    serde_json::json!({
        "id": nft.id,
        "owner": nft.owner,
        "creator": nft.creator,
        "metadata": nft.metadata,
        "price": nft.price.map(|price| price.into_inner().to_string()),
        "verified": nft.verified,
        "verification_count": nft.verification_count,
        "avg_confidence": nft.avg_confidence,
        "created_at": nft.created_at,
        "updated_at": nft.updated_at,
    })
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MintNftRequest {
    pub id: String,
    pub owner: String,
    pub metadata: Option<serde_json::Value>,
    pub price: Option<u64>,
    pub weather_output: Option<WeatherMintPayload>,
}

#[derive(Debug, Deserialize)]
pub struct WeatherMintPayload {
    pub station_id: String,
    pub model_name: String,
    pub observed_at_unix: u64,
    pub observation: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct TransferNftRequest {
    pub to: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyNftRequest {
    pub verifier: String,
    pub confidence: f64,
    pub result: bool,
}

const NFT_LIST_DEFAULT_LIMIT: u32 = 25;
const NFT_LIST_MAX_LIMIT: u32 = 100;

/// Deterministic timestamp (unix seconds) to stamp onto NFTs at mutation time.
///
/// NFT `created_at` / `updated_at` are hashed into the consensus `state_root`, so
/// they must NOT be derived from the wall clock — the producer and any later
/// WAL replayer would otherwise read different times and fork the state root
/// (#1968). The chain tip timestamp is consensus-agreed and identical on every
/// node and on every replay, so it is the deterministic source. Falls back to 0
/// before any block exists (these mutation routes only run at height 0 anyway).
fn deterministic_nft_timestamp(blockchain: &Blockchain) -> u64 {
    blockchain
        .last_block()
        .map(|block| block.timestamp.timestamp().max(0) as u64)
        .unwrap_or(0)
}

#[derive(Debug, Deserialize)]
pub struct ListNftsQuery {
    pub owner: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

// ---------------------------------------------------------------------------
// Mutation gating (#1764)
// ---------------------------------------------------------------------------

/// Returned by every NFT-mutation handler when the local mutation API is
/// disabled (the default). See [`AppState::nft_mutation_api_enabled`].
///
/// NFT state is part of the consensus `state_root`, but NFT mutations are
/// applied only via these node-local, non-gossiped endpoints and are dropped
/// from block extraction. Allowing a mutation here forks this node's
/// `state_root` away from peers that never saw the call, tripping the P2P
/// divergence latch and wedging sync. Disabling the endpoints keeps the `nfts`
/// map empty and identical on every node.
fn nft_mutations_disabled(req: &HttpRequest) -> HttpResponse {
    envelope_error(
        req,
        StatusCode::SERVICE_UNAVAILABLE,
        "NFT_MUTATIONS_DISABLED",
        "NFT mutation endpoints are disabled on this node. NFT state is part of \
         the consensus state root but mutations are node-local and never \
         gossiped, so enabling them would fork this node away from its peers \
         (core4#1764). Set WRFCOIN_ENABLE_NFT_MUTATION_API=1 to re-enable on \
         single-node or trusted-operator deployments only.",
    )
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn mint_nft(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<MintNftRequest>,
) -> impl Responder {
    if !state.nft_mutation_api_enabled {
        return nft_mutations_disabled(&req);
    }
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let nft_id_str = body.id.trim();
    if let Err(resp) = validate_nft_id(&req, nft_id_str) {
        return resp;
    }

    let owner: Address = match body.owner.trim().parse() {
        Ok(addr) => addr,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "owner must be a valid hex address",
            );
        },
    };

    let price = body.price.map(|p| Balance::new(p as u128));

    let mut blockchain = state.blockchain.write().await;
    if blockchain.height > 0 {
        return live_network_mutation_rejected(&req, blockchain.height);
    }
    // Deterministic, block-derived timestamp (#1968): created_at/updated_at are
    // hashed into the state_root, so they must be identical for the producer and
    // every WAL replayer. Derived under the write lock from the chain tip.
    let now = deterministic_nft_timestamp(&blockchain);

    let nft = if let Some(weather_output) = &body.weather_output {
        let observation = WeatherObservation::from_json(&weather_output.observation);
        let Some(observation) = observation else {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "weather_output.observation is invalid",
            );
        };

        match build_weather_model_output_nft(WeatherModelMintInput {
            nft_id: nft_id_str,
            owner,
            creator: owner,
            station_id: weather_output.station_id.trim(),
            model_name: weather_output.model_name.trim(),
            observed_at_unix: weather_output.observed_at_unix,
            observation: &observation,
            price,
            created_at_unix: now,
        }) {
            Ok(nft) => nft,
            Err(err) => return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY, "UNPROCESSABLE_ENTITY", err.to_string()),
        }
    } else {
        let Some(metadata_value) = body.metadata.clone() else {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "metadata is required when weather_output is not provided",
            );
        };
        let metadata: NFTMetadata = match serde_json::from_value(metadata_value) {
            Ok(m) => m,
            Err(_) => {
                return envelope_error(
                    &req,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "UNPROCESSABLE_ENTITY",
                    "metadata is invalid or missing required fields",
                );
            },
        };

        ComputationalNFT {
            id: NFTId::new(nft_id_str.to_string()),
            owner,
            creator: owner,
            metadata,
            price,
            verified: false,
            verification_count: 0,
            avg_confidence: None,
            created_at: now,
            updated_at: now,
        }
    };

    if let Err(err) = blockchain.apply_state_transition(StateTransition::CreateNFT { nft }) {
        return envelope_error(&req, StatusCode::BAD_REQUEST, "BAD_REQUEST", format!("{err}"));
    }

    HttpResponse::Created().json(serde_json::json!({
        "status": "created",
        "nft_id": nft_id_str
    }))
}

pub async fn get_nft(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let nft_id_str = path.into_inner();
    if let Err(resp) = validate_nft_id(&req, &nft_id_str) {
        return resp;
    }

    let blockchain = state.blockchain.read().await;
    let nft_id = NFTId::new(nft_id_str.clone());
    match blockchain.state.get_nft(&nft_id) {
        Some(nft) => envelope_success(&req, StatusCode::OK, nft_response(nft)),
        None => envelope_error(&req, StatusCode::NOT_FOUND, "NOT_FOUND", "NFT not found"),
    }
}

pub async fn list_nfts(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<ListNftsQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let owner_filter: Option<Address> = match &query.owner {
        Some(owner_str) => match owner_str.trim().parse::<Address>() {
            Ok(addr) => Some(addr),
            Err(_) => {
                return envelope_error(
                    &req,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "UNPROCESSABLE_ENTITY",
                    "owner must be a valid hex address",
                );
            },
        },
        None => None,
    };

    let limit = query.limit.unwrap_or(NFT_LIST_DEFAULT_LIMIT);
    let offset = query.offset.unwrap_or(0);

    if limit > NFT_LIST_MAX_LIMIT {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            format!("limit exceeds maximum of {NFT_LIST_MAX_LIMIT}"),
        );
    }

    let blockchain = state.blockchain.read().await;
    let mut all_nfts: Vec<&ComputationalNFT> = blockchain
        .state
        .nfts
        .values()
        .filter(|nft| match &owner_filter {
            Some(addr) => nft.owner == *addr,
            None => true,
        })
        .collect();

    // Sort by NFT id for deterministic pagination.
    all_nfts.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

    let total = all_nfts.len();
    let items: Vec<serde_json::Value> = all_nfts
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(nft_response)
        .collect();

    HttpResponse::Ok().json(serde_json::json!({
        "items": items,
        "total": total,
        "limit": limit,
        "offset": offset,
    }))
}

pub async fn verify_nft(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
    body: web::Json<VerifyNftRequest>,
) -> impl Responder {
    if !state.nft_mutation_api_enabled {
        return nft_mutations_disabled(&req);
    }
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }

    let nft_id_str = path.into_inner();
    if let Err(resp) = validate_nft_id(&req, &nft_id_str) {
        return resp;
    }

    let verifier: Address = match body.verifier.trim().parse() {
        Ok(addr) => addr,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "verifier must be a valid hex address",
            );
        },
    };

    if !(0.0..=1.0).contains(&body.confidence) {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "confidence must be between 0.0 and 1.0",
        );
    }

    let nft_id = NFTId::new(nft_id_str.clone());
    let mut blockchain = state.blockchain.write().await;
    if blockchain.height > 0 {
        return live_network_mutation_rejected(&req, blockchain.height);
    }
    let updated_at = deterministic_nft_timestamp(&blockchain);
    if let Err(err) = blockchain.apply_state_transition(StateTransition::VerifyNFT {
        nft_id,
        verifier,
        confidence: body.confidence,
        result: body.result,
        updated_at,
    }) {
        return envelope_error(&req, StatusCode::BAD_REQUEST, "BAD_REQUEST", format!("{err}"));
    }

    let verification_status = if body.result {
        if body.confidence >= 0.7 {
            VerificationStatus::Verified
        } else {
            VerificationStatus::InProgress
        }
    } else {
        VerificationStatus::Rejected
    };

    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "nft_id": nft_id_str,
        "verification_status": verification_status
    }))
}

pub async fn transfer_nft(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
    body: web::Json<TransferNftRequest>,
) -> impl Responder {
    if !state.nft_mutation_api_enabled {
        return nft_mutations_disabled(&req);
    }
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let nft_id_str = path.into_inner();
    if let Err(resp) = validate_nft_id(&req, &nft_id_str) {
        return resp;
    }

    let to: Address = match body.to.trim().parse() {
        Ok(addr) => addr,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "to must be a valid hex address",
            );
        },
    };

    let nft_id = NFTId::new(nft_id_str.clone());
    let mut blockchain = state.blockchain.write().await;
    if blockchain.height > 0 {
        return live_network_mutation_rejected(&req, blockchain.height);
    }

    // Operator role carries authority over all NFT transfers; current owner is
    // read from state under the write lock so there is no TOCTOU race.
    let from = match blockchain.state.get_nft(&nft_id) {
        Some(nft) => nft.owner,
        None => return envelope_error(&req, StatusCode::NOT_FOUND, "NOT_FOUND", "NFT not found"),
    };

    let updated_at = deterministic_nft_timestamp(&blockchain);
    if let Err(err) = blockchain.apply_state_transition(StateTransition::TransferNFT {
        nft_id,
        from,
        to,
        updated_at,
    }) {
        return envelope_error(&req, StatusCode::BAD_REQUEST, "BAD_REQUEST", format!("{err}"));
    }

    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "nft_id": nft_id_str
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The default page limit must be 25 to match the documented API contract.
    #[test]
    fn nft_list_default_limit_is_25() {
        assert_eq!(NFT_LIST_DEFAULT_LIMIT, 25);
    }

    /// Verify the max-limit cap and that values above it are detectable.
    #[test]
    fn nft_list_max_limit_is_100_and_500_exceeds_it() {
        assert_eq!(NFT_LIST_MAX_LIMIT, 100);
        assert!(500 > NFT_LIST_MAX_LIMIT, "limit=500 must exceed the cap");
    }

    /// Verify the pagination window logic: skip(offset).take(limit) over a
    /// sorted vec produces the expected slice.
    #[test]
    fn nft_list_pagination_window_limit50_offset10() {
        let items: Vec<u32> = (0..80).collect();
        let limit: u32 = 50;
        let offset: u32 = 10;

        let page: Vec<u32> = items
            .iter()
            .copied()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();

        assert_eq!(page.len(), 50, "window should contain exactly 50 items");
        assert_eq!(page[0], 10, "first item should be at position offset=10");
        assert_eq!(page[49], 59, "last item should be at position offset+limit-1=59");
    }

    /// Negative and non-numeric limit values fail u32 parsing — actix-web's
    /// query extractor rejects these before the handler is called, returning
    /// HTTP 400. Verified here at the type level since the field is `Option<u32>`.
    #[test]
    fn list_nfts_query_limit_field_is_u32_rejects_negative_and_non_numeric() {
        // u32 cannot represent -1.
        assert!(
            "-1".parse::<u32>().is_err(),
            "limit=-1 must fail u32 parsing (u32 is unsigned)"
        );
        // u32 cannot represent a non-numeric string.
        assert!(
            "abc".parse::<u32>().is_err(),
            "limit=abc must fail u32 parsing (not a valid integer)"
        );
        // Sanity: valid values succeed.
        assert_eq!("50".parse::<u32>().unwrap(), 50u32);
    }
}
