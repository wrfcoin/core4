//! Advisory governance node API (`governance::advisory`).
//!
//! - `POST /governance/advisory/submit` takes a signed
//!   `TransactionType::AdvisoryGovernance` transaction (built offline by
//!   `wrfcoin governance advisory propose|vote`), admits it to the mempool and
//!   gossips it. The node never fills in a sender or a nonce. Refused with 503
//!   `FEATURE_NOT_ACTIVE` until `advisory_governance` is active at tip + 1. The
//!   dry run here is advisory: consensus (apply) is the only authority, and a
//!   stateful refusal there is an inert drop at assembly.
//! - `GET /governance/advisory/proposals` lists proposals, newest first.
//! - `GET /governance/advisory/proposals/{id}` is one proposal.
//! - `GET /governance/advisory/proposals/{id}/eligibility?address=` says
//!   whether a wallet may vote on it and whether it already has.
//!
//! The legacy `/governance/proposals*` and `/governance/vote` routes never
//! see advisory records (they read a different map).

use crate::{
    helpers::{envelope_error, envelope_success, require_role},
    state::AppState,
};
use actix_web::{HttpRequest, Responder, http::StatusCode, web};
use serde::Deserialize;
use wrfcoin_native_chain::security::api_auth::ApiRole;
use wrfcoin_shared_protocol::{
    blockchain::transaction::{Transaction, TransactionType},
    common::types::Address,
    crypto::chain_binding::{ConsensusFeature, feature_active},
    governance::advisory::{self, AdvisoryProposalView, AdvisoryStatus},
};

/// Largest accepted JSON body (the payload itself is capped at 8 KiB).
pub const MAX_ADVISORY_JSON_BYTES: usize = 32 * 1024;
const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

fn limits() -> serde_json::Value {
    let identity = wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active();
    serde_json::json!({
        // What a signer must bind (`--chain-id` / `--network-id` in the CLI).
        "chain_id": identity.chain_id,
        "network_id": identity.network_id,
        "max_title_bytes": advisory::MAX_TITLE_BYTES,
        "max_summary_bytes": advisory::MAX_SUMMARY_BYTES,
        "deposit": advisory::ADVISORY_DEPOSIT.to_string(),
        "min_voter_spendable": advisory::MIN_VOTER_SPENDABLE.to_string(),
        "voting_period_blocks": advisory::ADVISORY_VOTING_PERIOD_BLOCKS,
        "quorum_min_votes": advisory::ADVISORY_QUORUM_MIN_VOTES,
        "approval_percent": advisory::ADVISORY_APPROVAL_PERCENT,
        "max_active_per_proposer": advisory::MAX_ACTIVE_PER_PROPOSER,
        "max_active_proposals": advisory::MAX_ACTIVE_PROPOSALS,
    })
}

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
    let next = tip.saturating_add(1);
    if !feature_active(ConsensusFeature::AdvisoryGovernance, next) {
        return envelope_error(
            &req,
            StatusCode::SERVICE_UNAVAILABLE,
            "FEATURE_NOT_ACTIVE",
            "advisory_governance is not active at the next height",
        );
    }
    if tx.tx_type != TransactionType::AdvisoryGovernance {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "ADVISORY_GOVERNANCE_PAYLOAD",
            "not an AdvisoryGovernance transaction",
        );
    }
    if state.mempool.read().await.check_transaction_admission_cost(&tx).is_err() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "ADVISORY_GOVERNANCE_REFUSED",
            "admission cost refused",
        );
    }
    // Advisory dry run: the exact consensus checks, read-only, at tip + 1, so
    // the signer hears about a stale nonce or a full slot now instead of
    // seeing a silent drop.
    let proposal_id = {
        let chain = state.blockchain.read().await;
        match chain.state.advisory_governance_check(&tx, next) {
            Ok(effect) => effect.proposal.id,
            Err(error) => {
                return envelope_error(
                    &req,
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "ADVISORY_GOVERNANCE_REFUSED",
                    error.to_string(),
                );
            },
        }
    };
    let id = tx.id.map(|id| id.to_hex());
    #[cfg(feature = "p2p")]
    let serialized = bincode::serialize(&tx).map_err(|error| error.to_string());
    if let Err(error) = state
        .mempool
        .write()
        .await
        .add_transaction_with_signing_keys_at_height(tx, None, Some(tip))
    {
        return envelope_error(&req, StatusCode::UNPROCESSABLE_ENTITY, "MEMPOOL_REJECTED", error.to_string());
    }
    #[cfg(feature = "p2p")]
    if let Some(ref p2p) = state.p2p {
        crate::helpers::queue_transaction_gossip(&p2p.command_tx, serialized, &state.tx_gossip_backpressure_streak);
    }
    // "Submitted" is not "counted": a client shows it as pending until the
    // proposal or vote appears in GET /governance/advisory/proposals/{id}.
    envelope_success(
        &req,
        StatusCode::ACCEPTED,
        serde_json::json!({"tx_id": id, "proposal_id": proposal_id, "admitted": true, "included": false}),
    )
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub status: Option<String>,
}

