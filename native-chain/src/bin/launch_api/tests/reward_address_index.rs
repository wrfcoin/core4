//! `GET /addresses/{address}/credits`: the API-only address index.
use super::*;
use crate::state::reward_index::{MAX_KEYS_EXAMINED_PER_READ, RewardAddressIndex};
use wrfcoin_shared_protocol::blockchain::state::{ContributionRewardRecord, StateTransition};
use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass as Provenance;

const OWNER: [u8; 20] = [0x77; 20];
const OTHER: [u8; 20] = [0x88; 20];

/// A chain tip value, for simulating a moved chain without mining a block.
fn tip_hash(byte: u8) -> wrfcoin_native_chain::crypto::hash::Hash {
    wrfcoin_native_chain::crypto::hash::Hash::from_bytes([byte; 32])
}

fn record(address: Address, id: &str, amount: u128, class: Provenance) -> ContributionRewardRecord {
    ContributionRewardRecord {
        address,
        amount: Balance::new(amount),
        reason: format!("contribution:{id}"),
        station_id: "index-test-station".into(),
        provenance_class: class,
    }
}

fn add_paid(chain: &mut Blockchain, address: Address, id: &str) {
    chain
        .state
        .contribution_reward_records
        .insert(id.into(), record(address, id, 5, Provenance::Real));
    chain.state.finalized_contribution_rewards.insert(id.into());
}

/// 1 owner record per `every` records, ids `00000000`.. in commit-id order.
async fn seed_sparse(state: &web::Data<AppState>, total: u32, every: u32) -> Vec<String> {
    let mut owned = Vec::new();
    let mut chain = state.blockchain.write().await;
    for n in 0..total {
        let id = format!("{n:08}");
        if n % every == 0 {
            add_paid(&mut chain, Address::new(OWNER), &id);
            owned.push(id);
        } else {
            add_paid(&mut chain, Address::new(OTHER), &id);
        }
    }
    owned
}

fn reader(uri: &str) -> test::TestRequest {
    test::TestRequest::get()
        .uri(uri)
        .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed #[cfg(test)] TEST_READER_TOKEN fixture for an in-process TestRequest and in-memory test_token_store; no runtime credentials
}

fn event_ids(data: &serde_json::Value) -> Vec<String> {
    data["credit_events"]
        .as_array()
        .expect("credit_events")
        .iter()
        .map(|event| {
            let id = event["event_id"].as_str().expect("event_id");
            id.strip_prefix("contribution:").expect("prefix").to_owned()
        })
        .collect()
}

macro_rules! get_data {
    ($app:expr, $uri:expr) => {{
        let resp = test::call_service($app, reader($uri).to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK, "{}", $uri);
        let body: serde_json::Value = test::read_body_json(resp).await;
        body["data"].clone()
    }};
}

/// Follow `credit_next_cursor` to the end and return ids in the order served.
macro_rules! walk_all {
    ($app:expr, $limit:expr) => {{
        let owner = Address::new(OWNER);
        let mut seen = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let suffix = cursor
                .as_ref()
                .map_or(String::new(), |c| format!("&credit_after={c}"));
            let data = get_data!(
                $app,
                &format!("/addresses/{owner}/credits?credit_limit={}{suffix}", $limit)
            );
            seen.extend(event_ids(&data));
            match data["credit_next_cursor"].as_str() {
                Some(next) => cursor = Some(next.to_owned()),
                None => break seen,
            }
        }
    }};
}

