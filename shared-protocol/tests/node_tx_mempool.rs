//! Node-service transactions in the pool: conflict key, caps, stale copies and the state-checked
//! half of admission. These transactions spend a constant, unsigned input by construction.
#![allow(clippy::unwrap_used)]
#[path = "common/node_receipt_fixture.rs"]
mod fixture;
use fixture::H;
use wrfcoin_shared_protocol::{
    blockchain::{
        state::{State, StateTransition},
        transaction::{Transaction, TransactionInput, TransactionOutput, TransactionType},
    },
    common::types::Balance,
    config::settings::Config,
    consensus::{
        node_bond_registry::NODE_BOND_UNITS, node_fraud, node_receipt_registry::*,
        node_share_contracts::*,
    },
    crypto::hash::hash_bytes,
    mempool::{
        node_admission::{self, MAX_PENDING_NODE_TXS_PER_KIND, NodeRefusal},
        pool::Mempool,
    },
};

/// A state with owners `id * 10` bonded as deployments `[id; 32]`, tipped at `H`.
fn licensed(ids: std::ops::RangeInclusive<u8>) -> State {
    let mut s = fixture::bonds::state();
    for id in ids {
        let owner = id * 10;
        s.create_account(
            fixture::bonds::address(owner),
            Balance::new(u128::from(NODE_BOND_UNITS) * 3),
        );
        fixture::bonds::apply(
            &mut s,
            fixture::bonds::auth(owner, id, 0, NodeBondAction::Deposit, 1),
            fixture::bonds::A,
        )
        .unwrap();
    }
    fixture::bonds::advance(&mut s, H);
    s
}
fn owner_claim(s: &State, id: u8, ticket: u8) -> BondedServiceClaim {
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
fn signed(claims: Vec<BondedServiceClaim>, signer: u8, nonce: u64) -> NodeReceiptSubmission {
    let batch = NodeServiceBatch { claims };
    let authorization = NodeReceiptSubmissionAuthorization {
        genesis_hash: [7; 32],
        sender: fixture::bonds::address(signer),
        nonce,
        batch_hash: batch_hash(&batch).unwrap(),
    };
    NodeReceiptSubmission {
        owner_signature: fixture::sign(&authorization, signer),
        authorization,
        batch,
        owner_key: fixture::pk(signer),
    }
}
/// tools/cli/WrfcoinCLI/src/commands/node_receipt.rs `assemble`, byte for byte on the wrapper.
fn as_cli(p: &NodeReceiptSubmission) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_bytes(b"wrfcoin.node_receipt_submission.input.v1"),
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
    tx.nonce = p.authorization.nonce;
    tx.fee = 1_000_000;
    tx.data = Some(hex::encode(p.to_wire().unwrap()));
    tx.set_id();
    tx
}
fn receipt(s: &State, id: u8, ticket: u8, nonce: u64) -> Transaction {
    as_cli(&signed(vec![owner_claim(s, id, ticket)], id * 10, nonce))
}
fn deposit_as_cli(owner: u8, id: u8, nonce: u64) -> Transaction {
    let mut tx = fixture::bonds::transaction(
        &fixture::bonds::auth(owner, id, nonce, NodeBondAction::Deposit, 2),
        1_000_000,
    );
    tx.inputs[0].txid = hash_bytes(b"wrfcoin.node_bond_deposit.input.v1");
    tx.set_id();
    tx
}
fn add(pool: &mut Mempool, tx: &Transaction) -> Result<(), String> {
    pool.add_transaction_with_signing_keys_at_height(tx.clone(), None, Some(H))
        .map_err(|e| e.to_string())
}
fn pool() -> Mempool {
    Mempool::new(&Config::default())
}

