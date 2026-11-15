//! Advisory governance (`governance::advisory`, unscheduled
//! `advisory_governance`), through the real protocol sink
//! (`ProcessGovernanceTransaction`), the real finalize emitter
//! (`governance_transitions_for_block`), mempool admission and the producer's
//! block self-screen (the shared block classifier).
//!
//! Covers: the activation-height boundary (refused and root byte-identical
//! below H), signature and deposit-owner binding, length caps, the electorate
//! snapshot, votes, every finalize outcome, the absence of any execution path
//! (parameters, laws and other accounts unchanged; supply conserved), the
//! legacy unsigned `GovernanceProposal` still refused with the flag on, and
//! replay determinism.
//!
//! ONE test function: the feature schedule is process-wide.

use std::sync::Arc;

use tokio::sync::RwLock;

use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::{State, StateError, StateTransition};
use wrfcoin_shared_protocol::blockchain::state_snapshot::{export_state_entries, import_state_entries};
use wrfcoin_shared_protocol::blockchain::transaction::{
    GovernanceProposalPayload, GovernanceVotePayload, Transaction, TransactionInput, TransactionOutput,
    TransactionType,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, feature_active, refresh_feature_schedule_extensions,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::advisory::{
    ADVISORY_DEPOSIT, ADVISORY_VOTING_PERIOD_BLOCKS, AdvisoryAction, AdvisoryStatus,
    MAX_ACTIVE_PROPOSALS, MAX_SUMMARY_BYTES, MAX_TITLE_BYTES, MIN_VOTER_SPENDABLE,
    SignedAdvisoryAction, proposal_id,
};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

const H: u64 = 10;
const WRF: u128 = 100_000_000;

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).unwrap()
}
fn address_of(k: &PrivateKey) -> Address {
    Address::from_public_key(&PublicKey::from_private_key(k))
}

fn wrap(signed: &SignedAdvisoryAction, sender: Address, nonce: u64, tag: &str) -> Transaction {
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("adv-{tag}-{nonce}-{sender}")),
            vout: 0,
            signature: None,
            public_key: Some(signed.public_key.clone()),
            coinbase_data: None,
        }],
        vec![TransactionOutput { amount: 0, address: sender.to_string(), script: "OP_TRUE".into(), nft_data: None }],
    );
    tx.tx_type = TransactionType::AdvisoryGovernance;
    tx.sender_address = sender;
    tx.nonce = nonce;
    tx.data = Some(serde_json::to_string(signed).unwrap());
    tx.set_id();
    tx
}

fn sign(k: &PrivateKey, action: AdvisoryAction, nonce: u64) -> SignedAdvisoryAction {
    let identity = ChainIdentity::active();
    SignedAdvisoryAction::sign(&identity.chain_id, &identity.network_id, k, action, nonce)
}

fn propose_tx(k: &PrivateKey, title: &str, summary: &str, nonce: u64) -> Transaction {
    let signed = sign(k, AdvisoryAction::Propose { title: title.into(), summary: summary.into() }, nonce);
    wrap(&signed, address_of(k), nonce, "propose")
}

fn vote_tx(k: &PrivateKey, id: &str, approve: bool, nonce: u64) -> Transaction {
    let signed = sign(k, AdvisoryAction::Vote { proposal_id: id.into(), approve }, nonce);
    wrap(&signed, address_of(k), nonce, "vote")
}

fn process(state: &mut State, tx: Transaction, height: u64) -> Result<(), StateError> {
    state.apply_transition(StateTransition::ProcessGovernanceTransaction { tx, current_block: height })
}

fn refused(result: Result<(), StateError>, code: &str) {
    let err = format!("{:?}", result.expect_err("must be refused"));
    assert!(err.contains(code), "expected {code}, got {err}");
}

