//! Offline oracle receipt producer (#3149). The CLI derives the commit identity
//! from the reveal inputs, uses explicit chain binding and self-verifies before
//! writing. Production custody is PKCS#11; file keys are debug rehearsal-only.
//! Producing a receipt does not establish oracle-set membership or submit it.

use anyhow::{Context, Result, anyhow, bail};
use std::{fs, io::Write, path::Path};
use wrfcoin_shared_protocol::{
    blockchain::transaction::{
        ChainBinding, ForecastAccuracyMetrics, ForecastArtifactManifest,
        ForecastVerificationReceipt,
    },
    crypto::signer::BlockSigner,
};

/// Env var whose value is folded into the receipt signing preimage as the chain id.
pub const CHAIN_ID_ENV: &str = "WRFCOIN_CHAIN_ID";
/// Env var whose value is folded into the receipt signing preimage as the network id.
pub const NETWORK_ID_ENV: &str = "WRFCOIN_NETWORK_ID";

/// Validate exact declared/environment identities without trimming signed bytes.
pub fn check_chain_binding(
    declared_chain_id: &str,
    declared_network_id: &str,
    env_chain_id: Option<&str>,
    env_network_id: Option<&str>,
) -> Result<ChainBinding> {
    for (label, declared) in [
        ("chain-id", declared_chain_id),
        ("network-id", declared_network_id),
    ] {
        if declared.is_empty() || declared.trim() != declared {
            bail!("--{label} must be non-blank with no surrounding whitespace");
        }
    }
    let read =
        |name, value: Option<&str>| -> Result<String> {
            value.filter(|v| !v.trim().is_empty()).map(str::to_owned).ok_or_else(|| {
            anyhow!("{name} is unset or blank; explicit identity is required (no mainnet fallback)") // secret-leak-ok: name is a public environment-variable name, never key material
        })
        };
    let binding = ChainBinding::new(
        read(CHAIN_ID_ENV, env_chain_id)?,
        read(NETWORK_ID_ENV, env_network_id)?,
    )?;
    if binding.chain_id() != declared_chain_id {
        bail!("WRFCOIN_CHAIN_ID differs from --chain-id; refusing to sign");
    }
    if binding.network_id() != declared_network_id {
        bail!("WRFCOIN_NETWORK_ID differs from --network-id; refusing to sign");
    }
    if binding.is_unbound() {
        bail!("unbound is not a signing chain identity");
    }
    Ok(binding)
}

/// Capture identity once; signing and self-verification use this same binding.
pub fn binding_from_env(chain_id: &str, network_id: &str) -> Result<ChainBinding> {
    let binding = ChainBinding::from_env()?;
    check_chain_binding(
        chain_id,
        network_id,
        Some(binding.chain_id()),
        Some(binding.network_id()),
    )
}

/// Derive the commit id through the same canonical helper used by commit/reveal.
pub fn sign_receipt_for_input(
    input: &crate::forecast_submission::ForecastInput,
    manifest: &ForecastArtifactManifest,
    metrics: ForecastAccuracyMetrics,
    verified_at: i64,
    nonce: Option<&str>,
    signer: &dyn BlockSigner,
    binding: &ChainBinding,
) -> Result<ForecastVerificationReceipt> {
    let commit_id = input.commit_id()?;
    let nonce = nonce
        .map(str::to_owned)
        .unwrap_or_else(|| default_nonce(&commit_id));
    sign_receipt(
        manifest,
        &commit_id,
        metrics,
        verified_at,
        &nonce,
        signer,
        binding,
    )
}