#[test]
fn two_nodes_receipts_coexist_on_the_constant_input() {
    let s = licensed(1..=3);
    let (a, b, c) = (
        receipt(&s, 1, 1, 1),
        receipt(&s, 2, 1, 1),
        receipt(&s, 3, 1, 1),
    );
    assert_eq!(
        a.inputs[0].txid, b.inputs[0].txid,
        "the fixture must use the shared CLI input"
    );
    let mut p = pool();
    assert_eq!(add(&mut p, &a), Ok(()));
    assert_eq!(
        add(&mut p, &b),
        Ok(()),
        "a second owner must not contend with the first"
    );
    assert_eq!(add(&mut p, &c), Ok(()));
    assert_eq!(p.size(), 3);
    // Draining one leaves the rest pending.
    p.drain_confirmed_block_txs(&[a.id.unwrap()]);
    assert_eq!(p.size(), 2);
    assert_eq!(
        add(&mut p, &a),
        Ok(()),
        "the slot is the owner's own, freed when it leaves"
    );
}

#[test]
fn the_conflict_key_is_sender_and_nonce() {
    let s = licensed(1..=2);
    let mut p = pool();
    let first = receipt(&s, 1, 1, 1);
    assert_eq!(add(&mut p, &first), Ok(()));
    // Same owner, same nonce, a different signed batch: the only real conflict.
    let other = receipt(&s, 1, 2, 1);
    assert_ne!(first.id, other.id);
    let refused = add(&mut p, &other).unwrap_err();
    assert!(refused.contains("SP_NODE_NONCE_PENDING"), "{refused}");
    // The next nonce is fine, the chain takes two per owner per block; a third pending is not.
    assert_eq!(add(&mut p, &receipt(&s, 1, 3, 2)), Ok(()));
    let capped = add(&mut p, &receipt(&s, 1, 4, 3)).unwrap_err();
    assert!(capped.contains("SP_NODE_SENDER_CAP"), "{capped}");
    assert_eq!(p.size(), 2);
    // Removal frees both indexes.
    p.remove_transaction(&first.id.unwrap());
    assert_eq!(add(&mut p, &other), Ok(()));
}

#[test]
fn a_kind_is_keyed_apart_from_the_others() {
    let s = licensed(1..=1);
    let mut p = pool();
    assert_eq!(add(&mut p, &receipt(&s, 1, 1, 1)), Ok(()));
    let exit =
        fixture::bonds::transaction(&fixture::bonds::auth(10, 1, 1, NodeBondAction::Exit, 2), 1);
    assert_eq!(
        add(&mut p, &exit),
        Ok(()),
        "an owner's exit is not blocked by its own pending receipt"
    );
}

