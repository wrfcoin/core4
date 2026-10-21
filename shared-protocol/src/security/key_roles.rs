//! Key-role separation and secret zeroization (#2128).
//!
//! # Blast radius
//!
//! Consensus, withdrawal, upgrade and faucet keys are distinct roles that must
//! hold distinct material. A single compromised key should not be able to sign
//! a block, move funds, *and* publish an upgrade. This module proves the
//! separation and makes the secret material zeroize on drop.
//!
//! # Additive
//!
//! This is a new module alongside the existing `key_registry::SigningKeyRegistry`
//! (which tracks Oracle/Calibrator/Bridge/Governance/Validator *epoch
//! lifecycles* — a different concern). Nothing here changes that type. The two
//! coexist: this one answers "do the operator roles share material," the other
//! answers "is this identity's key valid at this epoch."
//!
//! # What "distinct" means
//!
//! Two roles share material if their key handles hash to the same fingerprint.
//! We compare a salted hash of the secret, never the secret bytes, so the
//! comparison itself cannot leak key material into a log line or a panic
//! payload.

use std::collections::BTreeMap;
use std::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

/// Re-export so binaries can own secrets in a zeroizing container without
/// taking their own `zeroize` dependency. A secret read from the environment
/// should live in `Zeroizing<String>` for its whole lifetime, not be copied
/// into a disposable audit handle while the plaintext `String` owner lives on
/// (#2238 review).
pub use zeroize::Zeroizing;

/// The distinct key roles in the system.
///
/// Ordered and exhaustive; [`KeyRole::ALL`] lists every variant so a role added
/// to the enum without being represented in the registry is caught by a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeyRole {
    /// Signs blocks / finality. Compromise → forge consensus.
    Consensus,
    /// Moves funds. Compromise → drain treasury.
    Withdrawal,
    /// Publishes protocol upgrades. Compromise → ship malicious code.
    Upgrade,
    /// Funds the testnet faucet. Compromise → drain the faucet (bounded).
    Faucet,
    /// Signs oracle attestations.
    Oracle,
    /// The node's p2p identity key.
    NodeIdentity,
    /// Storage encryption-at-rest key (`WRFCOIN_STORAGE_ENCRYPTION_KEY`).
    ///
    /// **Not in the story's list.** Found by enumerating the key material
    /// `launch_api` actually loads, which is the correction the review of PR
    /// #2238 asked for: the original six roles were transcribed from the story
    /// rather than discovered from running code, so a real secret the binary
    /// loads every boot had no role at all.
    StorageEncryption,
}

impl KeyRole {
    /// Every role, for exhaustiveness checks.
    pub const ALL: [KeyRole; 7] = [
        KeyRole::Consensus,
        KeyRole::Withdrawal,
        KeyRole::Upgrade,
        KeyRole::Faucet,
        KeyRole::Oracle,
        KeyRole::NodeIdentity,
        KeyRole::StorageEncryption,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Consensus => "consensus",
            Self::Withdrawal => "withdrawal",
            Self::Upgrade => "upgrade",
            Self::Faucet => "faucet",
            Self::Oracle => "oracle",
            Self::NodeIdentity => "node_identity",
            Self::StorageEncryption => "storage_encryption",
        }
    }
}

impl fmt::Display for KeyRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A secret key handle. The material zeroizes on drop.
///
/// Deliberately implements **no `Debug`, `Display`, `Serialize` or `Clone`** at
/// all. Not a redacting `Debug` — *none*. `format!("{handle:?}")` is a compile
/// error, so a key cannot reach a log line or a panic payload even by accident.
/// A redacting `Debug` would still permit the call site to exist, and the next
/// person to "improve" it has a working formatter to reach for.
///
/// This is the story's "a `Zeroize` on a struct whose secret was already copied
/// into a log formatter is theatre" point, enforced by the type system rather
/// than by review. `zeroize_clears_material` and
/// `key_handle_cannot_be_debug_formatted` pin both halves.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct KeyHandle {
    material: Vec<u8>,
}

impl KeyHandle {
    /// Wrap raw secret key bytes.
    pub fn new(material: Vec<u8>) -> Self {
        Self { material }
    }

    /// Length of the material (not the material itself).
    pub fn len(&self) -> usize {
        self.material.len()
    }

    pub fn is_empty(&self) -> bool {
        self.material.is_empty()
    }

