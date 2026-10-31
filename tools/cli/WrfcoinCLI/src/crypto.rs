use anyhow::{Result, anyhow};
use base64::{Engine as _, engine::general_purpose};
use bip39::Mnemonic;
use log::debug;
use ring::hmac;
use ring::rand::SystemRandom;
use ring::signature::{self, Ed25519KeyPair, KeyPair};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use wrfcoin_shared_protocol::common::types::Address;
use wrfcoin_shared_protocol::crypto::signature::PublicKey;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

// Signing preimages read the process chain/network environment on every call.
// Readers must share this lock with tests that temporarily change that domain.
// This module is compiled into both lib and bin tests, each in its own process.
#[cfg(test)]
pub(crate) static SIGNING_ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Key pair wrapper for Wrfcoin
///
/// `Serialize`/`Deserialize` are the keystore round-trip (`save_to_file` /
/// `load_from_file`, written with owner-only permissions) — the private key
/// reaching that JSON is the feature, not a leak. `Debug` is hand-implemented
/// below so an incidental `{:?}` never prints it, and `Clone` is deliberately
/// absent (nothing cloned the whole pair; copies of key material should be
/// explicit).
#[derive(Serialize, Deserialize)] // secret-leak-ok: Serialize is the owner-only keystore file round-trip, not an output path
pub struct WrfKeyPair {
    /// Public key in base64 encoding
    pub public_key: String,
    /// Private key in base64 encoding (in a real implementation, this would be encrypted)
    private_key: String,
    /// Derived Wrfcoin address
    pub address: String,
}

impl std::fmt::Debug for WrfKeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WrfKeyPair")
            .field("public_key", &self.public_key)
            .field("private_key", &"<redacted>")
            .field("address", &self.address)
            .finish()
    }
}

/// Transaction signing data
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionSigningData {
    /// Transaction ID
    pub tx_id: String,
    /// Sender address
    pub from: String,
    /// Recipient address
    pub to: String,
    /// Transaction amount in base units (1 WRF = 100_000_000 units)
    pub amount: u64,
    /// Transaction fee in base units
    pub fee: u64,
    /// Transaction timestamp
    pub timestamp: String,
    /// Transaction type
    pub tx_type: String,
    /// Nonce to prevent replay attacks
    pub nonce: u64,
}

impl WrfKeyPair {
    /// Chain-canonical address derived from this key's ed25519 public key.
    pub fn canonical_address(&self) -> Result<String> {
        let bytes = general_purpose::STANDARD.decode(&self.public_key)?;
        derive_address_from_public_key(&bytes)
    }

    /// Address produced by CLI releases before core4#3236.
    pub fn legacy_address(&self) -> Result<String> {
        let bytes = general_purpose::STANDARD.decode(&self.public_key)?;
        Ok(legacy_wrf_address(&bytes))
    }

