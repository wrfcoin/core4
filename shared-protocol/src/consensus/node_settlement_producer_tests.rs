use super::*;
use crate::config::settings::Config;
#[path = "../../tests/common/node_receipt_fixture.rs"]
mod f;
use crate::consensus::{node_bond_registry::NODE_BOND_UNITS, node_share_contracts::NodeBondAction};

fn isolated(name: &str) -> bool {
    const CHILD: &str = "WRFCOIN_NODE_S4_PRODUCER_CHILD";
    // determinism-ok: child-process test isolation marker, never consensus input.
    if std::env::var(CHILD).ok().as_deref() == Some(name) {
        return false;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env(CHILD, name)
        .env("WRFCOIN_CHAIN_ID", "777")
        .env("WRFCOIN_NETWORK_ID", "testnet")
        .output()
        .unwrap();
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "isolated producer fixture failed");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("test result: ok. 1 passed; 0 failed;")
    );
    true
}

fn licensed_state() -> State {
    let mut s = f::bonds::state();
    s.genesis_emission_config = Some(crate::crypto::chain_binding::GenesisEmissionConfig {
        chain_id: "777".into(),
        faucet_address: None,
     ..Default::default() });
    for id in 1..=9 {
        let owner = id * 10;
        s.create_account(
            f::bonds::address(owner),
            Balance::new(u128::from(NODE_BOND_UNITS) * 3),
        );
        f::bonds::apply(
            &mut s,
            f::bonds::auth(owner, id, 0, NodeBondAction::Deposit, 1),
            f::bonds::A,
        )
        .unwrap();
    }
    s.supply_baseline_initialized = false;
    s.apply_block_finalization(f::bonds::A);
    for h in f::bonds::A + 1..=f::H {
        s.apply_transition(StateTransition::BeginNodeServiceBlock {
            block_height: h,
            parent_hash: [(h % 251) as u8; 32],
        })
        .unwrap();
        s.apply_block_finalization(h);
        assert!(s.supply_violation.is_none());
    }
    s
}

fn batch(s: &State, id: u8, ticket: u8, nonce: u64) -> Transaction {
    let consumer = if id == 9 { 1 } else { id + 1 };
    let mut c = f::claim(s, ticket);
    c.ticket.server_id = [id; 32];
    c.ticket.consumer_id = [consumer; 32];
    c.receipt.receipt.server_id = format!("node-{id}");
    c.receipt.receipt.consumer_pubkey = f::pk(consumer * 10 + 2);
    c.manifest.signer_id = hex::encode(f::pk(id * 10 + 1));
    f::resign_claim(&mut c, id * 10 + 1, consumer * 10 + 2);
    let mut p = f::submission(vec![c], nonce);
    p.authorization.sender = f::bonds::address(id * 10);
    p.owner_key = f::pk(id * 10);
    p.owner_signature = f::sign(&p.authorization, id * 10);
    let mut tx = f::transaction(&p);
    tx.inputs[0].txid = crate::crypto::hash::hash_bytes(&[id, ticket]);
    // Higher fee rate admits every owner's first nonce before its second.
    tx.fee = if nonce == 1 { 200_000 } else { 100_000 };
    tx.set_id();
    tx
}

