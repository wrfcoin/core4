//! Software keys confined to typed Node-share owner and possession subjects.
//! This deliberately does NOT implement BlockSigner or expose a byte signer.
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde::Deserialize;
use std::{io::Read, path::Path};
use wrfcoin_shared_protocol::{
    common::types::Address,
    consensus::{
        node_receipt_registry::NodeReceiptSubmissionAuthorization, node_share_contracts::*,
    },
    crypto::{
        chain_binding::{ChainIdentity, SigningProtocolVersion},
        domain_signing::DomainSeparated,
        signature::{PublicKey, Signature},
    },
};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug)]
pub enum OperatorRole {
    Owner,
    Server,
    Consumer,
    Producer,
    Attester,
    Governance,
    Payout,
}

pub struct OperatorKey {
    key: Ed25519KeyPair,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WalletKey {
    private_key: Zeroizing<String>,
    public_key: String,
    address: String,
}

impl OperatorKey {
    pub fn from_seed(seed: &[u8]) -> Result<Self> {
        ensure!(seed.len() == 32, "operator key format refused"); // secret-leak-ok: length predicate and fixed error, no seed contents.
        let key = Ed25519KeyPair::from_seed_unchecked(seed)
            .map_err(|_| anyhow::anyhow!("operator key format refused"))?;
        Ok(Self { key })
    }
    pub fn load(path: &Path) -> Result<Self> {
        // Existing permission guard plus stricter mode, UID and no-follow checks.
        wrfcoin_shared_protocol::security::key_file::assert_key_file_permissions(path)
            .map_err(|_| anyhow::anyhow!("operator key permissions refused"))?;
        #[cfg(not(unix))]
        anyhow::bail!("operator key ownership cannot be enforced on this platform");
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
                .open(path)
                .map_err(|_| anyhow::anyhow!("operator key file refused"))?;
            let meta = file
                .metadata()
                .map_err(|_| anyhow::anyhow!("operator key file refused"))?;
            // SAFETY: geteuid has no arguments or memory side effects.
            let uid = unsafe { libc::geteuid() };
            ensure!(
                file_policy(meta.is_file(), meta.mode(), meta.uid(), uid),
                "operator key must be regular, mode 0600 and owned by running user"
            );
            let mut bytes = Zeroizing::new(Vec::new());
            file.by_ref()
                .take(8193)
                .read_to_end(&mut bytes)
                .map_err(|_| anyhow::anyhow!("operator key file refused"))?;
            ensure!(bytes.len() <= 8192, "operator key exceeds bound");
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| anyhow::anyhow!("operator key format refused"))?
                .trim();
            if !text.starts_with('{') {
                let seed = Zeroizing::new(
                    hex::decode(text)
                        .map_err(|_| anyhow::anyhow!("operator key format refused"))?,
                );
                return Self::from_seed(&seed);
            }
            let wallet: WalletKey = serde_json::from_str(text)
                .map_err(|_| anyhow::anyhow!("operator wallet format refused"))?;
            let der = Zeroizing::new(
                STANDARD
                    .decode(wallet.private_key.as_bytes())
                    .map_err(|_| anyhow::anyhow!("operator wallet format refused"))?,
            );
            let key = Ed25519KeyPair::from_pkcs8(&der)
                .map_err(|_| anyhow::anyhow!("operator wallet format refused"))?;
            let signer = Self { key };
            let public = STANDARD
                .decode(&wallet.public_key)
                .map_err(|_| anyhow::anyhow!("operator wallet format refused"))?;
            ensure!(
                public == signer.key.public_key().as_ref()
                    && wallet.address == signer.address().to_string(),
                "operator wallet public binding refused"
            );
            Ok(signer)
        }
    }
    pub fn public_key(&self) -> PublicKey {
        PublicKey::from_bytes(self.key.public_key().as_ref()).expect("ring Ed25519 key is 32 bytes")
    }
    pub fn address(&self) -> Address {
        Address::from_public_key(&self.public_key())
    }
    pub fn sign_lifecycle(
        &self,
        role: OperatorRole,
        a: &NodeBondAuthorization,
        identity: &ChainIdentity,
    ) -> Result<Signature> {
        // Check role/key binding before constructing the only allowed domains.
        let bytes = match role {
            OperatorRole::Owner => {
                ensure!(self.address() == a.owner_account, "operator owner mismatch");
                a.signing_preimage_for(identity, SigningProtocolVersion::V2)
            },
            OperatorRole::Server | OperatorRole::Consumer => {
                ensure!(
                    matches!(a.action, NodeBondAction::Deposit | NodeBondAction::Update),
                    "operator possession action refused"
                );
                let server = matches!(role, OperatorRole::Server);
                ensure!(
                    self.key.public_key().as_ref()
                        == if server {
                            &a.server_key
                        } else {
                            &a.consumer_key
                        },
                    "operator possession key mismatch"
                );
                NodeBondPossession {
                    authorization_preimage_hash: node_contract_digest(a, identity),
                    role: if server {
                        NodeBondKeyRole::Server
                    } else {
                        NodeBondKeyRole::Consumer
                    },
                }
                .signing_preimage_for(identity, SigningProtocolVersion::V2)
            },
            _ => anyhow::bail!("software operator signer role refused"),
        };
        Signature::from_bytes(self.key.sign(&bytes).as_ref()).map_err(Into::into)
    }
    pub fn sign_receipt(
        &self,
        a: &NodeReceiptSubmissionAuthorization,
        identity: &ChainIdentity,
    ) -> Result<Signature> {
        ensure!(
            self.address() == a.sender,
            "operator receipt owner mismatch"
        );
        let bytes = a.signing_preimage_for(identity, SigningProtocolVersion::V2);
        Signature::from_bytes(self.key.sign(&bytes).as_ref()).map_err(Into::into)
    }
}

