//! Bonded snapshot fetch over libp2p (owner decision 2026-10-31).
//!
//! A home node that forwards only its P2P port (TCP 9000) must still be able to
//! serve the signed snapshot a bonded consumer fetches to produce a service
//! receipt. This protocol carries exactly the five server calls of
//! `wrfcoin snapshot fetch --bonded` and nothing else:
//!
//! | Request        | HTTP equivalent                                | Auth            |
//! |----------------|------------------------------------------------|-----------------|
//! | `Info`         | `GET /network/info` (identity fields only)     | none            |
//! | `SyncPayload`  | `GET /state/snapshot/sync-payload`             | bonded consumer |
//! | `ExportPage`   | `GET /state/snapshot/export?offset&limit`      | bonded consumer |
//! | `BondedTicket` | `POST /network/snapshot-sync/bonded-ticket`    | bonded consumer |
//! | `Receipt`      | `POST /network/snapshot-sync/receipt`          | bonded consumer |
//!
//! HTTP authorizes those calls with the server's Reader bearer. P2P has no
//! bearer, so every call but `Info` carries a [`BondedFetchAuth`]: the
//! consumer's registered bond key signs the (server peer id, client peer id,
//! time, nonce) under a domain tag of its own. The server accepts it only when
//! the key is a `consumer_key` in its current frozen bond roster and the
//! client peer id is the Noise-authenticated peer that sent the request, so a
//! copied token is useless from any other libp2p identity. No operator
//! capability exists on this protocol: there is no request type for one.
//!
//! The receipt, ticket, claim and endorsement formats are untouched: the
//! server answers with the same objects the HTTP routes return.
//!
//! # One source, two crates
//!
//! This file is compiled into `wrfcoin-p2p` (the serving node, and the test
//! client) and into the `wrfcoin` CLI (the fetching client) through a
//! `#[path]` module, so the wire types and the codec cannot drift between the
//! two ends. It therefore names only external crates, never `crate::`.

use std::{io, time::Duration};

use async_trait::async_trait;
use libp2p::{
    StreamProtocol,
    futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    request_response,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use wrfcoin_shared_protocol::{
    blockchain::snapshot_sync::{SignedSnapshotSyncPayload, SnapshotSyncPayload},
    consensus::node_service_receipt::SignedNodeServiceReceipt,
    crypto::{
        chain_binding::{ChainIdentity, SigningProtocolVersion, domain_preimage_bound},
        signature::{PublicKey, Signature},
    },
};

/// libp2p protocol id. A node that does not serve bonded snapshots does not
/// list it, so a fetch against it fails negotiation instead of timing out.
pub const BONDED_FETCH_PROTOCOL: &str = "/wrfcoin/bonded-snapshot-fetch/1.0.0";
/// Largest request frame a server reads. The biggest request is a ticket
/// request carrying one receipt and one manifest (HTTP caps that body at 16 KiB).
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
/// Largest response frame either side reads or writes.
pub const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
/// Hex bytes a server puts in one export page before it stops adding entries.
/// Small enough that a page crosses a 10 Mbit/s home uplink well inside
/// [`REQUEST_TIMEOUT`]; a page always carries at least one entry.
pub const EXPORT_PAGE_BUDGET_BYTES: usize = 4 * 1024 * 1024;
/// Most entries a client may ask for in one page (the HTTP route's cap).
pub const MAX_EXPORT_PAGE_ENTRIES: u32 = 1_000;
/// Per-request timeout on both ends.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// Domain tag of the consumer's fetch authorization. Off-chain only: it is
/// never a consensus preimage and no chain object carries it.
pub const AUTH_DOMAIN_TAG: &[u8] = b"wrfcoin.p2p_bonded_fetch_auth.v1";
/// How far an authorization's `issued_at` may sit from the server's clock.
pub const AUTH_MAX_SKEW_SECS: i64 = 600;
/// A client refreshes its authorization once it is this old.
pub const AUTH_REFRESH_AFTER_SECS: i64 = 300;
/// Longest peer id string accepted in an authorization (base58 ed25519 ids are 52).
pub const MAX_PEER_ID_CHARS: usize = 128;

// ---------------------------------------------------------------------------
// Authorization
// ---------------------------------------------------------------------------

/// A registered bond consumer's authorization for one client peer to fetch
/// from one server peer. All fields are hex or base58 text so the receiver
/// node can return it as JSON unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BondedFetchAuth {
    /// Hex ed25519 public key: the consumer key the bond registered.
    pub consumer_key: String,
    /// Base58 peer id of the serving node.
    pub server_peer_id: String,
    /// Base58 peer id of the libp2p client that will send the requests.
    pub client_peer_id: String,
    /// Unix seconds at signing.
    pub issued_at: i64,
    /// 16 random bytes, hex.
    pub nonce: String,
    /// Hex ed25519 signature over [`BondedFetchAuth::signing_preimage`].
    pub signature: String,
}

