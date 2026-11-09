//! Testnet-only software **server key** for the snapshot signer.
//!
//! A bonded node's registered server key must be the key that signs the
//! snapshots it serves (`node_service_archive::server_claim` refuses a ticket
//! otherwise). Release builds normally take that signer from a PKCS#11 token
//! (`WRFCOIN_HSM_*`), and that stays the default. A native Windows node has no
//! token, so on **testnet only** the operator may instead point
//! `WRFCOIN_SNAPSHOT_SERVER_KEY_FILE` at a file holding the 32-byte ed25519
//! seed as 64 hex characters (owner decision 2026-10-31).
//!
//! The file must be readable by no other account
//! ([`open_owner_only_key_file`]: mode 0600 on Unix; on Windows only the
//! current user, SYSTEM or Administrators in its DACL, which the Windows
//! launcher's `Protect-UserOnly` guarantees for files it creates). It is
//! refused when:
//!
//! * the node did not declare `WRFCOIN_NETWORK_ID=testnet` (mainnet, an
//!   undeclared network or any other network keeps requiring a token);
//! * the node did not declare a chain id, or declared the mainnet default;
//! * an HSM is also configured (`WRFCOIN_HSM_MODULE_PATH`): two signers for one
//!   role is a misconfiguration, never a silent choice.
//!
//! Node-local startup configuration only: it decides which key signs snapshot
//! payloads and bonded tickets this process serves, never block validity,
//! state transitions or the state root.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, bail, ensure, Result};
use wrfcoin_shared_protocol::crypto::domain_signing::DEFAULT_CHAIN_ID;
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey, Signature};
use wrfcoin_shared_protocol::crypto::signer::{BlockSigner, SignerProvenance};
use wrfcoin_shared_protocol::security::key_file::open_owner_only_key_file;
use zeroize::Zeroizing;

/// Path of the testnet software server key file.
pub const SNAPSHOT_SERVER_KEY_FILE_ENV: &str = "WRFCOIN_SNAPSHOT_SERVER_KEY_FILE";

/// The only network a software server key is accepted on.
const TESTNET_NETWORK_ID: &str = "testnet";

/// Upper bound on the key file: 64 hex characters plus generous whitespace.
const MAX_KEY_FILE_BYTES: u64 = 4096;

/// Software ed25519 signer loaded from an owner-only key file.
pub struct SoftwareServerKeySigner {
    signing: PrivateKey,
    verifying: PublicKey,
    provenance: SignerProvenance,
}

impl std::fmt::Debug for SoftwareServerKeySigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the private half.
        f.debug_struct("SoftwareServerKeySigner")
            .field("provenance", &self.provenance)
            .field("public_key", &self.verifying.to_hex())
            .finish_non_exhaustive()
    }
}

impl BlockSigner for SoftwareServerKeySigner {
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

/// Whether this node may use a software server key at all. Pure, so the
/// testnet-only rule is unit-tested without touching the environment.
pub fn software_server_key_allowed(
    network_id: Option<&str>,
    chain_id: Option<&str>,
    hsm_configured: bool,
) -> Result<()> {
    ensure!(
        !hsm_configured,
        "{SNAPSHOT_SERVER_KEY_FILE_ENV} and WRFCOIN_HSM_MODULE_PATH are both set. The HSM is the \
         default server signer; unset one of them"
    );
    let network = network_id.map(str::trim).unwrap_or("");
    ensure!(
        network == TESTNET_NETWORK_ID,
        "{SNAPSHOT_SERVER_KEY_FILE_ENV} is testnet-only, but WRFCOIN_NETWORK_ID is '{network}'. \
         Other networks require a PKCS#11 token (WRFCOIN_HSM_*); see docs/operator/hsm-migration.md"
    );
    let chain = chain_id.map(str::trim).unwrap_or("");
    ensure!(
        !chain.is_empty() && chain != DEFAULT_CHAIN_ID,
        "{SNAPSHOT_SERVER_KEY_FILE_ENV} needs an explicit testnet WRFCOIN_CHAIN_ID (got '{chain}')"
    );
    Ok(())
}

/// Read and parse the key file. Refuses a file another account can read, a
/// symlink, an oversized file and anything but 64 hex characters.
pub fn load_software_server_key(path: &Path) -> Result<SoftwareServerKeySigner> {
    let file = open_owner_only_key_file(path).map_err(|e| anyhow!("{SNAPSHOT_SERVER_KEY_FILE_ENV}: {e}"))?;
    let mut raw = Zeroizing::new(Vec::new());
    file.take(MAX_KEY_FILE_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|e| anyhow!("{SNAPSHOT_SERVER_KEY_FILE_ENV}: cannot read {}: {e}", path.display()))?;
    ensure!(
        raw.len() as u64 <= MAX_KEY_FILE_BYTES,
        "{SNAPSHOT_SERVER_KEY_FILE_ENV}: {} is larger than a key file",
        path.display()
    );
    let text = std::str::from_utf8(&raw)
        .map_err(|_| anyhow!("{SNAPSHOT_SERVER_KEY_FILE_ENV}: {} is not text", path.display()))?
        .trim();
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!(
            "{SNAPSHOT_SERVER_KEY_FILE_ENV}: {} must hold one ed25519 seed as 64 hex characters",
            path.display()
        );
    }
    let seed = Zeroizing::new(
        hex::decode(text).map_err(|_| anyhow!("{SNAPSHOT_SERVER_KEY_FILE_ENV}: seed is not hex"))?,
    );
    let signing = PrivateKey::from_bytes(&seed)
        .map_err(|_| anyhow!("{SNAPSHOT_SERVER_KEY_FILE_ENV}: seed rejected"))?;
    let verifying = PublicKey::from_private_key(&signing);
    Ok(SoftwareServerKeySigner {
        signing,
        verifying,
        provenance: SignerProvenance::KeyFile { path: path.display().to_string() },
    })
}