#[actix_web::test]
async fn sparse_wallet_gets_a_full_page_where_the_chain_wide_read_gets_few() {
    let state = test_state(None);
    let owned = seed_sparse(&state, 5_000, 200).await;
    assert_eq!(owned.len(), 25);
    assert!(state.reward_address_index.refresh(&state.blockchain).await);
    let app = test::init_service(test_app(state)).await;
    let owner = Address::new(OWNER);

    let old = get_data!(
        &app,
        &format!("/addresses/{owner}/transactions?credit_limit=20")
    );
    assert_eq!(
        old["credit_events"].as_array().unwrap().len(),
        1,
        "the old read's few rows"
    );

    let new = get_data!(&app, &format!("/addresses/{owner}/credits?credit_limit=20"));
    assert_eq!(event_ids(&new), owned[..20]);
    assert_eq!(new["credit_next_cursor"], json!(owned[19]));
    assert_eq!(new["credit_order"], "commit_id_ascending");
    assert!(
        new["credit_keys_examined"].as_u64().unwrap() <= 21,
        "cost follows the wallet"
    );
    // The row shape is the old read's, so a client changes only the path.
    let old_row = get_data!(
        &app,
        &format!("/addresses/{owner}/transactions?credit_limit=1")
    );
    assert_eq!(new["credit_events"][0], old_row["credit_events"][0]);
    // Its cursor is the old read's too: a walk can move across mid-way.
    let resumed = get_data!(
        &app,
        &format!(
            "/addresses/{owner}/credits?credit_limit=5&credit_after={}",
            old["credit_next_cursor"].as_str().unwrap()
        )
    );
    assert_eq!(event_ids(&resumed), owned[1..6]);
}

#[actix_web::test]
async fn walking_every_page_has_no_duplicates_or_gaps_and_matches_the_old_read() {
    let state = test_state(None);
    // Over two refresh chunks, so walking across a chunk edge is covered.
    let owned = seed_sparse(&state, 4_100, 7).await;
    state.reward_address_index.refresh(&state.blockchain).await;
    let app = test::init_service(test_app(state)).await;
    for limit in [1, 3, 7, 100] {
        assert_eq!(walk_all!(&app, limit), owned, "page size {limit}");
    }
    // The last page ends exactly: an exact multiple must not promise more.
    let owner = Address::new(OWNER);
    let last = get_data!(
        &app,
        &format!(
            "/addresses/{owner}/credits?credit_limit=1&credit_after={}",
            owned[owned.len() - 2]
        )
    );
    assert_eq!(last["credit_next_cursor"], serde_json::Value::Null);

    let mut old_walk = Vec::new();
    let mut cursor = String::new();
    loop {
        let data = get_data!(
            &app,
            &format!("/addresses/{owner}/transactions?credit_limit=100{cursor}")
        );
        old_walk.extend(event_ids(&data));
        match data["credit_next_cursor"].as_str() {
            Some(next) => cursor = format!("&credit_after={next}"),
            None => break,
        }
    }
    assert_eq!(
        old_walk, owned,
        "both routes must report the same paid credits"
    );
}

#[actix_web::test]
async fn cursor_stays_stable_across_new_blocks() {
    let state = test_state(None);
    let owned = seed_sparse(&state, 1_000, 10).await;
    state.reward_address_index.refresh(&state.blockchain).await;
    let app = test::init_service(test_app(state.clone())).await;
    let owner = Address::new(OWNER);

    let first = get_data!(&app, &format!("/addresses/{owner}/credits?credit_limit=10"));
    assert_eq!(event_ids(&first), owned[..10]);
    let cursor = first["credit_next_cursor"].as_str().unwrap().to_owned();

    // A new block pays this wallet on both sides of the cursor.
    {
        let mut chain = state.blockchain.write().await;
        add_paid(&mut chain, Address::new(OWNER), "00000001x");
        add_paid(&mut chain, Address::new(OWNER), "99999999");
        chain.height += 1;
        chain.tip = tip_hash(0xAB);
    }
    assert!(state.reward_address_index.refresh(&state.blockchain).await);

    let mut rest = Vec::new();
    let mut next = Some(cursor);
    while let Some(after) = next {
        let data = get_data!(
            &app,
            &format!("/addresses/{owner}/credits?credit_limit=10&credit_after={after}")
        );
        rest.extend(event_ids(&data));
        next = data["credit_next_cursor"].as_str().map(str::to_owned);
    }
    let mut expected = owned[10..].to_vec();
    expected.push("99999999".into());
    assert_eq!(
        rest, expected,
        "every earlier row exactly once, plus the new one ahead"
    );
    // The row behind the cursor shows on a fresh walk, never as a duplicate.
    assert!(walk_all!(&app, 50).contains(&"00000001x".to_string()));
}

