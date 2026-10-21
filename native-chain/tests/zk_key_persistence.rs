// An empty test binary reports "0 passed; 0 failed; 0 ignored" — identical
// to a passing target and invisible behind sibling suites (the vacuity shape
// scripts/cargo_test_nonvacuous.py rejects as EMPTY_TEST_BINARY). Without the
// feature this target now declares its absence as a countable `ignored`
// instead of compiling to silence — #2748's principle, extended to the cfg
// dimension.
#[cfg(not(feature = "zk"))]
#[test]
#[ignore = "this target's tests require --features zk; without it the binary would be empty and indistinguishable from a passing one. Run: cargo test -p wrfcoin-native-chain --features zk --test zk_key_persistence"]
fn tests_require_the_zk_feature() {}

#[cfg(feature = "zk")]
mod gated {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use ark_bn254::Fr;
    use ark_serialize::CanonicalSerialize;
    use sha2::{Digest, Sha256};
    use wrfcoin_native_chain::zk_service::{generate_range_proof, verify_range_proof};

    const CHILD_FLAG: &str = "WRFCOIN_ZK_PERSISTENCE_CHILD";
    const CORRUPT_CHILD_FLAG: &str = "WRFCOIN_ZK_CORRUPT_CHILD";
    const INTEROP_PRODUCER_FLAG: &str = "WRFCOIN_ZK_INTEROP_PRODUCER_CHILD";
    const INTEROP_VERIFIER_FLAG: &str = "WRFCOIN_ZK_INTEROP_VERIFIER_CHILD";
    const DATA_DIR_ENV: &str = "WRFCOIN_ZK_TEST_DATA_DIR";
    const PROOF_FILE_ENV: &str = "WRFCOIN_ZK_TEST_PROOF_FILE";
    const RANGE_PROOF_KEY_BASENAME: &str = "range-proof-64";
    const RANGE_PROOF_KEY_BYTES: &[u8] = include_bytes!("../zk-params/range-proof-64.bin");

    /// The libtest name of a test in THIS module, as `--exact` needs it.
    ///
    /// #3200: 680fd1186 moved these tests into `mod gated`, but the child
    /// re-exec calls kept the bare names, so every child matched nothing, ran
    /// zero tests and exited 0. One test went red and three went vacuously
    /// green. Deriving the path from `module_path!()` (minus the crate
    /// segment) keeps the name correct if the module moves again.
    fn exact_test_name(test_name: &str) -> String {
        let module = module_path!();
        match module.split_once("::") {
            Some((_crate, path)) => format!("{path}::{test_name}"),
            None => test_name.to_string(),
        }
    }

    /// Re-exec this test binary to run exactly one child test.
    fn child(test_name: &str) -> Command {
        let mut command =
            Command::new(env::current_exe().expect("current test binary should exist"));
        command.arg("--exact").arg(exact_test_name(test_name));
        command
    }

