//! Shared golden-vector conformance core (#2034 cut prep).
//!
//! Lives in `tests/common/` (a module, not a test target) so BOTH
//! `signing_preimage_v2_golden.rs` (the fixture generator + primary harness)
//! AND `signed_type_inventory.rs` (the audit binary) execute the *same*
//! byte-recompute-and-compare logic. That is deliberate: the §7 upgrade —
//! from "is this type registered" to "does it produce these pinned bytes" —
//! is only real if the audit lane recomputes the bytes itself. A helper that
//! merely inspects fixture strings is vacuous (it stays green when the pinned
//! bytes are corrupted); routing the audit through `check_case_by_kind` makes
//! byte drift fail the audit too, not only the separate fixture binary.

#![allow(dead_code)] // each including crate uses a subset

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use rand::SeedableRng;
use rand::rngs::StdRng;

use wrfcoin_forecast_score::blockchain::ForecastVerificationReceipt;
use wrfcoin_shared_protocol::accounts::recovery::{
    RecoveryPolicy, guardian_approval_message, owner_cancel_message,
    owner_cancel_policy_change_message, owner_set_policy_message,
};
use wrfcoin_shared_protocol::accounts::authorization::{
    declare_authorization_message, AccountAuthorization, AuthorizationRule,
};
use wrfcoin_shared_protocol::accounts::session_keys::{
    MethodId, SessionKeyGrant, SessionScope, session_key_register_message,
    session_key_revoke_message, session_key_use_message,
};
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::snapshot_sync::SnapshotSyncPayload;
use wrfcoin_shared_protocol::blockchain::transaction::{
    CalibrationAttestationPayload, ComputeReceipt, ConfidentialTransferPayload,
    OracleAttestationPayload, OracleRegistrationPayload, RewardClaimPayload,
    SelectiveDisclosureProofPayload, StationRegistrationPayload, Transaction,
    commit_authorization_message_with_domain,
    confidential_transfer_authorization_message_with_domain,
    gamification_authorization_message_with_domain,
    oracle_registration_authorization_message_with_domain,
    reward_claim_authorization_message_with_domain,
    station_registration_authorization_message_with_domain,
    selective_disclosure_authorization_digest_with_domain,
    stake_authorization_message_with_domain, system_transfer_authorization_message_with_domain,
    transparent_transfer_authorization_message_with_domain,
};
use wrfcoin_shared_protocol::cross_chain::types::BridgeMessage;
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::chain_binding::{ChainIdentity, SigningProtocolVersion};
use wrfcoin_shared_protocol::crypto::domain_signing::{DomainSeparated, verify_domain_for};
use wrfcoin_shared_protocol::crypto::rotation::rotation_pop_message_with_domain;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
use wrfcoin_shared_protocol::security::slashing::SlashingReportPayload;

pub const FIXTURE_PATH: &str = "tests/fixtures/golden/signing_preimage_v2.json";
pub const SCHEMA: &str = "wrfcoin.signing-preimage-v2.golden/4";
pub const REGEN_ENV: &str = "WRFCOIN_REGEN_GOLDEN_VECTORS";

/// Fixed identity for the vectors — explicit, never `ChainIdentity::active()`
/// (which reads process state and would make vectors depend on when/where they
/// were generated).
pub fn identity() -> ChainIdentity {
    ChainIdentity::new("wrfcoin-testnet", "testnet")
}

pub fn keypair(seed: u64) -> (PrivateKey, PublicKey) {
    let mut rng = StdRng::seed_from_u64(seed);
    let sk = PrivateKey::generate(&mut rng);
    let pk = PublicKey::from_private_key(&sk);
    (sk, pk)
}

pub fn load_fixture() -> Value {
    let raw = std::fs::read_to_string(FIXTURE_PATH).unwrap_or_else(|e| {
        panic!(
            "missing golden-vector fixture {FIXTURE_PATH}: {e}. Generate it with \
             {REGEN_ENV}=1 and commit the result."
        )
    });
    serde_json::from_str(&raw).expect("golden-vector fixture must parse")
}

