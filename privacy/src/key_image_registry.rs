//! Key-image registry for ring-signature double-spend prevention.
//!
//! Each ring signature produces a unique **key image** derived from the
//! signer's private key.  If the same key image appears in two different
//! transactions, the second transaction is a double-spend and must be
//! rejected.
//!
//! This module provides a thread-safe, in-memory registry backed by
//! [`RwLock`] and [`FxHashSet`] for fast constant-time lookups.
//!
//! # Thread safety
//!
//! The registry is safe to share across threads via `Arc<KeyImageRegistry>`.
//! Reads (`is_spent`, `count`) take a shared lock; writes (`register`) take
//! an exclusive lock.

use std::sync::RwLock;

use rustc_hash::FxHashSet;

use crate::error::{PrivacyError, PrivacyResult};
use crate::ring_signatures::KeyImage;

/// Thread-safe registry of spent key images.
///
/// Maintains a set of 32-byte key image identifiers.  Once registered, a key
/// image cannot be registered again -- the second attempt returns
/// [`PrivacyError::DoubleSpend`].
#[derive(Debug)]
pub struct KeyImageRegistry {
    /// Interior-mutable set protected by a read-write lock.
    spent: RwLock<FxHashSet<[u8; 32]>>,
}

impl KeyImageRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            spent: RwLock::new(FxHashSet::default()),
        }
    }

    /// Register a key image as spent.
    ///
    /// # Errors
    ///
    /// Returns [`PrivacyError::DoubleSpend`] if `key_image` has already been
    /// registered.
    pub fn register(&self, key_image: &KeyImage) -> PrivacyResult<()> {
        let mut set = self
            .spent
            .write()
            .map_err(|_| PrivacyError::Other("key image registry lock poisoned".into()))?;

        if !set.insert(key_image.image) {
            return Err(PrivacyError::DoubleSpend);
        }

        Ok(())
    }

    /// Check whether a key image has already been spent.
    pub fn is_spent(&self, key_image: &KeyImage) -> bool {
        self.spent
            .read()
            .map(|set| set.contains(&key_image.image))
            .unwrap_or(false)
    }

    /// Return the number of key images currently registered.
    pub fn count(&self) -> usize {
        self.spent.read().map(|set| set.len()).unwrap_or(0)
    }
}

impl Default for KeyImageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use curve25519_dalek::scalar::Scalar;
    use rand_core::{OsRng, RngCore};

    fn generate_key_image() -> KeyImage {
        let mut priv_bytes = [0u8; 64];
        OsRng.fill_bytes(&mut priv_bytes);
        let scalar = Scalar::from_bytes_mod_order_wide(&priv_bytes);
        let private_key = scalar.to_bytes();
        KeyImage::generate(&private_key).unwrap()
    }

    #[test]
    fn register_and_check() {
        let registry = KeyImageRegistry::new();
        let ki = generate_key_image();

        assert!(!registry.is_spent(&ki));
        assert_eq!(registry.count(), 0);

        registry.register(&ki).unwrap();

        assert!(registry.is_spent(&ki));
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn double_spend_rejected() {
        let registry = KeyImageRegistry::new();
        let ki = generate_key_image();

        registry.register(&ki).unwrap();
        let result = registry.register(&ki);
        assert!(result.is_err());
        match result.unwrap_err() {
            PrivacyError::DoubleSpend => {} // expected
            other => panic!("expected DoubleSpend, got: {:?}", other),
        }
    }

    #[test]
    fn distinct_key_images_accepted() {
        let registry = KeyImageRegistry::new();

        for _ in 0..10 {
            let ki = generate_key_image();
            registry.register(&ki).unwrap();
        }

        assert_eq!(registry.count(), 10);
    }

    #[test]
    fn concurrent_registration() {
        use std::sync::Arc;
        use std::thread;

        let registry = Arc::new(KeyImageRegistry::new());
        let mut handles = Vec::new();

        for _ in 0..8 {
            let reg = Arc::clone(&registry);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    let ki = generate_key_image();
                    reg.register(&ki).unwrap();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(registry.count(), 800);
    }
}