/// Why an authorization was refused. `code()` is the wire error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    Malformed(&'static str),
    WrongServer,
    WrongClient,
    Stale,
    BadSignature,
}

impl AuthError {
    pub fn code(&self) -> &'static str {
        match self {
            AuthError::Malformed(_) => "AUTH_MALFORMED",
            AuthError::WrongServer => "AUTH_WRONG_SERVER",
            AuthError::WrongClient => "AUTH_WRONG_CLIENT",
            AuthError::Stale => "AUTH_STALE",
            AuthError::BadSignature => "AUTH_SIGNATURE_INVALID",
        }
    }

    pub fn message(&self) -> String {
        match self {
            AuthError::Malformed(what) => format!("malformed authorization: {what}"),
            AuthError::WrongServer => "authorization names another server peer".into(),
            AuthError::WrongClient => "authorization names another client peer".into(),
            AuthError::Stale => format!("authorization is more than {AUTH_MAX_SKEW_SECS} s from this node's clock"),
            AuthError::BadSignature => "authorization signature does not verify".into(),
        }
    }
}

fn push_len_prefixed(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    buf.extend_from_slice(bytes);
}

fn peer_id_text_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PEER_ID_CHARS
        && value.bytes().all(|b| b.is_ascii_alphanumeric())
}

impl BondedFetchAuth {
    /// The exact bytes the consumer key signs. Framed by
    /// [`domain_preimage_bound`] under [`AUTH_DOMAIN_TAG`] and the chain
    /// identity, always at signing protocol V2, so it can never equal a
    /// consensus preimage of another domain.
    pub fn signing_preimage(
        identity: &ChainIdentity,
        consumer_key: &[u8; 32],
        server_peer_id: &str,
        client_peer_id: &str,
        issued_at: i64,
        nonce: &[u8; 16],
    ) -> Vec<u8> {
        let mut body = Vec::with_capacity(32 + 2 * MAX_PEER_ID_CHARS + 64);
        push_len_prefixed(&mut body, consumer_key);
        push_len_prefixed(&mut body, server_peer_id.as_bytes());
        push_len_prefixed(&mut body, client_peer_id.as_bytes());
        body.extend_from_slice(&issued_at.to_le_bytes());
        push_len_prefixed(&mut body, nonce);
        domain_preimage_bound(AUTH_DOMAIN_TAG, identity, SigningProtocolVersion::V2, &body)
    }

    /// Validate the fixed-width fields and return (consumer key, nonce).
    pub fn decoded(&self) -> Result<([u8; 32], [u8; 16]), AuthError> {
        if !peer_id_text_ok(&self.server_peer_id) || !peer_id_text_ok(&self.client_peer_id) {
            return Err(AuthError::Malformed("peer id"));
        }
        if self.consumer_key.len() != 64 || self.nonce.len() != 32 || self.signature.len() != 128 {
            return Err(AuthError::Malformed("field length"));
        }
        let mut key = [0u8; 32];
        hex::decode_to_slice(&self.consumer_key, &mut key)
            .map_err(|_| AuthError::Malformed("consumer_key"))?;
        let mut nonce = [0u8; 16];
        hex::decode_to_slice(&self.nonce, &mut nonce).map_err(|_| AuthError::Malformed("nonce"))?;
        Ok((key, nonce))
    }

