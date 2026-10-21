//! Runtime pass: a secret never appears in any output path (#2129).
//!
//! # Why a runtime pass is needed at all
//!
//! `scripts/check_secret_leak.py` is the static half — it catches a secret-named
//! identifier being handed to a log macro, a metric label, a `panic!`, or a
//! `format!`. But a static scan cannot see through a `Display` impl that
//! *derives* its output from a secret field, or a `Serialize` that happens to
//! include one. Those are invisible to grep and obvious to a sentinel.
//!
//! So each secret-bearing type is constructed with a recognisable sentinel
//! value, every output path is exercised, and the captured text is asserted not
//! to contain the sentinel.
//!
//! # Paths covered
//!
//! `Debug`, `Display`, `Serialize`, error rendering, and a forced panic payload.
//!
//! `KeyHandle` / `KeyRegistry` (#2128, PR #2238) are the other secret-bearing
//! types, but they do not exist on `main` yet and the epic contract forbids
//! branching from an unmerged sibling. They implement no `Debug`/`Display` at
//! all, so the compiler already rules out the paths this file checks; add the
//! sentinel coverage here once #2238 lands.
//!
//! # The negative control
//!
//! [`a_deliberately_leaking_type_fails_the_check`] defines a type that *does*
//! leak and asserts the harness catches it. Without that, this whole file would
//! pass just as happily if `sentinel_appears_in` were broken — a leak detector
//! that cannot detect a leak is worse than none, because it is believed.

use std::fmt;
use std::panic;

use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

/// A byte pattern that will not occur by accident.
const SENTINEL_BYTES: &[u8] = b"WRFCOIN-SENTINEL-9f3a7c21-DO-NOT-LEAK";
/// The hex rendering, since key material is usually hex-encoded on the way out.
fn sentinel_hex() -> String {
    hex::encode(SENTINEL_BYTES)
}

fn contains_sentinel(haystack: &str) -> bool {
    let plain = String::from_utf8_lossy(SENTINEL_BYTES).to_string();
    haystack.contains(&plain) || haystack.contains(&sentinel_hex())
}

/// Capture a panic's payload as a string.
fn panic_payload_of<F: FnOnce() + panic::UnwindSafe>(f: F) -> String {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(|_| {})); // silence the default stderr print
    let result = panic::catch_unwind(f);
    panic::set_hook(previous);

    match result {
        Ok(()) => String::new(),
        Err(payload) => {
            if let Some(s) = payload.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                String::new()
            }
        },
    }
}

// ---------------------------------------------------------------------------
// The negative control — prove the harness detects a leak
// ---------------------------------------------------------------------------

/// A type that deliberately leaks its secret through `Display`.
struct LeakyType {
    secret: Vec<u8>,
}

impl fmt::Display for LeakyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Exactly the mistake this story exists to prevent.
        write!(f, "key={}", String::from_utf8_lossy(&self.secret))
    }
}

#[test]
fn a_deliberately_leaking_type_fails_the_check() {
    // Acceptance criterion: "a deliberately leaking type fails the test".
    // If this assertion ever fails, every other test in this file is vacuous.
    let leaky = LeakyType {
        secret: SENTINEL_BYTES.to_vec(),
    };
    assert!(
        contains_sentinel(&leaky.to_string()),
        "the leak detector failed to detect a deliberate leak — the rest of \
         this file proves nothing"
    );
}

// ---------------------------------------------------------------------------
// PrivateKey — Debug, Display, Serialize, and a forced panic
// ---------------------------------------------------------------------------

/// A private key whose bytes are the sentinel (padded/truncated to 32).
fn sentinel_private_key() -> PrivateKey {
    let mut bytes = [0u8; 32];
    let n = SENTINEL_BYTES.len().min(32);
    bytes[..n].copy_from_slice(&SENTINEL_BYTES[..n]);
    PrivateKey::from_bytes(&bytes).expect("32 bytes is a valid ed25519 secret")
}

