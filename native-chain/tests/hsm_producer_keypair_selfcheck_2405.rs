//! End-to-end regression for the #2405 producer keypair self-check.
//!
//! This is the two-key test Frank's review asked for: it drives the **production
//! constructor** against a real PKCS#11 token provisioned with TWO distinct
//! keypairs, and asserts that pairing one key's private half with the other's
//! public half is REFUSED at startup, with both objects' `CKA_ID`s named.
//!
//! It cannot be a unit test. The defect class lives in the object-resolution
//! path — two labels, two handles, two `CKA_ID`s — and a unit test with
//! in-memory keys cannot exercise `find_key_object` at all. That is exactly how
//! the original hole survived: everything around it was tested.
//!
//! Disabled by default; most machines have no PKCS#11 module. To run:
//!
//! ```bash
//! # Provision TWO keypairs on one token. The labels below match the current
//! # SoftHSM CI fixture; they are operator-chosen labels, not PIV slot names.
//! # The #2405 N5 confusable pair measured on 2026-07-25 was CKA_ID 01 /
//! # PIV 9A (producer) versus CKA_ID 02 / PIV 9C (snapshot). CKA_ID 03 is
//! # PIV 9D (Key Management), not PIV 9C.
//! HSM_TEST_ENABLED=1 \
//! WRFCOIN_HSM_MODULE_PATH=/usr/lib/softhsm/libsofthsm2.so \
//! WRFCOIN_HSM_TOKEN_LABEL=wrfcoin-validator \
//! WRFCOIN_HSM_PIN_ENV=WRFCOIN_HSM_PIN \
//! WRFCOIN_HSM_PIN=1234 \
//! WRFCOIN_HSM_KEY_LABEL=wrfcoin-signing-key \
//! WRFCOIN_HSM_PUBKEY_LABEL=wrfcoin-signing-key-always \
//! WRFCOIN_HSM_MATCHED_PUBKEY_LABEL=wrfcoin-signing-key \
//! WRFCOIN_HSM_MISMATCHED_PUBKEY_LABEL=wrfcoin-signing-key-always \
//! cargo test -p wrfcoin-native-chain --features hsm --test hsm_producer_keypair_selfcheck_2405
//! ```

// An empty test binary reports "0 passed; 0 failed; 0 ignored" — identical
// to a passing target and invisible behind sibling suites (the vacuity shape
// scripts/cargo_test_nonvacuous.py rejects as EMPTY_TEST_BINARY). Without the
// feature this target now declares its absence as a countable `ignored`
// instead of compiling to silence — #2748's principle, extended to the cfg
// dimension.
#[cfg(not(feature = "hsm"))]
#[test]
#[ignore = "this target's tests require --features hsm; without it the binary would be empty and indistinguishable from a passing one. Run: cargo test -p wrfcoin-native-chain --features hsm --test hsm_producer_keypair_selfcheck_2405"]
fn tests_require_the_hsm_feature() {}

#[cfg(feature = "hsm")]
mod gated {
    use wrfcoin_native_chain::storage::Storage;
    use wrfcoin_shared_protocol::crypto::signer::{Pkcs11Config, Pkcs11Signer};

    fn gate() -> bool {
        std::env::var("HSM_TEST_ENABLED").as_deref() == Ok("1")
    }