/// The composed-frame conformance contract, executed for one case: rebuild
/// from `input`, reproduce body / V1-preimage / V2-preimage / signature
/// byte-for-byte, verify; rebuild from `tampered_input`, get a DIFFERENT
/// preimage and a FAILED verification under the golden signature.
pub fn check_case<T: DomainSeparated + Serialize + DeserializeOwned>(case: &Value) {
    let identity = identity();
    let kind = case["kind"].as_str().expect("kind");
    let seed = case["signer_seed"].as_u64().expect("seed");
    let (sk, pk) = keypair(seed);
    assert_eq!(
        pk.to_hex(),
        case["signer_public_key_hex"].as_str().expect("pk"),
        "{kind}: seeded keypair no longer reproduces the pinned public key"
    );
    // The portable seed must match the seeded key too — Node consumers rely on
    // it, so a drift between the two key encodings is a cross-language break.
    assert_eq!(
        sk.to_hex(),
        case["signer_secret_hex"].as_str().expect("secret"),
        "{kind}: signer_secret_hex drifted from the seeded key"
    );

    let golden: T =
        serde_json::from_value(case["input"].clone()).expect("golden input deserializes");
    assert_eq!(
        hex::encode(golden.canonical_body()),
        case["canonical_body_hex"].as_str().expect("body"),
        "{kind}: canonical_body drifted from the golden vector"
    );
    assert_eq!(
        hex::encode(golden.signing_preimage_for(&identity, SigningProtocolVersion::V1)),
        case["preimage_v1_hex"].as_str().expect("v1"),
        "{kind}: V1 preimage drifted — pre-cut signers are broken"
    );
    let preimage_v2 = golden.signing_preimage_for(&identity, SigningProtocolVersion::V2);
    assert_eq!(
        hex::encode(&preimage_v2),
        case["preimage_v2_hex"].as_str().expect("v2"),
        "{kind}: V2 preimage drifted from the golden vector"
    );
    let signature_v2 = sk.sign(&preimage_v2);
    assert_eq!(
        signature_v2.to_hex(),
        case["signature_v2_hex"].as_str().expect("sig"),
        "{kind}: deterministic ed25519 signature drifted"
    );
    assert!(
        verify_domain_for(&pk, &golden, &signature_v2, &identity, SigningProtocolVersion::V2),
        "{kind}: golden signature must verify through the production verifier"
    );

    let tampered: T =
        serde_json::from_value(case["tampered_input"].clone()).expect("tampered deserializes");
    let tampered_v2 = tampered.signing_preimage_for(&identity, SigningProtocolVersion::V2);
    assert_ne!(
        hex::encode(&tampered_v2),
        case["preimage_v2_hex"].as_str().expect("v2"),
        "{kind}: tampered input produced the GOLDEN preimage — the tampered \
         field is not folded into the body ({})",
        case["tamper_note"].as_str().unwrap_or("?")
    );
    assert!(
        !verify_domain_for(&pk, &tampered, &signature_v2, &identity, SigningProtocolVersion::V2),
        "{kind}: golden signature verified over the tampered input"
    );
}