/// The sentinel as it would appear inside a 32-byte key (truncated).
fn truncated_sentinel() -> String {
    let n = SENTINEL_BYTES.len().min(32);
    String::from_utf8_lossy(&SENTINEL_BYTES[..n]).to_string()
}

fn leaks_truncated_sentinel(haystack: &str) -> bool {
    let plain = truncated_sentinel();
    let mut padded = [0u8; 32];
    let n = SENTINEL_BYTES.len().min(32);
    padded[..n].copy_from_slice(&SENTINEL_BYTES[..n]);
    haystack.contains(&plain) || haystack.contains(&hex::encode(padded))
}

#[test]
fn private_key_debug_does_not_leak() {
    let sk = sentinel_private_key();
    let rendered = format!("{sk:?}");
    assert!(
        !leaks_truncated_sentinel(&rendered),
        "PrivateKey Debug leaked material: {rendered}"
    );
    assert!(rendered.contains("REDACTED"), "expected a redacted Debug");
}

#[test]
fn private_key_display_does_not_leak() {
    let sk = sentinel_private_key();
    let rendered = format!("{sk}");
    assert!(
        !leaks_truncated_sentinel(&rendered),
        "PrivateKey Display leaked material: {rendered}"
    );
}

#[test]
fn private_key_panic_payload_does_not_leak() {
    // Crash reports are an output path. A panic constructed near a secret must
    // not carry it into the payload.
    let sk = sentinel_private_key();
    let payload = panic_payload_of(move || {
        panic!("signing failed for key {sk:?}");
    });
    assert!(!payload.is_empty(), "the panic must actually have fired");
    assert!(
        !leaks_truncated_sentinel(&payload),
        "panic payload leaked key material: {payload}"
    );
}

#[test]
fn public_key_rendering_is_safe_and_derived_key_is_not_the_secret() {
    // The public key is not secret, but confirm deriving it does not surface
    // the private bytes.
    let sk = sentinel_private_key();
    let pk = PublicKey::from_private_key(&sk);
    let rendered = format!("{pk}|{pk:?}|{}", pk.to_hex());
    assert!(
        !leaks_truncated_sentinel(&rendered),
        "public key rendering exposed private material: {rendered}"
    );
}

#[test]
fn private_key_serialization_is_not_derivable_from_public_serde() {
    // `PublicKey` serializes to hex. Confirm the serialized form of the public
    // half never contains the private half.
    let sk = sentinel_private_key();
    let pk = PublicKey::from_private_key(&sk);
    let json = serde_json::to_string(&pk).expect("public key serializes");
    assert!(
        !leaks_truncated_sentinel(&json),
        "serialized public key leaked private material: {json}"
    );
}

// ---------------------------------------------------------------------------
// Error types — the surface the first version claimed but did not test
// ---------------------------------------------------------------------------

#[test]
fn signature_parse_errors_do_not_echo_the_input() {
    // Error paths are constructed next to the secret, which is what makes them
    // a leak surface. A parser that echoes its input into the error message
    // leaks whatever was being parsed.
    let sentinel_hexish = hex::encode(SENTINEL_BYTES);
    let rendered = match PrivateKey::from_hex(&sentinel_hexish) {
        Err(e) => format!("{e}|{e:?}"),
        Ok(_) => String::new(),
    };
    assert!(
        !contains_sentinel(&rendered),
        "a key-parse error echoed its input: {rendered}"
    );
}

#[test]
fn public_key_parse_error_does_not_echo_the_input() {
    let bogus = hex::encode(SENTINEL_BYTES);
    let rendered = match PublicKey::from_hex(&bogus) {
        Err(e) => format!("{e}|{e:?}"),
        Ok(_) => String::new(),
    };
    assert!(!contains_sentinel(&rendered), "leaked: {rendered}");
}

