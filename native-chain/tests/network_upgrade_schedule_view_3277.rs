//! #3277 S6 + #3361 F4 — `GET /network/upgrade` reports the EFFECTIVE feature
//! schedule (`scheduled`) and the faucet sink the next block pays
//! (`faucet_sink`), through the real route handler on the real `AppState`.
//!
//! - Each entry names its source: `genesis` (in the genesis schedule string),
//!   `bootstrap` (in the installed base but not the genesis string — the
//!   shape an S5 `LEGACY_CHAIN_BOOTSTRAP` row has), or `extension` with the
//!   applied extension's sequence.
//! - An entry this binary does not implement, inside the #3095 run-up
//!   window, is a warning, and the #3093 deadline the node installed for it
//!   is reported beside it.
//! - A node restarted from its checkpoint and a node that joined from a
//!   snapshot report byte-identical `scheduled` and `faucet_sink` objects.
//! - The body carries no operator token and no governance signer key.
//!
//! The base schedule, the extension overlay and the deadline are
//! process-wide, and the included launch_api modules carry their own unit
//! tests that share this binary, so the scenarios run one after another in a
//! child process that runs nothing else; the parent never touches them.

#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/helpers.rs"]
mod helpers;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/routes/network.rs"]
mod network;
#[cfg(feature = "p2p")]
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/peer_reputation.rs"]
mod peer_reputation;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/rate_limit.rs"]
mod rate_limit;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/state.rs"]
mod state;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/transition_extractor.rs"]
mod transition_extractor;
#[allow(dead_code, unused_imports)]
#[path = "../src/bin/launch_api/websocket.rs"]
mod websocket;

include!("support/launch_api_test_support.inc.rs");

use actix_web::{App, http::StatusCode, test, web};
use wrfcoin_native_chain::blockchain::state::State;
use wrfcoin_native_chain::upgrade::{refresh_schedule_from_canonical_state, reset_for_tests};
use wrfcoin_native_chain::upgrade_notice::RUN_UP_BLOCKS;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    FeatureSchedule, GenesisEmissionConfig, SigningProtocolVersion, install_activation_table,
    install_feature_schedule, install_genesis_activation_schedule_string,
    refresh_feature_schedule_extensions, resolve_activation_schedule,
};
use wrfcoin_shared_protocol::crypto::hash::Hash;
use wrfcoin_shared_protocol::governance::schedule_extension::{AppliedExtension, ScheduleEntry};

/// Genesis schedules `paymaster`; the installed base additionally carries
/// `session_keys`, which genesis does not — a bootstrap-shaped row.
const GENESIS_RAW: &str = "0:v1,10:paymaster";
/// `faucet_sink_binding` activates here through extension seq 0.
const H_F: u64 = 2_000;
/// A feature this binary does not implement, through extension seq 1.
const UNKNOWN: &str = "a_b4_feature_this_binary_lacks";
const H_U: u64 = 4_000;
/// Inside the run-up window of `H_U`, and past `H_F`.
const TIP: u64 = H_U - RUN_UP_BLOCKS + 10;
/// The committed #3361 row's public address for chain 777.
const FAUCET_777: &str = "0xf72059c3b4a6d3c431920c3cf3e40dbf68a6b5d3"; // pragma: allowlist secret -- public faucet wallet ADDRESS ruled on core4#3361
/// A governance signer key recorded in the applied extension; public, but the
/// view has no reason to carry it, so it must not appear.
const SIGNER: &str = "5151515151515151515151515151515151515151515151515151515151515151"; // pragma: allowlist secret -- fixture public key

const CHILD_ENV: &str = "WRFCOIN_NETWORK_UPGRADE_VIEW_CHILD_3277";
const TEST_NAME: &str = "network_upgrade_reports_the_effective_schedule_and_faucet_sink";

/// Boot-time globals, as the startup gate installs them from block 1. First
/// install wins; every test installs the same values.
fn install_base() {
    let table = resolve_activation_schedule(
        [("activation_schedule", GENESIS_RAW)],
        SigningProtocolVersion::V1,
    )
    .expect("table");
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(
        FeatureSchedule::from_entries(vec![
            ("paymaster".to_string(), 10),
            ("session_keys".to_string(), 20),
        ])
        .expect("base"),
    );
    install_genesis_activation_schedule_string(Some(GENESIS_RAW.to_string()));
}