#[test]
fn a_regular_spend_is_unaffected_and_a_node_tx_cannot_claim_its_outpoint() {
    let s = licensed(1..=1);
    let outpoint = hash_bytes(b"a real outpoint");
    let regular = |tag: &str| {
        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: outpoint,
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                address: tag.into(),
                amount: 1,
                script: "OP_TRUE".into(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::Regular;
        tx.set_id();
        tx
    };
    let mut squatter = receipt(&s, 1, 1, 1);
    squatter.inputs[0].txid = outpoint;
    squatter.set_id();
    let mut p = pool();
    assert_eq!(add(&mut p, &squatter), Ok(()));
    assert_eq!(
        add(&mut p, &regular("a")),
        Ok(()),
        "a node tx holds no outpoint"
    );
    let refused = add(&mut p, &regular("b")).unwrap_err();
    assert_eq!(
        refused, "Double spend detected in mempool",
        "ordinary double spends are still refused"
    );
}

#[test]
fn a_rewrapped_copy_of_a_signed_batch_cannot_starve_the_owner() {
    // The owner's signature covers sender, nonce and batch, not the wrapper around them.
    let s = licensed(1..=1);
    let honest = receipt(&s, 1, 1, 1);
    let mut low = honest.clone();
    low.fee = 1;
    low.set_id();
    let mut p = pool();
    assert_eq!(
        add(&mut p, &low),
        Ok(()),
        "well-formed and carries the same signed batch"
    );
    // A better-paying copy of the same batch replaces it; the batch is what the chain applies.
    assert_eq!(add(&mut p, &honest), Ok(()));
    assert!(p.contains(&honest.id.unwrap()));
    assert!(!p.contains(&low.id.unwrap()));
    assert_eq!(p.size(), 1);
    // The cheaper copy cannot displace it again, and finds the pending copy.
    assert!(
        add(&mut p, &low)
            .unwrap_err()
            .contains("SP_NODE_NONCE_PENDING")
    );
    assert_eq!(p.pending_node_copy(&low).unwrap().id, honest.id);
    assert_eq!(p.pending_node_copy(&honest).unwrap().id, honest.id);
    // A different batch is not a copy.
    assert!(p.pending_node_copy(&receipt(&s, 1, 9, 1)).is_none());
}

#[test]
fn a_malformed_wrapper_is_refused_before_it_can_hold_a_slot() {
    let s = licensed(1..=1);
    let honest = receipt(&s, 1, 1, 1);
    type Edit = fn(&mut Transaction);
    let edits: [(&str, Edit); 7] = [
        ("expired", |t| t.timestamp -= chrono::Duration::hours(2)),
        ("post-dated", |t| t.timestamp += chrono::Duration::hours(2)),
        ("second input", |t| {
            let again = t.inputs[0].clone();
            t.inputs.push(again);
            t.inputs[1].vout = 1;
        }),
        ("second output", |t| {
            let again = t.outputs[0].clone();
            t.outputs.push(again);
        }),
        ("paying output", |t| t.outputs[0].amount = 1),
        ("padded script", |t| t.outputs[0].script = "x".repeat(4096)),
        ("input signature", |t| {
            t.inputs[0].signature = Some("00".repeat(64))
        }),
    ];
    for (name, edit) in edits {
        let mut tx = honest.clone();
        edit(&mut tx);
        tx.set_id();
        let mut p = pool();
        let refused = add(&mut p, &tx).expect_err(name);
        assert!(refused.contains("SP_NODE_WRAPPER"), "{name}: {refused}");
        assert_eq!(p.size(), 0, "{name}");
        assert_eq!(
            add(&mut p, &honest),
            Ok(()),
            "{name} must leave the owner's slot free"
        );
    }
    // An id that is not this transaction's hash cannot squat on another transaction's id.
    let mut forged = receipt(&s, 1, 2, 1);
    forged.id = honest.id;
    let mut p = pool();
    assert!(
        add(&mut p, &forged)
            .unwrap_err()
            .contains("SP_NODE_WRAPPER")
    );
    assert!(!p.contains(&honest.id.unwrap()));
}

#[test]
fn an_unbonded_key_cannot_hold_the_slot_or_get_past_the_gate() {
    let s = licensed(1..=2);
    let honest = receipt(&s, 1, 1, 1);
    // Copying a real claim does not license a free key, nor does naming a real deployment.
    let junk = as_cli(&signed(vec![owner_claim(&s, 1, 3)], 99, 0));
    assert_eq!(
        node_admission::check(&s, &junk, H),
        Err(NodeRefusal::NotBonded)
    );
    let wrong_owner = as_cli(&signed(vec![owner_claim(&s, 2, 3)], 10, 1));
    assert_eq!(
        node_admission::check(&s, &wrong_owner, H),
        Err(NodeRefusal::NotBonded)
    );
    assert_eq!(node_admission::check(&s, &honest, H), Ok(()));
    // Even with the gate skipped, it holds only its own key.
    let mut p = pool();
    assert_eq!(add(&mut p, &junk), Ok(()));
    assert_eq!(
        add(&mut p, &honest),
        Ok(()),
        "junk must not block an honest owner"
    );
}

#[test]
fn a_flood_of_unbonded_keys_stops_at_the_gate_and_is_bounded_in_the_pool() {
    let s = licensed(1..=1);
    let flood: Vec<Transaction> = (100..=250u8)
        .map(|k| as_cli(&signed(vec![owner_claim(&s, 1, 3)], k, 0)))
        .collect();
    assert_eq!(flood.len(), 151);
    assert!(flood.len() > MAX_PENDING_NODE_TXS_PER_KIND);
    for tx in &flood {
        assert_eq!(
            node_admission::check(&s, tx, H),
            Err(NodeRefusal::NotBonded)
        );
    }
    // What the pool does if a caller skips the gate: bounded, and refused rather than evicting.
    let mut p = pool();
    let mut refused = 0;
    for tx in &flood {
        if let Err(e) = add(&mut p, tx) {
            assert!(e.contains("SP_NODE_KIND_CAP"), "{e}");
            refused += 1;
        }
    }
    assert_eq!(p.size(), MAX_PENDING_NODE_TXS_PER_KIND);
    assert_eq!(refused, flood.len() - MAX_PENDING_NODE_TXS_PER_KIND);
    assert_eq!(p.pending_node_txs().len(), MAX_PENDING_NODE_TXS_PER_KIND);
    // Every removal path gives the slots back.
    let mut aged = p.clone();
    let long_ago = std::time::Instant::now() - std::time::Duration::from_secs(60);
    for id in aged.transaction_ids() {
        aged.set_transaction_timestamp_for_test(&id, long_ago);
    }
    assert_eq!(
        aged.clone().purge_older_than_secs(1),
        MAX_PENDING_NODE_TXS_PER_KIND
    );
    let mut purged = aged.clone();
    purged.purge_older_than_secs(1);
    assert!(purged.pending_node_txs().is_empty());
    let long_ago = std::time::Instant::now() - std::time::Duration::from_secs(5 * 3600);
    for id in aged.transaction_ids() {
        aged.set_transaction_timestamp_for_test(&id, long_ago);
    }
    assert_eq!(aged.clean_old_transactions(), MAX_PENDING_NODE_TXS_PER_KIND);
    assert!(aged.pending_node_txs().is_empty());
    let ids = p.transaction_ids();
    p.drain_confirmed_block_txs(&ids);
    assert!(p.pending_node_txs().is_empty());
    assert_eq!(add(&mut p, &flood[0]), Ok(()));
    p.clear_all();
    assert!(p.pending_node_txs().is_empty());
}

#[test]
fn gate_reports_used_and_too_far_ahead_nonces() {
    let mut s = licensed(1..=1);
    assert_eq!(node_admission::check(&s, &receipt(&s, 1, 1, 1), H), Ok(()));
    assert_eq!(
        node_admission::check(&s, &receipt(&s, 1, 2, 2), H),
        Ok(()),
        "two per owner per block"
    );
    assert_eq!(
        node_admission::check(&s, &receipt(&s, 1, 3, 3), H),
        Err(NodeRefusal::NonceAhead)
    );
    s.accounts
        .get_mut(&fixture::bonds::address(10))
        .unwrap()
        .nonce = 2;
    assert_eq!(
        node_admission::check(&s, &receipt(&s, 1, 1, 1), H),
        Err(NodeRefusal::NonceUsed)
    );
}

#[test]
fn a_stale_copy_is_recognised_and_a_live_one_is_not() {
    let mut s = licensed(1..=2);
    let now = chrono::Utc::now().timestamp();
    let tx = receipt(&s, 1, 1, 1);
    assert!(!node_admission::is_stale(&s, &tx, H, now), "live copy");
    assert!(
        !node_admission::is_stale(&s, &receipt(&s, 1, 2, 2), H, now),
        "next nonce is early, not stale"
    );
    assert!(
        !node_admission::is_stale(&s, &receipt(&s, 1, 2, 3), H, now),
        "too far ahead is refused, not stale"
    );
    // The clock window bounds how long any copy can sit, mined or not.
    assert!(
        node_admission::is_stale(&s, &tx, H, now + 3601),
        "outside the block submission window"
    );
    // A claim is valid only in its own epoch.
    assert!(
        node_admission::is_stale(&s, &tx, H + NODE_EPOCH_BLOCKS - 1, now),
        "claim epoch has closed"
    );
    // The owner's nonce moved: this copy can never be mined.
    s.accounts
        .get_mut(&fixture::bonds::address(10))
        .unwrap()
        .nonce = 2;
    assert!(node_admission::is_stale(&s, &tx, H, now), "nonce used");
    // A stale copy held on a node that never assembles is what used to block everyone.
    let mut p = pool();
    assert_eq!(add(&mut p, &tx), Ok(()));
    assert_eq!(add(&mut p, &receipt(&s, 2, 1, 1)), Ok(()));
    let stale: Vec<_> = p
        .pending_node_txs()
        .into_iter()
        .filter(|t| node_admission::is_stale(&s, t, H, now))
        .filter_map(|t| t.id)
        .collect();
    assert_eq!(stale, vec![tx.id.unwrap()]);
    p.drain_confirmed_block_txs(&stale);
    assert_eq!(p.size(), 1);
}

#[test]
fn deposits_coexist_and_their_owner_keeps_the_only_slot() {
    let s = licensed(1..=1);
    for owner in [30u8, 40] {
        // Fixture owners 10 and 20 exist; these two need funded accounts to pass the gate.
        assert_eq!(
            node_admission::check(&s, &deposit_as_cli(owner, owner / 10, 0), H),
            Err(NodeRefusal::Unfunded)
        );
    }
    let mut funded = s.clone();
    for owner in [30u8, 40] {
        funded.create_account(
            fixture::bonds::address(owner),
            Balance::new(u128::from(NODE_BOND_UNITS) * 3),
        );
    }
    let (a, b) = (deposit_as_cli(30, 3, 0), deposit_as_cli(40, 4, 0));
    assert_eq!(a.inputs[0].txid, b.inputs[0].txid);
    assert_eq!(node_admission::check(&funded, &a, H), Ok(()));
    let mut p = pool();
    assert_eq!(add(&mut p, &a), Ok(()));
    assert_eq!(
        add(&mut p, &b),
        Ok(()),
        "a second owner's deposit must not contend with the first"
    );
    let again = deposit_as_cli(30, 5, 0);
    assert!(
        add(&mut p, &again)
            .unwrap_err()
            .contains("SP_NODE_NONCE_PENDING")
    );
    // Used nonce: stale, and no longer blocks that owner's next attempt.
    let now = chrono::Utc::now().timestamp();
    funded
        .accounts
        .get_mut(&fixture::bonds::address(30))
        .unwrap()
        .nonce = 1;
    assert!(node_admission::is_stale(&funded, &a, H, now));
    assert!(!node_admission::is_stale(&funded, &b, H, now));
    assert_eq!(p.pending_node_copy(&a).unwrap().id, a.id);
}

#[test]
fn a_free_key_cannot_flood_owner_only_bond_actions() {
    let s = licensed(1..=1);
    // Exit and Withdraw carry no governance co-sign, so the pool's own check is only self-signing.
    let stranger =
        fixture::bonds::transaction(&fixture::bonds::auth(99, 9, 0, NodeBondAction::Exit, 2), 1);
    assert_eq!(
        add(&mut pool(), &stranger),
        Ok(()),
        "the pool alone admits a self-signed exit"
    );
    assert_eq!(
        node_admission::check(&s, &stranger, H),
        Err(NodeRefusal::NotBonded)
    );
    // Naming someone else's deployment is the same refusal.
    let squat =
        fixture::bonds::transaction(&fixture::bonds::auth(20, 1, 0, NodeBondAction::Exit, 2), 1);
    assert_eq!(
        node_admission::check(&s, &squat, H),
        Err(NodeRefusal::NotBonded)
    );
    let owner =
        fixture::bonds::transaction(&fixture::bonds::auth(10, 1, 1, NodeBondAction::Exit, 2), 1);
    assert_eq!(node_admission::check(&s, &owner, H), Ok(()));
    // A forged co-sign never reaches the pool's slot.
    let mut bad = fixture::bonds::auth(30, 3, 0, NodeBondAction::Deposit, 2);
    bad.governance_signature[0] ^= 1;
    let bad = fixture::bonds::transaction(&bad, 1);
    assert!(
        add(&mut pool(), &bad)
            .unwrap_err()
            .contains("SP_NODE_BOND_AUTHORIZATION")
    );
}

#[test]
fn fraud_reports_are_keyed_alike_and_must_be_provable_at_the_gate() {
    let s = fixture::state();
    let mut fake = fixture::claim(&s, 1);
    fake.receipt.signature[0] ^= 1;
    fixture::resign_outer(&mut fake, 11, 22);
    let report = |claims: Vec<BondedServiceClaim>, signer: u8, nonce: u64| {
        let mut p = signed(claims, signer, nonce);
        p.authorization.batch_hash = node_fraud::evidence_hash(&p).unwrap();
        p.owner_signature = fixture::sign(&p.authorization, signer);
        let mut tx = as_cli(&p);
        tx.tx_type = TransactionType::NodeFraudReport;
        tx.inputs[0].txid = hash_bytes(b"wrfcoin.node_fraud_report.input.v1");
        tx.set_id();
        tx
    };
    let (a, b) = (
        report(vec![fake.clone()], 10, 1),
        report(vec![fake.clone()], 20, 1),
    );
    let mut p = pool();
    assert_eq!(add(&mut p, &a), Ok(()));
    assert_eq!(
        add(&mut p, &b),
        Ok(()),
        "a second reporter must not contend with the first"
    );
    assert_eq!(node_admission::check(&s, &a, H), Ok(()));
    // Unprovable, or from an empty account: a free key cannot occupy this slot either.
    assert_eq!(
        node_admission::check(&s, &report(vec![fixture::claim(&s, 2)], 10, 1), H),
        Err(NodeRefusal::Unprovable)
    );
    assert_eq!(
        node_admission::check(&s, &report(vec![fake], 99, 0), H),
        Err(NodeRefusal::Unfunded)
    );
}

#[test]
fn outside_the_bond_registry_the_gate_refuses_and_other_kinds_pass() {
    let bare = State::new();
    let s = licensed(1..=1);
    assert_eq!(
        node_admission::check(&bare, &receipt(&s, 1, 1, 1), H),
        Err(NodeRefusal::Unavailable)
    );
    assert!(
        !node_admission::is_stale(
            &bare,
            &receipt(&s, 1, 1, 1),
            H,
            chrono::Utc::now().timestamp()
        ),
        "no registry: nothing is known"
    );
    let mut regular = Transaction::new(
        vec![TransactionInput {
            txid: hash_bytes(b"x"),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput {
            address: "a".into(),
            amount: 1,
            script: "OP_TRUE".into(),
            nft_data: None,
        }],
    );
    regular.set_id();
    assert_eq!(node_admission::check(&s, &regular, H), Ok(()));
    assert!(
        !node_admission::is_stale(&s, &regular, H, i64::MAX / 2),
        "the sweep touches node kinds only"
    );
    assert!(node_admission::is_node_keyed(
        TransactionType::NodeServiceBatch
    ));
    assert!(node_admission::is_node_keyed(
        TransactionType::NodeBondLifecycle
    ));
    assert!(node_admission::is_node_keyed(
        TransactionType::NodeFraudReport
    ));
    assert!(!node_admission::is_node_keyed(TransactionType::Transfer));
}

#[test]
fn batch_included_sees_only_a_fully_accepted_batch() {
    let mut s = licensed(1..=2);
    let tx = receipt(&s, 1, 1, 1);
    assert!(!node_admission::batch_included(&s, &tx));
    s.apply_transition(StateTransition::BeginNodeServiceBlock {
        block_height: H + 1,
        parent_hash: [3; 32],
    })
    .unwrap();
    let p = NodeReceiptSubmission::parse(&tx).unwrap();
    s.apply_transition(StateTransition::ApplyNodeServiceBatch {
        payload: p,
        block_height: H + 1,
    })
    .unwrap();
    assert!(node_admission::batch_included(&s, &tx));
    assert!(!node_admission::batch_included(&s, &receipt(&s, 1, 2, 2)));
}

fn plain(kind: TransactionType, tag: &str) -> std::sync::Arc<Transaction> {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_bytes(tag.as_bytes()),
            vout: 0,
            signature: None,
            public_key: None,
            coinbase_data: None,
        }],
        vec![TransactionOutput { address: tag.into(), amount: 0, script: "OP_TRUE".into(), nft_data: None }],
    );
    tx.tx_type = kind;
    tx.set_id();
    std::sync::Arc::new(tx)
}