fn money_path_preimage(case: &Value, input_key: &str) -> Vec<u8> {
    let input = &case[input_key];
    let s = |k: &str| input[k].as_str().unwrap_or_else(|| panic!("{input_key}.{k}"));
    match case["kind"].as_str().expect("kind") {
        "commit_authorization_v2" => commit_authorization_message_with_domain(
            s("chain_id"),
            s("network_id"),
            s("station_id"),
            s("commit_id"),
            s("contribution_type"),
        ),
        "transparent_transfer_authorization_v2" => {
            transparent_transfer_authorization_message_with_domain(
                s("chain_id"),
                s("network_id"),
                s("from"),
                s("to"),
                input["amount"].as_u64().expect("amount"),
                input["nonce"].as_u64().expect("nonce"),
            )
        },
        "system_transfer_authorization_v2" => system_transfer_authorization_message_with_domain(
            s("chain_id"),
            s("network_id"),
            s("from"),
            s("to"),
            input["amount"].as_u64().expect("amount"),
            input["nonce"].as_u64().expect("nonce"),
        ),
        "stake_authorization_v2" => stake_authorization_message_with_domain(
            s("chain_id"),
            s("network_id"),
            s("account"),
            input["amount"].as_u64().expect("amount"),
            input["is_unstake"].as_bool().expect("is_unstake"),
            input["nonce"].as_u64().expect("nonce"),
        ),
        // #2370: the two payloads Dan's review corrected — each signs its OWN
        // domain-tagged authorization message, verified on live transition and
        // block-verification paths, NOT the outer Transaction preimage.
        "gamification_authorization_v1" => gamification_authorization_message_with_domain(
            s("chain_id"),
            s("network_id"),
            s("claim_id"),
            s("recipient"),
            input["amount"].as_u64().expect("amount"),
        ),
        "oracle_registration_authorization_v1" => {
            let payload = OracleRegistrationPayload::new(
                input["oracle_set"]
                    .as_array()
                    .expect("oracle_set")
                    .iter()
                    .map(|v| v.as_str().expect("oracle id").to_string())
                    .collect(),
                input["oracle_quorum"].as_u64().expect("quorum") as usize,
                s("identity"),
                s("public_key"),
                input["epoch"].as_u64().expect("epoch"),
                input["activated_at"].as_i64().expect("activated_at"),
            );
            oracle_registration_authorization_message_with_domain(
                s("chain_id"),
                s("network_id"),
                &payload,
            )
        },
        // #2537: the station-registration governance authorization. Pinned
        // because this preimage carries `provenance_class` — the byte that
        // decides whether a station's contributions can earn at all — and
        // `owner_pubkey`, which binds the possession proof governance approved.
        "station_registration_authorization_v1" => {
            let mut payload = StationRegistrationPayload::new(
                s("station_id"),
                s("owner_address"),
                input["latitude"].as_f64().expect("latitude"),
                input["longitude"].as_f64().expect("longitude"),
                s("hardware_model"),
                serde_json::from_value(input["data_quality_tier"].clone())
                    .expect("data_quality_tier"),
                serde_json::from_value(input["provenance_class"].clone())
                    .expect("provenance_class"),
            );
            payload.is_active = input["is_active"].as_bool().expect("is_active");
            payload.owner_pubkey = Some(s("owner_pubkey").to_string());
            station_registration_authorization_message_with_domain(
                s("chain_id"),
                s("network_id"),
                &payload,
            )
        },
        "station_registration_authorization_v2" => {
            let mut payload = StationRegistrationPayload::new(
                s("station_id"),
                s("owner_address"),
                input["latitude"].as_f64().expect("latitude"),
                input["longitude"].as_f64().expect("longitude"),
                s("hardware_model"),
                serde_json::from_value(input["data_quality_tier"].clone())
                    .expect("data_quality_tier"),
                serde_json::from_value(input["provenance_class"].clone())
                    .expect("provenance_class"),
            )
            .with_expected_nonce(input["expected_nonce"].as_u64().expect("expected_nonce"));
            payload.is_active = input["is_active"].as_bool().expect("is_active");
            payload.owner_pubkey = Some(s("owner_pubkey").to_string());
            station_registration_authorization_message_with_domain(
                s("chain_id"),
                s("network_id"),
                &payload,
            )
        },
        "reward_claim_authorization_v2" => {
            let payload = RewardClaimPayload {
                epoch: input["epoch"].as_u64().expect("epoch"),
                ring_signature: String::new(),
                key_image: s("key_image").to_string(),
                amount_commitment: s("amount_commitment").to_string(),
                range_proof: s("range_proof").to_string(),
                destination: s("destination").to_string(),
            };
            reward_claim_authorization_message_with_domain(
                s("chain_id"),
                s("network_id"),
                &payload,
            )
        },
        "validator_key_rotation_pop_v2" => {
            let old = PublicKey::from_hex(s("old_public_key")).expect("old public key");
            let new = PublicKey::from_hex(s("new_public_key")).expect("new public key");
            rotation_pop_message_with_domain(
                s("chain_id"),
                s("network_id"),
                &old,
                &new,
                input["effective_height"].as_u64().expect("effective_height"),
            )
        },
        "selective_disclosure_authorization_v2" => {
            let payload = SelectiveDisclosureProofPayload {
                disclosure_id: s("disclosure_id").to_string(),
                scope: input["scope"]
                    .as_array()
                    .expect("scope")
                    .iter()
                    .map(|v| v.as_str().expect("scope key").to_string())
                    .collect(),
                expires_at_block: input["expires_at_block"]
                    .as_u64()
                    .expect("expires_at_block"),
                disclosed_fields_hash: s("disclosed_fields_hash").to_string(),
                verifier_attestation: String::new(),
            };
            selective_disclosure_authorization_digest_with_domain(
                s("chain_id"),
                s("network_id"),
                &payload,
            )
            .as_bytes()
            .to_vec()
        },
        "confidential_transfer_authorization_v2" => {
            let payload: ConfidentialTransferPayload =
                serde_json::from_value(input.clone()).expect("confidential transfer payload");
            confidential_transfer_authorization_message_with_domain(
                s("chain_id"),
                s("network_id"),
                &payload,
            )
        },
        "account_recovery_guardian_approval_v2" => {
            let account: Address = s("account").parse().expect("account address");
            guardian_approval_message(
                &account,
                s("new_owner"),
                input["initiated_at_height"]
                    .as_u64()
                    .expect("initiated_at_height"),
            )
        },
        "account_recovery_owner_set_policy_v2" => {
            let account: Address = s("account").parse().expect("account address");
            let policy = RecoveryPolicy {
                guardians: input["guardians"]
                    .as_array()
                    .expect("guardians")
                    .iter()
                    .map(|guardian| {
                        guardian
                            .as_str()
                            .expect("guardian address")
                            .parse()
                            .expect("guardian address")
                    })
                    .collect(),
                threshold: input["threshold"].as_u64().expect("threshold") as u16,
                time_lock_blocks: input["time_lock_blocks"]
                    .as_u64()
                    .expect("time_lock_blocks"),
                cooldown_blocks: input["cooldown_blocks"]
                    .as_u64()
                    .expect("cooldown_blocks"),
            };
            owner_set_policy_message(
                &account,
                &policy,
                input["policy_nonce"].as_u64().expect("policy_nonce"),
            )
        },
        "account_recovery_owner_cancel_v2" => {
            let account: Address = s("account").parse().expect("account address");
            owner_cancel_message(
                &account,
                input["initiated_at_height"]
                    .as_u64()
                    .expect("initiated_at_height"),
            )
        },
        "account_recovery_owner_cancel_policy_change_v2" => {
            let account: Address = s("account").parse().expect("account address");
            owner_cancel_policy_change_message(
                &account,
                input["initiated_at_height"]
                    .as_u64()
                    .expect("initiated_at_height"),
            )
        },
        "session_key_register_v2" => {
            let owner: Address = s("owner").parse().expect("owner address");
            let grant = SessionKeyGrant {
                public_key: PublicKey::from_hex(s("session_public_key"))
                    .expect("session public key"),
                scope: SessionScope {
                    allowed_methods: input["allowed_methods"]
                        .as_array()
                        .expect("allowed_methods")
                        .iter()
                        .map(|v| MethodId(v.as_str().expect("method").to_string()))
                        .collect(),
                    allowed_targets: input["allowed_targets"]
                        .as_array()
                        .expect("allowed_targets")
                        .iter()
                        .map(|v| v.as_str().expect("target").parse().expect("target address"))
                        .collect(),
                    max_value_per_use: s("max_value_per_use").parse().expect("max per use"),
                    max_total_value: s("max_total_value").parse().expect("max total"),
                    window: None,
                },
                valid_from: input["valid_from"].as_u64().expect("valid_from"),
                valid_until: input["valid_until"].as_u64().expect("valid_until"),
                max_uses: input["max_uses"].as_u64().map(|v| v as u32),
            };
            session_key_register_message(
                s("chain_id"),
                &owner,
                input["registration_nonce"]
                    .as_u64()
                    .expect("registration_nonce"),
                &grant,
            )
        },
        "session_key_revoke_v2" => {
            let owner: Address = s("owner").parse().expect("owner address");
            let session_key: Address = s("session_key").parse().expect("session key address");
            session_key_revoke_message(
                s("chain_id"),
                &owner,
                &session_key,
                input["registration_nonce"]
                    .as_u64()
                    .expect("registration_nonce"),
            )
        },
        "session_key_use_v2" => {
            let owner: Address = s("owner").parse().expect("owner address");
            let session_key: Address = s("session_key").parse().expect("session key address");
            let target: Address = s("target").parse().expect("target address");
            session_key_use_message(
                s("chain_id"),
                &owner,
                &session_key,
                &MethodId(s("method").to_string()),
                &target,
                s("value").parse().expect("value"),
                input["registration_nonce"]
                    .as_u64()
                    .expect("registration_nonce"),
                input["use_index"].as_u64().expect("use_index") as u32,
            )
        },
        "send_allowlist_action_v2" => {
            use wrfcoin_shared_protocol::accounts::send_allowlist::{
                SendAllowlistAction, send_allowlist_action_message,
            };
            let account: Address = s("account").parse().expect("account address");
            let action: SendAllowlistAction =
                serde_json::from_value(input["action"].clone()).expect("send allowlist action");
            send_allowlist_action_message(
                s("chain_id"),
                &account,
                &action,
                input["nonce"].as_u64().expect("nonce"),
            )
        },
        "declare_authorization_v2" | "declare_authorization_signer_is_sender_v2" => {
            let account: Address = s("account").parse().expect("account address");
            let authorization = match s("authorization") {
                "none" => None,
                "signer_is_sender" => Some(AccountAuthorization::declaring(AuthorizationRule::SignerIsSender)),
                other => panic!("unknown declaration authorization {other:?}"),
            };
            declare_authorization_message(
                s("chain_id"),
                &account,
                &authorization,
                input["nonce"].as_u64().expect("nonce"),
                input["generation"].as_u64().expect("generation"),
            )
        },
        "security_key_action_v1" => {
            use wrfcoin_shared_protocol::accounts::security_key::{SecurityKeyActionBody, action_message};
            let account: Address = s("account").parse().expect("account address");
            assert_eq!(s("action"), "begin_escape", "golden action body");
            action_message(
                s("chain_id"),
                s("network_id"),
                &account,
                input["nonce"].as_u64().expect("nonce"),
                &SecurityKeyActionBody::BeginEscape,
            )
            .expect("begin_escape encodes")
        },
        // #3345 / #3347 owner ledger (owner rulings 2026-09-27): one kind per
        // domain tag; the sender/acceptor pairs share a body and differ only
        // in the tag.
        "station_owner_transfer_v1" | "station_owner_transfer_accept_v1" => {
            use wrfcoin_shared_protocol::blockchain::transaction::{
                STATION_OWNER_TRANSFER_ACCEPT_DOMAIN, STATION_OWNER_TRANSFER_DOMAIN,
                station_owner_transfer_message_with_domain,
            };
            let domain = if case["kind"] == "station_owner_transfer_v1" {
                STATION_OWNER_TRANSFER_DOMAIN
            } else {
                STATION_OWNER_TRANSFER_ACCEPT_DOMAIN
            };
            station_owner_transfer_message_with_domain(
                domain,
                s("chain_id"),
                s("network_id"),
                s("station_id"),
                s("from_owner_address"),
                s("to_owner_address"),
                s("to_owner_pubkey"),
                input["transfer_nonce"].as_u64().expect("transfer_nonce"),
                input["expires_at_height"].as_u64().expect("expires_at_height"),
            )
        },
        "station_owner_recovery_file_v1" | "station_owner_recovery_accept_v1" => {
            use wrfcoin_shared_protocol::blockchain::station_owner::{
                STATION_OWNER_RECOVERY_ACCEPT_DOMAIN, STATION_OWNER_RECOVERY_FILE_DOMAIN,
                station_owner_recovery_message_with_domain,
            };
            let domain = if case["kind"] == "station_owner_recovery_file_v1" {
                STATION_OWNER_RECOVERY_FILE_DOMAIN
            } else {
                STATION_OWNER_RECOVERY_ACCEPT_DOMAIN
            };
            station_owner_recovery_message_with_domain(
                domain,
                s("chain_id"),
                s("network_id"),
                s("station_id"),
                s("from_owner_address"),
                s("to_owner_address"),
                s("to_owner_pubkey"),
                input["recovery_nonce"].as_u64().expect("recovery_nonce"),
            )
        },
        "station_owner_recovery_cancel_v1" => {
            wrfcoin_shared_protocol::blockchain::station_owner::station_owner_recovery_cancel_message_with_domain(
                s("chain_id"),
                s("network_id"),
                s("station_id"),
                s("owner_address"),
                input["recovery_nonce"].as_u64().expect("recovery_nonce"),
            )
        },
        "station_owner_action_v1" => {
            wrfcoin_shared_protocol::blockchain::transaction::station_owner_action_message_with_domain(
                s("chain_id"),
                s("network_id"),
                s("station_id"),
                s("owner_address"),
                serde_json::from_value(input["action"].clone()).expect("action"),
                input["action_nonce"].as_u64().expect("action_nonce"),
                input["expires_at_height"].as_u64().expect("expires_at_height"),
            )
        },
        "sponsored_send_authorization_v1" => {
            let n = |k: &str| input[k].as_u64().unwrap_or_else(|| panic!("{input_key}.{k}"));
            wrfcoin_shared_protocol::accounts::paymaster::sponsored_send_authorization_message_with_domain(
                s("chain_id"),
                s("network_id"),
                s("sponsor"),
                s("from"),
                s("to"),
                n("amount"),
                n("nonce"),
                n("fee"),
                n("expires_at_height"),
                n("balance_floor"),
                input["first_send_only"].as_bool().expect("first_send_only"),
            )
        },
        other => panic!("unknown money-path kind {other:?}"),
    }
}

