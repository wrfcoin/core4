#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/query_cost.rs"]
mod query_cost;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/quality.rs"]
mod quality;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/state.rs"]
mod state;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/rate_limit.rs"]
mod rate_limit;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/websocket.rs"]
mod websocket;

include!("support/launch_api_test_support.inc.rs");

use actix_web::{App, http::StatusCode, test, web};
use wrfcoin_native_chain::blockchain::block::Block;
use wrfcoin_native_chain::blockchain::state::{
    QualityPolicyEventRecord, QualityPolicyEventType,
};
use wrfcoin_shared_protocol::weather::validation::{DataQualityBand, DataQualityPolicy};

fn test_app(
    state: web::Data<state::AppState>,
) -> actix_web::App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    App::new()
        .app_data(state)
        .route(
            "/transactions/quality/events",
            web::get().to(quality::get_quality_events),
        )
        .route(
            "/transactions/quality/participants/{address}/history",
            web::get().to(quality::get_quality_participant_history),
        )
        .route(
            "/transactions/quality/summary",
            web::get().to(quality::get_quality_summary),
        )
        .route(
            "/transactions/rewards/trust/timeline",
            web::get().to(quality::get_trust_reward_impact_timeline),
        )
        .route(
            "/transactions/rewards/trust/summary",
            web::get().to(quality::get_trust_reward_impact_summary),
        )
}

async fn seed_quality_event(
    state: &web::Data<state::AppState>,
    event_id: u64,
    participant: wrfcoin_native_chain::common::types::Address,
    commit_id: &str,
    block_height: u64,
    event_type: QualityPolicyEventType,
    band: DataQualityBand,
) {
    let mut blockchain = state.blockchain.write().await;
    blockchain.state.quality_policy_events.push(QualityPolicyEventRecord {
        event_id,
        event_type,
        block_height,
        commit_id: commit_id.to_string(),
        participant,
        quality_band: band,
        quality_score: None,
        scoring_multiplier: 0.6,
        reward_multiplier: 0.4,
        warning_only: false,
        trigger_challenge: matches!(event_type, QualityPolicyEventType::ChallengeTriggered),
        counts_toward_low_quality_streak: true,
        low_quality_streak: Some(1),
        low_quality_slash_threshold: Some(3),
        slashed_amount: None,
        policy_snapshot: DataQualityPolicy::default(),
    });
}

async fn seed_trust_reward_blocks(state: &web::Data<state::AppState>) {
    let mut blockchain = state.blockchain.write().await;

    let mut block_10 = Block::new(1, [0u8; 32].into(), vec![], 1, 10);
    block_10
        .metadata
        .insert("rewards_minted_count".to_string(), "2".to_string());
    block_10
        .metadata
        .insert("rewards_minted_total".to_string(), "140".to_string());
    block_10.metadata.insert(
        "rewards_trust_unverified_count".to_string(),
        "1".to_string(),
    );
    block_10.metadata.insert(
        "rewards_trust_verified_station_count".to_string(),
        "1".to_string(),
    );
    block_10.metadata.insert(
        "rewards_trust_trusted_provider_count".to_string(),
        "0".to_string(),
    );
    block_10
        .metadata
        .insert("rewards_quality_high_count".to_string(), "1".to_string());
    block_10.metadata.insert(
        "rewards_quality_borderline_count".to_string(),
        "1".to_string(),
    );
    block_10
        .metadata
        .insert("rewards_quality_warning_count".to_string(), "0".to_string());
    block_10.metadata.insert(
        "rewards_skipped_low_quality_count".to_string(),
        "0".to_string(),
    );
    block_10.metadata.insert(
        "rewards_trust_weights".to_string(),
        "unverified=0.60,verified_station=0.80,trusted_provider=1.00".to_string(),
    );
    block_10.metadata.insert(
        "rewards_quality_multipliers".to_string(),
        "borderline(score=0.90,reward=0.80),warning(score=0.60,reward=0.40),low(score=0.30,reward=0.20)".to_string(),
    );
    let hash_10 = block_10.calculate_hash();
    block_10.set_hash(hash_10);
    blockchain
        .add_block(block_10)
        .expect("block 10 should be added");

    let mut block_11 = Block::new(1, hash_10, vec![], 1, 11);
    block_11
        .metadata
        .insert("rewards_minted_count".to_string(), "4".to_string());
    block_11
        .metadata
        .insert("rewards_minted_total".to_string(), "310".to_string());
    block_11.metadata.insert(
        "rewards_trust_unverified_count".to_string(),
        "1".to_string(),
    );
    block_11.metadata.insert(
        "rewards_trust_verified_station_count".to_string(),
        "2".to_string(),
    );
    block_11.metadata.insert(
        "rewards_trust_trusted_provider_count".to_string(),
        "1".to_string(),
    );
    block_11
        .metadata
        .insert("rewards_quality_high_count".to_string(), "2".to_string());
    block_11.metadata.insert(
        "rewards_quality_borderline_count".to_string(),
        "1".to_string(),
    );
    block_11
        .metadata
        .insert("rewards_quality_warning_count".to_string(), "1".to_string());
    block_11.metadata.insert(
        "rewards_skipped_low_quality_count".to_string(),
        "0".to_string(),
    );
    block_11.metadata.insert(
        "rewards_trust_weights".to_string(),
        "unverified=0.60,verified_station=0.80,trusted_provider=1.00".to_string(),
    );
    block_11.metadata.insert(
        "rewards_quality_multipliers".to_string(),
        "borderline(score=0.95,reward=0.90),warning(score=0.70,reward=0.50),low(score=0.35,reward=0.25)".to_string(),
    );
    let hash_11 = block_11.calculate_hash();
    block_11.set_hash(hash_11);
    blockchain
        .add_block(block_11)
        .expect("block 11 should be added");
}

