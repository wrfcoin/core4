//! #2464 production-path proof for the oracle attester.
//!
//! Unlike the shared-protocol e2e, this target starts the actual Actix route
//! handlers on a TCP listener and drives the exact `oracle_attester` HTTP
//! cycle through authenticated GET and POST requests. It also mines through
//! the production `/blocks/mine` route so the same daemon instance sees both
//! the prompt mempool lane and the deferred mined-block lane.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/mining.rs"]
mod mining;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/oracle.rs"]
mod oracle;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/oracle_attester.rs"]
mod oracle_attester_bin;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/state.rs"]
mod state;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/rate_limit.rs"]
mod rate_limit;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/websocket.rs"]
mod websocket;

include!("support/launch_api_test_support.inc.rs");

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use actix_web::{App, HttpResponse, HttpServer, web};
use wrfcoin_native_chain::blockchain::state::StateTransition;
use wrfcoin_native_chain::blockchain::transaction::{
    CommitPayload, OracleAttestationPayload, RevealPayload, Transaction, TransactionInput,
    TransactionOutput, TransactionType,
};
use wrfcoin_native_chain::crypto::domain_signing::{signing_chain_id, signing_network_id};
use wrfcoin_native_chain::crypto::hash::hash_string;
use wrfcoin_shared_protocol::oracle::attester::Attester;
use wrfcoin_shared_protocol::oracle::mechanical::{MechanicalPolicy, UNCHECKED_CONFIDENCE_CAP};
use wrfcoin_shared_protocol::station_registry::{
    ContributionProvenanceClass, DataQualityTier, StationLocation, WeatherStation,
};
use wrfcoin_shared_protocol::weather::station_reference::DEFAULT_MAX_PAIR_DELTA_SECS;

#[derive(Clone)]
struct ReferenceFixture {
    available: Arc<AtomicBool>,
    attestation_posts: Arc<AtomicUsize>,
}

async fn nws_observations(reference: web::Data<ReferenceFixture>) -> HttpResponse {
    if !reference.available.load(Ordering::SeqCst) {
        return HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "error": "test reference unavailable"
        }));
    }
    HttpResponse::Ok().json(serde_json::json!({
        "type": "FeatureCollection",
        "features": [{
            "properties": {
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "temperature": {"value": 21.0},
                "barometricPressure": {"value": 101350.0},
                "relativeHumidity": {"value": 58.0},
                "windSpeed": {"value": 10.8}
            }
        }]
    }))
}

async fn counted_submit_oracle_attestation(
    req: actix_web::HttpRequest,
    state: web::Data<state::AppState>,
    request: web::Json<oracle::OracleAttestationRequest>,
    fixture: web::Data<ReferenceFixture>,
) -> impl actix_web::Responder {
    fixture.attestation_posts.fetch_add(1, Ordering::SeqCst);
    oracle::submit_oracle_attestation(req, state, request).await
}

fn build_tx(tx_type: TransactionType, tag: &str) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("input-{tag}")),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            amount: 1,
            address: test_address_string(0x44),
            script: "OP_TRUE".to_string(),
            nft_data: None,
        }],
    );
    tx.tx_type = tx_type;
    tx.set_id();
    tx
}

fn weather_commit_and_reveal(tag: &str) -> (CommitPayload, RevealPayload) {
    let timestamp = chrono::Utc::now().timestamp();
    let station_id = format!("PWS-RUNTIME-{tag}");
    let nonce = format!("nonce-{tag}");
    let data = serde_json::json!({
        "quality": 1.0,
        "volume": 1.0,
        "uniqueness": 1.0,
        "temperature_c": 21.5,
        "humidity_percent": 60.0,
        "pressure_hpa": 1014.0,
        "wind_speed_ms": 2.8,
        "precipitation_mm": 0.0,
        "dewpoint_c": 13.5
    });
    let commit_id =
        Transaction::compute_commit_id(&data, &nonce, &station_id, timestamp, "contribution");
    let mut rng = StdRng::seed_from_u64(
        tag.bytes()
            .fold(24_640u64, |acc, byte| acc.saturating_add(byte as u64)),
    );
    let auth_key = PrivateKey::generate(&mut rng);
    let mut commit = CommitPayload {
        commit_id: commit_id.clone(),
        station_id: station_id.clone(),
        timestamp,
        contribution_type: "contribution".to_string(),
        auth_pubkey: None,
        auth_signature: None,
    };
    commit.sign_authorization(&auth_key);
    let reveal = RevealPayload {
        commit_id,
        station_id,
        timestamp,
        contribution_type: "contribution".to_string(),
        data,
        nonce,
        data_cid: None,
        data_hash: None,
        storage_provider: None,
        provenance: None,
        manifest: None,
        verification_receipt: None,
        compute_receipt: None,
        quality_tier: None,
    };
    (commit, reveal)
}

async fn enqueue_reveal(
    app_state: &web::Data<state::AppState>,
    commit: &CommitPayload,
    reveal: &RevealPayload,
) {
    let block_height = app_state.blockchain.read().await.height;
    let mut blockchain = app_state.blockchain.write().await;
    if blockchain
        .state
        .station_registry
        .get(&commit.station_id)
        .is_none()
    {
        blockchain
            .apply_state_transition(StateTransition::ApplyStationRegistration {
                station: WeatherStation {
                    station_id: commit.station_id.clone(),
                    owner_address: test_address_string(0x44),
                    location: StationLocation {
                        latitude: 46.7867,
                        longitude: -92.1005,
                        elevation_meters: Some(210.0),
                    },
                    hardware_model: "runtime-test".to_string(),
                    calibration_date: None,
                    is_active: true,
                    reputation_score: 100,
                    data_quality_tier: DataQualityTier::Gold,
                    provenance_class: ContributionProvenanceClass::Real,
                    ring_public_key: None,
                },
            })
            .expect("seed a Real station through the production state transition");
    }
    blockchain
        .apply_state_transition(StateTransition::RegisterCommit {
            payload: commit.clone(),
            block_height,
        })
        .expect("seed the commit through the production state transition");
    drop(blockchain);

    let mut tx = build_tx(TransactionType::ContributionReveal, &reveal.commit_id[..12]);
    tx.data = Some(serde_json::to_string(reveal).expect("serialize reveal"));
    tx.sender_address = test_address(0x44);
    tx.fee = 1;
    tx.set_id();
    app_state
        .mempool
        .write()
        .await
        .add_transaction(tx)
        .expect("reveal must enter the real mempool");
}

async fn mine(client: &reqwest::Client, base_url: &str) -> (reqwest::StatusCode, String) {
    let response = client
        .post(format!("{base_url}/blocks/mine"))
        .bearer_auth(TEST_OPERATOR_TOKEN)
        .json(&serde_json::json!({
            "miner_address": test_address_string(0x55)
        }))
        .send()
        .await
        .expect("mine request reaches launch_api");
    let status = response.status();
    let body = response.text().await.expect("mine response body");
    (status, body)
}

