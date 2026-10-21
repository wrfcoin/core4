// #2464 / #2746 — prove the identity seam is a config change, empirically.
//
// The attester PR asserts: test key today, `WRFCOIN_HSM_MODULE_PATH` +
// `WRFCOIN_HSM_ORACLE_KEY_LABEL` after the ceremony, no code change. Every
// other test signs with a raw ed25519 key; this file is the one that actually
// crosses the PKCS#11 boundary. It:
//
//   1. creates a THROWAWAY SoftHSM token in a private tokendir (never the
//      live `wrfcoin-oracle-*` tokens — the module is pointed at its own
//      `SOFTHSM2_CONF`, so the live volumes are unreachable by construction),
//   2. generates an in-token ed25519 keypair with the same custody attributes
//      the ceremony produced (`sensitive, always sensitive, never
//      extractable`) and proves CKA_VALUE cannot be read,
//   3. resolves the signer through the PRODUCTION path — `load_oracle_signer`
//      reading the exact env contract the deployment doc names — builds the
//      same `Attester` the service bin builds, and attests the golden
//      observation,
//   4. verifies the result through the same paths that accept a test-key
//      attestation today: the protocol verifier, the wire round trip, and the
//      node's registry check (`verify_signature_for_identity_role`, the exact
//      function `submit_oracle_attestation` gates on) — with a test-key
//      attestation run through the identical check as the parity control, and
//      a tampered-signature negative control,
//   5. pins the C_Sign framing empirically: SoftHSM returns raw 64-byte
//      `r||s`, the input `normalize_eddsa_signature` expects (its DER branch
//      stays defensive, not load-bearing).
//
// # When SoftHSM is absent, this test FAILS — it never silently passes
//
// The module path resolves from `WRFCOIN_HSM_TEST_MODULE`, then well-known
// install paths. If nothing is found the test panics, naming every probed
// path and the fix. A vacuous green here would be exactly the "absent thing
// rendering as healthy" failure this repo keeps finding; if SoftHSM is not
// available the honest outcomes are RED or a VISIBLE skip, never a plain
// green.
//
// Under default features this target still compiles — a whole-file
// `#![cfg(feature = "hsm")]` would blank it into `running 0 tests ... ok`,
// a green check with zero tests executed (the same silent-pass shape this
// file flags in `hsm_signer.rs`, and exactly what an earlier report
// mis-stated as "doesn't compile"). So the gating is per-module instead,
// and the featureless build keeps one permanently-`#[ignore]`d marker test:
// a default run reports `1 ignored`, making the missing coverage countable
// instead of dressed as success.
//
// # Running it on this workstation (no system SoftHSM)
//
//   HSMDIR=target/hsm-proof && mkdir -p $HSMDIR
//   CID=$(docker create wrfcoin/native-chain:testnet-local)
//   docker cp $CID:/usr/lib/x86_64-linux-gnu/softhsm/libsofthsm2.so $HSMDIR/
//   docker rm $CID
//   WRFCOIN_HSM_TEST_MODULE=$PWD/$HSMDIR/libsofthsm2.so \
//     cargo test -p wrfcoin-shared-protocol --features hsm \
//     --test hsm_attestation_seam_2464
//
// On a host with softhsm2 installed (e.g. `apt install softhsm2`), the env
// var is unnecessary — the standard paths are probed.

/// Default-features half: a marker that keeps the target's emptiness visible.
/// `1 ignored` is countable; `0 tests ok` is indistinguishable from coverage.
#[cfg(not(feature = "hsm"))]
#[test]
#[ignore = "PKCS#11 seam proof requires --features hsm (plus a SoftHSM module); without \
            the feature this target would otherwise report an empty green"]
fn hsm_seam_proof_not_compiled_without_the_hsm_feature() {
    unreachable!("marker only — never runs; it exists to be counted as ignored");
}

