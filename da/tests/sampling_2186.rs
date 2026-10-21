//! #2186 acceptance, end to end over an in-memory wire.
//!
//! * full availability ⇒ `Available`;
//! * withholding at or above the reconstruction threshold ⇒ `Unavailable`
//!   with high probability across many node secrets;
//! * sample indices differ across nodes at one height;
//! * a peer serving an invalid proof is penalised;
//! * a timeout ⇒ `Inconclusive`, never `Unavailable`;
//! * one peer's "missing" without corroboration ⇒ `Inconclusive`, not an
//!   accusation.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use wrfcoin_da::{
    CHUNK_SIZE, ChunkInclusionProof, ChunkLayout, ChunkResponse, DaCommitment, ErasureParams,
    PeerPenalty, SampleFetchError, SampleOutcome, SamplePenalty, SampleSource, SamplingConfig,
    choose_indices, detection_probability, sample,
};

/// How one peer behaves for a given index.
#[derive(Clone, Copy)]
enum Behaviour {
    Serve,
    Withhold,
    Garbage,
    Timeout,
    Transport,
}

struct FakeNetwork {
    layout: ChunkLayout,
    /// Per peer: default behaviour, plus per-index overrides.
    peers: BTreeMap<String, (Behaviour, BTreeMap<u16, Behaviour>)>,
    fetches: Mutex<Vec<(String, u16)>>,
}

impl FakeNetwork {
    fn new(layout: ChunkLayout) -> Self {
        Self {
            layout,
            peers: BTreeMap::new(),
            fetches: Mutex::new(Vec::new()),
        }
    }
    fn peer(mut self, name: &str, default: Behaviour) -> Self {
        self.peers
            .insert(name.to_string(), (default, BTreeMap::new()));
        self
    }
    fn override_index(mut self, name: &str, index: u16, b: Behaviour) -> Self {
        self.peers.get_mut(name).expect("peer").1.insert(index, b);
        self
    }
    fn peer_names(&self) -> Vec<String> {
        self.peers.keys().cloned().collect()
    }
}

#[async_trait]
impl SampleSource for FakeNetwork {
    async fn fetch(
        &self,
        peer: &str,
        _height: u64,
        index: u16,
    ) -> Result<ChunkResponse, SampleFetchError> {
        self.fetches.lock().unwrap().push((peer.to_string(), index));
        let (default, overrides) = self.peers.get(peer).expect("known peer");
        let behaviour = overrides.get(&index).copied().unwrap_or(*default);
        match behaviour {
            Behaviour::Serve => Ok(ChunkResponse {
                proof: DaCommitment::prove(&self.layout, index).expect("in range"),
            }),
            Behaviour::Withhold => Err(SampleFetchError::Missing),
            Behaviour::Garbage => {
                let mut proof: ChunkInclusionProof =
                    DaCommitment::prove(&self.layout, index).unwrap();
                proof.chunk[0] ^= 0x5a;
                Ok(ChunkResponse { proof })
            },
            Behaviour::Timeout => {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                unreachable!("the sampler's deadline fires first")
            },
            Behaviour::Transport => Err(SampleFetchError::Transport("reset".into())),
        }
    }
}

#[derive(Default)]
struct Penalties(Mutex<Vec<(String, SamplePenalty)>>);

impl PeerPenalty for Penalties {
    fn penalize(&self, peer: &str, reason: SamplePenalty) {
        self.0.lock().unwrap().push((peer.to_string(), reason));
    }
}

fn payload(chunks: usize) -> Vec<u8> {
    (0..(CHUNK_SIZE * chunks - 5))
        .map(|i| (i % 241) as u8)
        .collect()
}

fn cfg(samples: u16) -> SamplingConfig {
    SamplingConfig {
        samples_per_block: samples,
        confidence_target: 0.99,
        request_timeout: Duration::from_millis(50),
        distinct_peers_required: 2,
    }
}

const SECRET: &[u8] = b"light-node-secret-0123456789";