async fn admitted_attestation(
    app_state: &web::Data<state::AppState>,
    commit_id: &str,
) -> OracleAttestationPayload {
    app_state
        .mempool
        .read()
        .await
        .get_transactions_by_type(TransactionType::OracleAttestation, 100)
        .into_iter()
        .filter_map(|tx| tx.parse_oracle_attestation_payload())
        .find(|payload| payload.commit_id == commit_id)
        .expect("attester POST must admit an OracleAttestation transaction")
}

#[actix_web::test]
async fn real_http_cycle_covers_auth_retry_restart_prompt_and_deferred_lanes() {
    let initial_state = test_state(None);
    let mut inner = initial_state.into_inner();
    let pop_config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
    let pop_adapter = ConsensusFactory::create_pop_adapter(&pop_config);
    pop_adapter.declare_no_genesis_oracles_blocking();
    Arc::get_mut(&mut inner)
        .expect("test AppState must have one owner before the server starts")
        .consensus = Arc::new(TypedConsensusManager::new(pop_adapter, pop_config));
    let app_state = web::Data::from(inner);
    let mut rng = StdRng::seed_from_u64(27_460);
    let oracle_key = PrivateKey::generate(&mut rng);
    let oracle_id = PublicKey::from_private_key(&oracle_key).to_hex();
    let attester = Attester::new(Arc::new(EnvKeySigner::from_private_key(
        oracle_key,
        "WRFCOIN_ORACLE_SIGNER_PRIVATE_KEY",
    )));
    app_state
        .blockchain
        .write()
        .await
        .apply_state_transition(StateTransition::SetOracleConfig {
            oracle_set: vec![oracle_id],
            oracle_quorum: 1,
        })
        .expect("initialize the oracle set before block 1");

    let reference = ReferenceFixture {
        available: Arc::new(AtomicBool::new(false)),
        attestation_posts: Arc::new(AtomicUsize::new(0)),
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral launch_api port");
    let address = listener.local_addr().expect("read launch_api port");
    let server_state = app_state.clone();
    let server_reference = reference.clone();
    let server = HttpServer::new(move || {
        App::new()
            .app_data(server_state.clone())
            .app_data(web::Data::new(server_reference.clone()))
            .route(
                "/transactions/oracle/pending",
                web::get().to(oracle::get_pending_attestations),
            )
            .route(
                "/transactions/oracle/attestation",
                web::post().to(counted_submit_oracle_attestation),
            )
            .route("/blocks/mine", web::post().to(mining::mine_block))
            .route(
                "/stations/{station}/observations",
                web::get().to(nws_observations),
            )
    })
    .listen(listener)
    .expect("listen on launch_api port")
    .run();
    let server_handle = server.handle();
    actix_web::rt::spawn(server);

    let base_url = format!("http://{address}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("build test HTTP client");

    // Authentication controls on the actual read and write routes.
    let no_auth = client
        .get(format!("{base_url}/transactions/oracle/pending"))
        .send()
        .await
        .expect("unauthenticated GET reaches launch_api");
    assert_eq!(no_auth.status(), reqwest::StatusCode::UNAUTHORIZED);
    let reader_get = client
        .get(format!("{base_url}/transactions/oracle/pending"))
        .bearer_auth(TEST_READER_TOKEN)
        .send()
        .await
        .expect("Reader GET reaches launch_api");
    assert_eq!(reader_get.status(), reqwest::StatusCode::OK);
    let reader_post = client
        .post(format!("{base_url}/transactions/oracle/attestation"))
        .bearer_auth(TEST_READER_TOKEN)
        .json(&serde_json::json!({
            "oracle_id": attester.oracle_id(),
            "commit_hash": "a".repeat(64),
            "approved": true,
            "signature": "00".repeat(64),
            "confidence": 0.5,
            "timestamp": chrono::Utc::now().timestamp(),
            "nonce": "reader-must-not-write"
        }))
        .send()
        .await
        .expect("Reader POST reaches launch_api");
    assert_eq!(reader_post.status(), reqwest::StatusCode::FORBIDDEN);
    let posts_before_cycle = reference.attestation_posts.load(Ordering::SeqCst);

    let policy = MechanicalPolicy::default();
    let reader_config = oracle_attester_bin::Config::for_test(
        &base_url,
        TEST_READER_TOKEN,
        signing_chain_id(),
        signing_network_id(),
        Some("KDLH".to_string()),
        &base_url,
        DEFAULT_MAX_PAIR_DELTA_SECS,
    );
    let oracle_config = oracle_attester_bin::Config::for_test(
        &base_url,
        TEST_ORACLE_TOKEN,
        signing_chain_id(),
        signing_network_id(),
        Some("KDLH".to_string()),
        &base_url,
        DEFAULT_MAX_PAIR_DELTA_SECS,
    );

    // Prompt lane. NWS is deliberately unavailable, so the real service must
    // still submit but visibly cap confidence. A Reader token can discover the
    // reveal but cannot POST it; the next Oracle-role cycle must retry.
    let (prompt_commit, prompt_reveal) = weather_commit_and_reveal("prompt");
    enqueue_reveal(&app_state, &prompt_commit, &prompt_reveal).await;
    let prompt_pending: serde_json::Value = client
        .get(format!("{base_url}/transactions/oracle/pending"))
        .bearer_auth(TEST_READER_TOKEN)
        .send()
        .await
        .expect("prompt pending GET")
        .json()
        .await
        .expect("prompt pending JSON");
    assert_eq!(prompt_pending["observations"][0]["source"], "mempool");
    // #3285: the station's registered elevation rides along for the
    // attester's sea-level pressure recompute.
    assert_eq!(prompt_pending["observations"][0]["elevation_meters"], 210.0);

    let mut failed_cycle_cache: HashMap<String, Instant> = HashMap::new();
    assert_eq!(
        oracle_attester_bin::run_cycle(
            &client,
            &reader_config,
            &attester,
            &policy,
            &mut failed_cycle_cache,
        )
        .await
        .expect("a rejected POST is a completed cycle, not a daemon crash"),
        0
    );
    assert!(
        failed_cycle_cache.is_empty(),
        "failed POST must remain retryable"
    );
    assert_eq!(
        reference.attestation_posts.load(Ordering::SeqCst),
        posts_before_cycle + 1,
        "Reader-role cycle must reach the production POST handler and be rejected"
    );

    let mut prompt_cache: HashMap<String, Instant> = HashMap::new();
    assert_eq!(
        oracle_attester_bin::run_cycle(
            &client,
            &oracle_config,
            &attester,
            &policy,
            &mut prompt_cache,
        )
        .await
        .expect("Oracle-role prompt cycle succeeds"),
        1
    );
    let prompt_attestation = admitted_attestation(&app_state, &prompt_reveal.commit_id).await;
    assert_eq!(
        reference.attestation_posts.load(Ordering::SeqCst),
        posts_before_cycle + 2,
        "Oracle-role retry must reach and pass the production POST handler"
    );
    assert!(
        prompt_attestation.confidence.expect("confidence is signed") <= UNCHECKED_CONFIDENCE_CAP,
        "NWS outage must take the visible physics-only confidence path"
    );

    // Simulate a process restart: local suppression is empty. The pending
    // endpoint must expose the admitted mempool attestation so the restarted
    // daemon does not generate a fresh duplicate before the block is mined.
    let mut restarted_cache: HashMap<String, Instant> = HashMap::new();
    assert_eq!(
        oracle_attester_bin::run_cycle(
            &client,
            &oracle_config,
            &attester,
            &policy,
            &mut restarted_cache,
        )
        .await
        .expect("restarted prompt cycle succeeds"),
        0,
        "already-admitted mempool work must suppress a post-restart duplicate"
    );
    assert_eq!(
        reference.attestation_posts.load(Ordering::SeqCst),
        posts_before_cycle + 2,
        "restart suppression must skip the duplicate before POST, not merely rely on API rejection"
    );

    let (prompt_mine_status, prompt_mine_body) = mine(&client, &base_url).await;
    assert_eq!(
        prompt_mine_status,
        reqwest::StatusCode::CREATED,
        "{prompt_mine_body}"
    );

    // Deferred lane: mine a reveal while the daemon is absent, then make the
    // reference endpoint available. The same HTTP cycle must discover source
    // `block`, pair the NWS clock, submit, and mine the late attestation.
    let (deferred_commit, deferred_reveal) = weather_commit_and_reveal("deferred");
    enqueue_reveal(&app_state, &deferred_commit, &deferred_reveal).await;
    let (reveal_only_mine_status, reveal_only_mine_body) = mine(&client, &base_url).await;
    assert_eq!(
        reveal_only_mine_status,
        reqwest::StatusCode::CREATED,
        "{reveal_only_mine_body}"
    );

    let deferred_pending: serde_json::Value = client
        .get(format!("{base_url}/transactions/oracle/pending"))
        .bearer_auth(TEST_READER_TOKEN)
        .send()
        .await
        .expect("deferred pending GET")
        .json()
        .await
        .expect("deferred pending JSON");
    let deferred_entry = deferred_pending["observations"]
        .as_array()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["commit_id"] == deferred_reveal.commit_id)
        })
        .unwrap_or_else(|| {
            panic!("mined reveal must remain discoverable: {deferred_pending}")
        });
    assert_eq!(deferred_entry["source"], "block");
    assert!(deferred_entry["revealed_at_height"].as_u64().is_some());

    reference.available.store(true, Ordering::SeqCst);
    let mut deferred_cache: HashMap<String, Instant> = HashMap::new();
    assert_eq!(
        oracle_attester_bin::run_cycle(
            &client,
            &oracle_config,
            &attester,
            &policy,
            &mut deferred_cache,
        )
        .await
        .expect("deferred HTTP cycle succeeds"),
        1
    );
    let deferred_attestation = admitted_attestation(&app_state, &deferred_reveal.commit_id).await;
    assert_eq!(
        reference.attestation_posts.load(Ordering::SeqCst),
        posts_before_cycle + 3,
        "deferred lane must submit through the same production POST handler"
    );
    assert!(
        deferred_attestation
            .confidence
            .expect("confidence is signed")
            > UNCHECKED_CONFIDENCE_CAP,
        "clock-paired NWS reference must escape the physics-only cap"
    );
    let (deferred_mine_status, deferred_mine_body) = mine(&client, &base_url).await;
    assert_eq!(
        deferred_mine_status,
        reqwest::StatusCode::CREATED,
        "{deferred_mine_body}"
    );

    server_handle.stop(true).await;
}