fn file_policy(regular: bool, mode: u32, file_uid: u32, running_uid: u32) -> bool {
    regular && mode & 0o7777 == 0o600 && file_uid == running_uid
}

#[cfg(test)]
mod tests {
    use super::*;
    fn authorization(key: &OperatorKey) -> NodeBondAuthorization {
        NodeBondAuthorization {
            genesis_hash: [7; 32],
            deployment_id: [1; 32],
            owner_account: key.address(),
            escrow_account: Address([8; 20]),
            payout_account: Address([9; 20]),
            server_key: [11; 32],
            consumer_key: [12; 32],
            service_alias: "node-1".into(),
            action: NodeBondAction::Deposit,
            amount_units: 25_000_000_000,
            expected_nonce: 0,
            effective_epoch: 1,
        }
    }
    #[test]
    fn operator_roles_refuse_producer_attester_governance_and_payout_in_release() {
        let key = OperatorKey::from_seed(&[10; 32]).unwrap();
        let a = authorization(&key);
        let identity = ChainIdentity::new("777", "testnet");
        for role in [
            OperatorRole::Producer,
            OperatorRole::Attester,
            OperatorRole::Governance,
            OperatorRole::Payout,
        ] {
            assert!(key.sign_lifecycle(role, &a, &identity).is_err());
        }
        let sig = key
            .sign_lifecycle(OperatorRole::Owner, &a, &identity)
            .unwrap();
        assert!(
            // secret-leak-ok: synthetic public key/signature verification boolean; assert prints no key material.
            wrfcoin_shared_protocol::crypto::domain_signing::verify_domain_for(
                &key.public_key(),
                &a,
                &sig,
                &identity,
                SigningProtocolVersion::V2
            )
        );
        // Cross-domain verification cannot turn an owner signature into a receipt.
        let receipt = NodeReceiptSubmissionAuthorization {
            genesis_hash: [7; 32],
            sender: key.address(),
            nonce: 0,
            batch_hash: [1; 32],
        };
        assert!(
            // secret-leak-ok: synthetic public key/signature domain control, no private-value formatting.
            !wrfcoin_shared_protocol::crypto::domain_signing::verify_domain_for(
                &key.public_key(),
                &receipt,
                &sig,
                &identity,
                SigningProtocolVersion::V2
            )
        );
        #[cfg(all(unix, not(debug_assertions)))]
        {
            use std::os::unix::fs::PermissionsExt;
            use wrfcoin_shared_protocol::crypto::signer::{
                SharedKeyFallback, SignerEnv, load_block_signer_for,
            };
            if let Some(path) = std::env::var_os("R169_SYNTHETIC_KEY_FILE") {
                let result = load_block_signer_for(SignerEnv {
                    hex_key_var: "R169_UNUSED_SYNTHETIC_KEY",
                    key_file_var: Some("R169_SYNTHETIC_KEY_FILE"),
                    hsm_key_label_var: None,
                    hsm_pubkey_label_var: None,
                    hsm_shared_fallback: SharedKeyFallback::Forbidden,
                });
                assert!(
                    result.unwrap().is_none(),
                    "release generic BlockSigner must refuse software file fallback"
                );
                assert!(OperatorKey::load(Path::new(&path)).is_ok());
                println!("NODE_OPERATOR_RELEASE_ROLE_GUARD_OK");
                return;
            }
            let root =
                std::env::temp_dir().join(format!("operator-release-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            let path = root.join("synthetic-key");
            std::fs::write(&path, hex::encode([10u8; 32])).unwrap(); // secret-leak-ok: fixed synthetic test seed in owned temporary mode0600 file.
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            // Child configuration is isolated; no process-global env mutation or operator/HSM credential inheritance.
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "node_operator::tests::operator_roles_refuse_producer_attester_governance_and_payout_in_release", "--nocapture", "--test-threads=1"])
                .env_clear().env("R169_SYNTHETIC_KEY_FILE", &path).output().unwrap();
            std::fs::remove_dir_all(root).unwrap();
            assert!(output.status.success());
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(stdout.contains("NODE_OPERATOR_RELEASE_ROLE_GUARD_OK"));
            println!("NODE_OPERATOR_RELEASE_ROLE_GUARD_OK");
        }
    }
    #[test]
    fn operator_wallet_binding_mode_owner_and_symlink_controls() {
        assert!(!file_policy(true, 0o600, 101, 100));
        assert!(!file_policy(false, 0o600, 100, 100));
        assert!(!file_policy(true, 0o400, 100, 100));
        assert!(!file_policy(true, 0o640, 100, 100));
        assert!(file_policy(true, 0o600, 100, 100));
        let root = std::env::temp_dir().join(format!("node-operator-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("wallet.json");
        let wallet = crate::crypto::WrfKeyPair::generate().unwrap();
        wallet.save_to_file(&path).unwrap();
        let key = OperatorKey::load(&path).unwrap();
        assert_eq!(key.address().to_string(), wallet.address);
        let public = key.public_key();
        assert_eq!(
            &wrfcoin_shared_protocol::crypto::hash::hash_bytes(public.as_bytes()).as_bytes()[..20],
            &key.address().0
        );
        let mut json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        json["address"] = serde_json::json!("0x0000000000000000000000000000000000000000");
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        assert!(OperatorKey::load(&path).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            wallet.save_to_file(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(OperatorKey::load(&path).is_err());
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::os::unix::fs::symlink(&path, root.join("alias")).unwrap();
            assert!(OperatorKey::load(&root.join("alias")).is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
