use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::helpers::{envelope_error, envelope_success, law_book_error, law_book_ok, require_role, submit_protocol_transaction};
use super::query_cost;
use wrfcoin_native_chain::security::query_cost::{CostMeter, Cursor, QueryCost, page_size};
use crate::state::{AppState, DEFAULT_LAW_BOOK_PAGE_SIZE, MAX_LAW_BOOK_PAGE_SIZE};
use wrfcoin_native_chain::blockchain::state::{
    GovernanceProposalStatus, LawEnforcementLevel, LawRecord, LawStatus,
};
use wrfcoin_native_chain::blockchain::transaction::{
    GovernanceDelegatePayload, GovernanceExecutePayload, GovernanceFinalizePayload,
    GovernanceProposalPayload, GovernanceVotePayload, TransactionType,
    TreasuryDepositPayload, TreasuryWithdrawPayload,
};
use wrfcoin_native_chain::governance::{
    GovernedParameter, ParameterUpdateProposal, build_quality_parameter_preview,
    build_trust_weight_parameter_preview,
};
use wrfcoin_native_chain::security::api_auth::ApiRole;

/// Submit a governance proposal transaction.
///
/// The proposer identity is read from the `X-Proposer-Address` header when
/// present, otherwise falls back to `"admin"`.  The header-based address
/// must correspond to a funded on-chain account that can pay the deposit.
pub async fn submit_governance_proposal(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<GovernanceProposalPayload>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let proposer = req
        .headers()
        .get("X-Proposer-Address")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("admin");

    let payload = body.into_inner();

    submit_protocol_transaction(
        &req,
        state,
        TransactionType::GovernanceProposal,
        proposer,
        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    )
    .await
}

/// Submit a governance vote transaction.
///
/// The voter identity is read from the `X-Voter-Address` header when present,
/// otherwise falls back to `"contributor"`.
pub async fn submit_governance_vote(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<GovernanceVotePayload>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }

    let voter = req
        .headers()
        .get("X-Voter-Address")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("contributor");

    let payload = body.into_inner();

    submit_protocol_transaction(
        &req,
        state,
        TransactionType::GovernanceVote,
        voter,
        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    )
    .await
}

const DEFAULT_PROPOSALS_LIMIT: usize = 100;
const MAX_PROPOSALS_LIMIT: usize = 500;

const DEFAULT_TREASURY_TX_PAGE_SIZE: usize = 50;
const MAX_TREASURY_TX_PAGE_SIZE: usize = 500;