    /// A mismatched pair — private from one object, public from another — must
    /// ABORT startup rather than produce a usable signer.
    ///
    /// RED without `assert_keypair_consistency`: `Pkcs11Signer::new` returns `Ok`
    /// and the node goes on to sign block 1 with a key nobody will accept.
    #[test]
    fn mismatched_producer_pair_refuses_to_construct_2405() {
        if !gate() {
            println!(
                "skipping: HSM_TEST_ENABLED!=1. This test needs a token with TWO keypairs; \
                 see the module docs for provisioning."
            );
            return;
        }

        let config = Pkcs11Config::from_env()
            .expect("WRFCOIN_HSM_* must all be set when HSM_TEST_ENABLED=1");

        // The premise: the two labels must name DIFFERENT objects, or this test
        // proves nothing. If they were the same object the pair would be valid.
        assert_ne!(
            Some(config.key_label.as_str()),
            config.pubkey_label.as_deref(),
            "test premise: the private and public labels must name different objects, \
             otherwise this is not the confusable-slot case (#2405)"
        );

        let err = match Pkcs11Signer::new(config) {
            Err(e) => e,
            Ok(_) => panic!(
                "a private key from one object paired with a public key from another must \
                 be REFUSED at startup. Constructing successfully here means the node would \
                 sign block 1 with a key its peers reject — the failure #2405 exists to \
                 prevent, and the one Alice flagged as her last gap before the cut."
            ),
        };

        let msg = format!("{err:#}");
        // Print the refusal verbatim: it is the evidence that the CKA_ID read and the
        // verified PIV annotation render correctly on real hardware, which no unit
        // test on this machine can show.
        println!("\n----- REFUSAL (paste this back) -----\n{msg}\n----- END REFUSAL -----\n");
        assert!(
            msg.contains("MISMATCHED"),
            "the refusal must be the keypair self-check, not an unrelated error: {msg}"
        );
        // Both objects' ids must appear, so the operator can see WHICH two objects
        // were paired. The token-level slot is shared and cannot show this.
        assert!(
            msg.contains("CKA_ID="),
            "the error must name each object's CKA_ID, not just the shared token slot \
             (#2405 blocker 1): {msg}"
        );
        assert!(
            msg.contains("DIFFERENT CKA_IDs"),
            "with two distinct objects the error must state the divergence as a read \
             fact: {msg}"
        );
    }

    /// The converse: a correctly provisioned pair constructs. A check that cannot
    /// pass would be deleted the first time it blocked a launch.
    ///
    /// The matching PUBLIC label comes from its own variable rather than by dropping
    /// `WRFCOIN_HSM_PUBKEY_LABEL`. An earlier revision did the latter, which works
    /// only on a token where both halves of a pair share one label (SoftHSM, where
    /// the fixture chooses the labels). On a YubiKey they never do — the halves are
    /// `Private key for X` and `Public key for X` — so dropping the override made
    /// `new()` look the PUBLIC object up under the PRIVATE label, found nothing, and
    /// failed for a reason that has nothing to do with what this test asserts.
    #[test]
    fn matched_producer_pair_constructs_2405() {
        if !gate() {
            println!("skipping: HSM_TEST_ENABLED!=1.");
            return;
        }
        let matched = match std::env::var("WRFCOIN_HSM_MATCHED_PUBKEY_LABEL") {
            Ok(v) if !v.trim().is_empty() => v,
            _ => {
                println!(
                    "skipping matched-pair case: set WRFCOIN_HSM_MATCHED_PUBKEY_LABEL to the \
                     PUBLIC object label belonging to the SAME slot as WRFCOIN_HSM_KEY_LABEL \
                     (e.g. 'Public key for PIV Authentication' when the private half is \
                     'Private key for PIV Authentication')."
                );
                return;
            },
        };
        // The matched label is applied to the CONFIG, never to the process
        // environment. An earlier revision called `set_var("WRFCOIN_HSM_PUBKEY_LABEL")`
        // here while `mismatched_producer_pair_refuses_to_construct_2405` was
        // concurrently calling `from_env()` and reading that same variable — a real
        // data race, not a style nit. `set_var` mutates the libc `environ` array
        // with no synchronisation, which is why Rust 2024 marks it `unsafe`, and
        // writing an equal value does not make it safe: a concurrent reader can
        // still observe a torn or freed pointer mid-write. The old
        // "no other thread reads these vars" note was simply false — the sibling
        // test does, and cargo runs them on separate threads by default.
        //
        // `Pkcs11Config`'s fields are public, so the override needs no global at
        // all. Nothing to serialise and nothing to restore.
        let mut config = Pkcs11Config::from_env().expect("WRFCOIN_HSM_* must all be set");
        config.pubkey_label = Some(matched.clone());
        assert_eq!(
            config.pubkey_label.as_deref(),
            Some(matched.as_str()),
            "test premise: the matched public label must be the one under test"
        );
        Pkcs11Signer::new(config).unwrap_or_else(|e| {
            panic!(
                "a correctly provisioned keypair (private and public halves from the SAME \
                 slot) must construct. Got: {e:#}\n\
                 If this says 'no PKCS#11 object found', the label is wrong — check \
                 `pkcs11-tool --module <path> -O` (#2405)."
            )
        });
        println!("matched pair constructed OK — the self-check accepts a correct pairing");
    }