    /// Verify for a request that `remote_peer_id` sent to `local_peer_id`.
    /// Returns the consumer key; the caller still checks it is registered.
    pub fn verify(
        &self,
        identity: &ChainIdentity,
        local_peer_id: &str,
        remote_peer_id: &str,
        now_unix: i64,
    ) -> Result<[u8; 32], AuthError> {
        let (key, nonce) = self.decoded()?;
        if self.server_peer_id != local_peer_id {
            return Err(AuthError::WrongServer);
        }
        if self.client_peer_id != remote_peer_id {
            return Err(AuthError::WrongClient);
        }
        if now_unix.abs_diff(self.issued_at) > AUTH_MAX_SKEW_SECS as u64 {
            return Err(AuthError::Stale);
        }
        let public = PublicKey::from_bytes(&key).map_err(|_| AuthError::Malformed("consumer_key"))?;
        let signature =
            Signature::from_hex(&self.signature).map_err(|_| AuthError::Malformed("signature"))?;
        let preimage = Self::signing_preimage(
            identity,
            &key,
            &self.server_peer_id,
            &self.client_peer_id,
            self.issued_at,
            &nonce,
        );
        if !public.verify(&preimage, &signature) {
            return Err(AuthError::BadSignature);
        }
        Ok(key)
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// One export entry, hex encoded as on the HTTP export route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportEntry {
    pub key_hex: String,
    pub value_hex: String,
}

/// Requests a bonded consumer may send. There is deliberately no other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BondedFetchRequest {
    /// Chain identity and the service id receipts must name. Unauthenticated,
    /// like `GET /network/info`.
    Info,
    /// The signed payload of the server's current cut. The server keeps that
    /// cut available for the export pages that follow, even if its chain moves.
    SyncPayload { auth: BondedFetchAuth },
    /// One page of the cut named by `height` + `snapshot_root_hex`.
    ExportPage {
        auth: BondedFetchAuth,
        height: u64,
        snapshot_root_hex: String,
        offset: u64,
        limit: u32,
    },
    /// The server's signed ticket for an installed receipt (the body of
    /// `POST /network/snapshot-sync/bonded-ticket`).
    BondedTicket {
        auth: BondedFetchAuth,
        receipt: SignedNodeServiceReceipt,
        manifest: SignedSnapshotSyncPayload,
    },
    /// Hand the consumer-signed receipt back to the server.
    Receipt { auth: BondedFetchAuth, receipt: SignedNodeServiceReceipt },
}

impl BondedFetchRequest {
    /// Short label for logs and metrics.
    pub fn label(&self) -> &'static str {
        match self {
            BondedFetchRequest::Info => "info",
            BondedFetchRequest::SyncPayload { .. } => "sync_payload",
            BondedFetchRequest::ExportPage { .. } => "export_page",
            BondedFetchRequest::BondedTicket { .. } => "bonded_ticket",
            BondedFetchRequest::Receipt { .. } => "receipt",
        }
    }
}

/// Server answers. `Error` carries the same codes the HTTP routes use where
/// one exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BondedFetchResponse {
    Info {
        chain_id: String,
        network_id: String,
        genesis_hash: String,
        service_id: String,
    },
    SyncPayload {
        payload: SnapshotSyncPayload,
        signed_payload: SignedSnapshotSyncPayload,
    },
    ExportPage {
        height: u64,
        snapshot_root_hex: String,
        total: u64,
        next_offset: Option<u64>,
        entries: Vec<ExportEntry>,
    },
    BondedTicket { claim_hex: String },
    ReceiptAck { accepted: bool, recorded_receipts: u64 },
    Error {
        code: String,
        message: String,
        retry_after_ms: Option<u64>,
    },
}

impl BondedFetchResponse {
    pub fn error(code: &str, message: impl Into<String>) -> Self {
        BondedFetchResponse::Error {
            code: code.to_string(),
            message: message.into(),
            retry_after_ms: None,
        }
    }

    pub fn retry_later(code: &str, message: impl Into<String>, retry_after_ms: u64) -> Self {
        BondedFetchResponse::Error {
            code: code.to_string(),
            message: message.into(),
            retry_after_ms: Some(retry_after_ms),
        }
    }
}

// ---------------------------------------------------------------------------
// Codec: u32 big-endian length, then JSON. Each direction has its own ceiling.
// ---------------------------------------------------------------------------

pub fn encode_frame<V: Serialize>(value: &V, max: usize) -> io::Result<Vec<u8>> {
    let body = serde_json::to_vec(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if body.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bonded-fetch frame of {} bytes exceeds {max}", body.len()),
        ));
    }
    let len = u32::try_from(body.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame length"))?;
    let mut framed = len.to_be_bytes().to_vec();
    framed.extend_from_slice(&body);
    Ok(framed)
}