// ---------------------------------------------------------------------------
// #3240: per-commit NWS reference selection, driven end to end.
//
// The node side is the production `/transactions/oracle/pending` handler over
// a real chain-state station registry; the attester side is the production
// HTTP cycle. One Actix server also plays api.weather.gov (`/points`,
// `/gridpoints/../stations`, `/stations/{id}`, `/stations/{id}/observations`),
// ordering candidates by distance the way NWS does. Everything above the
// `per_station_3240` module compiles against main as well, which is what makes
// the dark-ship golden a before/after proof rather than a self-comparison.
// ---------------------------------------------------------------------------

/// Test-fixture coordinates. Synthetic station ids so nothing here reads as a
/// claim about a real site.
const DULUTH_SITE_3240: (f64, f64) = (46.7867, -92.1005);
const REMOTE_SITE_3240: (f64, f64) = (41.95, -77.10);
/// (id, latitude, longitude, temperature °C, relative humidity %)
const KDLH_3240: (&str, f64, f64, f64, f64) = ("KDLH", 46.8369, -92.1833, 21.0, 58.0);
const KTGA_3240: (&str, f64, f64, f64, f64) = ("KTGA", 41.97, -77.12, 30.5, 93.0);

#[derive(Clone)]
struct NwsFixture3240 {
    /// (id, latitude, longitude, temperature °C, relative humidity %)
    stations: Arc<Vec<(String, f64, f64, f64, f64)>>,
    points_delay: Option<std::time::Duration>,
    /// When set, `points_delay` applies only to these point keys ("lat,lon").
    points_delay_only: Option<Vec<String>>,
    /// Stations whose `/observations` hang for this long (review N1).
    observations_delay: Option<(Vec<String>, std::time::Duration)>,
    points_requests: Arc<AtomicUsize>,
    /// Observation fetches per station id, in request order.
    observation_requests: Arc<std::sync::Mutex<Vec<String>>>,
}