#[test]
fn with_no_node_kinds_selection_is_exactly_fee_order() {
    let txs: Vec<_> = (0..50).map(|i| plain(TransactionType::Regular, &format!("r{i}"))).collect();
    for slots in [0, 1, 7, 50, 80] {
        let picked = node_admission::node_kinds_first(txs.iter(), slots);
        let expected: Vec<_> = txs.iter().take(slots).collect();
        assert_eq!(picked.len(), expected.len());
        assert!(picked.iter().zip(&expected).all(|(a, b)| a.id == b.id), "slots {slots}");
    }
}

#[test]
fn node_kinds_go_first_up_to_their_caps_and_the_rest_keep_their_order() {
    use TransactionType::{NodeBondLifecycle, NodeFraudReport, NodeServiceBatch, Regular, Transfer};
    let mut by_fee = Vec::new();
    // Ordinary traffic outbids everything; node kinds sit behind it, over their caps.
    for i in 0..30 {
        by_fee.push(plain(if i % 2 == 0 { Regular } else { Transfer }, &format!("high{i}")));
    }
    for i in 0..30 {
        by_fee.push(plain(NodeServiceBatch, &format!("batch{i}")));
        by_fee.push(plain(NodeBondLifecycle, &format!("bond{i}")));
        by_fee.push(plain(NodeFraudReport, &format!("fraud{i}")));
    }
    for i in 0..30 {
        by_fee.push(plain(Regular, &format!("low{i}")));
    }
    let count = |picked: &[&std::sync::Arc<Transaction>], kind| picked.iter().filter(|t| t.tx_type == kind).count();
    let picked = node_admission::node_kinds_first(by_fee.iter(), 1000);
    assert_eq!(count(&picked, NodeServiceBatch), MAX_NODE_RECEIPT_BATCHES_PER_BLOCK);
    assert_eq!(count(&picked, NodeBondLifecycle), wrfcoin_shared_protocol::consensus::node_bond_registry::MAX_NODE_BOND_LIFECYCLE_TXS_PER_BLOCK);
    assert_eq!(count(&picked, NodeFraudReport), node_fraud::MAX_NODE_FRAUD_REPORTS_PER_BLOCK as usize);
    let reserved = 16 + 16 + 8;
    assert!(picked[..reserved].iter().all(|t| node_admission::is_node_keyed(t.tx_type)), "node kinds lead");
    // Each node kind keeps fee order, taking its highest payers.
    let batches: Vec<_> = picked.iter().filter(|t| t.tx_type == NodeServiceBatch).map(|t| t.id).collect();
    let expected: Vec<_> = by_fee.iter().filter(|t| t.tx_type == NodeServiceBatch).take(16).map(|t| t.id).collect();
    assert_eq!(batches, expected);
    // Everything else is exactly fee order.
    let rest: Vec<_> = picked[reserved..].iter().map(|t| t.id).collect();
    let others: Vec<_> = by_fee.iter().filter(|t| !node_admission::is_node_keyed(t.tx_type)).map(|t| t.id).collect();
    assert_eq!(rest, others);
    // A full block: ordinary traffic gives up exactly the node slots, from its low-fee end.
    let picked = node_admission::node_kinds_first(by_fee.iter(), 45);
    assert_eq!(picked.len(), 45);
    let rest: Vec<_> = picked[reserved..].iter().map(|t| t.id).collect();
    assert_eq!(rest, others[..5].to_vec());
    // Fewer slots than node transactions: node kinds only, never over the slots.
    let picked = node_admission::node_kinds_first(by_fee.iter(), 3);
    assert_eq!(picked.len(), 3);
    assert!(picked.iter().all(|t| node_admission::is_node_keyed(t.tx_type)));
}

