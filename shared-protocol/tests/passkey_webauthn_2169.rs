//! #2169: passkey / WebAuthn (P-256) account authorization — verifier and
//! payload-level tests, plus the Wycheproof P-256 harness.
//!
//! Nothing here installs the process-wide feature schedule; the activation
//! flip is `tests/passkey_activation_2169.rs` (one test, own process). So in
//! this binary `ConsensusFeature::PasskeyP256Auth` is never active and every
//! height-gated form must refuse a passkey-only payload — which is the
//! pre-activation contract.

use wrfcoin_shared_protocol::accounts::abstraction::SignatureVerifier;
use wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator;
use wrfcoin_shared_protocol::accounts::passkey::{
    AUTHENTICATOR_DATA_MIN_LEN, CONSENSUS_WEBAUTHN_POLICY, FLAG_USER_PRESENT, FLAG_USER_VERIFIED,
    MAX_WEBAUTHN_FIELD_LEN, P256Error, P256PublicKey, P256Signature, PASSKEY_TRANSFER_AUTH_DOMAIN,
    PasskeyAuthorizationError, PasskeyCredential, PasskeyError, PasskeySignatureVerifier,
    WebAuthnAssertion, WebAuthnPolicy, base64url_nopad, passkey_account_address,
    passkey_transfer_authorization_message_with_domain, passkey_transfer_challenge, verify_sha256,
    verify_webauthn,
};
use wrfcoin_shared_protocol::blockchain::transaction::{
    TRANSPARENT_TRANSFER_AUTH_DOMAIN, TransparentTransferPayload,
};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

use rand::{SeedableRng, rngs::StdRng};

const SECRET: [u8; 32] = [
    0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39,
    0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
];
const RP_ID: &str = "wallet.wrfcoin.test";
const CHALLENGE: [u8; 32] = [0xc1; 32];

fn authenticator() -> SoftwareAuthenticator {
    SoftwareAuthenticator::new(&SECRET, RP_ID)
}

fn verify(
    auth: &SoftwareAuthenticator,
    assertion: &WebAuthnAssertion,
    challenge: &[u8],
) -> Result<u32, PasskeyError> {
    verify_webauthn(
        &auth.credential(0),
        assertion,
        challenge,
        WebAuthnPolicy::default(),
    )
    .map(|v| v.sign_count)
}

// ---------------------------------------------------------------------------
// Full WebAuthn verification, one refusal per check
// ---------------------------------------------------------------------------

#[test]
fn a_valid_assertion_is_accepted() {
    let auth = authenticator();
    let assertion = auth.assert(&CHALLENGE);
    assert_eq!(verify(&auth, &assertion, &CHALLENGE), Ok(1));
}

#[test]
fn a_wrong_challenge_is_rejected() {
    let auth = authenticator();
    let assertion = auth.assert(&CHALLENGE);
    assert_eq!(
        verify(&auth, &assertion, &[0xc2; 32]),
        Err(PasskeyError::ChallengeMismatch)
    );
}

#[test]
fn a_wrong_rp_id_is_rejected() {
    let auth = authenticator();
    let other = SoftwareAuthenticator::new(&SECRET, "phishing.example");
    // Same key, other relying party: the assertion names the other RP.
    let assertion = other.assert(&CHALLENGE);
    assert_eq!(
        verify(&auth, &assertion, &CHALLENGE),
        Err(PasskeyError::RpIdMismatch)
    );
}

#[test]
fn a_registration_ceremony_is_not_an_authorization() {
    let auth = authenticator();
    let client_data = format!(
        "{{\"type\":\"webauthn.create\",\"challenge\":\"{}\",\"origin\":\"https://{RP_ID}\"}}",
        base64url_nopad(&CHALLENGE)
    )
    .into_bytes();
    let assertion = auth.assert_with(auth.authenticator_data(), client_data);
    assert_eq!(
        verify(&auth, &assertion, &CHALLENGE),
        Err(PasskeyError::WrongCeremonyType {
            found: "webauthn.create".to_string()
        })
    );
}