#[tokio::test]
async fn create_next_block_includes_sixteen_of_eighteen_valid_node_batches() {
    const NAME: &str = "consensus::proof_of_publishing::node_settlement_producer_tests::create_next_block_includes_sixteen_of_eighteen_valid_node_batches";
    if isolated(NAME) {
        return;
    }
    let state = licensed_state();
    let txs: Vec<_> = (1..=9)
        .flat_map(|id| [batch(&state, id, 1, 1), batch(&state, id, 2, 2)])
        .collect();
    // Every candidate is independently applicable at the next block before
    // global capacity selection; this is not a pool of deliberately bad txs.
    for tx in &txs {
        let mut control = state.clone();
        control.begin_node_service_block(f::H + 1, [4; 32]).unwrap();
        let p = crate::consensus::node_receipt_registry::NodeReceiptSubmission::parse(tx).unwrap();
        control
            .accounts
            .get_mut(&p.authorization.sender)
            .unwrap()
            .nonce = p.authorization.nonce;
        control
            .apply_transition(StateTransition::ApplyNodeServiceBatch {
                payload: p,
                block_height: f::H + 1,
            })
            .unwrap();
    }
    let mut initial = Blockchain::default();
    initial.height = f::H;
    initial.tip = Hash::from_bytes([4; 32]);
    initial.state = state;
    let chain = Arc::new(RwLock::new(initial));
    let pool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    for tx in txs.clone() {
        pool.write()
            .await
            .add_transaction_with_signing_keys_at_height(tx, None, Some(f::H))
            .unwrap();
    }
    assert_eq!(pool.read().await.get_all_transactions().len(), 18);
    let mut pop = ProofOfPublishing::new(1, 60, 0, 100);
    let block = pop
        .create_next_block(
            chain.clone(),
            pool.clone(),
            &f::bonds::address(10).to_string(),
        )
        .await
        .unwrap();
    assert_eq!(
        block
            .transactions
            .iter()
            .filter(|tx| tx.tx_type == TransactionType::NodeServiceBatch)
            .count(),
        16
    );
    assert_eq!(
        chain
            .read()
            .await
            .state
            .node_receipt_registry
            .as_ref()
            .unwrap()
            .batch_count,
        16
    );
    let remaining = pool.read().await.get_all_transactions();
    assert!(
        remaining.len() <= 2,
        "only overflow candidates may remain; eviction is also allowed"
    );
    for tx in &remaining {
        assert!(txs.iter().any(|candidate| candidate.id == tx.id));
        assert!(!block.transactions.iter().any(|included|included.id==tx.id),"confirmed candidates must be drained");
    }
}

#[test]
fn direct_block_classifier_node_receipt_inactive_and_cap() {
    const NAME: &str = "consensus::proof_of_publishing::node_settlement_producer_tests::direct_block_classifier_node_receipt_inactive_and_cap";
    if isolated(NAME) {
        return;
    }
    let s = licensed_state();
    let tx = batch(&s, 1, 1, 1);
    let pop = ProofOfPublishing::new(1, 60, 0, 100);
    let empty = FxHashSet::default();
    let ctx = BlockTxValidationCtx {
        height: f::bonds::A - 1,
        block_time: tx.timestamp.timestamp() as u64,
        check_quality: false,
        check_privacy: false,
        check_compute_fee: false,
        check_disclosure: false,
        check_tx_signatures: false,
        low_quality_quorum_commit_ids: &empty,
        selective_disclosure_verifier_keys: &[],
    };
    assert_eq!(
        pop.classify_block_transaction(&ctx, &tx, 0, &mut BlockTxValidationAccumulators::default())
            .as_deref(),
        Some("SP_NODE_RECEIPT_INACTIVE")
    );
    let ctx = BlockTxValidationCtx {
        height: f::H + 1,
        ..ctx
    };
    let mut acc = BlockTxValidationAccumulators::default();
    for i in 0..17 {
        let refusal = pop.classify_block_transaction(&ctx, &tx, i, &mut acc);
        if i < 16 {
            assert!(refusal.is_none(), "{refusal:?}");
        } else {
            assert_eq!(refusal.as_deref(), Some("SP_NODE_RECEIPT_BLOCK_CAP"));
        }
    }
}