    /// A child that exits 0 after running zero tests proves nothing, so
    /// require that it ran exactly the one test it was asked for and that it
    /// passed.
    fn assert_child_ran(output: &std::process::Output, test_name: &str, what: &str) {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stdout.contains("running 1 test") && stdout.contains("test result: ok. 1 passed"),
            "child `{}` must run exactly one test and pass it ({what}); \
             a zero-test child is a vacuous pass (#3200).\nstdout:\n{stdout}\nstderr:\n{stderr}",
            exact_test_name(test_name)
        );
        assert!(output.status.success(), "{what}\nstdout:\n{stdout}\nstderr:\n{stderr}");
    }

    fn unique_temp_dir() -> PathBuf {
        let unique = format!(
            "wrfcoin-zk-keys-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        );
        env::temp_dir().join(unique)
    }

    fn key_file_path(data_dir: &Path) -> PathBuf {
        data_dir
            .join("zk-keys")
            .join(expected_key_file_name())
    }

    fn expected_key_file_name() -> String {
        let fingerprint = hex::encode(Sha256::digest(RANGE_PROOF_KEY_BYTES));
        format!("{RANGE_PROOF_KEY_BASENAME}-{}.bin", &fingerprint[..16])
    }

    fn blinding_hex(value: u64) -> String {
        let mut bytes = Vec::new();
        Fr::from(value)
            .serialize_compressed(&mut bytes)
            .expect("field serialization should succeed");
        hex::encode(bytes)
    }

    fn write_proof_artifact(path: &Path, proof_hex: &str, commitment_hex: &str) {
        let payload = format!("{proof_hex}\n{commitment_hex}\n");
        fs::write(path, payload).expect("proof artifact should be written");
    }

    fn read_proof_artifact(path: &Path) -> (String, String) {
        let payload = fs::read_to_string(path).expect("proof artifact should be readable");
        let mut lines = payload.lines();
        let proof_hex = lines
            .next()
            .expect("proof artifact must include proof hex")
            .to_string();
        let commitment_hex = lines
            .next()
            .expect("proof artifact must include commitment hex")
            .to_string();
        assert!(
            lines.next().is_none(),
            "proof artifact should contain exactly two lines"
        );
        (proof_hex, commitment_hex)
    }

    #[test]
    fn zk_keys_persist_across_process_restart() {
        let data_dir = unique_temp_dir();
        fs::create_dir_all(&data_dir).expect("temp data dir should be created");

        let blinding_hex = blinding_hex(7);
        let previous_data_dir = env::var_os("WRFCOIN_DATA_DIR");
        env::set_var("WRFCOIN_DATA_DIR", &data_dir);

        let (proof_hex, commitment_hex) =
            generate_range_proof(77, &blinding_hex).expect("initial proof generation should succeed");
        assert!(
            verify_range_proof(&proof_hex, &commitment_hex).expect("initial proof should verify"),
            "initial proof should verify against generated keys"
        );

        let key_file = key_file_path(&data_dir);
        assert!(key_file.exists(), "persisted key file should be written");
        let bytes_before = fs::read(&key_file).expect("persisted key file should be readable");
        assert!(
            !bytes_before.is_empty(),
            "persisted key file should contain serialized key material"
        );

        let status = child("zk_keys_persistence_child_process_loads_from_disk")
            .env(CHILD_FLAG, "1")
            .env(DATA_DIR_ENV, &data_dir)
            .output()
            .expect("child process should launch");

        if let Some(value) = previous_data_dir {
            env::set_var("WRFCOIN_DATA_DIR", value);
        } else {
            env::remove_var("WRFCOIN_DATA_DIR");
        }

        assert_child_ran(&status, "zk_keys_persistence_child_process_loads_from_disk", "child process should verify persisted keys");

        let bytes_after = fs::read(&key_file).expect("persisted key file should remain readable");
        assert_eq!(
            bytes_before, bytes_after,
            "child process should load existing keys instead of overwriting them"
        );

        fs::remove_dir_all(&data_dir).expect("temp data dir should be cleaned up");
    }

    #[test]
    fn zk_keys_persistence_child_process_loads_from_disk() {
        if env::var_os(CHILD_FLAG).is_none() {
            return;
        }

        let data_dir = env::var_os(DATA_DIR_ENV).expect("child process should receive test data dir");
        env::set_var("WRFCOIN_DATA_DIR", data_dir);

        let blinding_hex = blinding_hex(7);
        let (proof_hex, commitment_hex) =
            generate_range_proof(77, &blinding_hex).expect("child proof generation should succeed");
        assert!(
            verify_range_proof(&proof_hex, &commitment_hex).expect("child proof should verify"),
            "child process should verify proof using persisted keys"
        );
    }

    #[test]
    fn corrupt_persisted_keys_are_restored_from_embedded_bundle() {
        let data_dir = unique_temp_dir();
        let key_dir = data_dir.join("zk-keys");
        fs::create_dir_all(&key_dir)
            .expect("temp key dir should be created");
        let key_file = key_dir.join(format!("{RANGE_PROOF_KEY_BASENAME}-corrupt.bin"));
        let corrupt_bytes = b"corrupt-zk-key-material".to_vec();
        fs::write(&key_file, &corrupt_bytes).expect("corrupt key file should be written");

        let status = child("zk_keys_persistence_child_process_rejects_corrupt_file")
            .env(CORRUPT_CHILD_FLAG, "1")
            .env(DATA_DIR_ENV, &data_dir)
            .output()
            .expect("child process should launch");

        assert_child_ran(&status, "zk_keys_persistence_child_process_rejects_corrupt_file", "child process should restore the embedded key material");

        let restored_key_file = key_file_path(&data_dir);
        let bytes_after =
            fs::read(&restored_key_file).expect("restored key file should remain readable");
        assert_ne!(
            bytes_after, corrupt_bytes,
            "corrupt key file should be replaced with canonical key material"
        );
        assert_eq!(
            bytes_after, RANGE_PROOF_KEY_BYTES,
            "restored key file should match the embedded canonical parameters"
        );

        fs::remove_dir_all(&data_dir).expect("temp data dir should be cleaned up");
    }

    #[test]
    fn zk_keys_persistence_child_process_rejects_corrupt_file() {
        if env::var_os(CORRUPT_CHILD_FLAG).is_none() {
            return;
        }

        let data_dir = env::var_os(DATA_DIR_ENV).expect("child process should receive test data dir");
        env::set_var("WRFCOIN_DATA_DIR", data_dir);

        let blinding_hex = blinding_hex(11);
        assert!(
            generate_range_proof(55, &blinding_hex).is_ok(),
            "corrupt persisted key file should be restored from embedded canonical parameters"
        );
    }

    #[test]
    fn zk_proofs_verify_across_distinct_node_data_dirs() {
        let producer_dir = unique_temp_dir();
        let verifier_dir = unique_temp_dir();
        let producer_key_dir = producer_dir.join("zk-keys");
        let verifier_key_dir = verifier_dir.join("zk-keys");
        fs::create_dir_all(&producer_key_dir).expect("producer zk-keys dir should be created");
        fs::create_dir_all(&verifier_key_dir).expect("verifier zk-keys dir should be created");

        // Pre-populate both data dirs with the canonical embedded bundle so that
        // load_or_restore_key_pair takes the disk-load path rather than the
        // NotFound path. Without this both nodes trivially fall through to the
        // in-binary bundle without ever loading from disk, making the test vacuous.
        fs::write(key_file_path(&producer_dir), RANGE_PROOF_KEY_BYTES)
            .expect("producer canonical key file should be pre-populated");
        fs::write(key_file_path(&verifier_dir), RANGE_PROOF_KEY_BYTES)
            .expect("verifier canonical key file should be pre-populated");

        let artifact_file = producer_dir.join("proof-artifact.txt");

        let producer_status = child("zk_proofs_verify_across_distinct_node_data_dirs_producer")
            .env(INTEROP_PRODUCER_FLAG, "1")
            .env(DATA_DIR_ENV, &producer_dir)
            .env(PROOF_FILE_ENV, &artifact_file)
            .output()
            .expect("producer process should launch");
        assert_child_ran(&producer_status, "zk_proofs_verify_across_distinct_node_data_dirs_producer", "producer process should generate a proof with canonical parameters");

        let verifier_status = child("zk_proofs_verify_across_distinct_node_data_dirs_verifier")
            .env(INTEROP_VERIFIER_FLAG, "1")
            .env(DATA_DIR_ENV, &verifier_dir)
            .env(PROOF_FILE_ENV, &artifact_file)
            .output()
            .expect("verifier process should launch");
        assert_child_ran(&verifier_status, "zk_proofs_verify_across_distinct_node_data_dirs_verifier", "proof generated under one node data dir should verify under another");

        let producer_key_bytes = fs::read(key_file_path(&producer_dir))
            .expect("producer canonical key file should be readable");
        let verifier_key_bytes = fs::read(key_file_path(&verifier_dir))
            .expect("verifier canonical key file should be readable");
        assert_eq!(producer_key_bytes, RANGE_PROOF_KEY_BYTES);
        assert_eq!(verifier_key_bytes, RANGE_PROOF_KEY_BYTES);

        fs::remove_dir_all(&producer_dir).expect("producer temp data dir should be cleaned up");
        fs::remove_dir_all(&verifier_dir).expect("verifier temp data dir should be cleaned up");
    }

    #[test]
    fn zk_proofs_verify_across_distinct_node_data_dirs_producer() {
        if env::var_os(INTEROP_PRODUCER_FLAG).is_none() {
            return;
        }

        let data_dir = env::var_os(DATA_DIR_ENV).expect("producer should receive test data dir");
        let proof_file = env::var_os(PROOF_FILE_ENV).expect("producer should receive proof file path");
        env::set_var("WRFCOIN_DATA_DIR", data_dir);

        let blinding_hex = blinding_hex(13);
        let (proof_hex, commitment_hex) =
            generate_range_proof(99, &blinding_hex).expect("producer proof generation should succeed");
        write_proof_artifact(Path::new(&proof_file), &proof_hex, &commitment_hex);
    }

    #[test]
    fn zk_proofs_verify_across_distinct_node_data_dirs_verifier() {
        if env::var_os(INTEROP_VERIFIER_FLAG).is_none() {
            return;
        }

        let data_dir = env::var_os(DATA_DIR_ENV).expect("verifier should receive test data dir");
        let proof_file = env::var_os(PROOF_FILE_ENV).expect("verifier should receive proof file path");
        env::set_var("WRFCOIN_DATA_DIR", data_dir);

        let (proof_hex, commitment_hex) = read_proof_artifact(Path::new(&proof_file));
        assert!(
            verify_range_proof(&proof_hex, &commitment_hex)
                .expect("verifier process should load canonical keys"),
            "proof generated on another node should verify with the same embedded parameters"
        );
    }
}
