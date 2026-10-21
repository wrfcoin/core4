//! #2884: live, two-swarm evidence for the wire-format version skew gap and
//! for the fix already merged for it (`WIRE_FORMAT_VERSION`,
//! `detect_block_sync_version_skew`/`detect_topic_version_skew`, PR #2900).
//!
//! Everything upstream of this module — the constant, the tripwire test, the
//! skew-report pure functions and their unit tests — was merged before this
//! PR opened. What none of it exercised is the thing #2900 itself named as a
//! residual: "no live mixed-version two-swarm run exercises the wiring
//! end-to-end... If an end-to-end mixed-version exercise is wanted, it should
//! be a named follow-up." This module is that follow-up. Three tests:
//!
//! 1. [`historical_layout_break_reconstructed_from_0bb151ff9_retry_after_ms`] —
//!    no swarm at all. Reconstructs `BlockResponse` exactly as it stood
//!    before `0bb151ff9` (`fix(p2p): carry block-sync refusal backoff to the
//!    requester (#2539) (#2645)`) and proves, on real production framing
//!    code (`sync::read_message`/`write_message`), that the two shapes
//!    disagree on the SAME `/wrfcoin/block-sync/1.0.0` identifier: old bytes
//!    read as new is a loud decode failure, but new bytes read as old is a
//!    *silent* success that drops the refusal backoff — the exact ambiguity
//!    0bb151ff9 was written to remove, reintroduced by an old peer nobody
//!    told.
//! 2. [`matching_pair_interoperates_on_current_wire_format_version`] — two
//!    real libp2p swarms, both speaking today's `BLOCK_SYNC_PROTOCOL`,
//!    complete a real block-sync request/response over the wire. The
//!    positive control: the fix does not cry wolf on compatible peers.
//! 3. [`version_skewed_pair_is_detected_and_rejected_cleanly`] — two real
//!    libp2p swarms, one on `BLOCK_SYNC_PROTOCOL` and one on
//!    `BUMPED_BLOCK_SYNC_PROTOCOL` (what the constant's own doc says the
//!    *next* `WIRE_FORMAT_VERSION` bump renames it to). Proves the merged
//!    fix's actual runtime behavior: negotiation fails cleanly
//!    (`OutboundFailure::UnsupportedProtocols`, connection stays open — no
//!    payload ever reaches a decoder), and `detect_block_sync_version_skew`
//!    — fed the real identify-advertised protocol list, not a hand-written
//!    string — names the local and remote generations.
//!
//! Test 3 does not reuse the full [`crate::P2pNode`] harness: doing so would
//! require threading a `WIRE_FORMAT_VERSION` override through production
//! `P2pConfig` for exactly one test, which is the hazard #2900 already
//! declined ("parameterizing wire identifiers through production config
//! solely for a test is its own hazard"). Instead this module builds a
//! minimal two-behaviour swarm (identify + block-sync only, no
//! gossipsub/kademlia/genesis-handshake gating) on the in-memory transport,
//! the same recipe [`crate::behaviour::test_swarm_with_config`] uses to stay
//! socket-free in sandboxes that deny binds (#1821) — but composing its own
//! small behaviour rather than the full production one, so the "peer on the
//! next wire-format generation" is just a different `&'static str` passed to
//! the constructor, never a change to what `crate::sync::BLOCK_SYNC_PROTOCOL`
//! resolves to for this binary.

use std::time::Duration;

use libp2p::{
    Multiaddr, StreamProtocol, Swarm, SwarmBuilder, Transport as _,
    core::transport::MemoryTransport,
    core::upgrade::Version,
    identify,
    identity::Keypair,
    noise,
    request_response::{self, ProtocolSupport},
    swarm::{NetworkBehaviour, SwarmEvent},
    yamux,
};
use tokio::time::timeout;

use crate::message::detect_block_sync_version_skew;
use crate::sync::{BLOCK_SYNC_PROTOCOL, BlockRequest, BlockResponse, BlockSyncCodec};

const TEST_TIMEOUT: Duration = Duration::from_secs(15);

// ---------------------------------------------------------------------------
// Test 1: historical layout break, reconstructed from real history
// ---------------------------------------------------------------------------

/// `BlockResponse` exactly as it stood immediately before `0bb151ff9`
/// (`git show 0bb151ff9^:p2p/src/sync.rs`) — a single field, no refusal
/// backoff. The commit appended `retry_after_ms` without touching
/// `BLOCK_SYNC_PROTOCOL`, which is the concrete instance of the #2884
/// pattern: the identifier this struct travels under never moved.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PreRetryAfterMsBlockResponse {
    blocks: Vec<wrfcoin_shared_protocol::blockchain::block::Block>,
}