fn fraud_transaction(s:&State,id:u8)->Transaction {
    let tx=batch(s,id,1,1);
    let mut p=crate::consensus::node_receipt_registry::NodeReceiptSubmission::parse(&tx).unwrap();
    p.batch.claims[0].receipt.signature[0]^=1;
    let consumer=if id==9 {1} else {id+1};
    f::resign_outer(&mut p.batch.claims[0],id*10+1,consumer*10+2);
    p.authorization.batch_hash=crate::consensus::node_fraud::evidence_hash(&p).unwrap();
    p.owner_signature=f::sign(&p.authorization,id*10);
    let mut tx=f::transaction(&p);tx.tx_type=TransactionType::NodeFraudReport;
    tx.inputs[0].txid=crate::crypto::hash::hash_bytes(&[id,99]);tx.fee=200_000;tx.set_id();tx
}
#[tokio::test]
async fn create_next_block_includes_eight_of_nine_valid_node_fraud_reports() {
    const NAME: &str = "consensus::proof_of_publishing::node_settlement_producer_tests::create_next_block_includes_eight_of_nine_valid_node_fraud_reports";
    if isolated(NAME) {
        return;
    }
    let state = licensed_state();
    let txs: Vec<_> = (1..=9)
        .map(|id| fraud_transaction(&state,id))
        .collect();
    // Every candidate is independently applicable at the next block before
    // global capacity selection; this is not a pool of deliberately bad txs.
    for tx in &txs {
        let mut control = state.clone();
        control.begin_node_service_block(f::H + 1, [4; 32]).unwrap();
        let p = crate::consensus::node_fraud::parse(tx).unwrap();
        control
            .accounts
            .get_mut(&p.authorization.sender)
            .unwrap()
            .nonce = p.authorization.nonce;
        control
            .apply_transition(StateTransition::ApplyNodeFraudReport {
                payload: p,
                block_height: f::H + 1,
            })
            .unwrap();
    }
    let mut initial = Blockchain::default();
    initial.height = f::H;
    initial.tip = Hash::from_bytes([4; 32]);
    initial.state = state;
    let chain = Arc::new(RwLock::new(initial));
    let pool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    for tx in txs.clone() {
        pool.write()
            .await
            .add_transaction_with_signing_keys_at_height(tx, None, Some(f::H))
            .unwrap();
    }
    assert_eq!(pool.read().await.get_all_transactions().len(), 9);
    let mut pop = ProofOfPublishing::new(1, 60, 0, 100);
    let block = pop
        .create_next_block(
            chain.clone(),
            pool.clone(),
            &f::bonds::address(10).to_string(),
        )
        .await
        .unwrap();
    assert_eq!(
        block
            .transactions
            .iter()
            .filter(|tx| tx.tx_type == TransactionType::NodeFraudReport)
            .count(),
        8
    );
    assert_eq!(
        chain
            .read()
            .await
            .state
            .node_fraud
            .as_ref()
            .unwrap()
            .report_count,
        8
    );
    let remaining = pool.read().await.get_all_transactions();
    assert!(
        remaining.len() <= 1,
        "only overflow candidates may remain; eviction is also allowed"
    );
    for tx in &remaining {
        assert!(txs.iter().any(|candidate| candidate.id == tx.id));
        assert!(!block.transactions.iter().any(|included|included.id==tx.id),"confirmed candidates must be drained");
    }
}

/// Reveals take their slots before any regular transaction is considered, so node kinds cannot
/// displace one: not at a tight limit, and not with node kinds pending in numbers.
#[tokio::test]
async fn node_kinds_never_displace_a_reveal() {
    const NAME: &str = "consensus::proof_of_publishing::node_settlement_producer_tests::node_kinds_never_displace_a_reveal";
    if isolated(NAME) {
        return;
    }
    for limit in [3usize, 4, 100] {
        let mut initial = Blockchain::default();
        initial.height = f::H;
        initial.tip = Hash::from_bytes([4; 32]);
        initial.state = licensed_state();
        super::tests::register_real_test_station(&mut initial, "station-node-kinds", f::bonds::address(10));
        let s = initial.state.clone();
        let chain = Arc::new(RwLock::new(initial));
        let pool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
        let (commit, mut reveal, _) = super::tests::build_commit_and_reveal(
            TransactionType::ContributionReveal,
            "station-node-kinds",
            "contribution",
            chrono::Utc::now().timestamp(),
            serde_json::json!({ "quality": 0.9 }),
            "node-kinds",
        );
        reveal.fee = 1; // A zero-fee reveal needs a compute receipt.
        reveal.set_id();
        let reveal_id = reveal.id;
        {
            let mut p = pool.write().await;
            p.add_transaction(commit).unwrap();
            p.add_transaction(reveal).unwrap();
            for id in 1..=9 {
                p.add_transaction_with_signing_keys_at_height(batch(&s, id, 1, 1), None, Some(f::H)).unwrap();
            }
        }
        let mut pop = ProofOfPublishing::new(1, 60, 0, limit);
        let block = pop.create_next_block(chain.clone(), pool.clone(), &f::bonds::address(10).to_string()).await.unwrap();
        let reveal_at = block.transactions.iter().position(|t| t.id == reveal_id);
        assert!(reveal_at.is_some(), "limit {limit}: the reveal must be in this block");
        let batches = block.transactions.iter().filter(|t| t.tx_type == TransactionType::NodeServiceBatch).count();
        // A producer may hold node kinds back from a block with a reveal, never the reveal for slots.
        assert!(batches <= limit.saturating_sub(3).min(9), "limit {limit}: node kinds get only what is left");
        if let Some(first_batch) = block.transactions.iter().position(|t| t.tx_type == TransactionType::NodeServiceBatch) {
            assert!(reveal_at.unwrap() < first_batch, "limit {limit}: reveals apply first");
        }
    }
}

