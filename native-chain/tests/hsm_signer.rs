//! Integration test for the PKCS#11-backed `BlockSigner` path.
//!
//! This test runs a real signing round-trip against a local `softhsm2`
//! token. It is disabled by default because CI (and most dev machines)
//! have no PKCS#11 module installed. To run it locally:
//!
//! ```bash
//! # one-time softhsm2 provisioning — see docs/operator/hsm.md
//! WRFCOIN_HSM_MODULE_PATH=/usr/lib/softhsm/libsofthsm2.so \
//! WRFCOIN_HSM_TOKEN_LABEL=wrfcoin-validator \
//! WRFCOIN_HSM_KEY_LABEL=wrfcoin-signing-key \
//! WRFCOIN_HSM_PIN_ENV=WRFCOIN_HSM_PIN \
//! WRFCOIN_HSM_PIN=1234 \
//! cargo test -p wrfcoin-native-chain --features hsm --test hsm_signer -- --ignored
//! ```
//!
//! #2748: these tests are `#[ignore]`d instead of runtime-gated. The old
//! `HSM_TEST_ENABLED` early return reported PASSED on an unprovisioned
//! machine — a skipped HSM test indistinguishable from a verified one, with
//! the "skipping:" line invisible without `--nocapture`. Now an unprovisioned
//! run reports `2 ignored` (a countable absence), and an explicitly requested
//! run (`-- --ignored`, what CI does after provisioning the token) fails
//! loudly if the WRFCOIN_HSM_* environment is incomplete rather than
//! pretending success.

// An empty test binary reports "0 passed; 0 failed; 0 ignored" — identical
// to a passing target and invisible behind sibling suites (the vacuity shape
// scripts/cargo_test_nonvacuous.py rejects as EMPTY_TEST_BINARY). Without the
// feature this target now declares its absence as a countable `ignored`
// instead of compiling to silence — #2748's principle, extended to the cfg
// dimension.
#[cfg(not(feature = "hsm"))]
#[test]
#[ignore = "this target's tests require --features hsm; without it the binary would be empty and indistinguishable from a passing one. Run: cargo test -p wrfcoin-native-chain --features hsm --test hsm_signer -- --ignored"]
fn tests_require_the_hsm_feature() {}

#[cfg(feature = "hsm")]
mod gated {
    use wrfcoin_shared_protocol::crypto::signer::{
        BlockSigner, Pkcs11Config, Pkcs11Signer, SignerProvenance,
    };

    #[test]
    #[ignore = "needs a provisioned softhsm2 token (docs/operator/hsm.md); run with -- --ignored"]
    fn pkcs11_signer_signs_and_verifies_block_header() {
        let config = Pkcs11Config::from_env()
            .expect("WRFCOIN_HSM_* env vars must all be set to run the HSM signer tests");
        let signer = Pkcs11Signer::new(config).expect("Pkcs11Signer::new succeeds against softhsm2");

        // A realistic payload: a 32-byte blake3-like block header hash.
        let header_hash: [u8; 32] = [
            0x42, 0x13, 0x37, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
            0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16,
            0x17, 0x18,
        ];

        let sig = signer.sign(&header_hash).expect("sign succeeds");
        let pk = signer.public_key();
        assert!(
            pk.verify(&header_hash, &sig),
            "PKCS#11-produced signature must verify against the token's advertised public key"
        );

        // Provenance must identify the PKCS#11 token without leaking key material.
        match signer.provenance() {
            SignerProvenance::Pkcs11 {
                module_path,
                token_label,
                key_label,
            } => {
                assert!(!module_path.is_empty());
                assert!(!token_label.is_empty());
                assert!(!key_label.is_empty());
            }
            other => panic!("expected Pkcs11 provenance, got {other:?}"),
        }

        // A second sign call on the same payload must produce a byte-identical
        // ed25519 signature — ed25519 is deterministic. This catches tokens that
        // accidentally emit randomised (X)EdDSA variants.
        let sig2 = signer.sign(&header_hash).expect("second sign succeeds");
        assert_eq!(
            sig.as_bytes(),
            sig2.as_bytes(),
            "Ed25519 signatures must be deterministic; token may be using a non-deterministic variant"
        );
    }

    /// #2724 regression: TWO signers in ONE process against the SAME module.
    ///
    /// `launch_api` resolves a producer signer and then a snapshot signer
    /// (`load_snapshot_signer`). Before the shared-context fix each construction ran
    /// its own `C_Initialize`, and the second returned
    /// `CKR_CRYPTOKI_ALREADY_INITIALIZED` — "the Cryptoki library has already been
    /// initialized by a previous call to C_Initialize which did not have a matching
    /// C_Finalize call". `load_snapshot_signer` turns that into `process::exit(1)`,
    /// so a release binary with an HSM producer signer could not boot at all.
    ///
    /// Every pre-existing test here builds exactly ONE signer, which is why CI was
    /// green while the shipped binary restart-looped. The bug is only reachable with
    /// two live signers in one process, so that is what this asserts.
    ///
    /// Both signers must also remain USABLE afterwards: sharing a context is only
    /// correct if the second signer's session is independent of the first. A fix
    /// that merely avoided the error while returning a broken second signer would
    /// pass a construction-only check.
    #[test]
    #[ignore = "needs a provisioned softhsm2 token (docs/operator/hsm.md); run with -- --ignored"]
    fn two_signers_share_one_pkcs11_context() {
        let config = Pkcs11Config::from_env()
            .expect("WRFCOIN_HSM_* env vars must all be set to run the HSM signer tests");

        let producer =
            Pkcs11Signer::new(config.clone()).expect("first signer (producer) constructs");

        // THIS is the line that used to fail with CKR_CRYPTOKI_ALREADY_INITIALIZED.
        let snapshot = Pkcs11Signer::new(config)
            .expect("second signer (snapshot) must reuse the process context, not re-initialise it");

        let payload: [u8; 32] = [0x5a; 32];

        let producer_sig = producer.sign(&payload).expect("producer signs");
        let snapshot_sig = snapshot.sign(&payload).expect("snapshot signer signs");

        assert!(
            producer.public_key().verify(&payload, &producer_sig),
            "producer signature must verify"
        );
        assert!(
            snapshot.public_key().verify(&payload, &snapshot_sig),
            "snapshot signature must verify — a shared context must not break the second session"
        );

        // Interleave to prove the two sessions are genuinely independent rather than
        // one working only until the other is used.
        let producer_again = producer.sign(&payload).expect("producer still signs after snapshot did");
        assert!(
            producer.public_key().verify(&payload, &producer_again),
            "first signer must remain usable after a second signer shares its context"
        );
    }
}