pub async fn list(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<ListQuery>,
) -> impl Responder {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_LIMIT {
        return envelope_error(
            &req,
            StatusCode::BAD_REQUEST,
            "INVALID_LIMIT",
            format!("limit must be 1..={MAX_LIMIT}"),
        );
    }
    let status_filter = match query.status.as_deref().map(str::trim) {
        None | Some("") | Some("all") => None,
        Some("active") => Some(AdvisoryStatus::Active),
        Some("passed") => Some(AdvisoryStatus::Passed),
        Some("rejected") => Some(AdvisoryStatus::Rejected),
        Some("failed_quorum") => Some(AdvisoryStatus::FailedQuorum),
        Some(_) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "INVALID_STATUS",
                "status must be active, passed, rejected, failed_quorum or all",
            );
        },
    };
    let offset = query.offset.unwrap_or(0);
    let chain = state.blockchain.read().await;
    let next = chain.height.saturating_add(1);
    let mut matching: Vec<_> = chain
        .state
        .advisory_proposals
        .values()
        .filter(|p| status_filter.is_none_or(|s| p.status == s))
        .collect();
    // Newest first; id breaks ties so pages are stable.
    matching.sort_by(|a, b| b.created_at_block.cmp(&a.created_at_block).then_with(|| a.id.cmp(&b.id)));
    let total = matching.len();
    let page: Vec<AdvisoryProposalView> =
        matching.into_iter().skip(offset).take(limit).map(AdvisoryProposalView::from_record).collect();
    let next_offset = (offset.saturating_add(page.len()) < total).then(|| offset + page.len());
    envelope_success(
        &req,
        StatusCode::OK,
        serde_json::json!({
            "feature_active_next_height": feature_active(ConsensusFeature::AdvisoryGovernance, next),
            "observed_at_height": chain.height,
            "total": total,
            "offset": offset,
            "limit": limit,
            "next_offset": next_offset,
            "proposals": page,
            "limits": limits(),
        }),
    )
}

pub async fn show(
    req: HttpRequest,
    state: web::Data<AppState>,
    id: web::Path<String>,
) -> impl Responder {
    let id = id.into_inner();
    if !advisory::is_advisory_proposal_id(id.trim()) {
        return envelope_error(&req, StatusCode::BAD_REQUEST, "INVALID_PROPOSAL_ID", "not an advisory proposal id");
    }
    let chain = state.blockchain.read().await;
    match chain.state.advisory_proposals.get(id.trim()) {
        Some(proposal) => envelope_success(
            &req,
            StatusCode::OK,
            serde_json::json!({
                "observed_at_height": chain.height,
                "proposal": AdvisoryProposalView::from_record(proposal),
                "limits": limits(),
            }),
        ),
        None => envelope_error(&req, StatusCode::NOT_FOUND, "PROPOSAL_NOT_FOUND", "no advisory proposal with this id"),
    }
}

#[derive(Debug, Deserialize)]
pub struct EligibilityQuery {
    pub address: String,
}

pub async fn eligibility(
    req: HttpRequest,
    state: web::Data<AppState>,
    id: web::Path<String>,
    query: web::Query<EligibilityQuery>,
) -> impl Responder {
    let id = id.into_inner();
    let Ok(address) = query.address.trim().parse::<Address>() else {
        return envelope_error(&req, StatusCode::BAD_REQUEST, "INVALID_ADDRESS", "address must be 40 hex characters");
    };
    let chain = state.blockchain.read().await;
    let Some(proposal) = chain.state.advisory_proposals.get(id.trim()) else {
        return envelope_error(&req, StatusCode::NOT_FOUND, "PROPOSAL_NOT_FOUND", "no advisory proposal with this id");
    };
    let next = chain.height.saturating_add(1);
    let open = proposal.status == AdvisoryStatus::Active && next < proposal.voting_deadline;
    let eligible = proposal.eligible_voters.contains(&address);
    let voted = proposal.votes.get(&address).copied();
    let reason = if !open {
        Some("voting has closed")
    } else if !eligible {
        Some("this wallet held under 1 WRF spendable when the proposal was created")
    } else if voted.is_some() {
        Some("this wallet already voted")
    } else {
        None
    };
    envelope_success(
        &req,
        StatusCode::OK,
        serde_json::json!({
            "proposal_id": proposal.id,
            "address": address.to_string(),
            "observed_at_height": chain.height,
            "voting_open": open,
            "eligible": eligible,
            "voted": voted.map(|approve| if approve { "for" } else { "against" }),
            "can_vote": reason.is_none(),
            "reason": reason,
            "next_nonce": chain.state.accounts.get(&address).map(|a| a.nonce),
        }),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn json_body_cap_covers_the_payload_cap() {
        assert!(super::MAX_ADVISORY_JSON_BYTES > wrfcoin_shared_protocol::governance::advisory::MAX_ADVISORY_PAYLOAD_BYTES);
    }
}