/// Reconstructs the `0bb151ff9` break directly (no swarm — this is a
/// framing/decode question, not a negotiation one) using the crate's real
/// wire framing (`sync::read_message`/`write_message`, the exact
/// length-prefix + bincode code every live block-sync exchange uses) rather
/// than a hand-rolled stand-in, so the measured errors below are what a real
/// version-skewed pair would actually produce, not an approximation of it.
#[tokio::test]
async fn historical_layout_break_reconstructed_from_0bb151ff9_retry_after_ms() {
    use libp2p::futures::io::Cursor;

    use crate::sync::{read_message, write_message};

    // Direction 1: an old (pre-0bb151ff9) peer's bytes, read by today's
    // decoder. Both peers agree on `/wrfcoin/block-sync/1.0.0` throughout —
    // nothing about the identifier changed. bincode is positional, so the
    // missing second field is a loud, immediate failure: the decoder runs out
    // of bytes mid-struct.
    let old = PreRetryAfterMsBlockResponse { blocks: Vec::new() };
    let mut old_wire = Cursor::new(Vec::new());
    write_message(&mut old_wire, &old)
        .await
        .expect("old-shape BlockResponse must encode");
    let old_bytes = old_wire.into_inner();

    let mut reader = Cursor::new(old_bytes);
    let decoded: std::io::Result<BlockResponse> = read_message(&mut reader).await;
    let err = decoded.expect_err(
        "old-shape bytes (no retry_after_ms) must not silently decode as today's two-field \
         BlockResponse — if this starts passing, the codec stopped being positional",
    );
    let message = err.to_string();
    assert!(
        message.contains("bincode decode") && message.contains("VERSION SKEW"),
        "expected the production version-skew-attributed decode error, got: {message}"
    );

    // Direction 2: today's bytes, read by the OLD decoder — the more
    // dangerous half. `bincode::deserialize` does not require the whole
    // buffer be consumed, so the old peer's single-field struct reads its one
    // field, is satisfied, and returns Ok — silently discarding
    // `retry_after_ms`. That is precisely the ambiguity 0bb151ff9 introduced
    // the field to remove (a refusal cannot be told apart from an empty
    // range) reappearing on an old binary nobody told to upgrade. Same
    // identifier both directions; the bytes disagreed the whole time.
    let new = BlockResponse {
        blocks: Vec::new(),
        retry_after_ms: Some(2_500),
    };
    let mut new_wire = Cursor::new(Vec::new());
    write_message(&mut new_wire, &new)
        .await
        .expect("current BlockResponse must encode");
    let new_bytes = new_wire.into_inner();

    let mut reader = Cursor::new(new_bytes);
    let decoded: PreRetryAfterMsBlockResponse = read_message(&mut reader)
        .await
        .expect(
            "current bytes must decode under the OLD shape without error — that silence, not a \
             thrown error, is the hazard this test exists to make concrete",
        );
    assert_eq!(
        decoded.blocks.len(),
        0,
        "the fields both shapes share must still agree"
    );
    // `decoded` has no field to even hold `retry_after_ms`: an old peer given
    // this exact response has no way to learn it was refused rather than
    // told the range is empty, and nothing on `/wrfcoin/block-sync/1.0.0`
    // told it to expect otherwise.
}

// ---------------------------------------------------------------------------
// Live two-swarm harness (identify + block-sync only)
// ---------------------------------------------------------------------------

/// The block-sync protocol generation one `WIRE_FORMAT_VERSION` bump ahead of
/// what this binary speaks today — exactly what `BLOCK_SYNC_PROTOCOL` becomes
/// under the bump policy documented on [`crate::message::WIRE_FORMAT_VERSION`].
/// Defined locally rather than derived from the constant: `WIRE_FORMAT_VERSION`
/// governs only what THIS binary speaks, so "a peer that already bumped"
/// has to be built as a genuinely separate swarm, not a parameterization of
/// production config (the hazard #2900 declined for exactly this reason).
const BUMPED_BLOCK_SYNC_PROTOCOL: &str = "/wrfcoin/block-sync/2.0.0";

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "SkewProbeEvent")]
struct SkewProbeBehaviour {
    identify: identify::Behaviour,
    block_sync: request_response::Behaviour<BlockSyncCodec>,
}

#[derive(Debug)]
enum SkewProbeEvent {
    Identify(identify::Event),
    BlockSync(request_response::Event<BlockRequest, BlockResponse>),
}

impl From<identify::Event> for SkewProbeEvent {
    fn from(event: identify::Event) -> Self {
        SkewProbeEvent::Identify(event)
    }
}