#[cfg(feature = "hsm")]
mod proof {

use std::path::PathBuf;

use cryptoki::context::{CInitializeArgs, Pkcs11};
use cryptoki::mechanism::Mechanism;
use cryptoki::object::{Attribute, AttributeType};
use cryptoki::session::UserType;
use cryptoki::types::AuthPin;

use wrfcoin_shared_protocol::blockchain::transaction::{
    OracleAttestationPayload, Transaction,
};
use wrfcoin_shared_protocol::crypto::signature::PublicKey;
use wrfcoin_shared_protocol::oracle::attester::{
    Attester, AttestationSubmission, ForbiddenRoleKeys, PendingObservation,
};
use wrfcoin_shared_protocol::oracle::mechanical::MechanicalPolicy;
use wrfcoin_shared_protocol::security::key_registry::{SigningKeyRegistry, SigningRole};

const TOKEN_LABEL: &str = "wrfcoin-hsm-proof";
const KEY_LABEL: &str = "oracle-proof-key";
const SO_PIN: &str = "010203";
const USER_PIN: &str = "040506";
/// The double-indirection PIN env contract: `WRFCOIN_HSM_PIN_ENV` names the
/// var that actually holds the PIN.
const PIN_HOLDER_VAR: &str = "WRFCOIN_HSM_PROOF_PIN";

/// The pinned cross-language golden vector from the attester test suite —
/// attesting the same observation a test-key run attests.
const GOLDEN_DATA: &str = r#"{"humidity_pct":50,"pressure_hpa":1016.916,"rainfall_mm":0,"temperature_c":21.11111111111111,"wind_mps":2.2352}"#;
const GOLDEN_NONCE: &str = "n1";
const GOLDEN_STATION: &str = "WX-DAVIS-7";
const GOLDEN_TIMESTAMP: i64 = 1_699_999_999;

/// Locate a SoftHSM PKCS#11 module or FAIL LOUDLY. No silent skip: an absent
/// module must never render as a green test.
fn locate_softhsm_module() -> PathBuf {
    // determinism-ok: test-only module discovery
    if let Ok(explicit) = std::env::var("WRFCOIN_HSM_TEST_MODULE") {
        let path = PathBuf::from(explicit.trim());
        assert!(
            path.is_file(),
            "WRFCOIN_HSM_TEST_MODULE is set to {path:?} but no file exists there. \
             Refusing to skip: fix the path or unset the variable to probe defaults."
        );
        return path;
    }
    let candidates = [
        "/usr/lib/softhsm/libsofthsm2.so",
        "/usr/lib/x86_64-linux-gnu/softhsm/libsofthsm2.so",
        "/usr/local/lib/softhsm/libsofthsm2.so",
        // The workstation convention: extracted from the node image into the
        // build tree (see the module docs for the exact commands).
        "target/hsm-proof/libsofthsm2.so",
        "../target/hsm-proof/libsofthsm2.so",
    ];
    for candidate in candidates {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return path.canonicalize().unwrap_or(path);
        }
    }
    panic!(
        "SoftHSM module not found — this test REFUSES to pass without it.\n\
         Probed:\n  - $WRFCOIN_HSM_TEST_MODULE (unset)\n  - {}\n\
         On this workstation SoftHSM lives only inside the node image; extract it:\n\
         CID=$(docker create wrfcoin/native-chain:testnet-local) && \
         docker cp $CID:/usr/lib/x86_64-linux-gnu/softhsm/libsofthsm2.so target/hsm-proof/ && \
         docker rm $CID\n\
         then re-run with WRFCOIN_HSM_TEST_MODULE=$PWD/target/hsm-proof/libsofthsm2.so",
        candidates.join("\n  - ")
    );
}

struct ThrowawayToken {
    dir: PathBuf,
}

