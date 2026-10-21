//! Peer identity persistence.
//!
//! Saves and loads a libp2p ed25519 keypair so that the node's `PeerId`
//! is stable across restarts.

use std::path::Path;

use libp2p::identity::Keypair;
use log::info;

use crate::error::{P2pError, P2pResult};

/// Load a keypair from `path` if the file exists, otherwise generate a new
/// ed25519 keypair and persist it to `path`.
///
/// The file is written with mode `0600` (owner read/write only) because it
/// contains a private key.
pub fn load_or_generate_keypair(path: &Path) -> P2pResult<Keypair> {
    if path.exists() {
        // The file was generated 0600 (below), but permissions are mutable:
        // a later `chmod 644` on an existing key is exactly what OpenSSH
        // refuses on `id_ed25519`, and what generating the file correctly
        // once does NOT catch (#2238 review). Validate on every load.
        wrfcoin_shared_protocol::security::key_file::assert_key_file_permissions(path)
            .map_err(|e| P2pError::Config(e.to_string()))?;
        let bytes = std::fs::read(path)?;
        let keypair = Keypair::from_protobuf_encoding(&bytes).map_err(|e| {
            P2pError::Config(format!(
                "failed to decode keypair from {}: {e}",
                path.display()
            ))
        })?;
        let peer_id = keypair.public().to_peer_id();
        info!("Loaded peer ID: {peer_id}");
        Ok(keypair)
    } else {
        let keypair = Keypair::generate_ed25519();
        let bytes = keypair.to_protobuf_encoding().map_err(|e| {
            P2pError::Config(format!("failed to encode keypair: {e}"))
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        let peer_id = keypair.public().to_peer_id();
        info!("Generated new peer ID: {peer_id}");
        Ok(keypair)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_then_reload_preserves_peer_id() {
        let dir =
            std::env::temp_dir().join(format!("wrfcoin-identity-test-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("peer_identity.key");

        let kp1 = load_or_generate_keypair(&path).unwrap();
        assert!(path.exists());

        let kp2 = load_or_generate_keypair(&path).unwrap();
        assert_eq!(kp1.public().to_peer_id(), kp2.public().to_peer_id());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupted_file_returns_error() {
        let dir =
            std::env::temp_dir().join(format!("wrfcoin-identity-test-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("peer_identity.key");

        std::fs::write(&path, b"not a valid protobuf keypair").unwrap();
        assert!(load_or_generate_keypair(&path).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The #2238 review's precise case: a *pre-existing* key file that is
    /// valid but has been loosened to group-readable must be refused on load,
    /// not silently accepted. Generating a fresh 0600 file (tested above) does
    /// not exercise this — permissions on an existing file can change after
    /// creation.
    #[cfg(unix)]
    #[test]
    fn refuses_preexisting_group_readable_key_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir =
            std::env::temp_dir().join(format!("wrfcoin-identity-test-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("peer_identity.key");

        // First create a real, valid, correctly-permissioned key file.
        let kp = load_or_generate_keypair(&path).unwrap();
        // Then loosen it, as a careless operator or a bad umask would.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        // The next load must refuse — the key content is still valid, so only
        // the permission check can catch this.
        let err = load_or_generate_keypair(&path).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("looser than 0600") || msg.contains("refusing to load"),
            "expected a permission refusal, got: {msg}"
        );

        // Restoring 0600 makes the same file loadable again, proving it was the
        // mode and not the content that was rejected.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let kp2 = load_or_generate_keypair(&path).unwrap();
        assert_eq!(kp.public().to_peer_id(), kp2.public().to_peer_id());

        std::fs::remove_dir_all(&dir).ok();
    }
}
