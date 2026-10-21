//! `light_node` (#2188): follow a WRFCoin node by headers and verify without
//! trusting it.
//!
//! ```text
//! light_node --node http://127.0.0.1:8556 --trust trust.json \
//!            [--secret node-secret.bin] [--da required|when-committed|off] \
//!            [--trust-served-da-root] [--poll-secs 10] [--max N] [--checkpoint ckpt.json]
//! ```
//!
//! `trust.json` is the operator-verified starting point (NEVER fetched from
//! the node being followed):
//!
//! ```json
//! { "height": 0, "hash_hex": "<64 hex>",
//!   "producer_history": [ { "epoch": 0, "effective_height": 0,
//!       "members": [ { "producer_id": "mango", "payout_address": "0x…", "public_key_hex": "<64 hex>" } ] } ] }
//! ```
//!
//! Every header is accepted only after the checks in
//! `wrfcoin_da::light_client`: recomputed hash, chain linkage, producer
//! signature by a member of the tracked set, set transitions signed by a
//! current member, optional finality certificate, and data-availability
//! sampling when the header commits a DA root (#2185). `--trust-served-da-root`
//! opts into sampling against the root the *server* computes — labelled as
//! trusting the server, off by default.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use wrfcoin_da::{
    ChunkResponse, DaCommitment, DaPolicy, HeaderCommit, HeaderSource, LightCheckpoint,
    LightClient, LightClientConfig, PeerPenalty, SampleFetchError, SamplePenalty, SampleSource,
    SamplingConfig, SyncError,
};
use wrfcoin_shared_protocol::blockchain::light_header::LightHeader;
use wrfcoin_shared_protocol::consensus::producer_set::{ProducerMember, ProducerSetChange};
use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;
use wrfcoin_shared_protocol::crypto::hash::Hash;

#[derive(Debug, Deserialize)]
struct TrustMember {
    producer_id: String,
    payout_address: String,
    public_key_hex: String,
}

#[derive(Debug, Deserialize)]
struct TrustChange {
    epoch: u64,
    effective_height: u64,
    members: Vec<TrustMember>,
}

#[derive(Debug, Deserialize)]
struct TrustFile {
    height: u64,
    hash_hex: String,
    producer_history: Vec<TrustChange>,
}

impl TrustFile {
    fn into_checkpoint(self) -> Result<LightCheckpoint, String> {
        let bytes = hex::decode(self.hash_hex.trim()).map_err(|e| format!("hash_hex: {e}"))?;
        let hash: [u8; 32] = bytes
            .try_into()
            .map_err(|_| "hash_hex must be 32 bytes".to_string())?;
        let mut history = Vec::with_capacity(self.producer_history.len());
        for change in self.producer_history {
            let mut members = Vec::with_capacity(change.members.len());
            for m in change.members {
                let key = hex::decode(m.public_key_hex.trim())
                    .map_err(|e| format!("member {}: public_key_hex: {e}", m.producer_id))?;
                let public_key: [u8; 32] = key.try_into().map_err(|_| {
                    format!("member {}: public key must be 32 bytes", m.producer_id)
                })?;
                members.push(ProducerMember {
                    producer_id: m.producer_id,
                    payout_address: m.payout_address,
                    public_key,
                });
            }
            history.push(ProducerSetChange::new(
                change.epoch,
                change.effective_height,
                members,
            ));
        }
        Ok(LightCheckpoint {
            height: self.height,
            hash: Hash::from_bytes(hash),
            producer_history: history,
        })
    }
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct HeaderData {
    header: LightHeader,
    da_commitment: DaCommitment,
}

#[derive(Debug, Deserialize)]
struct ChunkData {
    proof: wrfcoin_da::ChunkInclusionProof,
}

struct HttpSource {
    client: reqwest::Client,
    node: String,
    trust_served_da_root: bool,
}

#[async_trait]
impl HeaderSource for HttpSource {
    async fn header_at(
        &self,
        height: u64,
    ) -> Result<Option<(LightHeader, HeaderCommit)>, SyncError> {
        let url = format!("{}/chain/header/{height}", self.node);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| SyncError::Source(format!("GET {url}: {e}")))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let env: Envelope<HeaderData> = resp
            .json()
            .await
            .map_err(|e| SyncError::Source(format!("GET {url}: body: {e}")))?;
        let Some(data) = env.data.filter(|_| env.ok) else {
            return Err(SyncError::Source(format!("GET {url}: {:?}", env.error)));
        };
        // The commitment a light node samples against comes from the header's
        // own metadata (#2185); the served root is used only when the operator
        // explicitly chose to trust this server for it.
        let da_commitment =
            wrfcoin_da::parse_da_commitment(&data.header).or(if self.trust_served_da_root {
                Some(data.da_commitment)
            } else {
                None
            });
        Ok(Some((
            data.header,
            HeaderCommit {
                da_commitment,
                ..Default::default()
            },
        )))
    }
}

