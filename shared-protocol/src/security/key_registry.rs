use crate::crypto::signature::{PublicKey, Signature};
use anyhow::{Result, anyhow};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

/// Signing roles tracked for key lifecycle and revocation checks.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SigningRole {
    Oracle,
    Calibrator,
    BridgeOperator,
    GovernanceOperator,
    Validator,
}

/// One key epoch entry for an identity and role.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyEpochRecord {
    pub epoch: u64,
    pub public_key: String,
    pub activated_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoleKeyLifecycle {
    epochs: Vec<KeyEpochRecord>,
}

impl RoleKeyLifecycle {
    pub fn epochs(&self) -> &[KeyEpochRecord] {
        &self.epochs
    }

    fn insert_epoch(&mut self, epoch: u64, public_key: String, activated_at: i64) -> Result<()> {
        if self.epochs.iter().any(|entry| entry.epoch == epoch) {
            return Err(anyhow!("key epoch {epoch} already exists"));
        }
        if PublicKey::from_hex(&public_key).is_err() {
            return Err(anyhow!("public key must be valid ed25519 hex"));
        }
        self.epochs.push(KeyEpochRecord {
            epoch,
            public_key,
            activated_at,
            revoked_at: None,
        });
        self.epochs.sort_by_key(|entry| entry.epoch);
        Ok(())
    }

    fn revoke_epoch(&mut self, epoch: u64, revoked_at: i64) -> Result<()> {
        let Some(entry) = self.epochs.iter_mut().find(|entry| entry.epoch == epoch) else {
            return Err(anyhow!("key epoch {epoch} not found"));
        };
        if let Some(existing) = entry.revoked_at {
            return Err(anyhow!("key epoch {epoch} already revoked at {existing}"));
        }
        if revoked_at < entry.activated_at {
            return Err(anyhow!(
                "revoked_at {} cannot be before activated_at {}",
                revoked_at,
                entry.activated_at
            ));
        }
        entry.revoked_at = Some(revoked_at);
        Ok(())
    }

    fn is_key_valid_at_epoch(&self, public_key: &str, epoch: u64, signed_at: i64) -> Option<bool> {
        let entry = self
            .epochs
            .iter()
            .find(|entry| entry.epoch == epoch && entry.public_key == public_key)?;
        if signed_at < entry.activated_at {
            return Some(false);
        }
        if let Some(revoked_at) = entry.revoked_at {
            return Some(signed_at < revoked_at);
        }
        Some(true)
    }

    fn active_keys_at(&self, signed_at: i64) -> Vec<(u64, String)> {
        self.epochs
            .iter()
            .filter(|entry| {
                signed_at >= entry.activated_at
                    && entry
                        .revoked_at
                        .map(|revoked_at| signed_at < revoked_at)
                        .unwrap_or(true)
            })
            .map(|entry| (entry.epoch, entry.public_key.clone()))
            .collect()
    }
}

/// Registry of signing key epochs and revocations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SigningKeyRegistry {
    /// identity -> role -> lifecycle
    entries: FxHashMap<String, FxHashMap<SigningRole, RoleKeyLifecycle>>,
}

impl SigningKeyRegistry {
    /// Retired keys remain reserved for their original consensus roles.
    pub fn all_registered_public_keys(&self) -> std::collections::BTreeSet<String> {
        // determinism-ok: collected into a BTreeSet before consumption; source traversal order cannot change the canonical set
        self.entries.values().flat_map(|roles| roles.values())
            .flat_map(|lifecycle| lifecycle.epochs.iter().map(|epoch| epoch.public_key.clone()))
            .collect()
    }
    /// Seed legacy mode where identity string is also the first signing key.
    pub fn ensure_legacy_identity_key(
        &mut self,
        identity: &str,
        role: SigningRole,
        activated_at: i64,
    ) -> Result<()> {
        let role_map = self.entries.entry(identity.to_string()).or_default();
        let lifecycle = role_map.entry(role).or_default();
        if lifecycle.epochs().iter().any(|entry| entry.epoch == 0) {
            return Ok(());
        }
        lifecycle.insert_epoch(0, identity.to_string(), activated_at)
    }

    pub fn register_key_epoch(
        &mut self,
        identity: &str,
        role: SigningRole,
        epoch: u64,
        public_key: String,
        activated_at: i64,
    ) -> Result<()> {
        let role_map = self.entries.entry(identity.to_string()).or_default();
        let lifecycle = role_map.entry(role).or_default();
        lifecycle.insert_epoch(epoch, public_key, activated_at)
    }

