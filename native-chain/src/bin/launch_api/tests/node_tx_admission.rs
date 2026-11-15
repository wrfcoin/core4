//! Node-service submit routes, gossip ingress and producer inclusion when several owners are
//! pending at once on the shared constant input.
use super::*;
#[path = "../../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod fixture;

// The harness's process-wide test lock is held for the whole test, as in its sibling tests.
#[allow(clippy::await_holding_lock)]
#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::{
        blockchain::{
            state::State,
            transaction::{Transaction, TransactionInput, TransactionOutput, TransactionType},
        },
        common::types::Balance,
        consensus::{
            node_bond_registry::NODE_BOND_UNITS, node_receipt_registry::*, node_share_contracts::*,
        },
        crypto::hash::hash_bytes,
    };

    const NODE_INPUT: &[u8] = b"wrfcoin.node_receipt_submission.input.v1";
    const DEPOSIT_INPUT: &[u8] = b"wrfcoin.node_bond_deposit.input.v1";

    struct Node {
        state: web::Data<AppState>,
        gossip: tokio::sync::mpsc::Receiver<wrfcoin_p2p::P2pCommand>,
        genesis: [u8; 32],
        _pin: crate::routes::snapshot::TestGenesisPin,
    }

    /// A real chain state at `H`, with owners `id * 10` bonded and `funded` accounts holding coins.
    async fn node(ids: std::ops::RangeInclusive<u8>, funded: &[u8]) -> Node {
        node_with_accounts(ids, funded, &[]).await
    }
    async fn node_with_accounts(
        ids: std::ops::RangeInclusive<u8>,
        funded: &[u8],
        accounts: &[wrfcoin_shared_protocol::common::types::Address],
    ) -> Node {
        let headers = crate::routes::snapshot::snapshot_test_support::chain_committing_to(
            "placeholder",
            fixture::H,
        );
        let genesis_hash = headers[0].hash.unwrap();
        let genesis = *genesis_hash.as_bytes();
        let mut bonded = fixture::bonds::state_on(
            genesis,
            *headers[(fixture::bonds::A - 2) as usize]
                .hash
                .unwrap()
                .as_bytes(),
        );
        for id in ids {
            let owner = id * 10;
            bonded.create_account(
                fixture::bonds::address(owner),
                Balance::new(u128::from(NODE_BOND_UNITS) * 3),
            );
            fixture::bonds::apply(
                &mut bonded,
                fixture::bonds::auth_on(owner, id, 0, NodeBondAction::Deposit, 1, genesis),
                fixture::bonds::A,
            )
            .unwrap();
        }
        for owner in funded {
            bonded.create_account(
                fixture::bonds::address(*owner),
                Balance::new(u128::from(NODE_BOND_UNITS) * 3),
            );
        }
        for address in accounts {
            bonded.create_account(*address, Balance::new(1_000_000_000));
        }
        for h in fixture::bonds::A + 1..=fixture::H {
            bonded
                .apply_transition(StateTransition::BeginNodeServiceBlock {
                    block_height: h,
                    parent_hash: *headers[(h - 2) as usize].hash.unwrap().as_bytes(),
                })
                .unwrap();
            bonded.apply_block_finalization(h);
        }
        let (gossip_client, gossip) =
            wrfcoin_p2p::P2pClient::for_testing_with_commands(libp2p::PeerId::random());
        let state = test_state_with_p2p(Some(fixture::key(11)), gossip_client);
        {
            let mut chain = state.blockchain.write().await;
            chain.state = bonded;
            *chain =
                crate::routes::snapshot::snapshot_test_support::with_chain_committing_to_its_state(
                    std::mem::take(&mut *chain),
                    fixture::H,
                );
            chain.set_finalized_height(fixture::H);
        }
        let pin = crate::routes::snapshot::TestGenesisPin::set(genesis_hash.to_hex());
        Node {
            state,
            gossip,
            genesis,
            _pin: pin,
        }
    }

    fn claim(s: &State, id: u8, ticket: u8) -> BondedServiceClaim {
        let consumer = if id == 1 { 2 } else { 1 };
        let mut c = fixture::claim(s, ticket);
        c.ticket.server_id = [id; 32];
        c.ticket.consumer_id = [consumer; 32];
        c.receipt.receipt.server_id = format!("node-{id}");
        c.receipt.receipt.consumer_pubkey = fixture::pk(consumer * 10 + 2);
        c.manifest.signer_id = hex::encode(fixture::pk(id * 10 + 1));
        fixture::resign_claim(&mut c, id * 10 + 1, consumer * 10 + 2);
        c
    }
    /// The wrapper the CLI builds: the shared constant input, zero output, its usual fee.
    fn receipt(
        s: &State,
        genesis: [u8; 32],
        id: u8,
        ticket: u8,
        nonce: u64,
        fee: u64,
    ) -> Transaction {
        let batch = NodeServiceBatch {
            claims: vec![claim(s, id, ticket)],
        };
        let owner = id * 10;
        let authorization = NodeReceiptSubmissionAuthorization {
            genesis_hash: genesis,
            sender: fixture::bonds::address(owner),
            nonce,
            batch_hash: batch_hash(&batch).unwrap(),
        };
        let p = NodeReceiptSubmission {
            owner_signature: fixture::sign(&authorization, owner),
            authorization,
            batch,
            owner_key: fixture::pk(owner),
        };
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_bytes(NODE_INPUT),
                vout: 0,
                signature: None,
                public_key: Some(hex::encode(p.owner_key)),
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                address: p.authorization.sender.to_string(),
                amount: 0,
                script: "OP_TRUE".into(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::NodeServiceBatch;
        tx.sender_address = p.authorization.sender;
        tx.nonce = nonce;
        tx.fee = fee;
        tx.data = Some(hex::encode(p.to_wire().unwrap()));
        tx.set_id();
        tx
    }
    fn deposit(genesis: [u8; 32], owner: u8, id: u8, nonce: u64) -> Transaction {
        let mut tx = fixture::bonds::transaction(
            &fixture::bonds::auth_on(owner, id, nonce, NodeBondAction::Deposit, 2, genesis),
            1_000_000,
        );
        tx.inputs[0].txid = hash_bytes(DEPOSIT_INPUT);
        tx.set_id();
        tx
    }
    async fn post(
        state: &web::Data<AppState>,
        uri: &str,
        tx: &Transaction,
    ) -> (StatusCode, serde_json::Value) {
        let app = test::init_service(test_app(state.clone())).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri(uri)
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed synthetic test bearer
                .set_json(tx)
                .to_request(),
        )
        .await;
        let status = response.status();
        let body = test::read_body(response).await;
        (
            status,
            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
        )
    }
    fn gossiped(rx: &mut tokio::sync::mpsc::Receiver<wrfcoin_p2p::P2pCommand>) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Ok(command) = rx.try_recv() {
            if let wrfcoin_p2p::P2pCommand::BroadcastTransactions(mut batch) = command {
                out.append(&mut batch);
            }
        }
        out
    }
    const RECEIPTS: &str = "/node/receipts/submit";
    const BONDS: &str = "/node/bonds/submit";

    macro_rules! isolate {
        ($name:literal) => {
            if crate::helpers::test_env::isolated_chain_domain(
                concat!("tests::node_tx_admission::tests::", $name),
                "777",
                "testnet",
                "0x2727272727272727272727272727272727272727",
                "none",
            ) {
                return;
            }
            let _lock = crate::helpers::test_env::lock();
        };
    }

    #[actix_web::test]
    async fn two_owners_receipts_both_admitted_on_one_node() {
        isolate!("two_owners_receipts_both_admitted_on_one_node");
        let mut n = node(1..=2, &[]).await;
        let s = n.state.blockchain.read().await.state.clone();
        let (a, b) = (
            receipt(&s, n.genesis, 1, 1, 1, 1_000_000),
            receipt(&s, n.genesis, 2, 1, 1, 1_000_000),
        );
        assert_eq!(a.inputs[0].txid, b.inputs[0].txid);
        let (status, body) = post(&n.state, RECEIPTS, &a).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let (status, body) = post(&n.state, RECEIPTS, &b).await;
        assert_eq!(
            status,
            StatusCode::ACCEPTED,
            "the second owner must not be refused: {body}"
        );
        assert_eq!(n.state.mempool.read().await.size(), 2);
        assert_eq!(gossiped(&mut n.gossip).len(), 2);
    }

    #[actix_web::test]
    async fn the_same_receipt_again_is_success_and_regossips_the_pending_copy() {
        isolate!("the_same_receipt_again_is_success_and_regossips_the_pending_copy");
        // Owner 1's claims name deployment 2 as consumer, so both must be bonded.
        let mut n = node(1..=2, &[]).await;
        let s = n.state.blockchain.read().await.state.clone();
        let tx = receipt(&s, n.genesis, 1, 1, 1, 1_000_000);
        assert_eq!(post(&n.state, RECEIPTS, &tx).await.0, StatusCode::ACCEPTED);
        let first = gossiped(&mut n.gossip);
        assert_eq!(first, vec![bincode::serialize(&tx).unwrap()]);

        let (status, body) = post(&n.state, RECEIPTS, &tx).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["admitted"], true);
        assert_eq!(body["duplicate"], true);
        assert_eq!(body["tx_id"], tx.id.unwrap().to_hex());
        assert_eq!(n.state.mempool.read().await.size(), 1);
        assert_eq!(
            gossiped(&mut n.gossip),
            first,
            "a re-post must send the pending copy again"
        );
    }

    #[actix_web::test]
    async fn a_dropped_gossip_recovers_on_repost() {
        isolate!("a_dropped_gossip_recovers_on_repost");
        // Owner 1's claims name deployment 2 as consumer, so both must be bonded.
        let mut n = node(1..=2, &[]).await;
        let s = n.state.blockchain.read().await.state.clone();
        let tx = receipt(&s, n.genesis, 1, 1, 1, 1_000_000);
        assert_eq!(post(&n.state, RECEIPTS, &tx).await.0, StatusCode::ACCEPTED);
        // The first gossip never reaches the producer.
        let lost = gossiped(&mut n.gossip);
        assert_eq!(lost.len(), 1);
        drop(lost);
        let mut producer = wrfcoin_shared_protocol::mempool::pool::Mempool::new(
            &wrfcoin_shared_protocol::config::settings::Config::default(),
        );
        assert_eq!(producer.size(), 0);
        // The owner re-posts the same bytes, and this time the gossip arrives.
        assert_eq!(post(&n.state, RECEIPTS, &tx).await.0, StatusCode::ACCEPTED);
        let delivered = gossiped(&mut n.gossip);
        assert_eq!(delivered.len(), 1, "the re-post must gossip");
        let decoded: Transaction = bincode::deserialize(&delivered[0]).unwrap();
        let admissible = crate::p2p_events::admissible_gossip_txs(&s, fixture::H, vec![decoded]);
        assert_eq!(admissible.len(), 1);
        let keys = wrfcoin_native_chain::security::key_registry::SigningKeyRegistry::default();
        for tx in admissible {
            assert_eq!(
                crate::p2p_events::ingest_gossiped_transaction_at_height(
                    &mut producer,
                    tx,
                    &keys,
                    Some(fixture::H)
                ),
                crate::p2p_events::GossipIngest::Admitted
            );
        }
        assert!(producer.contains(&tx.id.unwrap()));
    }

    #[actix_web::test]
    async fn the_route_returns_the_real_reason() {
        isolate!("the_route_returns_the_real_reason");
        let mut n = node(1..=2, &[]).await;
        let s = n.state.blockchain.read().await.state.clone();
        assert_eq!(
            post(
                &n.state,
                RECEIPTS,
                &receipt(&s, n.genesis, 1, 1, 1, 1_000_000)
            )
            .await
            .0,
            StatusCode::ACCEPTED
        );
        gossiped(&mut n.gossip);
        // A different batch at a nonce this owner already has pending.
        let (status, body) = post(
            &n.state,
            RECEIPTS,
            &receipt(&s, n.genesis, 1, 2, 1, 1_000_000),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "NODE_SUBMISSION_REFUSED");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("SP_NODE_NONCE_PENDING"),
            "{body}"
        );
        // The owner's nonce has moved past this one.
        n.state
            .blockchain
            .write()
            .await
            .state
            .accounts
            .get_mut(&fixture::bonds::address(20))
            .unwrap()
            .nonce = 5;
        let (status, body) = post(
            &n.state,
            RECEIPTS,
            &receipt(&s, n.genesis, 2, 3, 1, 1_000_000),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "NODE_TX_NONCE_USED");
        // Too far ahead.
        let (status, body) = post(
            &n.state,
            RECEIPTS,
            &receipt(&s, n.genesis, 2, 4, 9, 1_000_000),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "NODE_TX_NONCE_AHEAD");
        assert!(
            gossiped(&mut n.gossip).is_empty(),
            "a refused submit is never gossiped"
        );
        // Reasons are ours: no request material comes back.
        let text = body.to_string();
        assert!(!text.contains(TEST_OPERATOR_TOKEN), "{text}"); // secret-leak-ok: fixed synthetic test bearer
        assert!(
            !text.contains(&fixture::bonds::address(20).to_string()),
            "{text}"
        );
    }

    #[actix_web::test]
    async fn gossip_from_a_free_key_never_reaches_the_pool() {
        isolate!("gossip_from_a_free_key_never_reaches_the_pool");
        let n = node(1..=1, &[]).await;
        let s = n.state.blockchain.read().await.state.clone();
        let honest = receipt(&s, n.genesis, 1, 1, 1, 1_000_000);
        let batch = NodeServiceBatch {
            claims: vec![claim(&s, 1, 3)],
        };
        let junk = {
            let authorization = NodeReceiptSubmissionAuthorization {
                genesis_hash: n.genesis,
                sender: fixture::bonds::address(99),
                nonce: 0,
                batch_hash: batch_hash(&batch).unwrap(),
            };
            let p = NodeReceiptSubmission {
                owner_signature: fixture::sign(&authorization, 99),
                authorization,
                batch,
                owner_key: fixture::pk(99),
            };
            let mut tx = honest.clone();
            tx.sender_address = p.authorization.sender;
            tx.nonce = 0;
            tx.data = Some(hex::encode(p.to_wire().unwrap()));
            tx.inputs[0].public_key = Some(hex::encode(p.owner_key));
            tx.outputs[0].address = p.authorization.sender.to_string();
            tx.set_id();
            tx
        };
        let survivors = crate::p2p_events::admissible_gossip_txs(
            &s,
            fixture::H,
            vec![junk.clone(), honest.clone()],
        );
        assert_eq!(
            survivors.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![honest.id],
            "only the bonded owner survives"
        );
        // The live arm goes through the gate.
        let ingress = include_str!("../p2p_events.rs");
        let arm = ingress
            .split("P2pEvent::NewTransactions(tx_bytes_vec) =>")
            .nth(1)
            .unwrap()
            .split("P2pEvent::PeerConnected")
            .next()
            .unwrap();
        assert!(arm.contains("admissible_gossip_txs(&chain.state, chain.height, decoded)"));
        // A deposit with no co-sign or an unbonded exit from a free key never gets in either.
        let exit = fixture::bonds::transaction(
            &fixture::bonds::auth_on(99, 9, 0, NodeBondAction::Exit, 2, n.genesis),
            1,
        );
        assert!(crate::p2p_events::admissible_gossip_txs(&s, fixture::H, vec![exit]).is_empty());
    }

    #[actix_web::test]
    async fn a_stale_copy_clears_when_a_block_applies_on_a_non_producer() {
        isolate!("a_stale_copy_clears_when_a_block_applies_on_a_non_producer");
        let n = node(1..=2, &[]).await;
        let s = n.state.blockchain.read().await.state.clone();
        let (stale, live) = (
            receipt(&s, n.genesis, 1, 1, 1, 1_000_000),
            receipt(&s, n.genesis, 2, 1, 1, 1_000_000),
        );
        {
            let mut pool = n.state.mempool.write().await;
            pool.add_transaction_with_signing_keys_at_height(stale.clone(), None, Some(fixture::H))
                .unwrap();
            pool.add_transaction_with_signing_keys_at_height(live.clone(), None, Some(fixture::H))
                .unwrap();
        }
        let height = {
            let mut chain = n.state.blockchain.write().await;
            chain
                .state
                .accounts
                .get_mut(&fixture::bonds::address(10))
                .unwrap()
                .nonce = 2;
            chain.height += 1;
            chain.height
        };
        crate::p2p_events::drain_mempool_for_confirmed_range(
            height - 1,
            &n.state.blockchain,
            &n.state.mempool,
        )
        .await;
        let pool = n.state.mempool.read().await;
        assert!(
            !pool.contains(&stale.id.unwrap()),
            "the stale copy must be gone"
        );
        assert!(pool.contains(&live.id.unwrap()), "a live copy must stay");
    }

    #[actix_web::test]
    async fn a_producer_block_with_several_batches_is_accepted_by_a_peer() {
        isolate!("a_producer_block_with_several_batches_is_accepted_by_a_peer");
        let mut n = node(1..=6, &[]).await;
        let before = n.state.blockchain.read().await.clone();
        let s = before.state.clone();
        let txs: Vec<_> = (1..=6)
            .map(|id| receipt(&s, n.genesis, id, 1, 1, 1_000_000))
            .collect();
        for tx in &txs {
            let (status, body) = post(&n.state, RECEIPTS, tx).await;
            assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        }
        gossiped(&mut n.gossip);
        let block = n
            .state
            .consensus
            .create_pop_block_with_state(
                n.state.blockchain.clone(),
                n.state.mempool.clone(),
                &fixture::bonds::address(10).to_string(),
            )
            .await
            .expect("producer mines");
        let carried: Vec<_> = block
            .transactions
            .iter()
            .filter(|t| t.tx_type == TransactionType::NodeServiceBatch)
            .collect();
        assert_eq!(carried.len(), 6, "all six owners land in one block");
        assert_eq!(
            n.state.mempool.read().await.size(),
            0,
            "included batches leave the pool"
        );
        let producer = n.state.blockchain.read().await.state.clone();
        let registry = producer.node_receipt_registry.as_ref().unwrap();
        assert_eq!(registry.batch_count, 6);
        for id in 1..=6u8 {
            assert_eq!(
                registry.distinct_consumers(1, &[id; 32]).len(),
                1,
                "owner {id} got credit"
            );
        }
        let peer =
            crate::p2p_events::apply_node_receipt_block_for_test(before.clone(), &block).await;
        assert_eq!(
            peer.state_root, producer.state_root,
            "peer validates the block and reaches the same root"
        );
        assert!(crate::p2p_events::diff_states(&producer, &peer).is_empty());
        // The follower path, which rebuilds transitions from the block.
        let mut follower = before.state.clone();
        follower
            .apply_transitions_batch_at_height(
                block.height,
                crate::transition_extractor::extract_transitions(
                    &before.state,
                    &block,
                    block.height,
                    block.timestamp.timestamp(),
                ),
            )
            .unwrap();
        follower.apply_block_finalization_checked(&block).unwrap();
        assert_eq!(follower.state_root, producer.state_root, "extractor parity");
        // A re-post of a mined batch reports inclusion and does not re-enter the pool.
        let (status, body) = post(&n.state, RECEIPTS, &txs[0]).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["admitted"], true);
        assert_eq!(body["included"], true);
        assert_eq!(
            n.state.mempool.read().await.size(),
            0,
            "a mined batch must not leave a stale copy"
        );
        assert!(gossiped(&mut n.gossip).is_empty(), "nothing to spread");
    }

    #[actix_web::test]
    async fn the_block_cap_holds_and_the_overflow_lands_on_repost() {
        isolate!("the_block_cap_holds_and_the_overflow_lands_on_repost");
        let mut n = node(1..=9, &[]).await;
        let before = n.state.blockchain.read().await.clone();
        let s = before.state.clone();
        // Two pending per owner. The first nonce pays more so it sorts ahead of the second.
        let txs: Vec<_> = (1..=9)
            .flat_map(|id| {
                [
                    receipt(&s, n.genesis, id, 1, 1, 2_000_000),
                    receipt(&s, n.genesis, id, 2, 2, 1_000_000),
                ]
            })
            .collect();
        for tx in &txs {
            let (status, body) = post(&n.state, RECEIPTS, tx).await;
            assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        }
        assert_eq!(n.state.mempool.read().await.size(), 18);
        let miner = fixture::bonds::address(10).to_string();
        let block = n
            .state
            .consensus
            .create_pop_block_with_state(
                n.state.blockchain.clone(),
                n.state.mempool.clone(),
                &miner,
            )
            .await
            .expect("producer mines");
        let count = |b: &wrfcoin_shared_protocol::blockchain::block::Block| {
            b.transactions
                .iter()
                .filter(|t| t.tx_type == TransactionType::NodeServiceBatch)
                .count()
        };
        assert_eq!(
            count(&block),
            MAX_NODE_RECEIPT_BATCHES_PER_BLOCK,
            "the chain's cap holds"
        );
        let peer = crate::p2p_events::apply_node_receipt_block_for_test(before, &block).await;
        assert_eq!(
            peer.state_root,
            n.state.blockchain.read().await.state.state_root
        );
        // The two left over may have been evicted by assembly. Posting the same bytes is enough.
        gossiped(&mut n.gossip);
        let mined: Vec<_> = block.transactions.iter().filter_map(|t| t.id).collect();
        let left: Vec<_> = txs
            .iter()
            .filter(|t| !mined.contains(&t.id.unwrap()))
            .collect();
        assert_eq!(left.len(), 2);
        for tx in &left {
            let (status, body) = post(&n.state, RECEIPTS, tx).await;
            assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        }
        let next = n
            .state
            .consensus
            .create_pop_block_with_state(
                n.state.blockchain.clone(),
                n.state.mempool.clone(),
                &miner,
            )
            .await
            .expect("producer mines");
        assert_eq!(count(&next), 2);
    }

    #[actix_web::test]
    async fn same_fee_nonces_of_one_owner_both_land_given_a_repost() {
        isolate!("same_fee_nonces_of_one_owner_both_land_given_a_repost");
        // Owner 1's claims name deployment 2 as consumer, so both must be bonded.
        let n = node(1..=2, &[]).await;
        let s = n.state.blockchain.read().await.state.clone();
        let (first, second) = (
            receipt(&s, n.genesis, 1, 1, 1, 1_000_000),
            receipt(&s, n.genesis, 1, 2, 2, 1_000_000),
        );
        assert_eq!(
            post(&n.state, RECEIPTS, &first).await.0,
            StatusCode::ACCEPTED
        );
        assert_eq!(
            post(&n.state, RECEIPTS, &second).await.0,
            StatusCode::ACCEPTED
        );
        let miner = fixture::bonds::address(10).to_string();
        let block = n
            .state
            .consensus
            .create_pop_block_with_state(
                n.state.blockchain.clone(),
                n.state.mempool.clone(),
                &miner,
            )
            .await
            .expect("producer mines");
        let ids: Vec<_> = block.transactions.iter().filter_map(|t| t.id).collect();
        assert!(
            ids.contains(&first.id.unwrap()),
            "the live nonce is always mined"
        );
        if !ids.contains(&second.id.unwrap()) {
            // It sorted first and failed its nonce: assembly evicted it. The re-post recovers it.
            assert_eq!(
                post(&n.state, RECEIPTS, &second).await.0,
                StatusCode::ACCEPTED
            );
            let next = n
                .state
                .consensus
                .create_pop_block_with_state(
                    n.state.blockchain.clone(),
                    n.state.mempool.clone(),
                    &miner,
                )
                .await
                .expect("producer mines");
            assert!(next.transactions.iter().any(|t| t.id == second.id));
        }
    }

    /// A signed transparent transfer that outbids every receipt, from a key that signs for itself.
    fn high_fee_transfer(
        seed: u32,
        fee: u64,
    ) -> (wrfcoin_shared_protocol::common::types::Address, Transaction) {
        use wrfcoin_shared_protocol::{
            blockchain::transaction::TransparentTransferPayload,
            common::types::Address,
            crypto::signature::{PrivateKey, PublicKey},
        };
        let mut bytes = [7u8; 32];
        bytes[..4].copy_from_slice(&seed.to_le_bytes());
        let key = PrivateKey::from_bytes(&bytes).unwrap();
        let from = Address::from_public_key(&PublicKey::from_private_key(&key));
        let mut payload = TransparentTransferPayload {
            from: from.to_hex(),
            to: fixture::bonds::address(10).to_hex(),
            amount: 1,
            transparent_opt_out: true,
            opt_out_audit_tag: None,
            auth_pubkey: None,
            auth_signature: None,
            authority: None,
            authority_signature: None,
            passkey_auth: None,
            sponsorship: None,
            session_key_auth: None,
        };
        payload.sign_authorization(&key, 0);
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: hash_bytes(&seed.to_le_bytes()),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 0,
                address: from.to_hex(),
                script: "OP_TRUE".into(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::Transfer;
        tx.sender_address = from;
        tx.fee = fee;
        tx.data = Some(serde_json::to_string(&payload).unwrap());
        tx.set_id();
        (from, tx)
    }

    /// `funded`: the flood holds coins and is mined. Not funded: free keys, which assembly drops.
    async fn flood_case(funded: bool) {
        const FLOOD: u32 = 1_200;
        let limit = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing)
            .max_transactions_per_block as usize;
        assert!((FLOOD as usize) > limit, "the flood must overflow the default limit of {limit}");
        let flood: Vec<_> = (0..FLOOD).map(|i| high_fee_transfer(i, 50_000_000)).collect();
        let accounts: Vec<_> = if funded { flood.iter().map(|(a, _)| *a).collect() } else { vec![] };
        let mut n = node_with_accounts(1..=3, &[], &accounts).await;
        let before = n.state.blockchain.read().await.clone();
        let s = before.state.clone();
        let receipts: Vec<_> =
            (1..=3).map(|id| receipt(&s, n.genesis, id, 1, 1, 1_000_000)).collect();
        {
            let mut pool = n.state.mempool.write().await;
            for (_, tx) in &flood {
                pool.add_transaction_with_signing_keys_at_height(tx.clone(), None, Some(fixture::H))
                    .unwrap();
            }
        }
        for tx in &receipts {
            let (status, body) = post(&n.state, RECEIPTS, tx).await;
            assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        }
        gossiped(&mut n.gossip);
        let block = n
            .state
            .consensus
            .create_pop_block_with_state(
                n.state.blockchain.clone(),
                n.state.mempool.clone(),
                &fixture::bonds::address(10).to_string(),
            )
            .await
            .expect("producer mines");
        let ids: Vec<_> = block.transactions.iter().filter_map(|t| t.id).collect();
        for tx in &receipts {
            assert!(ids.contains(&tx.id.unwrap()), "every receipt must survive the flood");
        }
        let transfers =
            block.transactions.iter().filter(|t| t.tx_type == TransactionType::Transfer).count();
        println!("flood funded={funded}: {transfers} transfers mined, limit {limit}");
        assert!(block.transactions.len() <= limit);
        if funded {
            assert_eq!(transfers, limit - 1 - receipts.len(), "ordinary traffic fills the rest");
            // The transfers it displaced are still pending for the next block.
            assert_eq!(n.state.mempool.read().await.size(), FLOOD as usize - transfers);
        }
        let producer = n.state.blockchain.read().await.state.clone();
        assert_eq!(producer.node_receipt_registry.as_ref().unwrap().batch_count, 3);
        // Peer validation and WAL reload.
        let peer = crate::p2p_events::apply_node_receipt_block_for_test(before.clone(), &block).await;
        assert_eq!(peer.state_root, producer.state_root);
        assert!(crate::p2p_events::diff_states(&producer, &peer).is_empty());
        // The follower path.
        let mut follower = before.state.clone();
        follower
            .apply_transitions_batch_at_height(
                block.height,
                crate::transition_extractor::extract_transitions(
                    &before.state,
                    &block,
                    block.height,
                    block.timestamp.timestamp(),
                ),
            )
            .unwrap();
        follower.apply_block_finalization_checked(&block).unwrap();
        assert_eq!(follower.state_root, producer.state_root);
    }

    #[actix_web::test]
    async fn a_funded_flood_of_high_fee_transfers_cannot_push_receipts_out_of_a_block() {
        isolate!("a_funded_flood_of_high_fee_transfers_cannot_push_receipts_out_of_a_block");
        flood_case(true).await;
    }

    #[actix_web::test]
    async fn a_free_key_flood_of_high_fee_transfers_cannot_push_receipts_out_of_a_block() {
        isolate!("a_free_key_flood_of_high_fee_transfers_cannot_push_receipts_out_of_a_block");
        flood_case(false).await;
    }

    #[actix_web::test]
    async fn overflow_past_the_cap_stays_pending_for_the_next_block() {
        isolate!("overflow_past_the_cap_stays_pending_for_the_next_block");
        let n = node(1..=9, &[]).await;
        let before = n.state.blockchain.read().await.clone();
        let s = before.state.clone();
        let txs: Vec<_> = (1..=9)
            .flat_map(|id| {
                [
                    receipt(&s, n.genesis, id, 1, 1, 2_000_000),
                    receipt(&s, n.genesis, id, 2, 2, 1_000_000),
                ]
            })
            .collect();
        for tx in &txs {
            assert_eq!(post(&n.state, RECEIPTS, tx).await.0, StatusCode::ACCEPTED);
        }
        let miner = fixture::bonds::address(10).to_string();
        let count = |b: &wrfcoin_shared_protocol::blockchain::block::Block| {
            b.transactions.iter().filter(|t| t.tx_type == TransactionType::NodeServiceBatch).count()
        };
        let first = n
            .state
            .consensus
            .create_pop_block_with_state(n.state.blockchain.clone(), n.state.mempool.clone(), &miner)
            .await
            .expect("producer mines");
        assert_eq!(count(&first), MAX_NODE_RECEIPT_BATCHES_PER_BLOCK);
        assert_eq!(n.state.mempool.read().await.size(), 2, "the overflow must not be evicted");
        let peer = crate::p2p_events::apply_node_receipt_block_for_test(before, &first).await;
        assert_eq!(peer.state_root, n.state.blockchain.read().await.state.state_root);
        // No re-post needed.
        let second = n
            .state
            .consensus
            .create_pop_block_with_state(n.state.blockchain.clone(), n.state.mempool.clone(), &miner)
            .await
            .expect("producer mines");
        assert_eq!(count(&second), 2);
        assert_eq!(n.state.mempool.read().await.size(), 0);
    }

    #[actix_web::test]
    async fn deposits_share_the_fix_on_the_bond_route() {
        isolate!("deposits_share_the_fix_on_the_bond_route");
        let mut n = node(1..=1, &[30, 40]).await;
        let (a, b) = (deposit(n.genesis, 30, 3, 0), deposit(n.genesis, 40, 4, 0));
        assert_eq!(a.inputs[0].txid, b.inputs[0].txid);
        let (status, body) = post(&n.state, BONDS, &a).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let (status, body) = post(&n.state, BONDS, &b).await;
        assert_eq!(
            status,
            StatusCode::ACCEPTED,
            "the second deposit must not be refused: {body}"
        );
        assert_eq!(gossiped(&mut n.gossip).len(), 2);
        // Same bytes again: success, and the pending copy goes out again.
        let (status, body) = post(&n.state, BONDS, &a).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_eq!(body["duplicate"], true);
        assert_eq!(
            gossiped(&mut n.gossip),
            vec![bincode::serialize(&a).unwrap()]
        );
        assert_eq!(n.state.mempool.read().await.size(), 2);
        // The real reason on a refusal, and a free key's exit stops at the gate.
        let (status, body) = post(&n.state, BONDS, &deposit(n.genesis, 30, 5, 0)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "NODE_BOND_REFUSED");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("SP_NODE_NONCE_PENDING"),
            "{body}"
        );
        let exit = fixture::bonds::transaction(
            &fixture::bonds::auth_on(99, 9, 0, NodeBondAction::Exit, 2, n.genesis),
            1,
        );
        let (status, body) = post(&n.state, BONDS, &exit).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"]["code"], "NODE_TX_NOT_BONDED");
        // A stale deposit clears on a non-producer too.
        let height = {
            let mut chain = n.state.blockchain.write().await;
            chain
                .state
                .accounts
                .get_mut(&fixture::bonds::address(30))
                .unwrap()
                .nonce = 1;
            chain.height += 1;
            chain.height
        };
        crate::p2p_events::drain_mempool_for_confirmed_range(
            height - 1,
            &n.state.blockchain,
            &n.state.mempool,
        )
        .await;
        assert!(!n.state.mempool.read().await.contains(&a.id.unwrap()));
        assert!(n.state.mempool.read().await.contains(&b.id.unwrap()));
    }

    #[actix_web::test]
    async fn a_producer_block_with_several_deposits_is_accepted_by_a_peer() {
        isolate!("a_producer_block_with_several_deposits_is_accepted_by_a_peer");
        let n = node(1..=1, &[30, 40]).await;
        let before = n.state.blockchain.read().await.clone();
        for tx in [deposit(n.genesis, 30, 3, 0), deposit(n.genesis, 40, 4, 0)] {
            let (status, body) = post(&n.state, BONDS, &tx).await;
            assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        }
        let block = n
            .state
            .consensus
            .create_pop_block_with_state(
                n.state.blockchain.clone(),
                n.state.mempool.clone(),
                &fixture::bonds::address(10).to_string(),
            )
            .await
            .expect("producer mines");
        assert_eq!(
            block
                .transactions
                .iter()
                .filter(|t| t.tx_type == TransactionType::NodeBondLifecycle)
                .count(),
            2
        );
        let producer = n.state.blockchain.read().await.state.clone();
        let peer = crate::p2p_events::apply_node_receipt_block_for_test(before, &block).await;
        assert_eq!(peer.state_root, producer.state_root);
    }
}
