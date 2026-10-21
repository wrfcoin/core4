//! #3398 Option C S1: `ConsensusFeature::SlashEvidenceReports` is registered,
//! UNSCHEDULED and not extension-schedulable, and the report type stays refused
//! whether the gate is OFF or ON (apply lands in a later slice):
//!   - OFF (every height on the live chain): the refusal code is the type-only
//!     slice's, byte for byte;
//!   - ON: still refused, under `SP_SLASH_EVIDENCE_APPLY_PENDING`, so scheduling
//!     the gate early can never admit an unverified report.
//!
//! ONE test: the feature schedule is installed process-wide (first-install-wins
//! `OnceLock`), so this file owns its process, like
//! `station_owner_slash_identity_activation_3214`.

use wrfcoin_shared_protocol::crypto::chain_binding::{
    ConsensusFeature, GENESIS_ACTIVATION_SCHEDULE_KEY, SigningProtocolVersion, feature_active,
    install_activation_table, install_feature_schedule, resolve_activation_schedule,
    resolve_feature_schedule, try_feature_schedule,
};
use wrfcoin_shared_protocol::crypto::protocol_support::{
    KNOWN_FEATURES, upgrade_required_at, upgrade_required_in_schedule,
    upgrade_required_in_schedule_with,
};
use wrfcoin_shared_protocol::security::slash_evidence::dispatch_refusal;

const ACTIVATION_HEIGHT: u64 = 20;

#[tokio::test]
async fn slash_evidence_gate_is_registered_off_and_refuses_by_default_when_on() {
    let feature = ConsensusFeature::SlashEvidenceReports;
    let name = feature.name();
    assert_eq!(name, "slash_evidence_reports");
    assert_eq!(ConsensusFeature::parse(name), Some(feature));
    assert!(ConsensusFeature::ALL.contains(&feature));
    assert!(
        KNOWN_FEATURES.contains(&name),
        "KNOWN_FEATURES pairing (#3093 clean halt)"
    );

    // Not schedulable by a #3277 extension yet: apply is not implemented.
    assert!(!feature.schedulable(None));
    assert!(!feature.schedulable(Some("0000")));

    // The live chain schedules nothing: no halt, inactive at every height.
    let live_genesis: [(&str, &str); 0] = [];
    assert_eq!(upgrade_required_at(live_genesis), None);
    let raw = format!("0:v1,{ACTIVATION_HEIGHT}:{name}");
    assert_eq!(upgrade_required_in_schedule(&raw), None);
    let older: Vec<&str> = KNOWN_FEATURES
        .iter()
        .copied()
        .filter(|k| *k != name)
        .collect();
    let required = upgrade_required_in_schedule_with(&raw, &older)
        .expect("a binary without the gate must report the upgrade");
    assert_eq!(
        (required.feature.as_str(), required.height),
        (name, ACTIVATION_HEIGHT)
    );

    // ── OFF: nothing installed. The refusal is the type-only slice's code. ──
    assert!(
        try_feature_schedule().is_none(),
        "this file owns its process"
    );
    for height in [0, ACTIVATION_HEIGHT, u64::MAX] {
        assert!(!feature_active(feature, height));
        assert_eq!(dispatch_refusal(height), "SP_SLASH_EVIDENCE_NOT_SUPPORTED");
    }

    assert_call_sites_refuse(ACTIVATION_HEIGHT, "SP_SLASH_EVIDENCE_NOT_SUPPORTED").await;

    // ── ON from H (test-only schedule, as the boot gate would install). ─────
    let metadata = [(GENESIS_ACTIVATION_SCHEDULE_KEY, raw.as_str())];
    let table = resolve_activation_schedule(metadata, SigningProtocolVersion::V1).unwrap();
    let features = resolve_feature_schedule(metadata, &table).unwrap();
    SigningProtocolVersion::install_active(SigningProtocolVersion::V1);
    install_activation_table(table);
    install_feature_schedule(features);
    assert!(!feature_active(feature, ACTIVATION_HEIGHT - 1));
    assert!(feature_active(feature, ACTIVATION_HEIGHT));
    assert_eq!(
        dispatch_refusal(ACTIVATION_HEIGHT - 1),
        "SP_SLASH_EVIDENCE_NOT_SUPPORTED"
    );
    assert_eq!(
        dispatch_refusal(ACTIVATION_HEIGHT),
        "SP_SLASH_EVIDENCE_APPLY_PENDING"
    );
    assert_eq!(
        dispatch_refusal(u64::MAX),
        "SP_SLASH_EVIDENCE_APPLY_PENDING"
    );
    assert_call_sites_refuse(ACTIVATION_HEIGHT - 1, "SP_SLASH_EVIDENCE_NOT_SUPPORTED").await;
    assert_call_sites_refuse(ACTIVATION_HEIGHT, "SP_SLASH_EVIDENCE_APPLY_PENDING").await;
}

#[path = "common/slash_evidence_vectors.rs"]
mod vectors;

fn report() -> wrfcoin_shared_protocol::blockchain::transaction::Transaction {
    use wrfcoin_shared_protocol::blockchain::transaction::{Transaction, TransactionType};
    let mut tx = Transaction::new_coinbase("synthetic-report", 0, 20);
    tx.tx_type = TransactionType::SlashEvidenceReport;
    tx.inputs[0].coinbase_data = None;
    let mut payload = vectors::payload();
    payload
        .sign_for(&vectors::reporter(), &vectors::identity())
        .unwrap();
    tx.data = Some(serde_json::to_string(&payload).unwrap());
    tx.set_id();
    tx
}

async fn assert_call_sites_refuse(height: u64, code: &str) {
    use wrfcoin_shared_protocol::blockchain::transaction::Transaction;
    use wrfcoin_shared_protocol::blockchain::{
        block::Block,
        state::{State, StateTransition},
    };
    use wrfcoin_shared_protocol::consensus::proof_of_publishing::ProofOfPublishing;
    use wrfcoin_shared_protocol::crypto::hash::Hash;
    let tx = report();
    let mut state = State::new();
    let before = bincode::serialize(&state).unwrap();
    let error = state
        .apply_transition(StateTransition::ProcessGovernanceTransaction {
            tx: tx.clone(),
            current_block: height,
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains(code), "{error}");
    assert_eq!(bincode::serialize(&state).unwrap(), before);
    let mut pop = ProofOfPublishing::new(1, 60, 0, 10);
    let mut block = Block::new(
        3,
        Hash::default(),
        vec![Transaction::new_coinbase("synthetic-miner", 0, height)],
        1,
        height,
    );
    block.metadata.insert("publishing_score".into(), "0".into());
    // Positive control: all unrelated block checks pass before adding the report.
    assert!(
        pop.verify_block(&block, &Default::default(), 1, &[])
            .await
            .unwrap()
    );
    block.transactions.push(tx);
    assert!(
        !pop.verify_block(&block, &Default::default(), 1, &[])
            .await
            .unwrap(),
        "report-bearing block accepted at {height}"
    );
}