fn from(kind: TransactionType, tag: &str, sender: u8, nonce: u64) -> std::sync::Arc<Transaction> {
    let mut tx = (*plain(kind, tag)).clone();
    tx.sender_address = wrfcoin_shared_protocol::common::types::Address([sender; 20]);
    tx.nonce = nonce;
    tx.set_id();
    std::sync::Arc::new(tx)
}

fn tags(picked: &[&std::sync::Arc<Transaction>], by_fee: &[std::sync::Arc<Transaction>]) -> Vec<usize> {
    picked.iter().map(|p| by_fee.iter().position(|t| t.id == p.id).unwrap()).collect()
}

#[test]
fn a_senders_higher_nonce_node_tx_never_goes_ahead_of_its_lower_one() {
    use TransactionType::{NodeBondLifecycle, NodeServiceBatch, Regular};
    // Nonce 2 outbids nonce 1 from the same owner.
    let by_fee = vec![
        from(NodeServiceBatch, "n2", 1, 2),
        from(Regular, "r", 9, 0),
        from(NodeServiceBatch, "n1", 1, 1),
    ];
    let picked = node_admission::node_kinds_first(by_fee.iter(), 10);
    assert_eq!(tags(&picked, &by_fee), vec![2, 0, 1], "nonce 1 takes the first slot");
    // Across kinds too: a bond action at nonce 1 behind a batch at nonce 2.
    let by_fee = vec![from(NodeServiceBatch, "b2", 1, 2), from(NodeBondLifecycle, "l1", 1, 1)];
    let picked = node_admission::node_kinds_first(by_fee.iter(), 10);
    assert_eq!(tags(&picked, &by_fee), vec![1, 0]);
}

