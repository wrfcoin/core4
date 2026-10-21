use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rustc_hash::{FxHashMap, FxHashSet};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, broadcast};
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::config::settings::Config;
use crate::state::RECENT_DA_BLOCK_WINDOW;
use wrfcoin_p2p::{
    ContentRef, DataAvailabilityChallenge, DataAvailabilityChallengeResponse, P2pClient,
    P2pCommand, compute_proof, generate_challenge, verify_challenge_response,
    CHALLENGE_FRAGMENT_MAX_BYTES,
};

const DA_FAILURE_CHANNEL_CAPACITY: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaChallengeFailure {
    pub peer_id: String,
    pub fragment_id: String,
    pub reason: String,
}

#[derive(Clone, Debug)]
struct InflightDaChallenge {
    peer_id: String,
    challenge: DataAvailabilityChallenge,
    expected_fragment: Vec<u8>,
    deadline: Instant,
}

#[derive(Clone)]
pub struct DaChallengeRuntime {
    pub active_peers: Arc<RwLock<FxHashSet<String>>>,
    pub fragment_cache: Arc<RwLock<FxHashMap<String, Vec<u8>>>>,
    inflight: Arc<RwLock<FxHashMap<String, InflightDaChallenge>>>,
    failure_tx: broadcast::Sender<DaChallengeFailure>,
}

impl DaChallengeRuntime {
    pub fn new() -> Self {
        let (failure_tx, _) = broadcast::channel(DA_FAILURE_CHANNEL_CAPACITY);
        Self {
            active_peers: Arc::new(RwLock::new(FxHashSet::default())),
            fragment_cache: Arc::new(RwLock::new(FxHashMap::default())),
            inflight: Arc::new(RwLock::new(FxHashMap::default())),
            failure_tx,
        }
    }

    pub fn subscribe_failures(&self) -> broadcast::Receiver<DaChallengeFailure> {
        self.failure_tx.subscribe()
    }

    pub async fn record_peer_connected(&self, peer_id: String) {
        self.active_peers.write().await.insert(peer_id);
    }

    pub async fn record_peer_disconnected(&self, peer_id: &str) {
        self.active_peers.write().await.remove(peer_id);
    }

    pub async fn cache_content_ref_payload(
        &self,
        content_ref: &ContentRef,
        payload: &[u8],
        source: &str,
    ) {
        if !payload_matches_content_ref(content_ref, payload) {
            log::warn!(
                "Skipping DA fragment cache insert for cid={} from {source}: payload/hash metadata mismatch",
                content_ref.data_cid
            );
            return;
        }

        let mut cache = self.fragment_cache.write().await;
        if let Some(existing_payload) = cache.get(&content_ref.data_cid) {
            if existing_payload.as_slice() != payload {
                log::warn!(
                    "Rejecting DA fragment cache overwrite for cid={} from {source}: conflicting payload for existing CID",
                    content_ref.data_cid
                );
            }
            return;
        }

        cache.insert(content_ref.data_cid.clone(), payload.to_vec());
    }

    pub async fn handle_inbound_challenge(
        &self,
        peer_id: &str,
        challenge: DataAvailabilityChallenge,
        p2p: Option<&P2pClient>,
    ) {
        let Some(client) = p2p else {
            return;
        };

        let fragment = {
            let cache = self.fragment_cache.read().await;
            cache.get(&challenge.cid)
                .and_then(|data| slice_fragment(data, challenge.offset, challenge.length))
        };

        let Some(fragment) = fragment else {
            log::debug!(
                "Ignoring DA challenge {} from {peer_id}: local fragment for cid={} is unavailable",
                challenge.challenge_id,
                challenge.cid
            );
            return;
        };

        let response = DataAvailabilityChallengeResponse {
            challenge_id: challenge.challenge_id.clone(),
            data_fragment: fragment.clone(),
            proof: compute_proof(&challenge.nonce, &fragment),
            keeper_id: client.local_peer_id.to_string(),
            responded_at: chrono::Utc::now().timestamp(),
        };

        if let Err(err) = client
            .command_tx
            .send(P2pCommand::BroadcastDaChallengeResponse { response })
            .await
        {
            log::warn!(
                "Failed to broadcast DA challenge response {}: {err}",
                challenge.challenge_id
            );
        }
    }