pub async fn read_frame<T, V>(io: &mut T, max: usize) -> io::Result<V>
where
    T: AsyncRead + Unpin + Send,
    V: DeserializeOwned,
{
    let mut len_bytes = [0u8; 4];
    io.read_exact(&mut len_bytes).await?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    if len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bonded-fetch frame of {len} bytes exceeds {max}"),
        ));
    }
    let mut body = vec![0u8; len]; // panic-ok: len <= max is checked just above, and max is MAX_REQUEST_BYTES (64 KiB) or MAX_RESPONSE_BYTES (32 MiB)
    io.read_exact(&mut body).await?;
    serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

async fn write_frame<T, V>(io: &mut T, value: &V, max: usize) -> io::Result<()>
where
    T: AsyncWrite + Unpin + Send,
    V: Serialize,
{
    let framed = encode_frame(value, max)?;
    io.write_all(&framed).await?;
    io.flush().await
}

#[derive(Debug, Clone, Default)]
pub struct BondedFetchCodec;

#[async_trait]
impl request_response::Codec for BondedFetchCodec {
    type Protocol = StreamProtocol;
    type Request = BondedFetchRequest;
    type Response = BondedFetchResponse;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        read_frame(io, MAX_REQUEST_BYTES).await
    }

    async fn read_response<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        read_frame(io, MAX_RESPONSE_BYTES).await
    }

    async fn write_request<T>(&mut self, _: &Self::Protocol, io: &mut T, request: Self::Request) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_frame(io, &request, MAX_REQUEST_BYTES).await
    }

    async fn write_response<T>(&mut self, _: &Self::Protocol, io: &mut T, response: Self::Response) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_frame(io, &response, MAX_RESPONSE_BYTES).await
    }
}

/// Bytes an entry adds to an export page body (hex plus JSON punctuation).
pub fn export_entry_cost(key_len: usize, value_len: usize) -> usize {
    key_len
        .saturating_mul(2)
        .saturating_add(value_len.saturating_mul(2))
        .saturating_add(32)
}

/// Choose the end (exclusive) of an export page starting at `offset`: at most
/// `limit` entries and [`EXPORT_PAGE_BUDGET_BYTES`] of hex, but never empty
/// when entries remain. `sizes` yields (key len, value len) from `offset`.
pub fn export_page_end(
    offset: usize,
    total: usize,
    limit: u32,
    sizes: impl Iterator<Item = (usize, usize)>,
) -> usize {
    let limit = limit.clamp(1, MAX_EXPORT_PAGE_ENTRIES) as usize;
    let max_end = offset.saturating_add(limit).min(total);
    let mut end = offset;
    let mut used = 0usize;
    for (key_len, value_len) in sizes {
        if end >= max_end {
            break;
        }
        let cost = export_entry_cost(key_len, value_len);
        if end > offset && used.saturating_add(cost) > EXPORT_PAGE_BUDGET_BYTES {
            break;
        }
        used = used.saturating_add(cost);
        end += 1;
    }
    end
}

