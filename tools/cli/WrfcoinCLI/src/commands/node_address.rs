//! `wrfcoin node address publish`: a bonded node's own signed record of where
//! its server key can be reached, posted to the address mailbox once an epoch.
//!
//! The house consumer reads the bond roster from chain (which names each
//! bond's server key but no address) and these records to find a newcomer's
//! node. Nothing else keeps a list of node addresses. Not consensus: no
//! transaction, no state change; the record lives in the mailbox only.
//!
//! Signing preimage (frozen by `address_record_message_layout_is_frozen`, and
//! repeated byte for byte by the mailbox's verifier and its tests):
//!
//! ```text
//! str(s) = u32LE(UTF-8 length) || UTF-8 bytes
//! M = "wrfcoin_node_address_record_v1"
//!     || str(chain_id) || str(network_id) || str(genesis_hash)
//!     || str(deployment_id) || str(server_key) || str(peer_id)
//!     || u64LE(epoch) || u64LE(height) || u32LE(n) || str(multiaddr_1) .. str(multiaddr_n)
//! signature      = Ed25519(bond server key, M)
//! peer_signature = Ed25519(peer identity key, "wrfcoin_node_address_record_peer_v1" || M)
//! ```
//!
//! All hex is lowercase without `0x`. The server key signs through the same
//! path as the bond-grant request: the dedicated Node bond HSM signer
//! (`bond sign --role server`), or on testnet only the server seed file.
use anyhow::{Result, ensure};
use clap::Subcommand;
use libp2p_identity::Keypair;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
};
use wrfcoin_cli::{ApiClient, ApiConfig, bond_grant};
use wrfcoin_shared_protocol::crypto::{
    signature::{PublicKey, Signature},
    signer::BlockSigner,
};

use super::node_receipt::{
    bounded_read, checked_url, owner_only_dir, parse_u64, retry_later, secret_file,
};

pub const ADDRESS_RECORD_DOMAIN: &[u8] = b"wrfcoin_node_address_record_v1";
pub const ADDRESS_RECORD_PEER_DOMAIN: &[u8] = b"wrfcoin_node_address_record_peer_v1";
pub const ADDRESS_RECORD_VERSION: u32 = 1;
/// At most this many addresses in one record.
pub const MAX_RECORD_ADDRESSES: usize = 4;
/// Longest multiaddr text accepted (the bond-admission service's bound).
pub const MAX_MULTIADDR_BYTES: usize = 200;
/// The mailbox refuses bodies over 4 KiB; a kept record is never larger.
const RECORD_CAP: usize = 4 * 1024;
/// The only network a seed-file server key may publish on.
const FILE_SIGNER_NETWORK: &str = bond_grant::FILE_SIGNER_NETWORK;

#[derive(Subcommand)]
pub enum AddressCommands {
    /// Sign this node's address record for the current epoch with the bond's
    /// server key (and the node's peer key) and post it to the address
    /// mailbox. Meant for the node's existing timer: after the first success
    /// in an epoch it re-posts the same bytes. Exit 0: stored or unchanged.
    /// Exit 2: run again later (mailbox busy, its node behind, 5xx).
    Publish {
        /// This node's registered bond (deployment) id, 64 hex.
        #[clap(long)]
        deployment_id: String,
        /// A public address this node accepts P2P dials on, ending in
        /// `/p2p/<this node's peer id>`: `/ip4|ip6/<addr>/tcp/<port>/p2p/<id>`.
        /// Up to four.
        #[clap(long = "multiaddr", required = true, num_args = 1)]
        multiaddrs: Vec<String>,
        /// Address mailbox route that takes the POST, e.g. `https://wrfcoin.com/api/node-addresses`.
        #[clap(long, required_unless_present = "dry_run")]
        mailbox_url: Option<String>,
        /// This machine's own node.
        #[clap(long, default_value = "http://127.0.0.1:8556")]
        api_url: String,
        #[clap(long)]
        chain_id: String,
        #[clap(long, default_value = "testnet")]
        network_id: String,
        #[clap(long)]
        genesis_hash: String,
        /// Owner-only folder: the signed record for the current epoch
        /// (`address-record-epoch-<E>.json`).
        #[clap(long)]
        state_dir: PathBuf,
        /// The node's software server key (64-hex seed, owner-only). Testnet only.
        /// Without it the dedicated Node bond HSM signer is used, as `bond sign --role server`.
        #[clap(long)]
        server_key_file: Option<PathBuf>,
        /// Protobuf ed25519 peer identity; default $WRFCOIN_DATA_DIR/peer_identity.key.
        #[clap(long)]
        p2p_key_file: Option<PathBuf>,
        /// Owner-only file with the local node's reader/operator bearer, if its reads need one.
        #[clap(long)]
        operator_token_file: Option<PathBuf>,
        /// Build and sign the record and print it; nothing is kept or posted.
        #[clap(long)]
        dry_run: bool,
    },
}