#[test]
fn user_presence_is_mandatory() {
    let mut auth = authenticator();
    auth.flags = FLAG_USER_VERIFIED; // verified but not present: nonsensical, refused
    let assertion = auth.assert(&CHALLENGE);
    assert_eq!(
        verify(&auth, &assertion, &CHALLENGE),
        Err(PasskeyError::UserNotPresent)
    );
}

#[test]
fn user_verification_is_required_by_the_consensus_policy_and_relaxable_elsewhere() {
    let mut auth = authenticator();
    auth.flags = FLAG_USER_PRESENT;
    let assertion = auth.assert(&CHALLENGE);
    assert_eq!(
        verify_webauthn(
            &auth.credential(0),
            &assertion,
            &CHALLENGE,
            CONSENSUS_WEBAUTHN_POLICY
        )
        .unwrap_err(),
        PasskeyError::UserNotVerified
    );
    let relaxed = WebAuthnPolicy {
        require_user_verification: false,
    };
    let verified = verify_webauthn(&auth.credential(0), &assertion, &CHALLENGE, relaxed).unwrap();
    assert!(!verified.user_verified);
}

#[test]
fn a_high_s_signature_is_malleable_on_the_curve_and_refused_by_the_verifier() {
    let auth = authenticator();
    let mut assertion = auth.assert(&CHALLENGE);
    let low = P256Signature::from_der(&assertion.signature).unwrap();
    assert!(low.is_low_s());
    let high = low.malleable_twin().expect("s != n/2");

    // The twin is a real signature over the same bytes: pure ECDSA accepts it.
    let mut signed = assertion.authenticator_data.clone();
    signed.extend_from_slice(&<sha2::Sha256 as sha2::Digest>::digest(
        &assertion.client_data_json,
    ));
    assert!(verify_sha256(&auth.key.public_key(), &signed, &high));

    // The verifier does not.
    assertion.signature = high.to_der_bytes();
    assert_eq!(
        verify(&auth, &assertion, &CHALLENGE),
        Err(PasskeyError::Curve(P256Error::HighS))
    );
}

#[test]
fn a_tampered_signature_is_rejected() {
    let auth = authenticator();
    let mut assertion = auth.assert(&CHALLENGE);
    let last = assertion.signature.len() - 1;
    assertion.signature[last] ^= 0x01;
    // Either the DER no longer parses or the curve check fails; both are
    // `Curve(_)`, never `Ok`.
    assert!(matches!(
        verify(&auth, &assertion, &CHALLENGE),
        Err(PasskeyError::Curve(P256Error::SignatureInvalid))
            | Err(PasskeyError::Curve(P256Error::InvalidSignatureEncoding))
    ));
}

#[test]
fn signature_counter_regression_policy() {
    let mut auth = authenticator();

    // Stored 5, asserted 5: not strictly greater — a clone replayed it.
    auth.next_sign_count = 5;
    let assertion = auth.assert(&CHALLENGE);
    assert_eq!(
        verify_webauthn(
            &auth.credential(5),
            &assertion,
            &CHALLENGE,
            WebAuthnPolicy::default()
        ),
        Err(PasskeyError::CounterRegression {
            previous: 5,
            observed: 5
        })
    );
    // Stored 5, asserted 3: regression.
    auth.next_sign_count = 3;
    let assertion = auth.assert(&CHALLENGE);
    assert_eq!(
        verify_webauthn(
            &auth.credential(5),
            &assertion,
            &CHALLENGE,
            WebAuthnPolicy::default()
        ),
        Err(PasskeyError::CounterRegression {
            previous: 5,
            observed: 3
        })
    );
    // Stored 5, asserted 6: advances.
    auth.next_sign_count = 6;
    let assertion = auth.assert(&CHALLENGE);
    assert_eq!(
        verify_webauthn(
            &auth.credential(5),
            &assertion,
            &CHALLENGE,
            WebAuthnPolicy::default()
        )
        .unwrap()
        .sign_count,
        6
    );
    // Stored 3, asserted 0: a counter-less assertion from a counting
    // credential is a regression too.
    auth.next_sign_count = 0;
    let assertion = auth.assert(&CHALLENGE);
    assert_eq!(
        verify_webauthn(
            &auth.credential(3),
            &assertion,
            &CHALLENGE,
            WebAuthnPolicy::default()
        ),
        Err(PasskeyError::CounterRegression {
            previous: 3,
            observed: 0
        })
    );
    // Both zero: an authenticator that implements no counter (WebAuthn §6.1.1).
    assert_eq!(
        verify_webauthn(
            &auth.credential(0),
            &assertion,
            &CHALLENGE,
            WebAuthnPolicy::default()
        )
        .unwrap()
        .sign_count,
        0
    );
}