#[derive(Debug, Deserialize)]
pub struct TreasuryTxQuery {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    /// #2137: opaque continuation token from a previous page's `next_cursor`.
    /// Exclusive with `offset`.
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProposalListQuery {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    /// #2137: opaque continuation token from a previous page's `next_cursor`.
    /// Exclusive with `offset`.
    pub cursor: Option<String>,
}

/// #2137: the node's query limits with this route's own page ceiling folded in.
fn route_cost(route_max_page: usize) -> QueryCost {
    let node = *query_cost::query_cost();
    QueryCost {
        max_page_size: node.max_page_size.min(route_max_page as u32),
        ..node
    }
}

/// #2137: resolve `offset`/`cursor` to a start position. A cursor is the
/// position the previous page ended at, so page *k* costs the same as page 1;
/// `offset` is kept for existing callers and refused alongside a cursor.
fn start_position(req: &HttpRequest, offset: Option<usize>, cursor: Option<&str>) -> Result<usize, HttpResponse> {
    match (cursor, offset) {
        (Some(token), None) => Cursor::decode(token)
            .map(|c| usize::try_from(c.0).unwrap_or(usize::MAX))
            .map_err(|e| query_cost::cursor_rejected(req, e)),
        (Some(_), Some(_)) => Err(envelope_error(
            req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "offset and cursor are exclusive; continue with the cursor alone",
        )),
        (None, offset) => Ok(offset.unwrap_or(0)),
    }
}

fn next_cursor(start: usize, page_len: usize, total: usize) -> Option<String> {
    let end = start.saturating_add(page_len);
    (end < total).then(|| Cursor(end as u64).encode())
}

/// List governance proposals with optional `?offset=N&limit=N` pagination.
/// Default limit is 100; maximum is 500.  Returns a JSON envelope
/// `{total, offset, limit, proposals}` so callers can detect truncation.
pub async fn list_governance_proposals(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<ProposalListQuery>,
) -> impl Responder {
    // #2137: a page above the ceiling is refused, not clamped; a cursor
    // continues where the last page ended; the scan is metered.
    let cost = route_cost(MAX_PROPOSALS_LIMIT);
    let limit = match page_size(query.limit.map(|l| l.min(u32::MAX as usize) as u32), DEFAULT_PROPOSALS_LIMIT as u32, &cost) {
        Ok(l) => l as usize,
        Err(e) => return query_cost::page_rejected(&req, e),
    };
    let offset = match start_position(&req, query.offset, query.cursor.as_deref()) {
        Ok(o) => o,
        Err(resp) => return resp,
    };
    let mut meter = CostMeter::new(&cost);

    let blockchain = state.blockchain.read().await;
    let total = blockchain.state.governance.proposals.len();
    // Ordering by id examines every proposal once; charge that before doing it.
    if let Err(e) = meter.charge(total as u64) {
        return query_cost::cost_exceeded(&req, e);
    }
    let mut proposals: Vec<_> = blockchain
        .state
        .governance
        .proposals
        .values()
        .collect();
    // Sort by stable key so offset/limit pagination is deterministic across requests.
    proposals.sort_by(|a, b| a.id.cmp(&b.id));
    let proposals: Vec<_> = proposals.into_iter().skip(offset).take(limit).collect();
    let next_cursor = next_cursor(offset, proposals.len(), total);

    HttpResponse::Ok().json(serde_json::json!({
        "total": total,
        "offset": offset,
        "limit": limit,
        "next_cursor": next_cursor,
        "proposals": proposals,
    }))
}

/// Get a specific governance proposal by ID.
pub async fn get_governance_proposal(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    let proposal_id = path.into_inner();
    let blockchain = state.blockchain.read().await;
    match blockchain.state.governance.proposals.get(&proposal_id) {
        Some(proposal) => HttpResponse::Ok().json(proposal),
        None => envelope_error(&req, StatusCode::NOT_FOUND, "NOT_FOUND", "Proposal not found"),
    }
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct QualityGovernanceProposalBuildRequest {
    pub target: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct TrustGovernanceProposalBuildRequest {
    pub target: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QualityGovernanceProposalBuildResponse {
    pub proposal_payload: GovernanceProposalPayload,
    pub preview: wrfcoin_native_chain::governance::QualityProposalPreview,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustGovernanceProposalBuildResponse {
    pub proposal_payload: GovernanceProposalPayload,
    pub preview: wrfcoin_native_chain::governance::TrustProposalPreview,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustWeightsResponse {
    pub current_policy: wrfcoin_native_chain::blockchain::state::TrustWeightingPolicy,
    pub bounds: wrfcoin_native_chain::governance::TrustWeightBoundsPreview,
    pub ordering_hints: Vec<wrfcoin_native_chain::governance::TrustWeightOrderingHint>,
}

#[derive(Debug, Serialize)]
pub struct LawAmendmentApiRecord {
    pub proposal_id: String,
    pub updated_at_block: u64,
    pub summary: String,
    pub version: u32,
}

#[derive(Debug, Serialize)]
pub struct LawHistoricalVersionApiRecord {
    pub version: u32,
    pub proposal_id: String,
    pub updated_at_block: u64,
    pub summary: String,
    pub plain_language_change: String,
}

#[derive(Debug, Serialize)]
pub struct LawPlainLanguageRendering {
    pub headline: String,
    pub rule: String,
    pub enforcement: String,
    pub status: String,
    pub version_note: String,
}

#[derive(Debug, Serialize)]
pub struct LawApiRecord {
    pub law_id: String,
    pub title: String,
    pub category: String,
    pub plain_language_summary: String,
    pub plain_language_rendering: LawPlainLanguageRendering,
    pub enforcement_level: &'static str,
    pub status: &'static str,
    pub version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amendments: Option<Vec<LawAmendmentApiRecord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub historical_versions: Option<Vec<LawHistoricalVersionApiRecord>>,
}

#[derive(Debug, Serialize)]
pub struct LawSearchMetadata {
    pub query: String,
    pub tokens: Vec<String>,
    pub indexed_fields: Vec<String>,
    pub matched_law_count: usize,
}

#[derive(Debug, Serialize)]
pub struct LawListApiData {
    pub items: Vec<LawApiRecord>,
    pub next_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<LawSearchMetadata>,
}

#[derive(Debug, Deserialize)]
pub struct LawListQuery {
    pub status: Option<String>,
    pub enforcement: Option<String>,
    pub category: Option<String>,
    pub q: Option<String>,
    pub include_amendments: Option<bool>,
    pub include_history: Option<bool>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LawGetQuery {
    pub include_amendments: Option<bool>,
    pub include_history: Option<bool>,
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn law_status_name(value: LawStatus) -> &'static str {
    match value {
        LawStatus::Draft => "draft",
        LawStatus::Active => "active",
        LawStatus::Deprecated => "deprecated",
    }
}

fn law_enforcement_name(value: LawEnforcementLevel) -> &'static str {
    match value {
        LawEnforcementLevel::Advisory => "advisory",
        LawEnforcementLevel::Api => "api",
        LawEnforcementLevel::Consensus => "consensus",
    }
}

fn parse_law_status_filter(value: &str) -> Option<LawStatus> {
    match value.trim().to_ascii_lowercase().as_str() {
        "draft" => Some(LawStatus::Draft),
        "active" => Some(LawStatus::Active),
        "deprecated" => Some(LawStatus::Deprecated),
        _ => None,
    }
}

fn parse_law_enforcement_filter(value: &str) -> Option<LawEnforcementLevel> {
    match value.trim().to_ascii_lowercase().as_str() {
        "advisory" => Some(LawEnforcementLevel::Advisory),
        "api" => Some(LawEnforcementLevel::Api),
        "consensus" => Some(LawEnforcementLevel::Consensus),
        _ => None,
    }
}

fn validate_law_id_for_api(law_id: &str) -> Result<(), &'static str> {
    if law_id.trim() != law_id {
        return Err("law_id must not have leading/trailing whitespace");
    }
    if !law_id.starts_with("LAW-") {
        return Err("law_id must start with 'LAW-'");
    }
    if !(8..=80).contains(&law_id.len()) {
        return Err("law_id length must be between 8 and 80 characters");
    }
    if !law_id
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
    {
        return Err("law_id must use uppercase letters, digits, and '-' only");
    }
    Ok(())
}

fn law_rendering_for_enforcement(level: LawEnforcementLevel) -> &'static str {
    match level {
        LawEnforcementLevel::Advisory => {
            "Advisory only. This guides behavior but does not block transactions."
        },
        LawEnforcementLevel::Api => {
            "API enforced. Requests can be rejected at API validation time."
        },
        LawEnforcementLevel::Consensus => {
            "Consensus enforced. Invalid transactions are rejected during state transition."
        },
    }
}

fn law_rendering_for_status(status: LawStatus) -> &'static str {
    match status {
        LawStatus::Draft => "Draft. The law is recorded but not active.",
        LawStatus::Active => "Active. The law currently applies.",
        LawStatus::Deprecated => {
            "Deprecated. The law is retained for history and should not be used for new policy."
        },
    }
}

fn render_law_plain_language(record: &LawRecord) -> LawPlainLanguageRendering {
    LawPlainLanguageRendering {
        headline: format!("{}: {}", record.law_id, record.title),
        rule: format!("Rule: {}", record.plain_language_summary.trim()),
        enforcement: format!(
            "Enforcement: {}",
            law_rendering_for_enforcement(record.enforcement_level)
        ),
        status: format!("Status: {}", law_rendering_for_status(record.status)),
        version_note: format!("Version {}.", record.version),
    }
}

fn render_historical_versions(record: &LawRecord) -> Vec<LawHistoricalVersionApiRecord> {
    record
        .amendments
        .iter()
        .map(|entry| {
            let plain_language_change = match entry.summary.as_str() {
                "law created" => format!(
                    "Version {} created by proposal '{}' at block {}.",
                    entry.version, entry.proposal_id, entry.updated_at_block
                ),
                "summary amended" => format!(
                    "Version {} updated the plain-language summary via proposal '{}' at block {}.",
                    entry.version, entry.proposal_id, entry.updated_at_block
                ),
                "status updated" => format!(
                    "Version {} changed law status via proposal '{}' at block {}.",
                    entry.version, entry.proposal_id, entry.updated_at_block
                ),
                "enforcement level updated" => format!(
                    "Version {} changed enforcement level via proposal '{}' at block {}.",
                    entry.version, entry.proposal_id, entry.updated_at_block
                ),
                _ => format!(
                    "Version {} recorded '{}' via proposal '{}' at block {}.",
                    entry.version, entry.summary, entry.proposal_id, entry.updated_at_block
                ),
            };
            LawHistoricalVersionApiRecord {
                version: entry.version,
                proposal_id: entry.proposal_id.clone(),
                updated_at_block: entry.updated_at_block,
                summary: entry.summary.clone(),
                plain_language_change,
            }
        })
        .collect()
}

fn law_search_tokens(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| token.len() >= 2)
        .collect()
}

fn searchable_law_material(record: &LawRecord) -> String {
    let amendment_summaries = record
        .amendments
        .iter()
        .map(|entry| entry.summary.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        "{} {} {} {} {}",
        record.law_id,
        record.title,
        record.category,
        record.plain_language_summary,
        amendment_summaries
    )
}

fn build_law_search_index(laws: &[&LawRecord]) -> FxHashMap<String, BTreeSet<String>> {
    let mut index: FxHashMap<String, BTreeSet<String>> = FxHashMap::default();
    for law in laws {
        let searchable = searchable_law_material(law);
        for token in law_search_tokens(&searchable) {
            index.entry(token).or_default().insert(law.law_id.clone());
        }
    }
    index
}

fn law_record_to_api(
    record: &LawRecord,
    include_amendments: bool,
    include_history: bool,
) -> LawApiRecord {
    let amendments = include_amendments.then(|| {
        record
            .amendments
            .iter()
            .map(|entry| LawAmendmentApiRecord {
                proposal_id: entry.proposal_id.clone(),
                updated_at_block: entry.updated_at_block,
                summary: entry.summary.clone(),
                version: entry.version,
            })
            .collect()
    });
    let historical_versions = include_history.then(|| render_historical_versions(record));

    LawApiRecord {
        law_id: record.law_id.clone(),
        title: record.title.clone(),
        category: record.category.clone(),
        plain_language_summary: record.plain_language_summary.clone(),
        plain_language_rendering: render_law_plain_language(record),
        enforcement_level: law_enforcement_name(record.enforcement_level),
        status: law_status_name(record.status),
        version: record.version,
        amendments,
        historical_versions,
    }
}

fn parse_quality_governance_target(req: &HttpRequest, target: &str) -> Result<GovernedParameter, HttpResponse> {
    match target.trim().to_ascii_lowercase().as_str() {
        "high_threshold" | "quality.high_threshold" => Ok(GovernedParameter::QualityHighThreshold),
        "acceptance_threshold" | "quality.acceptance_threshold" => {
            Ok(GovernedParameter::QualityAcceptanceThreshold)
        },
        "warning_threshold" | "quality.warning_threshold" => {
            Ok(GovernedParameter::QualityWarningThreshold)
        },
        "borderline_scoring_multiplier" | "quality.borderline_scoring_multiplier" => {
            Ok(GovernedParameter::QualityBorderlineScoringMultiplier)
        },
        "borderline_reward_multiplier" | "quality.borderline_reward_multiplier" => {
            Ok(GovernedParameter::QualityBorderlineRewardMultiplier)
        },
        "warning_scoring_multiplier" | "quality.warning_scoring_multiplier" => {
            Ok(GovernedParameter::QualityWarningScoringMultiplier)
        },
        "warning_reward_multiplier" | "quality.warning_reward_multiplier" => {
            Ok(GovernedParameter::QualityWarningRewardMultiplier)
        },
        "low_scoring_multiplier" | "quality.low_scoring_multiplier" => {
            Ok(GovernedParameter::QualityLowScoringMultiplier)
        },
        "low_reward_multiplier" | "quality.low_reward_multiplier" => {
            Ok(GovernedParameter::QualityLowRewardMultiplier)
        },
        "low_slash_streak" | "quality.low_slash_streak" => {
            Ok(GovernedParameter::QualityLowSlashStreak)
        },
        "low_slash_amount" | "quality.low_slash_amount" => {
            Ok(GovernedParameter::QualityLowSlashAmount)
        },
        _ => Err(envelope_error(
            req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "target must be a supported quality parameter key (e.g. high_threshold, warning_threshold, low_slash_amount)",
        )),
    }
}

fn parse_trust_governance_target(req: &HttpRequest, target: &str) -> Result<GovernedParameter, HttpResponse> {
    match target.trim().to_ascii_lowercase().as_str() {
        "unverified_weight" | "trust.unverified_weight" => {
            Ok(GovernedParameter::TrustUnverifiedWeight)
        },
        "verified_station_weight" | "trust.verified_station_weight" => {
            Ok(GovernedParameter::TrustVerifiedStationWeight)
        },
        "trusted_provider_weight" | "trust.trusted_provider_weight" => {
            Ok(GovernedParameter::TrustTrustedProviderWeight)
        },
        _ => Err(envelope_error(
            req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "target must be one of: unverified_weight, verified_station_weight, trusted_provider_weight",
        )),
    }
}

fn normalize_quality_governance_value(
    req: &HttpRequest,
    value: &serde_json::Value,
    target: &GovernedParameter,
) -> Result<String, HttpResponse> {
    match target {
        GovernedParameter::QualityLowSlashStreak | GovernedParameter::QualityLowSlashAmount => {
            if let Some(as_u64) = value.as_u64() {
                return Ok(as_u64.to_string());
            }
            if let Some(text) = value.as_str() {
                return Ok(text.trim().to_string());
            }
            Err(envelope_error(
                req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "value must be an integer for low_slash_streak/low_slash_amount",
            ))
        },
        _ => {
            if let Some(as_f64) = value.as_f64() {
                return Ok(as_f64.to_string());
            }
            if let Some(text) = value.as_str() {
                return Ok(text.trim().to_string());
            }
            Err(envelope_error(
                req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                "value must be a number for quality thresholds/multipliers",
            ))
        },
    }
}

fn normalize_trust_governance_value(req: &HttpRequest, value: &serde_json::Value) -> Result<String, HttpResponse> {
    if let Some(as_f64) = value.as_f64() {
        return Ok(as_f64.to_string());
    }
    if let Some(text) = value.as_str() {
        return Ok(text.trim().to_string());
    }
    Err(envelope_error(
        req,
        StatusCode::UNPROCESSABLE_ENTITY,
        "UNPROCESSABLE_ENTITY",
        "value must be a number for trust weights",
    ))
}

fn trust_ordering_hints_for_policy(
    policy: &wrfcoin_native_chain::blockchain::state::TrustWeightingPolicy,
) -> Vec<wrfcoin_native_chain::governance::TrustWeightOrderingHint> {
    vec![
        wrfcoin_native_chain::governance::TrustWeightOrderingHint {
            rule: "trust.verified_station_weight >= trust.unverified_weight".to_string(),
            satisfied: policy.verified_station_weight >= policy.unverified_weight,
        },
        wrfcoin_native_chain::governance::TrustWeightOrderingHint {
            rule: "trust.trusted_provider_weight >= trust.verified_station_weight".to_string(),
            satisfied: policy.trusted_provider_weight >= policy.verified_station_weight,
        },
    ]
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn list_governance_laws(
    request: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<LawListQuery>,
) -> impl Responder {
    let status_filter = match query.status.as_deref() {
        Some(raw) => match parse_law_status_filter(raw) {
            Some(value) => Some(value),
            None => {
                return law_book_error(
                    &request,
                    StatusCode::BAD_REQUEST,
                    "VALIDATION_ERROR",
                    "status must be draft|active|deprecated",
                );
            },
        },
        None => None,
    };
    let enforcement_filter = match query.enforcement.as_deref() {
        Some(raw) => match parse_law_enforcement_filter(raw) {
            Some(value) => Some(value),
            None => {
                return law_book_error(
                    &request,
                    StatusCode::BAD_REQUEST,
                    "VALIDATION_ERROR",
                    "enforcement must be advisory|api|consensus",
                );
            },
        },
        None => None,
    };
    let limit = query.limit.unwrap_or(DEFAULT_LAW_BOOK_PAGE_SIZE);
    if limit == 0 {
        return law_book_error(
            &request,
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "limit must be greater than 0",
        );
    }
    let limit = limit.min(MAX_LAW_BOOK_PAGE_SIZE);
    let include_amendments = query.include_amendments.unwrap_or(false);
    let include_history = query.include_history.unwrap_or(false);
    if include_history && !include_amendments {
        return law_book_error(
            &request,
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "include_history=true requires include_amendments=true",
        );
    }
    let q = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let cursor = query.cursor.as_deref();

    let blockchain = state.blockchain.read().await;
    let mut matching_laws = blockchain
        .state
        .governance
        .law_book
        .laws
        .values()
        .filter(|law| {
            status_filter.is_none_or(|status| law.status == status)
                && enforcement_filter.is_none_or(|enforcement| law.enforcement_level == enforcement)
                && query
                    .category
                    .as_deref()
                    .is_none_or(|category| law.category == category)
        })
        .collect::<Vec<_>>();

    let search = if let Some(search_query) = q {
        let tokens = law_search_tokens(search_query);
        let search_index = build_law_search_index(&matching_laws);
        matching_laws.retain(|law| {
            tokens.iter().all(|token| {
                search_index
                    .get(token)
                    .is_some_and(|hits| hits.contains(&law.law_id))
            })
        });
        Some(LawSearchMetadata {
            query: search_query.to_string(),
            tokens,
            indexed_fields: vec![
                "law_id".to_string(),
                "title".to_string(),
                "category".to_string(),
                "plain_language_summary".to_string(),
                "amendments.summary".to_string(),
            ],
            matched_law_count: matching_laws.len(),
        })
    } else {
        None
    };

    matching_laws.sort_by(|left, right| left.law_id.cmp(&right.law_id));

    let start = cursor
        .map(|cursor_value| {
            matching_laws
                .iter()
                .position(|law| law.law_id.as_str() > cursor_value)
                .unwrap_or(matching_laws.len())
        })
        .unwrap_or(0);

    let page_items = matching_laws
        .iter()
        .skip(start)
        .take(limit)
        .map(|law| law_record_to_api(law, include_amendments, include_history))
        .collect::<Vec<_>>();
    let next_cursor = if start + page_items.len() < matching_laws.len() {
        page_items.last().map(|law| law.law_id.clone())
    } else {
        None
    };

    law_book_ok(
        &request,
        LawListApiData {
            items: page_items,
            next_cursor,
            search,
        },
    )
}

pub async fn get_governance_law(
    request: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
    query: web::Query<LawGetQuery>,
) -> impl Responder {
    let law_id = path.into_inner();
    if let Err(message) = validate_law_id_for_api(&law_id) {
        return law_book_error(
            &request,
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            message,
        );
    }
    let include_amendments = query.include_amendments.unwrap_or(true);
    let include_history = query.include_history.unwrap_or(true);
    if include_history && !include_amendments {
        return law_book_error(
            &request,
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "include_history=true requires include_amendments=true",
        );
    }

    let blockchain = state.blockchain.read().await;
    let law = match blockchain.state.get_law(&law_id) {
        Some(law) => law,
        None => {
            return law_book_error(
                &request,
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                format!("Law not found: {law_id}"),
            );
        },
    };

    law_book_ok(
        &request,
        law_record_to_api(law, include_amendments, include_history),
    )
}

pub async fn build_quality_governance_proposal(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<QualityGovernanceProposalBuildRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }
    let target = match parse_quality_governance_target(&req, &request.target) {
        Ok(target) => target,
        Err(resp) => return resp,
    };
    let value = match normalize_quality_governance_value(&req, &request.value, &target) {
        Ok(value) => value,
        Err(resp) => return resp,
    };

    let blockchain = state.blockchain.read().await;
    let preview = match build_quality_parameter_preview(
        &blockchain.state.governance.params.data_quality_policy,
        ParameterUpdateProposal {
            target,
            value: value.clone(),
        },
    ) {
        Ok(preview) => preview,
        Err(err) => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                format!("quality proposal validation failed: {err}"),
            );
        },
    };