impl NwsFixture3240 {
    fn new(stations: &[(&str, f64, f64, f64, f64)]) -> Self {
        Self {
            stations: Arc::new(
                stations
                    .iter()
                    .map(|(id, lat, lon, t, rh)| (id.to_string(), *lat, *lon, *t, *rh))
                    .collect(),
            ),
            points_delay: None,
            points_delay_only: None,
            observations_delay: None,
            points_requests: Arc::new(AtomicUsize::new(0)),
            observation_requests: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    fn observation_fetches(&self, id: &str) -> usize {
        self.observation_requests
            .lock()
            .expect("fixture lock")
            .iter()
            .filter(|s| s.as_str() == id)
            .count()
    }
}

fn station_feature_3240(id: &str, lat: f64, lon: f64) -> serde_json::Value {
    serde_json::json!({
        "properties": {"stationIdentifier": id},
        "geometry": {"type": "Point", "coordinates": [lon, lat]}
    })
}

async fn nws_points_3240(
    point: web::Path<String>,
    nws: web::Data<NwsFixture3240>,
) -> HttpResponse {
    nws.points_requests.fetch_add(1, Ordering::SeqCst);
    if let Some(delay) = nws.points_delay {
        let selected = nws
            .points_delay_only
            .as_ref()
            .is_none_or(|only| only.iter().any(|p| p == point.as_str()));
        if selected {
            actix_web::rt::time::sleep(delay).await;
        }
    }
    // Absolute URL, as NWS returns it; the attester re-roots it onto its base.
    HttpResponse::Ok().json(serde_json::json!({
        "properties": {
            "observationStations":
                format!("https://api.weather.gov/gridpoints/TST/{point}/stations")
        }
    }))
}

async fn nws_grid_stations_3240(
    point: web::Path<String>,
    nws: web::Data<NwsFixture3240>,
) -> HttpResponse {
    let mut parts = point.split(',').map(|v| v.parse::<f64>());
    let (Some(Ok(lat)), Some(Ok(lon))) = (parts.next(), parts.next()) else {
        return HttpResponse::BadRequest().finish();
    };
    // Independent of the code under test: rank by geo-grid's haversine.
    let km = |s: &(String, f64, f64, f64, f64)| wrfcoin_geo_grid::haversine_km((lat, lon), (s.1, s.2));
    let mut ranked: Vec<_> = nws.stations.iter().collect();
    ranked.sort_by(|a, b| km(a).total_cmp(&km(b)));
    HttpResponse::Ok().json(serde_json::json!({
        "type": "FeatureCollection",
        "features": ranked
            .iter()
            .map(|(id, lat, lon, _, _)| station_feature_3240(id, *lat, *lon))
            .collect::<Vec<_>>()
    }))
}

async fn nws_station_3240(id: web::Path<String>, nws: web::Data<NwsFixture3240>) -> HttpResponse {
    match nws.stations.iter().find(|s| s.0 == *id) {
        Some((id, lat, lon, _, _)) => HttpResponse::Ok().json(station_feature_3240(id, *lat, *lon)),
        None => HttpResponse::NotFound().finish(),
    }
}

async fn nws_station_observations_3240(
    id: web::Path<String>,
    nws: web::Data<NwsFixture3240>,
) -> HttpResponse {
    nws.observation_requests
        .lock()
        .expect("fixture lock")
        .push(id.to_string());
    if let Some((slow, delay)) = &nws.observations_delay {
        if slow.iter().any(|s| s == id.as_str()) {
            actix_web::rt::time::sleep(*delay).await;
        }
    }
    let Some((_, _, _, temperature, humidity)) = nws.stations.iter().find(|s| s.0 == *id) else {
        return HttpResponse::NotFound().finish();
    };
    HttpResponse::Ok().json(serde_json::json!({
        "type": "FeatureCollection",
        "features": [{
            "properties": {
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "temperature": {"value": temperature},
                "barometricPressure": {"value": 101350.0},
                "relativeHumidity": {"value": humidity},
                "windSpeed": {"value": 10.8}
            }
        }]
    }))
}

struct Harness3240 {
    app_state: web::Data<state::AppState>,
    base_url: String,
    client: reqwest::Client,
    attester: Attester,
    server_handle: actix_web::dev::ServerHandle,
    /// (timestamp, commit_id) of the last reveal. The pending route serves
    /// entries sorted by that pair, so each new reveal is made to sort after
    /// the previous one: tests then control the order the attester sees.
    last_order: std::sync::Mutex<Option<(i64, String)>>,
    /// Fixture time (#3369): every reveal is stamped with it and the attester's evaluation clock is
    /// pinned to it, so confidence never decays with how long the test takes.
    fixture_now: i64,
}

async fn start_harness_3240(nws: NwsFixture3240, seed: u64) -> Harness3240 {
    let initial_state = test_state(None);
    let mut inner = initial_state.into_inner();
    let pop_config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
    let pop_adapter = ConsensusFactory::create_pop_adapter(&pop_config);
    pop_adapter.declare_no_genesis_oracles_blocking();
    Arc::get_mut(&mut inner)
        .expect("test AppState must have one owner before the server starts")
        .consensus = Arc::new(TypedConsensusManager::new(pop_adapter, pop_config));
    let app_state = web::Data::from(inner);
    let mut rng = StdRng::seed_from_u64(seed);
    let oracle_key = PrivateKey::generate(&mut rng);
    let oracle_id = PublicKey::from_private_key(&oracle_key).to_hex();
    let attester = Attester::new(Arc::new(EnvKeySigner::from_private_key(
        oracle_key,
        "WRFCOIN_ORACLE_SIGNER_PRIVATE_KEY",
    )));
    app_state
        .blockchain
        .write()
        .await
        .apply_state_transition(StateTransition::SetOracleConfig {
            oracle_set: vec![oracle_id],
            oracle_quorum: 1,
        })
        .expect("initialize the oracle set before block 1");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let address = listener.local_addr().expect("read port");
    let server_state = app_state.clone();
    let server = HttpServer::new(move || {
        App::new()
            .app_data(server_state.clone())
            .app_data(web::Data::new(nws.clone()))
            .route(
                "/transactions/oracle/pending",
                web::get().to(oracle::get_pending_attestations),
            )
            .route(
                "/transactions/oracle/attestation",
                web::post().to(oracle::submit_oracle_attestation),
            )
            .route("/points/{point}", web::get().to(nws_points_3240))
            .route(
                "/gridpoints/TST/{point}/stations",
                web::get().to(nws_grid_stations_3240),
            )
            .route("/stations/{id}", web::get().to(nws_station_3240))
            .route(
                "/stations/{id}/observations",
                web::get().to(nws_station_observations_3240),
            )
    })
    .workers(2)
    .listen(listener)
    .expect("listen")
    .run();
    let server_handle = server.handle();
    actix_web::rt::spawn(server);
    // #3369: one fixture time for the reveals and the attester's evaluation clock.
    let fixture_now = chrono::Utc::now().timestamp();
    oracle_attester_bin::pin_evaluation_clock(fixture_now);
    Harness3240 {
        app_state,
        base_url: format!("http://{address}"),
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("build test HTTP client"),
        attester,
        server_handle,
        last_order: std::sync::Mutex::new(None),
        fixture_now,
    }
}

impl Harness3240 {
    fn fixed_config(&self) -> oracle_attester_bin::Config {
        oracle_attester_bin::Config::for_test(
            &self.base_url,
            TEST_ORACLE_TOKEN,
            signing_chain_id(),
            signing_network_id(),
            Some("KDLH".to_string()),
            &self.base_url,
            DEFAULT_MAX_PAIR_DELTA_SECS,
        )
    }

    /// Seed one weather reveal for `station_id` into the real mempool,
    /// registering the station at `site` first (or leaving it unregistered).
    async fn reveal(
        &self,
        station_id: &str,
        site: Option<(f64, f64)>,
        temperature_c: f64,
        humidity_percent: f64,
    ) -> String {
        // Stamped with the pinned fixture time (#3369): see
        // `golden_is_independent_of_run_delay_3369`.
        let timestamp = self.fixture_now;
        let data = serde_json::json!({
            "quality": 1.0,
            "volume": 1.0,
            "uniqueness": 1.0,
            "temperature_c": temperature_c,
            // `humidity` is the canonical meteo-types key (`humidity_percent`
            // is not read), so this variable is actually reference-compared.
            "humidity": humidity_percent,
            "pressure_hpa": 1014.0,
            "wind_speed_ms": 2.8,
            "precipitation_mm": 0.0,
            "dewpoint_c": 13.5
        });
        // Pick the first nonce whose (timestamp, commit_id) sorts after the
        // previous reveal's — the attester then sees reveals in call order.
        let mut last = self.last_order.lock().expect("order lock");
        let (nonce, commit_id) = (0u32..)
            .map(|k| {
                let nonce = format!("nonce-3240-{station_id}-{k}");
                let id = Transaction::compute_commit_id(
                    &data,
                    &nonce,
                    station_id,
                    timestamp,
                    "contribution",
                );
                (nonce, id)
            })
            .find(|(_, id)| last.as_ref().is_none_or(|(ts, prev)| (timestamp, id) > (*ts, prev)))
            .expect("some nonce sorts after the previous reveal");
        *last = Some((timestamp, commit_id.clone()));
        drop(last);
        let mut rng = StdRng::seed_from_u64(
            station_id.bytes().fold(32_400u64, |acc, b| acc.saturating_add(b as u64)),
        );
        let mut commit = CommitPayload {
            commit_id: commit_id.clone(),
            station_id: station_id.to_string(),
            timestamp,
            contribution_type: "contribution".to_string(),
            auth_pubkey: None,
            auth_signature: None,
        };
        commit.sign_authorization(&PrivateKey::generate(&mut rng));
        let reveal = RevealPayload {
            commit_id: commit_id.clone(),
            station_id: station_id.to_string(),
            timestamp,
            contribution_type: "contribution".to_string(),
            data,
            nonce,
            data_cid: None,
            data_hash: None,
            storage_provider: None,
            provenance: None,
            manifest: None,
            verification_receipt: None,
            compute_receipt: None,
            quality_tier: None,
        };
        let block_height = self.app_state.blockchain.read().await.height;
        let mut blockchain = self.app_state.blockchain.write().await;
        if let Some((latitude, longitude)) = site {
            blockchain
                .apply_state_transition(StateTransition::ApplyStationRegistration {
                    station: WeatherStation {
                        station_id: station_id.to_string(),
                        owner_address: test_address_string(0x44),
                        location: StationLocation {
                            latitude,
                            longitude,
                            elevation_meters: None,
                        },
                        hardware_model: "runtime-test-3240".to_string(),
                        calibration_date: None,
                        is_active: true,
                        reputation_score: 100,
                        data_quality_tier: DataQualityTier::Gold,
                        provenance_class: ContributionProvenanceClass::Real,
                        ring_public_key: None,
                    },
                })
                .expect("register the station through the production transition");
        }
        blockchain
            .apply_state_transition(StateTransition::RegisterCommit {
                payload: commit,
                block_height,
            })
            .expect("seed the commit through the production transition");
        drop(blockchain);
        let mut tx = build_tx(TransactionType::ContributionReveal, &commit_id[..12]);
        tx.data = Some(serde_json::to_string(&reveal).expect("serialize reveal"));
        tx.sender_address = test_address(0x44);
        tx.fee = 1;
        tx.set_id();
        self.app_state
            .mempool
            .write()
            .await
            .add_transaction(tx)
            .expect("reveal must enter the real mempool");
        commit_id
    }

    async fn pending(&self) -> serde_json::Value {
        self.client
            .get(format!("{}/transactions/oracle/pending", self.base_url))
            .bearer_auth(TEST_READER_TOKEN)
            .send()
            .await
            .expect("pending GET")
            .json()
            .await
            .expect("pending JSON")
    }

    /// (approved, confidence) the node admitted for `commit_id`.
    async fn verdict(&self, commit_id: &str) -> (bool, f64) {
        let payload = admitted_attestation(&self.app_state, commit_id).await;
        (payload.approved, payload.confidence.expect("confidence is signed"))
    }
}

/// #3240 route surface: a registered station's entry carries
/// `station_location` (coarsened to 0.01°), an unregistered station's entry
/// omits the key entirely. Asserted on the raw JSON so the same test runs
/// against main (where it is RED: the key does not exist there).
#[actix_web::test]
async fn pending_route_exposes_station_location_for_registered_only_3240() {
    let h = start_harness_3240(NwsFixture3240::new(&[KDLH_3240]), 32_401).await;
    let registered = h.reveal("PWS-3240-REG", Some((46.786_74, -92.100_49)), 21.5, 60.0).await;
    let unknown = h.reveal("PWS-3240-UNREG", None, 21.5, 60.0).await;
    let pending = h.pending().await;
    let entry = |commit_id: &str| {
        pending["observations"]
            .as_array()
            .and_then(|all| all.iter().find(|e| e["commit_id"] == commit_id))
            .cloned()
            .unwrap_or_else(|| panic!("{commit_id} missing from {pending}"))
    };
    let reg = entry(&registered);
    assert_eq!(
        reg["station_location"],
        serde_json::json!({"latitude": 46.79, "longitude": -92.1}),
        "registered station must expose its COARSENED registry location: {reg}"
    );
    let unreg = entry(&unknown);
    assert!(
        unreg.as_object().is_some_and(|o| !o.contains_key("station_location")),
        "an unregistered station's entry must omit station_location: {unreg}"
    );
    h.server_handle.stop(false).await;
}

/// #3240 dark ship: with no new configuration the attester is `fixed` and
/// ignores location entirely. A remote registered station is still paired
/// against the configured KDLH (Outlier for honest remote weather, exactly as
/// on main), no `/points` lookup is made, and the admitted verdicts match the
/// golden values pinned from main. This test compiles and passes on main
/// unchanged — the before/after proof that merging changes no reward.
#[actix_web::test]
async fn default_mode_is_fixed_and_identical_to_pre_change_3240() {
    let nws = NwsFixture3240::new(&[KDLH_3240, KTGA_3240]);
    let h = start_harness_3240(nws.clone(), 32_402).await;
    let local = h.reveal("PWS-3240-DULUTH", Some(DULUTH_SITE_3240), 21.5, 60.0).await;
    let remote = h.reveal("PWS-3240-REMOTE", Some(REMOTE_SITE_3240), 31.0, 95.0).await;
    let mut cache: HashMap<String, Instant> = HashMap::new();
    let submitted = oracle_attester_bin::run_cycle(
        &h.client,
        &h.fixed_config(),
        &h.attester,
        &MechanicalPolicy::default(),
        &mut cache,
    )
    .await
    .expect("default-mode cycle");
    assert_eq!(submitted, 2);
    assert_eq!(nws.points_requests.load(Ordering::SeqCst), 0, "fixed mode must not look up points");
    assert_eq!(nws.observation_fetches("KTGA"), 0, "fixed mode must not fetch a per-station reference");
    assert!(nws.observation_fetches("KDLH") >= 1);

    let (local_ok, local_conf) = h.verdict(&local).await;
    let (remote_ok, remote_conf) = h.verdict(&remote).await;
    // Golden values, pinned by running this exact test on main (see PR).
    assert!(local_ok && remote_ok);
    assert_eq!(local_conf.to_bits(), GOLDEN_LOCAL_CONFIDENCE_3240.to_bits(), "local {local_conf}");
    assert_eq!(
        remote_conf.to_bits(),
        GOLDEN_REMOTE_CONFIDENCE_3240.to_bits(),
        "remote {remote_conf}"
    );
    // And the remote station is exactly the location-blind defect #3240
    // describes: honest 31 °C / 95 % scored against Duluth's 21 °C / 58 %.
    assert!(remote_conf < local_conf);
    h.server_handle.stop(false).await;
}

/// #3369: the attester evaluates freshness against its evaluation clock and confidence decays linearly
/// with a reading's age. The harness stamps every reveal with one fixture time and pins the attester's
/// evaluation clock to it (`pin_evaluation_clock`, test builds only; production policy untouched), so
/// the golden holds however long the test runs. Delay between seeding and evaluation must not matter.
#[actix_web::test]
async fn golden_is_independent_of_run_delay_3369() {
    let nws = NwsFixture3240::new(&[KDLH_3240]);
    let h = start_harness_3240(nws, 33_690).await;
    let local = h
        .reveal("PWS-3369", Some(DULUTH_SITE_3240), 21.5, 60.0)
        .await;
    // Cross a wall-clock second boundary (the old failure mode) before the cycle evaluates.
    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    let mut cache: HashMap<String, Instant> = HashMap::new();
    let submitted = oracle_attester_bin::run_cycle(
        &h.client,
        &h.fixed_config(),
        &h.attester,
        &MechanicalPolicy::default(),
        &mut cache,
    )
    .await
    .expect("cycle");
    assert_eq!(submitted, 1);
    let (ok, conf) = h.verdict(&local).await;
    assert!(ok);
    assert_eq!(
        conf.to_bits(),
        GOLDEN_LOCAL_CONFIDENCE_3240.to_bits(),
        "confidence {conf}"
    );
    h.server_handle.stop(false).await;
}

/// Pinned from `default_mode_is_fixed_and_identical_to_pre_change_3240` run
/// on origin/main 24237c2d6 (before this change): the local station is Good
/// (1.0), the honest remote station is Outlier (×0.1).
const GOLDEN_LOCAL_CONFIDENCE_3240: f64 = 1.0;
const GOLDEN_REMOTE_CONFIDENCE_3240: f64 = 0.1;

/// Per-station selection (`shadow` / `per_station`). These use the #3240
/// configuration API and so do not exist on main.
mod per_station_3240 {
    use super::*;
    use oracle_attester_bin::{ReferenceMode, ReferenceResolver};

    async fn cycle(
        h: &Harness3240,
        config: &oracle_attester_bin::Config,
        resolver: &mut ReferenceResolver,
    ) -> usize {
        let mut cache: HashMap<String, Instant> = HashMap::new();
        oracle_attester_bin::run_cycle_with_resolver(
            &h.client,
            config,
            &h.attester,
            &MechanicalPolicy::default(),
            &mut cache,
            resolver,
        )
        .await
        .expect("cycle completes")
    }

    /// Each city is checked against its OWN nearest NWS station: the remote
    /// station that is Outlier (0.1) under `fixed` is Good (1.0) here, and
    /// both KDLH and KTGA are actually fetched.
    #[actix_web::test]
    async fn per_station_two_cities_pair_with_own_reference_3240() {
        let nws = NwsFixture3240::new(&[KDLH_3240, KTGA_3240]);
        let h = start_harness_3240(nws.clone(), 32_410).await;
        let local = h.reveal("PWS-3240-DULUTH", Some(DULUTH_SITE_3240), 21.5, 60.0).await;
        let remote = h.reveal("PWS-3240-REMOTE", Some(REMOTE_SITE_3240), 31.0, 95.0).await;
        let config = h.fixed_config().with_reference_selection(ReferenceMode::PerStation, 25.0, 8);
        assert_eq!(cycle(&h, &config, &mut ReferenceResolver::default()).await, 2);
        assert_eq!(h.verdict(&local).await, (true, 1.0));
        assert_eq!(
            h.verdict(&remote).await,
            (true, 1.0),
            "remote station must be tiered against its own reference, not Duluth's"
        );
        assert!(nws.observation_fetches("KDLH") >= 1);
        assert!(nws.observation_fetches("KTGA") >= 1);
        h.server_handle.stop(false).await;
    }

    /// No NWS station within the radius ⇒ capped PHYSICS-ONLY (0.5), never
    /// Outlier (0.1). Mutation control in the same test: the identical setup
    /// with the radius effectively removed pairs against the far station and
    /// lands in Outlier — so the radius check is what makes the difference.
    #[actix_web::test]
    async fn per_station_remote_station_is_physics_only_not_outlier_3240() {
        for (max_km, want, what) in [
            (25.0, UNCHECKED_CONFIDENCE_CAP, "radius enforced ⇒ physics-only cap"),
            (1.0e6, GOLDEN_REMOTE_CONFIDENCE_3240, "radius removed ⇒ Outlier (mutation control)"),
        ] {
            let nws = NwsFixture3240::new(&[KDLH_3240]);
            let h = start_harness_3240(nws.clone(), 32_411).await;
            let remote = h.reveal("PWS-3240-REMOTE", Some(REMOTE_SITE_3240), 31.0, 95.0).await;
            let config =
                h.fixed_config().with_reference_selection(ReferenceMode::PerStation, max_km, 8);
            assert_eq!(cycle(&h, &config, &mut ReferenceResolver::default()).await, 1);
            let (approved, confidence) = h.verdict(&remote).await;
            assert!(approved, "{what}: the reference never flips approval");
            assert_eq!(confidence.to_bits(), want.to_bits(), "{what}: got {confidence}");
            h.server_handle.stop(false).await;
        }
    }

    /// `shadow` submits exactly the `fixed` verdict (the pinned golden) while
    /// still doing the per-station lookup it logs.
    #[actix_web::test]
    async fn shadow_mode_submits_fixed_verdict_3240() {
        let nws = NwsFixture3240::new(&[KDLH_3240, KTGA_3240]);
        let h = start_harness_3240(nws.clone(), 32_412).await;
        let local = h.reveal("PWS-3240-DULUTH", Some(DULUTH_SITE_3240), 21.5, 60.0).await;
        let remote = h.reveal("PWS-3240-REMOTE", Some(REMOTE_SITE_3240), 31.0, 95.0).await;
        let config = h.fixed_config().with_reference_selection(ReferenceMode::Shadow, 25.0, 8);
        let mut resolver = ReferenceResolver::default();
        assert_eq!(cycle(&h, &config, &mut resolver).await, 2);
        assert_eq!(h.verdict(&local).await, (true, GOLDEN_LOCAL_CONFIDENCE_3240));
        assert_eq!(
            h.verdict(&remote).await,
            (true, GOLDEN_REMOTE_CONFIDENCE_3240),
            "shadow must sign the fixed verdict, not the per-station one"
        );
        assert!(nws.points_requests.load(Ordering::SeqCst) >= 1, "shadow did no lookup");
        assert!(nws.observation_fetches("KTGA") >= 1, "shadow never fetched the per-station reference");
        assert!(resolver.lookup_requests >= 1);
        h.server_handle.stop(false).await;
    }

    /// A hanging `/points` lookup is bounded by the named timeout, caps the
    /// affected commits PHYSICS-ONLY, and costs ONE timeout per cycle — not
    /// one per commit — so the poll loop is not stalled.
    #[actix_web::test]
    async fn points_lookup_timeout_caps_and_does_not_stall_cycle_3240() {
        let mut nws = NwsFixture3240::new(&[KDLH_3240]);
        nws.points_delay = Some(std::time::Duration::from_secs(4));
        let h = start_harness_3240(nws.clone(), 32_413).await;
        let sites = [REMOTE_SITE_3240, (40.0, -75.0), (39.0, -76.0)];
        let mut commits = Vec::new();
        for (i, site) in sites.iter().enumerate() {
            commits.push(h.reveal(&format!("PWS-3240-SLOW-{i}"), Some(*site), 31.0, 95.0).await);
        }
        // `with_reference_selection` sets the lookup timeout to 2 s.
        let config = h.fixed_config().with_reference_selection(ReferenceMode::PerStation, 25.0, 8);
        let started = Instant::now();
        assert_eq!(cycle(&h, &config, &mut ReferenceResolver::default()).await, 3);
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "three commits must not cost three 2 s timeouts: {elapsed:?}"
        );
        assert_eq!(nws.points_requests.load(Ordering::SeqCst), 1, "outage must short-circuit");
        for commit in &commits {
            assert_eq!(h.verdict(commit).await, (true, UNCHECKED_CONFIDENCE_CAP));
        }
        h.server_handle.stop(false).await;
    }

    /// A station the node serves without `station_location` (unregistered, or
    /// an older node) keeps today's behaviour: the configured station, no
    /// point lookup.
    #[actix_web::test]
    async fn node_without_station_location_falls_back_3240() {
        let nws = NwsFixture3240::new(&[KDLH_3240, KTGA_3240]);
        let h = start_harness_3240(nws.clone(), 32_414).await;
        let unlocated = h.reveal("PWS-3240-UNREG", None, 21.5, 60.0).await;
        let config = h.fixed_config().with_reference_selection(ReferenceMode::PerStation, 25.0, 8);
        assert_eq!(cycle(&h, &config, &mut ReferenceResolver::default()).await, 1);
        assert_eq!(h.verdict(&unlocated).await, (true, 1.0));
        assert_eq!(nws.points_requests.load(Ordering::SeqCst), 0);
        assert!(nws.observation_fetches("KDLH") >= 1);
        assert_eq!(nws.observation_fetches("KTGA"), 0);
        h.server_handle.stop(false).await;
    }
}


// ---------------------------------------------------------------------------
// #3240 review follow-ups (N1, N4) for the shadow evaluation. Both compile
// against the merged #3273 API, so they are RED-first on main.
// ---------------------------------------------------------------------------

/// Captures every log line so a test can read the attester's own `SHADOW
/// reference:` output — the exact line the owner reviews before any flip.
struct CaptureLog3240;
static CAPTURED_3240: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
impl log::Log for CaptureLog3240 {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            CAPTURED_3240.lock().expect("log capture").push(record.args().to_string());
        }
    }
    fn flush(&self) {}
}
fn capture_logs_3240() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = log::set_logger(&CaptureLog3240);
        log::set_max_level(log::LevelFilter::Info);
    });
}
fn shadow_line_3240(commit_id: &str) -> String {
    CAPTURED_3240
        .lock()
        .expect("log capture")
        .iter()
        .find(|l| l.contains("SHADOW reference:") && l.contains(commit_id))
        .cloned()
        .unwrap_or_else(|| panic!("no SHADOW reference line for {commit_id}"))
}