impl Drop for ThrowawayToken {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Point SoftHSM at a private, throwaway token directory. This is the
/// isolation guarantee: whatever else this module could reach, its object
/// store is a fresh directory this test owns and deletes.
fn throwaway_softhsm_conf() -> ThrowawayToken {
    let dir = std::env::temp_dir().join(format!("wrfcoin-hsm-proof-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let tokens = dir.join("tokens");
    std::fs::create_dir_all(&tokens).expect("create throwaway token dir");
    let conf = dir.join("softhsm2.conf");
    std::fs::write(
        &conf,
        format!(
            "directories.tokendir = {}\nobjectstore.backend = file\nlog.level = ERROR\n",
            tokens.display()
        ),
    )
    .expect("write throwaway softhsm2.conf");
    // SAFETY: single-test binary; no concurrent test mutates the environment.
    unsafe { std::env::set_var("SOFTHSM2_CONF", &conf) };
    ThrowawayToken { dir }
}

/// Everything learned during in-token provisioning that the assertions need.
struct ProvisionedKey {
    public_key: PublicKey,
    raw_sign_len: usize,
    raw_sign_first_two: [u8; 2],
    sensitive: bool,
    always_sensitive: bool,
    never_extractable: bool,
    cka_value_readable: bool,
}

/// Initialize the token and generate the in-token keypair with the ceremony's
/// custody attributes, through a SETUP context that is finalized before the
/// production signer initializes its own (C_Initialize is once per library
/// per process).
fn provision_throwaway_key(module: &PathBuf) -> ProvisionedKey {
    let ctx = Pkcs11::new(module).expect("load SoftHSM module for setup");
    ctx.initialize(CInitializeArgs::OsThreads)
        .expect("C_Initialize (setup context)");

    let free_slot = *ctx
        .get_all_slots()
        .expect("C_GetSlotList")
        .first()
        .expect("SoftHSM presents at least one slot");
    let so_pin = AuthPin::new(SO_PIN.into());
    ctx.init_token(free_slot, &so_pin, TOKEN_LABEL)
        .expect("C_InitToken");

    // SoftHSM re-slots an initialized token; find it again by label.
    let slot = ctx
        .get_slots_with_token()
        .expect("C_GetSlotList after init")
        .into_iter()
        .find(|slot| {
            ctx.get_token_info(*slot)
                .map(|info| info.label().trim() == TOKEN_LABEL)
                .unwrap_or(false)
        })
        .expect("initialized token findable by label");

    let session = ctx.open_rw_session(slot).expect("open RW session");
    session
        .login(UserType::So, Some(&so_pin))
        .expect("SO login");
    session
        .init_pin(&AuthPin::new(USER_PIN.into()))
        .expect("C_InitPIN");
    session.logout().expect("SO logout");
    session
        .login(UserType::User, Some(&AuthPin::new(USER_PIN.into())))
        .expect("user login");

    // Ed25519 keypair, generated IN the token with the ceremony's custody
    // shape: sensitive, non-extractable private half.
    let ed25519_oid_der: Vec<u8> = vec![0x06, 0x03, 0x2b, 0x65, 0x70];
    let (public_handle, private_handle) = session
        .generate_key_pair(
            &Mechanism::EccEdwardsKeyPairGen,
            &[
                Attribute::Token(true),
                Attribute::Label(KEY_LABEL.as_bytes().to_vec()),
                Attribute::EcParams(ed25519_oid_der),
                Attribute::Verify(true),
            ],
            &[
                Attribute::Token(true),
                Attribute::Label(KEY_LABEL.as_bytes().to_vec()),
                Attribute::Sensitive(true),
                Attribute::Extractable(false),
                Attribute::Sign(true),
            ],
        )
        .expect("C_GenerateKeyPair (EccEdwardsKeyPairGen)");

    // Custody attributes as the token reports them — the same triplet the
    // live ceremony keys report.
    let attrs = session
        .get_attributes(
            private_handle,
            &[
                AttributeType::Sensitive,
                AttributeType::AlwaysSensitive,
                AttributeType::NeverExtractable,
            ],
        )
        .expect("read custody attributes");
    let mut sensitive = false;
    let mut always_sensitive = false;
    let mut never_extractable = false;
    for attr in attrs {
        match attr {
            Attribute::Sensitive(v) => sensitive = v,
            Attribute::AlwaysSensitive(v) => always_sensitive = v,
            Attribute::NeverExtractable(v) => never_extractable = v,
            _ => {},
        }
    }

    // The extractability probe: reading CKA_VALUE off the private key must
    // yield nothing. This is what a file-backed test key can never prove.
    let cka_value_readable = session
        .get_attributes(private_handle, &[AttributeType::Value])
        .map(|attrs| {
            attrs
                .iter()
                .any(|a| matches!(a, Attribute::Value(v) if !v.is_empty()))
        })
        .unwrap_or(false);

    // Pin the C_Sign framing empirically (finding target #1): raw r||s,
    // 64 bytes, no DER prefix.
    let raw = session
        .sign(&Mechanism::Eddsa, private_handle, b"framing-probe")
        .expect("C_Sign (Eddsa) framing probe");
    let raw_sign_len = raw.len();
    let raw_sign_first_two = [raw[0], raw[1]];

    // Public key straight from the token, for cross-checking what the
    // production signer later resolves.
    let ec_point = session
        .get_attributes(public_handle, &[AttributeType::EcPoint])
        .expect("read CKA_EC_POINT")
        .into_iter()
        .find_map(|a| match a {
            Attribute::EcPoint(bytes) => Some(bytes),
            _ => None,
        })
        .expect("public key exposes CKA_EC_POINT");
    let raw_point = match ec_point.as_slice() {
        [0x04, 0x20, rest @ ..] if rest.len() == 32 => rest,
        other => other,
    };
    let public_key = PublicKey::from_bytes(raw_point).expect("token public key decodes");

    drop(session);
    // Release the library so the production signer's C_Initialize starts
    // clean. `finalize` consumes the context.
    ctx.finalize();

    ProvisionedKey {
        public_key,
        raw_sign_len,
        raw_sign_first_two,
        sensitive,
        always_sensitive,
        never_extractable,
        cka_value_readable,
    }
}

fn golden_observation() -> PendingObservation {
    PendingObservation {
        commit_id: Transaction::compute_commit_id(
            &serde_json::from_str(GOLDEN_DATA).expect("golden data parses"),
            GOLDEN_NONCE,
            GOLDEN_STATION,
            GOLDEN_TIMESTAMP,
            "contribution",
        ),
        station_id: GOLDEN_STATION.to_string(),
        nonce: GOLDEN_NONCE.to_string(),
        timestamp: GOLDEN_TIMESTAMP,
        data: serde_json::from_str(GOLDEN_DATA).expect("golden data parses"),
        contribution_type: "contribution".to_string(),
    }
}

/// The node's verification gate, verbatim: registered identity at an epoch,
/// then `verify_signature_for_identity_role` — the function
/// `submit_oracle_attestation`, mempool admission, and apply all gate on.
fn node_registry_accepts(payload: &OracleAttestationPayload) -> bool {
    let mut registry = SigningKeyRegistry::default();
    registry
        .register_key_epoch(
            &payload.oracle_id,
            SigningRole::Oracle,
            2, // the ceremony registered service keys at epoch 2
            payload.oracle_id.clone(),
            0,
        )
        .expect("register epoch-2 oracle key");
    registry.verify_signature_for_identity_role(
        &payload.oracle_id,
        SigningRole::Oracle,
        payload.timestamp,
        payload.signing_message().as_bytes(),
        payload.signature.as_deref().unwrap_or(""),
    )
}

#[test]
fn a_pkcs11_signed_attestation_passes_the_same_verification_as_a_test_key() {
    let module = locate_softhsm_module();
    let _token = throwaway_softhsm_conf();
    let provisioned = provision_throwaway_key(&module);

    // ---- custody shape (finding target #2) --------------------------------
    assert!(
        provisioned.sensitive && provisioned.always_sensitive && provisioned.never_extractable,
        "throwaway key must mirror the ceremony's custody attributes \
         (sensitive={}, always_sensitive={}, never_extractable={})",
        provisioned.sensitive,
        provisioned.always_sensitive,
        provisioned.never_extractable
    );
    assert!(
        !provisioned.cka_value_readable,
        "CKA_VALUE of the private key was READABLE — the token is not enforcing \
         sensitivity, and this proof would be vacuous"
    );

    // ---- C_Sign framing (finding target #1) -------------------------------
    assert_eq!(
        provisioned.raw_sign_len, 64,
        "SoftHSM C_Sign(Eddsa) returned {} bytes, not the raw 64-byte r||s \
         this codebase's normalize_eddsa_signature expects as its primary form",
        provisioned.raw_sign_len
    );
    assert_ne!(
        provisioned.raw_sign_first_two,
        [0x04, 0x40],
        "signature arrived DER-wrapped — normalize_eddsa_signature would still \
         accept it, but the raw-64 claim in its docs would be wrong for SoftHSM"
    );

    // ---- the seam: production resolution from env only --------------------
    // Exactly the deployment doc's contract. No signer code is touched.
    // SAFETY: single-test binary; nothing else reads these vars concurrently.
    unsafe {
        std::env::remove_var("WRFCOIN_ORACLE_SIGNER_PRIVATE_KEY");
        std::env::remove_var("WRFCOIN_ORACLE_SIGNER_KEY_FILE");
        std::env::set_var("WRFCOIN_HSM_MODULE_PATH", &module);
        std::env::set_var("WRFCOIN_HSM_TOKEN_LABEL", TOKEN_LABEL);
        std::env::set_var("WRFCOIN_HSM_ORACLE_KEY_LABEL", KEY_LABEL);
        std::env::set_var("WRFCOIN_HSM_PIN_ENV", PIN_HOLDER_VAR);
        std::env::set_var(PIN_HOLDER_VAR, USER_PIN);
    }

    // The same construction the oracle_attester daemon performs, custody
    // inventory included.
    let forbidden = ForbiddenRoleKeys::default()
        .with_key("snapshot", throwaway_pubkey(11))
        .with_key("producer", throwaway_pubkey(13))
        .with_key("governance", throwaway_pubkey(17));
    let attester = Attester::from_env(&forbidden)
        .expect("PKCS#11 oracle signer must resolve from env alone")
        .expect("an oracle signer is configured");

    assert_eq!(
        attester.oracle_id(),
        provisioned.public_key.to_hex(),
        "the production loader resolved a different public key than the token \
         generated — the CKA_EC_POINT path is broken"
    );

    // ---- attest the golden observation through the full pipeline ----------
    let observation = golden_observation();
    let now = GOLDEN_TIMESTAMP + 60;
    let verdict = MechanicalPolicy::default().evaluate(&observation, None, now);
    assert!(verdict.approved, "golden observation must approve: {:?}", verdict.reasons);

    let payload = attester
        .attest(&observation, verdict.approved, Some(verdict.confidence), now, "hsm-proof-1")
        .expect("attest() through PKCS#11 — includes the protocol self-verification");

    // ---- the same verification paths a test-key attestation passes --------
    assert!(payload.has_valid_signature(), "protocol verifier");

    let wire = AttestationSubmission::from(&payload).to_json();
    let parsed: OracleAttestationPayload =
        serde_json::from_str(&wire).expect("wire body parses");
    assert!(
        parsed.has_valid_signature(),
        "signature must survive the wire round trip"
    );
    assert!(
        node_registry_accepts(&parsed),
        "the node's registry gate (verify_signature_for_identity_role, epoch 2) \
         must accept the PKCS#11 attestation"
    );

    // Parity control: a TEST-KEY attestation through the IDENTICAL gate. This
    // is what makes "same verification path" a demonstrated fact rather than
    // a claim. Debug builds only — EnvKeySigner does not exist in release.
    #[cfg(debug_assertions)]
    {
        use std::sync::Arc;
        use wrfcoin_shared_protocol::crypto::signature::PrivateKey;
        use wrfcoin_shared_protocol::crypto::signer::EnvKeySigner;
        let sk = PrivateKey::from_hex(
            "4242424242424242424242424242424242424242424242424242424242424242",  // pragma: allowlist secret -- fixed test-only key, same as the attester unit suite
        )
        .expect("fixed test key");
        let test_attester = Attester::new(Arc::new(EnvKeySigner::from_private_key(
            sk,
            "WRFCOIN_ORACLE_SIGNER_PRIVATE_KEY",
        )));
        let test_payload = test_attester
            .attest(&observation, true, Some(verdict.confidence), now, "hsm-proof-tk")
            .expect("test-key attestation");
        assert!(
            node_registry_accepts(&test_payload),
            "parity control: the identical gate must accept the test-key attestation"
        );
    }

    // Negative control: the gate must have teeth.
    let mut tampered = parsed.clone();
    let mut sig = tampered.signature.clone().expect("signature present");
    // Flip one hex nibble.
    let flipped = if sig.as_bytes()[0] == b'0' { "1" } else { "0" };
    sig.replace_range(0..1, flipped);
    tampered.signature = Some(sig);
    assert!(
        !node_registry_accepts(&tampered),
        "a tampered PKCS#11 signature must be refused"
    );

    // ---- env hygiene ------------------------------------------------------
    // SAFETY: as above.
    unsafe {
        std::env::remove_var("WRFCOIN_HSM_MODULE_PATH");
        std::env::remove_var("WRFCOIN_HSM_TOKEN_LABEL");
        std::env::remove_var("WRFCOIN_HSM_ORACLE_KEY_LABEL");
        std::env::remove_var("WRFCOIN_HSM_PIN_ENV");
        std::env::remove_var(PIN_HOLDER_VAR);
        std::env::remove_var("SOFTHSM2_CONF");
    }
}

fn throwaway_pubkey(seed: u64) -> PublicKey {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let sk = wrfcoin_shared_protocol::crypto::signature::PrivateKey::generate(&mut rng);
    PublicKey::from_private_key(&sk)
}

} // mod proof