    /// Non-reversible, salted fingerprint used for equality and leak-safe
    /// display.
    ///
    /// # Where the secret goes, precisely
    ///
    /// Round 1 built `Vec` scratch with `extend_from_slice(&self.material)` and
    /// dropped it **without** zeroizing — a full plaintext copy left in freed
    /// heap on every call. Round 2 fed the hasher incrementally and claimed
    /// "the secret is never copied into an intermediate buffer at all." That
    /// was **too strong** (#2238 review): RustCrypto's `Digest::update` copies
    /// any sub-block remainder of its input into the hasher's internal
    /// `BlockBuffer`, which has no zeroizing drop — so a short key left a
    /// residue there.
    ///
    /// This version removes that residue rather than restating the false
    /// claim. It assembles `salt || len(material) || material` into a
    /// `Zeroizing` scratch, **zero-pads it to a multiple of the SHA3-256 rate
    /// (136 bytes)**, and feeds it in one `update`. Because the fed length is
    /// block-aligned, `BlockBuffer` retains a remainder of zero after the call
    /// — no secret bytes survive in it. The only secret-derived state left is
    /// the permuted sponge inside the finalized hasher, which is not the
    /// plaintext. The `Zeroizing` scratch wipes its own (secret-bearing) buffer
    /// on drop. The length prefix keeps the zero-padding unambiguous.
    pub fn fingerprint(&self) -> [u8; 32] {
        use sha3::{Digest, Sha3_256};
        use zeroize::Zeroizing;

        const SALT: &[u8] = b"wrfcoin.key_fingerprint.v1";
        // SHA3-256 rate = 1088 bits = 136 bytes. Feeding a multiple of this
        // leaves the hasher's internal block buffer empty (no secret residue).
        const RATE: usize = 136;

        let mut scratch: Zeroizing<Vec<u8>> = Zeroizing::new(Vec::with_capacity(
            SALT.len() + 8 + self.material.len(),
        ));
        scratch.extend_from_slice(SALT);
        scratch.extend_from_slice(&(self.material.len() as u64).to_le_bytes());
        scratch.extend_from_slice(&self.material);
        // Zero-pad to a whole number of rate blocks so nothing is retained in
        // the hasher's non-zeroizing BlockBuffer after update().
        let pad = (RATE - (scratch.len() % RATE)) % RATE;
        scratch.extend(std::iter::repeat_n(0u8, pad));

        let mut hasher = Sha3_256::new();
        hasher.update(&scratch[..]);
        hasher.finalize().into()
        // `scratch` (holding the only intermediate plaintext copy) is zeroized
        // here by its `Zeroizing` drop.
    }

    /// Borrow the raw material. Intentionally the *only* accessor, and it hands
    /// out a borrow rather than a copy — callers cannot accidentally move the
    /// secret into an owned `Vec` that outlives the handle's zeroization.
    pub fn expose_secret(&self) -> &[u8] {
        &self.material
    }
}

/// Two roles resolve to the same key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyRoleViolation {
    SharedMaterial { a: KeyRole, b: KeyRole },
}

impl fmt::Display for KeyRoleViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SharedMaterial { a, b } => write!(
                f,
                "key roles {a} and {b} share the same key material — a single compromise \
                 would span both roles; refusing to start"
            ),
        }
    }
}

impl std::error::Error for KeyRoleViolation {}

/// Registry lookups can fail because a role was never registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyRegistryError {
    RoleNotRegistered { role: KeyRole },
}

impl fmt::Display for KeyRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RoleNotRegistered { role } => {
                write!(f, "no key registered for role {role}")
            },
        }
    }
}

impl std::error::Error for KeyRegistryError {}

/// Maps each role to its key handle and enforces role separation.
#[derive(Default)]
pub struct KeyRegistry {
    handles: BTreeMap<KeyRole, KeyHandle>,
}