/// The libp2p request-response behaviour for this protocol. `serve` decides
/// whether this end answers inbound requests (a server) or only sends them.
pub fn behaviour(serve: bool) -> request_response::Behaviour<BondedFetchCodec> {
    let support = if serve {
        request_response::ProtocolSupport::Full
    } else {
        request_response::ProtocolSupport::Outbound
    };
    request_response::Behaviour::with_codec(
        BondedFetchCodec,
        std::iter::once((StreamProtocol::new(BONDED_FETCH_PROTOCOL), support)),
        request_response::Config::default().with_request_timeout(REQUEST_TIMEOUT),
    )
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// A one-server client: its own throwaway libp2p identity, TCP + Noise +
/// yamux, and this protocol only. It is not a node: no gossip, no Kademlia,
/// no block sync.
pub mod client {
    use super::*;
    use libp2p::{
        Multiaddr, PeerId, Swarm, SwarmBuilder,
        futures::StreamExt,
        multiaddr::Protocol,
        noise,
        swarm::SwarmEvent,
        tcp, yamux,
    };

    /// The peer id a `/.../p2p/<id>` multiaddr ends with.
    pub fn server_peer_id(addr: &Multiaddr) -> Result<PeerId, String> {
        match addr.iter().last() {
            Some(Protocol::P2p(peer)) => Ok(peer),
            _ => Err("P2P address must end with /p2p/<peer id>".to_string()),
        }
    }

    pub struct BondedFetchClient {
        swarm: Swarm<request_response::Behaviour<BondedFetchCodec>>,
        server: PeerId,
    }

    impl BondedFetchClient {
        /// Dial `addr` (which must name the server's peer id) and wait for the
        /// Noise handshake to complete.
        pub async fn connect(addr: Multiaddr, connect_timeout: Duration) -> Result<Self, String> {
            let server = server_peer_id(&addr)?;
            let mut swarm = SwarmBuilder::with_new_identity()
                .with_tokio()
                .with_tcp(tcp::Config::default().nodelay(true), noise::Config::new, yamux::Config::default)
                .map_err(|e| format!("P2P transport: {e}"))?
                .with_dns()
                .map_err(|e| format!("P2P DNS: {e}"))?
                .with_behaviour(|_| behaviour(false))
                .map_err(|e| format!("P2P behaviour: {e}"))?
                .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(600)))
                .build();
            swarm.dial(addr.clone()).map_err(|e| format!("dial {addr}: {e}"))?;
            let connected = tokio::time::timeout(connect_timeout, async {
                loop {
                    match swarm.select_next_some().await {
                        SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == server => {
                            return Ok(());
                        },
                        SwarmEvent::OutgoingConnectionError { error, .. } => {
                            return Err(format!("connect to {addr}: {error}"));
                        },
                        _ => {},
                    }
                }
            })
            .await
            .map_err(|_| format!("connect to {addr}: timed out"))?;
            connected?;
            Ok(Self { swarm, server })
        }

        pub fn local_peer_id(&self) -> PeerId {
            *self.swarm.local_peer_id()
        }

        pub fn server_peer_id(&self) -> PeerId {
            self.server
        }

        /// Send one request and wait for its answer (bounded by [`REQUEST_TIMEOUT`]).
        pub async fn call(&mut self, request: BondedFetchRequest) -> Result<BondedFetchResponse, String> {
            let id = self.swarm.behaviour_mut().send_request(&self.server, request);
            loop {
                match self.swarm.select_next_some().await {
                    SwarmEvent::Behaviour(request_response::Event::Message {
                        message: request_response::Message::Response { request_id, response },
                        ..
                    }) if request_id == id => return Ok(response),
                    SwarmEvent::Behaviour(request_response::Event::OutboundFailure {
                        request_id,
                        error,
                        ..
                    }) if request_id == id => {
                        return Err(match error {
                            request_response::OutboundFailure::UnsupportedProtocols => {
                                "server does not serve bonded snapshots over P2P".to_string()
                            },
                            other => format!("P2P request failed: {other}"),
                        });
                    },
                    _ => {},
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use wrfcoin_shared_protocol::crypto::signature::PrivateKey;

    fn identity() -> ChainIdentity {
        ChainIdentity::new("888", "testnet")
    }

    fn signed(sk: &PrivateKey, server: &str, client: &str, at: i64) -> BondedFetchAuth {
        let key: [u8; 32] = PublicKey::from_private_key(sk).as_bytes().try_into().unwrap();
        let nonce = [7u8; 16];
        let pre = BondedFetchAuth::signing_preimage(&identity(), &key, server, client, at, &nonce);
        BondedFetchAuth {
            consumer_key: hex::encode(key),
            server_peer_id: server.into(),
            client_peer_id: client.into(),
            issued_at: at,
            nonce: hex::encode(nonce),
            signature: sk.sign(&pre).to_hex(),
        }
    }

    fn key(seed: u8) -> PrivateKey {
        PrivateKey::from_bytes(&[seed; 32]).unwrap()
    }

    #[test]
    fn auth_verifies_only_for_its_server_client_time_and_chain() {
        let sk = key(5);
        let auth = signed(&sk, "Server1", "Client1", 1_000);
        let expected: [u8; 32] = PublicKey::from_private_key(&sk).as_bytes().try_into().unwrap();
        assert_eq!(auth.verify(&identity(), "Server1", "Client1", 1_000), Ok(expected));
        assert_eq!(auth.verify(&identity(), "Server1", "Client1", 1_000 + AUTH_MAX_SKEW_SECS), Ok(expected));
        assert_eq!(auth.verify(&identity(), "Server2", "Client1", 1_000), Err(AuthError::WrongServer));
        assert_eq!(auth.verify(&identity(), "Server1", "Client2", 1_000), Err(AuthError::WrongClient));
        assert_eq!(auth.verify(&identity(), "Server1", "Client1", 1_000 + AUTH_MAX_SKEW_SECS + 1), Err(AuthError::Stale));
        assert_eq!(auth.verify(&identity(), "Server1", "Client1", 1_000 - AUTH_MAX_SKEW_SECS - 1), Err(AuthError::Stale));
        // Another chain: same bytes, different domain.
        assert_eq!(
            auth.verify(&ChainIdentity::new("777", "testnet"), "Server1", "Client1", 1_000),
            Err(AuthError::BadSignature)
        );
        // A key swap without re-signing.
        let mut swapped = auth.clone();
        swapped.consumer_key = hex::encode(PublicKey::from_private_key(&key(6)).as_bytes());
        assert_eq!(swapped.verify(&identity(), "Server1", "Client1", 1_000), Err(AuthError::BadSignature));
        // Re-pointing the client is caught by the signature, not just the peer check.
        let mut moved = auth.clone();
        moved.client_peer_id = "Client2".into();
        assert_eq!(moved.verify(&identity(), "Server1", "Client2", 1_000), Err(AuthError::BadSignature));
        let mut later = auth;
        later.issued_at += 1;
        assert_eq!(later.verify(&identity(), "Server1", "Client1", 1_000), Err(AuthError::BadSignature));
    }

    #[test]
    fn auth_tag_collides_with_no_registered_signed_object() {
        use wrfcoin_shared_protocol::crypto::domain_registry::SIGNED_OBJECT_REGISTRY;
        assert!(SIGNED_OBJECT_REGISTRY.iter().all(|entry| entry.tag != AUTH_DOMAIN_TAG));
    }

    #[test]
    fn malformed_auth_fields_are_refused_before_any_crypto() {
        let good = signed(&key(5), "Server1", "Client1", 1);
        for mutate in [
            |a: &mut BondedFetchAuth| a.consumer_key.push('0'),
            |a: &mut BondedFetchAuth| a.nonce = "zz".repeat(16),
            |a: &mut BondedFetchAuth| a.signature.truncate(10),
            |a: &mut BondedFetchAuth| a.server_peer_id = "x".repeat(MAX_PEER_ID_CHARS + 1),
            |a: &mut BondedFetchAuth| a.client_peer_id = "bad/peer".into(),
            |a: &mut BondedFetchAuth| a.client_peer_id = String::new(),
        ] {
            let mut auth = good.clone();
            mutate(&mut auth);
            assert!(matches!(
                auth.verify(&identity(), &auth.server_peer_id.clone(), &auth.client_peer_id.clone(), 1),
                Err(AuthError::Malformed(_))
            ));
        }
    }

    #[test]
    fn preimage_is_domain_framed_and_binds_every_field() {
        let base = BondedFetchAuth::signing_preimage(&identity(), &[1; 32], "S", "C", 9, &[2; 16]);
        let tag_len = (AUTH_DOMAIN_TAG.len() as u64).to_le_bytes();
        assert!(base.starts_with(&tag_len), "length-prefixed domain tag comes first");
        assert_eq!(&base[8..8 + AUTH_DOMAIN_TAG.len()], AUTH_DOMAIN_TAG);
        for other in [
            BondedFetchAuth::signing_preimage(&identity(), &[3; 32], "S", "C", 9, &[2; 16]),
            BondedFetchAuth::signing_preimage(&identity(), &[1; 32], "S2", "C", 9, &[2; 16]),
            BondedFetchAuth::signing_preimage(&identity(), &[1; 32], "S", "C2", 9, &[2; 16]),
            BondedFetchAuth::signing_preimage(&identity(), &[1; 32], "S", "C", 10, &[2; 16]),
            BondedFetchAuth::signing_preimage(&identity(), &[1; 32], "S", "C", 9, &[4; 16]),
            // Moving a byte across the server/client boundary changes the framing.
            BondedFetchAuth::signing_preimage(&identity(), &[1; 32], "SC", "", 9, &[2; 16]),
        ] {
            assert_ne!(base, other);
        }
    }

    #[test]
    fn requests_and_responses_round_trip_through_the_frame() {
        let auth = signed(&key(5), "S", "C", 1);
        let requests = vec![
            BondedFetchRequest::Info,
            BondedFetchRequest::SyncPayload { auth: auth.clone() },
            BondedFetchRequest::ExportPage {
                auth,
                height: 5,
                snapshot_root_hex: "ab".repeat(32),
                offset: 10,
                limit: 20,
            },
        ];
        for request in requests {
            let framed = encode_frame(&request, MAX_REQUEST_BYTES).unwrap();
            let decoded: BondedFetchRequest =
                futures_executor_block_on(read_frame(&mut &framed[..], MAX_REQUEST_BYTES)).unwrap();
            assert_eq!(decoded, request);
        }
        let response = BondedFetchResponse::ExportPage {
            height: 5,
            snapshot_root_hex: "cd".repeat(32),
            total: 2,
            next_offset: None,
            entries: vec![ExportEntry { key_hex: "00".into(), value_hex: "ff".into() }],
        };
        let framed = encode_frame(&response, MAX_RESPONSE_BYTES).unwrap();
        let decoded: BondedFetchResponse =
            futures_executor_block_on(read_frame(&mut &framed[..], MAX_RESPONSE_BYTES)).unwrap();
        assert_eq!(decoded, response);
    }

    #[test]
    fn oversized_frames_are_refused_on_both_write_and_read() {
        let big = BondedFetchResponse::error("X", "y".repeat(MAX_REQUEST_BYTES));
        assert!(encode_frame(&big, MAX_REQUEST_BYTES).is_err(), "writer refuses");
        // A reader refuses a header claiming more than its ceiling without
        // allocating or reading the body.
        let mut lying = (MAX_REQUEST_BYTES as u32 + 1).to_be_bytes().to_vec();
        lying.extend_from_slice(b"{}");
        let read: io::Result<BondedFetchRequest> =
            futures_executor_block_on(read_frame(&mut &lying[..], MAX_REQUEST_BYTES));
        assert_eq!(read.unwrap_err().kind(), io::ErrorKind::InvalidData);
        // A truncated body is an error, not a partial value.
        let mut short = encode_frame(&BondedFetchRequest::Info, MAX_REQUEST_BYTES).unwrap();
        short.pop();
        let read: io::Result<BondedFetchRequest> =
            futures_executor_block_on(read_frame(&mut &short[..], MAX_REQUEST_BYTES));
        assert!(read.is_err());
        // Unknown operations (an "operator" call, say) do not decode at all.
        let forged = encode_frame(&serde_json::json!({"op":"shutdown"}), MAX_REQUEST_BYTES).unwrap();
        let read: io::Result<BondedFetchRequest> =
            futures_executor_block_on(read_frame(&mut &forged[..], MAX_REQUEST_BYTES));
        assert!(read.is_err());
    }

    #[test]
    fn export_pages_respect_the_entry_limit_and_the_byte_budget_but_never_stall() {
        let small = vec![(10usize, 10usize); 5000];
        assert_eq!(export_page_end(0, 5000, 50, small.iter().copied()), 50);
        assert_eq!(export_page_end(0, 5000, 0, small.iter().copied()), 1, "limit is clamped up to 1");
        assert_eq!(
            export_page_end(0, 5000, u32::MAX, small.iter().copied()),
            MAX_EXPORT_PAGE_ENTRIES as usize
        );
        assert_eq!(export_page_end(4990, 5000, 50, small[4990..].iter().copied()), 5000);
        assert_eq!(export_page_end(5000, 5000, 50, std::iter::empty()), 5000);
        let half = EXPORT_PAGE_BUDGET_BYTES / 4;
        let big = vec![(0usize, half); 10];
        // One entry of just over half the budget fits; a second would exceed it.
        assert_eq!(export_page_end(0, 10, 1000, big.iter().copied()), 1);
        let huge = vec![(0usize, EXPORT_PAGE_BUDGET_BYTES); 3];
        assert_eq!(export_page_end(1, 3, 1000, huge[1..].iter().copied()), 2, "one oversized entry still moves the page forward");
    }

    fn futures_executor_block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(future)
    }
}
