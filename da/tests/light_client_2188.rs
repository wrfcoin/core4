//! #2188 acceptance over an in-memory header + chunk source.
//!
//! * a valid header chain is followed;
//! * a header with insufficient voting power (finality certificate) is rejected;
//! * a forged producer signature is rejected;
//! * unavailable data prevents acceptance (the checkpoint does not advance);
//! * a producer-set change from the source is refused (fail closed; review of
//!   core4#3128), and a rotation delivered as an operator-verified checkpoint
//!   is enforced from its effective height;
//! * with a finality verifier configured, a missing certificate is refused;
//! * the header's signed DA commitment wins over a served one;
//! * the bandwidth and memory budgets hold over a long follow;
//! * the client resumes after a disconnect without re-syncing from genesis.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use rand::SeedableRng;
use rand::rngs::StdRng;
use wrfcoin_da::{
    BANDWIDTH_BUDGET_PER_BLOCK, ChunkLayout, ChunkResponse, DA_COMMITMENT_METADATA_KEY,
    DaCommitment, DaPolicy, FinalityVerifier, HeaderCommit, HeaderSource, LightCheckpoint,
    LightClient, LightClientConfig, MEMORY_BUDGET_BYTES, PeerPenalty, SampleFetchError,
    SamplePenalty, SampleSource, SamplingConfig, SyncError, encode_da_commitment,
};
use wrfcoin_shared_protocol::blockchain::block::Block;
use wrfcoin_shared_protocol::blockchain::light_header::LightHeader;
use wrfcoin_shared_protocol::consensus::block_producer_auth::sign_block_producer;
use wrfcoin_shared_protocol::consensus::producer_set::{
    ProducerMember, ProducerSetChange, ProducerSetChangeEnvelope,
};
use wrfcoin_shared_protocol::crypto::chain_binding::{ChainIdentity, protocol_version_at};
use wrfcoin_shared_protocol::crypto::hash::{Hash, hash_string};
use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};

const SECRET: &[u8] = b"light-node-secret-2188-xxxxxxxxx";

fn identity() -> ChainIdentity {
    ChainIdentity::new("wrfcoin-test", "testnet")
}

struct Producer {
    id: String,
    sk: PrivateKey,
}

impl Producer {
    fn new(seed: u64, id: &str) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        Self {
            id: id.to_string(),
            sk: PrivateKey::generate(&mut rng),
        }
    }
    fn key(&self) -> [u8; 32] {
        PublicKey::from_private_key(&self.sk)
            .as_bytes()
            .try_into()
            .unwrap()
    }
    fn member(&self) -> ProducerMember {
        ProducerMember {
            producer_id: self.id.clone(),
            payout_address: format!("0x{}", hex::encode(&self.key()[..20])),
            public_key: self.key(),
        }
    }
}

/// An in-memory chain: headers, commits, and one shared chunk layout every
/// height's commitment points at (indices still differ per height).
struct FakeChain {
    headers: BTreeMap<u64, (LightHeader, HeaderCommit)>,
    layout: ChunkLayout,
    commitment: DaCommitment,
    withheld: BTreeSet<u16>,
    fetches: Mutex<u64>,
}

impl FakeChain {
    fn new() -> Self {
        let payload: Vec<u8> = (0..(1024 * 64 - 3)).map(|i| (i % 233) as u8).collect();
        let layout = ChunkLayout::for_payload(&payload).unwrap();
        let commitment = DaCommitment::commit(&layout);
        Self {
            headers: BTreeMap::new(),
            layout,
            commitment,
            withheld: BTreeSet::new(),
            fetches: Mutex::new(0),
        }
    }

    /// Append a header at `height` signed by `producer`, chained onto `prev`.
    fn push(
        &mut self,
        height: u64,
        prev: Hash,
        producer: &Producer,
        with_commitment: bool,
        commit: HeaderCommit,
    ) -> Hash {
        let signed = with_commitment.then(|| self.commitment.clone());
        self.push_committing(height, prev, producer, signed.as_ref(), commit)
    }