    pub fn revoke_key_epoch(
        &mut self,
        identity: &str,
        role: SigningRole,
        epoch: u64,
        revoked_at: i64,
    ) -> Result<()> {
        let Some(role_map) = self.entries.get_mut(identity) else {
            return Err(anyhow!("identity {identity} not found"));
        };
        let Some(lifecycle) = role_map.get_mut(&role) else {
            return Err(anyhow!("no key lifecycle for requested role"));
        };
        lifecycle.revoke_epoch(epoch, revoked_at)
    }

    pub fn is_key_valid_at_epoch(
        &self,
        identity: &str,
        role: SigningRole,
        public_key: &str,
        epoch: u64,
        signed_at: i64,
    ) -> bool {
        let Some(role_map) = self.entries.get(identity) else {
            return identity == public_key && epoch == 0;
        };
        let Some(lifecycle) = role_map.get(&role) else {
            return identity == public_key && epoch == 0;
        };
        lifecycle
            .is_key_valid_at_epoch(public_key, epoch, signed_at)
            .unwrap_or(false)
    }

    pub fn active_keys_at(
        &self,
        identity: &str,
        role: SigningRole,
        signed_at: i64,
    ) -> Vec<(u64, String)> {
        let Some(role_map) = self.entries.get(identity) else {
            return vec![(0, identity.to_string())];
        };
        let Some(lifecycle) = role_map.get(&role) else {
            return vec![(0, identity.to_string())];
        };
        lifecycle.active_keys_at(signed_at)
    }

    /// Returns `true` only if `identity` has an *explicitly registered* key
    /// lifecycle for `role`.
    ///
    /// This deliberately does NOT trigger the legacy identity-as-public-key
    /// fallback that [`Self::active_keys_at`] / [`Self::verify_signature_for_identity_role`]
    /// use for unknown identities. Callers that must fail closed against
    /// self-certifying (forged) signatures — e.g. oracle-attestation mempool
    /// admission, see #1723 — use this to require that the signer is a known,
    /// registered operator before accepting a signature.
    pub fn has_identity_role(&self, identity: &str, role: SigningRole) -> bool {
        self.entries
            .get(identity)
            .map(|role_map| role_map.contains_key(&role))
            .unwrap_or(false)
    }

    pub fn verify_signature_for_identity_role(
        &self,
        identity: &str,
        role: SigningRole,
        signed_at: i64,
        message: &[u8],
        signature_hex: &str,
    ) -> bool {
        let signature = match Signature::from_hex(signature_hex) {
            Ok(sig) => sig,
            Err(_) => return false,
        };
        self.active_keys_at(identity, role, signed_at)
            .into_iter()
            .any(|(_epoch, key_hex)| {
                PublicKey::from_hex(&key_hex)
                    .map(|pk| pk.verify(message, &signature))
                    .unwrap_or(false)
            })
    }

    pub fn verify_signature_for_identity_role_epoch(
        &self,
        identity: &str,
        role: SigningRole,
        epoch: u64,
        signed_at: i64,
        message: &[u8],
        signature_hex: &str,
    ) -> bool {
        let signature = match Signature::from_hex(signature_hex) {
            Ok(sig) => sig,
            Err(_) => return false,
        };

        let key_hex = if let Some(role_map) = self.entries.get(identity) {
            if let Some(lifecycle) = role_map.get(&role) {
                let Some(entry) = lifecycle.epochs().iter().find(|entry| entry.epoch == epoch)
                else {
                    return false;
                };
                if signed_at < entry.activated_at {
                    return false;
                }
                if entry
                    .revoked_at
                    .map(|revoked_at| signed_at >= revoked_at)
                    .unwrap_or(false)
                {
                    return false;
                }
                entry.public_key.clone()
            } else if epoch == 0 {
                identity.to_string()
            } else {
                return false;
            }
        } else if epoch == 0 {
            identity.to_string()
        } else {
            return false;
        };

        PublicKey::from_hex(&key_hex)
            .map(|pk| pk.verify(message, &signature))
            .unwrap_or(false)
    }

