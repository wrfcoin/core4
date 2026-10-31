//! The cross-repo golden-vector fixture for the §3.1 composed signing frame
//! (#2034 cut prep; GENESIS_CUT_PLAN.md §3.4/§7).
//!
//! # What this is
//!
//! `tests/fixtures/golden/signing_preimage_v2.json` is the **normative,
//! versioned record** of `(logical input → canonical body → V1/V2 preimage →
//! signature)` for every type in the #2226 migration set — the registry rows
//! carrying `MigrationDeferred { issue: 2226 }`. Every producer of these
//! preimages, in every language, must reproduce these bytes exactly:
//!
//! 1. **core4 Rust** — this file executes the fixture on every test run.
//! 2. **infra faucet** (`signTransferAuthorization`) — dedicated
//!    `transparent_transfer_authorization_v2` money-path case (live), then the
//!    composed Transaction case at the cut.
//! 3. **infra collector** — dedicated `commit_authorization_v2` money-path
//!    case (live; Dan preserves the v2→v1 fallback coverage separately).
//! 4. **core4 JS SDK** (`sdks/extensions/javascript`, incl. the
//!    `compute-receipt` signer) — ComputeReceipt + canonicalSigningMessage.
//!
//! A prose spec does not catch signer drift; golden vectors do. #2076 — a v1
//! signer against a v2 verifier, 28 hours of silent earn-loop halt — is what
//! this fixture exists to make impossible.
//!
//! # Why each case carries a tampered twin
//!
//! Each case pins a `tampered_input`: the same object with ONE body-folded
//! field changed. A conforming harness must recompute from the input and
//! assert the tampered preimage **differs** and the golden signature **fails**
//! over it. A harness that echoes expected values without recomputing, or an
//! implementation that ignores a field, cannot satisfy both directions — the
//! vacuous-pass mode the #2236 review keeps finding is closed by construction.
//!
//! # Regeneration (a consensus decision, not a chore)
//!
//! ```bash
//! WRFCOIN_REGEN_GOLDEN_VECTORS=1 cargo test -p wrfcoin-shared-protocol \
//!     --test signing_preimage_v2_golden
//! git diff shared-protocol/tests/fixtures/golden/  # review EVERY byte change
//! ```
//!
//! Changed bytes mean the preimage of a signed consensus object changed. That
//! is only legitimate alongside the change that moves it (e.g. #2371 adds
//! `producer_pubkey` to Block's body — its case is EXPECTED to re-derive
//! there, and the schema version below must bump with it).

mod common;
use common::{FIXTURE_PATH, REGEN_ENV, SCHEMA, check_case_by_kind, identity, keypair, load_fixture};

use serde::Serialize;
use serde_json::{Value, json};

use wrfcoin_forecast_score::ForecastAccuracyMetrics;
use wrfcoin_forecast_score::blockchain::ForecastVerificationReceipt;
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::snapshot_sync::SnapshotSyncPayload;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CalibrationAttestationPayload, ComputeReceipt, OracleAttestationPayload, Transaction,
};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::cross_chain::types::BridgeMessage;
use wrfcoin_shared_protocol::crypto::chain_binding::SigningProtocolVersion;
use wrfcoin_shared_protocol::crypto::domain_registry::{
    DomainCoverage, MIGRATION_TRACKING_ISSUE, SIGNED_OBJECT_REGISTRY,
};
use wrfcoin_shared_protocol::crypto::domain_signing::DomainSeparated;
use wrfcoin_shared_protocol::security::slashing::{SlashingEvidenceType, SlashingReportPayload};



/// Fixed instant for every timestamped field: 2025-07-20T08:26:40Z, in millis.
const FIXED_MILLIS: i64 = 1_753_000_000_000;

fn fixed_dt() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp_millis(FIXED_MILLIS).expect("fixed timestamp is valid")
}


// ---------------------------------------------------------------------------
// Case construction — one golden + one tampered instance per #2226 type.
// The tampered twin changes exactly ONE body-folded field.
// ---------------------------------------------------------------------------

fn golden_transaction() -> (Transaction, Transaction, &'static str) {
    let mut tx = Transaction::new(Vec::new(), Vec::new());
    // `Transaction::new` derives `id` from the pre-overwrite `Utc::now()`
    // state. `id` is excluded from the canonical body, so its only effect
    // would be nondeterminism in the fixture's `input` record — null it.
    tx.id = None;
    tx.timestamp = fixed_dt();
    tx.sender_address = Address([0x11; 20]);
    tx.recipient_address = Some(Address([0x22; 20]));
    tx.amount = 1_000_000;
    tx.fee = 1_000;
    tx.nonce = 7;
    tx.data = Some("golden-vector transfer".to_string());
    let mut tampered = tx.clone();
    tampered.amount += 1;
    (tx, tampered, "amount 1000000 -> 1000001")
}

fn golden_block() -> (Block, Block, &'static str) {
    // Empty transaction list ON PURPOSE: each tx hash inside the body is
    // computed via `Transaction::calculate_hash`, which reads the
    // process-active protocol version — an empty list keeps this vector fully
    // explicit. Post-#2371, `producer_pubkey` is folded into the body, so this
    // vector sets it to a distinctive value and the tampered twin flips it —
    // pinning the exact byte position of the producer-key binding the cut adds.
    let mut block = Block::new(3, Default::default(), Vec::new(), 5, 42);
    block.timestamp = fixed_dt();
    block.nonce = 9_001;
    block.producer_pubkey = [0x77; 32];
    block
        .metadata
        .insert("publishing_score".to_string(), "0.75".to_string());
    let mut tampered = block.clone();
    tampered.producer_pubkey = [0x78; 32];
    (block, tampered, "producer_pubkey 0x77.. -> 0x78..")
}

fn golden_slashing() -> (SlashingReportPayload, SlashingReportPayload, &'static str) {
    let payload = SlashingReportPayload {
        validator_id: "validator-gv-1".to_string(),
        evidence_type: SlashingEvidenceType::DoubleSign,
        evidence_hash: [0x33; 32],
        reported_at_height: 4_242,
        penalty_amount: 5_000_000,
        reporter_pubkey: "aa".repeat(32),
        reporter_signature: None,
    };
    let mut tampered = payload.clone();
    tampered.penalty_amount += 1;
    (payload, tampered, "penalty_amount 5000000 -> 5000001")
}

fn golden_node_service_receipt() -> (
    wrfcoin_shared_protocol::consensus::node_service_receipt::NodeServiceReceipt,
    wrfcoin_shared_protocol::consensus::node_service_receipt::NodeServiceReceipt,
    &'static str,
) {
    use wrfcoin_shared_protocol::consensus::node_service_receipt::{NodeService, NodeServiceReceipt};
    let (_, pk) = keypair(900_140);
    let receipt = NodeServiceReceipt {
        service: NodeService::SnapshotJoin,
        server_id: "snapshot-golden.example".into(),
        consumer_pubkey: pk.as_bytes().try_into().unwrap(),
        chain_height: 77,
        subject_hash: [0x44; 32],
        units: 1_234,
        timestamp: FIXED_MILLIS / 1_000,
        nonce: [0x55; 32],
    };
    let mut tampered = receipt.clone();
    tampered.units += 1;
    (receipt, tampered, "units 1234 -> 1235")
}