    pub async fn handle_inbound_response(
        &self,
        peer_id: &str,
        response: DataAvailabilityChallengeResponse,
    ) {
        let tracked = {
            self.inflight
                .read()
                .await
                .get(&response.challenge_id)
                .cloned()
        };

        let Some(tracked) = tracked else {
            log::debug!(
                "Ignoring DA challenge response {} from {peer_id}: no in-flight challenge",
                response.challenge_id
            );
            return;
        };

        let response_matches_target = peer_id == tracked.peer_id;
        if !response_matches_target {
            log::debug!(
                "Ignoring DA challenge response {} from {peer_id}: expected peer {}",
                response.challenge_id,
                tracked.peer_id
            );
            return;
        }

        if tracked.deadline <= Instant::now() {
            self.remove_inflight(&response.challenge_id).await;
            self.emit_failure(DaChallengeFailure {
                peer_id: tracked.peer_id.clone(),
                fragment_id: fragment_id(&tracked.challenge),
                reason: "challenge timed out before response arrived".to_string(),
            });
            return;
        }

        match verify_challenge_response(&tracked.challenge, &response, &tracked.expected_fragment) {
            Ok(()) => {
                self.remove_inflight(&response.challenge_id).await;
                log::info!(
                    "Verified DA challenge response {} from peer {}",
                    response.challenge_id,
                    tracked.peer_id
                );
            },
            Err(err) => {
                self.remove_inflight(&response.challenge_id).await;
                self.emit_failure(DaChallengeFailure {
                    peer_id: tracked.peer_id.clone(),
                    fragment_id: fragment_id(&tracked.challenge),
                    reason: err.to_string(),
                });
            },
        }
    }

    pub async fn emit_timeouts(&self, now: Instant) {
        let expired = {
            let mut inflight = self.inflight.write().await;
            let expired_ids: Vec<String> = inflight
                .iter()
                .filter(|(_, tracked)| tracked.deadline <= now)
                .map(|(challenge_id, _)| challenge_id.clone())
                .collect();

            expired_ids
                .into_iter()
                .filter_map(|challenge_id| inflight.remove(&challenge_id))
                .collect::<Vec<_>>()
        };

        for tracked in expired {
            self.emit_failure(DaChallengeFailure {
                peer_id: tracked.peer_id,
                fragment_id: fragment_id(&tracked.challenge),
                reason: "challenge timed out waiting for response".to_string(),
            });
        }
    }

    async fn track_challenge(
        &self,
        peer_id: String,
        challenge: DataAvailabilityChallenge,
        expected_fragment: Vec<u8>,
        timeout: Duration,
    ) {
        self.inflight.write().await.insert(
            challenge.challenge_id.clone(),
            InflightDaChallenge {
                peer_id,
                challenge,
                expected_fragment,
                deadline: Instant::now() + timeout,
            },
        );
    }

    async fn remove_inflight(&self, challenge_id: &str) {
        self.inflight.write().await.remove(challenge_id);
    }

