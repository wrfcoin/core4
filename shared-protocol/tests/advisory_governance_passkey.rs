//! Advisory governance signed by passkey (P-256 WebAuthn) accounts, under the
//! same unscheduled `advisory_governance` flag, through the real protocol
//! sink (`ProcessGovernanceTransaction`), the finalize emitter
//! (`governance_transitions_for_block`), mempool admission and the producer's
//! block self-screen (the shared block classifier).
//!
//! Covers: dormant while unscheduled (refused everywhere, state root
//! unchanged), the activation boundary (refused below H, accepted at H),
//! every refusal path (bad signature, another relying party, another
//! account, wrong nonce, replay, both or neither authorization), a mixed
//! electorate (ed25519 and passkey proposers and voters) through a full
//! proposal lifecycle, replay determinism, and the P-256 verification charge.
//!
//! ONE test function: the feature schedule is process-wide.

use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator;
use wrfcoin_shared_protocol::accounts::passkey::FLAG_USER_PRESENT;
use wrfcoin_shared_protocol::blockchain::chain::Blockchain;
use wrfcoin_shared_protocol::blockchain::state::{State, StateError, StateTransition};
use wrfcoin_shared_protocol::blockchain::transaction::{
    Transaction, TransactionInput, TransactionOutput, TransactionType,
};
use wrfcoin_shared_protocol::common::types::{Address, Balance};
use wrfcoin_shared_protocol::config::settings::Config;
use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
use wrfcoin_shared_protocol::consensus::verify_cost::VerifyCost;
use wrfcoin_shared_protocol::crypto::chain_binding::{
    ChainIdentity, ConsensusFeature, feature_active, refresh_feature_schedule_extensions,
};
use wrfcoin_shared_protocol::crypto::hash::hash_string;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::governance::advisory::{
    ADVISORY_VOTING_PERIOD_BLOCKS, AdvisoryAction, AdvisoryStatus, SignedAdvisoryAction,
    advisory_passkey_challenge, proposal_id,
};
use wrfcoin_shared_protocol::mempool::pool::Mempool;

const H: u64 = 10;
const WRF: u128 = 100_000_000;
const RP: &str = "wrfcoin.org";

fn key(seed: u8) -> PrivateKey {
    PrivateKey::from_bytes(&[seed; 32]).unwrap()
}
fn ed_address(k: &PrivateKey) -> Address {
    Address::from_public_key(&PublicKey::from_private_key(k))
}
fn passkey(seed: u8) -> SoftwareAuthenticator {
    SoftwareAuthenticator::new(&[seed; 32], RP)
}