fn applied(sequence: u64, carried: u64, feature: &str, height: u64) -> AppliedExtension {
    AppliedExtension {
        sequence,
        carried_at_height: carried,
        entries: vec![ScheduleEntry::new(feature, height)],
        version_steps: Vec::new(),
        min_protocol_version: 3,
        release_commit: "c0ffee".into(),
        signers: vec![SIGNER.into()],
    }
}

/// The canonical state of chain 777 (no genesis faucet address) after both
/// extensions applied.
fn canonical_state() -> State {
    let mut state = State::new();
    state.genesis_emission_config = Some(GenesisEmissionConfig {
        chain_id: "777".into(),
        faucet_address: None,
     ..Default::default() });
    state.feature_schedule_extensions = vec![
        applied(0, 100, "faucet_sink_binding", H_F),
        applied(1, 200, UNKNOWN, H_U),
    ];
    state.recalculate_state_root();
    state
}

/// A fresh process's view of `state` at `TIP`: empty overlay, no deadline,
/// then the refresh the boot / snapshot-import path runs on the canonical
/// state it loaded.
async fn serve(state_value: State) -> serde_json::Value {
    reset_for_tests();
    refresh_feature_schedule_extensions(Vec::new());
    refresh_schedule_from_canonical_state(&state_value, "test boot");
    let app_state = test_state(None);
    {
        let mut chain = app_state.blockchain.write().await;
        chain.state = state_value;
        chain.height = TIP;
    }
    let app = test::init_service(App::new().app_data(app_state.clone()).route(
        "/network/upgrade",
        web::get().to(network::get_network_upgrade),
    ))
    .await;
    let req = test::TestRequest::get()
        .uri("/network/upgrade")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "the route is unauthenticated"
    );
    test::read_body_json(resp).await
}

fn entry<'a>(body: &'a serde_json::Value, feature: &str) -> &'a serde_json::Value {
    body["scheduled"]["entries"]
        .as_array()
        .expect("scheduled.entries")
        .iter()
        .find(|e| e["feature"] == feature)
        .unwrap_or_else(|| panic!("no entry {feature} in {body}"))
}

#[actix_web::test]
async fn network_upgrade_reports_the_effective_schedule_and_faucet_sink() {
    if std::env::var_os(CHILD_ENV).is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", TEST_NAME, "--nocapture", "--test-threads=1"])
            .env(CHILD_ENV, "1")
            .output()
            .expect("spawn child");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "child failed\n--- stdout\n{stdout}\n--- stderr\n{stderr}"
        );
        return;
    }
    install_base();
    the_effective_schedule_is_reported_with_sources_known_flags_and_run_up_warnings().await;
    before_the_binding_activates_the_faucet_sink_is_none().await;
    a_restarted_node_and_a_snapshot_joined_node_report_the_same_view().await;
}

async fn the_effective_schedule_is_reported_with_sources_known_flags_and_run_up_warnings() {
    let body = serve(canonical_state()).await;
    let scheduled = &body["scheduled"];

    assert_eq!(scheduled["next_height"], TIP + 1);
    assert_eq!(scheduled["version_at_next"], 1);
    assert_eq!(scheduled["total_entries"], 4);
    assert_eq!(scheduled["truncated"], false);

    let paymaster = entry(&body, "paymaster");
    assert_eq!(paymaster["source"], "genesis");
    assert_eq!(paymaster["known"], true);
    assert_eq!(paymaster["active_at_next"], true);
    assert!(paymaster.get("extension_sequence").is_none());

    assert_eq!(entry(&body, "session_keys")["source"], "bootstrap");

    let faucet = entry(&body, "faucet_sink_binding");
    assert_eq!(faucet["source"], "extension");
    assert_eq!(faucet["extension_sequence"], 0);
    assert_eq!(faucet["carried_at_height"], 100);
    assert_eq!(faucet["known"], true);
    assert_eq!(faucet["active_at_next"], true);

    let unknown = entry(&body, UNKNOWN);
    assert_eq!(unknown["extension_sequence"], 1);
    assert_eq!(unknown["known"], false);
    assert_eq!(unknown["active_at_next"], false);
    assert_eq!(unknown["blocks_remaining"], H_U - TIP);

    assert_eq!(scheduled["status"], "upgrade_required");
    let warnings = scheduled["warnings"].as_array().expect("warnings");
    assert_eq!(
        warnings.len(),
        1,
        "only the unknown entry warns: {warnings:?}"
    );
    assert_eq!(warnings[0]["feature"], UNKNOWN);
    assert_eq!(warnings[0]["level"], "warn");
    assert_eq!(warnings[0]["extension_sequence"], 1);
    let message = warnings[0]["message"].as_str().expect("message");
    assert!(message.contains(&format!("by height {H_U}")), "{message}");
    assert!(message.contains("extension seq 1"), "{message}");

    // The #3093 deadline the refresh installed for it, reported beside it.
    assert_eq!(scheduled["halt_deadline"]["feature"], UNKNOWN);
    assert_eq!(scheduled["halt_deadline"]["height"], H_U);

    // #3361 F4: 777 has no genesis faucet address; the committed row is in
    // force since the extension activated it.
    let sink = &body["faucet_sink"];
    assert_eq!(sink["address"], FAUCET_777);
    assert_eq!(sink["source"], "binding");
    assert_eq!(sink["active_from"], H_F);
    assert_eq!(sink["at_height"], TIP + 1);

    // Nothing secret: no operator token, no signer key.
    let raw = body.to_string();
    for secret in [
        TEST_OPERATOR_TOKEN,
        TEST_CONTRIBUTOR_TOKEN,
        TEST_ORACLE_TOKEN,
        TEST_READER_TOKEN,
        SIGNER,
    ] {
        assert!(!raw.contains(secret), "response leaks {secret}");
    }
    assert!(
        !raw.contains("signers"),
        "response carries extension signers"
    );
}