    /// Like `push`, but the header's SIGNED metadata commits `signed` (any
    /// commitment, not necessarily the one this chain serves chunks for).
    fn push_committing(
        &mut self,
        height: u64,
        prev: Hash,
        producer: &Producer,
        signed: Option<&DaCommitment>,
        commit: HeaderCommit,
    ) -> Hash {
        let mut block = Block::new(3, prev, Vec::new(), 1, height);
        block.nonce = height;
        if let Some(c) = signed {
            block.metadata.insert(
                DA_COMMITMENT_METADATA_KEY.to_string(),
                encode_da_commitment(c),
            );
        }
        block.producer_pubkey = producer.key();
        let version = protocol_version_at(height);
        let att = sign_block_producer(&block, &producer.sk, &identity(), version);
        block.producer_sig = att.producer_sig.to_vec();
        let hash = block.calculate_hash_for(version, Some(&identity()));
        block.set_hash(hash);
        let header = LightHeader::from_block(&block);
        self.headers.insert(height, (header, commit));
        hash
    }

    fn peers() -> Vec<String> {
        vec!["peer-a".into(), "peer-b".into(), "peer-c".into()]
    }
}

#[async_trait]
impl HeaderSource for FakeChain {
    async fn header_at(
        &self,
        height: u64,
    ) -> Result<Option<(LightHeader, HeaderCommit)>, SyncError> {
        Ok(self.headers.get(&height).cloned())
    }
}

#[async_trait]
impl SampleSource for FakeChain {
    async fn fetch(
        &self,
        _peer: &str,
        _height: u64,
        index: u16,
    ) -> Result<ChunkResponse, SampleFetchError> {
        *self.fetches.lock().unwrap() += 1;
        if self.withheld.contains(&index) {
            return Err(SampleFetchError::Missing);
        }
        Ok(ChunkResponse {
            proof: DaCommitment::prove(&self.layout, index).unwrap(),
        })
    }
}

struct NoPenalty;
impl PeerPenalty for NoPenalty {
    fn penalize(&self, _peer: &str, _reason: SamplePenalty) {}
}

fn cfg(policy: DaPolicy) -> LightClientConfig {
    LightClientConfig {
        sampling: SamplingConfig {
            samples_per_block: 8,
            confidence_target: 0.9,
            request_timeout: Duration::from_millis(100),
            distinct_peers_required: 2,
        },
        identity: identity(),
        da_policy: policy,
    }
}

fn genesis_checkpoint(producers: &[&Producer]) -> (LightCheckpoint, Hash) {
    let genesis_hash = hash_string("genesis-2188");
    let members = producers.iter().map(|p| p.member()).collect();
    (
        LightCheckpoint {
            height: 0,
            hash: genesis_hash,
            producer_history: vec![ProducerSetChange::new(0, 0, members)],
        },
        genesis_hash,
    )
}

/// Build `n` headers from the checkpoint, alternating producers.
fn build_chain(
    chain: &mut FakeChain,
    from_hash: Hash,
    from_height: u64,
    n: u64,
    producers: &[&Producer],
    with_commitment: bool,
) -> Hash {
    let mut prev = from_hash;
    for i in 0..n {
        let height = from_height + 1 + i;
        let p = producers[(height as usize) % producers.len()];
        prev = chain.push(height, prev, p, with_commitment, HeaderCommit::default());
    }
    prev
}

#[tokio::test]
async fn a_valid_header_chain_is_followed_with_da_sampling() {
    let a = Producer::new(1, "a");
    let b = Producer::new(2, "b");
    let (checkpoint, genesis) = genesis_checkpoint(&[&a, &b]);
    let mut chain = FakeChain::new();
    let tip = build_chain(&mut chain, genesis, 0, 20, &[&a, &b], true);

    let mut client =
        LightClient::new(checkpoint, SECRET.to_vec(), cfg(DaPolicy::Required)).unwrap();
    let accepted = client
        .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 100)
        .await
        .expect("a valid chain follows");
    assert_eq!(accepted, 20);
    assert_eq!(client.trusted_height(), 20);
    assert_eq!(client.trusted_hash(), tip);
    assert_eq!(client.stats().headers_verified, 20);
    assert_eq!(client.stats().headers_sampled, 20);
    assert_eq!(client.stats().headers_without_commitment, 0);
    assert!(
        *chain.fetches.lock().unwrap() >= 20 * 8,
        "every header was sampled"
    );
}