/// The default anti-replay nonce for a receipt over `commit_id`.
///
/// The receipt already binds `commit_id`, so a nonce derived from it is unique per
/// commit — which is the only replay a receipt is exposed to. It is also
/// reproducible, so re-running the signer over the same scored commit yields the
/// same preimage instead of an unrelated second receipt. `--nonce` overrides it.
pub fn default_nonce(commit_id: &str) -> String {
    // `chars().take` rather than a byte slice: this runs before `sign_receipt`
    // has rejected a malformed id, and a panic there would be a worse error
    // message than the refusal that follows.
    let head: String = commit_id.trim().chars().take(16).collect();
    format!("receipt-{head}")
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, kind: &str) -> Result<T> {
    let raw = fs::read(path).with_context(|| format!("cannot read {kind} file"))?;
    serde_json::from_slice(&raw).map_err(|_| anyhow!("invalid {kind} JSON file"))
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// Assemble and sign a receipt, or explain why the inputs cannot make a valid one.
///
/// Validates the manifest/metrics and self-verifies the explicit-domain signature.
/// This does not establish genesis oracle membership or scorer correctness.
pub fn sign_receipt(
    manifest: &ForecastArtifactManifest,
    commit_id: &str,
    metrics: ForecastAccuracyMetrics,
    verified_at: i64,
    nonce: &str,
    signer: &dyn BlockSigner,
    binding: &ChainBinding,
) -> Result<ForecastVerificationReceipt> {
    check_chain_binding(binding.chain_id(), binding.network_id(),
        Some(binding.chain_id()), Some(binding.network_id()))?;
    manifest
        .validate()
        .map_err(|e| anyhow!("invalid forecast manifest: {e}"))?;
    if !is_hex64(commit_id) {
        bail!("commit id must be a 64-character hex commit hash");
    }
    if nonce.trim().is_empty() {
        bail!("receipt nonce must not be blank");
    }

    let mut receipt = ForecastVerificationReceipt {
        manifest_digest: manifest.manifest_digest(),
        commit_id: commit_id.to_ascii_lowercase(),
        // Derived from the key, never taken from a flag: this is both the identity
        // `is_authorized_oracle` looks up and the key the signature is checked
        // against, so the two can never name different oracles.
        oracle_id: signer.public_key().to_hex(),
        metrics,
        verified_at,
        nonce: nonce.trim().to_string(),
        zk_proof: None,
        signature: None,
    };
    // Covers the metrics too (finite, non-negative errors, skill in [0,1]), so a
    // scorer that emitted a NaN cannot get it signed.
    receipt
        .validate()
        .map_err(|e| anyhow!("receipt is not valid: {e}"))?;

    let message = receipt.signing_message_with(binding);
    receipt.signature = Some(signer.sign(message.as_bytes())?.to_hex());
    if !receipt.has_valid_signature_with(binding) {
        bail!("receipt signature does not verify locally — refusing to emit it");
    }
    Ok(receipt)
}

/// Write `receipt` to `path` as 0600 JSON, refusing to clobber an existing file.
///
/// A receipt is not a secret, but it is signed evidence: silently replacing one
/// loses the record of what an oracle already attested to. Same refusal as
/// `forecast-proto keygen`.
pub fn write_receipt(receipt: &ForecastVerificationReceipt, path: &Path) -> Result<()> {
    let encoded = serde_json::to_vec_pretty(receipt).context("cannot encode receipt")?;
    #[cfg(not(unix))]
    bail!("0600 receipt output is unsupported on this platform");
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    anyhow!("refusing to overwrite existing receipt file")
                } else {
                    anyhow!("cannot create receipt file: {error}")
                }
            })?;
        file.write_all(&encoded)
            .context("cannot write receipt file")?;
        file.sync_all().context("cannot sync receipt file")?;
    }
    Ok(())
}

