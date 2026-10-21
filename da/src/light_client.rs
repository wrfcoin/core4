//! The light node (#2188): follow the chain by headers, verify producer
//! signatures against a tracked producer set, verify set transitions against
//! the previous set, and establish data availability by sampling — without
//! downloading blocks and without trusting the node that serves them.
//!
//! # Trust model
//!
//! A [`LightClient`] starts from a **trusted checkpoint**: a header height +
//! hash and the producer-set history in force there (from the genesis bundle
//! or an operator-verified checkpoint file; never from the serving node). From
//! there every header is accepted only if:
//!
//! 1. its declared hash is the hash recomputed from its fields ([`LightHeader::verify_declared_hash`]);
//! 2. it is the next height and names the trusted header as its parent;
//! 3. it carries **no** producer-set change. The chain has no on-chain rotation
//!    path yet (the P5 fast-follow), a change is not committed in the signed
//!    header, and a lone producer could otherwise sign one and capture the
//!    client. So any change a source attaches is refused (fail closed), and a
//!    rotation reaches the client only as a new operator-verified checkpoint
//!    ([`LightClient::resume`]). Review of core4#3128;
//! 4. its producer signature verifies (the producer's ed25519 signature over
//!    the header's own hash) **and** the signing key is a member of the set
//!    authorised at that height;
//! 5. when a [`FinalityVerifier`] is configured, the header carries a finality
//!    certificate **and** it verifies (voting-power threshold): a missing
//!    certificate is refused, so a source cannot strip finality by omission,
//!    and a certificate is never accepted because a peer asserted it;
//! 6. its data is available per [`DaPolicy`]: with a commitment in the header
//!    (`da_commitment` metadata, written by #2185 under its feature gate) the
//!    sampler (#2186) must return `Available`. `Unavailable` rejects the
//!    header; `Inconclusive` does not advance either. The **signed** header
//!    commitment always wins: a served commitment that differs rejects the
//!    header, and a served one is used only when the header carries none.
//!
//! WRFCoin's live consensus signal today is the producer signature (#2290);
//! BFT finality certificates exist as a type but no in-tree source emits them
//! (#1388), so the certificate check is pluggable and the producer check is
//! mandatory.
//!
//! # Resource budget (`docs/LIGHT_CLIENT.md`)
//!
//! Per block the client downloads one header (a few hundred bytes plus 32
//! bytes per transaction) and `samples_per_block` chunk proofs (≈1.3 KiB
//! each); steady-state memory is the checkpoint, the producer-set history and
//! the config — nothing per block is retained. [`BANDWIDTH_BUDGET_PER_BLOCK`]
//! and [`MEMORY_BUDGET_BYTES`] state the targets and the tests assert them.

use std::collections::BTreeSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use wrfcoin_shared_protocol::blockchain::light_header::{LightHeader, LightHeaderError};
use wrfcoin_shared_protocol::consensus::block_producer_auth::ProducerAuthError;
use wrfcoin_shared_protocol::consensus::producer_authority::resolve_authorized_producers_at;
use wrfcoin_shared_protocol::consensus::producer_set::{
    ProducerSetChange, ProducerSetChangeEnvelope, validate_producer_set_history,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{ChainIdentity, protocol_version_at};
use wrfcoin_shared_protocol::crypto::hash::Hash;

use crate::commitment::DaCommitment;
use crate::sampling::{
    PeerPenalty, SampleOutcome, SampleSource, SamplingConfig, choose_indices, sample,
};

/// Block metadata key under which #2185 commits the DA root:
/// `<root hex>:<total_chunks>:<payload_len>`.
pub const DA_COMMITMENT_METADATA_KEY: &str = "da_commitment";

/// Target: bytes a light node downloads per followed block (header + samples).
/// 16 samples × (1 KiB chunk + 16 × 32 B siblings + framing) ≈ 24 KiB, plus a
/// header of a few hundred bytes — 64 KiB leaves headroom for larger blocks.
pub const BANDWIDTH_BUDGET_PER_BLOCK: u64 = 64 * 1024;

/// Target: steady-state memory of the client state itself. A Raspberry Pi
/// class device runs this beside a weather station; the client retains no
/// per-block state, so the bound is the checkpoint + producer history + config.
pub const MEMORY_BUDGET_BYTES: usize = 1024 * 1024;

/// Bound on the producer-set history the client keeps: entries below the
/// trusted height minus this many changes are never consulted again.
pub const PRODUCER_HISTORY_RETAINED: usize = 64;

/// Why a header was not accepted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SyncError {
    #[error("producer signature invalid: {0:?}")]
    InvalidSignature(ProducerAuthError),
    #[error("finality certificate below the voting-power threshold at height {height}: {reason}")]
    InsufficientVotingPower { height: u64, reason: String },
    #[error("header chain broken: {0}")]
    HeaderChainBroken(LightHeaderError),
    #[error("data unavailable at height {height}: chunks {missing:?} withheld")]
    DataUnavailable { height: u64, missing: Vec<u16> },
    #[error("data availability inconclusive at height {height}: {reason}")]
    DataInconclusive { height: u64, reason: &'static str },
    #[error("producer-set transition invalid at height {height}: {reason}")]
    ValidatorSetTransitionInvalid { height: u64, reason: String },
    #[error("producer {producer} is not authorised at height {height}")]
    ProducerNotAuthorized { height: u64, producer: String },
    #[error("header rejected: {0}")]
    HeaderInvalid(LightHeaderError),
    #[error("trusted state invalid: {0}")]
    TrustInvalid(String),
    #[error("header source failed: {0}")]
    Source(String),
    #[error("no commitment in header at height {height} and DA policy requires one")]
    CommitmentMissing { height: u64 },
    #[error(
        "a finality verifier is configured but the source supplied no certificate at height {height}"
    )]
    FinalityCertificateMissing { height: u64 },
    #[error("DA commitment conflict at height {height}: {reason}")]
    DaCommitmentConflict { height: u64, reason: &'static str },
}

