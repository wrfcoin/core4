//! #2138 — every internal error variant maps to a public shape that leaks nothing.
//!
//! For each `ErrorKind`, an `InternalError` whose detail carries every class of
//! thing a dependency might put in an error string — filesystem paths, crate and
//! dependency versions, host names and addresses, store/SQL fragments, and the
//! secret-name segments `scripts/check_secret_leak.py` uses — is converted with
//! `to_public` and serialised the way the API envelope serialises it. The scanner
//! must find nothing in the public shape, must find every class in the detail
//! (so the scanner is not vacuous), and the server-side record must still carry
//! the detail under the same correlation id.

use uuid::Uuid;
use wrfcoin_shared_protocol::security::error_redaction::{
    ErrorKind, InternalError, Leak, PublicError, redaction_findings, to_public, to_public_with_id,
};

/// The nastiest detail a real failure could plausibly produce.
const LEAKY_DETAIL: &str = "storage write failed: redb table `blocks` at \
    /home/example/wrfcoin/core4/testnet/wrfcoin.redb (wrfcoin-native-chain 0.1.0, redb-2.1.3) \
    on host gh-runner.local 192.0.2.10:8556 — INSERT INTO blocks WHERE height=1 — \
    private_key=deadbeef signingKey=… WRFCOIN_OPERATOR_TOKEN api_token seed phrase \
    Caused by: native-chain/src/storage.rs:1234 io error";

/// The shape the launch_api envelope gives the error object (code string, static
/// message, correlation id); the numeric `PublicError.code` travels alongside.
fn envelope_json(public: &PublicError, kind: ErrorKind) -> String {
    serde_json::json!({
        "ok": false,
        "data": null,
        "error": {
            "code": kind.envelope_code(),
            "public_code": public.code,
            "message": public.message,
            "correlation_id": public.correlation_id.to_string(),
        },
        "meta": { "request_id": "req_test", "timestamp": "2026-09-11T00:00:00Z", "api_version": "v1" }
    })
    .to_string()
}

/// Compile-time proof that a public message can only ever be a `&'static str`.
fn takes_static(_: &'static str) {}

#[test]
fn the_scanner_finds_every_leak_class_in_the_detail() {
    let found = redaction_findings(LEAKY_DETAIL);
    let has = |f: fn(&Leak) -> bool| found.iter().any(f);
    assert!(has(|l| matches!(l, Leak::FilesystemPath(_))), "{found:?}");
    assert!(has(|l| matches!(l, Leak::Version(_))), "{found:?}");
    assert!(has(|l| matches!(l, Leak::HostOrAddress(_))), "{found:?}");
    assert!(has(|l| matches!(l, Leak::StoreFragment(_))), "{found:?}");
    assert!(has(|l| matches!(l, Leak::SecretName(_))), "{found:?}");
}

#[test]
fn every_kind_redacts_to_a_clean_static_shape_and_keeps_detail_server_side() {
    let mut covered = 0;
    for kind in ErrorKind::ALL {
        let internal = InternalError::new(kind, LEAKY_DETAIL);
        let redacted = to_public(&internal);

        // Public half: class only.
        takes_static(redacted.public.message);
        assert_eq!(redacted.public.code, kind.code());
        assert_eq!(redacted.public.message, kind.public_message());
        let json = envelope_json(&redacted.public, kind);
        let findings = redaction_findings(&json);
        assert!(findings.is_empty(), "{kind:?}: public shape leaks {findings:?}\n{json}");
        let raw = serde_json::to_string(&redacted.public).unwrap();
        assert!(redaction_findings(&raw).is_empty(), "{kind:?}: {raw}");
        for fragment in ["redb", "/home/", "gh-runner", "0.1.0", "INSERT", "private_key", "storage.rs"] {
            assert!(!json.contains(fragment), "{kind:?}: {fragment} reached the client");
        }

        // Server half: everything, under the same id.
        assert_eq!(redacted.record.correlation_id, redacted.public.correlation_id);
        assert_eq!(redacted.record.kind, kind);
        assert_eq!(redacted.record.detail, LEAKY_DETAIL);
        let line = redacted.record.to_string();
        assert!(line.contains(&redacted.public.correlation_id.to_string()));
        assert!(line.contains("redb") && line.contains("/home/example"));
        covered += 1;
    }
    assert_eq!(covered, ErrorKind::ALL.len());
    assert_eq!(ErrorKind::ALL.len(), 13, "a new ErrorKind must be added to ALL so this test covers it");
}

#[test]
fn correlation_ids_are_fresh_per_conversion_and_pinnable() {
    let e = InternalError::new(ErrorKind::Internal, "x");
    let a = to_public(&e);
    let b = to_public(&e);
    assert_ne!(a.public.correlation_id, b.public.correlation_id);
    let fixed = Uuid::from_u128(0x1234);
    let c = to_public_with_id(&e, fixed);
    assert_eq!(c.public.correlation_id, fixed);
    assert_eq!(c.record.correlation_id, fixed);
}

#[test]
fn envelope_codes_and_messages_carry_no_secret_names_or_fragments() {
    for kind in ErrorKind::ALL {
        assert!(redaction_findings(kind.envelope_code()).is_empty(), "{kind:?}");
        assert!(redaction_findings(kind.public_message()).is_empty(), "{kind:?}");
        assert!(!kind.public_message().is_empty());
        assert!(!kind.public_message().contains("{}"), "no interpolation slot");
    }
}

#[test]
fn wrapping_a_foreign_error_keeps_its_text_only_in_the_detail() {
    let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "open /var/lib/wrfcoin/wal: permission denied");
    let internal = InternalError::wrap(ErrorKind::Storage, &io);
    assert!(internal.detail.contains("/var/lib/wrfcoin/wal"));
    let redacted = to_public(&internal);
    let raw = serde_json::to_string(&redacted.public).unwrap();
    assert!(!raw.contains("/var/lib"));
    assert!(redaction_findings(&raw).is_empty());
}