fn golden_snapshot() -> (SnapshotSyncPayload, SnapshotSyncPayload, &'static str) {
    let payload = SnapshotSyncPayload {
        height: 77,
        state_root_hex: "44".repeat(32),
        snapshot_root_hex: "55".repeat(32),
        entry_count: 1_234,
        tip_hash_hex: "66".repeat(32),
        finalized_height: 70,
    };
    let mut tampered = payload.clone();
    tampered.state_root_hex = "45".repeat(32);
    (payload, tampered, "state_root_hex 4444… -> 4545…")
}

fn golden_oracle_attestation()
-> (OracleAttestationPayload, OracleAttestationPayload, &'static str) {
    let payload = OracleAttestationPayload {
        commit_id: "commit-gv-1".to_string(),
        oracle_id: "oracle-gv-1".to_string(),
        approved: true,
        signature: None,
        confidence: Some(0.875),
        timestamp: FIXED_MILLIS,
        nonce: Some("6f".repeat(16)),
    };
    let mut tampered = payload.clone();
    tampered.approved = false;
    (payload, tampered, "approved true -> false")
}

fn golden_calibration()
-> (CalibrationAttestationPayload, CalibrationAttestationPayload, &'static str) {
    let payload = CalibrationAttestationPayload {
        station_id: "station-gv-1".to_string(),
        calibrator_id: "calibrator-gv-1".to_string(),
        issued_at: FIXED_MILLIS,
        expires_at: FIXED_MILLIS + 86_400_000,
        calibrator_key_epoch: Some(2),
        signature: None,
        nonce: Some("7a".repeat(16)),
    };
    let mut tampered = payload.clone();
    tampered.expires_at += 1;
    (payload, tampered, "expires_at +1ms")
}

fn golden_bridge_message() -> (BridgeMessage, BridgeMessage, &'static str) {
    use wrfcoin_shared_protocol::cross_chain::types::{
        BridgeMessageType, CrossChainDirection, CrossChainNetwork,
    };
    let msg = BridgeMessage {
        message_id: "bridge-gv-1".to_string(),
        source_network: CrossChainNetwork::Ethereum,
        target_network: CrossChainNetwork::Polygon,
        direction: CrossChainDirection::Inbound,
        message_type: BridgeMessageType::Lock,
        sender: "0x".to_string() + &"66".repeat(20),
        recipient: "0x".to_string() + &"77".repeat(20),
        amount: 250_000,
        nonce: 11,
        timestamp: FIXED_MILLIS,
        tx_hash: None,
        payload: None,
    };
    let mut tampered = msg.clone();
    tampered.recipient = "0x".to_string() + &"78".repeat(20);
    (msg, tampered, "recipient 0x7777… -> 0x7878…")
}

fn golden_compute_receipt() -> (ComputeReceipt, ComputeReceipt, &'static str) {
    let receipt = ComputeReceipt {
        task_id: "task-gv-1".to_string(),
        worker_id: "worker-gv-1".to_string(),
        work_hash: "88".repeat(32),
        work_factor: 640,
        completed_at: FIXED_MILLIS,
        nonce: "9b".repeat(16),
        signature: None,
    };
    let mut tampered = receipt.clone();
    tampered.work_factor += 1;
    (receipt, tampered, "work_factor 640 -> 641")
}

fn golden_forecast_receipt()
-> (ForecastVerificationReceipt, ForecastVerificationReceipt, &'static str) {
    let receipt = ForecastVerificationReceipt {
        manifest_digest: "aa".repeat(32),
        commit_id: "commit-gv-2".to_string(),
        oracle_id: "oracle-gv-2".to_string(),
        metrics: ForecastAccuracyMetrics {
            mae: 0.5,
            rmse: 0.75,
            bias: -0.125,
            skill_score: 0.875,
        },
        verified_at: FIXED_MILLIS,
        nonce: "bc".repeat(16),
        zk_proof: None,
        signature: None,
    };
    let mut tampered = receipt.clone();
    tampered.metrics.skill_score = 0.5;
    (receipt, tampered, "metrics.skill_score 0.875 -> 0.5")
}

// ---------------------------------------------------------------------------
// Generic build / check over any DomainSeparated + Serialize + Deserialize
// ---------------------------------------------------------------------------

fn build_case<T: DomainSeparated + Serialize>(
    kind: &str,
    seed: u64,
    golden: &T,
    tampered: &T,
    tamper_note: &str,
) -> Value {
    let identity = identity();
    let (sk, pk) = keypair(seed);
    let body = golden.canonical_body();
    let preimage_v1 = golden.signing_preimage_for(&identity, SigningProtocolVersion::V1);
    let preimage_v2 = golden.signing_preimage_for(&identity, SigningProtocolVersion::V2);
    let signature_v2 = sk.sign(&preimage_v2);
    json!({
        "kind": kind,
        "frame": "composed_v2",
        "tag": String::from_utf8_lossy(T::DOMAIN_TAG),
        "signer_seed": seed,
        // Raw 32-byte ed25519 seed, hex — the PORTABLE key material. Node/JS
        // consumers cannot reproduce Rust's StdRng(u64) derivation; they sign
        // with this seed directly and must reproduce signature_v2_hex (test
        // keys only — nothing here ever holds value).
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": serde_json::to_value(golden).expect("golden serializes"),
        "tampered_input": serde_json::to_value(tampered).expect("tampered serializes"),
        "tamper_note": tamper_note,
        "canonical_body_hex": hex::encode(&body),
        "preimage_v1_hex": hex::encode(&preimage_v1),
        "preimage_v2_hex": hex::encode(&preimage_v2),
        "signature_v2_hex": signature_v2.to_hex(),
    })
}


// ---------------------------------------------------------------------------
// Pre-cut money-path cases (`frame: "money_path_v2"`) — infra coordination.
//
// The live collector and faucet do NOT sign the composed frame today: the
// collector signs `commit_authorization_message_with_domain`
// (`wrfcoin_commit_auth_v2`, u32-LE lengths) and the faucet signs
// `transparent_transfer_authorization_message_with_domain`
// (`wrfcoin_transparent_transfer_auth_v2`). Until the #2226 collapse retires
// these constructions at the cut, THEIR bytes are the ones cross-repo signers
// can drift on — so they are pinned in the SAME fixture, outside the registry
// reconciliation (they are message constructions, not DomainSeparated types).
// ---------------------------------------------------------------------------

