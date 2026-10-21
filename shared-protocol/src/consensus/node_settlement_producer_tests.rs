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