#[test]
fn a_regular_tx_at_n_holds_the_same_senders_node_tx_at_n_plus_one_for_the_next_block() {
    use TransactionType::{NodeServiceBatch, Regular, Transfer};
    let mut by_fee = vec![from(NodeServiceBatch, "batch", 1, 2)];
    for i in 0..5 {
        by_fee.push(from(Regular, &format!("other{i}"), 9, 0));
    }
    by_fee.push(from(Transfer, "own", 1, 1));
    let picked = node_admission::node_kinds_first(by_fee.iter(), 100);
    // The transfer takes the batch's slot. The batch waits for the next block: the trial apply
    // would not see the transfer's nonce. Everyone else keeps fee order.
    assert_eq!(tags(&picked, &by_fee), vec![6, 1, 2, 3, 4, 5]);
    let picked = node_admission::node_kinds_first(by_fee.iter(), 3);
    assert_eq!(tags(&picked, &by_fee), vec![6, 1, 2]);
    // Once the transfer is on chain, the batch is offered first again.
    let next: Vec<_> = by_fee.iter().filter(|t| t.tx_type != Transfer).cloned().collect();
    let picked = node_admission::node_kinds_first(next.iter(), 100);
    assert_eq!(tags(&picked, &next), vec![0, 1, 2, 3, 4, 5]);
    // A node tx at the same nonce as the account's transfer is a conflict, not a successor.
    let same = vec![from(NodeServiceBatch, "same", 1, 1), from(Transfer, "own1", 1, 1)];
    let picked = node_admission::node_kinds_first(same.iter(), 100);
    assert_eq!(tags(&picked, &same), vec![0, 1]);
}