fn build_commit_authorization_case() -> Value {
    use wrfcoin_shared_protocol::blockchain::transaction::commit_authorization_message_with_domain;
    let identity = identity();
    let (sk, pk) = keypair(900_101);
    let (station_id, commit_id, contribution_type) =
        ("station-gv-1", "commit-gv-auth-1", "weather");
    let preimage = commit_authorization_message_with_domain(
        &identity.chain_id,
        &identity.network_id,
        station_id,
        commit_id,
        contribution_type,
    );
    let signature = sk.sign(&preimage);
    json!({
        "kind": "commit_authorization_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin_commit_auth_v2",
        "signer_seed": 900_101,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": {
            "chain_id": identity.chain_id,
            "network_id": identity.network_id,
            "station_id": station_id,
            "commit_id": commit_id,
            "contribution_type": contribution_type,
        },
        "tampered_input": {
            "chain_id": identity.chain_id,
            "network_id": identity.network_id,
            "station_id": station_id,
            "commit_id": "commit-gv-auth-2",
            "contribution_type": contribution_type,
        },
        "tamper_note": "commit_id commit-gv-auth-1 -> commit-gv-auth-2",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_transparent_transfer_authorization_case() -> Value {
    use wrfcoin_shared_protocol::blockchain::transaction::transparent_transfer_authorization_message_with_domain;
    let identity = identity();
    let (sk, pk) = keypair(900_102);
    let (from, to, amount, nonce) = ("wrf_faucet_gv", "wrf_recipient_gv", 1_000u64, 5u64);
    let preimage = transparent_transfer_authorization_message_with_domain(
        &identity.chain_id,
        &identity.network_id,
        from,
        to,
        amount,
        nonce,
    );
    let signature = sk.sign(&preimage);
    json!({
        "kind": "transparent_transfer_authorization_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin_transparent_transfer_auth_v2",
        "signer_seed": 900_102,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": {
            "chain_id": identity.chain_id,
            "network_id": identity.network_id,
            "from": from,
            "to": to,
            "amount": amount,
            "nonce": nonce,
        },
        "tampered_input": {
            "chain_id": identity.chain_id,
            "network_id": identity.network_id,
            "from": from,
            "to": to,
            "amount": amount + 1,
            "nonce": nonce,
        },
        "tamper_note": "amount 1000 -> 1001",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_system_transfer_authorization_case() -> Value {
    use wrfcoin_shared_protocol::blockchain::transaction::system_transfer_authorization_message_with_domain;
    let identity = identity();
    let (sk, pk) = keypair(900_108);
    let (from, to, amount, nonce) =
        ("wrf_system_source_gv", "wrf_system_recipient_gv", 2_000u64, 9u64);
    let preimage = system_transfer_authorization_message_with_domain(
        &identity.chain_id,
        &identity.network_id,
        from,
        to,
        amount,
        nonce,
    );
    let signature = sk.sign(&preimage);
    json!({
        "kind": "system_transfer_authorization_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin_system_transfer_auth_v2",
        "signer_seed": 900_108,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": {
            "chain_id": identity.chain_id, "network_id": identity.network_id,
            "from": from, "to": to, "amount": amount, "nonce": nonce,
        },
        "tampered_input": {
            "chain_id": identity.chain_id, "network_id": identity.network_id,
            "from": from, "to": to, "amount": amount, "nonce": nonce + 1,
        },
        "tamper_note": "nonce 9 -> 10",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_stake_authorization_case() -> Value {
    use wrfcoin_shared_protocol::blockchain::transaction::stake_authorization_message_with_domain;
    let identity = identity();
    let (sk, pk) = keypair(900_109);
    let (account, amount, is_unstake, nonce) = ("wrf_staker_gv", 3_000u64, false, 11u64);
    let preimage = stake_authorization_message_with_domain(
        &identity.chain_id,
        &identity.network_id,
        account,
        amount,
        is_unstake,
        nonce,
    );
    let signature = sk.sign(&preimage);
    json!({
        "kind": "stake_authorization_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin_stake_auth_v2",
        "signer_seed": 900_109,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": {
            "chain_id": identity.chain_id, "network_id": identity.network_id,
            "account": account, "amount": amount, "is_unstake": is_unstake, "nonce": nonce,
        },
        "tampered_input": {
            "chain_id": identity.chain_id, "network_id": identity.network_id,
            "account": account, "amount": amount, "is_unstake": true, "nonce": nonce,
        },
        "tamper_note": "stake -> unstake",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

// #2370 (Dan's review): GamificationClaimPayload::sign and
// OracleRegistrationPayload::sign each sign their OWN domain-tagged
// authorization message — `wrfcoin_gamification_auth_v1` /
// `wrfcoin_oracle_registration_auth_v1` — verified on live transition and
// block-verification paths in proof_of_publishing.rs. The outer registered
// Transaction preimage is NOT those bytes, so these are first-class signed
// surfaces, pinned here with their real tags (not hidden behind Transaction).

fn build_gamification_authorization_case() -> Value {
    use wrfcoin_shared_protocol::blockchain::transaction::gamification_authorization_message_with_domain;
    let identity = identity();
    let (sk, pk) = keypair(900_103);
    let (claim_id, recipient, amount) = ("claim-gv-1", "wrf_reward_recipient_gv", 200u64);
    let preimage = gamification_authorization_message_with_domain(
        &identity.chain_id,
        &identity.network_id,
        claim_id,
        recipient,
        amount,
    );
    let signature = sk.sign(&preimage);
    json!({
        "kind": "gamification_authorization_v1",
        "frame": "money_path_v2",
        "tag": "wrfcoin_gamification_auth_v1",
        "signer_seed": 900_103,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": {
            "chain_id": identity.chain_id, "network_id": identity.network_id,
            "claim_id": claim_id, "recipient": recipient, "amount": amount,
        },
        "tampered_input": {
            "chain_id": identity.chain_id, "network_id": identity.network_id,
            "claim_id": claim_id, "recipient": recipient, "amount": amount + 1,
        },
        "tamper_note": "amount 200 -> 201",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

/// #2537 — the station-registration governance authorization.
///
/// Worth pinning for a reason specific to this surface: the preimage carries
/// `provenance_class`, the single byte that decides whether a station's
/// contributions can ever earn, and `owner_pubkey`, which binds the possession
/// proof that governance approved. The tampered twin flips Synthetic -> Real,
/// i.e. exactly the privilege escalation the signature exists to prevent.
fn build_station_registration_authorization_case() -> Value {
    use wrfcoin_shared_protocol::blockchain::transaction::{
        StationRegistrationPayload, station_registration_authorization_message_with_domain,
    };
    use wrfcoin_shared_protocol::station_registry::{
        ContributionProvenanceClass, DataQualityTier,
    };
    let identity = identity();
    let (sk, pk) = keypair(900_105);
    let (station_id, owner_address) = ("station-golden-1", "0xowner-golden-1");
    let (latitude, longitude) = (46.7867f64, -92.1005f64);
    let hardware_model = "golden-vector-station";
    let owner_pubkey = "dd".repeat(32);

    let mut payload = StationRegistrationPayload::new(
        station_id,
        owner_address,
        latitude,
        longitude,
        hardware_model,
        DataQualityTier::Gold,
        ContributionProvenanceClass::Synthetic,
    );
    payload.is_active = true;
    payload.owner_pubkey = Some(owner_pubkey.clone());

    let preimage = station_registration_authorization_message_with_domain(
        &identity.chain_id,
        &identity.network_id,
        &payload,
    );
    let signature = sk.sign(&preimage);
    let input = json!({
        "chain_id": identity.chain_id, "network_id": identity.network_id,
        "station_id": station_id, "owner_address": owner_address,
        "latitude": latitude, "longitude": longitude,
        "hardware_model": hardware_model, "is_active": true,
        "data_quality_tier": "gold", "provenance_class": "synthetic",
        "owner_pubkey": owner_pubkey,
    });
    let mut tampered = input.clone();
    tampered["provenance_class"] = json!("real");
    json!({
        "kind": "station_registration_authorization_v1",
        "frame": "money_path_v2",
        "tag": "wrfcoin_station_registration_auth_v1",
        "signer_seed": 900_105,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": input,
        "tampered_input": tampered,
        "tamper_note": "provenance_class synthetic -> real (the reward-authorising field)",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

/// The v2 (nonce-bound) governance registration authorization: same fields as
/// v1 under its own tag, plus the signed `expected_nonce`. Pinned because the
/// nonce is what makes a mined registration non-reusable.
fn build_station_registration_authorization_v2_case() -> Value {
    use wrfcoin_shared_protocol::blockchain::transaction::{
        StationRegistrationPayload, station_registration_authorization_message_with_domain,
    };
    use wrfcoin_shared_protocol::station_registry::{
        ContributionProvenanceClass, DataQualityTier,
    };
    let identity = identity();
    let (sk, pk) = keypair(900_139);
    let (station_id, owner_address) = ("station-golden-1", "0xowner-golden-1");
    let (latitude, longitude) = (46.7867f64, -92.1005f64);
    let hardware_model = "golden-vector-station";
    let owner_pubkey = "dd".repeat(32);
    let mut payload = StationRegistrationPayload::new(
        station_id,
        owner_address,
        latitude,
        longitude,
        hardware_model,
        DataQualityTier::Gold,
        ContributionProvenanceClass::Synthetic,
    )
    .with_expected_nonce(7);
    payload.is_active = true;
    payload.owner_pubkey = Some(owner_pubkey.clone());
    let preimage = station_registration_authorization_message_with_domain(
        &identity.chain_id,
        &identity.network_id,
        &payload,
    );
    let signature = sk.sign(&preimage);
    let input = json!({
        "chain_id": identity.chain_id, "network_id": identity.network_id,
        "station_id": station_id, "owner_address": owner_address,
        "latitude": latitude, "longitude": longitude,
        "hardware_model": hardware_model, "is_active": true,
        "data_quality_tier": "gold", "provenance_class": "synthetic",
        "owner_pubkey": owner_pubkey, "expected_nonce": 7u64,
    });
    let mut tampered = input.clone();
    tampered["expected_nonce"] = json!(8u64);
    json!({
        "kind": "station_registration_authorization_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin_station_registration_auth_v2",
        "signer_seed": 900_139,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": input,
        "tampered_input": tampered,
        "tamper_note": "expected_nonce 7 -> 8 (the per-station slot the registration is signed for)",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_oracle_registration_authorization_case() -> Value {
    use wrfcoin_shared_protocol::blockchain::transaction::{
        OracleRegistrationPayload, oracle_registration_authorization_message_with_domain,
    };
    let identity = identity();
    let (sk, pk) = keypair(900_104);
    let oracle_set = vec!["oracle-a".to_string(), "oracle-b".to_string()];
    let (quorum, id, pubkey, epoch, activated_at) =
        (2usize, "oracle-a", "cc".repeat(32), 3u64, FIXED_MILLIS);
    let payload = OracleRegistrationPayload::new(
        oracle_set.clone(),
        quorum,
        id,
        pubkey.clone(),
        epoch,
        activated_at,
    );
    let preimage = oracle_registration_authorization_message_with_domain(
        &identity.chain_id,
        &identity.network_id,
        &payload,
    );
    let signature = sk.sign(&preimage);
    let input = json!({
        "chain_id": identity.chain_id, "network_id": identity.network_id,
        "oracle_set": oracle_set, "oracle_quorum": quorum, "identity": id,
        "public_key": pubkey, "epoch": epoch, "activated_at": activated_at,
    });
    let mut tampered = input.clone();
    tampered["oracle_quorum"] = json!(quorum + 1);
    json!({
        "kind": "oracle_registration_authorization_v1",
        "frame": "money_path_v2",
        "tag": "wrfcoin_oracle_registration_auth_v1",
        "signer_seed": 900_104,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": input,
        "tampered_input": tampered,
        "tamper_note": "oracle_quorum 2 -> 3",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_reward_claim_authorization_case() -> Value {
    use wrfcoin_shared_protocol::blockchain::transaction::{
        RewardClaimPayload, reward_claim_authorization_message_with_domain,
    };
    let identity = identity();
    let (sk, pk) = keypair(900_105);
    let payload = RewardClaimPayload {
        epoch: 7,
        ring_signature: String::new(),
        key_image: "11".repeat(32),
        amount_commitment: "22".repeat(32),
        range_proof: "33".repeat(32),
        destination: "wrf_reward_destination_a".to_string(),
    };
    let preimage = reward_claim_authorization_message_with_domain(
        &identity.chain_id,
        &identity.network_id,
        &payload,
    );
    let signature = sk.sign(&preimage);
    let input = json!({
        "chain_id": identity.chain_id,
        "network_id": identity.network_id,
        "epoch": payload.epoch,
        "key_image": payload.key_image,
        "amount_commitment": payload.amount_commitment,
        "range_proof": payload.range_proof,
        "destination": payload.destination,
    });
    let mut tampered = input.clone();
    tampered["destination"] = json!("wrf_reward_destination_b");
    json!({
        "kind": "reward_claim_authorization_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin_reward_claim_auth_v2",
        "signer_seed": 900_105,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": input,
        "tampered_input": tampered,
        "tamper_note": "destination A -> destination B",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_validator_key_rotation_pop_case() -> Value {
    use wrfcoin_shared_protocol::crypto::rotation::rotation_pop_message_with_domain;
    let identity = identity();
    let (_, old) = keypair(900_106);
    let (new_sk, new) = keypair(900_107);
    let effective_height = 12_345u64;
    let preimage = rotation_pop_message_with_domain(
        &identity.chain_id,
        &identity.network_id,
        &old,
        &new,
        effective_height,
    );
    let signature = new_sk.sign(&preimage);
    let input = json!({
        "chain_id": identity.chain_id,
        "network_id": identity.network_id,
        "old_public_key": old.to_hex(),
        "new_public_key": new.to_hex(),
        "effective_height": effective_height,
    });
    let mut tampered = input.clone();
    tampered["effective_height"] = json!(effective_height + 1);
    json!({
        "kind": "validator_key_rotation_pop_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin.validator_key_rotation_pop.v2",
        "signer_seed": 900_107,
        "signer_secret_hex": new_sk.to_hex(),
        "signer_public_key_hex": new.to_hex(),
        "input": input,
        "tampered_input": tampered,
        "tamper_note": "effective height 12345 -> 12346",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_selective_disclosure_authorization_case() -> Value {
    use wrfcoin_shared_protocol::blockchain::transaction::{
        SelectiveDisclosureProofPayload,
        selective_disclosure_authorization_digest_with_domain,
    };
    let identity = identity();
    let (sk, pk) = keypair(900_108);
    let payload = SelectiveDisclosureProofPayload {
        disclosure_id: "disclosure-900108".to_string(),
        scope: vec!["amount.bucket".to_string(), "jurisdiction".to_string()],
        expires_at_block: 12_345,
        disclosed_fields_hash: "44".repeat(32),
        verifier_attestation: String::new(),
    };
    let preimage = selective_disclosure_authorization_digest_with_domain(
        &identity.chain_id,
        &identity.network_id,
        &payload,
    );
    let signature = sk.sign(preimage.as_bytes());
    let input = json!({
        "chain_id": identity.chain_id,
        "network_id": identity.network_id,
        "disclosure_id": payload.disclosure_id,
        "scope": payload.scope,
        "expires_at_block": payload.expires_at_block,
        "disclosed_fields_hash": payload.disclosed_fields_hash,
    });
    let mut tampered = input.clone();
    tampered["expires_at_block"] = json!(12_346);
    json!({
        "kind": "selective_disclosure_authorization_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin_selective_disclosure_auth_v2",
        "signer_seed": 900_108,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": input,
        "tampered_input": tampered,
        "tamper_note": "expires_at_block 12345 -> 12346",
        "preimage_hex": hex::encode(preimage.as_bytes()),
        "signature_hex": signature.to_hex(),
    })
}

fn build_confidential_transfer_authorization_case() -> Value {
    use wrfcoin_shared_protocol::blockchain::transaction::{
        ConfidentialTransferPayload, PrivacyLevel, SelectiveDisclosureProofPayload,
        confidential_transfer_authorization_message_with_domain,
    };
    let identity = identity();
    let (sk, pk) = keypair(900_110);
    let payload = ConfidentialTransferPayload {
        output_commitments: vec!["51".repeat(32), "52".repeat(32)],
        range_proof: Some("53".repeat(32)),
        stealth_data: Some("54".repeat(32)),
        ring_signature: None,
        key_images: vec!["55".repeat(32)],
        encrypted_payload: Some("56".repeat(32)),
        privacy_level: PrivacyLevel::High,
        selective_disclosures: vec![SelectiveDisclosureProofPayload {
            disclosure_id: "disclosure-nested-900110".to_string(),
            scope: vec!["amount.bucket".to_string()],
            expires_at_block: 12_345,
            disclosed_fields_hash: "57".repeat(32),
            verifier_attestation: "58".repeat(64),
        }],
        signed_message: None,
    };
    let preimage = confidential_transfer_authorization_message_with_domain(
        &identity.chain_id,
        &identity.network_id,
        &payload,
    );
    let signature = sk.sign(&preimage);
    let mut input = serde_json::to_value(&payload).expect("serialize payload");
    input["chain_id"] = json!(identity.chain_id);
    input["network_id"] = json!(identity.network_id);
    let mut tampered = input.clone();
    tampered["output_commitments"][0] = json!("59".repeat(32));
    json!({
        "kind": "confidential_transfer_authorization_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin_confidential_transfer_auth_v2",
        "signer_seed": 900_110,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": input,
        "tampered_input": tampered,
        "tamper_note": "output_commitments[0] 0x51 -> 0x59",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_account_recovery_guardian_approval_case() -> Value {
    use wrfcoin_shared_protocol::accounts::recovery::guardian_approval_message;

    let (sk, pk) = keypair(900_113);
    let account = Address([0x71; 20]);
    let new_owner = "72".repeat(32);
    let initiated_at_height = 2172u64;
    let preimage = guardian_approval_message(&account, &new_owner, initiated_at_height);
    let signature = sk.sign(&preimage);
    json!({
        "kind": "account_recovery_guardian_approval_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin.account_recovery_guardian_approval.v1",
        "signer_seed": 900_113,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": {
            "account": account.to_hex(),
            "new_owner": new_owner,
            "initiated_at_height": initiated_at_height,
        },
        "tampered_input": {
            "account": account.to_hex(),
            "new_owner": "73".repeat(32),
            "initiated_at_height": initiated_at_height,
        },
        "tamper_note": "new_owner 0x72.. -> 0x73..",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_account_recovery_owner_set_policy_case() -> Value {
    use wrfcoin_shared_protocol::accounts::recovery::{
        RecoveryPolicy, owner_set_policy_message,
    };

    let (sk, pk) = keypair(900_114);
    let account = Address::from_public_key(&pk);
    let policy = RecoveryPolicy {
        guardians: vec![Address([0x74; 20]), Address([0x75; 20])],
        threshold: 2,
        time_lock_blocks: 144,
        cooldown_blocks: 288,
    };
    let policy_nonce = 9u64;
    let preimage = owner_set_policy_message(&account, &policy, policy_nonce);
    let signature = sk.sign(&preimage);
    let input = json!({
        "account": account.to_hex(),
        "guardians": policy.guardians.iter().map(Address::to_hex).collect::<Vec<_>>(),
        "threshold": policy.threshold,
        "time_lock_blocks": policy.time_lock_blocks,
        "cooldown_blocks": policy.cooldown_blocks,
        "policy_nonce": policy_nonce,
    });
    let mut tampered = input.clone();
    tampered["cooldown_blocks"] = json!(289u64);
    json!({
        "kind": "account_recovery_owner_set_policy_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin.account_recovery_owner_set_policy.v1",
        "signer_seed": 900_114,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": input,
        "tampered_input": tampered,
        "tamper_note": "cooldown_blocks 288 -> 289",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_account_recovery_owner_cancel_case() -> Value {
    use wrfcoin_shared_protocol::accounts::recovery::owner_cancel_message;

    let (sk, pk) = keypair(900_115);
    let account = Address::from_public_key(&pk);
    let initiated_at_height = 2_172u64;
    let preimage = owner_cancel_message(&account, initiated_at_height);
    let signature = sk.sign(&preimage);
    json!({
        "kind": "account_recovery_owner_cancel_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin.account_recovery_owner_cancel.v1",
        "signer_seed": 900_115,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": {
            "account": account.to_hex(),
            "initiated_at_height": initiated_at_height,
        },
        "tampered_input": {
            "account": account.to_hex(),
            "initiated_at_height": initiated_at_height + 1,
        },
        "tamper_note": "initiated_at_height 2172 -> 2173",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_account_recovery_owner_cancel_policy_change_case() -> Value {
    use wrfcoin_shared_protocol::accounts::recovery::owner_cancel_policy_change_message;

    let (sk, pk) = keypair(900_116);
    let account = Address::from_public_key(&pk);
    let initiated_at_height = 2_173u64;
    let preimage = owner_cancel_policy_change_message(&account, initiated_at_height);
    let signature = sk.sign(&preimage);
    json!({
        "kind": "account_recovery_owner_cancel_policy_change_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin.account_recovery_owner_cancel_policy_change.v1",
        "signer_seed": 900_116,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": {
            "account": account.to_hex(),
            "initiated_at_height": initiated_at_height,
        },
        "tampered_input": {
            "account": account.to_hex(),
            "initiated_at_height": initiated_at_height + 1,
        },
        "tamper_note": "initiated_at_height 2173 -> 2174",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_session_key_register_case() -> Value {
    use wrfcoin_shared_protocol::accounts::session_keys::{
        MethodId, SessionKeyGrant, SessionScope, session_key_register_message,
    };

    let (sk, pk) = keypair(900_117);
    let (_, session_pk) = keypair(901_117);
    let owner = Address::from_public_key(&pk);
    let grant = SessionKeyGrant {
        public_key: session_pk,
        scope: SessionScope {
            allowed_methods: vec![MethodId("submit_weather_observation".to_string())],
            allowed_targets: vec![Address([0x82; 20])],
            max_value_per_use: 25,
            max_total_value: 250,
        },
        valid_from: 2_170,
        valid_until: 2_270,
        max_uses: Some(10),
    };
    let registration_nonce = 7u64;
    let preimage =
        session_key_register_message(&identity().chain_id, &owner, registration_nonce, &grant);
    let signature = sk.sign(&preimage);
    let input = json!({
        "chain_id": identity().chain_id,
        "owner": owner.to_hex(),
        "registration_nonce": registration_nonce,
        "session_public_key": grant.public_key.to_hex(),
        "allowed_methods": ["submit_weather_observation"],
        "allowed_targets": [Address([0x82; 20]).to_hex()],
        "max_value_per_use": "25",
        "max_total_value": "250",
        "valid_from": 2170,
        "valid_until": 2270,
        "max_uses": 10,
    });
    let mut tampered = input.clone();
    tampered["registration_nonce"] = json!(registration_nonce + 1);
    json!({
        "kind": "session_key_register_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin.session_key_register.v1",
        "signer_seed": 900_117,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": input,
        "tampered_input": tampered,
        "tamper_note": "registration_nonce 7 -> 8",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_session_key_revoke_case() -> Value {
    use wrfcoin_shared_protocol::accounts::session_keys::session_key_revoke_message;

    let (sk, pk) = keypair(900_118);
    let owner = Address::from_public_key(&pk);
    let session_key = Address([0x84; 20]);
    let registration_nonce = 8u64;
    let preimage = session_key_revoke_message(
        &identity().chain_id,
        &owner,
        &session_key,
        registration_nonce,
    );
    let signature = sk.sign(&preimage);
    json!({
        "kind": "session_key_revoke_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin.session_key_revoke.v1",
        "signer_seed": 900_118,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": {
            "chain_id": identity().chain_id,
            "owner": owner.to_hex(),
            "session_key": session_key.to_hex(),
            "registration_nonce": registration_nonce,
        },
        "tampered_input": {
            "chain_id": identity().chain_id,
            "owner": owner.to_hex(),
            "session_key": session_key.to_hex(),
            "registration_nonce": registration_nonce + 1,
        },
        "tamper_note": "registration_nonce 8 -> 9",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_session_key_use_case() -> Value {
    use wrfcoin_shared_protocol::accounts::session_keys::{MethodId, session_key_use_message};

    let (sk, pk) = keypair(900_119);
    let owner = Address([0x86; 20]);
    let session_key = Address::from_public_key(&pk);
    let method = MethodId("submit_weather_observation".to_string());
    let target = Address([0x87; 20]);
    let preimage = session_key_use_message(
        &identity().chain_id,
        &owner,
        &session_key,
        &method,
        &target,
        40,
        9,
        3,
    );
    let signature = sk.sign(&preimage);
    json!({
        "kind": "session_key_use_v2",
        "frame": "money_path_v2",
        "tag": "wrfcoin.session_key_use.v1",
        "signer_seed": 900_119,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": {
            "chain_id": identity().chain_id,
            "owner": owner.to_hex(),
            "session_key": session_key.to_hex(),
            "method": method.0,
            "target": target.to_hex(),
            "value": "40",
            "registration_nonce": 9,
            "use_index": 3,
        },
        "tampered_input": {
            "chain_id": identity().chain_id,
            "owner": owner.to_hex(),
            "session_key": session_key.to_hex(),
            "method": "submit_weather_observation",
            "target": target.to_hex(),
            "value": "40",
            "registration_nonce": 10,
            "use_index": 3,
        },
        "tamper_note": "registration_nonce 9 -> 10",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_declare_authorization_case(
    authorization: Option<wrfcoin_shared_protocol::accounts::authorization::AccountAuthorization>,
    authorization_name: &str,
    kind: &str,
    seed: u64,
) -> Value {
    use wrfcoin_shared_protocol::accounts::authorization::declare_authorization_message;

    let (sk, pk) = keypair(seed);
    let account = Address::from_public_key(&pk);
    let nonce = 11u64;
    let generation = 4u64;
    let preimage = declare_authorization_message(
        &identity().chain_id,
        &account,
        &authorization,
        nonce,
        generation,
    );
    let signature = sk.sign(&preimage);
    json!({
        "kind": kind,
        "frame": "money_path_v2",
        "tag": "wrfcoin.declare_authorization.v1",
        "signer_seed": seed,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": { "chain_id": identity().chain_id, "account": account.to_hex(), "authorization": authorization_name, "nonce": nonce, "generation": generation },
        "tampered_input": { "chain_id": identity().chain_id, "account": account.to_hex(), "authorization": authorization_name, "nonce": nonce + 1, "generation": generation },
        "tamper_note": "nonce 11 -> 12",
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

// ---------------------------------------------------------------------------
// #3345 / #3347 station owner ledger (owner rulings 2026-09-27). One case per
// domain tag. Each tampered twin flips a field that decides WHOSE station or
// WHICH replay slot the signature authorizes.
// ---------------------------------------------------------------------------

fn station_owner_case(
    kind: &str,
    tag: &str,
    seed: u64,
    preimage_of: impl Fn(&Value) -> Vec<u8>,
    input: Value,
    tampered: Value,
    tamper_note: &str,
) -> Value {
    let (sk, pk) = keypair(seed);
    let preimage = preimage_of(&input);
    let signature = sk.sign(&preimage);
    json!({
        "kind": kind,
        "frame": "money_path_v2",
        "tag": tag,
        "signer_seed": seed,
        "signer_secret_hex": sk.to_hex(),
        "signer_public_key_hex": pk.to_hex(),
        "input": input,
        "tampered_input": tampered,
        "tamper_note": tamper_note,
        "preimage_hex": hex::encode(&preimage),
        "signature_hex": signature.to_hex(),
    })
}

fn build_station_owner_transfer_cases() -> Vec<Value> {
    use wrfcoin_shared_protocol::blockchain::transaction::{
        STATION_OWNER_TRANSFER_ACCEPT_DOMAIN, STATION_OWNER_TRANSFER_DOMAIN,
        station_owner_transfer_message_with_domain,
    };
    let identity = identity();
    let input = json!({
        "chain_id": identity.chain_id, "network_id": identity.network_id,
        "station_id": "station-golden-3345",
        "from_owner_address": "wrf".to_string() + &"a1".repeat(20),
        "to_owner_address": "wrf".to_string() + &"b2".repeat(20),
        "to_owner_pubkey": "b3".repeat(32),
        "transfer_nonce": 7u64,
        "expires_at_height": 21_000u64,
    });
    let mut tampered = input.clone();
    tampered["transfer_nonce"] = json!(8u64);
    let build = |domain: &'static [u8]| {
        move |i: &Value| {
            let s = |k: &str| i[k].as_str().expect(k).to_string();
            station_owner_transfer_message_with_domain(
                domain,
                &s("chain_id"),
                &s("network_id"),
                &s("station_id"),
                &s("from_owner_address"),
                &s("to_owner_address"),
                &s("to_owner_pubkey"),
                i["transfer_nonce"].as_u64().unwrap(),
                i["expires_at_height"].as_u64().unwrap(),
            )
        }
    };
    vec![
        station_owner_case(
            "station_owner_transfer_v1",
            "wrfcoin_station_owner_transfer_v1",
            900_130,
            build(STATION_OWNER_TRANSFER_DOMAIN),
            input.clone(),
            tampered.clone(),
            "transfer_nonce 7 -> 8 (the per-station replay slot)",
        ),
        station_owner_case(
            "station_owner_transfer_accept_v1",
            "wrfcoin_station_owner_transfer_accept_v1",
            900_131,
            build(STATION_OWNER_TRANSFER_ACCEPT_DOMAIN),
            input,
            tampered,
            "transfer_nonce 7 -> 8 (the per-station replay slot)",
        ),
    ]
}

fn build_station_owner_recovery_filing_cases() -> Vec<Value> {
    use wrfcoin_shared_protocol::blockchain::station_owner::{
        STATION_OWNER_RECOVERY_ACCEPT_DOMAIN, STATION_OWNER_RECOVERY_FILE_DOMAIN,
        station_owner_recovery_message_with_domain,
    };
    let identity = identity();
    let input = json!({
        "chain_id": identity.chain_id, "network_id": identity.network_id,
        "station_id": "station-golden-3345",
        "from_owner_address": "wrf".to_string() + &"a1".repeat(20),
        "to_owner_address": "wrf".to_string() + &"c3".repeat(20),
        "to_owner_pubkey": "c4".repeat(32),
        "recovery_nonce": 7u64,
    });
    let mut tampered = input.clone();
    tampered["to_owner_address"] = json!("wrf".to_string() + &"d5".repeat(20));
    let build = |domain: &'static [u8]| {
        move |i: &Value| {
            let s = |k: &str| i[k].as_str().expect(k).to_string();
            station_owner_recovery_message_with_domain(
                domain,
                &s("chain_id"),
                &s("network_id"),
                &s("station_id"),
                &s("from_owner_address"),
                &s("to_owner_address"),
                &s("to_owner_pubkey"),
                i["recovery_nonce"].as_u64().unwrap(),
            )
        }
    };
    vec![
        station_owner_case(
            "station_owner_recovery_file_v1",
            "wrfcoin_station_owner_recovery_file_v1",
            900_132,
            build(STATION_OWNER_RECOVERY_FILE_DOMAIN),
            input.clone(),
            tampered.clone(),
            "to_owner_address c3… -> d5… (who the recovery hands the station to)",
        ),
        station_owner_case(
            "station_owner_recovery_accept_v1",
            "wrfcoin_station_owner_recovery_accept_v1",
            900_133,
            build(STATION_OWNER_RECOVERY_ACCEPT_DOMAIN),
            input,
            tampered,
            "to_owner_address c3… -> d5… (who the recovery hands the station to)",
        ),
    ]
}

fn build_station_owner_recovery_cancel_case() -> Value {
    use wrfcoin_shared_protocol::blockchain::station_owner::station_owner_recovery_cancel_message_with_domain;
    let identity = identity();
    let input = json!({
        "chain_id": identity.chain_id, "network_id": identity.network_id,
        "station_id": "station-golden-3345",
        "owner_address": "wrf".to_string() + &"a1".repeat(20),
        "recovery_nonce": 7u64,
    });
    let mut tampered = input.clone();
    tampered["recovery_nonce"] = json!(8u64);
    station_owner_case(
        "station_owner_recovery_cancel_v1",
        "wrfcoin_station_owner_recovery_cancel_v1",
        900_134,
        |i: &Value| {
            let s = |k: &str| i[k].as_str().expect(k).to_string();
            station_owner_recovery_cancel_message_with_domain(
                &s("chain_id"),
                &s("network_id"),
                &s("station_id"),
                &s("owner_address"),
                i["recovery_nonce"].as_u64().unwrap(),
            )
        },
        input,
        tampered,
        "recovery_nonce 7 -> 8 (which pending recovery is cancelled)",
    )
}

/// One golden per owner-action discriminant (#3347): the discriminant byte is
/// part of the preimage, so each of `report_stolen`=1, `unfreeze`=2,
/// `deactivate`=3, `reactivate`=4 needs its own pinned bytes. Each case's
/// tampered twin swaps in a DIFFERENT action (a signature for one action must
/// never authorize another). Seeds 900_135..900_138; 900_135 is the original
/// `report_stolen` case, byte-for-byte unchanged.
fn build_station_owner_action_case(action: &str, tamper_to: &str, seed: u64) -> Value {
    use wrfcoin_shared_protocol::blockchain::transaction::station_owner_action_message_with_domain;
    let identity = identity();
    let input = json!({
        "chain_id": identity.chain_id, "network_id": identity.network_id,
        "station_id": "station-golden-3347",
        "owner_address": "wrf".to_string() + &"a1".repeat(20),
        "action": action,
        "action_nonce": 7u64,
        "expires_at_height": 21_000u64,
    });
    let mut tampered = input.clone();
    tampered["action"] = json!(tamper_to);
    station_owner_case(
        "station_owner_action_v1",
        "wrfcoin_station_owner_action_v1",
        seed,
        |i: &Value| {
            let s = |k: &str| i[k].as_str().expect(k).to_string();
            station_owner_action_message_with_domain(
                &s("chain_id"),
                &s("network_id"),
                &s("station_id"),
                &s("owner_address"),
                serde_json::from_value(i["action"].clone()).expect("action"),
                i["action_nonce"].as_u64().unwrap(),
                i["expires_at_height"].as_u64().unwrap(),
            )
        },
        input,
        tampered,
        &format!(
            "action {action} -> {tamper_to} (a signed discriminant cannot be replayed as another action)"
        ),
    )
}

/// Sponsored first sends: the sponsor's authorization of one transfer's fee
/// (`accounts::paymaster::sponsored_send_authorization_message_with_domain`).
/// Pinned so the sponsor service's signer cannot drift from consensus.
fn build_sponsored_send_authorization_case() -> Value {
    use wrfcoin_shared_protocol::accounts::paymaster::sponsored_send_authorization_message_with_domain;
    let identity = identity();
    let input = json!({
        "chain_id": identity.chain_id, "network_id": identity.network_id,
        "sponsor": "0x".to_string() + &"5a".repeat(20),
        "from": "0x".to_string() + &"a1".repeat(20),
        "to": "0x".to_string() + &"b2".repeat(20),
        "amount": 10_000_000u64,
        "nonce": 0u64,
        "fee": 5_000u64,
        "expires_at_height": 21_000u64,
        "balance_floor": 100_000_000u64,
        "first_send_only": true,
    });
    let mut tampered = input.clone();
    tampered["first_send_only"] = json!(false);
    station_owner_case(
        "sponsored_send_authorization_v1",
        "wrfcoin_sponsored_send_auth_v1",
        900_150,
        |i: &Value| {
            let s = |k: &str| i[k].as_str().expect(k).to_string();
            let n = |k: &str| i[k].as_u64().expect(k);
            sponsored_send_authorization_message_with_domain(
                &s("chain_id"),
                &s("network_id"),
                &s("sponsor"),
                &s("from"),
                &s("to"),
                n("amount"),
                n("nonce"),
                n("fee"),
                n("expires_at_height"),
                n("balance_floor"),
                i["first_send_only"].as_bool().expect("first_send_only"),
            )
        },
        input,
        tampered,
        "first_send_only true -> false (a sponsor's rule cannot be widened after signing)",
    )
}

fn build_all_cases() -> Vec<Value> {
    let mut cases = Vec::new();
    {
        let (g, t, n) = golden_transaction();
        cases.push(build_case("Transaction", 900_001, &g, &t, n));
    }
    {
        let (g, t, n) = golden_block();
        cases.push(build_case("BlockHeader", 900_002, &g, &t, n));
    }
    {
        let (g, t, n) = golden_slashing();
        cases.push(build_case("SlashingEvidence", 900_003, &g, &t, n));
    }
    {
        let (g, t, n) = golden_snapshot();
        cases.push(build_case("SnapshotPayload", 900_004, &g, &t, n));
    }
    {
        let (g, t, n) = golden_oracle_attestation();
        cases.push(build_case("OracleAttestation", 900_005, &g, &t, n));
    }
    {
        let (g, t, n) = golden_calibration();
        cases.push(build_case("AttestationEnvelope", 900_006, &g, &t, n));
    }
    {
        let (g, t, n) = golden_bridge_message();
        cases.push(build_case("BridgeMessage", 900_007, &g, &t, n));
    }
    {
        let (g, t, n) = golden_compute_receipt();
        cases.push(build_case("ComputeReceipt", 900_008, &g, &t, n));
    }
    {
        let (g, t, n) = golden_forecast_receipt();
        cases.push(build_case("ForecastVerificationReceipt", 900_009, &g, &t, n));
    }
    {
        let (g, t, n) = golden_node_service_receipt();
        cases.push(build_case("NodeServiceReceipt", 900_140, &g, &t, n));
    }
    cases.push(build_commit_authorization_case());
    cases.push(build_transparent_transfer_authorization_case());
    cases.push(build_system_transfer_authorization_case());
    cases.push(build_stake_authorization_case());
    cases.push(build_gamification_authorization_case());
    cases.push(build_oracle_registration_authorization_case());
    cases.push(build_station_registration_authorization_case());
    cases.push(build_reward_claim_authorization_case());
    cases.push(build_validator_key_rotation_pop_case());
    cases.push(build_selective_disclosure_authorization_case());
    cases.push(build_confidential_transfer_authorization_case());
    cases.push(build_account_recovery_guardian_approval_case());
    cases.push(build_account_recovery_owner_set_policy_case());
    cases.push(build_account_recovery_owner_cancel_case());
    cases.push(build_account_recovery_owner_cancel_policy_change_case());
    cases.push(build_session_key_register_case());
    cases.push(build_session_key_revoke_case());
    cases.push(build_session_key_use_case());
    cases.push(build_declare_authorization_case(None, "none", "declare_authorization_v2", 900_120));
    cases.push(build_declare_authorization_case(
        Some(wrfcoin_shared_protocol::accounts::authorization::AccountAuthorization::declaring(
            wrfcoin_shared_protocol::accounts::authorization::AuthorizationRule::SignerIsSender,
        )),
        "signer_is_sender",
        "declare_authorization_signer_is_sender_v2",
        900_121,
    ));
    cases.extend(build_station_owner_transfer_cases());
    cases.extend(build_station_owner_recovery_filing_cases());
    cases.push(build_station_owner_recovery_cancel_case());
    cases.push(build_station_owner_action_case("report_stolen", "unfreeze", 900_135));
    cases.push(build_station_owner_action_case("unfreeze", "report_stolen", 900_136));
    cases.push(build_station_owner_action_case("deactivate", "reactivate", 900_137));
    cases.push(build_station_owner_action_case("reactivate", "deactivate", 900_138));
    cases.push(build_station_registration_authorization_v2_case());
    cases.push(build_sponsored_send_authorization_case());
    cases
}


fn fixture_document() -> Value {
    json!({
        "schema": SCHEMA,
        "supersedes": "docs/specifications/SIGNING_PREIMAGE_V2.md (prose is descriptive; these bytes are normative)",
        "frame": "len8le(tag)||tag || len8le(chain_id)||chain_id || len8le(network_id)||network_id || u32le(version) || len8le(body)||body ; V1 = len8le(tag)||tag||len8le(body)||body",
        "chain_id": identity().chain_id,
        "network_id": identity().network_id,
        "regenerate": format!("{REGEN_ENV}=1 cargo test -p wrfcoin-shared-protocol --test signing_preimage_v2_golden"),
        "cases": build_all_cases(),
    })
}


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Regenerates the fixture when `WRFCOIN_REGEN_GOLDEN_VECTORS=1`; otherwise a
/// no-op. Kept as a test so regeneration runs inside the exact same build.
#[test]
fn regenerate_fixture_when_requested() {
    if std::env::var(REGEN_ENV).as_deref() != Ok("1") {
        return;
    }
    let doc = fixture_document();
    let pretty = serde_json::to_string_pretty(&doc).expect("fixture serializes");
    std::fs::create_dir_all("tests/fixtures/golden").expect("fixture dir");
    std::fs::write(FIXTURE_PATH, pretty + "\n").expect("write fixture");
    eprintln!("[golden] regenerated {FIXTURE_PATH} — review the diff before committing");
}

/// The core conformance run: every pinned case reproduces byte-for-byte from
/// its logical input, and every tampered twin diverges and fails verification.
#[test]
fn golden_vectors_reproduce_byte_for_byte_and_tampered_twins_fail() {
    let fixture = load_fixture();
    assert_eq!(
        fixture["schema"].as_str(),
        Some(SCHEMA),
        "fixture schema mismatch — regenerate deliberately and bump SCHEMA together \
         with the consensus change that moved the bytes"
    );
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(!cases.is_empty(), "empty fixture");
    for case in cases {
        check_case_by_kind(case);
    }
}

/// The current build produces EXACTLY the committed fixture — regeneration is
/// a no-op. This is stronger than per-case checks alone: it also pins case
/// count, ordering, schema and metadata, so a case cannot be dropped.
#[test]
fn fixture_is_reproducible_from_this_build() {
    let committed = load_fixture();
    let regenerated = fixture_document();
    assert_eq!(
        committed, regenerated,
        "the committed fixture does not match what this build generates. If the \
         change is INTENDED (a consensus preimage change), regenerate with \
         {REGEN_ENV}=1, bump SCHEMA, and say so in the PR; anything else is signer \
         drift — the #2076 class this fixture exists to catch."
    );
}

/// Coverage reconciliation, the fixture side: every registry row in the #2226
/// migration set has exactly one golden case, and the fixture carries no kind
/// outside the set. (`signed_type_inventory.rs` holds the mirror-image check
/// so the audit binary fails too if the fixture goes stale.)
#[test]
fn every_2226_registry_row_has_exactly_one_golden_case() {
    let fixture = load_fixture();
    // Registry reconciliation is scoped to composed-frame cases: the
    // money_path_v2 cases pin message CONSTRUCTIONS (collector/faucet live
    // contracts), not DomainSeparated registry types.
    let fixture_kinds: Vec<&str> = fixture["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .filter(|c| c["frame"].as_str() == Some("composed_v2"))
        .map(|c| c["kind"].as_str().expect("kind"))
        .collect();

    let registry_kinds: Vec<&str> = SIGNED_OBJECT_REGISTRY
        .iter()
        .filter(|e| {
            matches!(
                e.coverage,
                DomainCoverage::MigrationDeferred { issue } if issue == MIGRATION_TRACKING_ISSUE
            ) || e.kind.as_str() == "NodeServiceReceipt"
        })
        .map(|e| e.kind.as_str())
        .collect();

    for kind in &registry_kinds {
        assert_eq!(
            fixture_kinds.iter().filter(|k| k == &kind).count(),
            1,
            "registry row {kind} (MigrationDeferred/#2226) must have exactly one \
             golden case — a signed type without a pinned vector is un-audited \
             cross-repo"
        );
    }
    for kind in &fixture_kinds {
        assert!(
            registry_kinds.contains(kind),
            "fixture case {kind} has no MigrationDeferred/#2226 registry row — \
             either the registry moved (re-derive the fixture deliberately) or \
             the case is orphaned"
        );
    }

    // The two pre-cut money-path cases must exist until the #2226 collapse
    // retires their constructions — the live collector/faucet contract must
    // not silently fall out of the fixture.
    for kind in [
        "commit_authorization_v2",
        "transparent_transfer_authorization_v2",
        "system_transfer_authorization_v2",
        "stake_authorization_v2",
        "gamification_authorization_v1",
        "oracle_registration_authorization_v1",
        "reward_claim_authorization_v2",
        "validator_key_rotation_pop_v2",
        "selective_disclosure_authorization_v2",
        "confidential_transfer_authorization_v2",
        "account_recovery_guardian_approval_v2",
        "account_recovery_owner_set_policy_v2",
        "account_recovery_owner_cancel_v2",
        "account_recovery_owner_cancel_policy_change_v2",
    ] {
        assert_eq!(
            fixture["cases"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|c| c["frame"].as_str() == Some("money_path_v2")
                    && c["kind"].as_str() == Some(kind))
                .count(),
            1,
            "money-path case {kind} missing — the live infra signer contract is unpinned"
        );
    }
}