    pub fn snapshot_for_role(&self, role: SigningRole) -> Vec<(String, Vec<KeyEpochRecord>)> {
        let mut out = self
            .entries
            .iter()
            .filter_map(|(identity, per_role)| {
                per_role
                    .get(&role)
                    .map(|lifecycle| (identity.clone(), lifecycle.epochs().to_vec()))
            })
            .collect::<Vec<_>>();
        out.sort_by(|(a, _), (b, _)| a.cmp(b));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::signature::{PrivateKey, PublicKey};
    use rand::{SeedableRng, rngs::StdRng};

    fn key_hexes() -> (String, String) {
        let mut rng = StdRng::seed_from_u64(42);
        let key0 = PrivateKey::generate(&mut rng);
        let key1 = PrivateKey::generate(&mut rng);
        (
            PublicKey::from_private_key(&key0).to_string(),
            PublicKey::from_private_key(&key1).to_string(),
        )
    }

    #[test]
    fn epoch_registration_and_revocation_work() {
        let mut registry = SigningKeyRegistry::default();
        let (identity, replacement) = key_hexes();

        registry
            .ensure_legacy_identity_key(&identity, SigningRole::Oracle, 100)
            .unwrap();
        registry
            .register_key_epoch(&identity, SigningRole::Oracle, 1, replacement.clone(), 200)
            .unwrap();
        assert!(registry.is_key_valid_at_epoch(
            &identity,
            SigningRole::Oracle,
            &replacement,
            1,
            250
        ));

        registry
            .revoke_key_epoch(&identity, SigningRole::Oracle, 1, 260)
            .unwrap();
        assert!(!registry.is_key_valid_at_epoch(
            &identity,
            SigningRole::Oracle,
            &replacement,
            1,
            270
        ));
    }

    #[test]
    fn active_keys_filter_by_time_and_revocation() {
        let mut registry = SigningKeyRegistry::default();
        let (identity, replacement) = key_hexes();
        registry
            .ensure_legacy_identity_key(&identity, SigningRole::Oracle, 100)
            .unwrap();
        registry
            .register_key_epoch(&identity, SigningRole::Oracle, 1, replacement, 200)
            .unwrap();

        let before_rotation = registry.active_keys_at(&identity, SigningRole::Oracle, 150);
        assert_eq!(before_rotation.len(), 1);
        assert_eq!(before_rotation[0].0, 0);

        let during_overlap = registry.active_keys_at(&identity, SigningRole::Oracle, 220);
        assert_eq!(during_overlap.len(), 2);

        registry
            .revoke_key_epoch(&identity, SigningRole::Oracle, 1, 230)
            .unwrap();
        let after_revocation = registry.active_keys_at(&identity, SigningRole::Oracle, 235);
        assert_eq!(after_revocation.len(), 1);
        assert_eq!(after_revocation[0].0, 0);
    }

    #[test]
    fn epoch_signature_verification_respects_revocation_window() {
        let mut registry = SigningKeyRegistry::default();
        let mut rng = StdRng::seed_from_u64(101);
        let legacy_key = PrivateKey::generate(&mut rng);
        let rotated_key = PrivateKey::generate(&mut rng);
        let identity = PublicKey::from_private_key(&legacy_key).to_string();
        let rotated_public = PublicKey::from_private_key(&rotated_key).to_string();

        registry
            .ensure_legacy_identity_key(&identity, SigningRole::Calibrator, 10)
            .unwrap();
        registry
            .register_key_epoch(&identity, SigningRole::Calibrator, 1, rotated_public, 100)
            .unwrap();

        let message = b"calibration-attestation-message";
        let signature = rotated_key.sign(message).to_string();
        assert!(registry.verify_signature_for_identity_role_epoch(
            &identity,
            SigningRole::Calibrator,
            1,
            150,
            message,
            &signature
        ));

        registry
            .revoke_key_epoch(&identity, SigningRole::Calibrator, 1, 160)
            .unwrap();
        assert!(!registry.verify_signature_for_identity_role_epoch(
            &identity,
            SigningRole::Calibrator,
            1,
            170,
            message,
            &signature
        ));
    }

    // ── #1735 slice 2c: signing failure paths ───────────────────────────────
    //
    // The oracle-attestation admission path (#1723, pool.rs ~l755) calls
    // `has_identity_role` to fail-closed: if the signing key is not explicitly
    // registered for the Oracle role, the attestation is rejected outright.
    // These tests pin the edge-cases of that check and the error paths of the
    // key-lifecycle management API.

    #[test]
    fn has_identity_role_fail_closed_for_unregistered_identity() {
        // An identity that has never been registered must return false for any role.
        // This is the primary fail-closed gate for oracle-attestation admission.
        let registry = SigningKeyRegistry::default();
        assert!(
            !registry.has_identity_role("unknown-oracle-id", SigningRole::Oracle),
            "unregistered identity must NOT have Oracle role (fail-closed admission)"
        );
        assert!(
            !registry.has_identity_role("unknown-id", SigningRole::Validator),
            "unregistered identity must NOT have Validator role"
        );
    }

    #[test]
    fn has_identity_role_registered_identity_returns_true() {
        // Counterpart: a registered oracle identity must return true.
        let mut registry = SigningKeyRegistry::default();
        let mut rng = StdRng::seed_from_u64(0x1735_5010);
        let key = PrivateKey::generate(&mut rng);
        let pk_hex = PublicKey::from_private_key(&key).to_string();

        registry
            .ensure_legacy_identity_key(&pk_hex, SigningRole::Oracle, 0)
            .unwrap();

        assert!(
            registry.has_identity_role(&pk_hex, SigningRole::Oracle),
            "registered Oracle identity must return true"
        );
        assert!(
            !registry.has_identity_role(&pk_hex, SigningRole::Validator),
            "Oracle registration must NOT grant Validator role"
        );
    }

    #[test]
    fn all_epochs_revoked_leaves_no_active_keys() {
        // When all key epochs for an identity/role are revoked, `active_keys_at`
        // must return an empty vec.  A caller using `active_keys_at` to obtain
        // verification keys then fails to verify any signature — a safe shutdown
        // of the oracle signing path when the operator rotates out.
        let mut registry = SigningKeyRegistry::default();
        let mut rng = StdRng::seed_from_u64(0x1735_5011);
        let key = PrivateKey::generate(&mut rng);
        let pk_hex = PublicKey::from_private_key(&key).to_string();

        // Register epoch 0 (legacy) and immediately revoke it.
        registry
            .ensure_legacy_identity_key(&pk_hex, SigningRole::Oracle, 100)
            .unwrap();
        registry
            .revoke_key_epoch(&pk_hex, SigningRole::Oracle, 0, 200)
            .unwrap();

        // At timestamp 250 (after revocation) there must be NO active keys.
        let active = registry.active_keys_at(&pk_hex, SigningRole::Oracle, 250);
        assert!(
            active.is_empty(),
            "revoking all epochs must leave no active keys; got: {active:?}"
        );
    }

    #[test]
    fn duplicate_epoch_registration_is_rejected() {
        // Registering the same epoch number twice for the same identity/role
        // must return an error.  This prevents accidental key clobber or
        // replay of a stale epoch.
        let mut registry = SigningKeyRegistry::default();
        let mut rng = StdRng::seed_from_u64(0x1735_5012);
        let key = PrivateKey::generate(&mut rng);
        let pk_hex = PublicKey::from_private_key(&key).to_string();

        registry
            .ensure_legacy_identity_key(&pk_hex, SigningRole::Oracle, 100)
            .unwrap();

        // Calling ensure_legacy_identity_key again for the same identity/role
        // must be idempotent (no error, but also no duplicate).
        registry
            .ensure_legacy_identity_key(&pk_hex, SigningRole::Oracle, 200)
            .expect("idempotent re-seed must succeed");

        // Directly registering epoch 0 again must fail.
        let another_key = PrivateKey::generate(&mut rng);
        let another_hex = PublicKey::from_private_key(&another_key).to_string();
        let err = registry
            .register_key_epoch(&pk_hex, SigningRole::Oracle, 0, another_hex, 300)
            .expect_err("re-registering epoch 0 must fail");
        assert!(
            err.to_string().contains("already exists"),
            "duplicate-epoch error must mention 'already exists', got: {err}"
        );
    }

    #[test]
    fn revoke_nonexistent_epoch_fails_gracefully() {
        // Trying to revoke an epoch that was never registered must return an
        // error rather than panicking or silently succeeding.
        let mut registry = SigningKeyRegistry::default();
        let mut rng = StdRng::seed_from_u64(0x1735_5013);
        let key = PrivateKey::generate(&mut rng);
        let pk_hex = PublicKey::from_private_key(&key).to_string();

        registry
            .ensure_legacy_identity_key(&pk_hex, SigningRole::Oracle, 100)
            .unwrap();

        // Revoking an epoch that does not exist (epoch=99) must return an error.
        let err = registry
            .revoke_key_epoch(&pk_hex, SigningRole::Oracle, 99, 200)
            .expect_err("revoking a non-existent epoch must return an error");
        assert!(
            err.to_string().contains("not found"),
            "missing-epoch error must mention 'not found', got: {err}"
        );
    }
}