/// The money-path (`frame: "money_path_v2"`) conformance contract — same
/// shape as `check_case` but over the `*_authorization_message_with_domain`
/// constructions the live collector/faucet sign.
pub fn check_money_path_case(case: &Value) {
    let kind = case["kind"].as_str().expect("kind");
    let seed = case["signer_seed"].as_u64().expect("seed");
    let (sk, pk) = keypair(seed);
    assert_eq!(
        sk.to_hex(),
        case["signer_secret_hex"].as_str().expect("secret"),
        "{kind}: pinned secret no longer derives from the seed"
    );
    assert_eq!(pk.to_hex(), case["signer_public_key_hex"].as_str().expect("pk"));

    let preimage = money_path_preimage(case, "input");
    assert_eq!(
        hex::encode(&preimage),
        case["preimage_hex"].as_str().expect("preimage"),
        "{kind}: money-path preimage drifted — the LIVE collector/faucet contract moved"
    );
    let signature = sk.sign(&preimage);
    assert_eq!(
        signature.to_hex(),
        case["signature_hex"].as_str().expect("sig"),
        "{kind}: deterministic signature drifted"
    );
    assert!(pk.verify(&preimage, &signature), "{kind}: golden signature must verify");

    let tampered = money_path_preimage(case, "tampered_input");
    assert_ne!(
        hex::encode(&tampered),
        case["preimage_hex"].as_str().expect("preimage"),
        "{kind}: tampered input produced the golden preimage ({})",
        case["tamper_note"].as_str().unwrap_or("?")
    );
    assert!(
        !pk.verify(&tampered, &signature),
        "{kind}: golden signature verified over the tampered preimage"
    );
}

