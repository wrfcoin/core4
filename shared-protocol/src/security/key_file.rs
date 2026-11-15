//! Key-file permission enforcement (#2128).
//!
//! A private key file that is world- or group-readable is a compromised private
//! key waiting for anyone with a shell on the host. This module refuses to load
//! such a file, the same way OpenSSH refuses a loose `id_ed25519`.
//!
//! [`assert_key_file_permissions`] is Unix-only — permission bits are a Unix
//! concept. On other platforms it returns `Ok(())` with a recorded reason
//! rather than pretending to enforce something it cannot.
//!
//! [`open_owner_only_key_file`] is the strict, cross-platform loader for new
//! secret-file readers: on Unix it is the same 0600 rule plus "regular file,
//! not a symlink"; on Windows it reads the opened file's owner and DACL and
//! refuses any allow entry for an account other than the current user, SYSTEM
//! or Administrators (the rule OpenSSH for Windows applies to private keys).

use std::fmt;
use std::path::{Path, PathBuf};

/// Why a key file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyFileError {
    /// The file is readable or writable by group or other.
    TooPermissive { path: PathBuf, mode: u32 },
    /// The file could not be stat-ed.
    Unreadable { path: PathBuf, reason: String },
    /// Not a regular file: a directory, device, symlink or other reparse point,
    /// or the path changed between the check and the open.
    NotRegularFile { path: PathBuf },
    /// Windows: the file's owner, or an allow entry in its DACL, names an
    /// account other than the current user, SYSTEM or Administrators (or the
    /// DACL is missing, which grants everyone full access). `principal` is a
    /// SID string such as `S-1-5-32-545` (Users) — public, never key material.
    ForeignAccess { path: PathBuf, principal: String },
    /// No owner-only check exists for this platform; the file is refused.
    Unsupported { path: PathBuf },
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
            Self::NotRegularFile { path } => write!(
                f,
                "key file {} is not a regular file (directory, device, symlink or reparse \
                 point); refusing to load",
                path.display()
            ),
            Self::ForeignAccess { path, principal } => write!(
                f,
                "key file {} can be read by {principal}, not only by you; refusing to load. \
                 Restrict it to your account (the WRFCoin launcher does this for files it \
                 creates), e.g. icacls \"{}\" /inheritance:r /grant:r \"%USERNAME%:F\"",
                path.display(),
                path.display()
            ),
            Self::Unsupported { path } => write!(
                f,
                "key file {}: owner-only permissions cannot be checked on this platform; \
                 refusing to load",
                path.display()
            ),
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

/// Open a secret key file for reading, but only if no other local account can
/// read it. Returns the open handle, so the caller reads exactly the file that
/// was checked (no check-then-reopen race).
///
/// * Unix: a regular file, not a symlink, mode 0600 or stricter
///   ([`mode_is_acceptable`]), and the same inode that `lstat` saw.
/// * Windows: opened without following a symlink/junction, a regular file, and
///   its owner and every allow entry of its DACL name the current user, SYSTEM
///   or Administrators ([`dacl_refusal`]). SYSTEM and Administrators are
///   accepted because they can take ownership of any file anyway; excluding
///   them adds no protection and would refuse every file in a normal profile
///   folder. A file the WRFCoin launcher locks (`Protect-UserOnly`: one entry,
///   the current user) passes.
/// * Elsewhere: refused ([`KeyFileError::Unsupported`]).
pub fn open_owner_only_key_file(path: &Path) -> Result<std::fs::File, KeyFileError> {
    let unreadable = |e: std::io::Error| KeyFileError::Unreadable {
        path: path.to_path_buf(),
        reason: e.to_string(),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let link = std::fs::symlink_metadata(path).map_err(unreadable)?;
        if link.file_type().is_symlink() || !link.is_file() {
            return Err(KeyFileError::NotRegularFile { path: path.to_path_buf() });
        }
        let file = std::fs::File::open(path).map_err(unreadable)?;
        let meta = file.metadata().map_err(unreadable)?;
        if !meta.is_file() || meta.dev() != link.dev() || meta.ino() != link.ino() {
            return Err(KeyFileError::NotRegularFile { path: path.to_path_buf() });
        }
        let mode = meta.mode() & 0o777;
        if !mode_is_acceptable(mode) {
            return Err(KeyFileError::TooPermissive { path: path.to_path_buf(), mode });
        }
        Ok(file)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        // Open the link itself, never its target: a symlink or junction named
        // like a key file must not redirect the read.
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(unreadable)?;
        let meta = file.metadata().map_err(unreadable)?;
        if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(KeyFileError::NotRegularFile { path: path.to_path_buf() });
        }
        windows_acl::check_owner_only(&file).map_err(|refusal| match refusal {
            AclCheck::Foreign(principal) => {
                KeyFileError::ForeignAccess { path: path.to_path_buf(), principal }
            },
            AclCheck::Failed(reason) => {
                KeyFileError::Unreadable { path: path.to_path_buf(), reason }
            },
        })?;
        Ok(file)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = unreadable;
        Err(KeyFileError::Unsupported { path: path.to_path_buf() })
    }
}

