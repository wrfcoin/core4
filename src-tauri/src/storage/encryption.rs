use argon2::Argon2;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rand::RngCore;

/// Salt length in bytes for Argon2 KDF
const SALT_LEN: usize = 16;
/// Nonce length in bytes for ChaCha20-Poly1305
const NONCE_LEN: usize = 12;

/// Derives a 32-byte encryption key from a passphrase and salt using Argon2id.
fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let argon2 = Argon2::default();
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| format!("Key derivation failed: {}", e))?;
    Ok(key)
}

/// Encrypts a private key with a user passphrase.
///
/// Returns a base64-encoded string containing: salt (16) || nonce (12) || ciphertext.
pub fn encrypt_private_key(private_key: &str, passphrase: &str) -> Result<String, String> {
    // Generate random salt and nonce
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    // Derive encryption key
    let key = derive_key(passphrase, &salt)?;

    // Encrypt
    let cipher = ChaCha20Poly1305::new_from_slice(&key)
        .map_err(|e| format!("Cipher init failed: {}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, private_key.as_bytes())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    // Concatenate salt || nonce || ciphertext and base64 encode
    let mut blob = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&blob))
}

/// Decrypts an encrypted private key with the user passphrase.
///
/// Expects a base64-encoded string containing: salt (16) || nonce (12) || ciphertext.
pub fn decrypt_private_key(encrypted: &str, passphrase: &str) -> Result<String, String> {
    let blob = BASE64
        .decode(encrypted)
        .map_err(|e| format!("Base64 decode failed: {}", e))?;

    if blob.len() < SALT_LEN + NONCE_LEN + 1 {
        return Err("Encrypted data too short".to_string());
    }

    let salt = &blob[..SALT_LEN];
    let nonce_bytes = &blob[SALT_LEN..SALT_LEN + NONCE_LEN];
    let ciphertext = &blob[SALT_LEN + NONCE_LEN..];

    // Derive the same key
    let key = derive_key(passphrase, salt)?;

    // Decrypt
    let cipher = ChaCha20Poly1305::new_from_slice(&key)
        .map_err(|e| format!("Cipher init failed: {}", e))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "Decryption failed: wrong passphrase or corrupted data".to_string())?;

    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 decode failed: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let private_key = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let passphrase = "my-strong-passphrase";

        let encrypted = encrypt_private_key(private_key, passphrase).unwrap();
        assert_ne!(encrypted, private_key, "Encrypted value must differ from plaintext");

        let decrypted = decrypt_private_key(&encrypted, passphrase).unwrap();
        assert_eq!(decrypted, private_key);
    }

    #[test]
    fn test_wrong_passphrase_fails() {
        let private_key = "deadbeef01234567deadbeef01234567deadbeef01234567deadbeef01234567";
        let passphrase = "correct-passphrase";

        let encrypted = encrypt_private_key(private_key, passphrase).unwrap();
        let result = decrypt_private_key(&encrypted, "wrong-passphrase");
        assert!(result.is_err());
    }

    #[test]
    fn test_different_encryptions_produce_different_ciphertext() {
        let private_key = "deadbeef01234567deadbeef01234567deadbeef01234567deadbeef01234567";
        let passphrase = "same-passphrase";

        let enc1 = encrypt_private_key(private_key, passphrase).unwrap();
        let enc2 = encrypt_private_key(private_key, passphrase).unwrap();
        // Random salt/nonce means different ciphertexts
        assert_ne!(enc1, enc2);

        // But both decrypt to the same key
        assert_eq!(decrypt_private_key(&enc1, passphrase).unwrap(), private_key);
        assert_eq!(decrypt_private_key(&enc2, passphrase).unwrap(), private_key);
    }

    #[test]
    fn test_corrupted_data_fails() {
        let private_key = "deadbeef01234567deadbeef01234567deadbeef01234567deadbeef01234567";
        let passphrase = "passphrase";

        let encrypted = encrypt_private_key(private_key, passphrase).unwrap();

        // Corrupt the base64 data
        let mut blob = BASE64.decode(&encrypted).unwrap();
        if let Some(byte) = blob.last_mut() {
            *byte ^= 0xFF;
        }
        let corrupted = BASE64.encode(&blob);

        let result = decrypt_private_key(&corrupted, passphrase);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_data_fails() {
        let result = decrypt_private_key("", "passphrase");
        assert!(result.is_err());
    }
}