    /// Update the stored address after an explicit, backed-up migration.
    pub fn set_canonical_address(&mut self) -> Result<String> {
        let canonical = self.canonical_address()?;
        self.address = canonical.clone();
        Ok(canonical)
    }
    /// Generate a new key pair
    pub fn generate() -> Result<Self> {
        let rng = SystemRandom::new();
        let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&rng)
            .map_err(|e| anyhow!("Failed to generate key pair: {e:?}"))?;
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8_bytes.as_ref())
            .map_err(|e| anyhow!("Failed to create key pair from PKCS8: {e:?}"))?;

        let public_key = key_pair.public_key().as_ref();
        let public_key_b64 = general_purpose::STANDARD.encode(public_key);

        // In a real implementation, the private key would be encrypted
        let private_key_b64 = general_purpose::STANDARD.encode(pkcs8_bytes.as_ref());

        // Derive Wrfcoin address from public key
        let address = derive_address_from_public_key(public_key)?;

        Ok(Self {
            public_key: public_key_b64,
            private_key: private_key_b64,
            address,
        })
    }

    /// Generate a new key pair with BIP-39 mnemonic phrase.
    /// Returns `(WrfKeyPair, mnemonic_phrase)`.
    pub fn generate_with_mnemonic() -> Result<(Self, String)> {
        let mnemonic =
            Mnemonic::generate(24).map_err(|e| anyhow!("Failed to generate mnemonic: {e}"))?;
        let phrase = mnemonic.to_string();
        let key_pair = Self::from_mnemonic(&phrase)?;
        Ok((key_pair, phrase))
    }

    /// Recover a key pair from a BIP-39 mnemonic phrase.
    pub fn from_mnemonic(phrase: &str) -> Result<Self> {
        let mnemonic: Mnemonic = phrase
            .parse()
            .map_err(|e| anyhow!("Invalid mnemonic phrase: {e}"))?;
        let seed = mnemonic.to_seed("");

        // SLIP-0010: derive ed25519 seed via HMAC-SHA512
        let hmac_key = hmac::Key::new(hmac::HMAC_SHA512, b"ed25519 seed");
        let tag = hmac::sign(&hmac_key, &seed);
        let ed25519_seed: [u8; 32] = tag.as_ref()[..32]
            .try_into()
            .map_err(|_| anyhow!("HMAC output too short"))?;

        // Build an Ed25519KeyPair from the derived seed to get the public key
        let temp_kp = Ed25519KeyPair::from_seed_unchecked(&ed25519_seed)
            .map_err(|e| anyhow!("Failed to create key pair from seed: {e:?}"))?;
        let public_key_bytes = temp_kp.public_key().as_ref().to_vec();

        // Construct PKCS#8 v2 DER so the key is compatible with existing storage
        let pkcs8 = build_ed25519_pkcs8_v2(&ed25519_seed, &public_key_bytes);
        // Verify round-trip
        let key_pair = Ed25519KeyPair::from_pkcs8(&pkcs8)
            .map_err(|e| anyhow!("PKCS8 round-trip failed: {e:?}"))?;

        let public_key_b64 = general_purpose::STANDARD.encode(key_pair.public_key().as_ref());
        let private_key_b64 = general_purpose::STANDARD.encode(&pkcs8);
        let address = derive_address_from_public_key(key_pair.public_key().as_ref())?;

        Ok(Self {
            public_key: public_key_b64,
            private_key: private_key_b64,
            address,
        })
    }

    /// Load a key pair from base64 encoded private key
    #[allow(dead_code)]
    pub fn from_private_key(private_key_b64: &str) -> Result<Self> {
        let pkcs8_bytes = general_purpose::STANDARD.decode(private_key_b64)?;
        let key_pair = Ed25519KeyPair::from_pkcs8(&pkcs8_bytes)
            .map_err(|e| anyhow!("Failed to load key pair from private key: {e:?}"))?;

        let public_key = key_pair.public_key().as_ref();
        let public_key_b64 = general_purpose::STANDARD.encode(public_key);

        // Derive Wrfcoin address from public key
        let address = derive_address_from_public_key(public_key)?;

        Ok(Self {
            public_key: public_key_b64,
            private_key: private_key_b64.to_string(),
            address,
        })
    }

    /// Get the raw key pair for signing
    fn get_key_pair(&self) -> Result<Ed25519KeyPair> {
        let pkcs8_bytes = general_purpose::STANDARD.decode(&self.private_key)?;
        let key_pair = Ed25519KeyPair::from_pkcs8(&pkcs8_bytes)
            .map_err(|e| anyhow!("Failed to reconstruct key pair: {e:?}"))?;
        Ok(key_pair)
    }

    /// Sign a message
    pub fn sign(&self, message: &[u8]) -> Result<String> {
        let key_pair = self.get_key_pair()?;
        let signature = key_pair.sign(message);
        Ok(general_purpose::STANDARD.encode(signature.as_ref()))
    }

    /// Hex forms used by the chain transparent-transfer contract.
    pub fn sign_hex(&self, message: &[u8]) -> Result<String> {
        let key_pair = self.get_key_pair()?;
        Ok(hex::encode(key_pair.sign(message).as_ref()))
    }

    pub fn public_key_hex(&self) -> Result<String> {
        Ok(hex::encode(
            general_purpose::STANDARD.decode(&self.public_key)?,
        ))
    }

    /// Sign a transaction
    pub fn sign_transaction(&self, tx_data: &TransactionSigningData) -> Result<String> {
        // Validate sender address
        if tx_data.from != self.address {
            return Err(anyhow!(
                "Transaction sender address doesn't match key pair address"
            ));
        }

        // Serialize transaction data for signing
        let message = serde_json::to_string(tx_data)?;
        self.sign(message.as_bytes())
    }

    /// Save key pair to file
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
            set_dir_permissions_owner_only(parent)?;
        }

        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        set_file_permissions_owner_only(path)?;

        debug!("Key pair saved to {}", path.display());
        Ok(())
    }

    /// Load key pair from file
    pub fn load_from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(anyhow!("Key pair file not found: {}", path.display()));
        }

        let json = fs::read_to_string(path)
            .map_err(|e| anyhow!("failed to read key pair file {}: {e}", path.display()))?;
        let key_pair: WrfKeyPair = serde_json::from_str(&json)
            .map_err(|e| anyhow!("failed to parse key pair file {}: {e}", path.display()))?;

        debug!("Key pair loaded from {}", path.display());
        Ok(key_pair)
    }
}

