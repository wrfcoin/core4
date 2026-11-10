//! Testnet-only software **oracle key file** for release builds.
//!
//! Release builds take the oracle attestation signer from a PKCS#11 token
//! (`WRFCOIN_HSM_*` + `WRFCOIN_HSM_ORACLE_KEY_LABEL`), and that stays the
//! default. The software paths of [`SignerEnv::oracle`](super::SignerEnv::oracle)
//! (`WRFCOIN_ORACLE_SIGNER_PRIVATE_KEY` / `WRFCOIN_ORACLE_SIGNER_KEY_FILE`) are
//! compiled out of release builds.
//!
//! An operator without a token (for example a native Windows node) may instead,
//! on **testnet only**, point [`TESTNET_ORACLE_KEY_FILE`] at a file holding the
//! 32-byte ed25519 seed as 64 hex characters. This mirrors the snapshot
//! server key (`WRFCOIN_SNAPSHOT_SERVER_KEY_FILE`).
//!
//! The file is opened only through
//! [`open_owner_only_key_file`](crate::security::key_file::open_owner_only_key_file):
//! mode 0600 or stricter on Unix; on Windows only the current user, SYSTEM or
//! Administrators may appear in its owner and DACL. It is refused when:
//!
//! * the process did not declare `WRFCOIN_NETWORK_ID=testnet`;
//! * the process did not declare a chain id, or declared the mainnet default;
//! * an HSM is also configured (`WRFCOIN_HSM_MODULE_PATH`): two signers for
//!   one role is a misconfiguration, never a silent choice;
//! * one of the debug-only oracle key variables is also set, for the same
//!   reason.
//!
//! Process-local startup configuration only: it decides which key signs this
//! attester's votes, never block validity, state transitions or the state root.
//! The attester's custody check (the oracle key must differ from every other
//! role's key) runs on the loaded key exactly as for an HSM key.

use std::fmt;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, anyhow, bail, ensure};
use zeroize::Zeroizing;

use super::{BlockSigner, ResolvedSigner, SignerMode, SignerProvenance, env_vars};
use crate::crypto::domain_signing::DEFAULT_CHAIN_ID;
use crate::crypto::signature::{PrivateKey, PublicKey, Signature};
use crate::security::key_file::open_owner_only_key_file;

/// Path of the testnet software oracle key file.
pub const TESTNET_ORACLE_KEY_FILE: &str = "WRFCOIN_TESTNET_ORACLE_KEY_FILE";

/// The only network a software oracle key is accepted on in a release build.
const TESTNET_NETWORK_ID: &str = "testnet";

/// Upper bound on the key file: 64 hex characters plus generous whitespace.
const MAX_KEY_FILE_BYTES: u64 = 4096;

/// Software ed25519 oracle signer loaded from an owner-only key file.
pub struct TestnetOracleKeySigner {
    signing: PrivateKey,
    verifying: PublicKey,
    provenance: SignerProvenance,
}

impl fmt::Debug for TestnetOracleKeySigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print the private half.
        f.debug_struct("TestnetOracleKeySigner")
            .field("provenance", &self.provenance)
            .field("public_key", &self.verifying.to_hex())
            .finish_non_exhaustive()
    }
}

impl BlockSigner for TestnetOracleKeySigner {
    fn sign(&self, payload: &[u8]) -> Result<Signature> {
        Ok(self.signing.sign(payload))
    }
    fn public_key(&self) -> PublicKey {
        self.verifying.clone()
    }
    fn provenance(&self) -> &SignerProvenance {
        &self.provenance
    }
}

/// Whether this process may use a software oracle key at all. Pure, so the
/// testnet-only rule is unit-tested without touching the environment.
pub fn testnet_oracle_key_allowed(
    network_id: Option<&str>,
    chain_id: Option<&str>,
    hsm_configured: bool,
    debug_oracle_key_configured: bool,
) -> Result<()> {
    ensure!(
        !hsm_configured,
        "{TESTNET_ORACLE_KEY_FILE} and {} are both set. The HSM is the default oracle signer; \
         unset one of them",
        env_vars::HSM_MODULE_PATH
    );
    ensure!(
        !debug_oracle_key_configured,
        "{TESTNET_ORACLE_KEY_FILE} and {} / {} are both set. Configure one oracle key, not two",
        env_vars::ORACLE_SIGNER_PRIVATE_KEY, // secret-leak-ok: interpolates the env-var NAME, never its value
        env_vars::ORACLE_SIGNER_KEY_FILE
    );
    let network = network_id.map(str::trim).unwrap_or("");
    ensure!(
        network == TESTNET_NETWORK_ID,
        "{TESTNET_ORACLE_KEY_FILE} is testnet-only, but WRFCOIN_NETWORK_ID is '{network}'. Other \
         networks require a PKCS#11 token (WRFCOIN_HSM_* with WRFCOIN_HSM_ORACLE_KEY_LABEL)"
    );
    let chain = chain_id.map(str::trim).unwrap_or("");
    ensure!(
        !chain.is_empty() && chain != DEFAULT_CHAIN_ID,
        "{TESTNET_ORACLE_KEY_FILE} needs an explicit testnet WRFCOIN_CHAIN_ID (got '{chain}')"
    );
    Ok(())
}

