//! `GET /state/snapshot/prefix-counts`: per-key-prefix entry counts of the
//! `/state/snapshot/entries` entry set, from one snapshot build.
use super::*;
use super::producers_bonds_list::reader_get;
use std::collections::BTreeMap;

#[actix_web::test]
async fn snapshot_prefix_counts_requires_reader_auth() {
    let state = test_state(None);
    let app = test::init_service(test_app(state.clone())).await;
    let resp = test::call_service(
        &app,
        test::TestRequest::get().uri("/state/snapshot/prefix-counts").to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let resp =
        test::call_service(&app, reader_get("/state/snapshot/prefix-counts").to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[::core::prelude::v1::test]
fn snapshot_key_prefix_is_the_bytes_before_the_first_slash() {
    use crate::routes::snapshot::snapshot_key_prefix;
    assert_eq!(snapshot_key_prefix(b"state/accounts/0xab"), "state");
    assert_eq!(snapshot_key_prefix(b"chain/block/00000000000000000001"), "chain");
    assert_eq!(snapshot_key_prefix(b"noslash"), "noslash");
    assert_eq!(snapshot_key_prefix(b"/leading"), "");
    assert_eq!(snapshot_key_prefix(b"\xff\xfe/x"), "\u{FFFD}\u{FFFD}");
}

#[actix_web::test]
async fn snapshot_prefix_counts_match_the_entries_route_and_summary() {
    let state = test_state(None);
    let app = test::init_service(test_app(state.clone())).await;
    {
        let mut blockchain = state.blockchain.write().await;
        for i in 1..=5u8 {
            let address = format!("0x{}", hex::encode([i; 20]));
            blockchain
                .apply_state_transition(StateTransition::ApplyReward {
                    address: address.parse::<Address>().expect("address"),
                    amount: Balance(1_000 + u128::from(i)),
                    reason: "prefix-counts fixture".to_string(),
                })
                .expect("reward");
        }
    }

    let resp =
        test::call_service(&app, reader_get("/state/snapshot/prefix-counts").to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let counts: serde_json::Value = test::read_body_json(resp).await;

    let resp = test::call_service(&app, reader_get("/state/snapshot").to_request()).await;
    let summary: serde_json::Value = test::read_body_json(resp).await;

    // Walk the entries route and count the prefixes the way the verifier does:
    // `bytes.fromhex(key_hex).split(b"/")[0]`.
    let mut expected = BTreeMap::<String, u64>::new();
    let mut seen = 0u64;
    let mut offset = 0u64;
    let mut total: u64;
    loop {
        let uri = format!("/state/snapshot/entries?offset={offset}&limit=7");
        let resp = test::call_service(&app, reader_get(&uri).to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let page: serde_json::Value = test::read_body_json(resp).await;
        for field in ["height", "state_root_hex", "snapshot_root_hex"] {
            assert_eq!(page[field], counts[field], "entries {field} matches prefix-counts");
        }
        total = page["total"].as_u64().expect("total");
        for entry in page["entries"].as_array().expect("entries") {
            let key = hex::decode(entry["key_hex"].as_str().expect("key_hex")).expect("hex");
            let prefix = key.split(|&b| b == b'/').next().unwrap_or(&[]);
            *expected.entry(String::from_utf8_lossy(prefix).into_owned()).or_insert(0) += 1;
            seen += 1;
        }
        match page["next_offset"].as_u64() {
            Some(next) => offset = next,
            None => break,
        }
    }
    assert!(total > 0, "the fixture snapshot is not empty");
    assert_eq!(seen, total);

    for field in ["height", "state_root_hex", "snapshot_root_hex", "entry_count"] {
        assert_eq!(counts[field], summary[field], "prefix-counts {field} matches the summary");
    }
    assert_eq!(counts["entry_count"].as_u64(), Some(total));
    let got: BTreeMap<String, u64> =
        serde_json::from_value(counts["counts"].clone()).expect("counts map");
    assert_eq!(got.values().sum::<u64>(), total, "counts sum to the entries total");
    assert_eq!(got, expected, "per-prefix counts match a count over the entries route");
    assert!(got.contains_key("state"), "fixture has state entries: {got:?}");
}