#[tokio::test]
async fn full_availability_is_available() {
    let layout = ChunkLayout::for_payload(&payload(64)).unwrap();
    let commitment = DaCommitment::commit(&layout);
    let net = FakeNetwork::new(layout)
        .peer("p1", Behaviour::Serve)
        .peer("p2", Behaviour::Serve)
        .peer("p3", Behaviour::Serve);
    let cfg = cfg(12);
    let indices = choose_indices(100, SECRET, &commitment, &cfg).unwrap();
    let penalties = Penalties::default();
    let outcome = sample(
        100,
        &commitment,
        &indices,
        &cfg,
        &net.peer_names(),
        &net,
        &penalties,
    )
    .await;
    assert_eq!(outcome, SampleOutcome::Available { sampled: 12 });
    assert!(penalties.0.lock().unwrap().is_empty());
    // Samples were spread across distinct peers, not all sent to one.
    let peers_used: BTreeSet<String> = net
        .fetches
        .lock()
        .unwrap()
        .iter()
        .map(|(p, _)| p.clone())
        .collect();
    assert!(
        peers_used.len() >= 2,
        "samples must be requested from multiple distinct peers"
    );
}

#[tokio::test]
async fn withholding_above_the_threshold_is_detected_across_many_seeds() {
    // Uncoded 64-chunk layout: the threshold is one chunk. A producer that
    // withholds 16 of 64 (all peers mirror the withholding, as they would if
    // the producer never published them) is caught by 16 samples with
    // probability 1 - C(48,16)/C(64,16) ≈ 0.993; over 200 secrets the miss
    // count must stay far below what an undetectable pattern would give.
    let layout = ChunkLayout::for_payload(&payload(64)).unwrap();
    let commitment = DaCommitment::commit(&layout);
    let withheld: BTreeSet<u16> = (0..64u16).filter(|i| i % 4 == 0).collect();
    let mut net = FakeNetwork::new(layout)
        .peer("p1", Behaviour::Serve)
        .peer("p2", Behaviour::Serve);
    for &i in &withheld {
        net = net
            .override_index("p1", i, Behaviour::Withhold)
            .override_index("p2", i, Behaviour::Withhold);
    }
    let cfg = cfg(16);
    let expected = detection_probability(
        ErasureParams {
            data_chunks: 64 - 16 + 1,
            total_chunks: 64,
        },
        16,
    );
    assert!(expected > 0.99, "{expected}");

    let mut detected = 0;
    let mut first_indices = BTreeSet::new();
    for seed in 0..200u32 {
        let secret = format!("seed-{seed:0>27}");
        let indices = choose_indices(500, secret.as_bytes(), &commitment, &cfg).unwrap();
        first_indices.insert(indices[0]);
        let outcome = sample(
            500,
            &commitment,
            &indices,
            &cfg,
            &net.peer_names(),
            &net,
            &wrfcoin_da::sampling::NoPenalty,
        )
        .await;
        match outcome {
            SampleOutcome::Unavailable { missing } => {
                assert!(
                    missing.iter().all(|m| withheld.contains(m)),
                    "only withheld chunks may be reported missing"
                );
                detected += 1;
            },
            SampleOutcome::Available { .. } => {},
            SampleOutcome::Inconclusive { reason } => {
                panic!("withholding must never read as inconclusive: {reason}")
            },
        }
    }
    assert!(
        detected >= 190,
        "detected {detected}/200, expected ≈ {}",
        (expected * 200.0) as u32
    );
    assert!(
        first_indices.len() > 20,
        "nodes must draw different chunks: {}",
        first_indices.len()
    );
}