#[actix_web::test]
async fn empty_cases_are_ok_and_empty() {
    let state = test_state(None);
    state.reward_address_index.refresh(&state.blockchain).await;
    let app = test::init_service(test_app(state)).await;
    let owner = Address::new(OWNER);
    for uri in [
        format!("/addresses/{owner}/credits"),
        format!("/addresses/{owner}/credits?credit_after=zzzz"),
    ] {
        let data = get_data!(&app, &uri);
        assert_eq!(data["credit_events"], json!([]));
        assert_eq!(data["credit_next_cursor"], serde_json::Value::Null);
        assert_eq!(data["credit_limit"], 50);
    }
}

#[actix_web::test]
async fn auth_role_and_bad_input_match_the_transactions_route() {
    let state = test_state(None);
    state.reward_address_index.refresh(&state.blockchain).await;
    let app = test::init_service(test_app(state)).await;
    let owner = Address::new(OWNER);
    for path in ["transactions", "credits"] {
        let uri = format!("/addresses/{owner}/{path}");
        let anon = test::call_service(&app, test::TestRequest::get().uri(&uri).to_request()).await;
        assert_eq!(anon.status(), StatusCode::UNAUTHORIZED, "{path}");
        let bad = test::call_service(
            &app,
            reader(&format!("/addresses/not-hex/{path}")).to_request(),
        )
        .await;
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST, "{path}");
    }
    let long = "a".repeat(257);
    let resp = test::call_service(
        &app,
        reader(&format!("/addresses/{owner}/credits?credit_after={long}")).to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let data = get_data!(
        &app,
        &format!("/addresses/{owner}/credits?credit_limit=100")
    );
    assert_eq!(data["credit_limit"], 100);
    for refused in [0, 101, 100_000] {
        let resp = test::call_service(
            &app,
            reader(&format!(
                "/addresses/{owner}/credits?credit_limit={refused}"
            ))
            .to_request(),
        )
        .await;
        assert!(
            resp.status().is_client_error(),
            "credit_limit={refused} must be refused"
        );
    }
}

#[actix_web::test]
async fn before_the_first_walk_the_route_says_warming() {
    let state = test_state(None);
    seed_sparse(&state, 10, 2).await;
    let app = test::init_service(test_app(state.clone())).await;
    let owner = Address::new(OWNER);
    let resp = test::call_service(
        &app,
        reader(&format!("/addresses/{owner}/credits")).to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["error"]["code"], "REWARD_INDEX_WARMING");
    // The older read does not depend on the index at all.
    let old = get_data!(
        &app,
        &format!("/addresses/{owner}/transactions?credit_limit=100")
    );
    assert_eq!(old["credit_events"].as_array().unwrap().len(), 5);

    state.reward_address_index.refresh(&state.blockchain).await;
    let data = get_data!(&app, &format!("/addresses/{owner}/credits"));
    assert_eq!(data["credit_events"].as_array().unwrap().len(), 5);
}

#[actix_web::test]
async fn only_paid_real_credits_show_and_a_late_quorum_needs_no_refresh() {
    let state = test_state(None);
    let owner = Address::new(OWNER);
    {
        let mut chain = state.blockchain.write().await;
        let records = &mut chain.state.contribution_reward_records;
        records.insert(
            "pending".into(),
            record(owner, "pending", 5, Provenance::Real),
        );
        records.insert(
            "synth".into(),
            record(owner, "synth", 0, Provenance::Synthetic),
        );
        records.insert("zero".into(), record(owner, "zero", 0, Provenance::Real));
        records.insert(
            "wrongkey".into(),
            record(owner, "other", 5, Provenance::Real),
        );
        let finalized = &mut chain.state.finalized_contribution_rewards;
        for id in ["synth", "zero", "wrongkey"] {
            finalized.insert(id.into());
        }
        add_paid(&mut chain, owner, "paid");
    }
    state.reward_address_index.refresh(&state.blockchain).await;
    let app = test::init_service(test_app(state.clone())).await;
    assert_eq!(walk_all!(&app, 50), ["paid"]);

    state
        .blockchain
        .write()
        .await
        .state
        .finalized_contribution_rewards
        .insert("pending".into());
    assert_eq!(
        walk_all!(&app, 50),
        ["paid", "pending"],
        "paid status is read live"
    );
}