/// The record as posted, field for field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddressRecord {
    pub version: u32,
    pub chain_id: String,
    pub network_id: String,
    pub genesis_hash: String,
    pub deployment_id: String,
    pub server_key: String,
    pub peer_id: String,
    /// Decimal text, like every height and epoch the node API returns.
    pub epoch: String,
    pub height: String,
    pub multiaddrs: Vec<String>,
    pub signature: String,
    pub peer_signature: String,
}

fn text(bytes: &mut Vec<u8>, value: &str) -> Result<()> {
    let len =
        u32::try_from(value.len()).map_err(|_| anyhow::anyhow!("record field exceeds bound"))?;
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn decimal(value: &str, what: &str) -> Result<u64> {
    ensure!(
        !value.is_empty()
            && value.len() <= 20
            && value.bytes().all(|b| b.is_ascii_digit())
            && (value == "0" || !value.starts_with('0')),
        "record {what} must be decimal"
    );
    value
        .parse()
        .map_err(|_| anyhow::anyhow!("record {what} must be decimal"))
}

fn lower_hex(value: &str, len: usize, what: &str) -> Result<()> {
    ensure!(
        value.len() == len
            && value
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "record {what} must be {len} lowercase hex"
    );
    Ok(())
}

impl AddressRecord {
    /// The server-key preimage `M` (module docs).
    pub fn message(&self) -> Result<Vec<u8>> {
        ensure!(
            self.version == ADDRESS_RECORD_VERSION,
            "unsupported record version"
        );
        let count = u32::try_from(self.multiaddrs.len())
            .map_err(|_| anyhow::anyhow!("too many addresses"))?;
        let mut bytes = ADDRESS_RECORD_DOMAIN.to_vec();
        for value in [
            &self.chain_id,
            &self.network_id,
            &self.genesis_hash,
            &self.deployment_id,
            &self.server_key,
            &self.peer_id,
        ] {
            text(&mut bytes, value)?;
        }
        bytes.extend_from_slice(&decimal(&self.epoch, "epoch")?.to_le_bytes());
        bytes.extend_from_slice(&decimal(&self.height, "height")?.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        for addr in &self.multiaddrs {
            text(&mut bytes, addr)?;
        }
        Ok(bytes)
    }

    pub fn peer_message(&self) -> Result<Vec<u8>> {
        Ok([ADDRESS_RECORD_PEER_DOMAIN, self.message()?.as_slice()].concat())
    }

    fn sign(&mut self, signer: &dyn BlockSigner, peer: &Keypair) -> Result<()> {
        ensure!(
            signer.public_key().to_hex() == self.server_key,
            "server signer mismatch"
        );
        ensure!(
            peer.public().to_peer_id().to_string() == self.peer_id,
            "peer identity does not match node"
        );
        let message = self.message()?;
        let signature = signer
            .sign(&message)
            .map_err(|_| anyhow::anyhow!("server signing failed"))?;
        self.signature = signature.to_hex();
        let peer_signature = peer
            .sign(&self.peer_message()?)
            .map_err(|_| anyhow::anyhow!("peer signing failed"))?;
        self.peer_signature = hex::encode(peer_signature);
        self.verify()
    }

    /// Shape plus both signatures; what the mailbox checks before any node read.
    pub fn verify(&self) -> Result<()> {
        for (value, what) in [
            (&self.genesis_hash, "genesis_hash"),
            (&self.deployment_id, "deployment_id"),
            (&self.server_key, "server_key"),
        ] {
            lower_hex(value, 64, what)?;
        }
        lower_hex(&self.signature, 128, "signature")?;
        lower_hex(&self.peer_signature, 128, "peer_signature")?;
        let peer_id: libp2p_identity::PeerId = self
            .peer_id
            .parse()
            .map_err(|_| anyhow::anyhow!("record peer id invalid"))?;
        check_multiaddrs(&self.multiaddrs, &peer_id)?;
        let message = self.message()?;
        let key = PublicKey::from_hex(&self.server_key)
            .map_err(|_| anyhow::anyhow!("record server key invalid"))?;
        let signature = Signature::from_hex(&self.signature)
            .map_err(|_| anyhow::anyhow!("record signature invalid"))?;
        ensure!(
            key.verify(&message, &signature),
            "record server signature does not verify"
        );
        let peer_key =
            libp2p_identity::PublicKey::try_decode_protobuf(peer_id.as_ref().digest())
                .map_err(|_| anyhow::anyhow!("peer id must embed an ed25519 identity key"))?;
        ensure!(
            peer_key.to_peer_id() == peer_id,
            "peer id must embed an ed25519 identity key"
        );
        let peer_signature = hex::decode(&self.peer_signature)?;
        ensure!(
            peer_key.verify(&self.peer_message()?, &peer_signature),
            "record peer signature does not verify"
        );
        Ok(())
    }
}

/// An IPv4 address a stranger's node can be dialled on from the internet.
fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_documentation()
        || a == 0
        || a >= 240
        || (a == 100 && (64..128).contains(&b)) // CGNAT: no port forward, no dial
        || (a == 192 && b == 0 && ip.octets()[2] == 0)
        || (a == 198 && (b == 18 || b == 19)))
}

