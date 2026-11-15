//! Faucet admission proof: two distinct possession signatures, never a chain transaction.
use anyhow::{ensure, Result};
use libp2p_identity::Keypair;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{io::Read, path::{Path, PathBuf}, time::Duration};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey, Signature};
use wrfcoin_shared_protocol::crypto::signer::{BlockSigner, SignerProvenance};
use zeroize::Zeroizing;

pub const DOMAIN: &[u8] = b"wrfcoin_faucet_bond_grant_v1";
pub const PEER_DOMAIN: &[u8] = b"wrfcoin_faucet_bond_grant_peer_v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantRequest {
    pub owner_address: String,
    pub server_key: String,
    pub peer_id: String,
    pub height: u64,
    pub block_hash: String,
    pub signature: String,
    pub peer_signature: String,
}

fn text(bytes: &mut Vec<u8>, value: &str) -> Result<()> {
    let len = u32::try_from(value.len()).map_err(|_| anyhow::anyhow!("grant field exceeds bound"))?;
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}
impl GrantRequest {
    pub fn message(&self, chain_id: &str, network_id: &str) -> Result<Vec<u8>> {
        let mut bytes = DOMAIN.to_vec();
        for value in [chain_id, network_id, &self.owner_address, &self.server_key, &self.peer_id] {
            text(&mut bytes, value)?;
        }
        bytes.extend_from_slice(&self.height.to_le_bytes());
        text(&mut bytes, &self.block_hash)?;
        Ok(bytes)
    }
    pub fn sign(&mut self, chain_id: &str, network_id: &str, signer: &dyn BlockSigner, peer: &Keypair) -> Result<()> {
        ensure!(peer.clone().try_into_ed25519().is_ok(), "peer key must be ed25519");
        ensure!(peer.public().to_peer_id().to_string() == self.peer_id, "peer identity does not match node");
        ensure!(signer.public_key().to_hex() == self.server_key, "server signer mismatch");
        let message = self.message(chain_id, network_id)?;
        let signature = signer.sign(&message).map_err(|_| anyhow::anyhow!("server signing failed"))?;
        ensure!(signer.public_key().verify(&message, &signature), "server signature verification failed");
        self.signature = signature.to_hex();
        let peer_message = [PEER_DOMAIN, message.as_slice()].concat();
        let signature = peer.sign(&peer_message).map_err(|_| anyhow::anyhow!("peer signing failed"))?;
        ensure!(peer.public().verify(&peer_message, &signature), "peer signature verification failed");
        self.peer_signature = hex::encode(signature);
        Ok(())
    }
}

/// Load existing local keys only: bounded, no symlink, regular, readable only
/// by the current user (Unix mode 0600 and owner; Windows owner-only DACL).
fn key_bytes(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    let mut file = crate::node_operator::open_owned_key_file(path)?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.by_ref().take(8193).read_to_end(&mut bytes)
        .map_err(|_| anyhow::anyhow!("key file read failed"))?;
    ensure!(bytes.len() <= 8192, "key file exceeds bound");
    Ok(bytes)
}

/// The node's software server key (a 64-hex ed25519 seed file, as the Windows
/// launcher's `-Action ServerKey` writes and `WRFCOIN_SNAPSHOT_SERVER_KEY_FILE`
/// loads). Release builds accept it for testnet only: [`prepare`] refuses it
/// unless the node reports `network_id` `testnet`.
pub struct SeedFileServerSigner {
    signing: PrivateKey,
    verifying: PublicKey,
    provenance: SignerProvenance,
}
impl std::fmt::Debug for SeedFileServerSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeedFileServerSigner").field("public_key", &self.verifying.to_hex()).finish_non_exhaustive()
    }
}
impl BlockSigner for SeedFileServerSigner {
    fn sign(&self, payload: &[u8]) -> Result<Signature> {
        Ok(self.signing.sign(payload))
    }
    fn public_key(&self) -> PublicKey {
        self.verifying.clone()
    }
    fn provenance(&self) -> &SignerProvenance {
        &self.provenance
    }
}
pub fn server_key(path: &Path) -> Result<SeedFileServerSigner> {
    let bytes = key_bytes(path)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| anyhow::anyhow!("server seed format refused"))?.trim();
    ensure!(text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit()), "server seed format refused");
    let signing = PrivateKey::from_hex(text).map_err(|_| anyhow::anyhow!("server seed format refused"))?;
    let verifying = PublicKey::from_private_key(&signing);
    Ok(SeedFileServerSigner { signing, verifying, provenance: SignerProvenance::KeyFile { path: path.display().to_string() } })
}
pub fn peer_key(path: &Path) -> Result<Keypair> {
    let bytes = key_bytes(path)?;
    let key = Keypair::from_protobuf_encoding(&bytes).map_err(|_| anyhow::anyhow!("peer key format refused"))?;
    ensure!(key.clone().try_into_ed25519().is_ok(), "peer key must be ed25519");
    Ok(key)
}
pub fn default_peer_key_path() -> PathBuf {
    // Matches launch_api/helpers.rs PreflightConfig and main.rs identity loading.
    PathBuf::from(std::env::var_os("WRFCOIN_DATA_DIR").unwrap_or_else(|| "./data".into()))
        .join("peer_identity.key")
}
fn endpoint(base: &str, route: &str) -> Result<url::Url> {
    let mut url = url::Url::parse(base).map_err(|_| anyhow::anyhow!("invalid service URL"))?;
    ensure!(matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
        && url.username().is_empty() && url.password().is_none()
        && url.query().is_none() && url.fragment().is_none(), "invalid service URL");
    url.set_path(&format!("{}{route}", url.path().trim_end_matches('/')));
    Ok(url)
}
pub fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10)).build().map_err(|_| anyhow::anyhow!("HTTP client unavailable"))
}
async fn body(mut response: reqwest::Response) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| anyhow::anyhow!("HTTP response read failed"))? {
        ensure!(bytes.len() + chunk.len() <= 1024 * 1024, "HTTP response exceeds bound");
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
async fn get(client: &reqwest::Client, base: &str, route: &str) -> Result<Value> {
    let response = client.get(endpoint(base, route)?).send().await
        .map_err(|_| anyhow::anyhow!("node GET failed"))?;
    ensure!(response.status().is_success(), "node GET returned HTTP {}", response.status().as_u16());
    let value: Value = serde_json::from_slice(&body(response).await?)
        .map_err(|_| anyhow::anyhow!("node JSON response invalid"))?;
    Ok(value.get("data").cloned().unwrap_or(value))
}
fn field(value: &Value, name: &str) -> Result<String> {
    let text = value.get(name).and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("node response missing {name}"))?;
    ensure!(!text.is_empty() && text.len() <= 256 && text.trim() == text, "node field {name} invalid");
    Ok(text.to_string())
}
fn hex_field(value: &str, count: usize, label: &str) -> Result<String> {
    let value = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")).unwrap_or(value);
    ensure!(value.len() == count && value.bytes().all(|b| b.is_ascii_hexdigit()), "invalid {label}");
    Ok(value.to_ascii_lowercase())
}
/// The only network a seed-file server key may request a grant on.
pub const FILE_SIGNER_NETWORK: &str = "testnet";