/// Resolve the software server key from the environment.
///
/// `Ok(None)` when [`SNAPSHOT_SERVER_KEY_FILE_ENV`] is unset or empty (the
/// existing HSM / debug resolution then runs unchanged). `Err` is fatal for the
/// caller: a configured-but-refused key must stop the node, never fall back.
pub fn load_from_env() -> Result<Option<Arc<dyn BlockSigner>>> {
    // determinism-ok: node-local startup credential read; selects which key signs served snapshot payloads, never block validity or state-transition results
    let Some(path) = std::env::var_os(SNAPSHOT_SERVER_KEY_FILE_ENV).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    // determinism-ok: node-local startup presence check of the HSM configuration, never consensus input
    let hsm_configured = std::env::var_os(
        wrfcoin_shared_protocol::crypto::signer::env_vars::HSM_MODULE_PATH,
    )
    .is_some();
    software_server_key_allowed(
        wrfcoin_shared_protocol::crypto::domain_signing::configured_network_id().as_deref(),
        wrfcoin_shared_protocol::crypto::domain_signing::configured_chain_id().as_deref(),
        hsm_configured,
    )?;
    let signer = load_software_server_key(Path::new(&path))?;
    log::warn!(
        "[signer] TESTNET software server key active: {} (public key {}). Snapshot payloads and \
         bonded tickets are signed with a key file, not a hardware token; mainnet refuses this.",
        signer.provenance,
        signer.verifying.to_hex()
    );
    Ok(Some(Arc::new(signer)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_declared_testnet_may_use_a_software_server_key() {
        assert!(software_server_key_allowed(Some("testnet"), Some("888"), false).is_ok());
        for network in [None, Some(""), Some("mainnet"), Some("devnet"), Some("Testnet")] {
            assert!(
                software_server_key_allowed(network, Some("888"), false).is_err(),
                "network {network:?} must be refused"
            );
        }
        for chain in [None, Some(" "), Some(DEFAULT_CHAIN_ID)] {
            assert!(software_server_key_allowed(Some("testnet"), chain, false).is_err());
        }
        // HSM stays the default: both configured is a refusal, not a choice.
        assert!(software_server_key_allowed(Some("testnet"), Some("888"), true).is_err());
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
    fn key_file_loads_signs_and_refuses_loose_or_malformed_files() {
        let dir = std::env::temp_dir().join(format!("wrfcoin-server-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let seed = "11".repeat(32);

        let good = key_file(&dir, "server.key", &format!("{seed}\n"), 0o600); // secret-leak-ok: fixed all-0x11 test fixture written to a temp file, not a real key
        let signer = load_software_server_key(&good).unwrap();
        let expected = PublicKey::from_private_key(&PrivateKey::from_hex(&seed).unwrap());
        assert_eq!(signer.public_key(), expected);
        let signature = signer.sign(b"payload").unwrap();
        assert!(expected.verify(b"payload", &signature));
        assert!(!format!("{signer:?}").contains(&seed)); // secret-leak-ok: asserts the Debug output does NOT contain the fixture; no message argument

        let loose = key_file(&dir, "loose.key", &seed, 0o644);
        assert!(load_software_server_key(&loose).is_err());
        for (name, body) in [("short.key", "abcd"), ("nothex.key", &"zz".repeat(32)[..])] {
            let path = key_file(&dir, name, body, 0o600);
            let err = format!("{:#}", load_software_server_key(&path).unwrap_err());
            assert!(err.contains("64 hex characters"), "{name}: {err}");
        }
        let big = key_file(&dir, "big.key", &"a".repeat(5000), 0o600);
        assert!(load_software_server_key(&big).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