/// Review N4: when the `fixed` verdict is PHYSICS-ONLY (the configured
/// station returned nothing), the shadow line must say so in `fixed_tier`,
/// not print the fold's default `good` next to a capped 0.5 confidence.
#[actix_web::test]
async fn shadow_log_fixed_tier_says_physics_only_when_fixed_was_capped_3240() {
    capture_logs_3240();
    // No KDLH in the fixture: the configured station 404s, so `fixed` is
    // PHYSICS-ONLY; the per-station KTGA path is checked.
    let nws = NwsFixture3240::new(&[KTGA_3240]);
    let h = start_harness_3240(nws.clone(), 32_420).await;
    let remote = h.reveal("PWS-3240-N4", Some(REMOTE_SITE_3240), 31.0, 95.0).await;
    let config = h
        .fixed_config()
        .with_reference_selection(oracle_attester_bin::ReferenceMode::Shadow, 25.0, 8);
    let mut cache: HashMap<String, Instant> = HashMap::new();
    let mut resolver = oracle_attester_bin::ReferenceResolver::default();
    oracle_attester_bin::run_cycle_with_resolver(
        &h.client,
        &config,
        &h.attester,
        &MechanicalPolicy::default(),
        &mut cache,
        &mut resolver,
    )
    .await
    .expect("shadow cycle");
    assert_eq!(h.verdict(&remote).await, (true, UNCHECKED_CONFIDENCE_CAP), "shadow signs fixed");
    let line = shadow_line_3240(&remote);
    assert!(line.contains("fixed_tier=PHYSICS-ONLY "), "fixed_tier must read PHYSICS-ONLY: {line}");
    assert!(line.contains("fixed_confidence=0.500"), "{line}");
    assert!(line.contains("per_station_tier=good "), "{line}");
    assert!(line.contains("would_change=true"), "{line}");
    h.server_handle.stop(false).await;
}