/// Verify a signature
#[allow(dead_code)]
pub fn verify_signature(public_key_b64: &str, message: &[u8], signature_b64: &str) -> Result<bool> {
    let public_key_bytes = general_purpose::STANDARD.decode(public_key_b64)?;
    let signature_bytes = general_purpose::STANDARD.decode(signature_b64)?;

    let public_key = signature::UnparsedPublicKey::new(&signature::ED25519, &public_key_bytes);

    match public_key.verify(message, &signature_bytes) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Verify a transaction signature
#[allow(dead_code)]
pub fn verify_transaction_signature(
    tx_data: &TransactionSigningData,
    public_key_b64: &str,
    signature_b64: &str,
) -> Result<bool> {
    // Serialize transaction data for verification
    let message = serde_json::to_string(tx_data)?;
    verify_signature(public_key_b64, message.as_bytes(), signature_b64)
}

/// Build a PKCS#8 v2 (OneAsymmetricKey) DER document for Ed25519.
/// This matches the format produced by `ring::Ed25519KeyPair::generate_pkcs8`.
fn build_ed25519_pkcs8_v2(seed: &[u8; 32], public_key: &[u8]) -> Vec<u8> {
    // SEQUENCE (83 bytes total content)
    //   INTEGER 1                              (version – v2)
    //   SEQUENCE { OID 1.3.101.112 }           (Ed25519)
    //   OCTET STRING { OCTET STRING { seed } } (private key)
    //   [1] EXPLICIT { BIT STRING { 0x00 || public_key } }
    let mut doc = Vec::with_capacity(85);
    doc.extend_from_slice(&[
        0x30, 0x53, // SEQUENCE, 83 bytes
        0x02, 0x01, 0x01, // INTEGER 1
        0x30, 0x05, // SEQUENCE, 5 bytes
        0x06, 0x03, 0x2b, 0x65, 0x70, // OID 1.3.101.112
        0x04, 0x22, // OCTET STRING, 34 bytes
        0x04, 0x20, // OCTET STRING, 32 bytes
    ]);
    doc.extend_from_slice(seed);
    doc.extend_from_slice(&[
        0xa1, 0x23, // [1] EXPLICIT, 35 bytes
        0x03, 0x21, // BIT STRING, 33 bytes
        0x00, // unused bits
    ]);
    doc.extend_from_slice(public_key);
    doc
}

/// Derive Wrfcoin address from public key
fn derive_address_from_public_key(public_key: &[u8]) -> Result<String> {
    let public_key = PublicKey::from_bytes(public_key)?;
    Ok(Address::from_public_key(&public_key).to_hex())
}

fn legacy_wrf_address(public_key: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, public_key);
    format!("Wrf{}", hex::encode(&digest.as_ref()[..20]))
}