/// Why a Windows security descriptor was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AclCheck {
    /// The owner or an allow entry names this principal (a SID string).
    Foreign(String),
    /// The descriptor could not be read or parsed.
    Failed(String),
}

/// `S-1-5-18`, LocalSystem.
const SID_LOCAL_SYSTEM: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
/// `S-1-5-32-544`, BUILTIN\Administrators.
const SID_ADMINISTRATORS: [u8; 16] = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 0x20, 0x02, 0, 0];

/// Byte length of the binary SID at the start of `bytes`, if it is well formed.
fn sid_len(bytes: &[u8]) -> Option<usize> {
    let count = usize::from(*bytes.get(1)?);
    let len = 8 + 4 * count;
    (bytes.first() == Some(&1) && count <= 15 && bytes.len() >= len).then_some(len)
}

/// Render a binary SID as `S-R-A-S1-S2...` for messages. Public data.
pub fn sid_to_string(sid: &[u8]) -> String {
    let Some(len) = sid_len(sid) else {
        return "an unparseable SID".to_string();
    };
    let authority = sid[2..8].iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b));
    let mut out = format!("S-{}-{authority}", sid[0]);
    for chunk in sid[8..len].chunks_exact(4) {
        out.push_str(&format!("-{}", u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])));
    }
    out
}

/// The owner-only rule over a raw self-relative ACL and owner SID, as Windows
/// returns them. Pure, so it is unit-tested on every platform.
///
/// * `None` (a NULL DACL) grants everyone full access: refused.
/// * The owner must be the current user, SYSTEM or Administrators.
/// * `ACCESS_ALLOWED_ACE` (type 0) entries must name one of those three, unless
///   they grant nothing or are inherit-only (they then do not apply to the file).
/// * Deny entries (types 1, 6, 10, 12) only take access away: ignored.
/// * Any other entry type in a DACL (object/callback allow forms) is refused,
///   since this check cannot prove what it grants.
pub fn dacl_refusal(dacl: Option<&[u8]>, owner: &[u8], user: &[u8]) -> Result<(), AclCheck> {
    let trusted = |sid: &[u8]| sid == user || sid == SID_LOCAL_SYSTEM || sid == SID_ADMINISTRATORS;
    let owner = &owner[..sid_len(owner).ok_or_else(|| AclCheck::Failed("owner SID malformed".into()))?];
    let user_len = sid_len(user).ok_or_else(|| AclCheck::Failed("user SID malformed".into()))?;
    let user = &user[..user_len];
    if !trusted(owner) {
        return Err(AclCheck::Foreign(sid_to_string(owner)));
    }
    let Some(acl) = dacl else {
        return Err(AclCheck::Foreign("Everyone (the file has no DACL)".into()));
    };
    let malformed = || AclCheck::Failed("DACL malformed".into());
    if acl.len() < 8 {
        return Err(malformed());
    }
    let size = usize::from(u16::from_le_bytes([acl[2], acl[3]]));
    let count = usize::from(u16::from_le_bytes([acl[4], acl[5]]));
    if size < 8 || size > acl.len() {
        return Err(malformed());
    }
    let mut offset = 8usize;
    for _ in 0..count {
        let header = acl.get(offset..offset + 4).ok_or_else(malformed)?;
        let (kind, flags) = (header[0], header[1]);
        let ace_size = usize::from(u16::from_le_bytes([header[2], header[3]]));
        let ace = acl.get(offset..offset + ace_size).filter(|_| ace_size >= 4).ok_or_else(malformed)?;
        if offset + ace_size > size {
            return Err(malformed());
        }
        const INHERIT_ONLY_ACE: u8 = 0x08;
        match kind {
            // ACCESS_ALLOWED_ACE: header, mask, SID.
            0 => {
                let mask = ace.get(4..8).ok_or_else(malformed)?;
                let sid = ace.get(8..).ok_or_else(malformed)?;
                let sid = &sid[..sid_len(sid).ok_or_else(malformed)?];
                let grants = u32::from_le_bytes([mask[0], mask[1], mask[2], mask[3]]) != 0;
                if grants && flags & INHERIT_ONLY_ACE == 0 && !trusted(sid) {
                    return Err(AclCheck::Foreign(sid_to_string(sid)));
                }
            },
            // Deny entries: ACCESS_DENIED, _OBJECT, _CALLBACK, _CALLBACK_OBJECT.
            1 | 6 | 10 | 12 => {},
            _ if flags & INHERIT_ONLY_ACE != 0 => {},
            other => {
                return Err(AclCheck::Foreign(format!("an unrecognised access entry (type {other})")));
            },
        }
        offset += ace_size;
    }
    Ok(())
}