    if let Err(err) = blockchain
        .state
        .parse_governance_parameter_update(&preview.parameter, &preview.value)
    {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("proposal payload does not satisfy governance parser: {err}"),
        );
    }

    HttpResponse::Ok().json(QualityGovernanceProposalBuildResponse {
        proposal_payload: GovernanceProposalPayload {
            proposal_id: String::new(),
            parameter: preview.parameter.clone(),
            value: preview.value.clone(),
            voting_deadline: None,
            deposit: None,
        },
        preview,
    })
}

pub async fn get_trust_weights(state: web::Data<AppState>) -> impl Responder {
    let blockchain = state.blockchain.read().await;
    let current_policy = blockchain.state.governance.params.trust_weighting_policy.clone();
    HttpResponse::Ok().json(TrustWeightsResponse {
        current_policy: current_policy.clone(),
        bounds: wrfcoin_native_chain::governance::TrustWeightBoundsPreview { min: 0.0, max: 1.0 },
        ordering_hints: trust_ordering_hints_for_policy(&current_policy),
    })
}

pub async fn build_trust_governance_proposal(
    req: HttpRequest,
    state: web::Data<AppState>,
    request: web::Json<TrustGovernanceProposalBuildRequest>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }
    let target = match parse_trust_governance_target(&req, &request.target) {
        Ok(target) => target,
        Err(resp) => return resp,
    };
    let value = match normalize_trust_governance_value(&req, &request.value) {
        Ok(value) => value,
        Err(resp) => return resp,
    };

    let blockchain = state.blockchain.read().await;
    let preview = match build_trust_weight_parameter_preview(
        &blockchain.state.governance.params.trust_weighting_policy,
        ParameterUpdateProposal {
            target,
            value: value.clone(),
        },
    ) {
        Ok(preview) => preview,
        Err(err) => {
            return envelope_error(
                &req,
                StatusCode::UNPROCESSABLE_ENTITY,
                "UNPROCESSABLE_ENTITY",
                format!("trust proposal validation failed: {err}"),
            );
        },
    };

    if let Err(err) = blockchain
        .state
        .parse_governance_parameter_update(&preview.parameter, &preview.value)
    {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            format!("proposal payload does not satisfy governance parser: {err}"),
        );
    }

    HttpResponse::Ok().json(TrustGovernanceProposalBuildResponse {
        proposal_payload: GovernanceProposalPayload {
            proposal_id: String::new(),
            parameter: preview.parameter.clone(),
            value: preview.value.clone(),
            voting_deadline: None,
            deposit: None,
        },
        preview,
    })
}