impl From<request_response::Event<BlockRequest, BlockResponse>> for SkewProbeEvent {
    fn from(event: request_response::Event<BlockRequest, BlockResponse>) -> Self {
        SkewProbeEvent::BlockSync(event)
    }
}

/// Builds a minimal two-behaviour swarm (identify + block-sync only) on the
/// in-memory transport — the same no-socket recipe
/// [`crate::behaviour::test_swarm_with_config`] uses (#1821) — speaking the
/// given block-sync protocol generation. Production's genesis handshake,
/// gossipsub mesh, and connection quotas are all deliberately absent: this
/// harness isolates exactly the two wire surfaces #2884 is about.
fn build_probe_swarm(block_sync_protocol: &'static str) -> Swarm<SkewProbeBehaviour> {
    let keypair = Keypair::generate_ed25519();
    SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_other_transport(|k| {
            MemoryTransport::default()
                .upgrade(Version::V1)
                .authenticate(
                    noise::Config::new(k).expect("noise config for an in-memory test transport"),
                )
                .multiplex(yamux::Config::default())
        })
        .expect("in-memory transport must build")
        .with_behaviour(|key| SkewProbeBehaviour {
            identify: identify::Behaviour::new(identify::Config::new(
                "/wrfcoin/id/1.0.0".to_string(),
                key.public(),
            )),
            block_sync: request_response::Behaviour::with_codec(
                BlockSyncCodec,
                std::iter::once((
                    StreamProtocol::new(block_sync_protocol),
                    ProtocolSupport::Full,
                )),
                request_response::Config::default(),
            ),
        })
        .expect("behaviour must build")
        .build()
}

async fn listen_and_get_memory_addr(swarm: &mut Swarm<SkewProbeBehaviour>) -> Multiaddr {
    use libp2p::futures::StreamExt as _;

    swarm
        .listen_on("/memory/0".parse().expect("valid memory multiaddr"))
        .expect("the in-memory transport must listen without binding a socket (#1821)");
    loop {
        if let SwarmEvent::NewListenAddr { address, .. } = swarm.select_next_some().await {
            return address;
        }
    }
}

// ---------------------------------------------------------------------------
// Test 2: matching pair still interoperates (positive control)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn matching_pair_interoperates_on_current_wire_format_version() {
    use libp2p::futures::StreamExt as _;

    let mut swarm_b = build_probe_swarm(BLOCK_SYNC_PROTOCOL);
    let addr_b = listen_and_get_memory_addr(&mut swarm_b).await;
    let peer_b = *swarm_b.local_peer_id();

    let mut swarm_a = build_probe_swarm(BLOCK_SYNC_PROTOCOL);
    swarm_a.dial(addr_b).expect("A must be able to dial B");

    let request = BlockRequest {
        start_height: 0,
        count: 1,
    };
    let mut sent = false;
    let mut response: Option<BlockResponse> = None;

    let drive = async {
        loop {
            tokio::select! {
                event = swarm_a.select_next_some() => match event {
                    SwarmEvent::ConnectionEstablished { .. } => {
                        if !sent {
                            sent = true;
                            swarm_a
                                .behaviour_mut()
                                .block_sync
                                .send_request(&peer_b, request.clone());
                        }
                    }
                    SwarmEvent::Behaviour(SkewProbeEvent::BlockSync(
                        request_response::Event::Message {
                            message: request_response::Message::Response { response: resp, .. },
                            ..
                        },
                    )) => {
                        response = Some(resp);
                    }
                    SwarmEvent::Behaviour(SkewProbeEvent::BlockSync(
                        request_response::Event::OutboundFailure { error, .. },
                    )) => {
                        panic!(
                            "identical protocol strings must negotiate — an outbound failure \
                             ({error:?}) here means the matching-pair CONTROL is broken, not the \
                             mismatch case"
                        );
                    }
                    _ => {}
                },
                event = swarm_b.select_next_some() => {
                    if let SwarmEvent::Behaviour(SkewProbeEvent::BlockSync(
                        request_response::Event::Message {
                            message: request_response::Message::Request { request, channel, .. },
                            ..
                        },
                    )) = event
                    {
                        assert_eq!(request.start_height, 0);
                        assert_eq!(request.count, 1);
                        let _ = swarm_b.behaviour_mut().block_sync.send_response(
                            channel,
                            BlockResponse {
                                blocks: Vec::new(),
                                retry_after_ms: None,
                            },
                        );
                    }
                },
            }
            if response.is_some() {
                break;
            }
        }
    };

    timeout(TEST_TIMEOUT, drive).await.expect(
        "a matching pair on today's WIRE_FORMAT_VERSION must negotiate and complete a real \
         block-sync round trip within the timeout",
    );

    let response = response.expect("response must have been captured");
    assert_eq!(response.blocks.len(), 0);
    assert_eq!(response.retry_after_ms, None);
}