#[test]
fn a_nonce_held_at_a_cap_holds_back_that_senders_later_nonces() {
    use TransactionType::{NodeFraudReport, Transfer};
    let cap = node_fraud::MAX_NODE_FRAUD_REPORTS_PER_BLOCK as usize;
    let mut by_fee: Vec<_> = (0..cap).map(|i| from(NodeFraudReport, &format!("f{i}"), 50 + i as u8, 1)).collect();
    by_fee.push(from(NodeFraudReport, "held", 1, 1));
    by_fee.push(from(Transfer, "later", 1, 2));
    by_fee.push(from(Transfer, "elsewhere", 9, 0));
    let picked = node_admission::node_kinds_first(by_fee.iter(), 100);
    let mut expected: Vec<_> = (0..cap).collect();
    expected.push(cap + 2);
    assert_eq!(tags(&picked, &by_fee), expected);
}

#[test]
fn distinct_senders_keep_node_kinds_first_in_fee_order() {
    use TransactionType::{NodeBondLifecycle, NodeServiceBatch, Regular, Transfer};
    let by_fee = vec![
        from(Regular, "r0", 90, 0),
        from(NodeServiceBatch, "b0", 1, 5),
        from(Transfer, "t0", 91, 0),
        from(NodeBondLifecycle, "l0", 2, 1),
        from(NodeServiceBatch, "b1", 3, 0),
        from(Regular, "r1", 92, 0),
    ];
    let picked = node_admission::node_kinds_first(by_fee.iter(), 100);
    assert_eq!(tags(&picked, &by_fee), vec![1, 3, 4, 0, 2, 5]);
    let picked = node_admission::node_kinds_first(by_fee.iter(), 4);
    assert_eq!(tags(&picked, &by_fee), vec![1, 3, 4, 0]);
}