/// Dispatch one fixture case to its typed conformance check, recomputing every
/// pinned byte from the logical input. This is the function the audit lane
/// calls, so byte drift fails the audit — not only the fixture binary.
pub fn check_case_by_kind(case: &Value) {
    if case["frame"].as_str() == Some("money_path_v2") {
        return check_money_path_case(case);
    }
    match case["kind"].as_str().expect("kind") {
        "Transaction" => check_case::<Transaction>(case),
        "BlockHeader" => check_case::<Block>(case),
        "SlashingEvidence" => check_case::<SlashingReportPayload>(case),
        "NodeServiceReceipt" => check_case::<wrfcoin_shared_protocol::consensus::node_service_receipt::NodeServiceReceipt>(case),
        "SnapshotPayload" => check_case::<SnapshotSyncPayload>(case),
        "OracleAttestation" => check_case::<OracleAttestationPayload>(case),
        "AttestationEnvelope" => check_case::<CalibrationAttestationPayload>(case),
        "BridgeMessage" => check_case::<BridgeMessage>(case),
        "ComputeReceipt" => check_case::<ComputeReceipt>(case),
        "ForecastVerificationReceipt" => check_case::<ForecastVerificationReceipt>(case),
        other => panic!("fixture carries unknown kind {other:?} — no harness drives it"),
    }
}