// ---------------------------------------------------------------------------
// Proposal lifecycle endpoints (activate / finalize / execute)
// ---------------------------------------------------------------------------

/// Activate a governance proposal (move to active voting).
///
/// PUT /governance/proposals/:id/activate
pub async fn activate_proposal(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let proposal_id = path.into_inner();

    // Verify proposal exists and is in a valid state for activation.
    {
        let blockchain = state.blockchain.read().await;
        match blockchain.state.governance.proposals.get(&proposal_id) {
            Some(proposal) => {
                if proposal.status != GovernanceProposalStatus::Active {
                    return envelope_error(
                        &req,
                        StatusCode::CONFLICT,
                        "CONFLICT",
                        format!(
                            "Proposal '{}' is already in status {:?} and cannot be activated",
                            proposal_id, proposal.status
                        ),
                    );
                }
            },
            None => {
                return envelope_error(
                    &req,
                    StatusCode::NOT_FOUND,
                    "NOT_FOUND",
                    format!("Proposal not found: {proposal_id}"),
                );
            },
        }
    }

    let payload = GovernanceExecutePayload {
        proposal_id: proposal_id.clone(),
    };

    submit_protocol_transaction(
        &req,
        state,
        TransactionType::GovernanceExecute,
        "governance",
        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    )
    .await
}