#[tokio::test]
async fn a_forged_signature_and_an_unauthorised_producer_are_rejected() {
    let a = Producer::new(1, "a");
    let b = Producer::new(2, "b");
    let stranger = Producer::new(99, "stranger");
    let (checkpoint, genesis) = genesis_checkpoint(&[&a, &b]);

    // Forged: valid shape, signature bytes flipped.
    let mut chain = FakeChain::new();
    build_chain(&mut chain, genesis, 0, 3, &[&a, &b], false);
    let (h2, _) = chain.headers.get_mut(&2).unwrap();
    h2.producer_sig[7] ^= 0x10;
    let mut client =
        LightClient::new(checkpoint.clone(), SECRET.to_vec(), cfg(DaPolicy::Off)).unwrap();
    let err = client
        .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 10)
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::InvalidSignature(_)), "{err:?}");
    assert_eq!(
        client.trusted_height(),
        1,
        "the checkpoint stops before the forged header"
    );

    // Unauthorised: correctly signed by a key outside the tracked set.
    let mut chain = FakeChain::new();
    let h1 = build_chain(&mut chain, genesis, 0, 1, &[&a, &b], false);
    chain.push(2, h1, &stranger, false, HeaderCommit::default());
    let mut client = LightClient::new(checkpoint, SECRET.to_vec(), cfg(DaPolicy::Off)).unwrap();
    let err = client
        .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 10)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SyncError::ProducerNotAuthorized { height: 2, .. }),
        "{err:?}"
    );
    assert_eq!(client.trusted_height(), 1);
}

struct ThresholdVerifier;
impl FinalityVerifier for ThresholdVerifier {
    fn verify(&self, _hash: &Hash, height: u64, certificate: &[u8]) -> Result<(), SyncError> {
        if certificate == b"supermajority" {
            Ok(())
        } else {
            Err(SyncError::InsufficientVotingPower {
                height,
                reason: "below 2/3 of stake".into(),
            })
        }
    }
}

#[tokio::test]
async fn a_header_with_insufficient_voting_power_is_rejected() {
    let a = Producer::new(1, "a");
    let (checkpoint, genesis) = genesis_checkpoint(&[&a]);
    let mut chain = FakeChain::new();
    let h1 = chain.push(
        1,
        genesis,
        &a,
        false,
        HeaderCommit {
            finality_certificate: Some(b"supermajority".to_vec()),
            ..Default::default()
        },
    );
    chain.push(
        2,
        h1,
        &a,
        false,
        HeaderCommit {
            finality_certificate: Some(b"one-vote".to_vec()),
            ..Default::default()
        },
    );
    let mut client = LightClient::new(checkpoint, SECRET.to_vec(), cfg(DaPolicy::Off))
        .unwrap()
        .with_finality_verifier(Box::new(ThresholdVerifier));
    let err = client
        .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 10)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SyncError::InsufficientVotingPower { height: 2, .. }),
        "{err:?}"
    );
    assert_eq!(client.trusted_height(), 1);
    assert_eq!(client.stats().finality_certificates_verified, 1);
}

#[tokio::test]
async fn unavailable_data_prevents_acceptance() {
    let a = Producer::new(1, "a");
    let (checkpoint, genesis) = genesis_checkpoint(&[&a]);
    let mut chain = FakeChain::new();
    build_chain(&mut chain, genesis, 0, 5, &[&a], true);
    // Withhold a quarter of the chunks on every peer: with 8 samples the
    // client hits one with probability 1 - 0.75^8 ≈ 0.9; withhold enough that
    // the first header is caught deterministically for this secret by
    // withholding everything the draw asks for.
    let mut client =
        LightClient::new(checkpoint.clone(), SECRET.to_vec(), cfg(DaPolicy::Required)).unwrap();
    let indices = wrfcoin_da::choose_indices(
        1,
        SECRET,
        &chain.commitment,
        &cfg(DaPolicy::Required).sampling,
    )
    .unwrap();
    chain.withheld = indices.iter().copied().collect();
    let err = client
        .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 10)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SyncError::DataUnavailable { height: 1, .. }),
        "{err:?}"
    );
    assert_eq!(
        client.trusted_height(),
        0,
        "a header whose data is unavailable is not accepted"
    );
    assert_eq!(client.stats().headers_verified, 0);
    assert_eq!(client.stats().headers_rejected, 1);

    // Required policy with no commitment in the header: also not accepted.
    let mut bare = FakeChain::new();
    build_chain(&mut bare, genesis, 0, 2, &[&a], false);
    let mut client =
        LightClient::new(checkpoint.clone(), SECRET.to_vec(), cfg(DaPolicy::Required)).unwrap();
    let err = client
        .follow(&bare, &bare, &NoPenalty, &FakeChain::peers(), 10)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SyncError::CommitmentMissing { height: 1 }),
        "{err:?}"
    );
    assert_eq!(client.trusted_height(), 0);

    // WhenCommitted: accepted, but counted — the pre-#2185 gap is visible.
    let mut client =
        LightClient::new(checkpoint, SECRET.to_vec(), cfg(DaPolicy::WhenCommitted)).unwrap();
    assert_eq!(
        client
            .follow(&bare, &bare, &NoPenalty, &FakeChain::peers(), 10)
            .await
            .unwrap(),
        2
    );
    assert_eq!(client.stats().headers_without_commitment, 2);
}