/// The block's governance transitions (the producer's own emitter), applied
/// as one batch, as producer, applier and WAL replay all do.
fn apply_block(state: &mut State, txs: &[Transaction], height: u64) -> Vec<StateTransition> {
    let transitions = state.governance_transitions_for_block(txs, height, 1_800_000_000 + height as i64);
    state.apply_transitions_batch(transitions.clone()).expect("block applies");
    transitions
}

fn balance(state: &State, who: &Address) -> (u128, u128, u64) {
    state
        .get_account(who)
        .map(|a| (a.balance.into_inner(), a.spendable_balance.into_inner(), a.nonce))
        .unwrap_or((0, 0, 0))
}

async fn packed(next_height: u64, funded: &[(Address, u128)], txs: &[Transaction]) -> Vec<Transaction> {
    let mut chain = Blockchain::default();
    chain.height = next_height - 1;
    for (who, amount) in funded {
        chain.state.create_account(*who, Balance::new(*amount));
    }
    let chain = Arc::new(RwLock::new(chain));
    let mempool = Arc::new(RwLock::new(Mempool::new(&Config::default())));
    {
        let mut pool = mempool.write().await;
        for tx in txs {
            // Admission's gate is asserted separately; force every tx into
            // the pool so the producer's self-screen is what decides.
            let _ = pool.add_transaction_with_signing_keys_at_height(tx.clone(), None, Some(next_height));
            let _ = pool.add_transaction(tx.clone());
        }
    }
    let mut pop = ProofOfPublishing::new(1, 60, 0, 64);
    pop.declare_no_genesis_oracles();
    let block = pop
        .create_next_block(chain, mempool, "wrf-advisory-miner")
        .await
        .expect("the producer must seal a block");
    assert_eq!(block.height, next_height);
    block.transactions
}