/// The licensed state at `f::H` with `txs` pending, and a producer.
async fn producer_with(
    txs: Vec<Transaction>,
) -> (Arc<RwLock<Blockchain>>, Arc<RwLock<Mempool>>, ProofOfPublishing) {
    let mut initial = Blockchain::default();
    initial.height = f::H;
    initial.tip = Hash::from_bytes([4; 32]);
    initial.state = licensed_state();
    let chain = Arc::new(RwLock::new(initial));
    let pool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    for tx in txs {
        pool.write().await.add_transaction_with_signing_keys_at_height(tx, None, Some(f::H)).unwrap();
    }
    (chain, pool, ProofOfPublishing::new(1, 60, 0, 100))
}

fn with_fee(mut tx: Transaction, fee: u64) -> Transaction {
    tx.fee = fee;
    tx.set_id();
    tx
}

/// The owner's second batch outbids its first. Both still land, in nonce order.
#[tokio::test]
async fn a_higher_paying_next_nonce_batch_waits_for_the_owners_first() {
    const NAME: &str = "consensus::proof_of_publishing::node_settlement_producer_tests::a_higher_paying_next_nonce_batch_waits_for_the_owners_first";
    if isolated(NAME) {
        return;
    }
    let s = licensed_state();
    let first = with_fee(batch(&s, 1, 1, 1), 100_000);
    let second = with_fee(batch(&s, 1, 2, 2), 900_000);
    let (chain, pool, mut pop) = producer_with(vec![second.clone(), first.clone()]).await;
    let block = pop.create_next_block(chain, pool.clone(), &f::bonds::address(10).to_string()).await.unwrap();
    let pending = pool.read().await.get_all_transactions();
    let at = |id| block.transactions.iter().position(|t| t.id == id);
    assert!(at(first.id).is_some() && at(second.id).is_some(), "both batches are mined");
    assert!(at(first.id) < at(second.id), "nonce order");
    assert!(pending.is_empty(), "nothing is evicted or left behind");
}