/// Review of core4#3128, finding 1: set {a, b, c}; `a` ALONE signs a change
/// to {evil} and the server attaches it to b's honest header 2. Before the fix
/// the client accepted evil's headers 3 and 4 and then refused honest `c` at 5.
/// Now any change from a source is refused, and following the honest chain
/// still accepts `c` at 5.
#[tokio::test]
async fn a_producer_set_change_from_the_source_is_refused() {
    let a = Producer::new(1, "a");
    let b = Producer::new(2, "b");
    let c = Producer::new(3, "c");
    let evil = Producer::new(66, "evil");
    let (checkpoint, genesis) = genesis_checkpoint(&[&a, &b, &c]);
    let capture = ProducerSetChange::new(1, 3, vec![evil.member()]);
    let envelope =
        ProducerSetChangeEnvelope::sign(capture, &a.sk, &identity(), protocol_version_at(2));

    let mut forked = FakeChain::new();
    let h1 = forked.push(1, genesis, &a, false, HeaderCommit::default());
    let h2 = forked.push(
        2,
        h1,
        &b,
        false,
        HeaderCommit {
            producer_set_change: Some(envelope),
            ..Default::default()
        },
    );
    let h3 = forked.push(3, h2, &evil, false, HeaderCommit::default());
    forked.push(4, h3, &evil, false, HeaderCommit::default());
    let mut client =
        LightClient::new(checkpoint.clone(), SECRET.to_vec(), cfg(DaPolicy::Off)).unwrap();
    let err = client
        .follow(&forked, &forked, &NoPenalty, &FakeChain::peers(), 20)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            SyncError::ValidatorSetTransitionInvalid { height: 2, .. }
        ),
        "{err:?}"
    );
    assert_eq!(
        client.trusted_height(),
        1,
        "the capture is refused at the header that carries it"
    );
    assert_eq!(
        client.checkpoint().producer_history.len(),
        1,
        "the tracked set is untouched"
    );

    // The honest chain (no change attached) still follows, including c at 5.
    let mut honest = FakeChain::new();
    let mut prev = genesis;
    for (height, p) in [(1, &a), (2, &b), (3, &a), (4, &b), (5, &c)] {
        prev = honest.push(height, prev, p, false, HeaderCommit::default());
    }
    let mut client =
        LightClient::new(checkpoint.clone(), SECRET.to_vec(), cfg(DaPolicy::Off)).unwrap();
    assert_eq!(
        client
            .follow(&honest, &honest, &NoPenalty, &FakeChain::peers(), 20)
            .await
            .unwrap(),
        5
    );
    assert_eq!(client.trusted_height(), 5);
    assert_eq!(client.stats().producer_set_transitions, 0);

    // Even a change signed by a current member and validly future-dated is
    // refused: there is no on-chain rotation path for it to mirror.
    let mut chain = FakeChain::new();
    let h1 = chain.push(1, genesis, &a, false, HeaderCommit::default());
    let to_c = ProducerSetChange::new(1, 8, vec![c.member()]);
    let envelope =
        ProducerSetChangeEnvelope::sign(to_c, &a.sk, &identity(), protocol_version_at(2));
    chain.push(
        2,
        h1,
        &b,
        false,
        HeaderCommit {
            producer_set_change: Some(envelope),
            ..Default::default()
        },
    );
    let mut client = LightClient::new(checkpoint, SECRET.to_vec(), cfg(DaPolicy::Off)).unwrap();
    let err = client
        .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 20)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            SyncError::ValidatorSetTransitionInvalid { height: 2, .. }
        ),
        "{err:?}"
    );
}

