use actix_web::{HttpRequest, HttpResponse, Responder, http::StatusCode, web};
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::helpers::{envelope_error, require_role};
use super::query_cost;
use wrfcoin_native_chain::security::query_cost::{CostMeter, Cursor, HistoryRange, QueryCost, page_size};
use wrfcoin_native_chain::security::api_auth::ApiRole;
use crate::state::{AppState, MAX_QUALITY_EVENTS_PAGE_SIZE, MAX_REWARD_IMPACT_PAGE_SIZE, TrustRewardCacheEntry};
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::blockchain::state::{
    QualityPolicyEventRecord, QualityPolicyEventType, State,
};
use wrfcoin_native_chain::common::types::Address;
use wrfcoin_shared_protocol::weather::validation::{DataQualityBand, DataQualityPolicy};

const MAX_QUALITY_EVENT_SCAN_RECORDS: usize = 10_000;
const MAX_QUALITY_ARCHIVE_SCAN_BUCKETS: usize = 10_000;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct QualityEventsQuery {
    pub participant: Option<String>,
    pub commit_id: Option<String>,
    pub band: Option<String>,
    pub event_type: Option<String>,
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    /// #2137: opaque continuation token from a previous page's `next_cursor`.
    /// With a cursor the scan resumes at the recorded position and stops after
    /// one page, so every page costs the same however deep it is.
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QualitySummaryQuery {
    pub participant: Option<String>,
    pub commit_id: Option<String>,
    pub band: Option<String>,
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustRewardImpactQuery {
    pub from_block: Option<u64>,
    pub to_block: Option<u64>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QualityEventResponse {
    pub event_id: u64,
    pub event_type: String,
    pub block_height: u64,
    pub commit_id: String,
    pub participant: String,
    pub quality_band: String,
    pub quality_score: Option<f64>,
    pub scoring_multiplier: f64,
    pub reward_multiplier: f64,
    pub warning_only: bool,
    pub trigger_challenge: bool,
    pub counts_toward_low_quality_streak: bool,
    pub low_quality_streak: Option<u32>,
    pub low_quality_slash_threshold: Option<u32>,
    pub slashed_amount: Option<u128>,
    pub policy_snapshot: DataQualityPolicy,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct QualityBandCountersResponse {
    pub high: u64,
    pub borderline: u64,
    pub warning_only: u64,
    pub low: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QualityEventsResponse {
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    /// #2137: continuation token for the next page, when the scan stopped
    /// before the oldest record. `None` on the legacy offset path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub archive: QualityArchiveStatusResponse,
    pub archived_records_omitted: bool,
    pub current_policy: DataQualityPolicy,
    pub items: Vec<QualityEventResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QualityArchiveStatusResponse {
    pub hot_event_count: usize,
    pub archived_event_count: u64,
    pub archived_through_event_id: Option<u64>,
    pub hot_first_event_id: Option<u64>,
    pub hot_first_block: Option<u64>,
    pub archived_detail_records_available: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QualitySummaryResponse {
    pub total_events: u64,
    pub challenge_events: u64,
    pub slashing_events: u64,
    pub band_counters: QualityBandCountersResponse,
    pub challenge_band_counters: QualityBandCountersResponse,
    pub slashing_band_counters: QualityBandCountersResponse,
    pub archive: QualityArchiveStatusResponse,
    pub archived_aggregates_included: bool,
    pub archived_aggregates_omitted: bool,
    pub current_policy: DataQualityPolicy,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TrustClassCountersResponse {
    pub unverified: u64,
    pub verified_station: u64,
    pub trusted_provider: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProvenanceDecisionCountersResponse {
    pub real: u64,
    pub synthetic: u64,
    pub unclassified: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProvenanceMintedTotalsResponse {
    pub real: u128,
    pub synthetic: u128,
    pub unclassified: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityRewardMultiplierResponse {
    pub high: f64,
    pub borderline: f64,
    pub warning_only: f64,
    pub low: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustRewardImpactMatrixRowResponse {
    pub quality_band: String,
    pub quality_reward_multiplier: f64,
    pub unverified: f64,
    pub verified_station: f64,
    pub trusted_provider: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustRewardImpactPointResponse {
    pub block_height: u64,
    pub minted_count: u64,
    pub minted_total: u128,
    pub provenance_decision_counts: ProvenanceDecisionCountersResponse,
    pub provenance_minted_totals: ProvenanceMintedTotalsResponse,
    pub trust_class_counts: TrustClassCountersResponse,
    pub quality_band_counts: QualityBandCountersResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_weights: Option<wrfcoin_native_chain::blockchain::state::TrustWeightingPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_reward_multipliers: Option<QualityRewardMultiplierResponse>,
    pub joint_multiplier_matrix: Vec<TrustRewardImpactMatrixRowResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustRewardImpactTimelineResponse {
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub items: Vec<TrustRewardImpactPointResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustRewardImpactSummaryResponse {
    pub block_count: usize,
    pub minted_count: u64,
    pub minted_total: u128,
    pub provenance_decision_counts: ProvenanceDecisionCountersResponse,
    pub provenance_minted_totals: ProvenanceMintedTotalsResponse,
    pub trust_class_counts: TrustClassCountersResponse,
    pub quality_band_counts: QualityBandCountersResponse,
    pub estimated_weighted_trust_multiplier: f64,
    pub estimated_weighted_quality_multiplier: f64,
    pub estimated_effective_multiplier: f64,
    pub current_trust_policy: wrfcoin_native_chain::blockchain::state::TrustWeightingPolicy,
    pub current_quality_policy: DataQualityPolicy,
    pub joint_multiplier_preview: Vec<TrustRewardImpactMatrixRowResponse>,
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

pub fn quality_band_label(band: DataQualityBand) -> &'static str {
    match band {
        DataQualityBand::High => "high",
        DataQualityBand::Borderline => "borderline",
        DataQualityBand::WarningOnly => "warning_only",
        DataQualityBand::Low => "low",
    }
}

pub fn quality_event_type_label(event_type: QualityPolicyEventType) -> &'static str {
    match event_type {
        QualityPolicyEventType::ChallengeTriggered => "challenge",
        QualityPolicyEventType::SlashingApplied => "slashing",
    }
}

pub fn parse_quality_band_filter(req: &HttpRequest, value: &str) -> Result<DataQualityBand, HttpResponse> {
    match value.trim().to_ascii_lowercase().as_str() {
        "high" => Ok(DataQualityBand::High),
        "borderline" => Ok(DataQualityBand::Borderline),
        "warning" | "warning_only" | "warning-only" => Ok(DataQualityBand::WarningOnly),
        "low" => Ok(DataQualityBand::Low),
        _ => Err(envelope_error(
            req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "band must be one of: high, borderline, warning_only, low",
        )),
    }
}

fn parse_quality_event_type_filter(req: &HttpRequest, value: &str) -> Result<QualityPolicyEventType, HttpResponse> {
    match value.trim().to_ascii_lowercase().as_str() {
        "challenge" => Ok(QualityPolicyEventType::ChallengeTriggered),
        "slashing" | "slash" => Ok(QualityPolicyEventType::SlashingApplied),
        _ => Err(envelope_error(
            req,
            StatusCode::BAD_REQUEST,
            "BAD_REQUEST",
            "event_type must be one of: challenge, slashing",
        )),
    }
}

pub fn increment_band_counter(counters: &mut QualityBandCountersResponse, band: DataQualityBand) {
    increment_band_counter_by(counters, band, 1);
}

pub fn increment_band_counter_by(
    counters: &mut QualityBandCountersResponse,
    band: DataQualityBand,
    delta: u64,
) {
    match band {
        DataQualityBand::High => counters.high = counters.high.saturating_add(delta),
        DataQualityBand::Borderline => {
            counters.borderline = counters.borderline.saturating_add(delta)
        },
        DataQualityBand::WarningOnly => {
            counters.warning_only = counters.warning_only.saturating_add(delta)
        },
        DataQualityBand::Low => counters.low = counters.low.saturating_add(delta),
    }
}

fn event_matches_quality_filters(
    event: &QualityPolicyEventRecord,
    participant: Option<Address>,
    commit_id: Option<&str>,
    band: Option<DataQualityBand>,
    event_type: Option<QualityPolicyEventType>,
    from_block: Option<u64>,
    to_block: Option<u64>,
) -> bool {
    if let Some(participant) = participant {
        if event.participant != participant {
            return false;
        }
    }
    if let Some(commit_id) = commit_id {
        if event.commit_id != commit_id {
            return false;
        }
    }
    if let Some(band) = band {
        if event.quality_band != band {
            return false;
        }
    }
    if let Some(event_type) = event_type {
        if event.event_type != event_type {
            return false;
        }
    }
    if let Some(from_block) = from_block {
        if event.block_height < from_block {
            return false;
        }
    }
    if let Some(to_block) = to_block {
        if event.block_height > to_block {
            return false;
        }
    }
    true
}

fn quality_event_response(event: &QualityPolicyEventRecord) -> QualityEventResponse {
    QualityEventResponse {
        event_id: event.event_id,
        event_type: quality_event_type_label(event.event_type).to_string(),
        block_height: event.block_height,
        commit_id: event.commit_id.clone(),
        participant: event.participant.to_string(),
        quality_band: quality_band_label(event.quality_band).to_string(),
        quality_score: event.quality_score.as_ref().map(|score| score.overall),
        scoring_multiplier: event.scoring_multiplier,
        reward_multiplier: event.reward_multiplier,
        warning_only: event.warning_only,
        trigger_challenge: event.trigger_challenge,
        counts_toward_low_quality_streak: event.counts_toward_low_quality_streak,
        low_quality_streak: event.low_quality_streak,
        low_quality_slash_threshold: event.low_quality_slash_threshold,
        slashed_amount: event.slashed_amount.map(|amount| amount.into_inner()),
        policy_snapshot: event.policy_snapshot.clone(),
    }
}

pub fn quality_archive_status(state: &State) -> QualityArchiveStatusResponse {
    let (hot_first_event_id, hot_first_block) = state
        .hot_quality_policy_event_frontier()
        .map(|(event_id, block_height)| (Some(event_id), Some(block_height)))
        .unwrap_or((None, None));
    QualityArchiveStatusResponse {
        hot_event_count: state.quality_policy_events().len(),
        archived_event_count: state.archived_quality_policy_event_count(),
        archived_through_event_id: state.archived_quality_policy_event_through_event_id(),
        hot_first_event_id,
        hot_first_block,
        archived_detail_records_available: state.archived_quality_policy_event_count() > 0,
    }
}

fn query_overlaps_archived_block_range(
    state: &State,
    from_block: Option<u64>,
    to_block: Option<u64>,
) -> bool {
    let Some(first) = state.quality_policy_event_archive().first() else {
        return false;
    };
    let Some(last) = state.quality_policy_event_archive().last() else {
        return false;
    };
    let from = from_block.unwrap_or(0);
    let to = to_block.unwrap_or(u64::MAX);
    from <= last.block_height && to >= first.block_height
}

fn archived_band_count(
    counters: &wrfcoin_native_chain::blockchain::state::QualityPolicyEventBandCounters,
    band: DataQualityBand,
) -> u64 {
    match band {
        DataQualityBand::High => counters.high,
        DataQualityBand::Borderline => counters.borderline,
        DataQualityBand::WarningOnly => counters.warning_only,
        DataQualityBand::Low => counters.low,
    }
}

pub fn reward_multipliers_from_policy(
    policy: &DataQualityPolicy,
) -> QualityRewardMultiplierResponse {
    QualityRewardMultiplierResponse {
        high: 1.0,
        borderline: policy.borderline_reward_multiplier,
        warning_only: policy.warning_only_reward_multiplier,
        low: policy.low_quality_reward_multiplier,
    }
}

pub fn metadata_u64(metadata: &rustc_hash::FxHashMap<String, String>, key: &str) -> u64 {
    metadata
        .get(key)
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or_default()
}

fn metadata_u128(metadata: &rustc_hash::FxHashMap<String, String>, key: &str) -> u128 {
    metadata
        .get(key)
        .and_then(|raw| raw.parse::<u128>().ok())
        .unwrap_or_default()
}

pub fn parse_trust_weights_metadata(
    raw: &str,
) -> Option<wrfcoin_native_chain::blockchain::state::TrustWeightingPolicy> {
    let mut unverified = None;
    let mut verified = None;
    let mut trusted = None;
    for item in raw.split(',') {
        let (key, value_raw) = item.trim().split_once('=')?;
        let value = value_raw.trim().parse::<f64>().ok()?;
        match key.trim() {
            "unverified" => unverified = Some(value),
            "verified_station" => verified = Some(value),
            "trusted_provider" => trusted = Some(value),
            _ => {},
        }
    }
    let policy = wrfcoin_native_chain::blockchain::state::TrustWeightingPolicy {
        unverified_weight: unverified?,
        verified_station_weight: verified?,
        trusted_provider_weight: trusted?,
    };
    if !policy.unverified_weight.is_finite()
        || !policy.verified_station_weight.is_finite()
        || !policy.trusted_provider_weight.is_finite()
    {
        return None;
    }
    if !(0.0..=1.0).contains(&policy.unverified_weight)
        || !(0.0..=1.0).contains(&policy.verified_station_weight)
        || !(0.0..=1.0).contains(&policy.trusted_provider_weight)
    {
        return None;
    }
    if policy.verified_station_weight < policy.unverified_weight {
        return None;
    }
    if policy.trusted_provider_weight < policy.verified_station_weight {
        return None;
    }
    Some(policy)
}

fn parse_quality_reward_segment(segment: &str) -> Option<(String, f64)> {
    let trimmed = segment.trim();
    let (band, rest) = trimmed.split_once('(')?;
    let mut reward = None;
    for part in rest.trim_end_matches(')').split(',') {
        let (k, v) = part.trim().split_once('=')?;
        if k.trim() == "reward" {
            reward = v.trim().parse::<f64>().ok();
        }
    }
    Some((band.trim().to_string(), reward?))
}

pub fn parse_quality_reward_multipliers_metadata(raw: &str) -> Option<QualityRewardMultiplierResponse> {
    let mut borderline = None;
    let mut warning = None;
    let mut low = None;
    for segment in raw.split("),") {
        let normalized = if segment.trim_end().ends_with(')') {
            segment.to_string()
        } else {
            format!("{segment})")
        };
        let (band, reward) = parse_quality_reward_segment(&normalized)?;
        match band.as_str() {
            "borderline" => borderline = Some(reward),
            "warning" => warning = Some(reward),
            "low" => low = Some(reward),
            _ => {},
        }
    }

    Some(QualityRewardMultiplierResponse {
        high: 1.0,
        borderline: borderline?,
        warning_only: warning?,
        low: low?,
    })
}

pub fn build_joint_multiplier_matrix(
    trust: &wrfcoin_native_chain::blockchain::state::TrustWeightingPolicy,
    quality: &QualityRewardMultiplierResponse,
) -> Vec<TrustRewardImpactMatrixRowResponse> {
    let bands = [
        ("high", quality.high),
        ("borderline", quality.borderline),
        ("warning_only", quality.warning_only),
        ("low", quality.low),
    ];
    bands
        .into_iter()
        .map(
            |(quality_band, quality_reward_multiplier)| TrustRewardImpactMatrixRowResponse {
                quality_band: quality_band.to_string(),
                quality_reward_multiplier,
                unverified: quality_reward_multiplier * trust.unverified_weight,
                verified_station: quality_reward_multiplier * trust.verified_station_weight,
                trusted_provider: quality_reward_multiplier * trust.trusted_provider_weight,
            },
        )
        .collect()
}

pub fn should_include_trust_impact_block(block: &Block) -> bool {
    block.metadata.contains_key("rewards_minted_count")
        || block
            .metadata
            .contains_key("rewards_trust_unverified_count")
        || block
            .metadata
            .contains_key("rewards_trust_verified_station_count")
        || block
            .metadata
            .contains_key("rewards_trust_trusted_provider_count")
}

/// Parse the trust/quality metadata for a single block into a `TrustRewardCacheEntry`.
/// Called at most once per block; the result is memoized in
/// `AppState::trust_reward_metrics_cache` and reused on subsequent scrapes.
pub fn build_trust_reward_cache_entry(block: &Block) -> TrustRewardCacheEntry {
    let trust_weights = block
        .metadata
        .get("rewards_trust_weights")
        .and_then(|raw| parse_trust_weights_metadata(raw));
    let quality_multipliers = block
        .metadata
        .get("rewards_quality_multipliers")
        .and_then(|raw| parse_quality_reward_multipliers_metadata(raw))
        .map(|m| [m.high, m.borderline, m.warning_only, m.low]);
    TrustRewardCacheEntry {
        trust_unverified: metadata_u64(&block.metadata, "rewards_trust_unverified_count"),
        trust_verified_station: metadata_u64(
            &block.metadata,
            "rewards_trust_verified_station_count",
        ),
        trust_trusted_provider: metadata_u64(
            &block.metadata,
            "rewards_trust_trusted_provider_count",
        ),
        quality_high: metadata_u64(&block.metadata, "rewards_quality_high_count"),
        quality_borderline: metadata_u64(&block.metadata, "rewards_quality_borderline_count"),
        quality_warning_only: metadata_u64(&block.metadata, "rewards_quality_warning_count"),
        quality_low: metadata_u64(&block.metadata, "rewards_skipped_low_quality_count"),
        trust_weights,
        quality_multipliers,
    }
}

pub fn collect_trust_reward_impact_points(
    blockchain: &Blockchain,
    from_block: Option<u64>,
    to_block: Option<u64>,
    with_matrix: bool,
) -> Vec<TrustRewardImpactPointResponse> {
    // `with_matrix` (#1392): callers that never serialise
    // `joint_multiplier_matrix` (e.g. `/chain/metrics`,
    // `get_trust_reward_impact_summary`) pass `false` to skip the per-block
    // `Vec<JointMultiplierEntry>` allocation, which compounds quickly across
    // the in-memory window. The timeline endpoint, which returns the matrix
    // to clients, passes `true`.
    let start = from_block.unwrap_or(0);
    let end = to_block.unwrap_or(u64::MAX);
    let mut points = blockchain
        .blocks_range(start, end)
        .filter(|block| should_include_trust_impact_block(block))
        .map(|block| {
            let trust_weights = block
                .metadata
                .get("rewards_trust_weights")
                .and_then(|raw| parse_trust_weights_metadata(raw));
            let quality_reward_multipliers = block
                .metadata
                .get("rewards_quality_multipliers")
                .and_then(|raw| parse_quality_reward_multipliers_metadata(raw));
            let joint_multiplier_matrix = match (with_matrix, &trust_weights, &quality_reward_multipliers) {
                (true, Some(trust), Some(quality)) => build_joint_multiplier_matrix(trust, quality),
                _ => Vec::new(),
            };
            TrustRewardImpactPointResponse {
                block_height: block.height,
                minted_count: metadata_u64(&block.metadata, "rewards_minted_count"),
                minted_total: metadata_u128(&block.metadata, "rewards_minted_total"),
                provenance_decision_counts: ProvenanceDecisionCountersResponse {
                    real: metadata_u64(
                        &block.metadata,
                        "rewards_provenance_real_decision_count",
                    ),
                    synthetic: metadata_u64(
                        &block.metadata,
                        "rewards_provenance_synthetic_decision_count",
                    ),
                    unclassified: metadata_u64(
                        &block.metadata,
                        "rewards_provenance_unclassified_decision_count",
                    ),
                },
                provenance_minted_totals: ProvenanceMintedTotalsResponse {
                    real: metadata_u128(
                        &block.metadata,
                        "rewards_provenance_real_minted_total",
                    ),
                    synthetic: metadata_u128(
                        &block.metadata,
                        "rewards_provenance_synthetic_minted_total",
                    ),
                    unclassified: metadata_u128(
                        &block.metadata,
                        "rewards_provenance_unclassified_minted_total",
                    ),
                },
                trust_class_counts: TrustClassCountersResponse {
                    unverified: metadata_u64(&block.metadata, "rewards_trust_unverified_count"),
                    verified_station: metadata_u64(
                        &block.metadata,
                        "rewards_trust_verified_station_count",
                    ),
                    trusted_provider: metadata_u64(
                        &block.metadata,
                        "rewards_trust_trusted_provider_count",
                    ),
                },
                quality_band_counts: QualityBandCountersResponse {
                    high: metadata_u64(&block.metadata, "rewards_quality_high_count"),
                    borderline: metadata_u64(&block.metadata, "rewards_quality_borderline_count"),
                    warning_only: metadata_u64(&block.metadata, "rewards_quality_warning_count"),
                    low: metadata_u64(&block.metadata, "rewards_skipped_low_quality_count"),
                },
                trust_weights,
                quality_reward_multipliers,
                joint_multiplier_matrix,
            }
        })
        .collect::<Vec<_>>();

    points.sort_by(|a, b| b.block_height.cmp(&a.block_height));
    points
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[instrument(skip(req, state, query), fields(participant = ?query.participant, from_block = ?query.from_block))]
pub async fn get_quality_events(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<QualityEventsQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let participant = match query.participant.as_deref() {
        Some(value) => match value.parse::<Address>() {
            Ok(address) => Some(address),
            Err(_) => {
                return envelope_error(
                    &req,
                    StatusCode::BAD_REQUEST,
                    "BAD_REQUEST",
                    "participant must be a valid 20-byte hex address",
                );
            },
        },
        None => None,
    };
    let band = match query.band.as_deref() {
        Some(value) => match parse_quality_band_filter(&req, value) {
            Ok(band) => Some(band),
            Err(resp) => return resp,
        },
        None => None,
    };
    let event_type = match query.event_type.as_deref() {
        Some(value) => match parse_quality_event_type_filter(&req, value) {
            Ok(event_type) => Some(event_type),
            Err(resp) => return resp,
        },
        None => None,
    };

    // #2137: page ceiling refused not clamped; block range bounded before any
    // record is read; the scan charged per record examined and aborted at the
    // budget; a cursor resumes at a position and stops after one page.
    let cost = QueryCost {
        max_page_size: query_cost::query_cost().max_page_size.min(MAX_QUALITY_EVENTS_PAGE_SIZE as u32),
        ..*query_cost::query_cost()
    };
    let limit = match page_size(query.limit.map(|l| l.min(u32::MAX as usize) as u32), 50, &cost) {
        Ok(l) => l as usize,
        Err(e) => return query_cost::page_rejected(&req, e),
    };
    let cursor = match query.cursor.as_deref().map(Cursor::decode).transpose() {
        Ok(c) => c,
        Err(e) => return query_cost::cursor_rejected(&req, e),
    };
    let offset = match (cursor, query.offset) {
        (Some(_), Some(_)) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
                "offset and cursor are exclusive; continue with the cursor alone",
            );
        }
        (Some(_), None) => 0,
        (None, offset) => offset.unwrap_or(0),
    };
    let mut meter = CostMeter::new(&cost);

    // #2137: an explicit block range is bounded before any record is read; an
    // open-ended one keeps its old meaning and is bounded by the scan budget.
    let (filter_from, filter_to) = match (query.from_block, query.to_block) {
        (Some(from), Some(to)) => match HistoryRange::bounded(Some(from), Some(to), to, &cost) {
            Ok(range) => (Some(range.from), Some(range.to)),
            Err(e) => return query_cost::range_rejected(&req, e),
        },
        (from, to) => (from, to),
    };
    let blockchain = state.blockchain.read().await;
    let archive = quality_archive_status(&blockchain.state);
    let archived_records_omitted =
        query_overlaps_archived_block_range(&blockchain.state, query.from_block, query.to_block);
    let events = blockchain.state.quality_policy_events();
    // Newest first. A cursor is the index the previous page stopped at.
    let mut position = match cursor {
        Some(c) => usize::try_from(c.0).unwrap_or(usize::MAX).min(events.len()),
        None => events.len(),
    };
    let scan_floor = position.saturating_sub(MAX_QUALITY_EVENT_SCAN_RECORDS);
    let mut matched: Vec<&QualityPolicyEventRecord> = Vec::new();
    while position > scan_floor {
        position -= 1;
        if let Err(e) = meter.charge(1) {
            return query_cost::cost_exceeded(&req, e);
        }
        // panic-ok: position starts at most events.len() and is decremented inside the positive, bounded loop; the read lock keeps events unchanged.
        let event = &events[position];
        if event_matches_quality_filters(
            event,
            participant,
            query.commit_id.as_deref(),
            band,
            event_type,
            filter_from,
            filter_to,
        ) {
            matched.push(event);
            if cursor.is_some() && matched.len() == limit {
                break;
            }
        }
    }
    let next_cursor = (cursor.is_some() && position > 0).then(|| Cursor(position as u64).encode());
    matched.sort_by(|a, b| {
        b.block_height
            .cmp(&a.block_height)
            .then_with(|| b.event_id.cmp(&a.event_id))
    });

    let total = matched.len();
    let items = matched
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(quality_event_response)
        .collect::<Vec<_>>();

    HttpResponse::Ok().json(QualityEventsResponse {
        total,
        offset,
        limit,
        next_cursor,
        archive,
        archived_records_omitted,
        current_policy: blockchain
            .state
            .governance
            .params
            .data_quality_policy
            .clone(),
        items,
    })
}

#[instrument(skip(req, state, path, query), fields(from_block = ?query.from_block))]
pub async fn get_quality_participant_history(
    req: HttpRequest,
    state: web::Data<AppState>,
    path: web::Path<String>,
    query: web::Query<QualityEventsQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let participant = match path.into_inner().parse::<Address>() {
        Ok(address) => address,
        Err(_) => {
            return envelope_error(
                &req,
                StatusCode::BAD_REQUEST,
                "BAD_REQUEST",
                "participant path must be a valid 20-byte hex address",
            );
        },
    };
    let band = match query.band.as_deref() {
        Some(value) => match parse_quality_band_filter(&req, value) {
            Ok(band) => Some(band),
            Err(resp) => return resp,
        },
        None => None,
    };
    let event_type = match query.event_type.as_deref() {
        Some(value) => match parse_quality_event_type_filter(&req, value) {
            Ok(event_type) => Some(event_type),
            Err(resp) => return resp,
        },
        None => None,
    };
    let offset = query.offset.unwrap_or(0);
    let limit = query
        .limit
        .unwrap_or(50)
        .clamp(1, MAX_QUALITY_EVENTS_PAGE_SIZE);

    let blockchain = state.blockchain.read().await;
    let archive = quality_archive_status(&blockchain.state);
    let archived_records_omitted =
        query_overlaps_archived_block_range(&blockchain.state, query.from_block, query.to_block);
    let mut matched = blockchain
        .state
        .quality_policy_events()
        .iter()
        .filter(|event| {
            event_matches_quality_filters(
                event,
                Some(participant),
                query.commit_id.as_deref(),
                band,
                event_type,
                query.from_block,
                query.to_block,
            )
        })
        .collect::<Vec<_>>();
    matched.sort_by(|a, b| {
        b.block_height
            .cmp(&a.block_height)
            .then_with(|| b.event_id.cmp(&a.event_id))
    });

    let total = matched.len();
    let items = matched
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(quality_event_response)
        .collect::<Vec<_>>();

    HttpResponse::Ok().json(QualityEventsResponse {
        total,
        offset,
        limit,
        next_cursor: None,
        archive,
        archived_records_omitted,
        current_policy: blockchain
            .state
            .governance
            .params
            .data_quality_policy
            .clone(),
        items,
    })
}

#[instrument(skip(req, state, query), fields(participant = ?query.participant, from_block = ?query.from_block))]
pub async fn get_quality_summary(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<QualitySummaryQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let participant = match query.participant.as_deref() {
        Some(value) => match value.parse::<Address>() {
            Ok(address) => Some(address),
            Err(_) => {
                return envelope_error(
                    &req,
                    StatusCode::BAD_REQUEST,
                    "BAD_REQUEST",
                    "participant must be a valid 20-byte hex address",
                );
            },
        },
        None => None,
    };
    let band = match query.band.as_deref() {
        Some(value) => match parse_quality_band_filter(&req, value) {
            Ok(band) => Some(band),
            Err(resp) => return resp,
        },
        None => None,
    };

    let blockchain = state.blockchain.read().await;
    let archive = quality_archive_status(&blockchain.state);
    let mut band_counters = QualityBandCountersResponse::default();
    let mut challenge_band_counters = QualityBandCountersResponse::default();
    let mut slashing_band_counters = QualityBandCountersResponse::default();
    let mut total_events = 0u64;
    let mut challenge_events = 0u64;
    let mut slashing_events = 0u64;

    for event in blockchain
        .state
        .quality_policy_events()
        .iter()
        .rev()
        .take(MAX_QUALITY_EVENT_SCAN_RECORDS)
        .filter(|event| {
            event_matches_quality_filters(
                event,
                participant,
                query.commit_id.as_deref(),
                band,
                None,
                query.from_block,
                query.to_block,
            )
        })
    {
        total_events = total_events.saturating_add(1);
        increment_band_counter(&mut band_counters, event.quality_band);
        match event.event_type {
            QualityPolicyEventType::ChallengeTriggered => {
                challenge_events = challenge_events.saturating_add(1);
                increment_band_counter(&mut challenge_band_counters, event.quality_band);
            },
            QualityPolicyEventType::SlashingApplied => {
                slashing_events = slashing_events.saturating_add(1);
                increment_band_counter(&mut slashing_band_counters, event.quality_band);
            },
        }
    }

    let supports_archived_aggregates = participant.is_none() && query.commit_id.is_none();
    let archived_aggregates_included = supports_archived_aggregates
        && query_overlaps_archived_block_range(&blockchain.state, query.from_block, query.to_block);
    let archived_aggregates_omitted = !supports_archived_aggregates
        && query_overlaps_archived_block_range(&blockchain.state, query.from_block, query.to_block);

    if archived_aggregates_included {
        for bucket in blockchain
            .state
            .quality_policy_event_archive()
            .iter()
            .rev()
            .take(MAX_QUALITY_ARCHIVE_SCAN_BUCKETS)
            .filter(|bucket| {
                if let Some(from_block) = query.from_block {
                    if bucket.block_height < from_block {
                        return false;
                    }
                }
                if let Some(to_block) = query.to_block {
                    if bucket.block_height > to_block {
                        return false;
                    }
                }
                true
            })
        {
            match band {
                Some(band_filter) => {
                    let band_total = archived_band_count(&bucket.band_counters, band_filter);
                    let band_challenge =
                        archived_band_count(&bucket.challenge_band_counters, band_filter);
                    let band_slashing =
                        archived_band_count(&bucket.slashing_band_counters, band_filter);
                    total_events = total_events.saturating_add(band_total);
                    challenge_events = challenge_events.saturating_add(band_challenge);
                    slashing_events = slashing_events.saturating_add(band_slashing);
                    increment_band_counter_by(&mut band_counters, band_filter, band_total);
                    increment_band_counter_by(
                        &mut challenge_band_counters,
                        band_filter,
                        band_challenge,
                    );
                    increment_band_counter_by(
                        &mut slashing_band_counters,
                        band_filter,
                        band_slashing,
                    );
                },
                None => {
                    total_events = total_events.saturating_add(bucket.total_events);
                    challenge_events = challenge_events.saturating_add(bucket.challenge_events);
                    slashing_events = slashing_events.saturating_add(bucket.slashing_events);
                    increment_band_counter_by(
                        &mut band_counters,
                        DataQualityBand::High,
                        bucket.band_counters.high,
                    );
                    increment_band_counter_by(
                        &mut band_counters,
                        DataQualityBand::Borderline,
                        bucket.band_counters.borderline,
                    );
                    increment_band_counter_by(
                        &mut band_counters,
                        DataQualityBand::WarningOnly,
                        bucket.band_counters.warning_only,
                    );
                    increment_band_counter_by(
                        &mut band_counters,
                        DataQualityBand::Low,
                        bucket.band_counters.low,
                    );
                    increment_band_counter_by(
                        &mut challenge_band_counters,
                        DataQualityBand::High,
                        bucket.challenge_band_counters.high,
                    );
                    increment_band_counter_by(
                        &mut challenge_band_counters,
                        DataQualityBand::Borderline,
                        bucket.challenge_band_counters.borderline,
                    );
                    increment_band_counter_by(
                        &mut challenge_band_counters,
                        DataQualityBand::WarningOnly,
                        bucket.challenge_band_counters.warning_only,
                    );
                    increment_band_counter_by(
                        &mut challenge_band_counters,
                        DataQualityBand::Low,
                        bucket.challenge_band_counters.low,
                    );
                    increment_band_counter_by(
                        &mut slashing_band_counters,
                        DataQualityBand::High,
                        bucket.slashing_band_counters.high,
                    );
                    increment_band_counter_by(
                        &mut slashing_band_counters,
                        DataQualityBand::Borderline,
                        bucket.slashing_band_counters.borderline,
                    );
                    increment_band_counter_by(
                        &mut slashing_band_counters,
                        DataQualityBand::WarningOnly,
                        bucket.slashing_band_counters.warning_only,
                    );
                    increment_band_counter_by(
                        &mut slashing_band_counters,
                        DataQualityBand::Low,
                        bucket.slashing_band_counters.low,
                    );
                },
            }
        }
    }

    HttpResponse::Ok().json(QualitySummaryResponse {
        total_events,
        challenge_events,
        slashing_events,
        band_counters,
        challenge_band_counters,
        slashing_band_counters,
        archive,
        archived_aggregates_included,
        archived_aggregates_omitted,
        current_policy: blockchain
            .state
            .governance
            .params
            .data_quality_policy
            .clone(),
    })
}

#[instrument(skip(state, query), fields(from_block = ?query.from_block, to_block = ?query.to_block))]
pub async fn get_trust_reward_impact_timeline(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<TrustRewardImpactQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let offset = query.offset.unwrap_or(0);
    let limit = query
        .limit
        .unwrap_or(50)
        .clamp(1, MAX_REWARD_IMPACT_PAGE_SIZE);
    let blockchain = state.blockchain.read().await;
    // Timeline endpoint serialises `joint_multiplier_matrix` to the client,
    // so it must keep `with_matrix=true` (#1392).
    let points = collect_trust_reward_impact_points(
        &blockchain,
        query.from_block,
        query.to_block,
        true,
    );
    let total = points.len();
    let items = points
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    HttpResponse::Ok().json(TrustRewardImpactTimelineResponse {
        total,
        offset,
        limit,
        items,
    })
}

#[instrument(skip(state, query), fields(from_block = ?query.from_block, to_block = ?query.to_block))]
pub async fn get_trust_reward_impact_summary(
    req: HttpRequest,
    state: web::Data<AppState>,
    query: web::Query<TrustRewardImpactQuery>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Reader) {
        return resp;
    }
    let blockchain = state.blockchain.read().await;
    // Summary endpoint never reads `joint_multiplier_matrix`; skip the
    // per-block allocation (#1392).
    let points = collect_trust_reward_impact_points(
        &blockchain,
        query.from_block,
        query.to_block,
        false,
    );
    let current_trust_policy = blockchain
        .state
        .governance
        .params
        .trust_weighting_policy
        .clone();
    let current_quality_policy = blockchain
        .state
        .governance
        .params
        .data_quality_policy
        .clone();
    let default_quality_multipliers = reward_multipliers_from_policy(&current_quality_policy);

    let mut minted_count = 0u64;
    let mut minted_total = 0u128;
    let mut provenance_decision_counts = ProvenanceDecisionCountersResponse::default();
    let mut provenance_minted_totals = ProvenanceMintedTotalsResponse::default();
    let mut trust_class_counts = TrustClassCountersResponse::default();
    let mut quality_band_counts = QualityBandCountersResponse::default();
    let mut trust_weighted_sum = 0.0f64;
    let mut trust_weighted_denominator = 0.0f64;
    let mut quality_weighted_sum = 0.0f64;
    let mut quality_weighted_denominator = 0.0f64;

    for point in &points {
        minted_count = minted_count.saturating_add(point.minted_count);
        minted_total = minted_total.saturating_add(point.minted_total);
        provenance_decision_counts.real = provenance_decision_counts
            .real
            .saturating_add(point.provenance_decision_counts.real);
        provenance_decision_counts.synthetic = provenance_decision_counts
            .synthetic
            .saturating_add(point.provenance_decision_counts.synthetic);
        provenance_decision_counts.unclassified = provenance_decision_counts
            .unclassified
            .saturating_add(point.provenance_decision_counts.unclassified);
        provenance_minted_totals.real = provenance_minted_totals
            .real
            .saturating_add(point.provenance_minted_totals.real);
        provenance_minted_totals.synthetic = provenance_minted_totals
            .synthetic
            .saturating_add(point.provenance_minted_totals.synthetic);
        provenance_minted_totals.unclassified = provenance_minted_totals
            .unclassified
            .saturating_add(point.provenance_minted_totals.unclassified);
        trust_class_counts.unverified = trust_class_counts
            .unverified
            .saturating_add(point.trust_class_counts.unverified);
        trust_class_counts.verified_station = trust_class_counts
            .verified_station
            .saturating_add(point.trust_class_counts.verified_station);
        trust_class_counts.trusted_provider = trust_class_counts
            .trusted_provider
            .saturating_add(point.trust_class_counts.trusted_provider);
        quality_band_counts.high = quality_band_counts
            .high
            .saturating_add(point.quality_band_counts.high);
        quality_band_counts.borderline = quality_band_counts
            .borderline
            .saturating_add(point.quality_band_counts.borderline);
        quality_band_counts.warning_only = quality_band_counts
            .warning_only
            .saturating_add(point.quality_band_counts.warning_only);
        quality_band_counts.low = quality_band_counts
            .low
            .saturating_add(point.quality_band_counts.low);

        let trust_policy = point
            .trust_weights
            .as_ref()
            .unwrap_or(&current_trust_policy);
        trust_weighted_sum +=
            point.trust_class_counts.unverified as f64 * trust_policy.unverified_weight;
        trust_weighted_sum +=
            point.trust_class_counts.verified_station as f64 * trust_policy.verified_station_weight;
        trust_weighted_sum +=
            point.trust_class_counts.trusted_provider as f64 * trust_policy.trusted_provider_weight;
        trust_weighted_denominator += (point.trust_class_counts.unverified
            + point.trust_class_counts.verified_station
            + point.trust_class_counts.trusted_provider)
            as f64;

        let quality_multipliers = point
            .quality_reward_multipliers
            .as_ref()
            .unwrap_or(&default_quality_multipliers);
        quality_weighted_sum += point.quality_band_counts.high as f64 * quality_multipliers.high;
        quality_weighted_sum +=
            point.quality_band_counts.borderline as f64 * quality_multipliers.borderline;
        quality_weighted_sum +=
            point.quality_band_counts.warning_only as f64 * quality_multipliers.warning_only;
        quality_weighted_sum += point.quality_band_counts.low as f64 * quality_multipliers.low;
        quality_weighted_denominator += (point.quality_band_counts.high
            + point.quality_band_counts.borderline
            + point.quality_band_counts.warning_only
            + point.quality_band_counts.low) as f64;
    }

    let estimated_weighted_trust_multiplier = if trust_weighted_denominator > f64::EPSILON {
        trust_weighted_sum / trust_weighted_denominator
    } else {
        0.0
    };
    let estimated_weighted_quality_multiplier = if quality_weighted_denominator > f64::EPSILON {
        quality_weighted_sum / quality_weighted_denominator
    } else {
        0.0
    };

    HttpResponse::Ok().json(TrustRewardImpactSummaryResponse {
        block_count: points.len(),
        minted_count,
        minted_total,
        provenance_decision_counts,
        provenance_minted_totals,
        trust_class_counts,
        quality_band_counts,
        estimated_weighted_trust_multiplier,
        estimated_weighted_quality_multiplier,
        estimated_effective_multiplier: estimated_weighted_trust_multiplier
            * estimated_weighted_quality_multiplier,
        current_trust_policy: current_trust_policy.clone(),
        current_quality_policy: current_quality_policy.clone(),
        joint_multiplier_preview: build_joint_multiplier_matrix(
            &current_trust_policy,
            &default_quality_multipliers,
        ),
    })
}