/// Drive one `per_station` cycle (25 km radius, 2 s lookup timeout).
async fn per_station_cycle_3240(h: &Harness3240) -> (usize, std::time::Duration) {
    let config = h
        .fixed_config()
        .with_reference_selection(oracle_attester_bin::ReferenceMode::PerStation, 25.0, 8);
    let mut cache: HashMap<String, Instant> = HashMap::new();
    let started = Instant::now();
    let submitted = oracle_attester_bin::run_cycle(
        &h.client,
        &config,
        &h.attester,
        &MechanicalPolicy::default(),
        &mut cache,
    )
    .await
    .expect("cycle completes");
    (submitted, started.elapsed())
}

/// Review N1, bounded: when api.weather.gov hangs on EVERY station's
/// observations, the cycle stops fetching after the second distinct station
/// times out — two timeouts, not one per station.
#[actix_web::test]
async fn hung_observation_fetches_are_bounded_per_cycle_3240() {
    // Four stations, each the nearest (within 25 km) to its own site; all
    // four hang on /observations for 4 s against the 2 s lookup timeout.
    let sites = [(41.95, -77.10), (40.00, -75.00), (39.00, -76.00), (38.00, -77.00)];
    let stations = [
        ("KSL1", 41.96, -77.11, 30.5, 93.0),
        ("KSL2", 40.01, -75.01, 30.5, 93.0),
        ("KSL3", 39.01, -76.01, 30.5, 93.0),
        ("KSL4", 38.01, -77.01, 30.5, 93.0),
    ];
    let mut nws = NwsFixture3240::new(&stations);
    nws.observations_delay = Some((
        stations.iter().map(|s| s.0.to_string()).collect(),
        std::time::Duration::from_secs(4),
    ));
    let h = start_harness_3240(nws.clone(), 32_421).await;
    let mut commits = Vec::new();
    for (i, site) in sites.iter().enumerate() {
        commits.push(h.reveal(&format!("PWS-3240-N1-{i}"), Some(*site), 31.0, 95.0).await);
    }
    let (submitted, elapsed) = per_station_cycle_3240(&h).await;
    assert_eq!(submitted, 4);
    let hung_fetches: usize = stations.iter().map(|s| nws.observation_fetches(s.0)).sum();
    assert_eq!(hung_fetches, 2, "the observation outage trips on the second distinct failure");
    assert!(
        elapsed < std::time::Duration::from_secs(6),
        "four hung stations must cost two 2 s timeouts, not four: {elapsed:?}"
    );
    for commit in &commits {
        assert_eq!(h.verdict(commit).await, (true, UNCHECKED_CONFIDENCE_CAP), "fails closed: capped");
    }
    h.server_handle.stop(false).await;
}