#[actix_web::test]
async fn rollback_and_snapshot_sync_never_show_a_row_state_does_not_hold() {
    let state = test_state(None);
    let owned = seed_sparse(&state, 500, 10).await;
    state.reward_address_index.refresh(&state.blockchain).await;
    let app = test::init_service(test_app(state.clone())).await;
    let owner = Address::new(OWNER);

    // Rollback: two rows vanish, one moves to another wallet, tip moves back.
    let (gone, moved) = (owned[1].clone(), owned[2].clone());
    {
        let mut chain = state.blockchain.write().await;
        let state = &mut chain.state;
        state.contribution_reward_records.remove(&gone);
        state.finalized_contribution_rewards.remove(&gone);
        state.contribution_reward_records.insert(
            moved.clone(),
            record(Address::new(OTHER), &moved, 5, Provenance::Real),
        );
        chain.height = chain.height.saturating_sub(1);
        chain.tip = tip_hash(0xCD);
    }
    let expected: Vec<_> = owned
        .iter()
        .filter(|id| ![&gone, &moved].contains(id))
        .cloned()
        .collect();
    assert_eq!(walk_all!(&app, 10), expected, "stale index, correct rows");
    assert!(state.reward_address_index.refresh(&state.blockchain).await);
    assert_eq!(walk_all!(&app, 10), expected, "after the refresh too");

    // Snapshot sync: the whole state is replaced. New rows appear on refresh.
    {
        let mut chain = state.blockchain.write().await;
        chain.state.contribution_reward_records.clear();
        chain.state.finalized_contribution_rewards.clear();
        add_paid(&mut chain, owner, "fromsnapshot");
        chain.height = 99;
        chain.tip = tip_hash(0xEF);
    }
    assert!(
        walk_all!(&app, 10).is_empty(),
        "no phantom rows before the refresh"
    );
    assert!(state.reward_address_index.refresh(&state.blockchain).await);
    assert_eq!(walk_all!(&app, 10), ["fromsnapshot"]);
    let data = get_data!(&app, &format!("/addresses/{owner}/credits"));
    assert_eq!(data["index_height"], 99);
}

#[actix_web::test]
async fn refresh_runs_only_when_the_chain_moved_and_a_restart_rebuilds_the_same_pages() {
    let state = test_state(None);
    let owned = seed_sparse(&state, 300, 3).await;
    let index = &state.reward_address_index;
    assert!(index.refresh(&state.blockchain).await, "first walk");
    assert!(
        !index.refresh(&state.blockchain).await,
        "no change, no walk"
    );

    // A restart is an empty index over the same chain.
    let restarted = RewardAddressIndex::default();
    assert!(restarted.refresh(&state.blockchain).await);
    let owner = Address::new(OWNER);
    let chain = state.blockchain.read().await;
    for index in [index.published().unwrap(), restarted.published().unwrap()] {
        let page = index.page(&chain.state, owner, 1_000, None);
        assert_eq!(page.records.len(), owned.len());
        assert_eq!(page.next_cursor, None);
    }
}

#[actix_web::test]
async fn one_read_examines_a_bounded_number_of_keys_and_still_skips_nothing() {
    let state = test_state(None);
    let owner = Address::new(OWNER);
    {
        let mut chain = state.blockchain.write().await;
        // Unpaid keys first, so a page runs out of budget before filling.
        for n in 0..MAX_KEYS_EXAMINED_PER_READ + 5 {
            let id = format!("a{n:06}");
            chain
                .state
                .contribution_reward_records
                .insert(id.clone(), record(owner, &id, 5, Provenance::Real));
        }
        add_paid(&mut chain, owner, "z1");
        add_paid(&mut chain, owner, "z2");
    }
    state.reward_address_index.refresh(&state.blockchain).await;
    let app = test::init_service(test_app(state)).await;
    let first = get_data!(&app, &format!("/addresses/{owner}/credits"));
    assert_eq!(first["credit_keys_examined"], MAX_KEYS_EXAMINED_PER_READ);
    assert_eq!(first["credit_events"], json!([]));
    assert!(
        first["credit_next_cursor"].is_string(),
        "short page must say there is more"
    );
    assert_eq!(walk_all!(&app, 50), ["z1", "z2"]);
}

