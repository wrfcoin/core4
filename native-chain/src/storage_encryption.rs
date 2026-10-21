use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, KeyInit},
};
use rand::{RngCore, rngs::OsRng};

const STORAGE_KEY_LEN: usize = 32;
const STORAGE_NONCE_LEN: usize = 12;

#[derive(Clone)]
pub(crate) struct StorageEncryption {
    key: [u8; STORAGE_KEY_LEN],
}

impl StorageEncryption {
    pub(crate) fn from_env_key(hex_key: &str) -> Result<Self, String> {
        let decoded = hex::decode(hex_key.trim())
            .map_err(|e| format!("WRFCOIN_STORAGE_ENCRYPTION_KEY is not valid hex: {e}"))?;
        let key: [u8; STORAGE_KEY_LEN] = decoded.try_into().map_err(|_| {
            format!("WRFCOIN_STORAGE_ENCRYPTION_KEY must decode to {STORAGE_KEY_LEN} bytes")
        })?;
        Ok(Self { key })
    }

    pub(crate) fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let mut nonce_bytes = [0u8; STORAGE_NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let cipher = self.cipher();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| format!("failed to encrypt storage value: {e}"))?;

        let mut blob = Vec::with_capacity(STORAGE_NONCE_LEN + ciphertext.len());
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);
        Ok(blob)
    }

    pub(crate) fn decrypt(&self, stored: &[u8]) -> Result<Vec<u8>, String> {
        if stored.len() <= STORAGE_NONCE_LEN {
            return Err(format!(
                "encrypted storage value is too short: expected nonce plus ciphertext, got {} bytes",
                stored.len()
            ));
        }

        let (nonce_bytes, ciphertext) = stored.split_at(STORAGE_NONCE_LEN);
        let cipher = self.cipher();
        let nonce = Nonce::from_slice(nonce_bytes);
        cipher.decrypt(nonce, ciphertext).map_err(|_| {
            "failed to decrypt storage value; database may be plaintext or the storage key is incorrect".to_string()
        })
    }

    fn cipher(&self) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new_from_slice(&self.key).expect("32-byte storage key")
    }
}

#[cfg(test)]
mod tests {
    use super::StorageEncryption;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = "11".repeat(32);
        let encryption = StorageEncryption::from_env_key(&key).expect("key");

        let encrypted = encryption.encrypt(b"hello storage").expect("encrypt");
        assert_ne!(encrypted, b"hello storage");
        assert_eq!(
            encryption.decrypt(&encrypted).expect("decrypt"),
            b"hello storage"
        );
    }

    #[test]
    fn rejects_invalid_key_hex_or_length() {
        assert!(StorageEncryption::from_env_key("zz").is_err());
        assert!(StorageEncryption::from_env_key(&"aa".repeat(31)).is_err());
    }
}
