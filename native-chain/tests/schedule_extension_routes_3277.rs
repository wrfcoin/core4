//! #3277 S4: `POST /transactions/governance/schedule-extensions` admits a
//! governance-signed extension to the mempool only after the same checks the
//! producer makes: operator role, admission policy, the full V1–V9 validity
//! at this node's next height (with a precise reason), then the mempool.
//!
//! Own process: the gate (base schedule) and the authority are process-wide;
//! every test installs identical values and runs under one lock.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/state.rs"]
mod state;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/rate_limit.rs"]
mod rate_limit;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/websocket.rs"]
mod websocket;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/schedule_extension.rs"]
mod schedule_extension;

include!("support/launch_api_test_support.inc.rs");

use actix_web::{App, http::StatusCode, test, web};
use wrfcoin_shared_protocol::crypto::chain_binding::{
    GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion, install_activation_table,
    install_feature_schedule, resolve_activation_schedule, resolve_feature_schedule,
};
use wrfcoin_shared_protocol::governance::schedule_extension::{
    FeatureScheduleExtension, MIN_RUNUP_BLOCKS, SCHEDULE_EXTENSION_FEATURE, ScheduleEntry,
    SignedFeatureScheduleExtension,
    install_schedule_extension_authority_for_tests,
};

const H_ENABLE: u64 = 1;
const ROUTE: &str = "/transactions/governance/schedule-extensions";

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn governance() -> PrivateKey {
    PrivateKey::generate(&mut StdRng::seed_from_u64(3277))
}

async fn gate() -> tokio::sync::MutexGuard<'static, ()> {
    let guard = LOCK.lock().await;
    let raw = format!("0:v1,{H_ENABLE}:{SCHEDULE_EXTENSION_FEATURE}");
    let pairs = vec![(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(pairs.clone(), SigningProtocolVersion::V1)
        .expect("activation table");
    let features = resolve_feature_schedule(pairs, &table).expect("feature schedule");
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    install_schedule_extension_authority_for_tests("777", "testnet", &"ab".repeat(32), vec![
            PublicKey::from_private_key(&governance())
                .to_hex()
                .to_ascii_lowercase(),
        ]);
    guard
}

fn signed(sequence: u64, entries: &[(&str, u64)]) -> SignedFeatureScheduleExtension {
    let extension = FeatureScheduleExtension {
        chain_id: "777".into(),
        network_id: "testnet".into(),
        genesis_hash: "ab".repeat(32),
        sequence,
        entries: entries
            .iter()
            .map(|(name, height)| ScheduleEntry::new(*name, *height))
            .collect(),
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "s4-test-release".into(),
    };
    // A raw preimage signature: the refusal cases include a HELD name
    // (#3361 F3) that every library signer refuses to sign.
    let signatures = vec![wrfcoin_shared_protocol::governance::schedule_extension::GovernanceSignature {
        signer_pubkey: PublicKey::from_private_key(&governance()).to_hex().to_ascii_lowercase(),
        signature: governance().sign(&extension.preimage()).to_hex(),
    }];
    SignedFeatureScheduleExtension {
        extension,
        signatures,
    }
}

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
        .route(ROUTE, web::post().to(schedule_extension::submit_schedule_extension))
}

async fn submit(
    state: &web::Data<state::AppState>,
    token: Option<&str>,
    body: &SignedFeatureScheduleExtension,
) -> (StatusCode, serde_json::Value) {
    let app = test::init_service(test_app(state.clone())).await;
    let mut request = test::TestRequest::post().uri(ROUTE).set_json(body);
    if let Some(token) = token {
        request = request.insert_header(bearer(token));
    }
    let resp = test::call_service(&app, request.to_request()).await;
    let status = resp.status();
    (status, test::read_body_json(resp).await)
}

fn message(body: &serde_json::Value) -> String {
    body["error"]["message"].as_str().unwrap_or_default().to_string()
}

#[actix_web::test]
async fn only_an_operator_may_submit() {
    let _g = gate().await;
    let state = test_state(None);
    let body = signed(0, &[("verify_unit_pricing", 1 + MIN_RUNUP_BLOCKS)]);
    let (status, json) = submit(&state, None, &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_enveloped_error::<schedule_extension::ScheduleExtensionSubmitResponse>(
        &json,
        "UNAUTHORIZED",
    );
    let (status, _) = submit(&state, Some(TEST_CONTRIBUTOR_TOKEN), &body).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(state.mempool.read().await.size(), 0);
}

#[actix_web::test]
async fn a_valid_extension_is_admitted_with_its_carrying_window() {
    let _g = gate().await;
    let state = test_state(None);
    let activation = 1 + MIN_RUNUP_BLOCKS + 50;
    let body = signed(0, &[("verify_unit_pricing", activation)]);
    let (status, json) = submit(&state, Some(TEST_OPERATOR_TOKEN), &body).await;
    assert_eq!(status, StatusCode::CREATED, "{json}");
    let response: schedule_extension::ScheduleExtensionSubmitResponse =
        serde_json::from_value(json).expect("response shape");
    assert_eq!(response.sequence, 0);
    assert_eq!(response.validated_for_height, 1);
    assert_eq!(response.latest_valid_carrying_height, 51);
    let pool = state.mempool.read().await;
    assert_eq!(pool.size(), 1);
    assert_eq!(
        pool.transaction_ids()[0].to_hex(),
        response.transaction_id,
        "the admitted transaction is the one reported"
    );

    // The same artifact again is a mempool duplicate.
    drop(pool);
    let (status, json) = submit(&state, Some(TEST_OPERATOR_TOKEN), &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    // Refused either by id or, once the wall-clock second (part of the tx id)
    // has moved, by the deterministic input outpoint the id derives from.
    let refusal = message(&json);
    assert!(
        refusal.contains("already in mempool") || refusal.contains("Double spend"),
        "{json}"
    );
}

#[actix_web::test]
async fn every_refusal_names_its_reason_and_admits_nothing() {
    let _g = gate().await;
    let state = test_state(None);
    let cases: [(SignedFeatureScheduleExtension, &str); 6] = [
        (
            signed(0, &[("paymaster", 1 + MIN_RUNUP_BLOCKS)]),
            "admission policy",
        ),
        (
            signed(0, &[("faucet_sink_binding_r2", 1 + MIN_RUNUP_BLOCKS)]),
            "has no faucet-binding row in this binary",
        ),
        (
            signed(0, &[("a_b4_feature", 1 + MIN_RUNUP_BLOCKS)]),
            "not implemented by this binary",
        ),
        (
            signed(0, &[("verify_unit_pricing", MIN_RUNUP_BLOCKS)]),
            "minimum run-up",
        ),
        (
            signed(1, &[("verify_unit_pricing", 1 + MIN_RUNUP_BLOCKS)]),
            "is not the next slot (0)",
        ),
        (
            SignedFeatureScheduleExtension {
                signatures: Vec::new(),
                ..signed(0, &[("verify_unit_pricing", 1 + MIN_RUNUP_BLOCKS)])
            },
            "no signatures",
        ),
    ];
    for (body, reason) in cases {
        let (status, json) = submit(&state, Some(TEST_OPERATOR_TOKEN), &body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reason}: {json}");
        assert!(message(&json).contains(reason), "{reason}: {json}");
    }
    assert_eq!(state.mempool.read().await.size(), 0);
}