#[test]
fn signature_parse_error_does_not_echo_the_input() {
    let bogus = hex::encode(SENTINEL_BYTES);
    let rendered = match wrfcoin_shared_protocol::crypto::signature::Signature::from_hex(&bogus) {
        Err(e) => format!("{e}|{e:?}"),
        Ok(_) => String::new(),
    };
    assert!(!contains_sentinel(&rendered), "leaked: {rendered}");
}

// ---------------------------------------------------------------------------
// Every secret-bearing type
// ---------------------------------------------------------------------------

#[test]
fn keypair_debug_does_not_leak_the_private_half() {
    use wrfcoin_shared_protocol::crypto::signature::KeyPair;
    let sk = sentinel_private_key();
    let pk = PublicKey::from_private_key(&sk);
    let pair = KeyPair {
        private_key: sk,
        public_key: pk,
    };
    let rendered = format!("{pair:?}");
    assert!(
        !leaks_truncated_sentinel(&rendered),
        "KeyPair Debug leaked the private half: {rendered}"
    );
}

/// `StealthKeys` must not leak private material through `Debug` — and must
/// not offer `Serialize`/`Clone` at all.
///
/// The pre-#2266 version of this test was `#[ignore]`d and *called*
/// `serde_json::to_string(&keys)`, with a companion test pinning the broken
/// behaviour. #2266 removes the `Serialize` and `Clone` derives entirely, so
/// the leak is now impossible to write rather than merely absent — and the
/// old tests stopped compiling, which is the derive-removal working as
/// intended. The compile-time assertions below turn that absence into a
/// regression gate: re-adding either derive makes this file fail to build.
/// (This test therefore requires #2266 to be merged; the two PRs are
/// order-coupled by design.)
#[test]
fn stealth_keys_do_not_leak_private_material() {
    use wrfcoin_privacy::StealthKeys;

    // Compile-time: StealthKeys implements neither Serialize nor Clone.
    // The trick: the blanket impl over () always applies; a second impl
    // over u8 applies only when the probed trait is implemented, making
    // the call ambiguous — so this compiles ONLY while the trait is absent.
    #[allow(dead_code)]
    const _NO_SERIALIZE: fn() = || {
        trait AmbiguousIfImpl<A> {
            fn probe() {}
        }
        impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
        impl<T: ?Sized + serde::Serialize> AmbiguousIfImpl<u8> for T {}
        let _ = <StealthKeys as AmbiguousIfImpl<_>>::probe;
    };
    #[allow(dead_code)]
    const _NO_CLONE: fn() = || {
        trait AmbiguousIfImpl<A> {
            fn probe() {}
        }
        impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
        impl<T: ?Sized + Clone> AmbiguousIfImpl<u8> for T {}
        let _ = <StealthKeys as AmbiguousIfImpl<_>>::probe;
    };

    // Runtime: Debug renders with both private keys redacted. serde renders
    // `[u8; 32]` as a number array and Debug used to render it the same way,
    // so match the byte sequence, not a hex encoding.
    let keys = StealthKeys::generate();
    let debug_rendered = format!("{keys:?}");
    let as_array = |bytes: &[u8; 32]| {
        bytes
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    let priv_spend = as_array(&keys.private_spend_key);
    let priv_view = as_array(&keys.private_view_key);

    assert!(
        !debug_rendered.contains(&priv_spend) && !debug_rendered.contains(&priv_view),
        "StealthKeys Debug exposes private spend/view keys (#2259)"
    );
    assert!(
        debug_rendered.contains("redacted"),
        "StealthKeys Debug no longer marks redaction — did the impl change?"
    );
}

#[test]
fn sentinel_helpers_are_self_consistent() {
    // Guards the guards: if `contains_sentinel` stopped matching, every test
    // above would pass trivially.
    assert!(contains_sentinel(&String::from_utf8_lossy(SENTINEL_BYTES)));
    assert!(contains_sentinel(&sentinel_hex()));
    assert!(!contains_sentinel("nothing to see here"));
    assert!(leaks_truncated_sentinel(&truncated_sentinel()));
}

/// #2129 / #2344: `BaseConnector.credential` redaction must be ONE-WAY.
///
/// The first attempt used `#[serde(skip)]`, which also suppresses
/// deserialization: credential-bearing input silently loaded as
/// `credential = None`, breaking every authenticated connector after
/// deserialization (proven in the #2344 cross-provider review). The fix is
/// `#[serde(skip_serializing)]`, and this module pins BOTH directions so
/// neither regression can come back:
///
/// * serializing a connector must NOT emit the credential (the #2129 leak);
/// * deserializing credential-bearing input must PRESERVE it as `Some(..)`
///   (the #2344 regression) — reverting the attribute to `skip` fails this
///   arm, deleting the attribute fails the other, so each direction is the
///   negative control for its twin.
#[cfg(feature = "data-hub")]
mod base_connector_credential {
    use super::{SENTINEL_BYTES, contains_sentinel};
    use wrfcoin_shared_protocol::data_hub::sources::BaseConnector;
    use wrfcoin_shared_protocol::data_hub::{DataSource, DataSourceCredential};

    fn sentinel_string() -> String {
        String::from_utf8_lossy(SENTINEL_BYTES).to_string()
    }

    fn test_source() -> DataSource {
        DataSource {
            id: "test-source".to_string(),
            name: "Test Source".to_string(),
            description: "connector redaction regression fixture".to_string(),
            categories: vec![],
            endpoint: "https://example.invalid".to_string(),
            formats: vec![],
            requires_auth: true,
            auth_method: Some("api_key".to_string()),
            added_at: chrono::Utc::now(),
            last_connected: None,
            avg_response_time_ms: None,
            rate_limit: None,
            reliability_score: 100,
            available_schemas: vec![],
            metadata: std::collections::HashMap::new(),
        }
    }

    fn sentinel_credential() -> DataSourceCredential {
        DataSourceCredential {
            source_id: "test-source".to_string(),
            api_key: sentinel_string(),
            parameters: std::collections::HashMap::new(),
            expires_at: None,
            owner_id: "test-owner".to_string(),
        }
    }

    #[test]
    fn credential_redaction_is_one_way() {
        // Direction 1 — OUTPUT REDACTS: a serialized connector never contains
        // the credential (#2129).
        let connector = BaseConnector::new(test_source(), Some(sentinel_credential()));
        let serialized = serde_json::to_string(&connector).expect("BaseConnector must serialize");
        assert!(
            !contains_sentinel(&serialized),
            "serialized BaseConnector leaked the credential sentinel (#2129): {serialized}"
        );
        let as_value: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized form parses");
        assert!(
            as_value.get("credential").is_none(),
            "serialized BaseConnector emitted a credential field at all: {serialized}"
        );

        // Direction 2 — INPUT PRESERVES: deserializing a connector that carries
        // a credential must keep it (#2344 review finding: `#[serde(skip)]`
        // silently dropped it to None).
        let mut input = serde_json::to_value(&connector).expect("value form");
        input.as_object_mut().expect("object").insert(
            "credential".to_string(),
            serde_json::to_value(sentinel_credential()).expect("credential value"),
        );
        let restored: BaseConnector =
            serde_json::from_value(input).expect("credential-bearing input must deserialize");
        let credential = restored
            .credential
            .expect("deserialization dropped the credential to None (#2344 regression)");
        assert_eq!(
            credential.api_key,
            sentinel_string(),
            "credential survived deserialization but was altered"
        );
    }

    #[test]
    fn credential_absent_input_still_deserializes() {
        // `skip_serializing` means the connector's own serialized form has no
        // credential field; that form must remain valid input (Option => None),
        // or every redacted dump becomes unreadable.
        let connector = BaseConnector::new(test_source(), Some(sentinel_credential()));
        let serialized = serde_json::to_string(&connector).expect("serialize");
        let restored: BaseConnector =
            serde_json::from_str(&serialized).expect("redacted form must stay deserializable");
        assert!(
            restored.credential.is_none(),
            "credential-less input should load as None"
        );
    }
}