/// Finalize a governance proposal (close the voting period).
///
/// PUT /governance/proposals/:id/finalize
pub async fn finalize_proposal(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let proposal_id = path.into_inner();

    // Verify proposal exists and is Active (eligible for finalization).
    {
        let blockchain = state.blockchain.read().await;
        match blockchain.state.governance.proposals.get(&proposal_id) {
            Some(proposal) => {
                if proposal.status != GovernanceProposalStatus::Active {
                    return envelope_error(
                        &req,
                        StatusCode::CONFLICT,
                        "CONFLICT",
                        format!(
                            "Proposal '{}' is in status {:?} and cannot be finalized",
                            proposal_id, proposal.status
                        ),
                    );
                }
            },
            None => {
                return envelope_error(
                    &req,
                    StatusCode::NOT_FOUND,
                    "NOT_FOUND",
                    format!("Proposal not found: {proposal_id}"),
                );
            },
        }
    }

    let payload = GovernanceFinalizePayload {
        proposal_id: proposal_id.clone(),
    };

    submit_protocol_transaction(
        &req,
        state,
        TransactionType::GovernanceFinalize,
        "governance",
        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    )
    .await
}

/// Execute an approved governance proposal.
///
/// PUT /governance/proposals/:id/execute
pub async fn execute_proposal(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let proposal_id = path.into_inner();

    // Verify proposal exists and is in Executed or Active state.
    {
        let blockchain = state.blockchain.read().await;
        match blockchain.state.governance.proposals.get(&proposal_id) {
            Some(proposal) => {
                if proposal.status == GovernanceProposalStatus::Rejected
                    || proposal.status == GovernanceProposalStatus::FailedQuorum
                    || proposal.status == GovernanceProposalStatus::FailedExecution
                {
                    return envelope_error(
                        &req,
                        StatusCode::CONFLICT,
                        "CONFLICT",
                        format!(
                            "Proposal '{}' is in status {:?} and cannot be executed",
                            proposal_id, proposal.status
                        ),
                    );
                }
            },
            None => {
                return envelope_error(
                    &req,
                    StatusCode::NOT_FOUND,
                    "NOT_FOUND",
                    format!("Proposal not found: {proposal_id}"),
                );
            },
        }
    }

    let payload = GovernanceExecutePayload {
        proposal_id: proposal_id.clone(),
    };

    submit_protocol_transaction(
        &req,
        state,
        TransactionType::GovernanceExecute,
        "governance",
        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    )
    .await
}

