//! Synthetic, public-only consensus check for the actual CLI loopback flow.
#[path = "../../../../shared-protocol/tests/common/node_receipt_fixture.rs"]
mod fixture;
use wrfcoin_shared_protocol::{
    blockchain::{
        state::{Account, StateTransition},
        transaction::Transaction,
    },
    common::types::Balance,
    consensus::{
        node_bond_registry::NodeBondLifecycle,
        node_receipt_registry::{NodeReceiptSubmission, claim_hash},
        node_share_contracts::NodeBondAction,
    },
};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 2 && args[1] == "--submission-window" {
        println!("{}", wrfcoin_shared_protocol::consensus::proof_of_publishing::DEFAULT_SUBMISSION_TIME_WINDOW_SECS);
        return;
    }
    assert!(
        args.len() == 3 || args.len() == 4,
        "public Deposit packet and output archive paths required"
    );
    let tx: Transaction = serde_json::from_slice(&std::fs::read(&args[1]).unwrap()).unwrap();
    assert_eq!(tx.id, Some(tx.calculate_hash()), "canonical Deposit ID");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(now.abs_diff(tx.timestamp.timestamp() as u64)
        <= wrfcoin_shared_protocol::consensus::proof_of_publishing::DEFAULT_SUBMISSION_TIME_WINDOW_SECS,
        "Deposit must fit the producer submission window");
    let p = NodeBondLifecycle::parse(&tx).unwrap();
    p.verify(
        &fixture::identity(),
        &[7; 32],
        &[hex::encode(fixture::bonds::pk(99))],
    )
    .unwrap();
    let mut state = fixture::bonds::state();
    state.accounts.insert(
        p.signed.authorization.owner_account,
        Account::new(Balance::new(100_000_000_000)),
    );
    state
        .apply_transition(StateTransition::ApplyNodeBondLifecycle {
            payload: p,
            block_height: fixture::bonds::A,
        })
        .unwrap();
    fixture::bonds::apply(
        &mut state,
        fixture::bonds::auth(20, 2, 0, NodeBondAction::Deposit, 1),
        fixture::bonds::A,
    )
    .unwrap();
    fixture::bonds::advance(&mut state, fixture::H);
    let claim = fixture::claim(&state, 1);
    wrfcoin_shared_protocol::consensus::node_receipt_registry::verify_claim(
        &state,
        &claim,
        fixture::H,
        &fixture::identity(),
    )
    .unwrap();
    let archive = serde_json::json!({"claim_hex":hex::encode(claim.to_wire().unwrap()),"receipt_id":hex::encode(claim_hash(&claim).unwrap()),"included":false});
    std::fs::write(&args[2], serde_json::to_vec(&archive).unwrap()).unwrap();
    if args.len() == 4 {
        let tx: Transaction = serde_json::from_slice(&std::fs::read(&args[3]).unwrap()).unwrap();
        let receipt = NodeReceiptSubmission::parse(&tx).unwrap();
        assert_eq!(receipt.batch.claims, vec![claim]);
        fixture::apply(&mut state, receipt).unwrap();
        println!("NODE_OPERATOR_DEPOSIT_AND_RECEIPT_CONSENSUS_OK");
    } else {
        println!("NODE_OPERATOR_DEPOSIT_CONSENSUS_AND_ARCHIVE_OK");
    }
}