/// What accompanies a header from the source.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderCommit {
    /// A bincode-serialised `FinalityCertificate`, when the network produced one.
    pub finality_certificate: Option<Vec<u8>>,
    /// A signed producer-set change. **Always refused** by
    /// [`LightClient::verify_header`] until the chain has an on-chain rotation
    /// path that commits the change in the signed header; kept in the type so
    /// a source that sends one is rejected loudly, not silently ignored.
    pub producer_set_change: Option<ProducerSetChangeEnvelope>,
    /// A DA commitment supplied by the source. The header's **signed**
    /// `da_commitment` metadata always wins: a different value here rejects
    /// the header. This is used only when the header carries no commitment,
    /// which is a trust decision the source must make explicit (see the
    /// `light_node` bin's `--trust-served-da-root`).
    pub da_commitment: Option<DaCommitment>,
}

/// How data availability gates acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DaPolicy {
    /// Every header must carry a commitment and sample `Available`.
    Required,
    /// Sample when a commitment is present; count headers accepted without one
    /// (`LightClientStats::headers_without_commitment`) so the gap is visible.
    WhenCommitted,
    /// Never sample (header-only following).
    Off,
}

#[derive(Debug, Clone)]
pub struct LightClientConfig {
    pub sampling: SamplingConfig,
    pub identity: ChainIdentity,
    pub da_policy: DaPolicy,
}

/// Verifies a finality certificate against the tracked committee. Pluggable
/// because the certificate type lives in `consensus-engines`, which a
/// Raspberry Pi light node need not link.
pub trait FinalityVerifier: Send + Sync {
    fn verify(&self, header_hash: &Hash, height: u64, certificate: &[u8]) -> Result<(), SyncError>;
}

/// Where headers come from (HTTP, P2P, a test double).
#[async_trait]
pub trait HeaderSource: Send + Sync {
    /// The header at `height` with its commit, or `None` if the source does
    /// not have it yet.
    async fn header_at(
        &self,
        height: u64,
    ) -> Result<Option<(LightHeader, HeaderCommit)>, SyncError>;
}

/// Serialisable trusted state — what `resume` needs after a disconnect, and
/// nothing more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightCheckpoint {
    pub height: u64,
    pub hash: Hash,
    pub producer_history: Vec<ProducerSetChange>,
}

/// Counters for the resource-budget assertions and operator reporting.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightClientStats {
    pub headers_verified: u64,
    pub headers_rejected: u64,
    pub headers_without_commitment: u64,
    pub headers_sampled: u64,
    pub bytes_headers: u64,
    pub bytes_samples: u64,
    pub finality_certificates_verified: u64,
    pub producer_set_transitions: u64,
    /// Largest per-block download observed.
    pub max_bytes_per_block: u64,
}

pub struct LightClient {
    checkpoint: LightCheckpoint,
    node_secret: Vec<u8>,
    cfg: LightClientConfig,
    finality: Option<Box<dyn FinalityVerifier>>,
    stats: LightClientStats,
}

