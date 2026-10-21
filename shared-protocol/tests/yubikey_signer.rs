//! Operator integration test: signs against a *real* YubiKey 5 via the
//! OpenSC PKCS#11 module.
//!
//! Distinct from `native-chain/tests/hsm_signer.rs` (softhsm2 path).
//! This test is `#[ignore]`d so `cargo test` never runs it without the
//! operator opting in. To run:
//!
//! ```bash
//! WRFCOIN_HSM_TEST_YUBIKEY=1 \
//! WRFCOIN_HSM_MODULE_PATH=/usr/lib/x86_64-linux-gnu/opensc-pkcs11.so \
//! WRFCOIN_HSM_TOKEN_LABEL="<from pkcs11-tool -T>" \
//! WRFCOIN_HSM_KEY_LABEL="SIGN key" \
//! WRFCOIN_HSM_PIN_ENV=WRFCOIN_HSM_PIN \
//! WRFCOIN_HSM_PIN=<PIV PIN> \
//! cargo test -p wrfcoin-shared-protocol --features hsm \
//!   --test yubikey_signer -- --ignored
//! ```
//!
//! Both the `#[ignore]` attribute and the runtime env-var gate are
//! required: the gate documents that a key is plugged in *and* lets
//! the test print a helpful skip message if the operator forgets to
//! pass `--ignored`. See `docs/hsm/yubikey.md` for provisioning.

// An empty test binary reports "0 passed; 0 failed; 0 ignored" — identical
// to a passing target and invisible behind sibling suites (the vacuity shape
// scripts/cargo_test_nonvacuous.py rejects as EMPTY_TEST_BINARY). Without the
// feature this target now declares its absence as a countable `ignored`
// instead of compiling to silence — #2748's principle, extended to the cfg
// dimension.
#[cfg(not(feature = "hsm"))]
#[test]
#[ignore = "this target's tests require --features hsm; without it the binary would be empty and indistinguishable from a passing one. Run: cargo test -p wrfcoin-shared-protocol --features hsm --test yubikey_signer -- --ignored"]
fn tests_require_the_hsm_feature() {}

#[cfg(feature = "hsm")]
mod gated {
    use wrfcoin_shared_protocol::crypto::signer::{
        BlockSigner, Pkcs11Config, Pkcs11Signer, SignerProvenance,
    };

    fn gate() -> bool {
        std::env::var("WRFCOIN_HSM_TEST_YUBIKEY").as_deref() == Ok("1")
    }

    #[test]
    #[ignore = "requires a physical YubiKey 5 (firmware >= 5.7) provisioned per docs/hsm/yubikey.md; opt in with WRFCOIN_HSM_TEST_YUBIKEY=1"]
    fn yubikey_signs_and_verifies_block_header() {
        if !gate() {
            println!(
                "skipping: WRFCOIN_HSM_TEST_YUBIKEY!=1. See docs/hsm/yubikey.md for setup."
            );
            return;
        }

        let config = Pkcs11Config::from_env()
            .expect("WRFCOIN_HSM_* env vars must all be set when WRFCOIN_HSM_TEST_YUBIKEY=1");
        let signer = Pkcs11Signer::new(config)
            .expect("Pkcs11Signer::new succeeds against the plugged-in YubiKey");

        // 32-byte block-header-shaped payload.
        let header_hash: [u8; 32] = [
            0x59, 0x21, 0x84, 0x77, 0x3c, 0xa1, 0x90, 0xee, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70,
            0x80, 0x90, 0xa0, 0xb0, 0xc0, 0xd0, 0xe0, 0xf0, 0x01, 0x12, 0x23, 0x34, 0x45, 0x56, 0x67,
            0x78, 0x89,
        ];

        let sig = signer
            .sign(&header_hash)
            .expect("YubiKey signs the payload via CKM_EDDSA");
        let pk = signer.public_key();
        assert!(
            pk.verify(&header_hash, &sig),
            "YubiKey-produced signature must verify against the token's advertised public key"
        );

        match signer.provenance() {
            SignerProvenance::Pkcs11 {
                module_path,
                token_label,
                key_label,
            } => {
                // The provenance check verifies the signer is wired through a real
                // PKCS#11 module — not which specific module. OpenSC and Yubico's
                // libykcs11 are both valid; libykcs11 is the one that actually
                // surfaces the YubiKey PIV slot 9c private key for sign (see
                // infra#511 and the recipe in `docs/hsm/yubikey.md`).
                let lower = module_path.to_lowercase();
                assert!(
                    lower.contains("opensc") || lower.contains("ykcs11"),
                    "expected an OpenSC or YubiKey PKCS#11 module path, got {module_path:?}"
                );
                assert!(!token_label.is_empty());
                assert!(!key_label.is_empty());
            }
            other => panic!("expected Pkcs11 provenance, got {other:?}"),
        }

        // Ed25519 is deterministic. A randomised EdDSA variant would fail
        // here; the chain's verifier relies on determinism in some
        // equivocation-detection paths. Do not ship a token that flunks this.
        let sig2 = signer
            .sign(&header_hash)
            .expect("YubiKey re-signs the same payload");
        assert_eq!(
            sig.as_bytes(),
            sig2.as_bytes(),
            "YubiKey produced non-deterministic Ed25519 — firmware or PKCS#11 module is wrong; do not use this token for block signing"
        );
    }
}