#[tokio::test]
async fn advisory_governance_end_to_end() {
    // ── Unscheduled: inert at every height ──────────────────────────────────
    refresh_feature_schedule_extensions(Vec::new());
    for h in [0, H, 1_000_000] {
        assert!(!feature_active(ConsensusFeature::AdvisoryGovernance, h));
    }
    let tony = key(0x11);
    let me = address_of(&tony);
    let unscheduled = propose_tx(&tony, "Welcome to governance", "It changes nothing.", 0);
    let err = Mempool::new(&Config::default())
        .add_transaction_with_signing_keys_at_height(unscheduled.clone(), None, Some(H))
        .unwrap_err()
        .to_string();
    assert!(err.contains("SP_ADVISORY_GOVERNANCE_INACTIVE"), "{err}");
    let block = packed(H, &[(me, 10 * WRF)], &[unscheduled.clone()]).await;
    assert!(!block.iter().any(|t| t.id == unscheduled.id), "producer self-screen drops it");

    // ── Scheduled at H ───────────────────────────────────────────────────────
    refresh_feature_schedule_extensions(vec![("advisory_governance".to_string(), H)]);
    assert!(!feature_active(ConsensusFeature::AdvisoryGovernance, H - 1));
    assert!(feature_active(ConsensusFeature::AdvisoryGovernance, H));

    let voters: Vec<PrivateKey> = (0x21..=0x25).map(key).collect();
    let poor = key(0x31);
    let mut state = State::new();
    state.create_account(me, Balance::new(10 * WRF));
    for v in &voters {
        state.create_account(address_of(v), Balance::new(2 * WRF));
    }
    // Exactly 1 WRF is in; one base unit under is out.
    state.create_account(address_of(&poor), Balance::new(WRF - 1));
    let exactly = key(0x32);
    state.create_account(address_of(&exactly), Balance::new(WRF));
    state.recalculate_state_root();

    // --- Activation boundary: below H refused at all three boundaries and
    // nothing, not even the root, changes.
    let root_before = state.state_root;
    let welcome = propose_tx(&tony, "Welcome to governance", "The first advisory proposal. It changes nothing.", 0);
    assert!(matches!(process(&mut state, welcome.clone(), H - 1), Err(StateError::InvalidTransition)));
    assert!(state.advisory_proposals.is_empty());
    assert_eq!(state.state_root, root_before);
    assert!(
        !state.state_root_material().windows(29).any(|w| w == b"wrfcoin_advisory_proposals_v1"),
        "no advisory section while empty"
    );
    // Mempool admission checks the NEXT height: at tip H-2 it is refused, at
    // tip H-1 (next = H) it is admitted.
    let err = Mempool::new(&Config::default())
        .add_transaction_with_signing_keys_at_height(welcome.clone(), None, Some(H - 2))
        .unwrap_err()
        .to_string();
    assert!(err.contains("SP_ADVISORY_GOVERNANCE_INACTIVE"), "{err}");
    Mempool::new(&Config::default())
        .add_transaction_with_signing_keys_at_height(welcome.clone(), None, Some(H - 1))
        .expect("admitted once active at the next height");
    let block = packed(H - 1, &[(me, 10 * WRF)], &[welcome.clone()]).await;
    assert!(!block.iter().any(|t| t.id == welcome.id), "a block below H cannot carry it");
    let block = packed(H, &[(me, 10 * WRF)], &[welcome.clone()]).await;
    assert!(block.iter().any(|t| t.id == welcome.id), "a block at H carries it");

    // --- The legacy unsigned GovernanceProposal stays refused with the flag ON.
    let mut legacy = Transaction::new(
        vec![TransactionInput { txid: hash_string("legacy"), vout: 0, signature: None, public_key: None, coinbase_data: None }],
        vec![TransactionOutput { amount: 0, address: me.to_string(), script: "OP_TRUE".into(), nft_data: None }],
    );
    legacy.tx_type = TransactionType::GovernanceProposal;
    legacy.sender_address = me;
    legacy.data = Some(
        serde_json::to_string(&GovernanceProposalPayload {
            proposal_id: "law-create".into(),
            parameter: "law.create".into(),
            value: "{}".into(),
            voting_deadline: None,
            deposit: None,
        })
        .unwrap(),
    );
    legacy.set_id();
    let err = Mempool::new(&Config::default())
        .add_transaction_with_signing_keys_at_height(legacy.clone(), None, Some(H + 5))
        .unwrap_err()
        .to_string();
    assert!(err.contains("#2024"), "{err}");
    let block = packed(H + 5, &[(me, 10 * WRF)], &[legacy.clone()]).await;
    assert!(!block.iter().any(|t| t.id == legacy.id), "legacy proposal never in a block");

    // --- Signature and deposit owner. An attacker cannot make a victim pay:
    // naming the victim as sender breaks `parse` (account != sender); naming
    // the victim as the signed account breaks `verify` (key != account).
    let attacker = key(0x41);
    state.create_account(address_of(&attacker), Balance::new(5 * WRF));
    let victim = address_of(&voters[0]);
    let signed = sign(&attacker, AdvisoryAction::Propose { title: "x".into(), summary: "y".into() }, 0);
    refused(process(&mut state, wrap(&signed, victim, 0, "steal-sender"), H), "SP_ADVISORY_GOVERNANCE_PAYLOAD_INVALID");
    let mut forged = signed.clone();
    forged.account = victim;
    refused(process(&mut state, wrap(&forged, victim, 0, "steal-account"), H), "public key does not hash to the account");
    let mut bad_sig = sign(&tony, AdvisoryAction::Propose { title: "x".into(), summary: "y".into() }, 0);
    bad_sig.signature = sign(&tony, AdvisoryAction::Propose { title: "z".into(), summary: "y".into() }, 0).signature;
    refused(process(&mut state, wrap(&bad_sig, me, 0, "bad-sig"), H), "signature does not verify");
    // Wrong nonce: the signed nonce must equal tx.nonce and the account nonce.
    refused(process(&mut state, propose_tx(&tony, "x", "y", 3), H), "InvalidNonce");
    assert_eq!(balance(&state, &victim), (2 * WRF, 2 * WRF, 0), "the victim is untouched");
    assert!(state.advisory_proposals.is_empty());

    // --- Length caps through the sink.
    refused(process(&mut state, propose_tx(&tony, &"t".repeat(MAX_TITLE_BYTES + 1), "y", 0), H), "title");
    refused(process(&mut state, propose_tx(&tony, "t", &"s".repeat(MAX_SUMMARY_BYTES + 1), 0), H), "summary");
    refused(process(&mut state, propose_tx(&tony, "t\u{202E}", "s", 0), H), "U+202E");
    // A payload naming a parameter is not even parsed.
    let mut with_parameter: serde_json::Value = serde_json::from_str(welcome.data.as_ref().unwrap()).unwrap();
    with_parameter["parameter"] = serde_json::json!("emission_rate");
    let mut param_tx = welcome.clone();
    param_tx.data = Some(with_parameter.to_string());
    param_tx.set_id();
    refused(process(&mut state, param_tx, H), "SP_ADVISORY_GOVERNANCE_PAYLOAD_INVALID");

    // --- Propose at H. Snapshot the parts nothing advisory may touch.
    let params_before = serde_json::to_string(&state.governance.params).unwrap();
    let laws_before = serde_json::to_string(&state.governance.law_book).unwrap();
    let held_before = state.supply_ledger().unwrap().held().unwrap();
    let others_before: Vec<_> = voters.iter().map(|v| balance(&state, &address_of(v))).collect();
    let mut replica = state.clone();

    let welcome = propose_tx(&tony, "Welcome to governance", "The first advisory proposal. It changes nothing.", 0);
    let created = apply_block(&mut state, std::slice::from_ref(&welcome), H);
    assert_eq!(created.len(), 1);
    let id = proposal_id(&me, 0);
    let p = state.advisory_proposals.get(&id).expect("recorded").clone();
    assert_eq!(p.proposer, me);
    assert_eq!(p.voting_deadline, H + ADVISORY_VOTING_PERIOD_BLOCKS);
    assert_eq!(p.deposit, Balance::new(u128::from(ADVISORY_DEPOSIT)));
    assert_eq!(balance(&state, &me), (9 * WRF, 9 * WRF, 1), "deposit from the signer only, nonce advanced");
    // Snapshot: tony (10), five voters (2), exactly-1-WRF, attacker (5); not the poor wallet.
    assert_eq!(p.eligible_count, 8);
    assert!(p.eligible_voters.contains(&address_of(&exactly)));
    assert!(!p.eligible_voters.contains(&address_of(&poor)));
    assert_eq!(MIN_VOTER_SPENDABLE as u128, WRF);
    assert!(state.state_root_material().windows(29).any(|w| w == b"wrfcoin_advisory_proposals_v1"));
    assert!(state.governance.proposals.is_empty(), "never in the legacy map");
    assert_eq!(state.supply_ledger().unwrap().held().unwrap(), held_before, "deposit is held, not lost");

    // --- One active proposal per wallet.
    refused(process(&mut state, propose_tx(&tony, "Second", "s", 1), H + 1), "already has an active proposal");

    // --- Votes: snapshot members only, once, before the deadline, strict nonce.
    refused(process(&mut state, vote_tx(&poor, &id, true, 0), H + 1), "held under 1 WRF");
    // A wallet funded after the snapshot still cannot vote.
    let late = key(0x51);
    state.create_account(address_of(&late), Balance::new(50 * WRF));
    let held_before = held_before + 50 * WRF; // a test-only credit, not issuance
    refused(process(&mut state, vote_tx(&late, &id, true, 0), H + 1), "held under 1 WRF");
    refused(process(&mut state, vote_tx(&voters[0], &id, true, 1), H + 1), "InvalidNonce");
    apply_block(&mut state, &[vote_tx(&voters[0], &id, true, 0), vote_tx(&voters[1], &id, true, 0)], H + 1);
    refused(process(&mut state, vote_tx(&voters[0], &id, false, 1), H + 2), "already voted");
    // The legacy GovernanceVote cannot reach an advisory record.
    let mut legacy_vote = Transaction::new(Vec::new(), Vec::new());
    legacy_vote.tx_type = TransactionType::GovernanceVote;
    legacy_vote.sender_address = address_of(&voters[2]);
    legacy_vote.data = Some(
        serde_json::to_string(&GovernanceVotePayload {
            proposal_id: id.clone(),
            approve: false,
            voting_power: None,
            signature: None,
            signer_public_key: None,
        })
        .unwrap(),
    );
    assert!(process(&mut state, legacy_vote, H + 2).is_err());
    apply_block(&mut state, &[vote_tx(&voters[2], &id, false, 0)], H + 2);
    let deadline = H + ADVISORY_VOTING_PERIOD_BLOCKS;
    refused(process(&mut state, vote_tx(&voters[3], &id, true, 0), deadline), "voting period has ended");
    assert!(state.governance_transitions_for_block(&[], deadline - 1, 0).is_empty(), "not due yet");

    // --- Finalize at the deadline: 2 for, 1 against = passed (66%). Only the
    // status and the deposit change: no parameter, law or other account.
    let finalize = apply_block(&mut state, &[], deadline);
    assert!(matches!(finalize.as_slice(), [StateTransition::FinalizeAdvisoryProposal { .. }]));
    let p = state.advisory_proposals.get(&id).unwrap().clone();
    assert_eq!(p.status, AdvisoryStatus::Passed);
    assert_eq!((p.yes_votes, p.no_votes, p.eligible_count), (2, 1, 8));
    assert!(p.eligible_voters.is_empty() && p.votes.is_empty(), "per-voter maps cleared");
    assert_eq!(p.deposit, Balance::new(0));
    assert_eq!(balance(&state, &me), (10 * WRF, 10 * WRF, 1), "deposit returned to the proposer");
    assert_eq!(serde_json::to_string(&state.governance.params).unwrap(), params_before);
    assert_eq!(serde_json::to_string(&state.governance.law_book).unwrap(), laws_before);
    assert!(state.governance.proposals.is_empty());
    for (v, before) in voters.iter().zip(&others_before) {
        let (b, s, _) = balance(&state, &address_of(v));
        assert_eq!((b, s), (before.0, before.1), "voters' balances untouched");
    }
    assert_eq!(state.supply_ledger().unwrap().held().unwrap(), held_before);
    assert!(state.governance_transitions_for_block(&[], deadline + 1, 0).is_empty(), "finalized once");
    refused(
        state.apply_transition(StateTransition::FinalizeAdvisoryProposal { proposal_id: id.clone(), current_block: deadline + 1 }),
        "not active",
    );

    // --- Rejected (returned) and failed quorum (held for good).
    let t = deadline + 10;
    apply_block(&mut state, &[propose_tx(&tony, "Rejected one", "s", 1)], t);
    let rejected = proposal_id(&me, 1);
    apply_block(
        &mut state,
        &[vote_tx(&voters[0], &rejected, false, 1), vote_tx(&voters[1], &rejected, false, 1), vote_tx(&voters[2], &rejected, true, 1)],
        t + 1,
    );
    apply_block(&mut state, &[propose_tx(&voters[4], "No quorum", "s", 0)], t + 2);
    let quiet = proposal_id(&address_of(&voters[4]), 0);
    apply_block(&mut state, &[vote_tx(&voters[3], &quiet, true, 0)], t + 3);
    let due = apply_block(&mut state, &[], t + 2 + ADVISORY_VOTING_PERIOD_BLOCKS);
    assert_eq!(due.len(), 2, "both due by then, in id order");
    assert_eq!(state.advisory_proposals[&rejected].status, AdvisoryStatus::Rejected);
    assert_eq!(state.advisory_proposals[&rejected].deposit, Balance::new(0));
    assert_eq!(state.advisory_proposals[&quiet].status, AdvisoryStatus::FailedQuorum);
    assert_eq!(state.advisory_proposals[&quiet].deposit, Balance::new(WRF), "held after a failed quorum");
    assert_eq!(balance(&state, &address_of(&voters[4])).0, WRF, "2 WRF minus the held deposit");
    assert_eq!(state.supply_ledger().unwrap().held().unwrap(), held_before, "supply conserved at every step");

    // --- 20 active proposals on chain at most.
    let mut busy = State::new();
    let crowd: Vec<PrivateKey> = (0x60..0x60 + MAX_ACTIVE_PROPOSALS as u8 + 1).map(key).collect();
    for k in &crowd {
        busy.create_account(address_of(k), Balance::new(2 * WRF));
    }
    for (i, k) in crowd.iter().enumerate().take(MAX_ACTIVE_PROPOSALS) {
        process(&mut busy, propose_tx(k, "slot", "s", 0), H + i as u64).unwrap();
    }
    refused(process(&mut busy, propose_tx(crowd.last().unwrap(), "slot", "s", 0), H + 30), "maximum number of active proposals");
    // Two proposals per block at most (the classifier cap), so the producer
    // carries two of three.
    let three: Vec<Transaction> = crowd[..3].iter().map(|k| propose_tx(k, "cap", "s", 0)).collect();
    let funded: Vec<(Address, u128)> = crowd[..3].iter().map(|k| (address_of(k), 2 * WRF)).collect();
    let block = packed(H + 1, &funded, &three).await;
    assert_eq!(block.iter().filter(|t| t.tx_type == TransactionType::AdvisoryGovernance).count(), 2);

    // --- Replay determinism: the same blocks on an independent copy give the
    // same root, and the same proposal cannot apply twice.
    let welcome = propose_tx(&tony, "Welcome to governance", "The first advisory proposal. It changes nothing.", 0);
    apply_block(&mut replica, std::slice::from_ref(&welcome), H);
    apply_block(&mut replica, &[vote_tx(&voters[0], &id, true, 0), vote_tx(&voters[1], &id, true, 0)], H + 1);
    apply_block(&mut replica, &[vote_tx(&voters[2], &id, false, 0)], H + 2);
    apply_block(&mut replica, &[], deadline);
    replica.recalculate_state_root();
    assert_eq!(
        replica.advisory_proposals.get(&id),
        state.advisory_proposals.get(&id),
        "same record on an independent replay"
    );
    let mut again = replica.clone();
    refused(process(&mut again, welcome, deadline + 1), "InvalidNonce");
    let mut a = State::new();
    let mut b = State::new();
    for s in [&mut a, &mut b] {
        s.create_account(me, Balance::new(10 * WRF));
        s.create_account(address_of(&voters[0]), Balance::new(2 * WRF));
        s.recalculate_state_root();
        apply_block(s, &[propose_tx(&tony, "Same", "Same", 0)], H);
        apply_block(s, &[vote_tx(&voters[0], &proposal_id(&me, 0), true, 0)], H + 1);
        apply_block(s, &[], H + ADVISORY_VOTING_PERIOD_BLOCKS);
    }
    assert_eq!(a.state_root, b.state_root);
    assert_eq!(a.state_root_material(), b.state_root_material());

    // --- Snapshot carries the map and restores the same root; an empty map
    // is not exported at all.
    let entries = export_state_entries(&state).unwrap();
    assert!(entries.iter().any(|(k, _)| String::from_utf8_lossy(k).contains("advisory_proposals")));
    let restored = import_state_entries(&State::new(), &entries).unwrap();
    assert_eq!(restored.advisory_proposals, state.advisory_proposals);
    assert_eq!(restored.state_root, state.state_root);
    let mut empty = state.clone();
    empty.advisory_proposals.clear();
    let entries = export_state_entries(&empty).unwrap();
    assert!(!entries.iter().any(|(k, _)| String::from_utf8_lossy(k).contains("advisory_proposals")));
}
