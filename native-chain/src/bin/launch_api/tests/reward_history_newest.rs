//! `GET /addresses/{address}/credits?order=newest`: newest-first reward history
//! with payout heights taken from block `reward_entries` metadata.
#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::state::reward_index::RewardAddressIndex;
    use wrfcoin_native_chain::blockchain::block::Block;
    use wrfcoin_shared_protocol::blockchain::state::ContributionRewardRecord;
    use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass as Provenance;

    const OWNER: [u8; 20] = [0x71; 20];
    const OTHER: [u8; 20] = [0x82; 20];
    const STRANGER: [u8; 20] = [0x93; 20];

    fn record(address: Address, id: &str) -> ContributionRewardRecord {
        ContributionRewardRecord {
            address,
            amount: Balance::new(5),
            reason: format!("contribution:{id}"),
            station_id: "newest-test-station".into(),
            provenance_class: Provenance::Real,
        }
    }

    /// A distinct block hash per (height, fork) pair.
    fn block_hash(height: u64, fork: u8) -> wrfcoin_native_chain::crypto::hash::Hash {
        let mut bytes = [fork; 32];
        bytes[..8].copy_from_slice(&height.to_be_bytes());
        wrfcoin_native_chain::crypto::hash::Hash::from_bytes(bytes)
    }

    /// The next block on `chain`, naming `paid` in its `reward_entries`.
    fn next_block(chain: &Blockchain, paid: &[ContributionRewardRecord], fork: u8) -> Block {
        let height = chain.height + 1;
        let mut block = Block::new(1, chain.tip, Vec::new(), 0, height);
        if !paid.is_empty() {
            block.metadata.insert(
                "reward_entries".into(),
                serde_json::to_string(paid).expect("entries"),
            );
        }
        block.set_hash(block_hash(height, fork));
        block
    }

    fn credit_state(chain: &mut Blockchain, paid: &[ContributionRewardRecord]) {
        for record in paid {
            let id = record.commit_id().expect("commit id").to_owned();
            chain
                .state
                .contribution_reward_records
                .insert(id.clone(), record.clone());
            chain.state.finalized_contribution_rewards.insert(id);
        }
    }

    /// Mine one block paying `paid`: state and block together, as apply does.
    async fn pay_block(state: &web::Data<AppState>, paid: &[ContributionRewardRecord]) -> Block {
        let mut chain = state.blockchain.write().await;
        credit_state(&mut chain, paid);
        let block = next_block(&chain, paid, 0);
        chain.add_block(block.clone()).expect("add block");
        block
    }

    /// `blocks` blocks of 20 other-wallet credits each; every `every`th also pays
    /// the owner once. Commit ids run against height order on purpose, so a walk
    /// in commit id order would not pass. Returns owner ids newest first.
    async fn seed_sparse_blocks(
        state: &web::Data<AppState>,
        blocks: u64,
        every: u64,
    ) -> Vec<String> {
        let mut owned = Vec::new();
        for n in 1..=blocks {
            let mut paid: Vec<_> = (0..20)
                .map(|i| record(Address::new(OTHER), &format!("o{n:05}-{i:02}")))
                .collect();
            if n % every == 0 {
                let id = format!("{:08}", 99_999_999 - n);
                paid.push(record(Address::new(OWNER), &id));
                owned.push(id);
            }
            pay_block(state, &paid).await;
        }
        owned.reverse();
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

    fn newest_uri(address: Address, limit: usize, cursor: Option<&str>) -> String {
        let suffix = cursor.map_or(String::new(), |c| format!("&credit_after={c}"));
        format!("/addresses/{address}/credits?order=newest&credit_limit={limit}{suffix}")
    }

    /// Follow the newest-first cursor to the end; ids in the order served.
    macro_rules! walk_newest {
        ($app:expr, $address:expr, $limit:expr) => {{
            let mut seen = Vec::new();
            let mut cursor: Option<String> = None;
            loop {
                let data = get_data!($app, &newest_uri($address, $limit, cursor.as_deref()));
                seen.extend(event_ids(&data));
                match data["credit_next_cursor"].as_str() {
                    Some(next) => cursor = Some(next.to_owned()),
                    None => break seen,
                }
            }
        }};
    }

    #[actix_web::test]
    async fn sparse_wallet_pages_newest_first_with_every_row_exactly_once() {
        let state = test_state(None);
        let owned = seed_sparse_blocks(&state, 150, 7).await;
        assert_eq!(owned.len(), 21);
        assert!(state.reward_address_index.refresh(&state.blockchain).await);
        let app = test::init_service(test_app(state)).await;
        let owner = Address::new(OWNER);

        // A full page from 3,000 records of which 21 are this wallet's.
        let first = get_data!(&app, &newest_uri(owner, 10, None));
        assert_eq!(event_ids(&first), owned[..10]);
        assert_eq!(first["credit_order"], "payout_height_descending");
        assert_eq!(first["credit_events"][0]["payout_height"], 147);
        assert_eq!(first["credit_events"][9]["payout_height"], 84);
        assert_eq!(first["credit_events"][0]["settlement"], "paid");
        assert!(first["credit_keys_examined"].as_u64().unwrap() <= 11);

        for limit in [1, 3, 7, 20, 21, 100] {
            assert_eq!(walk_newest!(&app, owner, limit), owned, "page size {limit}");
        }
        // An exact last page does not promise more.
        let page = get_data!(&app, &newest_uri(owner, 21, None));
        assert_eq!(page["credit_next_cursor"], serde_json::Value::Null);
    }

    #[actix_web::test]
    async fn cursor_stays_valid_when_new_blocks_arrive_between_pages() {
        let state = test_state(None);
        let owned = seed_sparse_blocks(&state, 60, 3).await;
        assert!(state.reward_address_index.refresh(&state.blockchain).await);
        let app = test::init_service(test_app(state.clone())).await;
        let owner = Address::new(OWNER);

        let first = get_data!(&app, &newest_uri(owner, 5, None));
        assert_eq!(event_ids(&first), owned[..5]);
        let cursor = first["credit_next_cursor"].as_str().unwrap().to_owned();

        // New blocks pay this wallet, one with a commit id that sorts below
        // everything already shown, and the index refreshes, before the next tap.
        let fresh = ["00000000", "zzzzzzzz"];
        for id in fresh {
            pay_block(&state, &[record(owner, id)]).await;
        }
        pay_block(&state, &[record(Address::new(OTHER), "late-other")]).await;
        assert!(state.reward_address_index.refresh(&state.blockchain).await);

        let mut rest = Vec::new();
        let mut next = Some(cursor);
        while let Some(after) = next {
            let data = get_data!(&app, &newest_uri(owner, 5, Some(&after)));
            rest.extend(event_ids(&data));
            next = data["credit_next_cursor"].as_str().map(str::to_owned);
        }
        assert_eq!(
            rest,
            owned[5..],
            "the rest exactly once, nothing new mixed in"
        );

        // A fresh walk shows the new rows on top, newest first.
        let mut expected = vec!["zzzzzzzz".to_string(), "00000000".to_string()];
        expected.extend(owned.iter().cloned());
        assert_eq!(walk_newest!(&app, owner, 4), expected);
    }

    #[actix_web::test]
    async fn unknown_address_returns_an_empty_page() {
        let state = test_state(None);
        state.reward_address_index.refresh(&state.blockchain).await;
        let app = test::init_service(test_app(state.clone())).await;
        let stranger = Address::new(STRANGER);
        let empty = get_data!(&app, &newest_uri(stranger, 50, None));
        assert_eq!(empty["credit_events"], json!([]));
        assert_eq!(empty["credit_next_cursor"], serde_json::Value::Null);

        seed_sparse_blocks(&state, 30, 2).await;
        assert!(state.reward_address_index.refresh(&state.blockchain).await);
        for cursor in [None, Some("n1.25.00000000")] {
            let data = get_data!(&app, &newest_uri(stranger, 50, cursor));
            assert_eq!(data["credit_events"], json!([]));
            assert_eq!(data["credit_next_cursor"], serde_json::Value::Null);
            assert_eq!(data["credit_keys_examined"], 0);
        }
    }

    #[actix_web::test]
    async fn existing_order_is_unchanged_and_bad_order_or_cursor_is_refused() {
        let state = test_state(None);
        seed_sparse_blocks(&state, 40, 4).await;
        state.reward_address_index.refresh(&state.blockchain).await;
        let app = test::init_service(test_app(state)).await;
        let owner = Address::new(OWNER);

        // Without `order`, the route is #119's: commit id order and its cursor.
        let old = get_data!(&app, &format!("/addresses/{owner}/credits?credit_limit=3"));
        assert_eq!(old["credit_order"], "commit_id_ascending");
        let ids = event_ids(&old);
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
        assert_eq!(old["credit_next_cursor"], json!(ids[2]));
        assert!(old["credit_events"][0].get("payout_height").is_none());
        // Newest-first rows are the old rows plus `payout_height`.
        let newest = get_data!(&app, &newest_uri(owner, 100, None));
        let mut row = newest["credit_events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["event_id"] == old["credit_events"][0]["event_id"])
            .expect("same credit")
            .clone();
        row.as_object_mut().unwrap().remove("payout_height");
        assert_eq!(row, old["credit_events"][0]);

        for bad in [
            format!("/addresses/{owner}/credits?order=oldest"),
            // A commit-order cursor is not a newest-first cursor.
            format!(
                "/addresses/{owner}/credits?order=newest&credit_after={}",
                ids[0]
            ),
            format!("/addresses/{owner}/credits?order=newest&credit_after=n1.x.abc"),
            format!("/addresses/{owner}/credits?order=newest&credit_after=n1.5."),
            format!("/addresses/{owner}/credits?order=newest&credit_limit=101"),
        ] {
            let resp = test::call_service(&app, reader(&bad).to_request()).await;
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{bad}");
        }
        let anon = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&newest_uri(owner, 5, None))
                .to_request(),
        )
        .await;
        assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn blocks_evicted_from_memory_are_dated_from_storage_after_a_restart() {
        let state = test_state(None);
        // A small in-memory window, so most paying blocks live only on disk.
        *state.blockchain.write().await = Blockchain::with_capacity(16);
        let owner = Address::new(OWNER);
        let mut owned = Vec::new();
        for n in 1..=80u64 {
            let paid = if n % 5 == 0 {
                let id = format!("{:04}", 9_999 - n);
                owned.push(id.clone());
                vec![record(owner, &id)]
            } else {
                Vec::new()
            };
            let block = pay_block(&state, &paid).await;
            state
                .storage
                .write()
                .await
                .persist_block(block, Vec::new(), None)
                .await
                .expect("persist");
        }
        owned.reverse();

        let restarted = RewardAddressIndex::default();
        assert!(
            restarted
                .refresh_with_storage(&state.blockchain, Some(&state.storage))
                .await
        );
        let chain = state.blockchain.read().await;
        let page = restarted
            .published()
            .unwrap()
            .newest_page(&chain.state, owner, 100, None);
        let ids: Vec<_> = page
            .records
            .iter()
            .map(|(r, _)| r.commit_id().unwrap().to_owned())
            .collect();
        assert_eq!(ids, owned);
        let heights: Vec<_> = page.records.iter().map(|(_, h)| *h).collect();
        let expected: Vec<_> = (1..=16u64).rev().map(|k| Some(k * 5)).collect();
        assert_eq!(heights, expected, "every row dated, disk and memory alike");

        // Without the blocks, credits stay listed: undated, after the dated ones.
        let memory_only = RewardAddressIndex::default();
        drop(chain);
        assert!(memory_only.refresh(&state.blockchain).await);
        let chain = state.blockchain.read().await;
        let page = memory_only
            .published()
            .unwrap()
            .newest_page(&chain.state, owner, 100, None);
        assert_eq!(page.records.len(), owned.len());
        let dated = page.records.iter().take_while(|(_, h)| h.is_some()).count();
        assert_eq!(dated, 4, "blocks 65..=80 are in memory: 65, 70, 75 and 80");
        assert!(page.records[dated..].iter().all(|(_, h)| h.is_none()));
    }

    #[actix_web::test]
    async fn a_reorg_moves_a_credit_to_the_block_that_now_pays_it() {
        let state = test_state(None);
        let owner = Address::new(OWNER);
        for n in 1..=10u64 {
            let paid = if n == 9 {
                vec![record(owner, "moved")]
            } else {
                Vec::new()
            };
            pay_block(&state, &paid).await;
        }
        pay_block(&state, &[]).await;
        assert!(state.reward_address_index.refresh(&state.blockchain).await);
        let heights = |state: &web::Data<AppState>| {
            let state = state.clone();
            async move {
                let chain = state.blockchain.read().await;
                state
                    .reward_address_index
                    .published()
                    .unwrap()
                    .newest_page(&chain.state, owner, 10, None)
                    .records
                    .into_iter()
                    .map(|(_, h)| h)
                    .collect::<Vec<_>>()
            }
        };
        assert_eq!(heights(&state).await, [Some(9)]);

        // Blocks 9..=11 are replaced by a branch that pays the credit in block 10.
        {
            let mut chain = state.blockchain.write().await;
            let kept: std::collections::BTreeMap<u64, Block> = chain
                .blocks()
                .range(..9)
                .map(|(h, b)| (*h, b.clone()))
                .collect();
            let tip = kept[&8].hash.unwrap();
            let state_copy = chain.state.clone();
            let mut fork = Blockchain::from_persisted(8, tip, state_copy, kept, 8, 0);
            for n in 9..=11u64 {
                let paid = if n == 10 {
                    vec![record(owner, "moved")]
                } else {
                    Vec::new()
                };
                let block = next_block(&fork, &paid, 0xF0);
                fork.add_block(block).expect("fork block");
            }
            *chain = fork;
        }
        assert!(state.reward_address_index.refresh(&state.blockchain).await);
        assert_eq!(heights(&state).await, [Some(10)]);
    }
}