/// The owner sends a transfer at nonce 1 and a receipt batch at nonce 2 that pays more. The
/// transfer is mined first; the batch is held, not evicted, and is mined in the next block.
#[tokio::test]
async fn a_transfer_at_n_and_a_batch_at_n_plus_one_are_mined_in_nonce_order() {
    const NAME: &str = "consensus::proof_of_publishing::node_settlement_producer_tests::a_transfer_at_n_and_a_batch_at_n_plus_one_are_mined_in_nonce_order";
    if isolated(NAME) {
        return;
    }
    let s = licensed_state();
    let owner = f::bonds::address(10);
    assert_eq!(s.accounts.get(&owner).unwrap().nonce, 1);
    let mut payload = crate::blockchain::transaction::TransparentTransferPayload {
        from: owner.to_hex(),
        to: f::bonds::address(20).to_hex(),
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
    payload.sign_authorization(&f::key(10), 1);
    let mut transfer = Transaction::new(
        vec![crate::blockchain::transaction::TransactionInput {
            txid: crate::crypto::hash::hash_bytes(b"owner-transfer"),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![crate::blockchain::transaction::TransactionOutput { amount: 0, address: owner.to_hex(), script: "OP_TRUE".into(), nft_data: None }],
    );
    transfer.tx_type = TransactionType::Transfer;
    transfer.data = Some(serde_json::to_string(&payload).unwrap());
    transfer.nonce = 1;
    let transfer = with_fee(transfer, 1_000);
    let receipt = with_fee(batch(&s, 1, 1, 2), 900_000);
    let (chain, pool, mut pop) = producer_with(vec![receipt.clone(), transfer.clone()]).await;
    let producer = f::bonds::address(10).to_string();
    let first = pop.create_next_block(chain.clone(), pool.clone(), &producer).await.unwrap();
    let has = |block: &Block, id| block.transactions.iter().any(|t| t.id == id);
    assert!(has(&first, transfer.id), "the transfer is mined");
    assert!(!has(&first, receipt.id), "the batch waits for the transfer's nonce");
    assert_eq!(chain.read().await.state.accounts.get(&owner).unwrap().nonce, 2);
    let pending = pool.read().await.get_all_transactions();
    assert!(pending.iter().any(|t| t.id == receipt.id), "the batch is still pending, not evicted");
    let second = pop.create_next_block(chain.clone(), pool.clone(), &producer).await.unwrap();
    assert!(has(&second, receipt.id), "the batch is mined in the next block");
    assert!(pool.read().await.get_all_transactions().is_empty());
}

// ---- r3 reveal evidence, real producer path with the separation guard ----
struct RevealRun { commit_at: u64, expires: u64, mined_at: u64, blocks_with_node: usize }

fn advance_state(s: &mut State, to: u64) {
    let from = s.node_bond_registry.as_ref().unwrap().block_height;
    for h in from + 1..=to {
        s.apply_transition(StateTransition::BeginNodeServiceBlock { block_height: h, parent_hash: [(h % 251) as u8; 32] }).unwrap();
        s.apply_block_finalization(h);
    }
}

async fn reveal_run(start: u64, initial_nodes: usize, saturate: bool, flood: usize, tag: &str) -> RevealRun {
    let mut initial = Blockchain::default();
    let mut st = licensed_state();
    advance_state(&mut st, start);
    initial.height = start;
    initial.tip = Hash::from_bytes([4; 32]);
    initial.state = st;
    super::tests::register_real_test_station(&mut initial, "station-guard", f::bonds::address(10));
    let chain = Arc::new(RwLock::new(initial));
    let pool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let (commit, mut reveal, commit_id) = super::tests::build_commit_and_reveal(
        TransactionType::ContributionReveal, "station-guard", "contribution",
        // determinism-ok: test fixture timestamp and quality; test-only module, never on a consensus path
        chrono::Utc::now().timestamp(), serde_json::json!({ "quality": 0.9 }), tag,
    );
    reveal.fee = 1;
    reveal.set_id();
    let reveal_id = reveal.id;
    let mut pop = ProofOfPublishing::new(1, 60, 0, 1000);
    let miner = f::bonds::address(10).to_string();
    pool.write().await.add_transaction(commit).unwrap();
    let first = pop.create_next_block(chain.clone(), pool.clone(), &miner).await.unwrap();
    let commit_at = first.height;
    let expires = chain.read().await.state.pending_commits[&commit_id].reveal_expires_at;
    pool.write().await.add_transaction(reveal).unwrap();
    let mut ticket = 1u8;
    let mut refill = |state: &State, p: &mut Mempool, want: usize, height: u64| {
        let mut added = 0;
        'owners: for id in 1..=9u8 {
            let nonce = state.accounts[&f::bonds::address(id * 10)].nonce;
            for n in [nonce, nonce + 1] {
                if added >= want { break 'owners; }
                ticket = ticket.wrapping_add(1);
                if p.add_transaction_with_signing_keys_at_height(batch(state, id, ticket, n), None, Some(height)).is_ok() { added += 1; }
            }
        }
    };
    {
        let state = chain.read().await.state.clone();
        let mut p = pool.write().await;
        refill(&state, &mut p, initial_nodes, commit_at);
        for i in 0..flood {
            let mut tx = Transaction::new(
                vec![crate::blockchain::transaction::TransactionInput { txid: crate::crypto::hash::hash_bytes(format!("{tag}{i}").as_bytes()), vout: 0, signature: None, public_key: None, coinbase_data: None }],
                vec![crate::blockchain::transaction::TransactionOutput { address: format!("f{i}"), amount: 0, script: "OP_TRUE".into(), nft_data: None }],
            );
            tx.tx_type = TransactionType::Regular;
            tx.fee = 50_000_000;
            let mut sender = [9u8; 20];
            sender[..8].copy_from_slice(&(i as u64).to_le_bytes());
            tx.sender_address = crate::common::types::Address(sender);
            tx.set_id();
            p.add_transaction(tx).unwrap();
        }
    }
    let mut blocks_with_node = 0;
    for _ in 0..12 {
        let block = pop.create_next_block(chain.clone(), pool.clone(), &miner).await.unwrap();
        let has_reveal = block.transactions.iter().any(|t| t.id == reveal_id);
        let has_node = block.transactions.iter().any(|t| t.tx_type == TransactionType::NodeServiceBatch);
        assert!(!(has_reveal && has_node), "the guard must never mix them");
        blocks_with_node += usize::from(has_node);
        if has_reveal {
            assert!(block.height <= expires, "mined past expiry");
            assert!(!chain.read().await.state.pending_commits.contains_key(&commit_id), "reveal applied");
            return RevealRun { commit_at, expires, mined_at: block.height, blocks_with_node };
        }
        assert!(pool.read().await.contains(&reveal_id.unwrap()), "{tag}: reveal was DROPPED at {}", block.height);
        assert!(block.height < expires, "{tag}: reveal missed its window at {}", block.height);
        if saturate {
            let state = chain.read().await.state.clone();
            let height = chain.read().await.height;
            let mut p = pool.write().await;
            refill(&state, &mut p, 32, height);
        }
    }
    panic!("{tag}: reveal never mined");
}

#[tokio::test]
async fn r3_reveal_is_never_dropped_and_measure_delay() {
    const NAME: &str = "consensus::proof_of_publishing::node_settlement_producer_tests::r3_reveal_is_never_dropped_and_measure_delay";
    if isolated(NAME) {
        return;
    }
    let close = crate::consensus::node_bond_registry::epoch_close(f::bonds::A, 1).unwrap();
    let far = f::H + 20;
    let tail = close - 6;
    // (case, start, nodes, saturate, flood)
    let cases: [(&str, u64, usize, bool, usize); 8] = [
        ("far  no-node", far, 0, false, 0),
        ("far  2-node", far, 2, false, 0),
        ("far  saturated", far, 18, true, 0),
        ("far  saturated+flood", far, 18, true, 1200),
        ("tail no-node", tail, 0, false, 0),
        ("tail 2-node", tail, 2, false, 0),
        ("tail saturated", tail, 18, true, 0),
        ("tail saturated+flood", tail, 18, true, 1200),
    ];
    for (case, start, nodes, saturate, flood) in cases {
        let r = reveal_run(start, nodes, saturate, flood, case).await;
        println!("REVEAL {case}: commit {} expires {} mined {} (slack left {}, {} node blocks first)", r.commit_at, r.expires, r.mined_at, r.expires - r.mined_at, r.blocks_with_node);
    }
}

// ---- The separation guard must not strand a same-owner later nonce ----

fn owner_transfer(owner_id: u8, to_id: u8, nonce: u64, tag: &[u8]) -> Transaction {
    let owner = f::bonds::address(owner_id);
    let mut payload = crate::blockchain::transaction::TransparentTransferPayload {
        from: owner.to_hex(),
        to: f::bonds::address(to_id).to_hex(),
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
    payload.sign_authorization(&f::key(owner_id), nonce);
    let mut transfer = Transaction::new(
        vec![crate::blockchain::transaction::TransactionInput {
            txid: crate::crypto::hash::hash_bytes(tag),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![crate::blockchain::transaction::TransactionOutput { amount: 0, address: owner.to_hex(), script: "OP_TRUE".into(), nft_data: None }],
    );
    transfer.tx_type = TransactionType::Transfer;
    transfer.data = Some(serde_json::to_string(&payload).unwrap());
    transfer.nonce = nonce;
    with_fee(transfer, 1_000)
}

/// The owner's receipt batch at nonce n and transfer at n+1 are pending with a contribution
/// reveal. The node-service / reveal separation guard holds the batch for a later block. The
/// transfer must wait with it: mined in this block it fails its nonce check, does nothing, and
/// leaves the pool as "confirmed", so the owner's payment is silently lost.
#[tokio::test]
async fn guard_held_node_tx_holds_the_owners_next_nonce_transfer() {
    const NAME: &str = "consensus::proof_of_publishing::node_settlement_producer_tests::guard_held_node_tx_holds_the_owners_next_nonce_transfer";
    if isolated(NAME) {
        return;
    }
    let mut initial = Blockchain::default();
    let mut st = licensed_state();
    let start = f::H + 20;
    advance_state(&mut st, start);
    initial.height = start;
    initial.tip = Hash::from_bytes([4; 32]);
    initial.state = st;
    super::tests::register_real_test_station(&mut initial, "station-edge", f::bonds::address(30));
    let owner = f::bonds::address(10);
    let payee = f::bonds::address(20);
    let s = initial.state.clone();
    let n = s.accounts[&owner].nonce;
    let payee_before = s.accounts[&payee].balance;
    let chain = Arc::new(RwLock::new(initial));
    let pool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    let (commit, mut reveal, _) = super::tests::build_commit_and_reveal(
        TransactionType::ContributionReveal,
        "station-edge",
        "contribution",
        chrono::Utc::now().timestamp(),
        serde_json::json!({ "quality": 0.9 }),
        "edge",
    );
    reveal.fee = 1;
    reveal.set_id();
    let reveal_id = reveal.id;
    let receipt = with_fee(batch(&s, 1, 1, n), 900_000);
    let transfer = owner_transfer(10, 20, n + 1, b"edge-transfer");
    {
        let mut p = pool.write().await;
        p.add_transaction(commit).unwrap();
        p.add_transaction(reveal).unwrap();
        p.add_transaction_with_signing_keys_at_height(receipt.clone(), None, Some(start)).unwrap();
        p.add_transaction_with_signing_keys_at_height(transfer.clone(), None, Some(start)).unwrap();
    }
    let mut pop = ProofOfPublishing::new(1, 60, 0, 100);
    let producer = f::bonds::address(10).to_string();
    let has = |block: &Block, id| block.transactions.iter().any(|t| t.id == id);
    let first = pop.create_next_block(chain.clone(), pool.clone(), &producer).await.unwrap();
    assert!(has(&first, reveal_id), "fixture: the reveal is mined first");
    assert!(!has(&first, receipt.id), "fixture: the guard holds the batch back");
    assert!(
        !has(&first, transfer.id),
        "the transfer at n+1 was mined while its owner's nonce-n batch was held back"
    );
    assert!(pool.read().await.contains(&transfer.id.unwrap()), "the transfer must stay pending");
    for _ in 0..3 {
        if pool.read().await.get_all_transactions().is_empty() {
            break;
        }
        pop.create_next_block(chain.clone(), pool.clone(), &producer).await.unwrap();
    }
    let state = chain.read().await.state.clone();
    assert_eq!(state.accounts[&owner].nonce, n + 2, "both of the owner's nonces were spent");
    assert_eq!(
        state.accounts[&payee].balance.0,
        payee_before.0 + 1,
        "the payment arrived"
    );
}

/// No reveal, no guard. The owner's batch at n and transfer at n+1 are both pending. A
/// block applies transfers before node-service transactions, so mined together the transfer
/// fails its nonce check and is lost. The transfer must wait one block and then land.
#[tokio::test]
async fn a_batch_at_n_and_a_transfer_at_n_plus_one_both_apply() {
    const NAME: &str = "consensus::proof_of_publishing::node_settlement_producer_tests::a_batch_at_n_and_a_transfer_at_n_plus_one_both_apply";
    if isolated(NAME) {
        return;
    }
    let s = licensed_state();
    let owner = f::bonds::address(10);
    let payee = f::bonds::address(20);
    let n = s.accounts[&owner].nonce;
    let payee_before = s.accounts[&payee].balance.0;
    let receipt = with_fee(batch(&s, 1, 1, n), 900_000);
    let transfer = owner_transfer(10, 20, n + 1, b"probe-transfer");
    let (chain, pool, mut pop) = producer_with(vec![receipt.clone(), transfer.clone()]).await;
    let producer = f::bonds::address(10).to_string();
    for _ in 0..3 {
        if pool.read().await.get_all_transactions().is_empty() {
            break;
        }
        pop.create_next_block(chain.clone(), pool.clone(), &producer).await.unwrap();
    }
    let state = chain.read().await.state.clone();
    assert_eq!(state.accounts[&owner].nonce, n + 2);
    assert_eq!(state.accounts[&payee].balance.0, payee_before + 1);
}