/// Key pair manager
pub struct KeyManager {
    /// Base directory for key storage
    base_dir: PathBuf,
}

impl KeyManager {
    /// Create a new key manager
    pub fn new(base_dir: &Path) -> Result<Self> {
        let key_dir = base_dir.join("keys");

        // Create key directory if it doesn't exist
        if !key_dir.exists() {
            fs::create_dir_all(&key_dir)?;
            debug!("Created key directory: {}", key_dir.display());
        }
        set_dir_permissions_owner_only(&key_dir)?;

        Ok(Self {
            base_dir: base_dir.to_path_buf(),
        })
    }

    /// Get key directory
    fn key_dir(&self) -> PathBuf {
        self.base_dir.join("keys")
    }

    /// Generate a new key pair and save it
    pub fn generate_key_pair(&self, label: Option<&str>) -> Result<WrfKeyPair> {
        let key_pair = WrfKeyPair::generate()?;

        // Save key pair
        let file_name = match label {
            Some(l) => format!("{}-{l}.json", key_pair.address),
            None => format!("{}.json", key_pair.address),
        };

        let path = self.key_dir().join(file_name);
        key_pair.save_to_file(&path)?;

        Ok(key_pair)
    }

    /// Generate a new key pair with BIP-39 mnemonic and save it.
    pub fn generate_key_pair_with_mnemonic(
        &self,
        label: Option<&str>,
    ) -> Result<(WrfKeyPair, String)> {
        let (key_pair, mnemonic) = WrfKeyPair::generate_with_mnemonic()?;

        let file_name = match label {
            Some(l) => format!("{}-{l}.json", key_pair.address),
            None => format!("{}.json", key_pair.address),
        };

        let path = self.key_dir().join(file_name);
        key_pair.save_to_file(&path)?;

        Ok((key_pair, mnemonic))
    }

    /// Recover a key pair from BIP-39 mnemonic and save it.
    pub fn recover_key_pair(&self, mnemonic: &str, label: Option<&str>) -> Result<WrfKeyPair> {
        let key_pair = WrfKeyPair::from_mnemonic(mnemonic)?;

        let file_name = match label {
            Some(l) => format!("{}-{l}.json", key_pair.address),
            None => format!("{}.json", key_pair.address),
        };

        let path = self.key_dir().join(file_name);
        key_pair.save_to_file(&path)?;

        Ok(key_pair)
    }

    /// Load a key pair by address
    pub fn load_key_pair(&self, address: &str) -> Result<WrfKeyPair> {
        let key_dir = self.key_dir();

        // Find key file for address
        let entries = fs::read_dir(&key_dir)?;
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with(address) {
                return WrfKeyPair::load_from_file(&entry.path());
            }
        }

        Err(anyhow!("No key pair found for address: {address}"))
    }

    /// List all key pairs
    pub fn list_key_pairs(&self) -> Result<Vec<(String, PathBuf)>> {
        let key_dir = self.key_dir();
        let mut key_pairs = Vec::new();

        let entries = fs::read_dir(&key_dir)?;
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.ends_with(".json") {
                // Extract address from filename (address-label.json or address.json)
                if let Some(address) = file_name.split('-').next() {
                    let address = address.trim_end_matches(".json").to_string();
                    key_pairs.push((address, entry.path()));
                }
            }
        }

        Ok(key_pairs)
    }

    /// Delete a key pair
    #[allow(dead_code)]
    pub fn delete_key_pair(&self, address: &str) -> Result<()> {
        let key_dir = self.key_dir();

        // Find key file for address
        let entries = fs::read_dir(&key_dir)?;
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with(address) {
                fs::remove_file(entry.path())?;
                debug!("Deleted key pair: {}", entry.path().display());
                return Ok(());
            }
        }

        Err(anyhow!("No key pair found for address: {address}"))
    }
}