#[test]
fn structural_refusals_are_typed_and_come_before_any_curve_work() {
    let auth = authenticator();

    let assertion = auth.assert_with(auth.authenticator_data(), b"not json".to_vec());
    assert_eq!(
        verify(&auth, &assertion, &CHALLENGE),
        Err(PasskeyError::ClientDataNotJson)
    );

    let assertion = auth.assert_with(
        vec![0u8; AUTHENTICATOR_DATA_MIN_LEN - 1],
        auth.client_data_json(&CHALLENGE),
    );
    assert_eq!(
        verify(&auth, &assertion, &CHALLENGE),
        Err(PasskeyError::AuthenticatorDataTooShort {
            len: AUTHENTICATOR_DATA_MIN_LEN - 1
        })
    );

    let mut oversized = auth.client_data_json(&CHALLENGE);
    oversized.resize(MAX_WEBAUTHN_FIELD_LEN + 1, b' ');
    let assertion = WebAuthnAssertion {
        authenticator_data: auth.authenticator_data(),
        client_data_json: oversized,
        signature: vec![],
    };
    assert_eq!(
        verify(&auth, &assertion, &CHALLENGE),
        Err(PasskeyError::FieldTooLong {
            field: "clientDataJSON",
            len: MAX_WEBAUTHN_FIELD_LEN + 1
        })
    );
}

#[test]
fn an_invalid_credential_key_is_a_curve_error() {
    let auth = authenticator();
    let assertion = auth.assert(&CHALLENGE);
    let mut credential = auth.credential(0);
    credential.public_key_p256 = [0xff; 64];
    assert_eq!(
        verify_webauthn(
            &credential,
            &assertion,
            &CHALLENGE,
            WebAuthnPolicy::default()
        ),
        Err(PasskeyError::Curve(P256Error::InvalidPublicKey))
    );
}

// ---------------------------------------------------------------------------
// Wycheproof: ECDSA-P256-SHA256, IEEE P1363 (r || s) encoding
// ---------------------------------------------------------------------------

