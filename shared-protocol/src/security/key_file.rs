//! Key-file permission enforcement (#2128).
//!
//! A private key file that is world- or group-readable is a compromised private
//! key waiting for anyone with a shell on the host. This module refuses to load
//! such a file, the same way OpenSSH refuses a loose `id_ed25519`.
//!
//! The check is Unix-only — permission bits are a Unix concept. On other
//! platforms [`assert_key_file_permissions`] returns `Ok(())` with a recorded
//! reason rather than pretending to enforce something it cannot.

use std::fmt;
use std::path::{Path, PathBuf};

/// Why a key file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyFileError {
    /// The file is readable or writable by group or other.
    TooPermissive { path: PathBuf, mode: u32 },
    /// The file could not be stat-ed.
    Unreadable { path: PathBuf, reason: String },
}

impl fmt::Display for KeyFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooPermissive { path, mode } => write!(
                f,
                "key file {} has mode {:o}, looser than 0600 — group/other can read it; \
                 refusing to load. Run: chmod 600 {}",
                path.display(),
                mode,
                path.display()
            ),
            Self::Unreadable { path, reason } => {
                write!(f, "cannot stat key file {}: {reason}", path.display())
            },
        }
    }
}

impl std::error::Error for KeyFileError {}

/// Any permission bit outside `0600` (owner read+write). Group and other bits,
/// plus owner-execute, are all disallowed for a secret key file.
const DISALLOWED_MASK: u32 = 0o177;

/// Whether a raw Unix mode is acceptable for a private key file.
///
/// Pure and platform-independent so it can be unit-tested everywhere, even
/// though [`assert_key_file_permissions`] only enforces it on Unix.
pub fn mode_is_acceptable(mode: u32) -> bool {
    mode & DISALLOWED_MASK == 0
}

/// Refuse to load a key file whose permissions are looser than 0600.
///
/// On non-Unix platforms this is a no-op returning `Ok(())` — permission bits
/// are not portable, and failing closed there would block every non-Unix build
/// from loading any key.
#[cfg(unix)]
pub fn assert_key_file_permissions(path: &Path) -> Result<(), KeyFileError> {
    use std::os::unix::fs::PermissionsExt;

    let meta = std::fs::metadata(path).map_err(|e| KeyFileError::Unreadable {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    let mode = meta.permissions().mode() & 0o777;
    if mode_is_acceptable(mode) {
        Ok(())
    } else {
        Err(KeyFileError::TooPermissive {
            path: path.to_path_buf(),
            mode,
        })
    }
}

/// Non-Unix stub — see the Unix version's docs.
#[cfg(not(unix))]
pub fn assert_key_file_permissions(_path: &Path) -> Result<(), KeyFileError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review of PR #2238 asked for `assert_key_file_permissions` to be called
    /// from every applicable file-backed key load. The round-1 version of this
    /// test scanned only `native-chain/src` and `shared-protocol/src` and
    /// concluded "no loader exists" — but the real file-backed private-key
    /// loader is `p2p/src/identity.rs::load_or_generate_keypair`, which reads
    /// `peer_identity.key`. It was invisible because the scanned root set
    /// omitted `p2p/src`. A clean scan over the wrong roots is not a clean scan.
    ///
    /// That loader now calls the permission check (verified separately by
    /// `refuses_preexisting_group_readable_key_file` in the p2p crate). This
    /// test roots the guard: it scans the three Rust source trees that hold
    /// key-loading code and fails if any file-backed key load is added without
    /// wiring in the check — a `mod tests` self-match is excluded so a file
    /// that merely *mentions* the pattern in an assertion is not a false hit.
    #[test]
    fn no_file_backed_key_load_exists_without_the_permission_check() {
        use std::path::Path;

        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");

        // File reads whose surrounding line also names key material.
        let read_pattern = ["read_to_string", "fs::read", "File::open"];
        let key_words = [
            "private_key",
            "signing_key",
            "secret_key",
            "keyfile",
            "key_file",
            "keypair",
            "identity.key",
        ];

        let mut offenders: Vec<String> = Vec::new();
        let mut stack = vec![
            root.join("native-chain/src"),
            root.join("shared-protocol/src"),
            root.join("p2p/src"),
        ];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                // A file that already calls the check is compliant.
                if text.contains("assert_key_file_permissions") {
                    continue;
                }
                // FILE granularity, not line. The round-1 version required the
                // read and a key word on the SAME line, so it missed
                // `let bytes = std::fs::read(path)?;` in a `load_..._keypair`
                // function whose key-ness is stated in the fn name, the doc,
                // and the `identity.key` path literal — never on the read line
                // (#2238 review). A file that both reads from disk AND names
                // key material anywhere, without calling the check, is flagged.
                // This over-flags (a file reading a non-key file that merely
                // mentions "private_key" in a comment must add an
                // acknowledgement) — the safe direction for a guard: a false
                // positive is a one-line annotation, a false negative is a
                // silently loaded world-readable key.
                let lower = text.to_ascii_lowercase();
                let reads_a_file = read_pattern.iter().any(|p| text.contains(p));
                let names_key_material = key_words.iter().any(|k| lower.contains(k));
                if !(reads_a_file && names_key_material) {
                    continue;
                }
                // Report the specific read line(s) so the finding is
                // actionable. A read carrying a trailing `// key-file-exempt:
                // <reason>` is a triaged non-private-key read (e.g. a ZK
                // circuit parameter file or a JSON cursor) and is skipped — the
                // reason is required and visible in the diff, so an exemption is
                // a deliberate, reviewable act, never a silent one.
                for (idx, line) in text.lines().enumerate() {
                    if read_pattern.iter().any(|p| line.contains(p))
                        && !line.contains("key-file-exempt:")
                    {
                        offenders.push(format!(
                            "{}:{}: {}",
                            path.strip_prefix(root).unwrap_or(&path).display(),
                            idx + 1,
                            line.trim()
                        ));
                    }
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "a file-backed key load exists without calling \
             assert_key_file_permissions — a world- or group-readable key file \
             would be loaded silently:\n{}",
            offenders.join("\n")
        );
    }

    #[test]
    fn only_0600_and_0400_are_acceptable() {
        assert!(mode_is_acceptable(0o600));
        assert!(mode_is_acceptable(0o400));
        // Owner-execute, group, other — all rejected.
        assert!(!mode_is_acceptable(0o700));
        assert!(!mode_is_acceptable(0o640));
        assert!(!mode_is_acceptable(0o604));
        assert!(!mode_is_acceptable(0o660));
        assert!(!mode_is_acceptable(0o644));
        assert!(!mode_is_acceptable(0o777));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_group_readable_key_file() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir();
        let path = dir.join(format!("wrfcoin-key-test-{}.key", std::process::id()));
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"secret").unwrap();
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

        let err = assert_key_file_permissions(&path).unwrap_err();
        assert!(matches!(err, KeyFileError::TooPermissive { mode, .. } if mode == 0o640));

        // And 0600 is accepted.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(assert_key_file_permissions(&path).is_ok());

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn missing_file_is_unreadable_not_a_panic() {
        let path = Path::new("/nonexistent/wrfcoin/key/definitely-not-here.key");
        assert!(matches!(
            assert_key_file_permissions(path),
            Err(KeyFileError::Unreadable { .. })
        ));
    }
}