    // ---------------------------------------------------------------------------
    // The PRODUCTION CALL SITE must be load-bearing (Elsa's blocker 1)
    // ---------------------------------------------------------------------------
    //
    // The two tests above cover `Pkcs11Signer::new` as a HELPER: they call it
    // directly. That is not the same as covering the production invocation, and the
    // difference is exactly the gap this repo keeps shipping — the guard exists, the
    // call site is never exercised, and deleting the call leaves every test green.
    //
    // Elsa proved that asymmetry on this branch: removing `assert_keypair_consistency`
    // from the constructor left the 11 helper tests passing and turned NO real-binary
    // test red. So "a mismatched configured pair aborts the real startup before any
    // block is produced or persisted" was an untested claim.
    //
    // The test below closes it by running the built `launch_api` binary. Nothing in
    // this crate's library can make that process exit — only `main.rs` can — so the
    // required asymmetry holds by construction:
    //
    //   remove `assert_keypair_consistency(...)` from `Pkcs11Signer::new`
    //     -> helper tests stay GREEN (they assert on the returned Result)
    //     -> this test goes RED (the process no longer refuses)
    //
    // Modelled on the `Defect 2` section of producer_auth_persisted_boot_2290.rs,
    // which established this pattern for the boot-scan call site.

    use std::io::Read as _;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    fn selfcheck_temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wrf-2405-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn read_log(path: &Path) -> String {
        let mut s = String::new();
        if let Ok(mut f) = std::fs::File::open(path) {
            let _ = f.read_to_string(&mut s);
        }
        s
    }

    fn wait_for_exit(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait().expect("try_wait") {
                Some(status) => return Some(status),
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                },
                None => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    }