/// Read and parse the key file. Refuses a file another account can read, a
/// symlink, an oversized file and anything but 64 hex characters. Errors name
/// the path, never the file's contents.
pub fn load_testnet_oracle_key(path: &Path) -> Result<TestnetOracleKeySigner> {
    let file =
        open_owner_only_key_file(path).map_err(|e| anyhow!("{TESTNET_ORACLE_KEY_FILE}: {e}"))?;
    let mut raw = Zeroizing::new(Vec::new());
    file.take(MAX_KEY_FILE_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|e| {
            anyhow!(
                "{TESTNET_ORACLE_KEY_FILE}: cannot read {}: {e}",
                path.display()
            )
        })?;
    ensure!(
        raw.len() as u64 <= MAX_KEY_FILE_BYTES,
        "{TESTNET_ORACLE_KEY_FILE}: {} is larger than a key file",
        path.display()
    );
    let text = std::str::from_utf8(&raw)
        .map_err(|_| anyhow!("{TESTNET_ORACLE_KEY_FILE}: {} is not text", path.display()))?
        .trim();
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!(
            "{TESTNET_ORACLE_KEY_FILE}: {} must hold one ed25519 seed as 64 hex characters",
            path.display()
        );
    }
    let seed = Zeroizing::new(
        hex::decode(text).map_err(|_| anyhow!("{TESTNET_ORACLE_KEY_FILE}: seed is not hex"))?,
    );
    let signing = PrivateKey::from_bytes(&seed)
        .map_err(|_| anyhow!("{TESTNET_ORACLE_KEY_FILE}: seed rejected"))?;
    let verifying = PublicKey::from_private_key(&signing);
    Ok(TestnetOracleKeySigner {
        signing,
        verifying,
        provenance: SignerProvenance::KeyFile {
            path: path.display().to_string(),
        },
    })
}