impl LightClient {
    /// Start from a trusted checkpoint. The producer history must be non-empty
    /// and internally valid: a light client with nothing to verify producers
    /// against would be trusting the server, so that is a refusal.
    pub fn new(
        checkpoint: LightCheckpoint,
        node_secret: Vec<u8>,
        cfg: LightClientConfig,
    ) -> Result<Self, SyncError> {
        if checkpoint.producer_history.is_empty() {
            return Err(SyncError::TrustInvalid(
                "a trusted producer set is required; refusing to follow producers the server names"
                    .into(),
            ));
        }
        validate_producer_set_history(&checkpoint.producer_history)
            .map_err(|e| SyncError::TrustInvalid(format!("{e:?}")))?;
        if node_secret.len() < crate::sampling::NODE_SECRET_MIN_LEN {
            return Err(SyncError::TrustInvalid(format!(
                // secret-leak-ok: interpolates only the NODE_SECRET_MIN_LEN constant, never the secret bytes
                "node secret must be at least {} bytes",
                crate::sampling::NODE_SECRET_MIN_LEN
            )));
        }
        Ok(Self {
            checkpoint,
            node_secret,
            cfg,
            finality: None,
            stats: LightClientStats::default(),
        })
    }

    /// Resume from a checkpoint taken earlier — no re-sync from genesis.
    pub fn resume(
        checkpoint: LightCheckpoint,
        node_secret: Vec<u8>,
        cfg: LightClientConfig,
    ) -> Result<Self, SyncError> {
        Self::new(checkpoint, node_secret, cfg)
    }

    pub fn with_finality_verifier(mut self, verifier: Box<dyn FinalityVerifier>) -> Self {
        self.finality = Some(verifier);
        self
    }

    pub fn checkpoint(&self) -> &LightCheckpoint {
        &self.checkpoint
    }

    pub fn trusted_height(&self) -> u64 {
        self.checkpoint.height
    }

    pub fn trusted_hash(&self) -> Hash {
        self.checkpoint.hash
    }

    pub fn stats(&self) -> &LightClientStats {
        &self.stats
    }

    /// Approximate resident size of the client state (for the memory budget).
    pub fn memory_estimate(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.node_secret.len()
            + self
                .checkpoint
                .producer_history
                .iter()
                .map(|c| {
                    std::mem::size_of::<ProducerSetChange>()
                        + c.members
                            .iter()
                            .map(|m| m.producer_id.len() + m.payout_address.len() + 32)
                            .sum::<usize>()
                })
                .sum::<usize>()
    }

    /// Steps 1–5: verify `header` against the trusted state and, on success,
    /// advance the checkpoint. Data availability is [`Self::verify_data_availability`];
    /// [`Self::follow`] runs both in order.
    pub fn verify_header(
        &mut self,
        header: &LightHeader,
        commit: &HeaderCommit,
    ) -> Result<(), SyncError> {
        let height = header.height;
        let hash = header
            .verify_declared_hash()
            .map_err(SyncError::HeaderInvalid)?;
        header
            .verify_chains_onto(self.checkpoint.height, self.checkpoint.hash)
            .map_err(SyncError::HeaderChainBroken)?;

        // Producer-set changes are refused outright (fail closed). The real
        // chain cannot rotate producers yet (the P5 fast-follow), the change
        // is not committed in the signed header, and accepting one signed by
        // any single member let one colluding producer capture every client
        // (review of core4#3128). Rotation is an operator-verified checkpoint.
        if commit.producer_set_change.is_some() {
            return Err(SyncError::ValidatorSetTransitionInvalid {
                height,
                reason: "producer-set changes are not accepted from a header source; rotate \
                         by resuming from an operator-verified checkpoint"
                    .into(),
            });
        }
        let mut history = self.checkpoint.producer_history.clone();

        // Producer signature, then authority at this height.
        let version = protocol_version_at(height);
        let producer = header
            .verify_producer(&self.cfg.identity, version)
            .map_err(SyncError::InvalidSignature)?;
        let authorised = resolve_authorized_producers_at(&history, height)
            .map_err(|e| SyncError::ValidatorSetTransitionInvalid { height, reason: e })?;
        let producer_bytes: &[u8] = producer.as_bytes();
        if !authorised
            .iter()
            .any(|m| m.public_key.as_slice() == producer_bytes)
        {
            return Err(SyncError::ProducerNotAuthorized {
                height,
                producer: producer.to_hex(),
            });
        }

        // Finality: with a verifier configured a certificate is REQUIRED, so
        // a source cannot strip the finality layer by omitting it.
        if let Some(verifier) = &self.finality {
            let cert = commit
                .finality_certificate
                .as_ref()
                .ok_or(SyncError::FinalityCertificateMissing { height })?;
            verifier.verify(&hash, height, cert)?;
            self.stats.finality_certificates_verified += 1;
        }

        // Accept: advance the checkpoint; retain a bounded history.
        if history.len() > PRODUCER_HISTORY_RETAINED {
            let drop = history.len() - PRODUCER_HISTORY_RETAINED;
            // Only drop entries whose successor is already effective at this
            // height; the last entry is always kept.
            let keep_from = history
                .iter()
                .enumerate()
                .take(drop)
                .take_while(|(i, _)| {
                    history
                        .get(i + 1)
                        .map(|n| n.effective_height <= height)
                        .unwrap_or(false)
                })
                .count();
            history.drain(..keep_from);
        }
        self.checkpoint = LightCheckpoint {
            height,
            hash,
            producer_history: history,
        };
        self.stats.headers_verified += 1;
        self.stats.bytes_headers += header.wire_len_estimate() as u64;
        Ok(())
    }