/// Rotation is an operator-verified checkpoint: resuming with a history that
/// includes the change enforces the new set from its effective height.
#[tokio::test]
async fn a_rotation_arrives_as_an_operator_verified_checkpoint() {
    let a = Producer::new(1, "a");
    let b = Producer::new(2, "b");
    let c = Producer::new(3, "c");
    let (checkpoint, genesis) = genesis_checkpoint(&[&a, &b]);
    let mut chain = FakeChain::new();
    let h4 = build_chain(&mut chain, genesis, 0, 4, &[&a, &b], false);
    let h5 = chain.push(5, h4, &b, false, HeaderCommit::default());
    let h6 = chain.push(6, h5, &a, false, HeaderCommit::default());
    let h7 = chain.push(7, h6, &b, false, HeaderCommit::default());
    let h8 = chain.push(8, h7, &c, false, HeaderCommit::default());
    chain.push(9, h8, &a, false, HeaderCommit::default()); // `a` is no longer authorised

    let mut client =
        LightClient::new(checkpoint.clone(), SECRET.to_vec(), cfg(DaPolicy::Off)).unwrap();
    assert_eq!(
        client
            .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 4)
            .await
            .unwrap(),
        4
    );
    // The operator verifies the rotation out of band and hands the client a
    // checkpoint whose history includes it.
    let mut rotated = client.checkpoint().clone();
    rotated
        .producer_history
        .push(ProducerSetChange::new(1, 8, vec![c.member()]));
    let mut client = LightClient::resume(rotated, SECRET.to_vec(), cfg(DaPolicy::Off)).unwrap();
    let err = client
        .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 20)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SyncError::ProducerNotAuthorized { height: 9, .. }),
        "{err:?}"
    );
    assert_eq!(
        client.trusted_height(),
        8,
        "the new set is enforced from its effective height"
    );
}

struct RejectAll;
impl FinalityVerifier for RejectAll {
    fn verify(&self, _hash: &Hash, height: u64, _certificate: &[u8]) -> Result<(), SyncError> {
        Err(SyncError::InsufficientVotingPower {
            height,
            reason: "rejects every certificate".into(),
        })
    }
}

/// Review of core4#3128, finding 3: with a reject-all verifier configured and
/// NO certificates served, all headers used to be accepted with
/// finality_certificates_verified = 0. A missing certificate is now refused.
#[tokio::test]
async fn a_configured_finality_verifier_refuses_a_missing_certificate() {
    let a = Producer::new(1, "a");
    let (checkpoint, genesis) = genesis_checkpoint(&[&a]);
    let mut chain = FakeChain::new();
    build_chain(&mut chain, genesis, 0, 3, &[&a], false);
    let mut client = LightClient::new(checkpoint.clone(), SECRET.to_vec(), cfg(DaPolicy::Off))
        .unwrap()
        .with_finality_verifier(Box::new(RejectAll));
    let err = client
        .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 10)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SyncError::FinalityCertificateMissing { height: 1 }),
        "{err:?}"
    );
    assert_eq!(client.trusted_height(), 0);
    assert_eq!(client.stats().finality_certificates_verified, 0);

    // Without a verifier configured, the same chain follows (header-only).
    let mut client = LightClient::new(checkpoint, SECRET.to_vec(), cfg(DaPolicy::Off)).unwrap();
    assert_eq!(
        client
            .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 10)
            .await
            .unwrap(),
        3
    );
}