#[test]
fn wycheproof_ecdsa_secp256r1_sha256_p1363_vectors() {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/wycheproof/upstream/ecdsa_secp256r1_sha256_p1363_test.json"
    ))
    .expect("vendored Wycheproof file");
    let fixture: serde_json::Value = serde_json::from_str(&raw).expect("json");
    assert_eq!(fixture["algorithm"].as_str(), Some("ECDSA"));
    assert_eq!(
        fixture["schema"].as_str(),
        Some("ecdsa_p1363_verify_schema_v1.json")
    );
    let declared = fixture["numberOfTests"].as_u64().expect("numberOfTests") as usize;

    let mut executed = 0usize;
    let mut valid = 0usize;
    let mut invalid = 0usize;
    let mut acceptable = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for group in fixture["testGroups"].as_array().expect("testGroups") {
        assert_eq!(group["publicKey"]["curve"].as_str(), Some("secp256r1"));
        assert_eq!(group["sha"].as_str(), Some("SHA-256"));
        let key_hex = group["publicKey"]["uncompressed"]
            .as_str()
            .expect("uncompressed");
        let key = P256PublicKey::from_sec1_bytes(&hex::decode(key_hex).unwrap());

        for test in group["tests"].as_array().expect("tests") {
            executed += 1;
            let tc_id = test["tcId"].as_u64().unwrap();
            let msg = hex::decode(test["msg"].as_str().unwrap()).unwrap();
            let sig = hex::decode(test["sig"].as_str().unwrap()).unwrap();
            let result = test["result"].as_str().unwrap();

            let verified = match (&key, P256Signature::from_fixed_bytes(&sig)) {
                (Ok(key), Ok(sig)) => verify_sha256(key, &msg, &sig),
                _ => false,
            };

            match result {
                "valid" => {
                    valid += 1;
                    if !verified {
                        failures.push(format!(
                            "tcId {tc_id} ({}) valid but refused",
                            test["comment"]
                        ));
                    }
                },
                "invalid" => {
                    invalid += 1;
                    if verified {
                        failures.push(format!(
                            "tcId {tc_id} ({}) invalid but accepted",
                            test["comment"]
                        ));
                    }
                },
                "acceptable" => acceptable += 1,
                other => panic!("unknown result {other} at tcId {tc_id}"),
            }
        }
    }

    assert_eq!(executed, declared, "every declared vector must execute");
    // The vendored file's exact composition, pinned so that re-vendoring a
    // different corpus is a visible, reviewable change rather than a silent
    // reduction in coverage. A harness that ran zero `invalid` vectors would
    // pass every other assertion here while proving nothing.
    assert_eq!(
        (valid, invalid, acceptable),
        (173, 89, 0),
        "vendored corpus composition changed: valid={valid} invalid={invalid} acceptable={acceptable}"
    );
    assert!(
        failures.is_empty(),
        "{} Wycheproof failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Address derivation and the authorization message
// ---------------------------------------------------------------------------

#[test]
fn passkey_addresses_are_domain_separated_and_rp_bound() {
    let auth = authenticator();
    let credential = auth.credential(0);
    let address = passkey_account_address(&credential);

    // A different relying party is a different account.
    let other_rp = SoftwareAuthenticator::new(&SECRET, "other.wrfcoin.test");
    assert_ne!(other_rp.account(), address);
    // A different key is a different account.
    let mut other_secret = SECRET;
    other_secret[31] ^= 1;
    assert_ne!(
        SoftwareAuthenticator::new(&other_secret, RP_ID).account(),
        address
    );
    // And no ed25519 address collides with it: ed25519 hashes the bare
    // 32-byte key; the passkey preimage is tagged and 129 bytes long.
    let mut rng = StdRng::seed_from_u64(2169);
    let ed = PrivateKey::generate(&mut rng);
    assert_ne!(
        Address::from_public_key(&PublicKey::from_private_key(&ed)),
        address
    );
    // Deterministic.
    assert_eq!(authenticator().account(), address);
}

#[test]
fn the_authorization_message_bytes_are_pinned() {
    // Golden: tag || len(chain) chain || len(net) net || len(from) from ||
    // len(to) to || amount le64 || nonce le64. A wallet SDK must reproduce
    // these bytes exactly to obtain the challenge a passkey signs over.
    const PINNED: &str = "777266636f696e5f706173736b65795f7472616e736665725f617574685f76310f000000777266636f696e2d746573746e657407000000746573746e65742a0000003078313131313131313131313131313131313131313131313131313131313131313131313131313131312a000000307832323232323232323232323232323232323232323232323232323232323232323232323232323232e8030000000000000500000000000000";
    let message = passkey_transfer_authorization_message_with_domain(
        "wrfcoin-testnet",
        "testnet",
        "0x1111111111111111111111111111111111111111",
        "0x2222222222222222222222222222222222222222",
        1000,
        5,
    );
    assert_eq!(hex::encode(&message), PINNED);
    assert!(message.starts_with(PASSKEY_TRANSFER_AUTH_DOMAIN));
    assert_ne!(
        PASSKEY_TRANSFER_AUTH_DOMAIN,
        TRANSPARENT_TRANSFER_AUTH_DOMAIN
    );
    // The challenge is the SHA3-256 of the message: 32 bytes, deterministic.
    assert_eq!(
        passkey_transfer_challenge(&message),
        passkey_transfer_challenge(&message)
    );
}

// ---------------------------------------------------------------------------
// Payload level: the ed25519 rule is untouched; the passkey rule is gated
// ---------------------------------------------------------------------------

fn passkey_payload(
    auth: &SoftwareAuthenticator,
    to: &Address,
    amount: u64,
    nonce: u64,
) -> TransparentTransferPayload {
    TransparentTransferPayload {
        from: auth.account().to_hex(),
        to: to.to_hex(),
        amount,
        transparent_opt_out: true,
        opt_out_audit_tag: None,
        auth_pubkey: None,
        auth_signature: None,
        authority: None,
        authority_signature: None,
        passkey_auth: Some(auth.authorize_transfer(&to.to_hex(), amount, nonce)),
        sponsorship: None,
        session_key_auth: None,
    }
}

#[test]
fn a_passkey_only_payload_is_cryptographically_valid_but_never_ed25519_authorized() {
    let auth = authenticator();
    let payload = passkey_payload(&auth, &Address([0x66; 20]), 1_000, 0);

    assert!(payload.has_passkey_authorization());
    assert!(payload.verify_passkey_authorization(0));
    // The ed25519 rule never reads `passkey_auth`.
    assert!(!payload.verify_authorization(0));
    // And with no schedule installed in this process, every gated form is the
    // ed25519 rule — the pre-activation contract.
    for height in [0, 1, 1_000, u64::MAX] {
        assert!(
            !payload.verify_authorization_at_height(0, height),
            "height {height}"
        );
    }
    assert!(!payload.verify_authorization_for_admission(0));
}

#[test]
fn the_credential_must_derive_from() {
    let auth = authenticator();
    let mut payload = passkey_payload(&auth, &Address([0x66; 20]), 1_000, 0);
    payload.from = Address([0x99; 20]).to_hex();
    assert!(!payload.verify_passkey_authorization(0));
    assert_eq!(
        payload
            .passkey_auth
            .as_ref()
            .unwrap()
            .verify_for_transfer(&payload.from, &payload.to, payload.amount, 0)
            .unwrap_err(),
        PasskeyAuthorizationError::FromNotBoundToCredential
    );
    payload.from = "wrf_faucet".to_string();
    assert_eq!(
        payload
            .passkey_auth
            .as_ref()
            .unwrap()
            .verify_for_transfer(&payload.from, &payload.to, payload.amount, 0)
            .unwrap_err(),
        PasskeyAuthorizationError::FromNotAnAddress
    );
}

#[test]
fn tampering_with_any_bound_field_breaks_the_challenge() {
    let auth = authenticator();
    let to = Address([0x66; 20]);
    let good = passkey_payload(&auth, &to, 1_000, 7);
    assert!(good.verify_passkey_authorization(7));

    let mut amount = good.clone();
    amount.amount = 1_001;
    assert!(!amount.verify_passkey_authorization(7));

    let mut recipient = good.clone();
    recipient.to = Address([0x67; 20]).to_hex();
    assert!(!recipient.verify_passkey_authorization(7));

    // Nonce is bound: replaying the same authorization at the next nonce fails.
    assert!(!good.verify_passkey_authorization(8));

    let err = good
        .passkey_auth
        .as_ref()
        .unwrap()
        .verify_for_transfer(&good.from, &good.to, good.amount, 8)
        .unwrap_err();
    assert_eq!(
        err,
        PasskeyAuthorizationError::Assertion(PasskeyError::ChallengeMismatch)
    );
}

#[test]
fn malformed_hex_fields_are_typed_and_fail_closed() {
    let auth = authenticator();
    let mut payload = passkey_payload(&auth, &Address([0x66; 20]), 1_000, 0);
    let pa = payload.passkey_auth.as_mut().unwrap();
    pa.public_key_p256 = "zz".to_string();
    assert_eq!(
        pa.verify_for_transfer(&payload.from, &payload.to, payload.amount, 0)
            .unwrap_err(),
        PasskeyAuthorizationError::MalformedField("public_key_p256")
    );
    assert!(!payload.verify_passkey_authorization(0));
}

#[test]
fn the_ed25519_path_is_byte_identical_with_or_without_a_passkey_member() {
    let mut rng = StdRng::seed_from_u64(1727);
    let sk = PrivateKey::generate(&mut rng);
    let from = Address::from_public_key(&PublicKey::from_private_key(&sk));
    let mut payload = TransparentTransferPayload {
        from: from.to_hex(),
        to: Address([0x66; 20]).to_hex(),
        amount: 500,
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
    payload.sign_authorization(&sk, 3);
    assert!(payload.verify_authorization(3));
    for height in [0, 4, 5, u64::MAX] {
        assert!(payload.verify_authorization_at_height(3, height));
    }
    assert!(payload.verify_authorization_for_admission(3));

    // Serialised without the member: exactly the pre-#2169 bytes (no
    // `passkey_auth` key at all).
    let json = serde_json::to_string(&payload).unwrap();
    assert!(!json.contains("passkey_auth"));

    // A pre-#2169 payload (no member) parses to `None`.
    let parsed: TransparentTransferPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, payload);
    assert!(parsed.passkey_auth.is_none());

    // A malformed member is read leniently as `None`, so an ed25519-authorised
    // payload that a pre-#2169 binary parses (ignoring the unknown member)
    // parses here too, and verifies identically.
    let mut with_junk: serde_json::Value = serde_json::from_str(&json).unwrap();
    with_junk["passkey_auth"] = serde_json::json!(42);
    let lenient: TransparentTransferPayload = serde_json::from_value(with_junk.clone()).unwrap();
    assert!(lenient.passkey_auth.is_none());
    assert!(lenient.verify_authorization(3));
    with_junk["passkey_auth"] = serde_json::json!({"public_key_p256": 1});
    let lenient: TransparentTransferPayload = serde_json::from_value(with_junk).unwrap();
    assert!(lenient.passkey_auth.is_none());

    // A well-formed passkey member round-trips.
    let auth = authenticator();
    let pk = passkey_payload(&auth, &Address([0x66; 20]), 1, 0);
    let round: TransparentTransferPayload =
        serde_json::from_str(&serde_json::to_string(&pk).unwrap()).unwrap();
    assert_eq!(round, pk);
    assert!(round.verify_passkey_authorization(0));
}

// ---------------------------------------------------------------------------
// The account-abstraction verifier hook
// ---------------------------------------------------------------------------

#[test]
fn the_signature_verifier_hook_verifies_a_serialised_assertion() {
    let auth = authenticator();
    let verifier = PasskeySignatureVerifier::default();
    let credential: PasskeyCredential = auth.credential(0);
    let mut signer = Vec::with_capacity(96);
    signer.extend_from_slice(&credential.rp_id_hash);
    signer.extend_from_slice(&credential.public_key_p256);
    let assertion = serde_json::to_vec(&auth.assert(&CHALLENGE)).unwrap();

    assert!(verifier.verify(&CHALLENGE, &assertion, &signer));
    assert!(!verifier.verify(&[0u8; 32], &assertion, &signer));
    assert!(!verifier.verify(&CHALLENGE, b"{}", &signer));
    assert!(!verifier.verify(&CHALLENGE, &assertion, &signer[1..]));
}