    /// Step 6: sample `commitment` for `header` and return `Ok` only on
    /// `Available`. Withholding is an error; an inconclusive round is an error
    /// too (the caller retries), never an acceptance.
    pub async fn verify_data_availability<S: SampleSource + ?Sized, P: PeerPenalty + ?Sized>(
        &mut self,
        header: &LightHeader,
        commitment: &DaCommitment,
        peers: &[String],
        source: &S,
        penalty: &P,
    ) -> Result<u64, SyncError> {
        let height = header.height;
        let indices = choose_indices(height, &self.node_secret, commitment, &self.cfg.sampling)
            .map_err(|e| SyncError::DataInconclusive {
                height,
                reason: match e {
                    crate::sampling::SamplingError::NoChunks => "commitment has no chunks",
                    crate::sampling::SamplingError::NoSamples => "samples_per_block is zero",
                    crate::sampling::SamplingError::SecretTooShort(_) => "node secret too short",
                },
            })?;
        let outcome = sample(
            height,
            commitment,
            &indices,
            &self.cfg.sampling,
            peers,
            source,
            penalty,
        )
        .await;
        match outcome {
            SampleOutcome::Available { sampled } => {
                let bytes = u64::from(sampled) * sample_wire_bytes(commitment.total_chunks);
                self.stats.headers_sampled += 1;
                self.stats.bytes_samples += bytes;
                Ok(bytes)
            },
            SampleOutcome::Unavailable { missing } => {
                Err(SyncError::DataUnavailable { height, missing })
            },
            SampleOutcome::Inconclusive { reason } => {
                Err(SyncError::DataInconclusive { height, reason })
            },
        }
    }

    /// Follow `headers` from the trusted height for up to `max_headers`
    /// headers, applying every check. Returns the number accepted. Stops
    /// cleanly when the source has no next header; any verification failure
    /// is returned and the checkpoint stays at the last accepted header.
    pub async fn follow<H, S, P>(
        &mut self,
        headers: &H,
        da_source: &S,
        penalty: &P,
        peers: &[String],
        max_headers: u64,
    ) -> Result<u64, SyncError>
    where
        H: HeaderSource + ?Sized,
        S: SampleSource + ?Sized,
        P: PeerPenalty + ?Sized,
    {
        let mut accepted = 0u64;
        while accepted < max_headers {
            let next = self.checkpoint.height.saturating_add(1);
            let Some((header, commit)) = headers.header_at(next).await? else {
                break;
            };
            let before = self.checkpoint.clone();
            let mut block_bytes = header.wire_len_estimate() as u64;
            if let Err(e) = self.verify_header(&header, &commit) {
                self.stats.headers_rejected += 1;
                return Err(e);
            }
            // The SIGNED header commitment wins (review of core4#3128); a
            // served value is used only when the header carries none.
            let commitment = match resolve_da_commitment(&header, commit.da_commitment.as_ref()) {
                Ok(c) => c,
                Err(reason) => {
                    self.checkpoint = before;
                    self.stats.headers_verified -= 1;
                    self.stats.headers_rejected += 1;
                    return Err(SyncError::DaCommitmentConflict {
                        height: header.height,
                        reason,
                    });
                },
            };
            match (self.cfg.da_policy, commitment) {
                (DaPolicy::Off, _) => {},
                (DaPolicy::WhenCommitted, None) => {
                    self.stats.headers_without_commitment += 1;
                },
                (DaPolicy::Required, None) => {
                    self.checkpoint = before;
                    self.stats.headers_verified -= 1;
                    self.stats.headers_rejected += 1;
                    return Err(SyncError::CommitmentMissing {
                        height: header.height,
                    });
                },
                (_, Some(c)) => match self
                    .verify_data_availability(&header, &c, peers, da_source, penalty)
                    .await
                {
                    Ok(bytes) => block_bytes += bytes,
                    Err(e) => {
                        // A header whose data is unavailable (or unproven) is
                        // NOT accepted: roll the checkpoint back.
                        self.checkpoint = before;
                        self.stats.headers_verified -= 1;
                        self.stats.headers_rejected += 1;
                        return Err(e);
                    },
                },
            }
            self.stats.max_bytes_per_block = self.stats.max_bytes_per_block.max(block_bytes);
            accepted += 1;
        }
        Ok(accepted)
    }
}