#[async_trait]
impl SampleSource for HttpSource {
    async fn fetch(
        &self,
        _peer: &str,
        height: u64,
        index: u16,
    ) -> Result<ChunkResponse, SampleFetchError> {
        let url = format!("{}/da/chunk/{height}/{index}", self.node);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| SampleFetchError::Transport(e.to_string()))?;
        match resp.status() {
            reqwest::StatusCode::NOT_FOUND => return Err(SampleFetchError::Missing),
            reqwest::StatusCode::TOO_MANY_REQUESTS | reqwest::StatusCode::SERVICE_UNAVAILABLE => {
                return Err(SampleFetchError::Refused(resp.status().to_string()));
            },
            _ => {},
        }
        let env: Envelope<ChunkData> = resp
            .json()
            .await
            .map_err(|e| SampleFetchError::Transport(format!("body: {e}")))?;
        match env.data {
            Some(data) if env.ok => Ok(ChunkResponse { proof: data.proof }),
            _ => Err(SampleFetchError::Refused(format!("{:?}", env.error))),
        }
    }
}

struct LogPenalty;

impl PeerPenalty for LogPenalty {
    fn penalize(&self, peer: &str, reason: SamplePenalty) {
        eprintln!("[light_node] penalty: peer {peer}: {reason:?}");
    }
}

#[derive(Debug, Serialize)]
struct Report<'a> {
    trusted_height: u64,
    trusted_hash: String,
    stats: &'a wrfcoin_da::LightClientStats,
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn usage() -> ! {
    eprintln!(
        // secret-leak-ok: static usage text; `--secret <file>` is the flag name, no secret value is interpolated
        "usage: light_node --node <url> --trust <trust.json> [--secret <file>] \
         [--da required|when-committed|off] [--trust-served-da-root] [--poll-secs N] \
         [--max N] [--checkpoint <file>] [--chain-id ID --network-id ID]"
    );
    std::process::exit(64);
}

/// Load this node's private sampling secret, or create one.
///
/// The secret is what keeps sample indices unpredictable to the serving node
/// (#3124), so (review of core4#3128):
/// * a new secret file is created exclusively with mode 0600 (owner only);
/// * an existing file that is too short, or cannot be read, is REFUSED rather
///   than silently replaced: replacing it would hide a truncated or tampered
///   secret and change which chunks this node samples.
fn load_or_create_secret_at(path: &std::path::Path) -> Result<Vec<u8>, String> {
    match std::fs::read(path) {
        Ok(bytes) if bytes.len() >= wrfcoin_da::NODE_SECRET_MIN_LEN => return Ok(bytes),
        Ok(bytes) => {
            return Err(format!(
                // secret-leak-ok: reports only the file path, the observed length and the minimum constant, never secret bytes
                "node secret {} is {} bytes, shorter than the {}-byte minimum; refusing to replace it (remove it deliberately to generate a new one)",
                path.display(),
                bytes.len(),
                wrfcoin_da::NODE_SECRET_MIN_LEN
            ));
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
        Err(e) => return Err(format!("cannot read node secret {}: {e}", path.display())),
    }
    let mut secret = vec![0u8; 32];
    // determinism-ok: the light node's private sampling secret, generated once on this client and never sent anywhere; it selects which chunks THIS node samples and touches no consensus state
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut secret);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options
        .open(path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, &secret));
    if let Err(e) = written {
        eprintln!(
            "[light_node] could not persist node secret to {}: {e} (using an ephemeral secret for this run)",
            path.display()
        );
    }
    Ok(secret)
}