/// The bond-admission service's rule: global unicast (`2000::/3`) only,
/// minus Teredo (`2001::/32`), documentation (`2001:db8::/32`) and 6to4
/// (`2002::/16`). That also refuses loopback, IPv4-mapped, NAT64, ULA,
/// link-local and multicast.
fn public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    (s[0] & 0xe000) == 0x2000 && !(s[0] == 0x2001 && (s[1] == 0 || s[1] == 0xdb8)) && s[0] != 0x2002
}

/// 1..=4 distinct `/ip4|ip6/<public>/tcp/<port>/p2p/<peer>` addresses, each
/// at most 200 bytes, written in canonical form, all ending in `peer`.
pub fn check_multiaddrs(addrs: &[String], peer: &libp2p_identity::PeerId) -> Result<()> {
    use libp2p::multiaddr::{Multiaddr, Protocol};
    ensure!(
        (1..=MAX_RECORD_ADDRESSES).contains(&addrs.len()),
        "between 1 and {MAX_RECORD_ADDRESSES} addresses"
    );
    for (i, text) in addrs.iter().enumerate() {
        ensure!(
            text.len() <= MAX_MULTIADDR_BYTES,
            "multiaddr longer than {MAX_MULTIADDR_BYTES} bytes"
        );
        ensure!(!addrs[..i].contains(text), "a multiaddr appears twice");
        let addr: Multiaddr = text
            .parse()
            .map_err(|_| anyhow::anyhow!("multiaddr invalid"))?;
        ensure!(
            addr.to_string() == *text,
            "multiaddr must be written in canonical form"
        );
        let parts: Vec<Protocol> = addr.iter().collect();
        let [host, Protocol::Tcp(port), Protocol::P2p(id)] = parts.as_slice() else {
            anyhow::bail!("multiaddr must be /ip4|ip6/<address>/tcp/<port>/p2p/<peer id>");
        };
        ensure!(*port != 0, "multiaddr port invalid");
        ensure!(id == peer, "multiaddr must end in this node's own peer id");
        let public = match host {
            Protocol::Ip4(ip) => public_v4(*ip),
            Protocol::Ip6(ip) => public_v6(*ip),
            _ => anyhow::bail!("multiaddr must be /ip4|ip6/<address>/tcp/<port>/p2p/<peer id>"),
        };
        ensure!(
            public,
            "multiaddr must be a public address (no private, CGNAT, loopback or tunnel range)"
        );
    }
    Ok(())
}

fn id(text: &str, what: &str) -> Result<String> {
    let text = text.trim().to_ascii_lowercase();
    lower_hex(&text, 64, what)?;
    Ok(text)
}

/// `/node/bonds/<id>` names the binding's server key as a 32-number array
/// (serde of `[u8; 32]`); accept hex too, for a future API.
fn registered_server_key(bond: &Value) -> Option<String> {
    let key = &bond["registration"]["binding"]["server_key"];
    if let Some(text) = key.as_str() {
        return Some(text.trim_start_matches("0x").to_ascii_lowercase());
    }
    let bytes: Vec<u8> = key
        .as_array()?
        .iter()
        .map(|b| b.as_u64().and_then(|b| u8::try_from(b).ok()))
        .collect::<Option<_>>()?;
    (bytes.len() == 32).then(|| hex::encode(bytes))
}

fn api_client(base: &str, token_file: Option<&Path>, what: &str) -> Result<ApiClient> {
    checked_url(base, what)?;
    let bearer_token = match token_file {
        Some(path) => {
            let token = secret_file(path, 16 * 1024)?;
            let token = std::str::from_utf8(&token)
                .map_err(|_| anyhow::anyhow!("invalid operator credential"))?
                .trim()
                .to_owned();
            ensure!(!token.is_empty(), "operator credential unavailable"); // secret-leak-ok: constant error, no credential contents.
            Some(token)
        },
        None => None,
    };
    ApiClient::new_without_redirects(ApiConfig {
        base_url: base.trim_end_matches('/').to_owned(),
        bearer_token,
        ..ApiConfig::default()
    })
}