/// Review of core4#3128, finding 2: the header's SIGNED metadata commits
/// payload X, of which no chunk is served; the source names the commitment of
/// payload Y, which is served. The served value used to win and the block was
/// judged Available against data it does not commit to.
#[tokio::test]
async fn the_signed_header_commitment_wins_over_a_served_one() {
    let a = Producer::new(1, "a");
    let (checkpoint, genesis) = genesis_checkpoint(&[&a]);
    let mut chain = FakeChain::new(); // serves chunks of payload Y only
    let served_y = chain.commitment.clone();
    let payload_x: Vec<u8> = (0..(1024 * 48)).map(|i| (i % 251) as u8 ^ 0x5a).collect();
    let signed_x = DaCommitment::commit(&ChunkLayout::for_payload(&payload_x).unwrap());
    assert_ne!(signed_x, served_y);
    chain.push_committing(
        1,
        genesis,
        &a,
        Some(&signed_x),
        HeaderCommit {
            da_commitment: Some(served_y.clone()),
            ..Default::default()
        },
    );
    let mut client =
        LightClient::new(checkpoint.clone(), SECRET.to_vec(), cfg(DaPolicy::Required)).unwrap();
    let err = client
        .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 10)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SyncError::DaCommitmentConflict { height: 1, .. }),
        "{err:?}"
    );
    assert_eq!(client.trusted_height(), 0);
    assert_eq!(client.stats().headers_verified, 0);
    assert_eq!(client.stats().headers_sampled, 0);

    // The served commitment is used only when the header carries none.
    let mut opt_in = FakeChain::new();
    opt_in.push(
        1,
        genesis,
        &a,
        false,
        HeaderCommit {
            da_commitment: Some(served_y.clone()),
            ..Default::default()
        },
    );
    let mut client =
        LightClient::new(checkpoint.clone(), SECRET.to_vec(), cfg(DaPolicy::Required)).unwrap();
    assert_eq!(
        client
            .follow(&opt_in, &opt_in, &NoPenalty, &FakeChain::peers(), 10)
            .await
            .unwrap(),
        1
    );
    assert_eq!(client.stats().headers_sampled, 1);

    // A served value equal to the signed one is fine.
    let mut same = FakeChain::new();
    same.push(
        1,
        genesis,
        &a,
        true,
        HeaderCommit {
            da_commitment: Some(served_y),
            ..Default::default()
        },
    );
    let mut client =
        LightClient::new(checkpoint, SECRET.to_vec(), cfg(DaPolicy::Required)).unwrap();
    assert_eq!(
        client
            .follow(&same, &same, &NoPenalty, &FakeChain::peers(), 10)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn resource_budget_holds_over_a_long_follow() {
    let a = Producer::new(1, "a");
    let b = Producer::new(2, "b");
    let (checkpoint, genesis) = genesis_checkpoint(&[&a, &b]);
    let mut chain = FakeChain::new();
    build_chain(&mut chain, genesis, 0, 2_000, &[&a, &b], true);
    let mut client =
        LightClient::new(checkpoint, SECRET.to_vec(), cfg(DaPolicy::Required)).unwrap();
    let memory_at_start = client.memory_estimate();
    let accepted = client
        .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 5_000)
        .await
        .unwrap();
    assert_eq!(accepted, 2_000);
    let stats = client.stats();
    assert!(
        stats.max_bytes_per_block <= BANDWIDTH_BUDGET_PER_BLOCK,
        "per-block download {} exceeds the {} byte budget",
        stats.max_bytes_per_block,
        BANDWIDTH_BUDGET_PER_BLOCK
    );
    let total = stats.bytes_headers + stats.bytes_samples;
    assert!(total / 2_000 <= BANDWIDTH_BUDGET_PER_BLOCK);
    let memory_at_end = client.memory_estimate();
    assert!(
        memory_at_end <= MEMORY_BUDGET_BYTES,
        "{memory_at_end} bytes"
    );
    assert_eq!(
        memory_at_end, memory_at_start,
        "nothing per block is retained"
    );
}

#[tokio::test]
async fn client_resumes_after_disconnect_without_resyncing_from_genesis() {
    let a = Producer::new(1, "a");
    let b = Producer::new(2, "b");
    let (checkpoint, genesis) = genesis_checkpoint(&[&a, &b]);
    let mut chain = FakeChain::new();
    build_chain(&mut chain, genesis, 0, 30, &[&a, &b], true);

    let mut first = LightClient::new(checkpoint, SECRET.to_vec(), cfg(DaPolicy::Required)).unwrap();
    assert_eq!(
        first
            .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 12)
            .await
            .unwrap(),
        12
    );
    let saved = serde_json::to_string(first.checkpoint()).unwrap();
    drop(first); // disconnect / process restart

    let restored: LightCheckpoint = serde_json::from_str(&saved).unwrap();
    assert_eq!(restored.height, 12);
    let fetches_before = *chain.fetches.lock().unwrap();
    let mut second =
        LightClient::resume(restored, SECRET.to_vec(), cfg(DaPolicy::Required)).unwrap();
    assert_eq!(
        second
            .follow(&chain, &chain, &NoPenalty, &FakeChain::peers(), 100)
            .await
            .unwrap(),
        18
    );
    assert_eq!(second.trusted_height(), 30);
    assert_eq!(
        second.stats().headers_verified,
        18,
        "only the headers after the checkpoint were verified"
    );
    let fetches_after = *chain.fetches.lock().unwrap();
    assert!(
        fetches_after - fetches_before <= 18 * 8 * 2 + 18,
        "no re-sampling of already-verified heights"
    );
}