/// `file_backed_signer`: the signer is a software seed file, which is accepted
/// only when the node reports [`FILE_SIGNER_NETWORK`] (HSM signers: any network).
pub async fn prepare(client: &reqwest::Client, api_url: &str, owner: &str, signer: &dyn BlockSigner, peer: &Keypair, file_backed_signer: bool) -> Result<GrantRequest> {
    let owner = owner.trim();
    ensure!(owner.starts_with("0x") || owner.starts_with("0X"), "owner address must have 0x prefix");
    let owner = format!("0x{}", hex_field(owner, 40, "owner address")?);
    let info = get(client, api_url, "/network/info").await?;
    let chain = field(&info, "chain_id")?;
    let network = field(&info, "network_id")?;
    ensure!(network != "unknown", "node network identity unavailable");
    ensure!(!file_backed_signer || network == FILE_SIGNER_NETWORK,
        "a server key file is accepted on testnet only; this node reports network {network}");
    let peer_id = field(&info, "local_peer_id")?;
    ensure!(peer.public().to_peer_id().to_string() == peer_id, "peer identity does not match node");
    let tip = info.get("chain_height").and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("node chain height invalid"))?;
    ensure!(tip > 2 && tip <= 9_007_199_254_740_991, "node chain height outside faucet range");
    let height = tip - 2;
    let block = get(client, api_url, &format!("/chain/block/{height}")).await?;
    let block_hash = hex_field(&field(&block, "hash")?, 64, "block hash")?;
    let mut request = GrantRequest { owner_address: owner, server_key: signer.public_key().to_hex(),
        peer_id, height, block_hash, signature: String::new(), peer_signature: String::new() };
    request.sign(&chain, &network, signer, peer)?;
    Ok(request)
}
/// Exactly two explicit POSTs: challenge creation, then grant submission after operator input.
/// Redirects and retry loops are disabled, including on non-2xx.
pub async fn submit(client: &reqwest::Client, backend: &str, request: &GrantRequest) -> Result<Value> {
    let challenge_response = client.post(endpoint(backend, "/api/onboarding/faucet/challenge")?)
        .json(&json!({})).send().await.map_err(|_| anyhow::anyhow!("captcha challenge POST failed"))?;
    ensure!(challenge_response.status().is_success(), "captcha challenge returned HTTP {}",
        challenge_response.status().as_u16());
    let challenge: Value = serde_json::from_slice(&body(challenge_response).await?)
        .map_err(|_| anyhow::anyhow!("captcha challenge JSON invalid"))?;
    let challenge_id = field(&challenge, "challengeId")?;
    let question = field(&challenge, "question")?;
    let answer = read_captcha_answer(&question)?;
    let mut payload = serde_json::to_value(request)?;
    payload["challengeId"] = Value::String(challenge_id);
    payload["answer"] = Value::String(answer);
    let response = client.post(endpoint(backend, "/api/faucet/bond-grant")?).json(&payload).send().await
        .map_err(|_| anyhow::anyhow!("faucet POST failed; outcome unknown, inspect before retry"))?;
    let status = response.status().as_u16();
    let bytes = body(response).await?;
    let value = serde_json::from_slice::<Value>(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    Ok(json!({"status": status, "body": value}))
}

/// Display the backend's question and accept an operator-supplied answer; never solve it.
pub fn read_captcha_answer(question: &str) -> Result<String> {
    use std::io::{BufRead, Write};
    ensure!(!question.is_empty() && question.len() <= 512 && !question.chars().any(char::is_control),
        "captcha question invalid");
    let mut stderr = std::io::stderr().lock();
    write!(stderr, "Captcha: {question}\nAnswer: ")?;
    stderr.flush()?;
    let mut answer = String::new();
    let count = std::io::stdin().lock().take(1025).read_line(&mut answer)?;
    ensure!(count > 0 && answer.len() <= 256, "captcha answer missing or too long");
    let answer = answer.trim();
    answer.parse::<i64>().map_err(|_| anyhow::anyhow!("captcha answer must be an integer"))?;
    Ok(answer.to_string())
}