/// Bytes one sample costs on the wire: the chunk plus its sibling path.
pub fn sample_wire_bytes(total_chunks: u16) -> u64 {
    (crate::layout::CHUNK_SIZE + 32 * crate::commitment::tree_depth(total_chunks) + 16) as u64
}

/// The commitment to sample against: the header's signed `da_commitment`
/// metadata when present (a served value that differs is an error, as is
/// metadata that does not decode), else the served value, else none.
pub fn resolve_da_commitment(
    header: &LightHeader,
    served: Option<&DaCommitment>,
) -> Result<Option<DaCommitment>, &'static str> {
    match header.metadata.get(DA_COMMITMENT_METADATA_KEY) {
        Some(raw) => {
            let signed = decode_da_commitment(raw)
                .ok_or("the header's signed da_commitment metadata does not decode")?;
            match served {
                Some(s) if *s != signed => {
                    Err("the source's commitment differs from the header's signed commitment")
                },
                _ => Ok(Some(signed)),
            }
        },
        None => Ok(served.cloned()),
    }
}

/// Read the #2185 commitment out of header metadata, if present.
pub fn parse_da_commitment(header: &LightHeader) -> Option<DaCommitment> {
    let raw = header.metadata.get(DA_COMMITMENT_METADATA_KEY)?;
    decode_da_commitment(raw)
}

/// `<root hex>:<total_chunks>:<payload_len>`
pub fn encode_da_commitment(c: &DaCommitment) -> String {
    format!(
        "{}:{}:{}",
        hex::encode(c.root),
        c.total_chunks,
        c.payload_len
    )
}

pub fn decode_da_commitment(raw: &str) -> Option<DaCommitment> {
    let mut parts = raw.split(':');
    let root_hex = parts.next()?;
    let total: u16 = parts.next()?.parse().ok()?;
    let len: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let bytes = hex::decode(root_hex).ok()?;
    let root: [u8; 32] = bytes.try_into().ok()?;
    Some(DaCommitment {
        root,
        total_chunks: total,
        payload_len: len,
    })
}

/// Distinct peers seen by a source — a helper for callers that pass the
/// same list into `follow`.
pub fn distinct_peers<'a, I: IntoIterator<Item = &'a str>>(peers: I) -> Vec<String> {
    let set: BTreeSet<&str> = peers.into_iter().collect();
    set.into_iter().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn da_commitment_metadata_round_trips_and_rejects_garbage() {
        let c = DaCommitment {
            root: [7u8; 32],
            total_chunks: 300,
            payload_len: 300 * 1024 - 9,
        };
        let raw = encode_da_commitment(&c);
        assert_eq!(decode_da_commitment(&raw), Some(c));
        assert_eq!(decode_da_commitment("nope"), None);
        assert_eq!(
            decode_da_commitment(&format!("{}:x:1", hex::encode([1u8; 32]))),
            None
        );
        assert_eq!(
            decode_da_commitment(&format!("{}:1:1:extra", hex::encode([1u8; 32]))),
            None
        );
        assert_eq!(decode_da_commitment("abcd:1:1"), None);
    }

    #[test]
    fn sample_wire_bytes_is_chunk_plus_path() {
        assert_eq!(sample_wire_bytes(1), 1024 + 16);
        assert_eq!(sample_wire_bytes(1024), 1024 + 32 * 10 + 16);
    }
}