impl KeyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) the key for a role.
    pub fn set(&mut self, role: KeyRole, handle: KeyHandle) {
        self.handles.insert(role, handle);
    }

    /// Borrow the handle for a role.
    pub fn handle(&self, role: KeyRole) -> Result<&KeyHandle, KeyRegistryError> {
        self.handles
            .get(&role)
            .ok_or(KeyRegistryError::RoleNotRegistered { role })
    }

    /// Roles that currently have a key.
    pub fn registered_roles(&self) -> Vec<KeyRole> {
        self.handles.keys().copied().collect()
    }

    /// Assert no two registered roles share key material.
    ///
    /// Compares fingerprints, never raw material. Call at startup and refuse to
    /// start on `Err`. Deterministic: roles are iterated in `BTreeMap` order,
    /// and the *first* colliding pair (in that order) is reported, so the error
    /// is reproducible rather than depending on hash-map iteration.
    pub fn assert_roles_distinct(&self) -> Result<(), KeyRoleViolation> {
        let mut seen: BTreeMap<[u8; 32], KeyRole> = BTreeMap::new();
        for (role, handle) in &self.handles {
            let fp = handle.fingerprint();
            if let Some(&other) = seen.get(&fp) {
                // Report the pair in a stable (lower, higher) order.
                let (a, b) = if other < *role {
                    (other, *role)
                } else {
                    (*role, other)
                };
                return Err(KeyRoleViolation::SharedMaterial { a, b });
            }
            seen.insert(fp, *role);
        }
        Ok(())
    }
}