// ---------------------------------------------------------------------------
// Test 3: version-skewed pair is detected and rejected cleanly
// ---------------------------------------------------------------------------

#[tokio::test]
async fn version_skewed_pair_is_detected_and_rejected_cleanly() {
    use libp2p::futures::StreamExt as _;

    // B has already bumped past this binary's WIRE_FORMAT_VERSION.
    let mut swarm_b = build_probe_swarm(BUMPED_BLOCK_SYNC_PROTOCOL);
    let addr_b = listen_and_get_memory_addr(&mut swarm_b).await;
    let peer_b = *swarm_b.local_peer_id();

    // A still speaks today's identifier.
    let mut swarm_a = build_probe_swarm(BLOCK_SYNC_PROTOCOL);
    swarm_a.dial(addr_b).expect("A must be able to dial B");

    let mut request_sent = false;
    let mut outbound_failure: Option<request_response::OutboundFailure> = None;
    let mut remote_protocols: Option<Vec<String>> = None;
    let mut b_ever_saw_a_request = false;

    let drive = async {
        loop {
            tokio::select! {
                event = swarm_a.select_next_some() => match event {
                    SwarmEvent::ConnectionEstablished { .. } => {
                        // The transport connects regardless of app-level
                        // protocol identifiers — TCP/noise/yamux don't know
                        // wrfcoin exists. This is exactly why #2884 matters:
                        // connection success proves nothing about wire
                        // compatibility.
                        if !request_sent {
                            request_sent = true;
                            swarm_a.behaviour_mut().block_sync.send_request(
                                &peer_b,
                                BlockRequest { start_height: 0, count: 1 },
                            );
                        }
                    }
                    SwarmEvent::Behaviour(SkewProbeEvent::Identify(
                        identify::Event::Received { info, .. },
                    )) => {
                        // The exact call production makes on every identify
                        // exchange (crate::message::detect_block_sync_version_skew,
                        // p2p/src/node.rs) — fed a real peer's advertised
                        // protocol list here, not a hand-written string.
                        remote_protocols =
                            Some(info.protocols.iter().map(|p| p.to_string()).collect());
                    }
                    SwarmEvent::Behaviour(SkewProbeEvent::BlockSync(
                        request_response::Event::OutboundFailure { error, .. },
                    )) => {
                        outbound_failure = Some(error);
                    }
                    _ => {}
                },
                event = swarm_b.select_next_some() => {
                    if let SwarmEvent::Behaviour(SkewProbeEvent::BlockSync(
                        request_response::Event::Message {
                            message: request_response::Message::Request { .. },
                            ..
                        },
                    )) = event
                    {
                        b_ever_saw_a_request = true;
                    }
                },
            }
            if outbound_failure.is_some() && remote_protocols.is_some() {
                break;
            }
        }
    };

    timeout(TEST_TIMEOUT, drive).await.expect(
        "a version-skewed pair must both fail negotiation and complete an identify exchange \
         within the timeout",
    );

    // Rejected cleanly: failure surfaces at protocol negotiation, before any
    // payload is framed or decoded — B's decoder never runs at all.
    assert!(
        matches!(
            outbound_failure,
            Some(request_response::OutboundFailure::UnsupportedProtocols)
        ),
        "expected UnsupportedProtocols, got {outbound_failure:?}"
    );
    assert!(
        !b_ever_saw_a_request,
        "B must never receive a BlockRequest at all — the mismatch must be caught at the \
         identifier layer, not by B decoding and rejecting a request"
    );
    assert!(
        swarm_a.is_connected(&peer_b),
        "the transport connection must survive a block-sync negotiation refusal — \
         version skew must not look like a hostile disconnect"
    );

    // Detected: the identifier mismatch is attributable, not anonymous. This
    // is the merged #2884 fix's own pure function, run on real
    // identify-advertised data.
    let remote_protocols = remote_protocols.expect("A must have received B's identify info");
    let skew = detect_block_sync_version_skew(remote_protocols.iter().map(String::as_str))
        .expect("B's bumped protocol string must be flagged as block-sync version skew");
    assert_eq!(skew.local_protocol, BLOCK_SYNC_PROTOCOL);
    assert_eq!(skew.remote_protocol, BUMPED_BLOCK_SYNC_PROTOCOL);
    assert_eq!(skew.remote_wire_version(), Some(2));
    let report = skew.operator_report(&peer_b);
    assert!(report.contains(BLOCK_SYNC_PROTOCOL));
    assert!(report.contains(BUMPED_BLOCK_SYNC_PROTOCOL));
}