/// Retrieve the vote history for a specific proposal.
///
/// GET /governance/proposals/:id/votes
pub async fn get_proposal_votes(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    let proposal_id = path.into_inner();
    let blockchain = state.blockchain.read().await;
    match blockchain.state.governance.proposals.get(&proposal_id) {
        Some(proposal) => {
            #[derive(Serialize)]
            struct VoteHistoryResponse {
                proposal_id: String,
                yes_votes: u64,
                no_votes: u64,
                quorum_percent: u64,
                approval_percent: u64,
                eligible_voting_power: u64,
                votes: Vec<VoteRecord>,
            }
            #[derive(Serialize)]
            struct VoteRecord {
                voter: String,
                approve: bool,
                voting_power: u64,
                cast_at_block: u64,
            }
            // Cap at 500 to avoid unbounded response amplification (#1834).
            let mut votes: Vec<VoteRecord> = proposal
                .votes_by_voter
                .iter()
                .take(500)
                .map(|(voter, record)| VoteRecord {
                    voter: voter.clone(),
                    approve: record.approve,
                    voting_power: record.voting_power,
                    cast_at_block: record.cast_at_block,
                })
                .collect();
            votes.sort_by_key(|v| v.cast_at_block);

            envelope_success(
                &req,
                StatusCode::OK,
                VoteHistoryResponse {
                    proposal_id: proposal.id.clone(),
                    yes_votes: proposal.yes_votes,
                    no_votes: proposal.no_votes,
                    quorum_percent: proposal.quorum_percent,
                    approval_percent: proposal.approval_percent,
                    eligible_voting_power: proposal.eligible_voting_power,
                    votes,
                },
            )
        },
        None => envelope_error(&req, StatusCode::NOT_FOUND, "NOT_FOUND", "Proposal not found"),
    }
}