/// Review of #3341, F1: ONE station's hung observations must not cap other
/// honest stations. The healthy nearest station and the configured-station
/// fallback for an unregistered commit keep the score they get on main.
#[actix_web::test]
async fn one_hung_station_does_not_cap_a_healthy_station_or_the_fallback_3240() {
    let stations = [
        KDLH_3240,
        ("KSL1", 41.96, -77.11, 30.5, 93.0),
        ("KSL2", 40.01, -75.01, 30.5, 93.0),
    ];
    let mut nws = NwsFixture3240::new(&stations);
    nws.observations_delay = Some((vec!["KSL1".to_string()], std::time::Duration::from_secs(4)));
    let h = start_harness_3240(nws.clone(), 32_422).await;
    // The hung station's commit first, so the others are evaluated after it.
    let hung = h.reveal("PWS-3240-F1-A", Some((41.95, -77.10)), 31.0, 95.0).await;
    let healthy = h.reveal("PWS-3240-F1-B", Some((40.00, -75.00)), 31.0, 95.0).await;
    let unregistered = h.reveal("PWS-3240-F1-C", None, 21.5, 60.0).await;
    let (submitted, _) = per_station_cycle_3240(&h).await;
    assert_eq!(submitted, 3);
    assert_eq!(h.verdict(&hung).await, (true, UNCHECKED_CONFIDENCE_CAP), "hung station: capped");
    // Checked (~1.0; > 0.99 tolerates a second of freshness decay), not the 0.5 cap.
    let (ok, healthy_conf) = h.verdict(&healthy).await;
    assert!(ok && healthy_conf > 0.99, "healthy nearest station must stay checked: {healthy_conf}");
    let (ok, fallback_conf) = h.verdict(&unregistered).await;
    assert!(ok && fallback_conf > 0.99, "configured fallback must stay checked: {fallback_conf}");
    assert_eq!(nws.observation_fetches("KSL1"), 1);
    assert_eq!(nws.observation_fetches("KSL2"), 1);
    assert!(nws.observation_fetches("KDLH") >= 1);
    h.server_handle.stop(false).await;
}