/// The hard line: the same transitions give the same state root with the index
/// refreshing after every apply and with no index at all.
#[actix_web::test]
async fn state_root_is_identical_with_the_index_on_and_off() {
    let state = test_state(None);
    seed_sparse(&state, 4_500, 5).await;
    let mut index_off = {
        let mut chain = state.blockchain.write().await;
        chain.state.recalculate_state_root();
        chain.state.clone()
    };
    let index = &state.reward_address_index;
    for n in 0..40u32 {
        let id = format!("applied{n:04}");
        let class = if n % 3 == 0 {
            Provenance::Synthetic
        } else {
            Provenance::Real
        };
        let amount = if n % 3 == 0 { 0 } else { 7 };
        let transition = StateTransition::ApplyContributionRewardDecision {
            record: record(Address::new(OWNER), &id, amount, class),
            current_block: u64::from(n),
        };
        {
            let mut chain = state.blockchain.write().await;
            chain
                .state
                .apply_transition(transition.clone())
                .expect("index-on apply");
            chain.height += 1;
            chain.tip = tip_hash(n as u8);
        }
        assert!(index.refresh(&state.blockchain).await, "index on: walk {n}");
        index_off
            .apply_transition(transition)
            .expect("index-off apply");
        let chain = state.blockchain.read().await;
        assert_eq!(
            chain.state.state_root_material(),
            index_off.state_root_material(),
            "root material diverged at step {n}"
        );
        assert_eq!(
            chain.state.state_root, index_off.state_root,
            "root diverged at step {n}"
        );
    }
    // Serving reads must not disturb the root either.
    let before = state.blockchain.read().await.state.state_root;
    let app = test::init_service(test_app(state.clone())).await;
    walk_all!(&app, 3);
    assert_eq!(state.blockchain.read().await.state.state_root, before);
}

/// Static half of the hard line: the module takes read guards only and is wired
/// into startup, outside any consensus crate.
#[actix_web::test]
async fn index_module_only_reads_the_chain_and_is_started_at_boot() {
    let module = include_str!("../reward_index.rs");
    let code = &module[module.find("\nuse ").expect("code start")..];
    assert!(
        !code.contains("chain.write()"),
        "the index must never take the chain write lock"
    );
    assert!(code.contains("chain.read().await"));
    let main = include_str!("../main.rs");
    let startup = &main[..main.find("\nmod tests {").expect("tests module")];
    assert!(
        startup.contains("reward_index::spawn_refresh("),
        "nothing would refresh the index"
    );
}

/// Refresh cost, for the PR report. Run with:
/// `cargo test -p wrfcoin-native-chain --bin launch_api reward_index_refresh_cost -- --ignored --nocapture`
#[actix_web::test]
#[ignore = "measurement, not a check"]
async fn reward_index_refresh_cost() {
    for total in [14_000u32, 100_000, 500_000] {
        let state = test_state(None);
        let _ = seed_sparse(&state, total, 200).await;
        let start = std::time::Instant::now();
        state.reward_address_index.refresh(&state.blockchain).await;
        let refresh = start.elapsed();
        let owner = Address::new(OWNER);
        let index = state.reward_address_index.published().unwrap();
        let chain = state.blockchain.read().await;
        let start = std::time::Instant::now();
        let page = index.page(&chain.state, owner, 100, None);
        eprintln!(
            "records={total} refresh={refresh:?} per_chunk={:?} page_100={:?} rows={}",
            refresh / (total / 2_000).max(1),
            start.elapsed(),
            page.records.len()
        );
    }
}