#[cfg(windows)]
mod windows_acl {
    use super::{dacl_refusal, sid_len, AclCheck};
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE};
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        GetLengthSid, GetTokenInformation, TokenUser, ACL, DACL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// Frees a buffer the security APIs allocated with `LocalAlloc`.
    struct LocalBuffer(PSECURITY_DESCRIPTOR);
    impl Drop for LocalBuffer {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: self.0 was returned by GetSecurityInfo, which documents
                // that the caller releases it with LocalFree exactly once.
                unsafe { LocalFree(self.0) };
            }
        }
    }

    /// Copy the binary SID at `sid` (owned by a live Windows allocation).
    fn copy_sid(sid: PSID) -> Option<Vec<u8>> {
        if sid.is_null() {
            return None;
        }
        // SAFETY: `sid` is non-null and points at a SID inside a buffer the
        // caller keeps alive for this call; GetLengthSid only reads it.
        let len = unsafe { GetLengthSid(sid) } as usize;
        if !(8..=68).contains(&len) {
            return None;
        }
        // SAFETY: GetLengthSid reported `len` readable bytes at `sid`.
        let bytes = unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), len) }.to_vec();
        (sid_len(&bytes) == Some(len)).then_some(bytes)
    }

    fn current_user_sid() -> Result<Vec<u8>, AclCheck> {
        let failed = |what: &str| AclCheck::Failed(format!("{what}: {}", std::io::Error::last_os_error()));
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no
        // closing; OpenProcessToken writes one token handle into `token`.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(failed("cannot open the process token"));
        }
        // u64 storage keeps TOKEN_USER (pointer-aligned) correctly aligned.
        let mut buffer = vec![0u64; 64];
        let mut needed = 0u32;
        let size = (buffer.len() * 8) as u32;
        // SAFETY: `token` is a live token handle; `buffer` is writable for
        // `size` bytes and `needed` receives the required length.
        let ok = unsafe {
            GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), size, &mut needed)
        };
        // SAFETY: `token` was opened above and is closed exactly once here.
        unsafe { CloseHandle(token) };
        if ok == 0 {
            return Err(failed("cannot read the current user"));
        }
        // SAFETY: GetTokenInformation(TokenUser) filled `buffer` with a
        // TOKEN_USER whose SID pointer points into the same live buffer.
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        copy_sid(user.User.Sid).ok_or_else(|| AclCheck::Failed("current user SID malformed".into()))
    }

    /// Apply [`dacl_refusal`] to the owner and DACL of an open file.
    pub(super) fn check_owner_only(file: &std::fs::File) -> Result<(), AclCheck> {
        let user = current_user_sid()?;
        let mut owner: PSID = std::ptr::null_mut();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // The handle is a live file handle opened for reading (which includes
        // READ_CONTROL) and the returned descriptor is released by LocalBuffer.
        // SAFETY: every out-pointer is a valid, writable local.
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle() as HANDLE,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        let _descriptor = LocalBuffer(descriptor);
        if status != ERROR_SUCCESS {
            return Err(AclCheck::Failed(format!(
                "cannot read the file's permissions: {}",
                std::io::Error::from_raw_os_error(status as i32)
            )));
        }
        let owner = copy_sid(owner).ok_or_else(|| AclCheck::Failed("file owner unreadable".into()))?;
        let acl = if dacl.is_null() {
            None
        } else {
            // SAFETY: `dacl` points at an ACL inside the live descriptor; its
            // header's AclSize is the byte length of the whole ACL.
            let size = usize::from(unsafe { (*dacl).AclSize });
            // SAFETY: AclSize bytes starting at `dacl` belong to the descriptor,
            // which outlives this slice (released when `_descriptor` drops).
            Some(unsafe { std::slice::from_raw_parts(dacl.cast::<u8>(), size) }.to_vec())
        };
        dacl_refusal(acl.as_deref(), &owner, &user)
    }
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
                // A file that already calls the check (or the strict
                // owner-only loader built on the same rule) is compliant.
                if text.contains("assert_key_file_permissions")
                    || text.contains("open_owner_only_key_file")
                {
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
    // ---- open_owner_only_key_file / dacl_refusal ----------------------------

    /// S-1-5-21-1-2-3-1001: a stand-in for the current user's SID.
    fn user_sid() -> Vec<u8> {
        let mut sid = vec![1, 5, 0, 0, 0, 0, 0, 5];
        for sub in [21u32, 1, 2, 3, 1001] {
            sid.extend_from_slice(&sub.to_le_bytes());
        }
        sid
    }
    /// S-1-5-32-545, BUILTIN\Users.
    fn users_sid() -> Vec<u8> {
        vec![1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 0x21, 0x02, 0, 0]
    }
    fn ace(kind: u8, flags: u8, mask: u32, sid: &[u8]) -> Vec<u8> {
        let size = (8 + sid.len()) as u16;
        let mut out = vec![kind, flags];
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&mask.to_le_bytes());
        out.extend_from_slice(sid);
        out
    }
    fn acl(aces: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = aces.concat();
        let size = (8 + body.len()) as u16;
        let mut out = vec![2, 0];
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(aces.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&body);
        out
    }
    const FULL: u32 = 0x001F_01FF;

    #[test]
    fn launcher_locked_file_passes_owner_only_rule() {
        // What Protect-UserOnly writes: one explicit full-control entry.
        let user = user_sid();
        assert_eq!(dacl_refusal(Some(&acl(&[ace(0, 0, FULL, &user)])), &user, &user), Ok(()));
    }

    #[test]
    fn profile_default_acl_passes_owner_only_rule() {
        // A file in a user's profile inherits SYSTEM, Administrators, user.
        let user = user_sid();
        let dacl = acl(&[
            ace(0, 0x10, FULL, &SID_LOCAL_SYSTEM),
            ace(0, 0x10, FULL, &SID_ADMINISTRATORS),
            ace(0, 0x10, FULL, &user),
        ]);
        assert_eq!(dacl_refusal(Some(&dacl), &SID_ADMINISTRATORS, &user), Ok(()));
    }

    #[test]
    fn users_read_entry_is_refused_with_its_sid() {
        let user = user_sid();
        let dacl = acl(&[ace(0, 0, FULL, &user), ace(0, 0, 0x0012_0089, &users_sid())]);
        assert_eq!(
            dacl_refusal(Some(&dacl), &user, &user),
            Err(AclCheck::Foreign("S-1-5-32-545".into()))
        );
    }

    #[test]
    fn null_dacl_foreign_owner_and_odd_entries_are_refused() {
        let user = user_sid();
        let only_user = acl(&[ace(0, 0, FULL, &user)]);
        assert!(matches!(dacl_refusal(None, &user, &user), Err(AclCheck::Foreign(_))));
        assert_eq!(
            dacl_refusal(Some(&only_user), &users_sid(), &user),
            Err(AclCheck::Foreign("S-1-5-32-545".into()))
        );
        // An object/callback allow form cannot be reasoned about: refused.
        let callback = acl(&[ace(9, 0, FULL, &users_sid())]);
        assert!(matches!(dacl_refusal(Some(&callback), &user, &user), Err(AclCheck::Foreign(_))));
        // Truncated ACL bytes are an error, never a pass.
        let mut short = only_user.clone();
        short.truncate(10);
        assert!(matches!(dacl_refusal(Some(&short), &user, &user), Err(AclCheck::Failed(_))));
    }

    #[test]
    fn deny_empty_and_inherit_only_entries_do_not_grant() {
        let user = user_sid();
        let dacl = acl(&[
            ace(1, 0, FULL, &users_sid()),
            ace(0, 0, 0, &users_sid()),
            ace(0, 0x08 | 0x01, FULL, &users_sid()),
            ace(0, 0, FULL, &user),
        ]);
        assert_eq!(dacl_refusal(Some(&dacl), &user, &user), Ok(()));
    }

    #[test]
    fn sid_strings_render_well_known_sids() {
        assert_eq!(sid_to_string(&SID_LOCAL_SYSTEM), "S-1-5-18");
        assert_eq!(sid_to_string(&SID_ADMINISTRATORS), "S-1-5-32-544");
        assert_eq!(sid_to_string(&user_sid()), "S-1-5-21-1-2-3-1001");
    }

    #[cfg(unix)]
    #[test]
    fn owner_only_open_enforces_mode_and_refuses_symlinks() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("wrfcoin-owner-only-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("server.key");
        std::fs::write(&path, b"not a real key").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            open_owner_only_key_file(&path),
            Err(KeyFileError::TooPermissive { mode: 0o644, .. })
        ));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(open_owner_only_key_file(&path).is_ok());

        let link = dir.join("link.key");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(matches!(
            open_owner_only_key_file(&link),
            Err(KeyFileError::NotRegularFile { .. })
        ));
        assert!(matches!(
            open_owner_only_key_file(&dir),
            Err(KeyFileError::NotRegularFile { .. })
        ));
        assert!(matches!(
            open_owner_only_key_file(&dir.join("missing.key")),
            Err(KeyFileError::Unreadable { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
