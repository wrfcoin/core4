//! Real mounted payout-route controls; synthetic file keys only.
use super::*;
use actix_web::test;
use std::ffi::OsString;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;

const LEGACY: &str = "WRFCOIN_GAMIFICATION_OPERATOR_SIGNING_KEY";
const KEY_FILE: &str = "WRFCOIN_GAMIFICATION_OPERATOR_SIGNING_KEY_FILE";
const ALLOWLIST: &str = "WRFCOIN_GAMIFICATION_OPERATOR_PUBKEYS";

struct Fixture {
    prior: Vec<(&'static str, Option<OsString>)>,
    directory: tempfile::TempDir,
    path: PathBuf,
    secret: String,
    public: String,
}

#[cfg(test)]
impl Fixture {
    // Caller runs only in an isolated chain-domain child and holds its env lock until drop.
    fn new() -> Self {
        let prior = [
            LEGACY,
            KEY_FILE,
            ALLOWLIST,
            "WRFCOIN_CHAIN_ID",
            "WRFCOIN_NETWORK_ID",
        ]
        .into_iter()
        .map(|name| (name, std::env::var_os(name)))
        .collect();
        let directory = tempfile::tempdir().expect("synthetic fixture directory");
        let path = directory.path().join("payout.key");
        let key = PrivateKey::generate(&mut rand::thread_rng());
        let secret = key.to_hex();
        let public = PublicKey::from_private_key(&key).to_hex();
        std::fs::write(&path, format!("{secret}\n")).expect("synthetic fixture write"); // secret-leak-ok: synthetic fixture key written only to the temporary protected test file.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::env::remove_var(LEGACY);
        std::env::set_var(KEY_FILE, &path);
        std::env::set_var(ALLOWLIST, &public);
        Self {
            prior,
            directory,
            path,
            secret,
            public,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for (name, value) in &self.prior {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

#[cfg(test)]
mod supply_journal {
    use super::*;

    #[actix_web::test]
    async fn payout_route_mined_reward_keeps_supply_balanced() {
        if crate::helpers::test_env::isolated_chain_domain(
            "tests::payout_signer_3499::supply_journal::payout_route_mined_reward_keeps_supply_balanced",
            "777",
            "dev",
            "0x2727272727272727272727272727272727272727",
            "none",
        ) {
            return;
        }
        let _lock = crate::helpers::test_env::lock();
        let _fixture = Fixture::new();
        let state = test_state(None);
        let supervisor = wrfcoin_shared_protocol::invariants::balance::SharedSupplyInvariant::new();
        {
            let mut chain = state.blockchain.write().await;
            // Initialize BEFORE the payout so first-check adoption cannot mask an
            // unjournalled mint. This models an already-running node.
            chain.state.initialize_supply_baseline().unwrap();
            chain.state.supply_invariant = Some(supervisor.clone());
        }
        let app = test::init_service(test_app(state.clone())).await;
        let recipient = "0x1111111111111111111111111111111111111111";
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/rewards/payout")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: synthetic token supplied only to the in-process service.
                .set_json(json!({"address": recipient, "amount_base_units": 500,
                "claim_id": "supply-journal-route"}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let tx_id = state.mempool.read().await.get_all_transactions()[0].id;
        let config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
        let adapter = ConsensusFactory::create_pop_adapter(&config);
        adapter.declare_no_genesis_oracles_blocking();
        adapter
            .set_genesis_faucet_address(
                "0x2727272727272727272727272727272727272727"
                    .parse()
                    .unwrap(),
            )
            .await;
        let consensus = TypedConsensusManager::new(adapter, config);
        let block = consensus
            .create_pop_block_with_state(
                Arc::clone(&state.blockchain),
                Arc::clone(&state.mempool),
                "0x2222222222222222222222222222222222222222",
            )
            .await
            .expect("real producer must mine the admitted payout");
        assert!(
            block.transactions.iter().any(|tx| tx.id == tx_id),
            "payout must land"
        );
        let mut chain = state.blockchain.write().await;
        let address = recipient.parse().unwrap();
        assert_eq!(chain.state.get_account(&address).unwrap().balance.0, 500);
        assert!(
            chain
                .state
                .gamification_claims_seen
                .contains("supply-journal-route")
        );
        assert!(
            chain.state.supply_violation.is_none(),
            "{:?}",
            chain.state.supply_violation
        );
        chain.state.check_supply_conservation().unwrap();
        assert!(
            supervisor.violation().is_none(),
            "{:?}",
            supervisor.violation()
        );
    }
}

#[actix_web::test]
async fn payout_signer_file_refusals_3499() {
    if crate::helpers::test_env::isolated_chain_domain(
        "tests::payout_signer_3499::payout_signer_file_refusals_3499",
        "777",
        "testnet",
        "0x2727272727272727272727272727272727272727",
        "none",
    ) {
        return;
    }
    let _lock = crate::helpers::test_env::lock();
    let fixture = Fixture::new();
    let state = test_state(None);
    let app = test::init_service(test_app(state.clone())).await;
    macro_rules! refuse {
        ($code:expr) => {{
            let response = test::call_service(&app, test::TestRequest::post()
                .uri("/rewards/payout")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: synthetic test operator token supplied to the in-process test service.
                .set_json(json!({"address": "0x1111111111111111111111111111111111111111",
                    "amount_base_units": 100000000, "claim_id": "file-refusal-3499"}))
                .to_request()).await;
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body: serde_json::Value = test::read_body_json(response).await;
            assert_eq!(body["error"]["code"], $code);
            let text = body.to_string();
            assert!(!text.contains(&fixture.secret), "error leaked synthetic secret"); // secret-leak-ok: negative leak assertion compares only the synthetic fixture key.
            assert!(!text.contains(fixture.path.to_str().unwrap()), "error leaked private path");
            assert!(state.mempool.read().await.get_all_transactions().is_empty());
        }};
    }
    // Legacy presence must refuse with a valid file; even empty/non-Unicode
    // legacy values are invalid configuration, never a fallback selector.
    for sentinel in [
        OsString::from("legacy-setting-sentinel"),
        OsString::new(),
        <OsString as std::os::unix::ffi::OsStringExt>::from_vec(vec![0xff]),
    ] {
        std::env::set_var(LEGACY, sentinel);
        refuse!("OPERATOR_SIGNING_KEY_ENV_REFUSED");
    }
    // A seen-claim early return must not hide the unsupported env setting.
    state
        .blockchain
        .write()
        .await
        .state
        .gamification_claims_seen
        .insert("file-refusal-3499".into());
    refuse!("OPERATOR_SIGNING_KEY_ENV_REFUSED");
    state
        .blockchain
        .write()
        .await
        .state
        .gamification_claims_seen
        .clear();
    std::env::remove_var(LEGACY);
    for mode in [0o644, 0o640, 0o700, 0o400, 0o4600] {
        std::fs::set_permissions(&fixture.path, std::fs::Permissions::from_mode(mode)).unwrap();
        refuse!("OPERATOR_SIGNING_KEY_FILE_INSECURE");
    }
    let link = fixture.directory.path().join("payout-link.key");
    symlink(&fixture.path, &link).unwrap();
    std::env::set_var(KEY_FILE, &link);
    // World-readable target, then protected target: both symlinks refuse.
    std::fs::set_permissions(&fixture.path, std::fs::Permissions::from_mode(0o644)).unwrap();
    refuse!("OPERATOR_SIGNING_KEY_FILE_UNAVAILABLE");
    std::fs::set_permissions(&fixture.path, std::fs::Permissions::from_mode(0o600)).unwrap();
    refuse!("OPERATOR_SIGNING_KEY_FILE_UNAVAILABLE");
    std::env::set_var(KEY_FILE, fixture.directory.path().join("missing.key"));
    refuse!("OPERATOR_SIGNING_KEY_FILE_UNAVAILABLE");
    std::env::remove_var(KEY_FILE);
    refuse!("OPERATOR_SIGNING_KEY_UNSET");
    std::env::set_var(KEY_FILE, fixture.directory.path());
    refuse!("OPERATOR_SIGNING_KEY_FILE_INSECURE");
    std::env::set_var(KEY_FILE, &fixture.path);
    for contents in ["malformed-synthetic-secret".to_owned(), "a".repeat(129)] {
        std::fs::write(&fixture.path, &contents).unwrap();
        refuse!("OPERATOR_SIGNING_KEY_INVALID");
    }
    std::fs::write(&fixture.path, format!("{}\n", fixture.secret)).unwrap(); // secret-leak-ok: synthetic fixture key restored only in the temporary protected test file.
    std::env::remove_var(ALLOWLIST);
    refuse!("OPERATOR_KEY_NOT_ALLOWLISTED");
    std::env::set_var(ALLOWLIST, &fixture.public);

    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/rewards/payout")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: synthetic test operator token supplied to the in-process test service.
            .set_json(
                json!({"address": "0x1111111111111111111111111111111111111111",
            "amount_base_units": 100000000, "claim_id": "file-positive-3499"}),
            )
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body: serde_json::Value = test::read_body_json(response).await;
    assert_eq!(body["data"]["status"], "accepted");
    assert!(!body.to_string().contains(&fixture.secret)); // secret-leak-ok: negative leak assertion compares only the synthetic fixture key.
    let transactions = state.mempool.read().await.get_all_transactions();
    assert_eq!(transactions.len(), 1);
    assert!(transactions[0].verify_gamification_authorization());
}

#[actix_web::test]
async fn payout_signer_file_signature_binding_3499() {
    if crate::helpers::test_env::isolated_chain_domain(
        "tests::payout_signer_3499::payout_signer_file_signature_binding_3499",
        "777",
        "testnet",
        "0x2727272727272727272727272727272727272727",
        "none",
    ) {
        return;
    }
    let _lock = crate::helpers::test_env::lock();
    let fixture = Fixture::new();
    let state = test_state(None);
    let app = test::init_service(test_app(state.clone())).await;
    let response = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/rewards/payout")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: synthetic test operator token supplied to the in-process test service.
            .set_json(
                json!({"address": "0x1111111111111111111111111111111111111111",
            "amount_base_units": 100000000, "claim_id": "file-binding-3499"}),
            )
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let tx = state.mempool.read().await.get_all_transactions()[0]
        .as_ref()
        .clone();
    assert!(tx.verify_gamification_authorization());
    let mut changed = tx.clone();
    changed.outputs[0].amount += 1;
    assert!(!changed.verify_gamification_authorization());
    let mut changed = tx.clone();
    changed.outputs[0].address = "0x2222222222222222222222222222222222222222".into();
    assert!(!changed.verify_gamification_authorization());
    let mut changed = tx.clone();
    let mut claim: serde_json::Value =
        serde_json::from_str(changed.data.as_ref().unwrap()).unwrap();
    claim["claim_id"] = json!("changed-claim");
    changed.data = Some(claim.to_string());
    assert!(!changed.verify_gamification_authorization());
    std::env::set_var("WRFCOIN_CHAIN_ID", "778");
    assert!(!tx.verify_gamification_authorization());
    std::env::set_var("WRFCOIN_CHAIN_ID", "777");
    std::env::set_var("WRFCOIN_NETWORK_ID", "different-network");
    assert!(!tx.verify_gamification_authorization());
    std::env::set_var("WRFCOIN_NETWORK_ID", "testnet");
    std::env::remove_var(ALLOWLIST);
    assert!(!tx.verify_gamification_authorization());
    std::env::set_var(ALLOWLIST, &fixture.public);
    assert!(tx.verify_gamification_authorization());
}