fn load_or_create_secret(path: Option<PathBuf>) -> Vec<u8> {
    let path = path.unwrap_or_else(|| PathBuf::from("light-node-secret.bin"));
    load_or_create_secret_at(&path).unwrap_or_else(|e| {
        eprintln!("[light_node] {e}");
        std::process::exit(78)
    })
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let node = arg(&args, "--node").unwrap_or_else(|| usage());
    let trust_path = arg(&args, "--trust").unwrap_or_else(|| usage());
    let da_policy = match arg(&args, "--da").as_deref() {
        None | Some("when-committed") => DaPolicy::WhenCommitted,
        Some("required") => DaPolicy::Required,
        Some("off") => DaPolicy::Off,
        Some(other) => {
            eprintln!("unknown --da value {other:?}");
            usage()
        },
    };
    let trust_served_da_root = args.iter().any(|a| a == "--trust-served-da-root");
    let poll_secs: u64 = arg(&args, "--poll-secs")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let max: u64 = arg(&args, "--max")
        .and_then(|v| v.parse().ok())
        .unwrap_or(u64::MAX);
    let checkpoint_path = arg(&args, "--checkpoint").map(PathBuf::from);
    let secret = load_or_create_secret(arg(&args, "--secret").map(PathBuf::from));
    // determinism-ok: light-node client identity selection; this binary verifies headers and never produces state
    let chain_id_env = std::env::var("WRFCOIN_CHAIN_ID").ok();
    // determinism-ok: light-node client identity selection; this binary verifies headers and never produces state
    let network_id_env = std::env::var("WRFCOIN_NETWORK_ID").ok();
    let chain_id = arg(&args, "--chain-id").or(chain_id_env);
    let network_id = arg(&args, "--network-id").or(network_id_env);
    let (Some(chain_id), Some(network_id)) = (chain_id, network_id) else {
        eprintln!(
            "chain identity required: --chain-id/--network-id or WRFCOIN_CHAIN_ID/WRFCOIN_NETWORK_ID"
        );
        usage()
    };

    // Resume from a saved checkpoint when one exists; otherwise start from trust.
    let checkpoint = match checkpoint_path.as_ref().and_then(|p| std::fs::read(p).ok()) {
        Some(bytes) => serde_json::from_slice::<LightCheckpoint>(&bytes).unwrap_or_else(|e| {
            eprintln!("checkpoint file unreadable: {e}");
            std::process::exit(65)
        }),
        None => {
            let raw = std::fs::read_to_string(&trust_path).unwrap_or_else(|e| {
                eprintln!("cannot read {trust_path}: {e}");
                std::process::exit(66)
            });
            let trust: TrustFile = serde_json::from_str(&raw).unwrap_or_else(|e| {
                eprintln!("trust file invalid: {e}");
                std::process::exit(65)
            });
            trust.into_checkpoint().unwrap_or_else(|e| {
                eprintln!("trust file invalid: {e}");
                std::process::exit(65)
            })
        },
    };

    let cfg = LightClientConfig {
        sampling: SamplingConfig::default(),
        identity: ChainIdentity::new(&chain_id, &network_id),
        da_policy,
    };
    let mut client = LightClient::new(checkpoint, secret, cfg).unwrap_or_else(|e| {
        eprintln!("refusing to start: {e}");
        std::process::exit(65)
    });
    let source = HttpSource {
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .expect("reqwest client"),
        node: node.trim_end_matches('/').to_string(),
        trust_served_da_root,
    };
    // One HTTP node is one peer; the sampler's distinct-peer corroboration
    // needs `distinct_peers_required` distinct servers. With a single node
    // the outcome is honest: a missing chunk reads Inconclusive, not
    // Unavailable, so a lone server cannot be accused — nor trusted.
    let peers = vec![source.node.clone()];

    eprintln!(
        "[light_node] following {} from trusted height {} ({}), da={da_policy:?}, served-root-trusted={trust_served_da_root}",
        source.node,
        client.trusted_height(),
        client.trusted_hash()
    );
    let mut total = 0u64;
    loop {
        match client
            .follow(
                &source,
                &source,
                &LogPenalty,
                &peers,
                max.saturating_sub(total),
            )
            .await
        {
            Ok(n) => {
                total += n;
                if n > 0 {
                    let report = Report {
                        trusted_height: client.trusted_height(),
                        trusted_hash: client.trusted_hash().to_string(),
                        stats: client.stats(),
                    };
                    println!(
                        "{}",
                        serde_json::to_string(&report).expect("report serializes")
                    );
                    if let Some(path) = &checkpoint_path {
                        if let Err(e) = std::fs::write(
                            path,
                            serde_json::to_vec(client.checkpoint()).expect("checkpoint"),
                        ) {
                            eprintln!("[light_node] checkpoint not saved: {e}");
                        }
                    }
                }
            },
            Err(SyncError::DataInconclusive { height, reason }) => {
                eprintln!(
                    "[light_node] height {height}: data availability inconclusive ({reason}); retrying"
                );
            },
            Err(e) => {
                eprintln!(
                    "[light_node] REFUSED at height {}: {e}",
                    client.trusted_height() + 1
                );
                std::process::exit(70);
            },
        }
        if total >= max {
            break;
        }
        tokio::time::sleep(Duration::from_secs(poll_secs)).await;
    }
}

#[cfg(test)]
mod secret_file_tests {
    use super::load_or_create_secret_at;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "light-node-secret-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("secret.bin")
    }

    #[cfg(unix)]
    #[test]
    fn a_new_secret_is_created_owner_only_and_reused() {
        use std::os::unix::fs::PermissionsExt;
        let path = scratch("create");
        let first = load_or_create_secret_at(&path).unwrap();
        assert_eq!(first.len(), 32);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the key file must be owner-only"); // secret-leak-ok: test; prints only the file mode
        // Compared without printing either value on failure.
        assert!(
            load_or_create_secret_at(&path).unwrap() == first,
            "an existing file is reused"
        ); // secret-leak-ok: test; assert! with a static message never prints the compared bytes
    }

    #[test]
    fn a_too_short_existing_secret_is_refused_not_replaced() {
        let path = scratch("short");
        std::fs::write(&path, b"short").unwrap();
        assert!(load_or_create_secret_at(&path).is_err()); // secret-leak-ok: test; asserts only that the load is refused
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"short",
            "the file is left untouched"
        );
    }
}