#[tokio::test]
async fn one_peers_missing_is_not_an_accusation_without_corroboration() {
    let layout = ChunkLayout::for_payload(&payload(32)).unwrap();
    let commitment = DaCommitment::commit(&layout);
    let cfg = cfg(8);
    let indices = choose_indices(9, SECRET, &commitment, &cfg).unwrap();
    // p1 claims to lack the first sampled chunk; p2 has it: satisfied.
    let net = FakeNetwork::new(layout.clone())
        .peer("p1", Behaviour::Serve)
        .peer("p2", Behaviour::Serve)
        .override_index("p1", indices[0], Behaviour::Withhold);
    let outcome = sample(
        9,
        &commitment,
        &indices,
        &cfg,
        &net.peer_names(),
        &net,
        &wrfcoin_da::sampling::NoPenalty,
    )
    .await;
    assert_eq!(outcome, SampleOutcome::Available { sampled: 8 });

    // p1 lacks it and p2 times out: nobody corroborated ⇒ Inconclusive.
    let net = FakeNetwork::new(layout)
        .peer("p1", Behaviour::Serve)
        .peer("p2", Behaviour::Serve)
        .override_index("p1", indices[0], Behaviour::Withhold)
        .override_index("p2", indices[0], Behaviour::Timeout);
    let outcome = sample(
        9,
        &commitment,
        &indices,
        &cfg,
        &net.peer_names(),
        &net,
        &wrfcoin_da::sampling::NoPenalty,
    )
    .await;
    assert!(
        matches!(outcome, SampleOutcome::Inconclusive { .. }),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn timeouts_and_transport_failures_are_inconclusive_never_unavailable() {
    let layout = ChunkLayout::for_payload(&payload(16)).unwrap();
    let commitment = DaCommitment::commit(&layout);
    let cfg = cfg(4);
    let indices = choose_indices(1, SECRET, &commitment, &cfg).unwrap();
    for degraded in [Behaviour::Timeout, Behaviour::Transport] {
        let net = FakeNetwork::new(layout.clone())
            .peer("p1", degraded)
            .peer("p2", degraded);
        let outcome = sample(
            1,
            &commitment,
            &indices,
            &cfg,
            &net.peer_names(),
            &net,
            &wrfcoin_da::sampling::NoPenalty,
        )
        .await;
        assert!(
            matches!(outcome, SampleOutcome::Inconclusive { .. }),
            "{outcome:?}"
        );
    }
    // Fewer distinct peers than required: no verdict, no accusation.
    let net = FakeNetwork::new(layout).peer("only", Behaviour::Withhold);
    let outcome = sample(
        1,
        &commitment,
        &indices,
        &cfg,
        &net.peer_names(),
        &net,
        &wrfcoin_da::sampling::NoPenalty,
    )
    .await;
    assert_eq!(
        outcome,
        SampleOutcome::Inconclusive {
            reason: "fewer distinct peers than distinct_peers_required"
        }
    );
}

#[tokio::test]
async fn an_invalid_proof_is_penalised_and_counts_as_not_served() {
    let layout = ChunkLayout::for_payload(&payload(16)).unwrap();
    let commitment = DaCommitment::commit(&layout);
    let cfg = cfg(4);
    let indices = choose_indices(2, SECRET, &commitment, &cfg).unwrap();
    // p1 serves garbage for every index, p2 serves honestly: available, p1 penalised.
    let net = FakeNetwork::new(layout.clone())
        .peer("p1", Behaviour::Garbage)
        .peer("p2", Behaviour::Serve);
    let penalties = Penalties::default();
    let outcome = sample(
        2,
        &commitment,
        &indices,
        &cfg,
        &net.peer_names(),
        &net,
        &penalties,
    )
    .await;
    assert_eq!(outcome, SampleOutcome::Available { sampled: 4 });
    let recorded = penalties.0.lock().unwrap().clone();
    assert!(!recorded.is_empty(), "garbage must be penalised");
    assert!(
        recorded
            .iter()
            .all(|(p, r)| p == "p1" && *r == SamplePenalty::InvalidProof)
    );

    // Both serve garbage for one index: corroborated ⇒ that chunk is unavailable.
    let net = FakeNetwork::new(layout)
        .peer("p1", Behaviour::Serve)
        .peer("p2", Behaviour::Serve)
        .override_index("p1", indices[1], Behaviour::Garbage)
        .override_index("p2", indices[1], Behaviour::Garbage);
    let outcome = sample(
        2,
        &commitment,
        &indices,
        &cfg,
        &net.peer_names(),
        &net,
        &penalties,
    )
    .await;
    assert_eq!(
        outcome,
        SampleOutcome::Unavailable {
            missing: vec![indices[1]]
        }
    );
}