#[actix_web::test]
async fn quality_events_reject_invalid_participant_query_at_production_route() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/transactions/quality/events?participant=not-an-address")
            .insert_header(bearer(TEST_READER_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_enveloped_error::<quality::QualityEventsResponse>(&body, "BAD_REQUEST");
}

#[actix_web::test]
async fn quality_events_collection_uses_exact_production_route() {
    let state = test_state(None);
    seed_quality_event(
        &state,
        1,
        test_address(0x31),
        "commit-001",
        12,
        QualityPolicyEventType::ChallengeTriggered,
        DataQualityBand::Low,
    )
    .await;
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!(
                "/transactions/quality/events?participant={}",
                test_address_string(0x31)
            ))
            .insert_header(bearer(TEST_READER_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: quality::QualityEventsResponse = test::read_body_json(resp).await;
    assert_eq!(body.total, 1);
    assert_eq!(body.items[0].participant, test_address_string(0x31));
    assert_eq!(body.items[0].commit_id, "commit-001");
    assert_eq!(body.items[0].event_type, "challenge");
}

#[actix_web::test]
async fn quality_participant_history_uses_exact_production_route() {
    let state = test_state(None);
    seed_quality_event(
        &state,
        1,
        test_address(0x41),
        "commit-041",
        20,
        QualityPolicyEventType::ChallengeTriggered,
        DataQualityBand::Low,
    )
    .await;
    seed_quality_event(
        &state,
        2,
        test_address(0x42),
        "commit-042",
        21,
        QualityPolicyEventType::SlashingApplied,
        DataQualityBand::Borderline,
    )
    .await;
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!(
                "/transactions/quality/participants/{}/history",
                test_address_string(0x41)
            ))
            .insert_header(bearer(TEST_READER_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: quality::QualityEventsResponse = test::read_body_json(resp).await;
    assert_eq!(body.total, 1);
    assert_eq!(body.items[0].participant, test_address_string(0x41));
    assert_eq!(body.items[0].commit_id, "commit-041");
}

#[actix_web::test]
async fn quality_summary_uses_exact_production_route() {
    let state = test_state(None);
    seed_quality_event(
        &state,
        1,
        test_address(0x51),
        "commit-051",
        30,
        QualityPolicyEventType::ChallengeTriggered,
        DataQualityBand::Low,
    )
    .await;
    let app = test::init_service(test_app(state)).await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!(
                "/transactions/quality/summary?participant={}",
                test_address_string(0x51)
            ))
            .insert_header(bearer(TEST_READER_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: quality::QualitySummaryResponse = test::read_body_json(resp).await;
    assert_eq!(body.total_events, 1);
    assert_eq!(body.challenge_events, 1);
    assert_eq!(body.slashing_events, 0);
    assert_eq!(body.band_counters.low, 1);
    assert_eq!(body.challenge_band_counters.low, 1);
}

/// Denial-path control for the two Reader-gated trust-reward endpoints
/// (#1986 hardening): without it, an accidental public-route regression
/// would pass every authenticated test in this file.
#[actix_web::test]
async fn trust_reward_timeline_and_summary_require_auth() {
    let state = test_state(None);
    let app = test::init_service(test_app(state)).await;

    for uri in [
        "/transactions/rewards/trust/timeline?from_block=10&to_block=11",
        "/transactions/rewards/trust/summary?from_block=10&to_block=11",
    ] {
        let resp = test::call_service(
            &app,
            test::TestRequest::get().uri(uri).to_request(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "{uri} must reject unauthenticated requests"
        );
    }
}

#[actix_web::test]
async fn trust_reward_timeline_and_summary_use_exact_production_routes() {
    let state = test_state(None);
    seed_trust_reward_blocks(&state).await;
    let app = test::init_service(test_app(state)).await;

    let timeline_resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/transactions/rewards/trust/timeline?from_block=10&to_block=11")
            .insert_header(bearer(TEST_READER_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(timeline_resp.status(), StatusCode::OK);
    let timeline: quality::TrustRewardImpactTimelineResponse =
        test::read_body_json(timeline_resp).await;
    assert_eq!(timeline.total, 2);
    assert_eq!(timeline.items[0].block_height, 11);
    assert_eq!(timeline.items[0].trust_class_counts.verified_station, 2);

    let summary_resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/transactions/rewards/trust/summary?from_block=10&to_block=11")
            .insert_header(bearer(TEST_READER_TOKEN))
            .to_request(),
    )
    .await;

    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary: quality::TrustRewardImpactSummaryResponse =
        test::read_body_json(summary_resp).await;
    assert_eq!(summary.block_count, 2);
    assert_eq!(summary.minted_count, 6);
    assert_eq!(summary.minted_total, 450);
    assert_eq!(summary.quality_band_counts.high, 3);
    assert_eq!(summary.quality_band_counts.warning_only, 1);
}