    /// A deliberately mismatched producer pair must abort the REAL startup, with no
    /// block produced and nothing persisted.
    ///
    /// The private half comes from `WRFCOIN_HSM_KEY_LABEL` and the public half from
    /// `WRFCOIN_HSM_MISMATCHED_PUBKEY_LABEL` — two distinct objects on one token,
    /// which is the N5 layout (PIV 9A vs 9C) and the CI layout (`--id 01` vs
    /// `--id 02`).
    ///
    /// The assertion is pinned to the `MISMATCHED` refusal specifically, not merely
    /// to a nonzero exit. A node misconfigured this way has several other ways to
    /// fail at startup, and a test that accepted any of them would be attributing
    /// the refusal to the wrong rule — the mistake made twice already on the parent
    /// branch.
    #[test]
    fn a_mismatched_producer_pair_aborts_the_real_startup_2405() {
        if !gate() {
            println!("skipping: HSM_TEST_ENABLED!=1. Needs a token with TWO keypairs.");
            return;
        }
        let mismatched = match std::env::var("WRFCOIN_HSM_MISMATCHED_PUBKEY_LABEL") {
            Ok(v) if !v.trim().is_empty() => v,
            _ => {
                println!(
                    "skipping: set WRFCOIN_HSM_MISMATCHED_PUBKEY_LABEL to a PUBLIC object \
                     label belonging to a DIFFERENT slot than WRFCOIN_HSM_KEY_LABEL."
                );
                return;
            },
        };
        let key_label = std::env::var("WRFCOIN_HSM_KEY_LABEL").expect("WRFCOIN_HSM_KEY_LABEL");
        assert_ne!(
            key_label, mismatched,
            "test premise: the two labels must name DIFFERENT objects, else this is not \
             the confusable-slot case (#2405)"
        );

        let dir = selfcheck_temp_dir("proc-mismatch");
        let log_path = dir.join("node.log");
        let out = std::fs::File::create(&log_path).expect("log file");
        let err = out.try_clone().expect("dup log file");

        // A seeded producer set makes producer auth ACTIVE, which is what forces the
        // producer signer to resolve at startup. The member itself is irrelevant:
        // the self-check lives inside `Pkcs11Signer::new`, which runs while the
        // signer is being constructed — strictly before any membership or payout
        // check could look at this set.
        // Both fields must be REAL, not merely well-shaped: the payout address
        // canonical (`0x` + 40 lowercase hex), and the public key a point that
        // actually decompresses on Edwards25519.
        //
        // Two CI runs of this test failed here before it ever reached the signer —
        // first on a bare un-prefixed address, then on `0…01` as a public key
        // ("Cannot decompress Edwards point"). Both times the binary exited NONZERO
        // for a reason that had nothing to do with the keypair self-check, and both
        // times the `MISMATCHED` assertion below caught it and failed the test.
        //
        // That is the whole argument for pinning to the message rather than the exit
        // code: a test asserting only "exited nonzero" would have gone GREEN on both
        // of those genesis-parse errors while proving nothing about #2405.
        //
        // The key below is the ed25519 public half of the all-ones seed — a fixed,
        // publicly derivable test vector, not key material.
        let genesis_set = r#"{"version":1,"members":[{"producer_id":"selfcheck-2405","payout_address":"0x0000000000000000000000000000000000000001","public_key":"8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c"}]}"#;

        let mut cmd = Command::new(env!("CARGO_BIN_EXE_launch_api"));
        cmd.env("WRFCOIN_DATA_DIR", &dir)
            .env("WRFCOIN_API_HOST", "127.0.0.1")
            .env("WRFCOIN_API_PORT", "0")
            .env("ALLOW_UNENCRYPTED_STORAGE", "true")
            .env("WRFCOIN_ALLOW_NO_AUTH", "1")
            .env("WRFCOIN_CHAIN_ID", "777")
            .env("WRFCOIN_NETWORK_ID", "testnet")
            .env("WRFCOIN_GENESIS_PRODUCER_SET", genesis_set)
            // The mismatch, expressed through the PRODUCER-role variables so the
            // resolver under test is the producer one, not the snapshot one.
            .env("WRFCOIN_HSM_PRODUCER_KEY_LABEL", &key_label)
            .env("WRFCOIN_HSM_PRODUCER_PUBKEY_LABEL", &mismatched)
            .env("RUST_LOG", "info")
            .env_remove("WRFCOIN_PRODUCER_SIGNER_PRIVATE_KEY")
            .env_remove("WRFCOIN_PRODUCER_SIGNER_KEY_FILE")
            .env_remove("WRFCOIN_STORAGE_ENCRYPTION_KEY")
            .env_remove("WRFCOIN_OPERATOR_TOKEN")
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err));

        let mut child = cmd.spawn().expect("spawn launch_api");
        let status = wait_for_exit(&mut child, Duration::from_secs(120));
        let text = read_log(&log_path);

        let status = status.unwrap_or_else(|| {
            panic!(
                "launch_api must EXIT when the configured producer keypair is mismatched; \
                 it kept running. A node that starts here signs block 1 with a key whose \
                 public half it does not hold — every peer rejects the block and the node \
                 reports itself healthy (#2405).\n{text}"
            )
        });
        assert!(
            !status.success(),
            "launch_api must exit NONZERO on a mismatched producer pair, got {status:?}.\n{text}"
        );
        assert!(
            text.contains("MISMATCHED"),
            "the refusal must be the keypair self-check, not an unrelated startup error. \
             Without this the test would pass on any misconfiguration and would be \
             attributing the exit to the wrong rule.\n{text}"
        );

        // Nothing may have been produced or persisted. Secondary to the refusal
        // assertion above — stated as defence in depth, not claimed as the RED
        // dimension unless a mutation run is observed to move it.
        let db = dir.join("wrfcoin.redb");
        if db.exists() {
            if let Ok(storage) = Storage::open(&db, 1_000_000) {
                if let Ok(Some(chain)) = storage.load_state() {
                    assert_eq!(
                        chain.in_memory_block_count(),
                        0,
                        "a refused startup must not have produced or persisted any block.\n{text}"
                    );
                }
            }
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