/// Review of #3341, F2 (and #3273's N3): an unregistered commit keeps
/// today's configured-station score even after BOTH a `/points` hang and an
/// observation hang earlier in the same cycle.
#[actix_web::test]
async fn unregistered_commit_keeps_configured_score_through_points_and_observation_hangs_3240() {
    let stations = [KDLH_3240, ("KSL1", 41.96, -77.11, 30.5, 93.0)];
    let mut nws = NwsFixture3240::new(&stations);
    nws.observations_delay = Some((vec!["KSL1".to_string()], std::time::Duration::from_secs(4)));
    nws.points_delay = Some(std::time::Duration::from_secs(4));
    nws.points_delay_only = Some(vec!["40.00,-75.00".to_string()]);
    let h = start_harness_3240(nws.clone(), 32_423).await;
    let obs_hang = h.reveal("PWS-3240-F2-A", Some((41.95, -77.10)), 31.0, 95.0).await;
    let points_hang = h.reveal("PWS-3240-F2-B", Some((40.00, -75.00)), 31.0, 95.0).await;
    let unregistered = h.reveal("PWS-3240-F2-C", None, 21.5, 60.0).await;
    let (submitted, _) = per_station_cycle_3240(&h).await;
    assert_eq!(submitted, 3);
    assert_eq!(h.verdict(&obs_hang).await, (true, UNCHECKED_CONFIDENCE_CAP));
    assert_eq!(h.verdict(&points_hang).await, (true, UNCHECKED_CONFIDENCE_CAP));
    let (ok, conf) = h.verdict(&unregistered).await;
    assert!(
        ok && conf > 0.99,
        "unregistered commit must keep its configured-station (KDLH) score: {conf}"
    );
    assert!(nws.observation_fetches("KDLH") >= 1);
    h.server_handle.stop(false).await;
}