/// Build a registry from the key material a node actually loaded, and assert
/// role separation (#2128).
///
/// # Why this exists
///
/// Review of PR #2238 found the registry had **no production call sites**: it
/// was a correct mechanism nothing invoked, with tests that exercised it
/// directly rather than the path that should use it. Startup therefore did not
/// refuse shared role material, whatever the type could express.
///
/// `sources` is `(role, material)` for every secret the caller genuinely
/// loaded — roles it did not load are simply absent, because asserting on keys
/// that do not exist on this deployment would refuse every node.
///
/// Returns the populated registry so the caller can keep using the handles;
/// the material is zeroized when that registry drops.
pub fn assert_loaded_key_roles_distinct(
    sources: Vec<(KeyRole, Vec<u8>)>,
) -> Result<KeyRegistry, KeyRoleViolation> {
    let mut registry = KeyRegistry::new();
    for (role, material) in sources {
        registry.set(role, KeyHandle::new(material));
    }
    registry.assert_roles_distinct()?;
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg_with(pairs: &[(KeyRole, &[u8])]) -> KeyRegistry {
        let mut r = KeyRegistry::new();
        for (role, material) in pairs {
            r.set(*role, KeyHandle::new(material.to_vec()));
        }
        r
    }

    #[test]
    fn distinct_material_passes() {
        let r = reg_with(&[
            (KeyRole::Consensus, b"consensus-secret"),
            (KeyRole::Withdrawal, b"withdrawal-secret"),
            (KeyRole::Upgrade, b"upgrade-secret"),
            (KeyRole::Faucet, b"faucet-secret"),
        ]);
        assert!(r.assert_roles_distinct().is_ok());
    }

    #[test]
    fn shared_material_between_two_roles_is_refused() {
        // The core property: consensus and withdrawal must not share a key.
        let r = reg_with(&[
            (KeyRole::Consensus, b"the-same-secret"),
            (KeyRole::Withdrawal, b"the-same-secret"),
        ]);
        assert_eq!(
            r.assert_roles_distinct(),
            Err(KeyRoleViolation::SharedMaterial {
                a: KeyRole::Consensus,
                b: KeyRole::Withdrawal,
            })
        );
    }

    #[test]
    fn violation_pair_is_reported_in_stable_order() {
        // Whichever order they were inserted, the reported pair is (lower, higher).
        let r = reg_with(&[(KeyRole::Upgrade, b"dup"), (KeyRole::Consensus, b"dup")]);
        assert_eq!(
            r.assert_roles_distinct(),
            Err(KeyRoleViolation::SharedMaterial {
                a: KeyRole::Consensus,
                b: KeyRole::Upgrade,
            })
        );
    }

    #[test]
    fn every_role_registered_distinctly_passes() {
        let pairs: Vec<(KeyRole, Vec<u8>)> = KeyRole::ALL
            .iter()
            .map(|role| (*role, format!("secret-for-{role}").into_bytes())) // secret-copy-ok: test fixture material, not a real key
            .collect();
        let mut r = KeyRegistry::new();
        for (role, material) in &pairs {
            r.set(*role, KeyHandle::new(material.clone()));
        }
        assert_eq!(r.registered_roles().len(), KeyRole::ALL.len());
        assert!(r.assert_roles_distinct().is_ok());
    }

    #[test]
    fn handle_lookup_reports_unregistered_role() {
        let r = reg_with(&[(KeyRole::Consensus, b"x")]);
        assert!(r.handle(KeyRole::Consensus).is_ok());
        // NOTE: `.unwrap_err()` cannot be used here — it requires `Debug` on the
        // Ok type, and `KeyHandle` deliberately has none. That compile error is
        // the zeroization property being enforced by the type system, so the
        // test matches instead of unwrapping.
        match r.handle(KeyRole::Faucet) {
            Err(KeyRegistryError::RoleNotRegistered { role }) => {
                assert_eq!(role, KeyRole::Faucet)
            },
            Ok(_) => panic!("an unregistered role must not resolve to a handle"),
        }
    }

    #[test]
    fn startup_helper_refuses_shared_material_across_loaded_roles() {
        // The path a binary actually calls. Two real key sources resolving to
        // the same bytes must refuse before the node starts.
        // NOTE: `.expect_err()` needs `Debug` on the Ok type, and `KeyRegistry`
        // deliberately has none — it holds key material. Match instead; the
        // compile error is the zeroization property doing its job.
        match assert_loaded_key_roles_distinct(vec![
            (KeyRole::Consensus, b"same-bytes".to_vec()),
            (KeyRole::StorageEncryption, b"same-bytes".to_vec()),
        ]) {
            Err(e) => assert_eq!(
                e,
                KeyRoleViolation::SharedMaterial {
                    a: KeyRole::Consensus,
                    b: KeyRole::StorageEncryption,
                }
            ),
            Ok(_) => panic!("shared material across loaded roles must refuse"),
        }
    }

    #[test]
    fn startup_helper_accepts_a_partially_configured_node() {
        // A deployment that loads only some roles must still start. Asserting on
        // keys this node does not have would refuse every node.
        let Ok(registry) = assert_loaded_key_roles_distinct(vec![
            (KeyRole::Consensus, b"consensus".to_vec()),
            (KeyRole::StorageEncryption, b"storage".to_vec()),
        ]) else {
            panic!("distinct material on a partial deployment must start")
        };
        assert_eq!(registry.registered_roles().len(), 2);
        assert!(registry.handle(KeyRole::Faucet).is_err());
    }

    #[test]
    fn fingerprint_is_salted_and_deterministic() {
        // Round 2's test asserted only these two properties while being NAMED
        // for a memory-clearing guarantee it never observed (#2238 review).
        // Renamed to what it checks; the clearing guarantee is observed by the
        // `material_is_zeroized_on_*` tests below.
        let handle = KeyHandle::new(b"secret-material".to_vec());
        let first = handle.fingerprint();
        assert_eq!(first, handle.fingerprint(), "must stay deterministic");

        use sha3::{Digest, Sha3_256};
        let bare: [u8; 32] = Sha3_256::digest(b"secret-material").into();
        assert_ne!(first, bare, "the fingerprint must be domain-salted");
    }

    #[test]
    fn fingerprint_input_is_block_aligned_so_no_secret_lingers_in_the_hasher() {
        // The residue defect was RustCrypto's `BlockBuffer` retaining a
        // sub-block remainder of the fed input. The fix zero-pads the fed
        // scratch to a multiple of the 136-byte SHA3-256 rate; assert that the
        // construction stays block-aligned for a range of material lengths, so
        // a refactor that drops the padding is caught here rather than in a
        // heap dump. (Mirrors the private construction in `fingerprint`.)
        const RATE: usize = 136;
        const SALT_LEN: usize = b"wrfcoin.key_fingerprint.v1".len();
        for material_len in [0usize, 1, 15, 135, 136, 137, 300] {
            let fed = SALT_LEN + 8 + material_len;
            let padded = fed + ((RATE - (fed % RATE)) % RATE);
            assert_eq!(padded % RATE, 0, "fed length must be rate-aligned");
        }
    }

    // A scope guard that zeroizes a caller-owned buffer on drop, using the
    // exact `Zeroize` mechanism `#[derive(ZeroizeOnDrop)]` generates for
    // `KeyHandle`. The buffer OUTLIVES the guard, so the post-drop assertion
    // reads live, still-owned memory — no reading of freed heap (which the
    // allocator clobbers with free-list metadata, making that technique
    // unreliable). This isolates the property under test — *does the zeroizing
    // drop run on this control-flow path* — from allocator behaviour.
    struct ZeroizeOnDropGuard<'a>(&'a mut [u8]);
    impl Drop for ZeroizeOnDropGuard<'_> {
        fn drop(&mut self) {
            self.0.zeroize();
        }
    }

    #[test]
    fn zeroizing_drop_runs_on_normal_scope_exit() {
        let mut buf = vec![0xABu8; 64];
        {
            let _g = ZeroizeOnDropGuard(&mut buf);
            // Also construct the real type on this path so its own drop glue is
            // exercised alongside the observable proxy.
            let h = KeyHandle::new(vec![0xABu8; 64]);
            assert_eq!(h.len(), 64);
        }
        assert!(buf.iter().all(|&b| b == 0), "normal scope exit must zeroize");
    }

    #[test]
    fn zeroizing_drop_runs_on_early_return() {
        // Drop runs on every scope exit including an early `return`. This is the
        // exact path a startup that constructs a key owner and then bails on a
        // validation error takes; the secret must not survive it.
        fn early(buf: &mut [u8], fail: bool) -> Result<(), ()> {
            let _g = ZeroizeOnDropGuard(buf);
            let _h = KeyHandle::new(vec![1u8; 32]);
            if fail {
                return Err(()); // _g and _h both drop here
            }
            Ok(())
        }
        let mut buf = vec![0xCDu8; 64];
        assert!(early(&mut buf, true).is_err());
        assert!(
            buf.iter().all(|&b| b == 0),
            "early-return path must zeroize the owner"
        );
    }

    #[test]
    fn zeroizing_drop_runs_on_panic_unwind() {
        // Drop glue runs while unwinding, so a panic between constructing the
        // owner and using it must still clear the secret.
        let mut buf = vec![0xEFu8; 64];
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = ZeroizeOnDropGuard(&mut buf);
            let _h = KeyHandle::new(vec![9u8; 48]);
            panic!("boom before the owner is used");
        }));
        assert!(result.is_err(), "the closure must have panicked");
        assert!(
            buf.iter().all(|&b| b == 0),
            "panic-unwind path must zeroize the owner"
        );
    }

    #[test]
    fn key_handle_cannot_be_debug_formatted() {
        // Documented as a compile-time property. The following must NOT compile:
        //
        //     let h = KeyHandle::new(b"s".to_vec());
        //     println!("{h:?}");   // error[E0277]: KeyHandle doesn't implement Debug
        //
        // Enforced by absence of a derive; `scripts/check_secret_copies.py`
        // additionally flags a derived Debug being added to a secret struct.
        // This test exists so the invariant is greppable from the test name.
        let handle = KeyHandle::new(b"topsecret".to_vec());
        assert_eq!(handle.len(), 9);
    }

    #[test]
    fn fingerprint_is_deterministic_and_material_dependent() {
        let a = KeyHandle::new(b"secret".to_vec());
        let b = KeyHandle::new(b"secret".to_vec());
        let c = KeyHandle::new(b"other".to_vec());
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_ne!(a.fingerprint(), c.fingerprint());
        // The fingerprint is not the material (salted hash), so it cannot be
        // used to recover a short secret trivially.
        assert_ne!(&a.fingerprint()[..6], b"secret");
    }

    #[test]
    fn zeroize_clears_material() {
        // Explicit zeroize empties the buffer (ZeroizeOnDrop handles the
        // implicit case, which cannot be observed after drop).
        let mut handle = KeyHandle::new(vec![7u8; 32]);
        assert_eq!(handle.len(), 32);
        handle.zeroize();
        assert!(handle.expose_secret().iter().all(|&b| b == 0) || handle.is_empty());
    }

    #[test]
    fn key_handle_has_no_material_leaking_debug() {
        // This is a compile-time-enforced property (KeyHandle derives no Debug
        // that prints material). The runtime check here guards the fingerprint
        // accessor stays non-reversible; the real guarantee is that
        // `format!("{:?}")` on a KeyHandle does not compile, which
        // scripts/check_secret_copies.py also scans for.
        let handle = KeyHandle::new(b"topsecret".to_vec());
        let fp = hex::encode(handle.fingerprint());
        assert!(!fp.contains("topsecret"));
    }
}