async fn before_the_binding_activates_the_faucet_sink_is_none() {
    let mut state = canonical_state();
    state.feature_schedule_extensions[0] = applied(0, 100, "faucet_sink_binding", TIP + 500);
    let body = serve(state).await;
    let sink = &body["faucet_sink"];
    assert_eq!(sink["address"], serde_json::Value::Null);
    assert_eq!(sink["source"], "none");
    assert_eq!(sink["active_from"], serde_json::Value::Null);

    // A genesis faucet address wins over any row, from block 1.
    let mut state = canonical_state();
    state.genesis_emission_config = Some(GenesisEmissionConfig {
        chain_id: "777".into(),
        faucet_address: Some(test_address(0x42)),
     ..Default::default() });
    let body = serve(state).await;
    assert_eq!(body["faucet_sink"]["source"], "genesis");
    assert_eq!(body["faucet_sink"]["active_from"], 1);
    assert_eq!(
        body["faucet_sink"]["address"],
        test_address(0x42).to_string()
    );
}

async fn a_restarted_node_and_a_snapshot_joined_node_report_the_same_view() {
    let producer = serve(canonical_state()).await;

    // Restart: the producer's checkpoint, reloaded by a new process.
    let restart_path =
        std::env::temp_dir().join(format!("wrfcoin-s6-restart-{}.redb", fastrand::u64(..)));
    let restarted_state = {
        let storage = Storage::open(&restart_path, DEFAULT_CHECKPOINT_INTERVAL).expect("open");
        storage
            .force_checkpoint(&canonical_state(), TIP)
            .await
            .expect("checkpoint");
        drop(storage);
        let reopened = Storage::open(&restart_path, DEFAULT_CHECKPOINT_INTERVAL).expect("reopen");
        reopened
            .load_checkpoint_at_or_before(TIP)
            .expect("read")
            .expect("checkpoint present")
            .1
    };
    let _ = std::fs::remove_file(&restart_path);
    let restarted = serve(restarted_state).await;

    // Snapshot join: a fresh volume installs the served snapshot state.
    let join_path =
        std::env::temp_dir().join(format!("wrfcoin-s6-join-{}.redb", fastrand::u64(..)));
    let joined_state = {
        let storage = Storage::open(&join_path, DEFAULT_CHECKPOINT_INTERVAL).expect("open");
        storage
            .install_snapshot(&canonical_state(), TIP, Hash::default(), &[], TIP)
            .await
            .expect("install snapshot");
        storage
            .load_checkpoint_at_or_before(TIP)
            .expect("read")
            .expect("snapshot checkpoint present")
            .1
    };
    let _ = std::fs::remove_file(&join_path);
    let joined = serve(joined_state).await;

    for (name, other) in [("restarted", &restarted), ("snapshot-joined", &joined)] {
        assert_eq!(
            other["scheduled"], producer["scheduled"],
            "{name} scheduled view"
        );
        assert_eq!(
            other["faucet_sink"], producer["faucet_sink"],
            "{name} faucet sink"
        );
    }
    // And the view is the interesting one, not two empty objects agreeing.
    assert_eq!(producer["scheduled"]["total_entries"], 4);
    assert_eq!(producer["faucet_sink"]["source"], "binding");
}