fn set_dir_permissions_owner_only(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn set_file_permissions_owner_only(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_pair_generation() {
        let key_pair = WrfKeyPair::generate().unwrap();
        assert!(!key_pair.public_key.is_empty());
        assert!(!key_pair.private_key.is_empty()); // secret-leak-ok: test-only generated key; assert! prints the expression source, not the value
        assert!(key_pair.address.starts_with("0x"));
    }

    #[test]
    fn test_key_pair_signing() {
        let key_pair = WrfKeyPair::generate().unwrap();
        let message = b"Test message";
        let signature = key_pair.sign(message).unwrap();
        assert!(!signature.is_empty());

        let verified = verify_signature(&key_pair.public_key, message, &signature).unwrap();
        assert!(verified);
    }

    #[test]
    fn test_generate_with_mnemonic() {
        let (key_pair, mnemonic) = WrfKeyPair::generate_with_mnemonic().unwrap();
        assert!(key_pair.address.starts_with("0x"));
        let words: Vec<&str> = mnemonic.split_whitespace().collect();
        assert_eq!(words.len(), 24);

        // Verify round-trip: recovering from the same mnemonic produces the same address
        let recovered = WrfKeyPair::from_mnemonic(&mnemonic).unwrap();
        assert_eq!(key_pair.address, recovered.address);
        assert_eq!(key_pair.public_key, recovered.public_key);
    }

    #[test]
    fn test_mnemonic_signing() {
        let (key_pair, _) = WrfKeyPair::generate_with_mnemonic().unwrap();
        let message = b"Test BIP-39 signing";
        let signature = key_pair.sign(message).unwrap();
        assert!(!signature.is_empty());

        let verified = verify_signature(&key_pair.public_key, message, &signature).unwrap();
        assert!(verified);
    }

    #[test]
    fn shared_wallet_key_scheme_vector_matches_chain_address() {
        const VECTOR_BYTES: &[u8] = include_bytes!(
            "../../../../contract-fixtures/wallet-key-scheme-vector.json"
        );
        assert_eq!(
            hex::encode(ring::digest::digest(&ring::digest::SHA256, VECTOR_BYTES)),
            "b232464dea5cab623a481fb5eddbb2ec6a0cf04c6e8b80e590fe531f844b27d4",
            "wallet key-scheme fixture bytes differ from the core4#3234 decision"
        );
        let vector: serde_json::Value = serde_json::from_slice(VECTOR_BYTES).unwrap();
        assert_eq!(
            vector,
            serde_json::json!({
                "mnemonic": "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
                "passphrase": "",
                "derivation": "slip10-ed25519-master (HMAC-SHA512 key \"ed25519 seed\", IL, no child path)",
                "bip39_seed_hex_prefix": "5eb00bbddcf069084889a8ab91555681",
                "ed25519_public_key_hex": "e96b1c6b8769fdb0b34fbecfdf85c33b053cecad9517e1ab88cba614335775c1",
                "address": "0x3f995ceb27e8177077dbed8b1e4f696af74da492",
                "legacy_cli_address_for_the_same_key": "Wrfada8e3423e041d247dca60598e6d3d8834161fe5"
            })
        );
        let phrase = vector["mnemonic"].as_str().unwrap();
        let passphrase = vector["passphrase"].as_str().unwrap();
        let seed = phrase.parse::<Mnemonic>().unwrap().to_seed(passphrase);
        assert!(hex::encode(seed).starts_with(
            // secret-leak-ok: test; published BIP-39 test vector, not a live seed
            vector["bip39_seed_hex_prefix"].as_str().unwrap()
        ));
        let key_pair = WrfKeyPair::from_mnemonic(phrase).unwrap();
        assert_eq!(
            key_pair.public_key_hex().unwrap(),
            vector["ed25519_public_key_hex"].as_str().unwrap()
        );
        assert_eq!(key_pair.address, vector["address"].as_str().unwrap());
        assert_eq!(key_pair.canonical_address().unwrap(), key_pair.address);
        assert_eq!(
            key_pair.legacy_address().unwrap(),
            vector["legacy_cli_address_for_the_same_key"]
                .as_str()
                .unwrap()
        );
    }

    /// Cross-repo transparent-transfer authorization vector for the live testnet
    /// (core4#3260). core4 is the producer; frontend and mobile vendor the same
    /// bytes and rebuild the preimage. Regenerate with
    /// `WRFCOIN_REGEN_TRANSFER_AUTH_VECTOR=1 cargo test -p wrfcoin_cli --lib
    /// transfer_auth_vector_777`, then update the pinned SHA-256 below.
    const TRANSFER_AUTH_VECTOR_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../contract-fixtures/transparent-transfer-auth-vector-777.json"
    );
    const TRANSFER_AUTH_VECTOR_BYTES: &[u8] =
        include_bytes!("../../../../contract-fixtures/transparent-transfer-auth-vector-777.json");
    const TRANSFER_AUTH_VECTOR_SHA256: &str =
        "9a37d7f71d316aaf863ec1612e4fe3a7cb43cf0f9b300717565958b4a08a20c8";

    fn build_transfer_auth_vector_777() -> serde_json::Value {
        use wrfcoin_shared_protocol::blockchain::transaction::{
            TRANSPARENT_TRANSFER_AUTH_DOMAIN,
            transparent_transfer_authorization_message_with_domain,
        };

        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let key_pair = WrfKeyPair::from_mnemonic(mnemonic).unwrap();
        let (chain_id, network_id) = ("777", "testnet");
        let to = "0x2222222222222222222222222222222222222222";
        // 1.5 WRF. The preimage carries integer base units (1 WRF = 10^8).
        let amount: u64 = 150_000_000;
        let nonce: u64 = 7;
        let preimage = transparent_transfer_authorization_message_with_domain(
            chain_id,
            network_id,
            &key_pair.address,
            to,
            amount,
            nonce,
        );
        serde_json::json!({
            "schema": "wrfcoin.transparent-transfer-auth-vector.v1",
            "producer": "core4 tools/cli/WrfcoinCLI/src/crypto.rs transfer_auth_vector_777 (core4#3260)",
            "verified_by": "core4 authorize_debit, the check behind TransparentTransferPayload::verify_authorization",
            "domain_tag": std::str::from_utf8(TRANSPARENT_TRANSFER_AUTH_DOMAIN).unwrap(),
            "framing": "tag || u32le(len chain_id) || chain_id || u32le(len network_id) || network_id || u32le(len from) || from || u32le(len to) || to || u64le(amount) || u64le(nonce)",
            "amount_unit": "integer base units (1 WRF = 100000000)",
            "address_encoding": "from and to are hashed as the exact submitted ASCII strings: 0x + 40 lowercase hex",
            "signer": {
                "key_vector": "contract-fixtures/wallet-key-scheme-vector.json",
                "mnemonic": mnemonic,
                "ed25519_public_key_hex": key_pair.public_key_hex().unwrap(),
            },
            "case": {
                "chain_id": chain_id,
                "network_id": network_id,
                "from": key_pair.address,
                "to": to,
                "amount": amount,
                "nonce": nonce,
                "preimage_hex": hex::encode(&preimage),
                "signature_hex": key_pair.sign_hex(&preimage).unwrap(),
            },
        })
    }

    #[test]
    fn transfer_auth_vector_777_matches_chain_preimage_and_signature() {
        use wrfcoin_shared_protocol::accounts::authorization::{
            EmptyAuthorizationView, SpendAuthorization, authorize_debit,
        };
        use wrfcoin_shared_protocol::common::types::Timestamp;

        let expected = build_transfer_auth_vector_777();
        if std::env::var_os("WRFCOIN_REGEN_TRANSFER_AUTH_VECTOR").is_some() {
            let mut text = serde_json::to_string_pretty(&expected).unwrap();
            text.push('\n');
            std::fs::write(TRANSFER_AUTH_VECTOR_PATH, &text).unwrap();
            panic!(
                "regenerated {TRANSFER_AUTH_VECTOR_PATH}; pin sha256 {} and rerun without the env var",
                hex::encode(ring::digest::digest(&ring::digest::SHA256, text.as_bytes()))
            );
        }

        assert_eq!(
            hex::encode(ring::digest::digest(
                &ring::digest::SHA256,
                TRANSFER_AUTH_VECTOR_BYTES
            )),
            TRANSFER_AUTH_VECTOR_SHA256,
            "transfer-auth vector bytes changed; frontend and mobile vendor these exact bytes"
        );
        let vector: serde_json::Value = serde_json::from_slice(TRANSFER_AUTH_VECTOR_BYTES).unwrap();
        // Rebuilt from the production preimage builder and the CLI key derivation.
        assert_eq!(vector, expected);

        // The signer is the shared key-scheme vector's key and address.
        let key_vector: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../../contract-fixtures/wallet-key-scheme-vector.json"
        ))
        .unwrap();
        let case = &vector["case"];
        assert_eq!(case["from"], key_vector["address"]);
        assert_eq!(
            vector["signer"]["ed25519_public_key_hex"],
            key_vector["ed25519_public_key_hex"]
        );

        // The chain's debit check accepts exactly these bytes, and only them.
        let from: Address = case["from"].as_str().unwrap().parse().unwrap();
        let preimage = hex::decode(case["preimage_hex"].as_str().unwrap()).unwrap();
        let auth = || SpendAuthorization {
            signer_public_key: vector["signer"]["ed25519_public_key_hex"].as_str().unwrap(),
            signature: case["signature_hex"].as_str().unwrap(),
        };
        let check = |message: &[u8]| {
            authorize_debit(
                &EmptyAuthorizationView,
                &from,
                message,
                auth(),
                0,
                Timestamp::new(0),
            )
        };
        assert!(check(&preimage).is_ok());
        let mut tampered = preimage.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(check(&tampered).is_err());
    }

    #[test]
    fn cli_signature_passes_chain_transparent_transfer_authorization() {
        let _domain = SIGNING_ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use wrfcoin_shared_protocol::blockchain::transaction::TransparentTransferPayload;

        let key_pair = WrfKeyPair::from_mnemonic(
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
        )
        .unwrap();
        let mut payload = TransparentTransferPayload {
            from: key_pair.address.clone(),
            to: "0x2222222222222222222222222222222222222222".to_string(),
            amount: 12,
            transparent_opt_out: true,
            opt_out_audit_tag: None,
            auth_pubkey: None,
            auth_signature: None,
            authority: None,
            authority_signature: None,
            passkey_auth: None,
            sponsorship: None,
        };
        let nonce = 7;
        payload.auth_pubkey = Some(key_pair.public_key_hex().unwrap());
        payload.auth_signature = Some(
            key_pair
                .sign_hex(&payload.authorization_message(nonce))
                .unwrap(),
        );
        assert!(payload.verify_authorization(nonce));
        assert!(!payload.verify_authorization(nonce + 1));
    }

    #[test]
    fn test_transaction_signing() {
        let key_pair = WrfKeyPair::generate().unwrap();

        let tx_data = TransactionSigningData {
            tx_id: "tx123".to_string(),
            from: key_pair.address.clone(),
            to: "WrfAnotherAddress".to_string(),
            amount: 10_000_000_000, // 100 WRF in base units
            fee: 100_000_000,       // 1 WRF in base units
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            tx_type: "Regular".to_string(),
            nonce: 12345,
        };

        let signature = key_pair.sign_transaction(&tx_data).unwrap();
        assert!(!signature.is_empty());

        let verified =
            verify_transaction_signature(&tx_data, &key_pair.public_key, &signature).unwrap();
        assert!(verified);
    }
}