    fn emit_failure(&self, failure: DaChallengeFailure) {
        log::warn!(
            "DA challenge failure emitted: peer={} fragment={} reason={}",
            failure.peer_id,
            failure.fragment_id,
            failure.reason
        );
        let _ = self.failure_tx.send(failure);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DaProducerConfig {
    pub interval_secs: u64,
    pub peer_count: usize,
    pub timeout_secs: u64,
}

pub fn da_producer_config_from_env(defaults: &Config) -> DaProducerConfig {
    DaProducerConfig {
        interval_secs: parse_env_or_default(
            "WRFCOIN_DA_CHALLENGE_INTERVAL_SECS",
            defaults.da_challenge_interval_secs,
        ),
        peer_count: parse_env_or_default(
            "WRFCOIN_DA_CHALLENGE_PEER_COUNT",
            defaults.da_challenge_peer_count,
        ),
        timeout_secs: parse_env_or_default(
            "WRFCOIN_DA_CHALLENGE_TIMEOUT_SECS",
            defaults.da_challenge_timeout_secs,
        ),
    }
}

pub async fn run_da_challenge_producer(
    blockchain: Arc<RwLock<Blockchain>>,
    p2p: P2pClient,
    runtime: DaChallengeRuntime,
    config: DaProducerConfig,
) {
    if config.interval_secs == 0 || config.peer_count == 0 {
        log::info!(
            "DA challenge producer disabled (interval_secs={}, peer_count={})",
            config.interval_secs,
            config.peer_count
        );
        return;
    }

    let timeout = Duration::from_secs(config.timeout_secs.max(1));
    let mut producer_tick = tokio::time::interval(Duration::from_secs(config.interval_secs));
    producer_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    producer_tick.tick().await;

    let sweep_period = Duration::from_secs(config.timeout_secs.clamp(1, 5));
    let mut timeout_tick = tokio::time::interval(sweep_period);
    timeout_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    timeout_tick.tick().await;

    log::info!(
        "DA challenge producer enabled: interval={}s peer_count={} timeout={}s",
        config.interval_secs,
        config.peer_count,
        config.timeout_secs
    );

    loop {
        tokio::select! {
            _ = producer_tick.tick() => {
                issue_random_challenges(&blockchain, &p2p, &runtime, config.peer_count, timeout).await;
            }
            _ = timeout_tick.tick() => {
                runtime.emit_timeouts(Instant::now()).await;
            }
        }
    }
}

async fn issue_random_challenges(
    blockchain: &Arc<RwLock<Blockchain>>,
    p2p: &P2pClient,
    runtime: &DaChallengeRuntime,
    peer_count: usize,
    timeout: Duration,
) {
    let mut peers: Vec<String> = runtime.active_peers.read().await.iter().cloned().collect();
    if peers.is_empty() {
        log::debug!("Skipping DA challenge tick: no active peers");
        return;
    }

    let challengeable_blocks = {
        let chain = blockchain.read().await;
        let cache = runtime.fragment_cache.read().await;
        collect_recent_challengeable_blocks(&chain, &cache)
    };

    let mut rng = StdRng::from_entropy();
    let Some(block) = challengeable_blocks.choose(&mut rng) else {
        log::debug!("Skipping DA challenge tick: no recent CID-backed fragments in cache");
        return;
    };

    peers.shuffle(&mut rng);
    peers.truncate(peer_count.min(peers.len()));

    for peer_id in peers {
        let Some((cid, offset, fragment)) = select_challenge_fragment(runtime, &block.cids, &mut rng)
            .await
        else {
            continue;
        };

        let challenge = match generate_challenge(&cid, offset as u64, fragment.len() as u32) {
            Ok(challenge) => challenge,
            Err(err) => {
                log::warn!("Failed to build DA challenge for cid={cid}: {err}");
                continue;
            },
        };

        runtime
            .track_challenge(peer_id.clone(), challenge.clone(), fragment, timeout)
            .await;

        if let Err(err) = p2p
            .command_tx
            .send(P2pCommand::BroadcastDaChallenge {
                challenge: challenge.clone(),
            })
            .await
        {
            runtime.remove_inflight(&challenge.challenge_id).await;
            log::warn!(
                "Failed to broadcast DA challenge {} for target peer {}: {err}",
                challenge.challenge_id,
                peer_id
            );
            continue;
        }

        log::info!(
            "Broadcast DA challenge {} for block {} cid={} target_peer={}",
            challenge.challenge_id,
            block.height,
            cid,
            peer_id
        );
    }
}

async fn select_challenge_fragment<R: Rng>(
    runtime: &DaChallengeRuntime,
    cids: &[String],
    rng: &mut R,
) -> Option<(String, usize, Vec<u8>)> {
    let cid = cids.choose(rng)?.clone();
    let cache = runtime.fragment_cache.read().await;
    let blob = cache.get(&cid)?;
    let (offset, fragment) = random_fragment(blob, rng)?;
    Some((cid, offset, fragment))
}

#[derive(Debug)]
struct RecentChallengeBlock {
    height: u64,
    cids: Vec<String>,
}

fn collect_recent_challengeable_blocks(
    blockchain: &Blockchain,
    cache: &FxHashMap<String, Vec<u8>>,
) -> Vec<RecentChallengeBlock> {
    blockchain
        .blocks()
        .iter()
        .rev()
        .take(RECENT_DA_BLOCK_WINDOW)
        .filter_map(|(height, block)| {
            let mut cids = Vec::new();
            for tx in &block.transactions {
                let Some(payload) = tx.parse_reveal_payload() else {
                    continue;
                };
                let Some(cid) = payload.data_cid else {
                    continue;
                };
                if cache.get(&cid).is_some_and(|bytes| !bytes.is_empty()) {
                    cids.push(cid);
                }
            }

            if cids.is_empty() {
                None
            } else {
                Some(RecentChallengeBlock {
                    height: *height,
                    cids,
                })
            }
        })
        .collect()
}

fn random_fragment<R: Rng>(blob: &[u8], rng: &mut R) -> Option<(usize, Vec<u8>)> {
    let max_len = blob.len().min(CHALLENGE_FRAGMENT_MAX_BYTES as usize);
    if max_len == 0 {
        return None;
    }
    let fragment_len = rng.gen_range(1..=max_len);
    let max_offset = blob.len().saturating_sub(fragment_len);
    let offset = if max_offset == 0 {
        0
    } else {
        rng.gen_range(0..=max_offset)
    };
    Some((offset, blob[offset..offset + fragment_len].to_vec()))
}

fn slice_fragment(data: &[u8], offset: u64, length: u32) -> Option<Vec<u8>> {
    let offset = usize::try_from(offset).ok()?;
    let length = usize::try_from(length).ok()?;
    let end = offset.checked_add(length)?;
    data.get(offset..end).map(|fragment| fragment.to_vec())
}

fn payload_matches_content_ref(content_ref: &ContentRef, payload: &[u8]) -> bool {
    if content_ref.data_size != payload.len() as u64 {
        return false;
    }

    let payload_hash = hex::encode(Sha256::digest(payload));
    payload_hash.eq_ignore_ascii_case(&content_ref.data_hash)
}

fn fragment_id(challenge: &DataAvailabilityChallenge) -> String {
    format!(
        "{}:{}:{}",
        challenge.cid, challenge.offset, challenge.length
    )
}

fn parse_env_or_default<T>(name: &str, default: T) -> T
where
    T: Copy + std::str::FromStr,
{
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<T>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn content_ref_for(cid: &str, payload: &[u8]) -> ContentRef {
        ContentRef {
            data_cid: cid.to_string(),
            data_hash: hex::encode(Sha256::digest(payload)),
            storage_provider: "ipfs".to_string(),
            data_size: payload.len() as u64,
        }
    }

    fn sample_content_ref() -> ContentRef {
        content_ref_for(
            "ipfs:bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi",
            b"cached payload for da challenge",
        )
    }

    #[test]
    fn challenge_fragment_proof_matches_blake3_nonce_and_fragment() {
        let content_ref = sample_content_ref();
        let payload = b"cached payload for da challenge";

        let challenge = DataAvailabilityChallenge {
            challenge_id: "challenge-1".to_string(),
            cid: content_ref.data_cid.clone(),
            nonce: [9u8; 32],
            offset: 7,
            length: 6,
            issued_at: Utc::now().timestamp(),
        };

        let fragment = slice_fragment(payload, challenge.offset, challenge.length)
            .expect("challenge fragment must be sliceable");
        let response = DataAvailabilityChallengeResponse {
            challenge_id: challenge.challenge_id.clone(),
            data_fragment: fragment.clone(),
            proof: compute_proof(&challenge.nonce, &fragment),
            keeper_id: "peer-a".to_string(),
            responded_at: Utc::now().timestamp(),
        };

        assert_eq!(response.proof, compute_proof(&challenge.nonce, &fragment));
    }

    #[tokio::test]
    async fn timeout_detection_emits_da_challenge_failure() {
        let runtime = DaChallengeRuntime::new();
        let mut failure_rx = runtime.subscribe_failures();
        let challenge = DataAvailabilityChallenge {
            challenge_id: "timeout-1".to_string(),
            cid: "ipfs:test".to_string(),
            nonce: [7u8; 32],
            offset: 3,
            length: 5,
            issued_at: Utc::now().timestamp(),
        };

        runtime
            .track_challenge(
                "peer-timeout".to_string(),
                challenge,
                b"12345".to_vec(),
                Duration::from_millis(1),
            )
            .await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        runtime.emit_timeouts(Instant::now()).await;

        let failure = failure_rx.recv().await.expect("timeout failure must be emitted");
        assert_eq!(failure.peer_id, "peer-timeout");
        assert_eq!(failure.fragment_id, "ipfs:test:3:5");
        assert!(failure.reason.contains("timed out"));
    }

    #[tokio::test]
    async fn spoofed_keeper_id_does_not_authorize_response() {
        let runtime = DaChallengeRuntime::new();
        let mut failure_rx = runtime.subscribe_failures();
        let challenge = DataAvailabilityChallenge {
            challenge_id: "spoof-1".to_string(),
            cid: "ipfs:test".to_string(),
            nonce: [5u8; 32],
            offset: 0,
            length: 4,
            issued_at: Utc::now().timestamp(),
        };

        runtime
            .track_challenge(
                "peer-y".to_string(),
                challenge.clone(),
                b"data".to_vec(),
                Duration::from_secs(30),
            )
            .await;

        let spoofed_response = DataAvailabilityChallengeResponse {
            challenge_id: challenge.challenge_id.clone(),
            data_fragment: b"data".to_vec(),
            proof: compute_proof(&challenge.nonce, b"data"),
            keeper_id: "peer-y".to_string(),
            responded_at: Utc::now().timestamp(),
        };

        runtime
            .handle_inbound_response("peer-x", spoofed_response)
            .await;

        assert!(
            runtime
                .inflight
                .read()
                .await
                .contains_key(&challenge.challenge_id),
            "spoofed sender must not clear the in-flight challenge"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(20), failure_rx.recv())
                .await
                .is_err(),
            "spoofed sender should be ignored, not converted into a failure event"
        );
    }

    #[tokio::test]
    async fn conflicting_cid_reinsertion_keeps_original_payload() {
        let runtime = DaChallengeRuntime::new();
        let cid = "ipfs:stable-cid";
        let first_payload = b"first payload";
        let second_payload = b"second payload";

        runtime
            .cache_content_ref_payload(
                &content_ref_for(cid, first_payload),
                first_payload,
                "first-insert",
            )
            .await;
        runtime
            .cache_content_ref_payload(
                &content_ref_for(cid, second_payload),
                second_payload,
                "conflicting-insert",
            )
            .await;

        let cache = runtime.fragment_cache.read().await;
        assert_eq!(
            cache.get(cid).map(Vec::as_slice),
            Some(first_payload.as_slice()),
            "conflicting payload must not overwrite the original CID mapping"
        );
        assert_eq!(cache.len(), 1, "conflicting reinsertion must not create duplicates");
    }
}