fn wrap(signed: &SignedAdvisoryAction, sender: Address, nonce: u64, tag: &str) -> Transaction {
    let public_key = (!signed.public_key.is_empty()).then(|| signed.public_key.clone());
    let mut tx = Transaction::new(
        vec![TransactionInput {
            txid: hash_string(&format!("adv-pk-{tag}-{nonce}-{sender}")),
            vout: 0,
            signature: None,
            public_key,
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

fn propose(title: &str) -> AdvisoryAction {
    AdvisoryAction::Propose { title: title.into(), summary: "Advisory only. It changes nothing.".into() }
}
fn vote(id: &str, approve: bool) -> AdvisoryAction {
    AdvisoryAction::Vote { proposal_id: id.into(), approve }
}

fn pk_signed(a: &SoftwareAuthenticator, action: AdvisoryAction, nonce: u64) -> SignedAdvisoryAction {
    let id = ChainIdentity::active();
    SignedAdvisoryAction::sign_passkey(&id.chain_id, &id.network_id, a, action, nonce)
}
fn pk_tx(a: &SoftwareAuthenticator, action: AdvisoryAction, nonce: u64, tag: &str) -> Transaction {
    wrap(&pk_signed(a, action, nonce), a.account(), nonce, tag)
}
fn ed_tx(k: &PrivateKey, action: AdvisoryAction, nonce: u64, tag: &str) -> Transaction {
    let id = ChainIdentity::active();
    let signed = SignedAdvisoryAction::sign(&id.chain_id, &id.network_id, k, action, nonce);
    wrap(&signed, ed_address(k), nonce, tag)
}

fn process(state: &mut State, tx: Transaction, height: u64) -> Result<(), StateError> {
    state.apply_transition(StateTransition::ProcessGovernanceTransaction { tx, current_block: height })
}

fn refused(result: Result<(), StateError>, code: &str) {
    let err = format!("{:?}", result.expect_err("must be refused"));
    assert!(err.contains(code), "expected {code}, got {err}");
}

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
            let _ = pool.add_transaction_with_signing_keys_at_height(tx.clone(), None, Some(next_height));
            let _ = pool.add_transaction(tx.clone());
        }
    }
    let mut pop = ProofOfPublishing::new(1, 60, 0, 64);
    pop.declare_no_genesis_oracles();
    let block = pop
        .create_next_block(chain, mempool, "wrf-advisory-passkey-miner")
        .await
        .expect("the producer must seal a block");
    assert_eq!(block.height, next_height);
    block.transactions
}

fn admit(tx: &Transaction, tip: u64) -> Result<(), String> {
    Mempool::new(&Config::default())
        .add_transaction_with_signing_keys_at_height(tx.clone(), None, Some(tip))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tokio::test]
async fn passkey_advisory_governance_end_to_end() {
    let alice = passkey(0x11); // passkey proposer
    let bob = passkey(0x12); // passkey voter
    let carol = passkey(0x13); // passkey voter
    let dave = key(0x21); // ed25519 proposer / voter
    let erin = key(0x22); // ed25519 voter
    let (a, b, c, d, e) = (alice.account(), bob.account(), carol.account(), ed_address(&dave), ed_address(&erin));
    let funded = [(a, 10 * WRF), (b, 2 * WRF), (c, 2 * WRF), (d, 10 * WRF), (e, 2 * WRF)];
    let fresh = || {
        let mut s = State::new();
        for (who, amount) in funded {
            s.create_account(who, Balance::new(amount));
        }
        s.recalculate_state_root();
        s
    };

    // ── Unscheduled: dormant at every height, state root unchanged ──────────
    refresh_feature_schedule_extensions(Vec::new());
    for h in [0, H, 1_000_000] {
        assert!(!feature_active(ConsensusFeature::AdvisoryGovernance, h));
    }
    let welcome = pk_tx(&alice, propose("Welcome, passkey wallets"), 0, "welcome");
    let err = admit(&welcome, H).unwrap_err();
    assert!(err.contains("SP_ADVISORY_GOVERNANCE_INACTIVE"), "{err}");
    let block = packed(H, &funded, std::slice::from_ref(&welcome)).await;
    assert!(!block.iter().any(|t| t.id == welcome.id), "producer self-screen drops it");
    let mut dormant = fresh();
    let root = dormant.state_root;
    let material = dormant.state_root_material();
    for h in [0, H, 1_000_000] {
        assert!(matches!(process(&mut dormant, welcome.clone(), h), Err(StateError::InvalidTransition)));
        let vote_tx = pk_tx(&bob, vote(&proposal_id(&a, 0), true), 0, "dormant-vote");
        assert!(matches!(process(&mut dormant, vote_tx, h), Err(StateError::InvalidTransition)));
        // The block emitter adds nothing for it (no proposal, no finalize).
        assert!(dormant.governance_transitions_for_block(&[], h, 0).is_empty());
    }
    dormant.recalculate_state_root();
    assert_eq!(dormant.state_root, root, "dormant: root byte-identical");
    assert_eq!(dormant.state_root_material(), material);
    assert!(dormant.advisory_proposals.is_empty());
    assert_eq!(balance(&dormant, &a), (10 * WRF, 10 * WRF, 0));

    // ── Scheduled at H: refused below, accepted at H ─────────────────────────
    refresh_feature_schedule_extensions(vec![("advisory_governance".to_string(), H)]);
    let mut state = fresh();
    let root_before = state.state_root;
    assert!(matches!(process(&mut state, welcome.clone(), H - 1), Err(StateError::InvalidTransition)));
    assert_eq!(state.state_root, root_before);
    assert!(state.advisory_proposals.is_empty());
    let err = admit(&welcome, H - 2).unwrap_err();
    assert!(err.contains("SP_ADVISORY_GOVERNANCE_INACTIVE"), "{err}");
    admit(&welcome, H - 1).expect("admitted once active at the next height");
    let block = packed(H - 1, &funded, std::slice::from_ref(&welcome)).await;
    assert!(!block.iter().any(|t| t.id == welcome.id), "a block below H cannot carry it");
    let block = packed(H, &funded, std::slice::from_ref(&welcome)).await;
    assert!(block.iter().any(|t| t.id == welcome.id), "a block at H carries a passkey proposal");

    // Pricing: one P-256 verification on top of the flat charge, from
    // activation only; below H it is exactly `verify_work`.
    assert_eq!(welcome.verify_work().ecdsa, 0);
    assert_eq!(
        wrfcoin_shared_protocol::consensus::verify_cost::block_verify_work_at_height(std::slice::from_ref(&welcome), H - 1),
        wrfcoin_shared_protocol::consensus::verify_cost::block_verify_work_at_height(std::slice::from_ref(&welcome), 0),
    );
    assert_eq!(
        wrfcoin_shared_protocol::consensus::verify_cost::block_verify_work_at_height(std::slice::from_ref(&welcome), H - 1).ecdsa,
        0
    );
    let ed_welcome = ed_tx(&dave, propose("ed25519 one"), 0, "price");
    assert_eq!(
        wrfcoin_shared_protocol::consensus::verify_cost::block_verify_work_at_height(std::slice::from_ref(&welcome), H).ecdsa,
        1
    );
    assert_eq!(
        wrfcoin_shared_protocol::consensus::verify_cost::block_verify_work_at_height(std::slice::from_ref(&ed_welcome), H).ecdsa,
        0
    );

    // ── Refusal paths, through the sink, mempool and classifier ──────────────
    let id_chain = ChainIdentity::active();
    // Bad signature: an assertion made for another action.
    let mut bad_sig = pk_signed(&alice, propose("Welcome, passkey wallets"), 0);
    bad_sig.passkey = pk_signed(&alice, propose("Something else"), 0).passkey;
    let bad_sig_tx = wrap(&bad_sig, a, 0, "bad-sig");
    refused(process(&mut state, bad_sig_tx.clone(), H), "challenge");
    let err = admit(&bad_sig_tx, H).unwrap_err();
    assert!(err.contains("SP_ADVISORY_GOVERNANCE_AUTHORIZATION"), "{err}");
    let block = packed(H, &funded, std::slice::from_ref(&bad_sig_tx)).await;
    assert!(!block.iter().any(|t| t.id == bad_sig_tx.id), "classifier refuses a bad assertion");
    // A tampered DER signature over the right challenge.
    let mut tampered = pk_signed(&alice, propose("Welcome, passkey wallets"), 0);
    {
        let p = tampered.passkey.as_mut().unwrap();
        let mut der = hex::decode(&p.signature).unwrap();
        let last = der.len() - 1;
        der[last] ^= 0x01;
        p.signature = hex::encode(der);
    }
    refused(process(&mut state, wrap(&tampered, a, 0, "tampered"), H), "p256");
    // Another relying party (a phishing origin gets assertions for its own RP
    // ID only): named for alice's account, its credential derives another
    // account; claiming alice's RP ID hash fails the RP ID check.
    let mut evil = passkey(0x11);
    evil.rp_id_hash = Sha256::digest(b"wrfcoin-login.example").into();
    evil.origin = "https://wrfcoin-login.example".into();
    let action = propose("Welcome, passkey wallets");
    let challenge = advisory_passkey_challenge(&id_chain.chain_id, &id_chain.network_id, &a, &action, 0);
    let mut phished =
        SignedAdvisoryAction::with_passkey(a, action.clone(), 0, evil.authorization_for(&evil.assert(&challenge)));
    refused(process(&mut state, wrap(&phished, a, 0, "rp"), H), "public key does not hash to the account");
    phished.passkey.as_mut().unwrap().rp_id_hash = hex::encode(alice.rp_id_hash);
    refused(process(&mut state, wrap(&phished, a, 0, "rp2"), H), "rpIdHash");
    // The evil RP's own account is a different (unfunded) account.
    assert_ne!(evil.account(), a);
    // User present but not verified (consensus policy demands UV).
    let mut no_uv = passkey(0x11);
    no_uv.flags = FLAG_USER_PRESENT;
    refused(process(&mut state, pk_tx(&no_uv, propose("x"), 0, "no-uv"), H), "user-verified");
    // Naming a victim: as tx sender breaks `parse`; as the signed account
    // breaks the credential-to-account binding. The victim pays nothing.
    let signed = pk_signed(&alice, propose("x"), 0);
    refused(process(&mut state, wrap(&signed, b, 0, "steal-sender"), H), "SP_ADVISORY_GOVERNANCE_PAYLOAD_INVALID");
    let mut forged = signed.clone();
    forged.account = b;
    refused(process(&mut state, wrap(&forged, b, 0, "steal-account"), H), "public key does not hash to the account");
    // Both forms, or an ed25519 key alongside the passkey: not parsed.
    let mut both = signed.clone();
    let ed = SignedAdvisoryAction::sign(&id_chain.chain_id, &id_chain.network_id, &dave, propose("x"), 0);
    both.public_key = ed.public_key;
    both.signature = ed.signature;
    refused(process(&mut state, wrap(&both, a, 0, "both"), H), "SP_ADVISORY_GOVERNANCE_PAYLOAD_INVALID");
    // Wrong nonce: signed for nonce 3, account is at 0.
    refused(process(&mut state, pk_tx(&alice, propose("x"), 3, "nonce"), H), "InvalidNonce");
    // Moving a nonce-0 assertion to nonce 1 breaks the challenge.
    let mut moved = pk_signed(&alice, propose("x"), 0);
    moved.nonce = 1;
    refused(process(&mut state, wrap(&moved, a, 1, "moved"), H), "challenge");
    assert!(state.advisory_proposals.is_empty(), "nothing refused wrote anything");
    assert_eq!(balance(&state, &a), (10 * WRF, 10 * WRF, 0));
    assert_eq!(balance(&state, &b), (2 * WRF, 2 * WRF, 0));
    state.recalculate_state_root();
    assert_eq!(state.state_root, root_before, "refusals leave the root unchanged");

    // ── Mixed electorate, full lifecycle ─────────────────────────────────────
    let mut replica = state.clone();
    let held_before = state.supply_ledger().unwrap().held().unwrap();
    let pk_prop = pk_tx(&alice, propose("Welcome, passkey wallets"), 0, "lifecycle");
    let ed_prop = ed_tx(&dave, propose("Welcome, phrase wallets"), 0, "lifecycle");
    let blocks: Vec<(u64, Vec<Transaction>)> = {
        let p1 = proposal_id(&a, 0);
        let p2 = proposal_id(&d, 0);
        vec![
            (H, vec![pk_prop.clone(), ed_prop.clone()]),
            // Passkey and ed25519 voters on both proposals.
            (H + 1, vec![pk_tx(&bob, vote(&p1, true), 0, "v"), ed_tx(&erin, vote(&p1, true), 0, "v"), pk_tx(&carol, vote(&p1, false), 0, "v")]),
            (H + 2, vec![pk_tx(&bob, vote(&p2, false), 1, "v"), pk_tx(&carol, vote(&p2, false), 1, "v"), ed_tx(&erin, vote(&p2, true), 1, "v"), pk_tx(&alice, vote(&p2, false), 1, "v")]),
            (H + 3, vec![ed_tx(&dave, vote(&p1, true), 1, "v")]),
            (H + ADVISORY_VOTING_PERIOD_BLOCKS, vec![]),
        ]
    };
    let p1 = proposal_id(&a, 0);
    let p2 = proposal_id(&d, 0);
    // A block at H carries both proposals (producer self-screen).
    let block = packed(H, &funded, &blocks[0].1).await;
    assert_eq!(block.iter().filter(|t| t.tx_type == TransactionType::AdvisoryGovernance).count(), 2);
    for (i, (height, txs)) in blocks.iter().enumerate() {
        let transitions = apply_block(&mut state, txs, *height);
        if i == 0 {
            let rec = &state.advisory_proposals[&p1];
            assert_eq!(rec.proposer, a);
            // Snapshot: every funded wallet, passkey and ed25519 alike.
            for who in [a, b, c, d, e] {
                assert!(rec.eligible_voters.contains(&who), "{who} eligible");
            }
            assert_eq!(rec.eligible_count, 5);
            assert_eq!(balance(&state, &a), (9 * WRF, 9 * WRF, 1), "deposit from the passkey signer, nonce advanced");
            assert_eq!(state.supply_ledger().unwrap().held().unwrap(), held_before);
        }
        if i == 1 {
            // Replay of an applied passkey vote: the nonce moved on.
            refused(process(&mut state, txs[0].clone(), H + 2), "InvalidNonce");
            // A fresh vote from the same passkey: already voted.
            refused(process(&mut state, pk_tx(&bob, vote(&p1, false), 1, "again"), H + 2), "already voted");
        }
        if i == 4 {
            assert_eq!(transitions.len(), 2, "both finalize at the deadline");
        }
    }
    // Replay of the passkey proposal after the lifecycle.
    refused(process(&mut state, pk_prop.clone(), H + ADVISORY_VOTING_PERIOD_BLOCKS + 1), "InvalidNonce");

    let r1 = &state.advisory_proposals[&p1];
    assert_eq!((r1.status, r1.yes_votes, r1.no_votes), (AdvisoryStatus::Passed, 3, 1));
    let r2 = &state.advisory_proposals[&p2];
    assert_eq!((r2.status, r2.yes_votes, r2.no_votes), (AdvisoryStatus::Rejected, 1, 3));
    assert_eq!(balance(&state, &a), (10 * WRF, 10 * WRF, 2), "passkey proposer's deposit returned");
    assert_eq!(balance(&state, &d), (10 * WRF, 10 * WRF, 2), "ed25519 proposer's deposit returned");
    assert_eq!(balance(&state, &b), (2 * WRF, 2 * WRF, 2), "voting costs no balance");
    assert_eq!(state.supply_ledger().unwrap().held().unwrap(), held_before, "supply conserved");
    assert!(state.governance.proposals.is_empty(), "never in the legacy map");

    // Replay determinism: an independent copy, same blocks, same root.
    for (height, txs) in &blocks {
        apply_block(&mut replica, txs, *height);
    }
    state.recalculate_state_root();
    replica.recalculate_state_root();
    assert_eq!(replica.advisory_proposals, state.advisory_proposals);
    assert_eq!(replica.state_root_material(), state.state_root_material());
    assert_eq!(replica.state_root, state.state_root);
}