/// Read the manifest and metrics files a signing run needs.
pub fn read_inputs(
    manifest_file: &Path,
    metrics_file: &Path,
) -> Result<(ForecastArtifactManifest, ForecastAccuracyMetrics)> {
    let manifest: ForecastArtifactManifest = read_json(manifest_file, "manifest")?;
    let metrics: ForecastAccuracyMetrics = read_json(metrics_file, "metrics")?;
    Ok((manifest, metrics))
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::crypto::{signature::PrivateKey, signer::EnvKeySigner};

    fn binding() -> ChainBinding {
        ChainBinding::new("777", "testnet").unwrap()
    }

    fn manifest() -> ForecastArtifactManifest {
        ForecastArtifactManifest {
            schema_version: 1,
            model_id: "diurnal-anomaly/v1".into(),
            input_hash: "a".repeat(64),
            output_hash: "b".repeat(64),
            produced_at: 1_700_000_000,
            region: "synthetic-region".into(),
            horizon_hours: 6,
            input_variables: vec!["temperature".into()],
            output_variables: vec!["temperature".into()],
        }
    }

    fn metrics(skill: f64) -> ForecastAccuracyMetrics {
        ForecastAccuracyMetrics {
            mae: 0.8,
            rmse: 1.1,
            bias: -0.2,
            skill_score: skill,
        }
    }

    fn key() -> EnvKeySigner {
        EnvKeySigner::from_private_key(
            PrivateKey::from_hex(&"11".repeat(32)).expect("test key"),
            "synthetic-test",
        )
    }

    const COMMIT: &str = "c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00";

    fn signed(skill: f64) -> ForecastVerificationReceipt {
        sign_receipt(
            &manifest(),
            COMMIT,
            metrics(skill),
            1_700_000_100,
            "n-1",
            &key(),
            &binding(),
        )
        .expect("receipt signs")
    }

    #[test]
    fn a_signed_receipt_binds_the_manifest_digest_and_verifies_3149() {
        let receipt = signed(0.87);
        assert_eq!(receipt.manifest_digest, manifest().manifest_digest());
        assert_eq!(receipt.commit_id, COMMIT);
        assert!(receipt.validate().is_ok());
        assert!(
            receipt.has_valid_signature_with(&binding()),
            "the signer must not emit a receipt the reveal path would drop"
        );
    }

    #[test]
    fn oracle_id_is_the_signing_keys_public_key_not_an_operator_claim_3149() {
        // `is_authorized_oracle` looks this field up in the genesis oracle set and
        // `has_valid_signature` verifies against it. Deriving it from the key is
        // what makes "signed by an oracle-set key" mean what it says.
        let receipt = signed(0.5);
        assert_eq!(receipt.oracle_id, key().public_key().to_hex());
    }

    #[test]
    fn editing_the_skill_score_after_signing_breaks_the_signature_3149() {
        // The whole economic point: the skill score is what the reveal mints at,
        // so it must be inside the signed preimage, not alongside it.
        let mut receipt = signed(0.20);
        assert!(receipt.has_valid_signature_with(&binding()));
        receipt.metrics.skill_score = 0.99;
        assert!(
            !receipt.has_valid_signature_with(&binding()),
            "a receipt whose payout field can be edited in flight is not evidence"
        );
    }

    #[test]
    fn a_nan_skill_score_is_refused_before_it_is_signed_3149() {
        let err = sign_receipt(
            &manifest(),
            COMMIT,
            metrics(f64::NAN),
            1_700_000_100,
            "n-1",
            &key(),
            &binding(),
        )
        .expect_err("NaN skill must not sign");
        assert!(err.to_string().contains("receipt is not valid"), "{err}");
    }

    #[test]
    fn a_short_commit_id_is_refused_3149() {
        let err = sign_receipt(
            &manifest(),
            "c0ffee",
            metrics(0.5),
            1,
            "n-1",
            &key(),
            &binding(),
        )
        .expect_err("a truncated commit id must not sign");
        assert!(err.to_string().contains("64-character hex"), "{err}");
    }

    #[test]
    fn an_unset_chain_id_is_refused_instead_of_silently_signing_for_mainnet_3149() {
        // The command refuses missing identities before signing, independent
        // of the library's compatibility fallback for ambient callers.
        for absent in [None, Some(""), Some("   ")] {
            let err = check_chain_binding("wrfcoin-777", "testnet", absent, Some("testnet"))
                .expect_err("an unset chain id must refuse");
            let text = err.to_string();
            assert!(
                text.contains(CHAIN_ID_ENV),
                "the refusal must name the variable: {text}"
            );
            assert!(
                text.contains("mainnet"),
                "the refusal must say what would happen: {text}"
            );
        }
    }

    #[test]
    fn an_unset_network_id_is_refused_and_named_3149() {
        let err = check_chain_binding("wrfcoin-777", "testnet", Some("wrfcoin-777"), None)
            .expect_err("an unset network id must refuse");
        assert!(err.to_string().contains(NETWORK_ID_ENV), "{err}");
    }

    #[test]
    fn a_declaration_the_environment_contradicts_is_refused_3149() {
        let err = check_chain_binding(
            "wrfcoin-777",
            "testnet",
            Some("wrfcoin-main"),
            Some("testnet"),
        )
        .expect_err("a contradicted declaration must refuse");
        let text = err.to_string();
        assert!(text.contains(CHAIN_ID_ENV), "{text}");
    }

    #[test]
    fn an_agreeing_declaration_is_accepted_3149() {
        assert!(
            check_chain_binding(
                "wrfcoin-777",
                "testnet",
                Some("wrfcoin-777"),
                Some("testnet"),
            )
            .is_ok()
        );
    }

    #[test]
    fn a_blank_declared_binding_is_refused_3149() {
        let err = check_chain_binding("  ", "testnet", Some("wrfcoin-777"), Some("testnet"))
            .expect_err("a blank declaration must refuse");
        assert!(err.to_string().contains("chain-id"), "{err}");
    }

    #[test]
    fn the_default_nonce_is_derived_from_the_commit_and_never_panics_3149() {
        assert_eq!(default_nonce(COMMIT), format!("receipt-{}", &COMMIT[..16]));
        // Reached before `sign_receipt` has rejected a malformed id.
        assert_eq!(default_nonce("abc"), "receipt-abc");
        assert_eq!(default_nonce(""), "receipt-");
    }

    #[test]
    fn an_existing_receipt_file_is_never_overwritten_3149() {
        let dir = std::env::temp_dir().join(format!("wrf-3149-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("receipt.json");
        let _ = std::fs::remove_file(&path);

        write_receipt(&signed(0.4), &path).expect("first write succeeds");
        let first = std::fs::read(&path).expect("written");
        let err = write_receipt(&signed(0.9), &path).expect_err("second write must refuse");
        assert!(err.to_string().contains("refusing to overwrite"), "{err}");
        assert_eq!(
            std::fs::read(&path).expect("still there"),
            first,
            "the file is untouched"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "receipt must be written 0600");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn padded_declared_or_environment_bindings_are_refused() {
        for (chain, network, env_chain, env_network) in [
            ("777 ", "testnet", "777 ", "testnet"),
            ("777", " testnet", "777", " testnet"),
            ("777", "testnet", "777 ", "testnet"),
            ("777", "testnet", "777", "testnet "),
        ] {
            assert!(
                check_chain_binding(chain, network, Some(env_chain), Some(env_network)).is_err()
            );
        }
        assert!(
            check_chain_binding("unbound", "unbound", Some("unbound"), Some("unbound")).is_err()
        );
    }

    #[test]
    fn receipt_cannot_verify_on_another_chain() {
        let receipt = signed(0.5);
        assert!(!receipt.has_valid_signature_with(&ChainBinding::new("778", "testnet").unwrap()));
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_is_never_followed_for_output() {
        let dir = std::env::temp_dir().join(format!("receipt-link-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let target = dir.join("absent.json");
        let link = dir.join("receipt.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(write_receipt(&signed(0.5), &link).is_err());
        assert!(!target.exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn signer_returning_a_signature_for_other_bytes_is_refused() {
        use wrfcoin_shared_protocol::crypto::{
            signature::{PublicKey, Signature}, signer::SignerProvenance,
        };
        #[derive(Debug)]
        struct CorruptSigner(EnvKeySigner);
        impl BlockSigner for CorruptSigner {
            fn sign(&self, _: &[u8]) -> Result<Signature> { self.0.sign(b"wrong preimage") }
            fn public_key(&self) -> PublicKey { self.0.public_key() }
            fn provenance(&self) -> &SignerProvenance { self.0.provenance() }
        }
        let error = sign_receipt(&manifest(), COMMIT, metrics(0.5), 1_700_000_100,
            "n-1", &CorruptSigner(key()), &binding()).expect_err("self-verification must refuse");
        assert!(error.to_string().contains("does not verify"));
    }

}