fn durable_record(path: &Path, record: &AddressRecord) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    file.write_all(&serde_json::to_vec(record)?)?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Older epochs' records hold this node's old addresses: drop them.
fn drop_older_records(state_dir: &Path, epoch: u64) {
    let Ok(entries) = std::fs::read_dir(state_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(e) = name
            .to_str()
            .and_then(|n| n.strip_prefix("address-record-epoch-"))
            .and_then(|n| n.strip_suffix(".json"))
            .and_then(|n| n.parse::<u64>().ok())
        else {
            continue;
        };
        if e < epoch {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Mailbox refusals worth another pass later rather than a hard failure.
fn retryable(error: &str) -> bool {
    ["RECEIPT_TRANSPORT", "RECEIPT_HTTP_429", "RECEIPT_HTTP_5"]
        .iter()
        .any(|p| error.starts_with(p))
        || [
            "STALE_EPOCH",
            "FUTURE_EPOCH",
            "FUTURE_HEIGHT",
            "NODE_UNAVAILABLE",
            "BUSY",
        ]
        .iter()
        .any(|code| error.contains(&format!(" {code}")) || error.contains(&format!("_{code}")))
}

pub async fn handle(command: &AddressCommands) -> Result<Value> {
    let AddressCommands::Publish {
        deployment_id,
        multiaddrs,
        mailbox_url,
        api_url,
        chain_id,
        network_id,
        genesis_hash,
        state_dir,
        server_key_file,
        p2p_key_file,
        operator_token_file,
        dry_run,
    } = command;
    let deployment = id(deployment_id, "deployment id")?;
    let genesis = id(genesis_hash.trim_start_matches("0x"), "genesis hash")?;
    ensure!(
        server_key_file.is_none() || network_id == FILE_SIGNER_NETWORK,
        "a server key file is accepted on testnet only"
    );
    let peer = bond_grant::peer_key(
        &p2p_key_file
            .clone()
            .unwrap_or_else(bond_grant::default_peer_key_path),
    )?;
    let peer_id = peer.public().to_peer_id();
    check_multiaddrs(multiaddrs, &peer_id)?;

    let node = api_client(api_url, operator_token_file.as_deref(), "node API")?;
    let info = node.receipt_request("GET", "/network/info", None).await?;
    ensure!(
        info["chain_id"].as_str() == Some(chain_id.as_str())
            && info["network_id"].as_str() == Some(network_id.as_str())
            && info["genesis_hash"]
                .as_str()
                .map(|g| g.trim_start_matches("0x").to_ascii_lowercase())
                == Some(genesis.clone()),
        "node chain, network or genesis differs from the one given"
    );
    ensure!(
        info["local_peer_id"].as_str() == Some(peer_id.to_string().as_str()),
        "peer identity does not match the node's"
    );
    let bond = node
        .receipt_request("GET", &format!("/node/bonds/{deployment}"), None)
        .await?;
    ensure!(
        bond["status"] == "REGISTERED",
        "this bond is not registered on chain"
    );
    ensure!(bond["jailed"] != true, "this bond is jailed");
    let Some(epoch) = parse_u64(&bond["epoch"]) else {
        return Ok(json!({"status":"NOT_ACTIVE","posted":false}));
    };
    let height =
        parse_u64(&bond["height"]).ok_or_else(|| anyhow::anyhow!("node height unavailable"))?;
    let server_key = registered_server_key(&bond)
        .ok_or_else(|| anyhow::anyhow!("node did not report the bond's server key"))?;

    let path = state_dir.join(format!("address-record-epoch-{epoch}.json"));
    let kept = if !dry_run && path.exists() {
        let record: AddressRecord = serde_json::from_slice(&bounded_read(&path, RECORD_CAP)?)?;
        let same = record.verify().is_ok()
            && record.chain_id == *chain_id
            && record.network_id == *network_id
            && record.genesis_hash == genesis
            && record.deployment_id == deployment
            && record.server_key == server_key
            && record.peer_id == peer_id.to_string()
            && record.epoch == epoch.to_string()
            && record.multiaddrs == *multiaddrs;
        same.then_some(record)
    } else {
        None
    };
    let resigned = kept.is_none();
    let record = match kept {
        Some(record) => record,
        None => {
            let signer: std::sync::Arc<dyn BlockSigner> = match server_key_file {
                Some(file) => std::sync::Arc::new(bond_grant::server_key(file)?),
                None => {
                    super::node_bond::resolve_signer()
                        .map_err(|_| anyhow::anyhow!("dedicated Node bond signer unavailable"))?
                        .signer
                },
            };
            ensure!(
                signer.public_key().to_hex() == server_key,
                "the server key does not match this bond's registered server key"
            );
            let mut record = AddressRecord {
                version: ADDRESS_RECORD_VERSION,
                chain_id: chain_id.clone(),
                network_id: network_id.clone(),
                genesis_hash: genesis,
                deployment_id: deployment,
                server_key,
                peer_id: peer_id.to_string(),
                epoch: epoch.to_string(),
                height: height.to_string(),
                multiaddrs: multiaddrs.clone(),
                signature: String::new(),
                peer_signature: String::new(),
            };
            record.sign(signer.as_ref(), &peer)?;
            record
        },
    };
    let summary = json!({"epoch":epoch.to_string(),"height":record.height,"server_key":record.server_key,
        "peer_id":record.peer_id,"addresses":record.multiaddrs.len(),"signed_now":resigned});
    if *dry_run {
        return Ok(json!({"status":"DRY_RUN","posted":false,"summary":summary,"record":record}));
    }
    owner_only_dir(state_dir)?;
    if resigned {
        durable_record(&path, &record)?;
    }
    drop_older_records(state_dir, epoch);

    let mailbox_url = mailbox_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--mailbox-url is required"))?;
    // A public route: never the node's bearer.
    let mailbox = api_client(mailbox_url, None, "mailbox")?;
    let body: Value = serde_json::from_slice(&bounded_read(&path, RECORD_CAP)?)?;
    match mailbox.receipt_request("POST", "", Some(body)).await {
        Ok(answer) => {
            let status = answer["status"].as_str().unwrap_or("STORED").to_owned();
            Ok(
                json!({"status":status,"posted":true,"summary":summary,"record_file":path.display().to_string()}),
            )
        },
        Err(e) if retryable(&e.to_string()) => Err(retry_later(format!("epoch {epoch}: {e}"))),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::crypto::signature::PrivateKey;

    const SEED: [u8; 32] = [11; 32];

    fn server() -> bond_grant::SeedFileServerSigner {
        let dir = std::env::temp_dir().join(format!("addr-seed-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let file = secret(&dir, "server.key", hex::encode(SEED).as_bytes());
        let signer = bond_grant::server_key(&file).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        signer
    }

    fn peer() -> Keypair {
        Keypair::ed25519_from_bytes([2u8; 32]).unwrap()
    }

    fn secret(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        path
    }

    fn record(epoch: u64, height: u64) -> AddressRecord {
        let peer_id = peer().public().to_peer_id().to_string();
        AddressRecord {
            version: 1,
            chain_id: "888".into(),
            network_id: "testnet".into(),
            genesis_hash: "ab".repeat(32),
            deployment_id: "01".repeat(32),
            server_key: PublicKey::from_private_key(&PrivateKey::from_bytes(&SEED).unwrap())
                .to_hex(),
            peer_id: peer_id.clone(),
            epoch: epoch.to_string(),
            height: height.to_string(),
            multiaddrs: vec![format!("/ip4/198.51.99.7/tcp/9000/p2p/{peer_id}")],
            signature: String::new(),
            peer_signature: String::new(),
        }
    }

    /// The mailbox (infra claim-mailbox `address-record.js`) repeats these
    /// exact bytes and signatures; change both together or not at all.
    #[test]
    fn address_record_message_layout_is_frozen() {
        let mut r = record(41, 27_300);
        let message = r.message().unwrap();
        let mut expected = b"wrfcoin_node_address_record_v1".to_vec();
        for value in [
            &r.chain_id,
            &r.network_id,
            &r.genesis_hash,
            &r.deployment_id,
            &r.server_key,
            &r.peer_id,
        ] {
            expected.extend_from_slice(&(value.len() as u32).to_le_bytes());
            expected.extend_from_slice(value.as_bytes());
        }
        expected.extend_from_slice(&41u64.to_le_bytes());
        expected.extend_from_slice(&27_300u64.to_le_bytes());
        expected.extend_from_slice(&1u32.to_le_bytes());
        expected.extend_from_slice(&(r.multiaddrs[0].len() as u32).to_le_bytes());
        expected.extend_from_slice(r.multiaddrs[0].as_bytes());
        assert_eq!(message, expected);
        assert_eq!(
            r.peer_message().unwrap(),
            [b"wrfcoin_node_address_record_peer_v1".as_slice(), &expected].concat()
        );
        r.sign(&server(), &peer()).unwrap();
        // Ed25519 is deterministic: these vectors are pinned in the mailbox tests.
        assert_eq!(
            r.signature,
            "bb8cad257bc2e8ce5f67be29f9757e1c5144e8dabb8080067445a955dfc0f8bc91408e8317a4f9593517702e01a7343fe329b862544e8ee464ae3a389a022a0e"
        );
        assert_eq!(
            r.peer_signature,
            "5a695757d6c8345da86e9dcb73dfc17905971e9cc23492f6b47aaf669aacee956586427b36697c8536abb555425b1f8a1cceb27de39dd53affb1a9649aa5aa08"
        );
    }

    type Tamper<'a> = Box<dyn Fn(&mut AddressRecord) + 'a>;

    #[test]
    fn a_signed_record_verifies_and_every_field_is_bound() {
        let mut r = record(41, 27_300);
        r.sign(&server(), &peer()).unwrap();
        r.verify().unwrap();
        let peer_id = r.peer_id.clone();
        let other_peer = Keypair::ed25519_from_bytes([3u8; 32])
            .unwrap()
            .public()
            .to_peer_id()
            .to_string();
        let tampered: Vec<Tamper<'_>> = vec![
            Box::new(|r| r.chain_id = "777".into()),
            Box::new(|r| r.network_id = "mainnet".into()),
            Box::new(|r| r.genesis_hash = "cd".repeat(32)),
            Box::new(|r| r.deployment_id = "02".repeat(32)),
            Box::new(|r| {
                r.server_key =
                    PublicKey::from_private_key(&PrivateKey::from_bytes(&[12; 32]).unwrap())
                        .to_hex()
            }),
            Box::new(|r| r.epoch = "40".into()),
            Box::new(|r| r.height = "27301".into()),
            Box::new(|r| r.multiaddrs = vec![format!("/ip4/198.51.99.8/tcp/9000/p2p/{peer_id}")]),
            Box::new(|r| {
                r.multiaddrs
                    .push(format!("/ip6/2a01:4f8::1/tcp/9000/p2p/{peer_id}"))
            }),
            Box::new(move |r| {
                r.peer_id = other_peer.clone();
                r.multiaddrs = vec![format!("/ip4/198.51.99.7/tcp/9000/p2p/{other_peer}")];
            }),
            Box::new(|r| r.version = 2),
            Box::new(|r| r.epoch = "041".into()),
        ];
        for (i, change) in tampered.iter().enumerate() {
            let mut bad = r.clone();
            change(&mut bad);
            assert!(bad.verify().is_err(), "tamper {i} still verified");
        }
        // The two signatures are not interchangeable.
        let mut swapped = r.clone();
        std::mem::swap(&mut swapped.signature, &mut swapped.peer_signature);
        assert!(swapped.verify().is_err());
        // The wrong server key cannot sign for this record.
        let mut wrong = record(41, 27_300);
        wrong.server_key =
            PublicKey::from_private_key(&PrivateKey::from_bytes(&[12; 32]).unwrap()).to_hex();
        assert!(wrong.sign(&server(), &peer()).is_err());
    }

    #[test]
    fn only_public_canonical_addresses_ending_in_this_peer() {
        let peer_id = peer().public().to_peer_id();
        let ok = |a: &str| {
            check_multiaddrs(&[a.replace("PEER", &peer_id.to_string())], &peer_id).is_ok()
        };
        assert!(ok("/ip4/198.51.99.7/tcp/9000/p2p/PEER"));
        assert!(ok("/ip6/2a01:4f8::1/tcp/9000/p2p/PEER"));
        for bad in [
            "/ip4/10.0.0.5/tcp/9000/p2p/PEER",
            "/ip4/192.168.0.110/tcp/9000/p2p/PEER",
            "/ip4/100.64.1.1/tcp/9000/p2p/PEER",
            "/ip4/127.0.0.1/tcp/9000/p2p/PEER",
            "/ip4/203.0.113.7/tcp/9000/p2p/PEER",
            "/ip4/198.51.99.7/tcp/0/p2p/PEER",
            "/ip6/::1/tcp/9000/p2p/PEER",
            "/ip6/::ffff:198.51.99.7/tcp/9000/p2p/PEER",
            "/ip6/fd00::1/tcp/9000/p2p/PEER",
            "/ip6/fe80::1/tcp/9000/p2p/PEER",
            "/ip6/2002:c633:6307::1/tcp/9000/p2p/PEER",
            "/ip6/2001:0:1::1/tcp/9000/p2p/PEER",
            "/ip6/2001:db8::1/tcp/9000/p2p/PEER",
            "/dns4/node.example.org/tcp/9000/p2p/PEER",
            "/ip4/198.51.99.7/udp/9000/quic-v1/p2p/PEER",
            "/ip4/198.51.99.7/tcp/9000",
            "/ip6/2A01:4F8::1/tcp/9000/p2p/PEER",
        ] {
            assert!(!ok(bad), "{bad} accepted");
        }
        let other = Keypair::ed25519_from_bytes([3u8; 32])
            .unwrap()
            .public()
            .to_peer_id();
        assert!(
            check_multiaddrs(
                &[format!("/ip4/198.51.99.7/tcp/9000/p2p/{other}")],
                &peer_id
            )
            .is_err()
        );
        let one = format!("/ip4/198.51.99.7/tcp/9000/p2p/{peer_id}");
        assert!(check_multiaddrs(&[one.clone(), one.clone()], &peer_id).is_err());
        assert!(check_multiaddrs(&[], &peer_id).is_err());
        let five: Vec<String> = (1..=5)
            .map(|i| format!("/ip4/198.51.99.{i}/tcp/9000/p2p/{peer_id}"))
            .collect();
        assert!(check_multiaddrs(&five, &peer_id).is_err());
        assert!(check_multiaddrs(&five[..4], &peer_id).is_ok());
    }

    struct Fixture {
        root: PathBuf,
        node: mockito::ServerGuard,
        mailbox: mockito::ServerGuard,
        peer_id: String,
    }

    fn fixture(tag: &str) -> Fixture {
        let root =
            std::env::temp_dir().join(format!("addr-publish-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        secret(&root, "server.key", hex::encode(SEED).as_bytes()); // secret-leak-ok: synthetic test seed.
        secret(&root, "peer.key", &peer().to_protobuf_encoding().unwrap());
        Fixture {
            root,
            node: mockito::Server::new(),
            mailbox: mockito::Server::new(),
            peer_id: peer().public().to_peer_id().to_string(),
        }
    }

    fn command(f: &Fixture, addrs: &[&str], dry_run: bool) -> AddressCommands {
        AddressCommands::Publish {
            deployment_id: "01".repeat(32),
            multiaddrs: addrs
                .iter()
                .map(|a| a.replace("PEER", &f.peer_id))
                .collect(),
            mailbox_url: Some(format!("{}/addresses", f.mailbox.url())),
            api_url: f.node.url(),
            chain_id: "888".into(),
            network_id: "testnet".into(),
            genesis_hash: "ab".repeat(32),
            state_dir: f.root.join("state"),
            server_key_file: Some(f.root.join("server.key")),
            p2p_key_file: Some(f.root.join("peer.key")),
            operator_token_file: None,
            dry_run,
        }
    }

    fn node_mocks(
        f: &mut Fixture,
        epoch: u64,
        height: u64,
        server_seed: [u8; 32],
        status: &str,
    ) -> Vec<mockito::Mock> {
        let key = PublicKey::from_private_key(&PrivateKey::from_bytes(&server_seed).unwrap());
        let key_array: Vec<u8> = key.as_bytes().to_vec();
        vec![
            f.node
                .mock("GET", "/network/info")
                .with_body(json!({"chain_id":"888","network_id":"testnet","genesis_hash":format!("0x{}","ab".repeat(32)),
                    "local_peer_id":f.peer_id,"chain_height":height}).to_string())
                .create(),
            f.node
                .mock("GET", format!("/node/bonds/{}", "01".repeat(32)).as_str())
                .with_body(json!({"height":height.to_string(),"epoch":epoch.to_string(),"status":status,"jailed":false,
                    "registration":{"binding":{"server_key":key_array}}}).to_string())
                .create(),
        ]
    }

    fn run(cmd: &AddressCommands) -> Result<Value> {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(handle(cmd))
    }

    #[test]
    fn publish_signs_once_per_epoch_reposts_the_same_bytes_and_resigns_on_change() {
        let mut f = fixture("cycle");
        let addrs = ["/ip4/198.51.99.7/tcp/9000/p2p/PEER"];
        let state = f.root.join("state");

        // Pass 1: signed at the node's height, kept, posted without any bearer.
        let node = node_mocks(&mut f, 41, 27_300, SEED, "REGISTERED");
        let post = f
            .mailbox
            .mock("POST", "/addresses")
            .match_header("authorization", mockito::Matcher::Missing)
            .with_body_from_request(|request| {
                let r: AddressRecord = serde_json::from_slice(request.body().unwrap()).unwrap();
                r.verify().unwrap();
                assert_eq!((r.epoch.as_str(), r.height.as_str()), ("41", "27300"));
                json!({"ok":true,"data":{"status":"STORED"}})
                    .to_string()
                    .into_bytes()
            })
            .expect(1)
            .create();
        let out = run(&command(&f, &addrs, false)).unwrap();
        assert_eq!(out["status"], "STORED");
        assert_eq!(out["summary"]["signed_now"], true);
        post.assert();
        let kept = std::fs::read(state.join("address-record-epoch-41.json")).unwrap();
        drop((post, node));

        // Pass 2, later in the same epoch: the SAME bytes, no new signature.
        let node = node_mocks(&mut f, 41, 27_350, SEED, "REGISTERED");
        let same: Value = serde_json::from_slice(&kept).unwrap();
        let post = f
            .mailbox
            .mock("POST", "/addresses")
            .match_body(mockito::Matcher::Json(same))
            .with_body(json!({"ok":true,"data":{"status":"UNCHANGED"}}).to_string())
            .expect(1)
            .create();
        let out = run(&command(&f, &addrs, false)).unwrap();
        assert_eq!(out["status"], "UNCHANGED");
        assert_eq!(out["summary"]["signed_now"], false);
        post.assert();
        drop((post, node));

        // Pass 3: the home address changed mid-epoch: a new record at the current height.
        let node = node_mocks(&mut f, 41, 27_400, SEED, "REGISTERED");
        let post = f
            .mailbox
            .mock("POST", "/addresses")
            .with_body_from_request(|request| {
                let r: AddressRecord = serde_json::from_slice(request.body().unwrap()).unwrap();
                r.verify().unwrap();
                assert_eq!(r.height, "27400");
                assert!(r.multiaddrs[0].starts_with("/ip4/198.51.99.9/"));
                json!({"ok":true,"data":{"status":"STORED"}})
                    .to_string()
                    .into_bytes()
            })
            .expect(1)
            .create();
        run(&command(&f, &["/ip4/198.51.99.9/tcp/9000/p2p/PEER"], false)).unwrap();
        post.assert();
        drop((post, node));

        // Pass 4: next epoch: a new record, and the old epoch's file (old address) is gone.
        let node = node_mocks(&mut f, 42, 27_900, SEED, "REGISTERED");
        let post = f
            .mailbox
            .mock("POST", "/addresses")
            .with_body(json!({"ok":true,"data":{"status":"STORED"}}).to_string())
            .expect(1)
            .create();
        run(&command(&f, &["/ip4/198.51.99.9/tcp/9000/p2p/PEER"], false)).unwrap();
        post.assert();
        assert!(state.join("address-record-epoch-42.json").exists());
        assert!(!state.join("address-record-epoch-41.json").exists());
        drop((post, node));
        std::fs::remove_dir_all(&f.root).unwrap();
    }

    #[test]
    fn publish_refuses_a_wrong_chain_key_bond_or_peer_and_retries_a_busy_mailbox() {
        let mut f = fixture("refuse");
        let addrs = ["/ip4/198.51.99.7/tcp/9000/p2p/PEER"];
        let no_post = f.mailbox.mock("POST", "/addresses").expect(0).create();

        // The bond names another server key.
        let node = node_mocks(&mut f, 41, 27_300, [12; 32], "REGISTERED");
        let err = run(&command(&f, &addrs, false)).unwrap_err();
        assert!(err.to_string().contains("registered server key"), "{err}");
        drop(node);

        // The bond is not registered.
        let node = node_mocks(&mut f, 41, 27_300, SEED, "UNKNOWN");
        assert!(
            run(&command(&f, &addrs, false))
                .unwrap_err()
                .to_string()
                .contains("not registered")
        );
        drop(node);

        // Another chain than the operator named.
        let node = node_mocks(&mut f, 41, 27_300, SEED, "REGISTERED");
        let mut cmd = command(&f, &addrs, false);
        let AddressCommands::Publish { chain_id, .. } = &mut cmd;
        *chain_id = "777".into();
        assert!(
            run(&cmd)
                .unwrap_err()
                .to_string()
                .contains("chain, network or genesis")
        );

        // A private address, or another node's peer id, never reaches the node or the mailbox.
        assert!(
            run(&command(
                &f,
                &["/ip4/192.168.0.110/tcp/9000/p2p/PEER"],
                false
            ))
            .is_err()
        );
        let other = Keypair::ed25519_from_bytes([3u8; 32])
            .unwrap()
            .public()
            .to_peer_id()
            .to_string();
        assert!(
            run(&command(
                &f,
                &[&format!("/ip4/198.51.99.7/tcp/9000/p2p/{other}")],
                false
            ))
            .is_err()
        );

        // A seed file off testnet is refused before anything is read.
        let mut cmd = command(&f, &addrs, false);
        let AddressCommands::Publish { network_id, .. } = &mut cmd;
        *network_id = "mainnet".into();
        assert!(run(&cmd).unwrap_err().to_string().contains("testnet only"));
        no_post.assert();
        drop(no_post);

        // Dry run: signed and printed, nothing kept or posted.
        let no_post = f.mailbox.mock("POST", "/addresses").expect(0).create();
        let out = run(&command(&f, &addrs, true)).unwrap();
        assert_eq!(out["status"], "DRY_RUN");
        let r: AddressRecord = serde_json::from_value(out["record"].clone()).unwrap();
        r.verify().unwrap();
        assert!(!f.root.join("state").exists());
        no_post.assert();
        drop(no_post);

        // The mailbox is busy, or its node is an epoch behind: exit 2, run again later.
        for (status, code) in [
            (429, "BUSY"),
            (409, "FUTURE_EPOCH"),
            (503, "NODE_UNAVAILABLE"),
        ] {
            let post = f
                .mailbox
                .mock("POST", "/addresses")
                .with_status(status)
                .with_body(
                    json!({"ok":false,"error":{"code":code,"message":"try later"}}).to_string(),
                )
                .expect(1)
                .create();
            let err = run(&command(&f, &addrs, false)).unwrap_err();
            assert_eq!(crate::commands::emit::classify_exit_code(&err), 2, "{err}");
            post.assert();
        }
        // A hard refusal (not bonded per the mailbox) is not retried.
        let post = f
            .mailbox
            .mock("POST", "/addresses")
            .with_status(403)
            .with_body(
                json!({"ok":false,"error":{"code":"NOT_BONDED","message":"no such bond"}})
                    .to_string(),
            )
            .expect(1)
            .create();
        let err = run(&command(&f, &addrs, false)).unwrap_err();
        assert_ne!(crate::commands::emit::classify_exit_code(&err), 2, "{err}");
        post.assert();
        drop((post, node));
        std::fs::remove_dir_all(&f.root).unwrap();
    }
}