/// Resolve the testnet oracle key from the environment.
///
/// `Ok(None)` when [`TESTNET_ORACLE_KEY_FILE`] is unset or empty (the existing
/// HSM / debug resolution then runs unchanged). `Err` is fatal for the caller:
/// a configured-but-refused key must stop the attester, never fall back.
pub(super) fn resolve_from_env() -> Result<Option<ResolvedSigner>> {
    // determinism-ok: process-local startup credential read; selects which key signs this attester's votes, never block validity or state-transition results
    let Some(path) = std::env::var_os(TESTNET_ORACLE_KEY_FILE).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    // determinism-ok: startup presence checks of other signer configuration, never consensus input
    let hsm_configured = std::env::var_os(env_vars::HSM_MODULE_PATH).is_some();
    let debug_oracle_key_configured = [
        env_vars::ORACLE_SIGNER_PRIVATE_KEY,
        env_vars::ORACLE_SIGNER_KEY_FILE,
    ]
    .iter()
    // determinism-ok: startup presence check only; the value is never read
    .any(|var| std::env::var_os(var).is_some_and(|v| !v.is_empty()));
    testnet_oracle_key_allowed(
        crate::crypto::domain_signing::configured_network_id().as_deref(),
        crate::crypto::domain_signing::configured_chain_id().as_deref(),
        hsm_configured,
        debug_oracle_key_configured,
    )?;
    let path_str = path.to_string_lossy();
    let signer = load_testnet_oracle_key(Path::new(path_str.trim()))?;
    Ok(Some(ResolvedSigner {
        signer: Arc::new(signer),
        mode: SignerMode::TestnetKeyFile,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_declared_testnet_may_use_a_software_oracle_key() {
        assert!(testnet_oracle_key_allowed(Some("testnet"), Some("888"), false, false).is_ok());
        for network in [
            None,
            Some(""),
            Some("mainnet"),
            Some("devnet"),
            Some("Testnet"),
        ] {
            assert!(
                testnet_oracle_key_allowed(network, Some("888"), false, false).is_err(),
                "network {network:?} must be refused"
            );
        }
        for chain in [None, Some(" "), Some(DEFAULT_CHAIN_ID)] {
            assert!(testnet_oracle_key_allowed(Some("testnet"), chain, false, false).is_err());
        }
        // HSM stays the default: both configured is a refusal, not a choice.
        assert!(testnet_oracle_key_allowed(Some("testnet"), Some("888"), true, false).is_err());
        // Two software oracle keys at once is also a refusal.
        assert!(testnet_oracle_key_allowed(Some("testnet"), Some("888"), false, true).is_err());
    }

    #[cfg(unix)]
    fn key_file(dir: &Path, name: &str, contents: &str, mode: u32) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn key_file_loads_signs_and_refuses_loose_malformed_or_linked_files() {
        let dir =
            std::env::temp_dir().join(format!("wrfcoin-testnet-oracle-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let seed = "22".repeat(32);

        let good = key_file(&dir, "oracle.key", &format!("{seed}\n"), 0o600); // secret-leak-ok: fixed all-0x22 test fixture written to a temp file, not a real key
        let signer = load_testnet_oracle_key(&good).unwrap();
        let expected = PublicKey::from_private_key(&PrivateKey::from_hex(&seed).unwrap());
        assert_eq!(signer.public_key(), expected);
        let signature = signer.sign(b"payload").unwrap();
        assert!(expected.verify(b"payload", &signature));
        assert!(!format!("{signer:?}").contains(&seed)); // secret-leak-ok: asserts the Debug output does NOT contain the fixture

        let loose = key_file(&dir, "loose.key", &seed, 0o644);
        assert!(load_testnet_oracle_key(&loose).is_err());
        for (name, body) in [
            ("short.key", "abcd"),
            ("nothex.key", &"zz".repeat(32)[..]),
            ("empty.key", ""),
        ] {
            let path = key_file(&dir, name, body, 0o600);
            let err = format!("{:#}", load_testnet_oracle_key(&path).unwrap_err());
            assert!(err.contains("64 hex characters"), "{name}: {err}");
            assert!(
                !err.contains(&seed),
                "{name}: error must not echo key material"
            );
        }
        let big = key_file(&dir, "big.key", &"a".repeat(5000), 0o600);
        assert!(load_testnet_oracle_key(&big).is_err());

        let link = dir.join("link.key");
        std::os::unix::fs::symlink(&good, &link).unwrap();
        assert!(
            load_testnet_oracle_key(&link).is_err(),
            "a symlink must not redirect the read"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// End to end through the public oracle loader: a release-capable path
    /// that resolves, is wrapped like every other signer, and refuses when it
    /// is not on a declared testnet.
    #[cfg(unix)]
    #[test]
    fn load_oracle_signer_uses_the_testnet_key_file_only_on_testnet() {
        let _guard = super::super::test_env_lock();
        let dir =
            std::env::temp_dir().join(format!("wrfcoin-testnet-oracle-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let seed = "33".repeat(32);
        let path = key_file(&dir, "oracle.key", &seed, 0o600); // secret-leak-ok: fixed all-0x33 test fixture written to a temp file, not a real key
        let expected = PublicKey::from_private_key(&PrivateKey::from_hex(&seed).unwrap());

        let vars = [
            TESTNET_ORACLE_KEY_FILE,
            env_vars::HSM_MODULE_PATH,
            env_vars::ORACLE_SIGNER_PRIVATE_KEY,
            env_vars::ORACLE_SIGNER_KEY_FILE,
            crate::crypto::domain_signing::CHAIN_ID_ENV,
            crate::crypto::domain_signing::NETWORK_ID_ENV,
        ];
        let saved: Vec<_> = vars.iter().map(|v| (*v, std::env::var_os(v))).collect();
        // SAFETY: serialised by `test_env_lock`; restored below.
        unsafe {
            for v in vars {
                std::env::remove_var(v);
            }
            std::env::set_var(TESTNET_ORACLE_KEY_FILE, &path);
            std::env::set_var(crate::crypto::domain_signing::CHAIN_ID_ENV, "888");
            std::env::set_var(crate::crypto::domain_signing::NETWORK_ID_ENV, "testnet");
        }
        let resolved = super::super::load_oracle_signer().unwrap().expect("signer");
        assert_eq!(resolved.mode, SignerMode::TestnetKeyFile);
        assert_eq!(resolved.signer.public_key(), expected);

        unsafe { std::env::set_var(crate::crypto::domain_signing::NETWORK_ID_ENV, "mainnet") };
        assert!(
            super::super::load_oracle_signer().is_err(),
            "mainnet must refuse, never fall back"
        );

        unsafe {
            std::env::set_var(crate::crypto::domain_signing::NETWORK_ID_ENV, "testnet");
            std::env::set_var(env_vars::HSM_MODULE_PATH, "/nonexistent/module.so");
        }
        assert!(
            super::super::load_oracle_signer().is_err(),
            "HSM + key file must refuse"
        );

        unsafe {
            for (v, value) in saved {
                match value {
                    Some(value) => std::env::set_var(v, value),
                    None => std::env::remove_var(v),
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