// ---------------------------------------------------------------------------
// Delegation endpoint
// ---------------------------------------------------------------------------

/// Delegate voting power to another address.
///
/// POST /governance/delegate
pub async fn delegate_voting_power(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<GovernanceDelegatePayload>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Contributor) {
        return resp;
    }

    let delegator = req
        .headers()
        .get("X-Delegator-Address")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("contributor");

    let payload = body.into_inner();

    if payload.delegate_to.trim().is_empty() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "delegate_to address is required",
        );
    }

    if payload.delegate_to.trim() == delegator {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "Cannot delegate voting power to yourself",
        );
    }

    submit_protocol_transaction(
        &req,
        state,
        TransactionType::GovernanceDelegate,
        delegator,
        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    )
    .await
}

// ---------------------------------------------------------------------------
// Treasury endpoints
// ---------------------------------------------------------------------------

/// Get governance treasury balance and info.
///
/// GET /governance/treasury
pub async fn get_treasury(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> impl Responder {
    let blockchain = state.blockchain.read().await;
    let gov = &blockchain.state.governance;

    #[derive(Serialize)]
    struct TreasuryResponse {
        balance: String,
        total_deposits: String,
        total_withdrawals: String,
        transaction_count: usize,
    }

    let total_deposits: u64 = gov
        .treasury_transactions
        .iter()
        .filter(|tx| tx.tx_type == "deposit")
        .map(|tx| tx.amount)
        .sum();
    let total_withdrawals: u64 = gov
        .treasury_transactions
        .iter()
        .filter(|tx| tx.tx_type == "withdraw")
        .map(|tx| tx.amount)
        .sum();

    envelope_success(
        &req,
        StatusCode::OK,
        TreasuryResponse {
            balance: gov.treasury_balance.to_string(),
            total_deposits: total_deposits.to_string(),
            total_withdrawals: total_withdrawals.to_string(),
            transaction_count: gov.treasury_transactions.len(),
        },
    )
}

/// Get treasury transaction history.
///
/// GET /governance/treasury/transactions?offset=N&limit=N
///
/// Requires Reader role. Supports pagination — default page size is
/// `DEFAULT_TREASURY_TX_PAGE_SIZE`, capped at `MAX_TREASURY_TX_PAGE_SIZE`.
pub async fn get_treasury_transactions(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<TreasuryTxQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    // #2137: refuse an oversized page, resume from a cursor at constant cost
    // (a slice iterator's `skip` is O(1)), and charge only what is examined.
    let cost = route_cost(MAX_TREASURY_TX_PAGE_SIZE);
    let limit = match page_size(query.limit.map(|l| l.min(u32::MAX as usize) as u32), DEFAULT_TREASURY_TX_PAGE_SIZE as u32, &cost) {
        Ok(l) => l as usize,
        Err(e) => return query_cost::page_rejected(&req, e),
    };
    let offset = match start_position(&req, query.offset, query.cursor.as_deref()) {
        Ok(o) => o,
        Err(resp) => return resp,
    };
    let mut meter = CostMeter::new(&cost);
    let blockchain = state.blockchain.read().await;
    let all = &blockchain.state.governance.treasury_transactions;
    let total = all.len();
    if let Err(e) = meter.charge(limit.min(total.saturating_sub(offset)) as u64) {
        return query_cost::cost_exceeded(&req, e);
    }
    let page: Vec<_> = all.iter().skip(offset).take(limit).map(|tx| {
        serde_json::json!({
            "tx_type": tx.tx_type,
            "amount": tx.amount.to_string(),
            "address": tx.address,
            "block_height": tx.block_height,
            "memo": tx.memo,
            "proposal_id": tx.proposal_id,
        })
    }).collect();
    envelope_success(
        &req,
        StatusCode::OK,
        serde_json::json!({
            "total": total,
            "offset": offset,
            "limit": limit,
            "next_cursor": next_cursor(offset, page.len(), total),
            "transactions": page,
        }),
    )
}

/// Deposit funds into the governance treasury.
///
/// POST /governance/treasury/deposit
pub async fn treasury_deposit(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<TreasuryDepositPayload>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let depositor = req
        .headers()
        .get("X-Depositor-Address")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("admin");

    let payload = body.into_inner();

    if payload.amount == 0 {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "Deposit amount must be greater than 0",
        );
    }

    submit_protocol_transaction(
        &req,
        state,
        TransactionType::TreasuryDeposit,
        depositor,
        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    )
    .await
}

/// Withdraw funds from the governance treasury.
///
/// POST /governance/treasury/withdraw
pub async fn treasury_withdraw(
    req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<TreasuryWithdrawPayload>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let payload = body.into_inner();

    if payload.amount == 0 {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "Withdrawal amount must be greater than 0",
        );
    }

    if payload.recipient.trim().is_empty() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "Recipient address is required",
        );
    }

    if payload.proposal_id.trim().is_empty() {
        return envelope_error(
            &req,
            StatusCode::UNPROCESSABLE_ENTITY,
            "UNPROCESSABLE_ENTITY",
            "Proposal ID authorizing the withdrawal is required",
        );
    }

    // Verify the referenced proposal exists.
    {
        let blockchain = state.blockchain.read().await;
        if !blockchain
            .state
            .governance
            .proposals
            .contains_key(&payload.proposal_id)
        {
            return envelope_error(
                &req,
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                format!("Authorizing proposal not found: {}", payload.proposal_id),
            );
        }
    }

    submit_protocol_transaction(
        &req,
        state,
        TransactionType::TreasuryWithdraw,
        "treasury",
        serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    )
    .await
}

// ---------------------------------------------------------------------------
// Analytics endpoint
// ---------------------------------------------------------------------------

/// Maximum number of proposals the analytics scan will visit in a single
/// request (#1835). Caps the O(proposals × votes) lock-hold time so a
/// reader-authenticated caller cannot stall all writers indefinitely.
pub const MAX_ANALYTICS_PROPOSALS_SCAN: usize = 5_000;

/// Get governance analytics dashboard data.
///
/// GET /governance/analytics/dashboard
///
/// Requires `ApiRole::Reader` (#1835).  The proposal scan is capped at
/// [`MAX_ANALYTICS_PROPOSALS_SCAN`] to bound the blockchain read-lock hold
/// time; `proposals_scanned` / `total_proposals` in the response indicate
/// whether the result covers the full set.
pub async fn get_governance_analytics(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let blockchain = state.blockchain.read().await;
    let gov = &blockchain.state.governance;

    #[derive(Serialize)]
    struct GovernanceAnalytics {
        total_proposals: usize,
        /// Number of proposals actually scanned (≤ total_proposals).
        proposals_scanned: usize,
        active_proposals: usize,
        executed_proposals: usize,
        rejected_proposals: usize,
        failed_quorum_proposals: usize,
        failed_execution_proposals: usize,
        total_votes_cast: usize,
        unique_voters: usize,
        total_delegations: usize,
        treasury_balance: String,
        law_count: usize,
    }

    let mut active = 0usize;
    let mut executed = 0usize;
    let mut rejected = 0usize;
    let mut failed_quorum = 0usize;
    let mut failed_execution = 0usize;
    let mut total_votes = 0usize;
    // Borrow voter keys rather than cloning to avoid heap allocation per voter
    // (#1835). The `&str` lifetime is bounded by the blockchain read guard.
    let mut unique_voters: std::collections::HashSet<&str> =
        std::collections::HashSet::new();

    // Bounded scan: at most MAX_ANALYTICS_PROPOSALS_SCAN proposals so the
    // read lock is not held for an arbitrarily long O(proposals × votes) scan.
    let mut proposals_scanned = 0usize;
    for proposal in gov
        .proposals
        .values()
        .take(MAX_ANALYTICS_PROPOSALS_SCAN)
    {
        proposals_scanned += 1;
        match proposal.status {
            GovernanceProposalStatus::Active => active += 1,
            GovernanceProposalStatus::Executed => executed += 1,
            GovernanceProposalStatus::Rejected => rejected += 1,
            GovernanceProposalStatus::FailedQuorum => failed_quorum += 1,
            GovernanceProposalStatus::FailedExecution => failed_execution += 1,
        }
        total_votes += proposal.votes_by_voter.len();
        for voter in proposal.votes_by_voter.keys() {
            unique_voters.insert(voter.as_str());
        }
    }

    envelope_success(
        &req,
        StatusCode::OK,
        GovernanceAnalytics {
            total_proposals: gov.proposals.len(),
            proposals_scanned,
            active_proposals: active,
            executed_proposals: executed,
            rejected_proposals: rejected,
            failed_quorum_proposals: failed_quorum,
            failed_execution_proposals: failed_execution,
            total_votes_cast: total_votes,
            unique_voters: unique_voters.len(),
            total_delegations: gov.delegations.len(),
            treasury_balance: gov.treasury_balance.to_string(),
            law_count: gov.law_book.laws.len(),
        },
    )
}
