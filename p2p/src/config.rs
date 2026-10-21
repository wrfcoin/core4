//! P2P node configuration.
//!
//! Mesh parameters can be overridden via environment variables:
//! - `WRFCOIN_GOSSIPSUB_MESH_N` -- target mesh peers per topic (D)
//! - `WRFCOIN_GOSSIPSUB_MESH_N_LOW` -- graft trigger (D_lo)
//! - `WRFCOIN_GOSSIPSUB_MESH_N_HIGH` -- prune trigger (D_hi)
//! - `WRFCOIN_GOSSIPSUB_MESH_OUTBOUND_MIN` -- outbound-only mesh peers (D_out)
//! - `WRFCOIN_GOSSIPSUB_FLOOD_PUBLISH` -- flood new publishes beyond mesh peers
//! - `WRFCOIN_IDLE_CONNECTION_TIMEOUT_SECS` -- idle connection timeout
//! - `WRFCOIN_MAX_PEERS_PER_SUBNET` -- max peers from same /16 subnet (0 = disabled)
//! - `WRFCOIN_P2P_EXTERNAL_ADDR` -- operator-specified external multiaddr (Docker / NAT)
//! - `WRFCOIN_AUTONAT_ONLY_GLOBAL_IPS` -- restrict AutoNAT probes to global IPs (default: true)
//! - `WRFCOIN_P2P_BOOTSTRAP_REDIAL_INTERVAL_SECS` -- base interval between bootstrap-peer redial attempts (default: 30)
//! - `WRFCOIN_P2P_BOOTSTRAP_REDIAL_MAX_BACKOFF_SECS` -- maximum exponential backoff cap (default: 300)
//! - `WRFCOIN_P2P_INBOUND_RATE_LIMIT_BYTES` -- per-peer inbound gossip bytes per window (default: 8388608; 0 disables)
//! - `WRFCOIN_P2P_INBOUND_RATE_LIMIT_WINDOW_SECS` -- inbound rate-limit window (default: 10)
//! - `WRFCOIN_P2P_MAX_INCOMING_CONNECTIONS` -- hard cap on established inbound connections (default: 128; 0 = unlimited)
//! - `WRFCOIN_P2P_MAX_PENDING_INCOMING` -- hard cap on in-flight inbound connections awaiting upgrade, rejected before the Noise handshake (default: 32; 0 = unlimited)
//! - `WRFCOIN_P2P_MAX_ESTABLISHED_PER_PEER` -- max simultaneous connections to a single peer (default: 4; 0 = unlimited)
//! - `WRFCOIN_P2P_MAX_CONNECTIONS` -- hard cap on total established connections, inbound + outbound (default: 256; 0 = unlimited)
//!
//! Chain identity:
//! - `WRFCOIN_NETWORK_ID` -- network identifier (default: `mainnet`)
//! - `WRFCOIN_CHAIN_ID` -- chain identifier (default: `wrfcoin-main`)
//! - `WRFCOIN_GENESIS_HASH` -- hex genesis hash advertised in the P2P handshake.
//!   When unset and `WRFCOIN_NETWORK_ID=testnet`, defaults to the canonical
//!   testnet genesis hash (see `docs/operations/TESTNET_GENESIS.md`, #1715) so
//!   testnet nodes fail closed against peers on any other chain. Otherwise
//!   defaults to the all-zero placeholder, which the handshake rejects as
//!   launch-unsafe.

use libp2p::{Multiaddr, PeerId};
use std::net::SocketAddr;
use std::time::Duration;

/// Read an environment variable as `usize`, falling back to `default`.
fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Read an environment variable as `u64`, falling back to `default`.
fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Read an environment variable as `u16`, falling back to `default`.
fn env_u16(name: &str, default: u16) -> u16 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Read an environment variable as a connection-limit setting.
///
/// Returns `Some(n)` for a positive value (the hard cap) and `None` for `0`
/// (unlimited / limit disabled). Unparseable values fall back to `default`.
/// This mirrors the `0 = disabled` convention used by `max_peers_per_subnet`.
fn env_conn_limit(name: &str, default: Option<u32>) -> Option<u32> {
    match std::env::var(name).ok().and_then(|v| v.trim().parse::<u32>().ok()) {
        Some(0) => None,
        Some(n) => Some(n),
        None => default,
    }
}

/// Read an environment variable as `u32`, falling back to `default`.
fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// Read an environment variable as `bool`, falling back to `default`.
fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(default)
}

/// Read an environment variable as an optional `String`.
fn env_string(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}

/// Launch-unsafe placeholder genesis hash (all-zero). A node advertising this
/// is rejected by the genesis handshake (`has_placeholder_identity`).
const ZERO_GENESIS_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// Canonical genesis hash of the public testnet (`network_id = "testnet"`,
/// `chain_id = 777`).
///
/// This is the hash of the chain's first block (**height 1**; this chain has no
/// block 0). It was read from the live N5 testnet seed and is documented in
/// `docs/operations/TESTNET_GENESIS.md` (core4 #1715). It is **not**
/// deterministically derivable from a spec — `Block::calculate_hash` folds in
/// the wall-clock genesis timestamp — so it is pinned here as a literal so that
/// a testnet node fails closed against peers on any other chain even when the
/// operator forgets to set `WRFCOIN_GENESIS_HASH` explicitly.
///
/// Re-pinned **2026-09-13** after the testnet re-cut on core4 `72a993552d18`
/// (infra `c3ce814cf596`). Prior values `805efc973fe5…3442625` (2026-08-05
/// cut, `8f072b31` binary), `734a1c4bfa6e…b82130b` (2026-09-11 cut),
/// `a3301def68…a437658`, `c452095921…0715fa` and `62c9e7e1…0f0f640` are dead
/// (their chains were retired). Because the genesis folds in a wall-clock
/// timestamp, every relaunch mints a new hash; update this literal to the new
/// live value each time (cf. #1719).
///
/// **Updating this is part of the cut, not a follow-up.** The fallback below is
/// reached whenever an operator does not set `WRFCOIN_GENESIS_HASH`, and a stale
/// literal there does not mean "no check" — it means the node fails closed
/// against the *current* chain while reporting a genesis mismatch naming a chain
/// that no longer exists. Measured on the seed immediately after the 2026-08-05
/// cut: it connected and was graylisted in a loop, `expected a3301def… got
/// 805efc97…`, until pinned explicitly.
const TESTNET_GENESIS_HASH: &str =
    "6ad436386cfc6c746146f6ef2f9d32ddb7a93f18efd3635606abd27c9ca17db8"; // pragma: allowlist secret -- public genesis hash

/// Resolve the genesis hash for the local node.
///
/// Precedence:
/// 1. An explicit, non-empty `WRFCOIN_GENESIS_HASH` always wins (operators can
///    pin any chain, and a future redeploy can override the built-in default).
/// 2. Otherwise, if `WRFCOIN_NETWORK_ID` is `testnet`, default to the canonical
///    testnet genesis hash so testnet peers fail closed against other chains.
/// 3. Otherwise fall back to the all-zero placeholder (the handshake then
///    treats the identity as launch-unsafe and refuses to peer).
fn resolve_genesis_hash() -> String {
    if let Some(explicit) = env_string("WRFCOIN_GENESIS_HASH") {
        return explicit;
    }
    let network_id = std::env::var("WRFCOIN_NETWORK_ID").unwrap_or_default();
    if network_id.trim().eq_ignore_ascii_case("testnet") {
        return TESTNET_GENESIS_HASH.to_string();
    }
    ZERO_GENESIS_HASH.to_string()
}

/// The genesis hash this node *expects* its local chain to have, if any, using
/// the same precedence as the p2p handshake ([`resolve_genesis_hash`]).
///
/// Returns `Some(hash)` (bare lowercase hex, no `0x` prefix) when a *committing*
/// expectation exists — i.e. an explicit `WRFCOIN_GENESIS_HASH`, or the pinned
/// [`TESTNET_GENESIS_HASH`] when `WRFCOIN_NETWORK_ID=testnet`. Returns `None`
/// when the resolver falls back to the all-zero placeholder (dev / mainnet with
/// nothing pinned), signalling "no genesis expectation to assert against".
///
/// This is the single source of truth shared with the handshake so a boot-time
/// genesis assertion (#2035) and peer rejection agree on what "the right chain"
/// means.
pub fn expected_genesis_hash() -> Option<String> {
    let resolved = resolve_genesis_hash();
    if resolved.eq_ignore_ascii_case(ZERO_GENESIS_HASH) {
        None
    } else {
        Some(resolved)
    }
}



// --- Node role and interface separation (#2120) ---------------------------------------

/// What this node is for, which decides which interfaces it may expose.
///
/// Defaults to [`NodeRole::Public`] — deliberately the least-privileged role, and the one
/// that reproduces today's behaviour exactly. Defaulting to `Validator` would impose the
/// sentry-allowlist requirement on every node that has never heard of roles, and the
/// ~12-node testnet configures none: every one of them would refuse to start. A hardening
/// default that halts the chain is not hardening.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeRole {
    /// Produces blocks. Must not expose RPC or admin off-box, and accepts inbound
    /// consensus traffic only from named sentries.
    Validator,
    /// Sits between public peers and validators. May expose public interfaces.
    Sentry,
    /// An ordinary peer. No additional restriction beyond the shared metrics rule.
    #[default]
    Public,
}

impl NodeRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Validator => "validator",
            Self::Sentry => "sentry",
            Self::Public => "public",
        }
    }

    /// Parse from configuration. Unrecognised input is `None` so the caller can refuse
    /// rather than silently pick a role — guessing here would pick the node's exposure.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "validator" => Some(Self::Validator),
            "sentry" => Some(Self::Sentry),
            "public" => Some(Self::Public),
            _ => None,
        }
    }
}

/// #2120: may this node keep an **inbound** connection from `peer`?
///
/// Only a validator restricts inbound peers, and only to its sentry allowlist.
/// Outbound dials are never subject to it — a validator dials its own sentries,
/// and the node chose that peer. A missing or empty allowlist cannot occur on a
/// running validator (`validate` refuses to start), so it means "nobody" here,
/// never "anybody": fail closed.
pub fn inbound_consensus_allowed(
    role: NodeRole,
    sentry_allowlist: Option<&[PeerId]>,
    is_dialer: bool,
    peer: &PeerId,
) -> bool {
    if is_dialer || role != NodeRole::Validator {
        return true;
    }
    sentry_allowlist.map(|list| list.contains(peer)).unwrap_or(false)
}

/// Environment names the role settings are read from (#2120). Consumed by
/// [`P2pConfig::apply_role_env`]; the boot path decides when to call it.
pub const P2P_NODE_ROLE_ENV: &str = "WRFCOIN_P2P_NODE_ROLE";
/// Comma-separated libp2p peer ids of the sentries a validator accepts inbound from.
pub const P2P_SENTRY_ALLOWLIST_ENV: &str = "WRFCOIN_P2P_SENTRY_ALLOWLIST";

/// Parse the role settings from their raw values. Pure, so it is testable
/// without touching the process environment. An unknown role word, or an
/// allowlist entry that is not a peer id, is refused rather than guessed.
pub fn parse_role_settings(
    role: Option<&str>,
    allowlist: Option<&str>,
) -> Result<(NodeRole, Option<Vec<PeerId>>), String> {
    let role = match role.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => NodeRole::parse(raw).ok_or_else(|| {
            format!("{P2P_NODE_ROLE_ENV}={raw:?} is not one of validator|sentry|public")
        })?,
        None => NodeRole::default(),
    };
    let allowlist = match allowlist {
        None => None,
        Some(raw) => {
            let mut ids = Vec::new();
            for entry in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let id = entry
                    .parse::<PeerId>()
                    .map_err(|e| format!("{P2P_SENTRY_ALLOWLIST_ENV} entry {entry:?} is not a peer id: {e}"))?;
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            Some(ids)
        }
    };
    Ok((role, allowlist))
}

/// Addresses this node exposes, separated by purpose (#2120).
///
/// `None` means "this interface is not exposed at all", which is distinct from "exposed on
/// loopback". Both are safe; conflating them would lose the ability to say a validator
/// serves no RPC whatsoever.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InterfaceBinding {
    pub public_rpc: Option<SocketAddr>,
    pub metrics: Option<SocketAddr>,
    pub admin: Option<SocketAddr>,
}

impl InterfaceBinding {
    fn offending(name: &'static str, addr: Option<SocketAddr>) -> Option<(&'static str, SocketAddr)> {
        match addr {
            Some(a) if !a.ip().is_loopback() => Some((name, a)),
            _ => None,
        }
    }
}

/// Configuration for a WRFCoin P2P node.
///
/// Sensible defaults are provided via the [`Default`] implementation so
/// callers only need to override the values they care about.
#[derive(Debug, Clone)]
pub struct P2pConfig {
    /// Addresses the node listens on.
    /// Default: `["/ip4/0.0.0.0/tcp/8555"]`
    pub listen_addresses: Vec<Multiaddr>,

    /// Bootstrap peers to connect to on startup.
    pub bootstrap_peers: Vec<Multiaddr>,

    // -- Gossipsub mesh parameters ------------------------------------------
    /// Target number of mesh peers per topic (D).
    pub mesh_n: usize,
    /// Minimum mesh peers before the router tries to graft more (D_lo).
    pub mesh_n_low: usize,
    /// Maximum mesh peers before the router prunes excess (D_hi).
    pub mesh_n_high: usize,
    /// Minimum outbound-only mesh peers (D_out, gossipsub v1.1 eclipse
    /// attack mitigation).
    pub mesh_outbound_min: usize,

    /// Interval between gossipsub heartbeats.
    pub heartbeat_interval: Duration,

    /// Maximum size of a single gossipsub RPC message in bytes.
    pub max_transmit_size: usize,

    /// Maximum size of inline data payloads (weather observations, forecast
    /// receipts) in gossipsub messages. Data exceeding this threshold must
    /// use content-addressed (CID) references instead of inline bytes.
    /// Default: 64 KiB.
    pub inline_data_max: usize,

    /// When `true`, new messages are flooded to all known subscribers instead
    /// of only to mesh peers.  This is the correct setting for small networks
    /// (<50 nodes) and should be turned off once the network grows past ~50
    /// peers.
    pub flood_publish: bool,

    // -- Kademlia -----------------------------------------------------------
    /// Protocol name advertised on the DHT.
    pub kademlia_protocol: String,

    // -- General networking -------------------------------------------------
    /// Size of the internal command channel buffer.
    pub command_buffer: usize,

    /// Size of the internal event channel buffer.
    pub event_buffer: usize,

    /// Per-peer inbound gossip bytes allowed per rate-limit window.
    /// Set to `0` to disable the app-level limiter.
    pub inbound_rate_limit_bytes: u64,

    /// Window used with [`inbound_rate_limit_bytes`](Self::inbound_rate_limit_bytes).
    pub inbound_rate_limit_window: Duration,

    /// Timeout applied when dialling a bootstrap peer.
    pub dial_timeout: Duration,

    /// How long an idle connection is kept open before being closed.
    pub idle_connection_timeout: Duration,

    // -- Eclipse / Sybil resistance -------------------------------------------
    /// Maximum number of connected peers allowed from the same /16 IPv4 subnet.
    /// Set to `0` to disable subnet diversity enforcement (useful for local
    /// testing or single-datacenter deployments).
    pub max_peers_per_subnet: usize,

    // -- Outbound slot separation and diversity (#2114) -----------------------
    /// Ceilings on how concentrated the self-chosen *outbound* peer set may
    /// become, plus the threshold below which the set is reported as
    /// low-diversity (the observable signature of an eclipse in progress).
    ///
    /// The inbound/outbound split itself is *derived* from
    /// `max_established_total` and `max_established_incoming` rather than
    /// configured separately — see [`crate::slots::PeerSlots`]. Two independent
    /// sets of numbers deciding one budget is how the reservation gets
    /// dissolved by accident.
    pub outbound_diversity: crate::slots::OutboundDiversityPolicy,
    // -- Address-derived connection quotas (#2113) ----------------------------
    /// Concurrent-connection ceilings by source IP and subnet, enforced at the
    /// pending-inbound stage — before the Noise handshake — by
    /// [`crate::quota::QuotaBehaviour`].
    ///
    /// This is a distinct axis from `max_peers_per_subnet` above, and both are
    /// live. That one counts *peer identities* per `/16` and acts after the
    /// connection is established; this one counts *concurrent connections* per
    /// exact IP, per IPv4 `/24` and per IPv6 `/64`, and acts before any
    /// handshake work is done. Neither subsumes the other, so they are kept
    /// separate rather than merged into a single misleading number.
    pub connection_quota: crate::quota::ConnectionLimits,
    // -- Pre-authentication resource budget (#2117) ---------------------------
    /// Ceilings on work an unproven peer can make this node commit. See
    /// [`crate::preauth_budget`] for exactly what these units measure — and,
    /// importantly, what they do not.
    pub preauth_limits: crate::preauth_budget::PreAuthLimits,
    // -- Unproven-peer bounds (#2116) -----------------------------------------
    /// Ceilings on peers that have connected but not yet proven chain identity
    /// by publishing a matching genesis handshake. See
    /// [`crate::handshake_guard`] for why these bound a gossipsub-mediated
    /// handshake rather than a byte-level one.
    pub handshake_limits: crate::handshake_guard::HandshakeLimits,

    // -- Connection-flood DoS resistance (#2037) ------------------------------
    /// Hard cap on the number of established **inbound** connections. Enforced
    /// by libp2p's `connection_limits::Behaviour`. `None` disables the cap.
    /// Parsed from `WRFCOIN_P2P_MAX_INCOMING_CONNECTIONS` (default: 128).
    pub max_established_incoming: Option<u32>,

    /// Hard cap on **pending inbound** connections (accepted sockets still
    /// negotiating the transport / Noise upgrade). Rejecting here makes a
    /// connection flood cheap to shed — the handshake cost is never paid.
    /// `None` disables the cap. Parsed from `WRFCOIN_P2P_MAX_PENDING_INCOMING`
    /// (default: 32).
    pub max_pending_incoming: Option<u32>,

    /// Maximum number of simultaneous connections to any single peer. A small
    /// value bounds per-peer resource use while still allowing the transient
    /// dual TCP+QUIC / relay-upgrade connections this node opens. `None`
    /// disables the cap. Parsed from `WRFCOIN_P2P_MAX_ESTABLISHED_PER_PEER`
    /// (default: 4).
    pub max_established_per_peer: Option<u32>,

    /// Hard cap on the **total** number of established connections (inbound +
    /// outbound). `None` disables the cap. Parsed from
    /// `WRFCOIN_P2P_MAX_CONNECTIONS` (default: 256).
    pub max_established_total: Option<u32>,

    // -- NAT traversal ------------------------------------------------------
    /// Known relay node addresses for NAT'd operators.
    /// Parsed from `WRFCOIN_P2P_RELAY_ADDRESSES` (comma-separated multiaddrs).
    pub relay_addresses: Vec<Multiaddr>,

    /// Operator-specified external address for NAT/Docker deployments.
    /// Parsed from `WRFCOIN_P2P_EXTERNAL_ADDR`.
    pub external_address: Option<Multiaddr>,

    /// QUIC listen addresses for opt-in stream-sync transport.
    /// Parsed from `WRFCOIN_P2P_QUIC_ENABLED` and `WRFCOIN_P2P_QUIC_PORT`.
    pub quic_listen_addresses: Vec<Multiaddr>,

    /// Operator-specified external QUIC address for NAT/Docker deployments.
    /// Parsed from `WRFCOIN_P2P_QUIC_EXTERNAL_ADDR`.
    pub quic_external_address: Option<Multiaddr>,

    /// Restrict AutoNAT probes to globally-routable IPs.
    /// Default: `true`. Set `false` for single-host Docker testnets.
    pub only_global_ips: bool,

    // -- Genesis handshake ----------------------------------------------------
    /// Network identifier sent in the genesis handshake (e.g. "mainnet", "testnet").
    pub network_id: String,

    /// Chain identifier sent in the genesis handshake (e.g. "wrfcoin-main").
    pub chain_id: String,

    /// Hex-encoded SHA-256 hash of the genesis block (64 hex chars).
    /// Peers whose genesis hash differs from ours will be disconnected.
    pub genesis_hash: String,

    // -- Bootstrap auto-redial (#1668) ---------------------------------------
    /// Base interval between redial attempts for a disconnected bootstrap peer.
    /// A jittered exponential backoff (factor 2x, cap = `bootstrap_redial_max_backoff`)
    /// is applied on consecutive failures, then reset on successful (re)connection.
    /// Parsed from `WRFCOIN_P2P_BOOTSTRAP_REDIAL_INTERVAL_SECS` (default: 30).
    pub bootstrap_redial_interval: Duration,

    /// Maximum backoff between redial attempts.
    /// Parsed from `WRFCOIN_P2P_BOOTSTRAP_REDIAL_MAX_BACKOFF_SECS` (default: 300).
    pub bootstrap_redial_max_backoff: Duration,

    /// What this node is for (#2120). Defaults to [`NodeRole::Public`].
    pub role: NodeRole,

    /// Purpose-separated interface addresses (#2120).
    pub interfaces: InterfaceBinding,

    /// Peers permitted to open inbound consensus connections to a [`NodeRole::Validator`].
    ///
    /// `None` means "no allowlist configured". For a validator that is a REFUSAL, not
    /// "allow everyone" — and so is `Some(empty)`. Absence and emptiness are different
    /// facts and neither may silently mean "unrestricted"; that confusion is how a gate
    /// ends up open while looking configured.
    pub sentry_allowlist: Option<Vec<PeerId>>,
}

impl Default for P2pConfig {
    fn default() -> Self {
        let quic_enabled = env_bool("WRFCOIN_P2P_QUIC_ENABLED", false);
        let tcp_port = env_u16("WRFCOIN_P2P_PORT", 8555);
        let quic_port = env_u16("WRFCOIN_P2P_QUIC_PORT", tcp_port);
        Self {
            listen_addresses: vec![
                "/ip4/0.0.0.0/tcp/8555"
                    .parse()
                    .expect("hard-coded multiaddr is valid"),
            ],
            bootstrap_peers: Vec::new(),
            mesh_n: env_usize("WRFCOIN_GOSSIPSUB_MESH_N", 4),
            mesh_n_low: env_usize("WRFCOIN_GOSSIPSUB_MESH_N_LOW", 3),
            mesh_n_high: env_usize("WRFCOIN_GOSSIPSUB_MESH_N_HIGH", 8),
            mesh_outbound_min: env_usize("WRFCOIN_GOSSIPSUB_MESH_OUTBOUND_MIN", 2),
            heartbeat_interval: Duration::from_secs(1),
            max_transmit_size: 1_048_576, // 1 MiB
            inline_data_max: env_usize("WRFCOIN_INLINE_DATA_MAX", 65_536), // 64 KiB
            flood_publish: env_bool("WRFCOIN_GOSSIPSUB_FLOOD_PUBLISH", true),
            kademlia_protocol: "/wrfcoin/kad/1.0.0".to_string(),
            command_buffer: 256,
            event_buffer: 256,
            inbound_rate_limit_bytes: env_u64(
                "WRFCOIN_P2P_INBOUND_RATE_LIMIT_BYTES",
                8 * 1024 * 1024,
            ),
            inbound_rate_limit_window: Duration::from_secs(env_u64(
                "WRFCOIN_P2P_INBOUND_RATE_LIMIT_WINDOW_SECS",
                10,
            )),
            dial_timeout: Duration::from_secs(10),
            idle_connection_timeout: Duration::from_secs(env_u64(
                "WRFCOIN_IDLE_CONNECTION_TIMEOUT_SECS",
                300,
            )),
            max_peers_per_subnet: env_usize("WRFCOIN_MAX_PEERS_PER_SUBNET", 2),
            outbound_diversity: crate::slots::OutboundDiversityPolicy {
                max_per_ipv4_subnet: env_u32("WRFCOIN_P2P_OUTBOUND_MAX_PER_SUBNET", 3),
                max_per_discovery_source: env_u32("WRFCOIN_P2P_OUTBOUND_MAX_PER_SOURCE", 8),
                min_distinct_subnets: env_u32("WRFCOIN_P2P_OUTBOUND_MIN_SUBNETS", 2),
                // #2114: bootstrap peers are exempt only via explicit
                // configuration, so this defaults off.
                exempt_bootstrap: env_bool("WRFCOIN_P2P_OUTBOUND_EXEMPT_BOOTSTRAP", false),
                // Observe-only by default; see `OutboundDiversityPolicy::enforce`.
                enforce: env_bool("WRFCOIN_P2P_OUTBOUND_ENFORCE_DIVERSITY", false),
            },
            connection_quota: crate::quota::ConnectionLimits {
                max_per_ip: env_u32("WRFCOIN_P2P_MAX_CONNS_PER_IP", 16),
                max_per_ipv4_subnet: env_u32("WRFCOIN_P2P_MAX_CONNS_PER_IPV4_SUBNET", 128),
                max_per_ipv6_subnet: env_u32("WRFCOIN_P2P_MAX_CONNS_PER_IPV6_SUBNET", 128),
            },
            preauth_limits: crate::preauth_budget::PreAuthLimits {
                max_bytes_total: env_usize("WRFCOIN_P2P_PREAUTH_MAX_BYTES_TOTAL", 64 * 1024 * 1024),
                max_verify_ops_per_window: env_u32("WRFCOIN_P2P_PREAUTH_MAX_VERIFY_PER_SEC", 64),
                verify_window: Duration::from_secs(1),
            },
            handshake_limits: crate::handshake_guard::HandshakeLimits {
                handshake_timeout: Duration::from_secs(env_u64(
                    "WRFCOIN_P2P_HANDSHAKE_TIMEOUT_SECS",
                    30,
                )),
                max_concurrent_per_ip: env_u32("WRFCOIN_P2P_MAX_CONCURRENT_HANDSHAKES_PER_IP", 4),
                // Observe-only by default; see `HandshakeLimits::enforce`.
                enforce: env_bool("WRFCOIN_P2P_ENFORCE_HANDSHAKE_LIMITS", false),
            },
            max_established_incoming: env_conn_limit(
                "WRFCOIN_P2P_MAX_INCOMING_CONNECTIONS",
                Some(128),
            ),
            max_pending_incoming: env_conn_limit("WRFCOIN_P2P_MAX_PENDING_INCOMING", Some(32)),
            max_established_per_peer: env_conn_limit(
                "WRFCOIN_P2P_MAX_ESTABLISHED_PER_PEER",
                Some(4),
            ),
            max_established_total: env_conn_limit("WRFCOIN_P2P_MAX_CONNECTIONS", Some(256)),
            relay_addresses: Vec::new(),
            external_address: env_string("WRFCOIN_P2P_EXTERNAL_ADDR")
                .and_then(|s| s.parse::<Multiaddr>().ok()),
            quic_listen_addresses: if quic_enabled {
                vec![
                    format!("/ip4/0.0.0.0/udp/{quic_port}/quic-v1")
                        .parse()
                        .expect("hard-coded QUIC multiaddr is valid"),
                ]
            } else {
                Vec::new()
            },
            quic_external_address: env_string("WRFCOIN_P2P_QUIC_EXTERNAL_ADDR")
                .and_then(|s| s.parse::<Multiaddr>().ok()),
            only_global_ips: env_bool("WRFCOIN_AUTONAT_ONLY_GLOBAL_IPS", true),
            network_id: std::env::var("WRFCOIN_NETWORK_ID")
                .unwrap_or_else(|_| "mainnet".to_string()),
            chain_id: std::env::var("WRFCOIN_CHAIN_ID")
                .unwrap_or_else(|_| "wrfcoin-main".to_string()),
            genesis_hash: resolve_genesis_hash(),
            bootstrap_redial_interval: Duration::from_secs(env_u64(
                "WRFCOIN_P2P_BOOTSTRAP_REDIAL_INTERVAL_SECS",
                30,
            )),
            bootstrap_redial_max_backoff: Duration::from_secs(env_u64(
                "WRFCOIN_P2P_BOOTSTRAP_REDIAL_MAX_BACKOFF_SECS",
                300,
            )),
            // #2120: Public + no interfaces + no allowlist == today's behaviour
            // exactly. Roles are opt-in; see NodeRole's docs for why the default
            // cannot be Validator.
            role: NodeRole::default(),
            interfaces: InterfaceBinding::default(),
            sentry_allowlist: None,
        }
    }
}

impl P2pConfig {
    /// Create a new configuration with only the listen address overridden.
    pub fn with_listen(addr: Multiaddr) -> Self {
        Self {
            listen_addresses: vec![addr],
            ..Self::default()
        }
    }

    /// Builder-style setter for bootstrap peers.
    #[must_use]
    pub fn set_bootstrap_peers(mut self, peers: Vec<Multiaddr>) -> Self {
        self.bootstrap_peers = peers;
        self
    }

    /// Builder-style setter for QUIC listen addresses.
    #[must_use]
    pub fn set_quic_listen_addresses(mut self, addrs: Vec<Multiaddr>) -> Self {
        self.quic_listen_addresses = addrs;
        self
    }

    /// The inbound/outbound slot split derived from the connection ceilings
    /// (#2114).
    pub fn peer_slots(&self) -> Result<crate::slots::PeerSlots, crate::slots::SlotConfigError> {
        crate::slots::PeerSlots::from_limits(
            self.max_established_total,
            self.max_established_incoming,
        )
    }


    /// Interface-separation rules (#2120). A **hard refusal**, never a warning.
    ///
    /// Called first from [`P2pConfig::validate`], which `P2pNode::start_with_keypair` runs
    /// before any socket or swarm state exists — so a misconfigured validator fails to
    /// start rather than coming up exposed. A warning here would be useless: the whole
    /// point is that nobody reads startup logs on the node that is already listening.
    ///
    /// Scope note: this validates what the p2p layer can see. Deployment-level bindings
    /// that live in Compose rather than in this struct are covered by
    /// `scripts/check_interface_binding.py`, which is the pre-flight half of the same rule.
    pub fn validate_interface_separation(&self) -> crate::error::P2pResult<()> {
        use crate::error::P2pError;

        // Metrics is loopback-only for EVERY role. Verified on the reference node: every
        // operator surface (node API, backend, Prometheus, Grafana, Alertmanager) is
        // already 127.0.0.1-bound, so this pins current reality rather than tightening it.
        if let Some((name, addr)) = InterfaceBinding::offending("metrics", self.interfaces.metrics) {
            return Err(P2pError::Config(format!(
                "{name} interface is bound to {addr}, which is not loopback. Metrics is \
                 loopback-only for every node role (#2120): it is an operator surface, and \
                 exposing it off-box hands an attacker the node's internal state for free. \
                 Bind it to 127.0.0.1 and reach it over an SSH tunnel or the tailnet."
            )));
        }

        if self.role != NodeRole::Validator {
            return Ok(());
        }

        for (name, addr) in [
            InterfaceBinding::offending("public_rpc", self.interfaces.public_rpc),
            InterfaceBinding::offending("admin", self.interfaces.admin),
        ]
        .into_iter()
        .flatten()
        {
            return Err(P2pError::Config(format!(
                "node role is `validator` but the {name} interface is bound to {addr}, which \
                 is not loopback (#2120). A block producer must not be directly reachable on \
                 a public interface -- that is what sentry nodes are for. Either bind {name} \
                 to loopback, or run this node as `sentry`/`public` if it is not producing \
                 blocks."
            )));
        }

        // Absence and emptiness are DIFFERENT facts, and neither may mean "allow everyone".
        // An unconfigured allowlist is the common case and must refuse; an empty one is a
        // validator that admits nobody, which is a configuration error rather than a
        // silently-open gate.
        match self.sentry_allowlist.as_deref() {
            None => Err(P2pError::Config(
                "node role is `validator` but no sentry allowlist is configured (#2120). A \
                 validator accepts inbound consensus connections only from named sentries; \
                 an absent allowlist is refused rather than treated as `allow all`, because \
                 `allow all` is exactly the exposure the role exists to prevent."
                    .to_string(),
            )),
            Some([]) => Err(P2pError::Config(
                "node role is `validator` and the sentry allowlist is EMPTY (#2120). That is \
                 not the same as absent: it names zero permitted sentries, so no peer could \
                 ever connect. Configure the sentry peer IDs, or change the role."
                    .to_string(),
            )),
            Some(_) => Ok(()),
        }
    }

    /// Validate that the configuration is internally consistent.
    pub fn validate(&self) -> crate::error::P2pResult<()> {
        self.validate_interface_separation()?;
        if self.listen_addresses.is_empty() && self.quic_listen_addresses.is_empty() {
            return Err(crate::error::P2pError::Config(
                "at least one TCP or QUIC listen address is required".into(),
            ));
        }
        if self.mesh_n_low > self.mesh_n {
            return Err(crate::error::P2pError::Config(
                "mesh_n_low must be <= mesh_n".into(),
            ));
        }
        if self.mesh_n > self.mesh_n_high {
            return Err(crate::error::P2pError::Config(
                "mesh_n must be <= mesh_n_high".into(),
            ));
        }
        if self.mesh_outbound_min > self.mesh_n_low {
            return Err(crate::error::P2pError::Config(
                "mesh_outbound_min must be <= mesh_n_low".into(),
            ));
        }
        // #2114: the reserved-outbound-slot invariant. Without this, setting the
        // inbound ceiling to or above the total silently dissolves the
        // reservation that makes an inbound flood unable to displace the peers
        // we chose — with no error and no failing test anywhere.
        self.peer_slots()
            .map_err(|e| crate::error::P2pError::Config(e.to_string()))?;

        if self.max_transmit_size == 0 {
            return Err(crate::error::P2pError::Config(
                "max_transmit_size must be > 0".into(),
            ));
        }
        if self.inline_data_max >= self.max_transmit_size {
            return Err(crate::error::P2pError::Config(
                "inline_data_max must be < max_transmit_size".into(),
            ));
        }

        // Advisory: warn if listening on all interfaces without an external address.
        if self.external_address.is_none() && self.quic_external_address.is_none() {
            for listen in self
                .listen_addresses
                .iter()
                .chain(self.quic_listen_addresses.iter())
            {
                let is_wildcard = listen.iter().any(
                    |p| matches!(p, libp2p::multiaddr::Protocol::Ip4(ip) if ip.is_unspecified()),
                );
                if is_wildcard {
                    log::warn!(
                        "Listening on {listen} without an explicit external address. \
                         Nodes behind NAT/Docker may advertise unreachable TCP or QUIC addresses."
                    );
                    break;
                }
            }
        }

        Ok(())
    }
}

/// Returns `true` if the multiaddr contains an IPv4 address in 172.16.0.0/12
/// (Docker bridge range: 172.16.x.x -- 172.31.x.x).
pub fn is_docker_bridge_addr(addr: &Multiaddr) -> bool {
    use libp2p::multiaddr::Protocol;
    for proto in addr.iter() {
        if let Protocol::Ip4(ip) = proto {
            let octets = ip.octets();
            return octets[0] == 172 && (octets[1] & 0xF0) == 16;
        }
    }
    false
}

/// Returns `true` if a peer-advertised multiaddr can never be dialable from
/// another machine: loopback, unspecified (wildcard), or a docker-bridge
/// address (#2819).
///
/// Deliberately narrow: RFC1918 LAN addresses (192.168/16, 10/8) and CGNAT
/// tailnet addresses (100.64/10) are NOT rejected — on this testnet they are
/// exactly the addresses honest peers are reachable at. Only address classes
/// that are unroutable from every other host are dropped.
pub fn is_undialable_advertised_addr(addr: &Multiaddr) -> bool {
    is_loopback_addr(addr) || is_unspecified_addr(addr) || is_docker_bridge_addr(addr)
}

fn is_loopback_addr(addr: &Multiaddr) -> bool {
    use libp2p::multiaddr::Protocol;
    for proto in addr.iter() {
        match proto {
            Protocol::Ip4(ip) => return ip.is_loopback(),
            Protocol::Ip6(ip) => return ip.is_loopback(),
            _ => {},
        }
    }
    false
}

fn is_unspecified_addr(addr: &Multiaddr) -> bool {
    use libp2p::multiaddr::Protocol;
    for proto in addr.iter() {
        match proto {
            Protocol::Ip4(ip) => return ip.is_unspecified(),
            Protocol::Ip6(ip) => return ip.is_unspecified(),
            _ => {},
        }
    }
    false
}

/// Should an identify-advertised address be ingested as a dial candidate?
///
/// `observed_addr` is how the advertising peer sees US. A peer that observes
/// us at a loopback address shares our host, so its loopback addresses are
/// genuinely dialable from here (this is every localhost multi-node cluster,
/// including the in-process test topologies); likewise a peer that observes
/// us at a docker-bridge address shares our docker network. Outside that
/// same-scope exception, loopback/unspecified/docker-bridge advertisements
/// are unroutable junk and must not reach the routing table (#2819).
pub fn should_ingest_advertised_addr(addr: &Multiaddr, observed_addr: &Multiaddr) -> bool {
    if !is_undialable_advertised_addr(addr) {
        return true;
    }
    if is_unspecified_addr(addr) {
        return false;
    }
    if is_loopback_addr(addr) {
        return is_loopback_addr(observed_addr);
    }
    if is_docker_bridge_addr(addr) {
        return is_docker_bridge_addr(observed_addr);
    }
    false
}

impl P2pConfig {
    /// #2120: read `WRFCOIN_P2P_NODE_ROLE` and `WRFCOIN_P2P_SENTRY_ALLOWLIST` into
    /// `role` / `sentry_allowlist`. Unset variables leave the fields as they are.
    /// Call before `validate` (which `P2pNode::start` runs) so a validator with a
    /// bad or missing allowlist refuses to start rather than silently running as
    /// `public`.
    pub fn apply_role_env(&mut self) -> Result<(), String> {
        let role = std::env::var(P2P_NODE_ROLE_ENV).ok();
        let allowlist = std::env::var(P2P_SENTRY_ALLOWLIST_ENV).ok();
        if role.is_none() && allowlist.is_none() {
            return Ok(());
        }
        let (parsed_role, parsed_list) = parse_role_settings(role.as_deref(), allowlist.as_deref())?;
        if role.is_some() {
            self.role = parsed_role;
        }
        if parsed_list.is_some() {
            self.sentry_allowlist = parsed_list;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{InterfaceBinding, NodeRole};

    fn addr(s: &str) -> std::net::SocketAddr { s.parse().expect("test addr") }
    fn sentry() -> libp2p::PeerId { libp2p::identity::Keypair::generate_ed25519().public().to_peer_id() }

    #[test]
    fn validator_keeps_inbound_only_from_allowlisted_sentries_2120() {
        let (a, b, stranger) = (sentry(), sentry(), sentry());
        let list = vec![a, b];
        assert!(super::inbound_consensus_allowed(NodeRole::Validator, Some(&list), false, &a));
        assert!(super::inbound_consensus_allowed(NodeRole::Validator, Some(&list), false, &b));
        assert!(!super::inbound_consensus_allowed(NodeRole::Validator, Some(&list), false, &stranger));
        // Outbound: the validator dialed it (its sentry, or anything it chose).
        assert!(super::inbound_consensus_allowed(NodeRole::Validator, Some(&list), true, &stranger));
        // No allowlist on a validator is "nobody", never "anybody".
        assert!(!super::inbound_consensus_allowed(NodeRole::Validator, None, false, &a));
        assert!(!super::inbound_consensus_allowed(NodeRole::Validator, Some(&[]), false, &a));
    }

    #[test]
    fn sentries_and_public_nodes_accept_any_inbound_peer_2120() {
        let stranger = sentry();
        for role in [NodeRole::Sentry, NodeRole::Public] {
            assert!(super::inbound_consensus_allowed(role, None, false, &stranger), "{role:?}");
            assert!(super::inbound_consensus_allowed(role, Some(&[]), false, &stranger), "{role:?}");
        }
    }

    // --- #2120 interface separation ---------------------------------------------------

    /// The property that protects the running testnet: an unconfigured node is unchanged.
    #[test]
    fn default_config_is_public_and_still_validates() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        let cfg = P2pConfig::default();
        assert_eq!(cfg.role, NodeRole::Public);
        assert!(cfg.sentry_allowlist.is_none());
        assert!(
            cfg.validate_interface_separation().is_ok(),
            "a node that has never heard of roles must keep starting"
        );
    }

    #[test]
    fn metrics_off_loopback_is_refused_for_every_role() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        for role in [NodeRole::Public, NodeRole::Sentry, NodeRole::Validator] {
            let mut cfg = P2pConfig::default();
            cfg.role = role;
            cfg.interfaces = InterfaceBinding {
                metrics: Some(addr("0.0.0.0:9090")),
                ..InterfaceBinding::default()
            };
            cfg.sentry_allowlist = Some(vec![sentry()]);
            let err = cfg
                .validate_interface_separation()
                .expect_err("metrics is loopback-only for every role");
            assert!(format!("{err:?}").contains("metrics"), "got {err:?}");
        }
    }

    #[test]
    fn validator_refuses_public_rpc_or_admin_off_loopback() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        for (label, iface) in [
            ("public_rpc", InterfaceBinding { public_rpc: Some(addr("0.0.0.0:8556")), ..Default::default() }),
            ("admin",      InterfaceBinding { admin:      Some(addr("0.0.0.0:9999")), ..Default::default() }),
        ] {
            let mut cfg = P2pConfig::default();
            cfg.role = NodeRole::Validator;
            cfg.interfaces = iface;
            cfg.sentry_allowlist = Some(vec![sentry()]);
            let err = cfg
                .validate_interface_separation()
                .expect_err(&format!("{label} bound off-loopback must refuse startup"));
            let msg = format!("{err:?}");
            assert!(msg.contains(label), "refusal must name {label}; got {msg}");
            assert!(
                msg.contains("validator"),
                "refusal must name the role that makes it a violation; got {msg}"
            );
        }
    }

    /// Absence and emptiness are different facts, and NEITHER may mean "allow everyone".
    #[test]
    fn validator_refuses_absent_and_empty_sentry_allowlist() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        let mut cfg = P2pConfig::default();
        cfg.role = NodeRole::Validator;

        cfg.sentry_allowlist = None;
        let absent = format!("{:?}", cfg.validate_interface_separation().unwrap_err());
        assert!(absent.contains("no sentry allowlist"), "got {absent}");

        cfg.sentry_allowlist = Some(Vec::new());
        let empty = format!("{:?}", cfg.validate_interface_separation().unwrap_err());
        assert!(empty.contains("EMPTY"), "got {empty}");

        assert_ne!(absent, empty, "absent and empty must not report identically");
    }

    #[test]
    fn correctly_configured_validator_starts() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        let mut cfg = P2pConfig::default();
        cfg.role = NodeRole::Validator;
        cfg.interfaces = InterfaceBinding {
            public_rpc: Some(addr("127.0.0.1:8556")),
            metrics: Some(addr("127.0.0.1:9090")),
            admin: None,
        };
        cfg.sentry_allowlist = Some(vec![sentry(), sentry()]);
        assert!(cfg.validate_interface_separation().is_ok());
    }

    /// Non-validators are not subjected to the allowlist rule.
    #[test]
    fn sentry_and_public_do_not_need_an_allowlist() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        for role in [NodeRole::Sentry, NodeRole::Public] {
            let mut cfg = P2pConfig::default();
            cfg.role = role;
            cfg.interfaces = InterfaceBinding {
                public_rpc: Some(addr("0.0.0.0:8556")),
                ..Default::default()
            };
            assert!(cfg.validate_interface_separation().is_ok(), "{role:?}");
        }
    }

    /// The wiring itself, not just the rule.
    ///
    /// Every other test here calls `validate_interface_separation` directly, so they would
    /// all still pass if it were never called from `validate()` — the function
    /// `P2pNode::start_with_keypair` actually runs. Found by RED control: unwiring the call
    /// left 34/34 green. A rule nothing invokes is decoration, so this test goes through
    /// the real entry point.
    #[test]
    fn validate_invokes_the_interface_separation_rule() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        let mut cfg = P2pConfig::default();
        cfg.role = NodeRole::Validator;
        cfg.sentry_allowlist = None; // refused by the separation rule, nothing else

        let err = cfg
            .validate()
            .expect_err("P2pConfig::validate must run the #2120 separation rule");
        assert!(
            format!("{err:?}").contains("sentry allowlist"),
            "validate() must surface the separation refusal, got {err:?}"
        );
    }

    #[test]
    fn role_parsing_refuses_unknown_input() {
        assert_eq!(NodeRole::parse("validator"), Some(NodeRole::Validator));
        assert_eq!(NodeRole::parse("  Sentry "), Some(NodeRole::Sentry));
        assert_eq!(NodeRole::parse("producer"), None, "unknown role must not be guessed");
    }

    use super::{
        P2pConfig, TESTNET_GENESIS_HASH, ZERO_GENESIS_HASH, expected_genesis_hash,
        is_docker_bridge_addr, resolve_genesis_hash,
    };
    use libp2p::Multiaddr;
    use std::sync::Mutex;

    pub(super) static ENV_MUTEX: Mutex<()> = Mutex::new(());
    const GENESIS_HASH_ENV: &str = "WRFCOIN_GENESIS_HASH";
    const NETWORK_ID_ENV: &str = "WRFCOIN_NETWORK_ID";
    const FLOOD_PUBLISH_ENV: &str = "WRFCOIN_GOSSIPSUB_FLOOD_PUBLISH";
    const EXTERNAL_ADDR_ENV: &str = "WRFCOIN_P2P_EXTERNAL_ADDR";
    const TCP_PORT_ENV: &str = "WRFCOIN_P2P_PORT";
    const QUIC_ENABLED_ENV: &str = "WRFCOIN_P2P_QUIC_ENABLED";
    const QUIC_PORT_ENV: &str = "WRFCOIN_P2P_QUIC_PORT";
    const QUIC_EXTERNAL_ADDR_ENV: &str = "WRFCOIN_P2P_QUIC_EXTERNAL_ADDR";
    const ONLY_GLOBAL_IPS_ENV: &str = "WRFCOIN_AUTONAT_ONLY_GLOBAL_IPS";
    const MAX_INCOMING_ENV: &str = "WRFCOIN_P2P_MAX_INCOMING_CONNECTIONS";
    const MAX_PENDING_ENV: &str = "WRFCOIN_P2P_MAX_PENDING_INCOMING";
    const MAX_PER_PEER_ENV: &str = "WRFCOIN_P2P_MAX_ESTABLISHED_PER_PEER";
    const MAX_TOTAL_ENV: &str = "WRFCOIN_P2P_MAX_CONNECTIONS";

    fn clear_conn_limit_env() {
        for var in [MAX_INCOMING_ENV, MAX_PENDING_ENV, MAX_PER_PEER_ENV, MAX_TOTAL_ENV] {
            std::env::remove_var(var);
        }
    }

    #[test]
    fn genesis_hash_explicit_env_takes_precedence() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::set_var(NETWORK_ID_ENV, "testnet");
        std::env::set_var(GENESIS_HASH_ENV, "deadbeefdeadbeef");

        assert_eq!(resolve_genesis_hash(), "deadbeefdeadbeef");

        std::env::remove_var(GENESIS_HASH_ENV);
        std::env::remove_var(NETWORK_ID_ENV);
    }

    #[test]
    fn genesis_hash_defaults_to_canonical_testnet_value_when_unset() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::remove_var(GENESIS_HASH_ENV);
        std::env::set_var(NETWORK_ID_ENV, "testnet");

        assert_eq!(resolve_genesis_hash(), TESTNET_GENESIS_HASH);
        // Must not be the launch-unsafe placeholder.
        assert_ne!(resolve_genesis_hash(), ZERO_GENESIS_HASH);

        std::env::remove_var(NETWORK_ID_ENV);
    }

    #[test]
    fn genesis_hash_testnet_match_is_case_insensitive() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::remove_var(GENESIS_HASH_ENV);
        std::env::set_var(NETWORK_ID_ENV, "TestNet");

        assert_eq!(resolve_genesis_hash(), TESTNET_GENESIS_HASH);

        std::env::remove_var(NETWORK_ID_ENV);
    }

    #[test]
    fn genesis_hash_falls_back_to_placeholder_off_testnet() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::remove_var(GENESIS_HASH_ENV);
        std::env::set_var(NETWORK_ID_ENV, "mainnet");

        assert_eq!(resolve_genesis_hash(), ZERO_GENESIS_HASH);

        std::env::remove_var(NETWORK_ID_ENV);
    }

    // #2035: the boot-assertion source of truth. `expected_genesis_hash` must
    // return `Some` for a committing expectation (explicit hash or testnet
    // default) and `None` for the non-committing zero placeholder, so the
    // launch_api boot check only fires when there is a real chain identity to
    // enforce.
    #[test]
    fn expected_genesis_hash_reflects_committing_expectation_2035() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");

        // Testnet default → Some(pinned).
        std::env::remove_var(GENESIS_HASH_ENV);
        std::env::set_var(NETWORK_ID_ENV, "testnet");
        assert_eq!(
            expected_genesis_hash().as_deref(),
            Some(TESTNET_GENESIS_HASH)
        );

        // Explicit override → Some(explicit).
        std::env::set_var(GENESIS_HASH_ENV, "deadbeefdeadbeef");
        assert_eq!(expected_genesis_hash().as_deref(), Some("deadbeefdeadbeef"));

        // Off-testnet with nothing pinned → None (no expectation to assert).
        std::env::remove_var(GENESIS_HASH_ENV);
        std::env::set_var(NETWORK_ID_ENV, "mainnet");
        assert_eq!(expected_genesis_hash(), None);
        // Sanity: the resolver itself still yields the placeholder here.
        assert_eq!(resolve_genesis_hash(), ZERO_GENESIS_HASH);

        std::env::remove_var(NETWORK_ID_ENV);
    }

    #[test]
    fn genesis_hash_canonical_value_is_32_byte_hex() {
        // 32 bytes => 64 hex chars; guards against accidental truncation.
        assert_eq!(TESTNET_GENESIS_HASH.len(), 64);
        assert!(TESTNET_GENESIS_HASH.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn flood_publish_defaults_to_true() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::remove_var(FLOOD_PUBLISH_ENV);

        let config = P2pConfig::default();

        assert!(config.flood_publish);
    }

    #[test]
    fn flood_publish_env_override_enables_flooding() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::set_var(FLOOD_PUBLISH_ENV, "true");

        let config = P2pConfig::default();

        assert!(config.flood_publish);
        std::env::remove_var(FLOOD_PUBLISH_ENV);
    }

    #[test]
    fn flood_publish_env_override_disables_flooding() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::set_var(FLOOD_PUBLISH_ENV, "false");

        let config = P2pConfig::default();

        assert!(!config.flood_publish);
        std::env::remove_var(FLOOD_PUBLISH_ENV);
    }

    #[test]
    fn invalid_flood_publish_env_falls_back_to_default() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::set_var(FLOOD_PUBLISH_ENV, "definitely-not-a-bool");

        let config = P2pConfig::default();

        assert!(config.flood_publish); // default is now true
        std::env::remove_var(FLOOD_PUBLISH_ENV);
    }

    // -- Docker bridge address detection ----------------------------------------

    #[test]
    fn docker_bridge_addr_detected() {
        let docker: Multiaddr = "/ip4/172.17.0.2/tcp/8555".parse().unwrap();
        assert!(is_docker_bridge_addr(&docker));

        let docker_custom: Multiaddr = "/ip4/172.20.0.5/tcp/8555".parse().unwrap();
        assert!(is_docker_bridge_addr(&docker_custom));

        let docker_upper: Multiaddr = "/ip4/172.31.255.255/tcp/8555".parse().unwrap();
        assert!(is_docker_bridge_addr(&docker_upper));
    }

    #[test]
    fn non_docker_addrs_not_flagged() {
        let public: Multiaddr = "/ip4/1.2.3.4/tcp/8555".parse().unwrap();
        assert!(!is_docker_bridge_addr(&public));

        let private_10: Multiaddr = "/ip4/10.0.0.1/tcp/8555".parse().unwrap();
        assert!(!is_docker_bridge_addr(&private_10));  // secret-leak-ok: private IP range constant, not a key

        let private_192: Multiaddr = "/ip4/192.168.1.1/tcp/8555".parse().unwrap();
        assert!(!is_docker_bridge_addr(&private_192));  // secret-leak-ok: private IP range constant, not a key

        let loopback: Multiaddr = "/ip4/127.0.0.1/tcp/8555".parse().unwrap();
        assert!(!is_docker_bridge_addr(&loopback));

        // 172.15.x.x is NOT in 172.16.0.0/12
        let near_miss: Multiaddr = "/ip4/172.15.0.1/tcp/8555".parse().unwrap();
        assert!(!is_docker_bridge_addr(&near_miss));

        // 172.32.x.x is NOT in 172.16.0.0/12
        let above_range: Multiaddr = "/ip4/172.32.0.1/tcp/8555".parse().unwrap();
        assert!(!is_docker_bridge_addr(&above_range));
    }

    #[test]
    fn docker_bridge_non_ipv4_returns_false() {
        let dns: Multiaddr = "/dns4/example.com/tcp/8555".parse().unwrap();
        assert!(!is_docker_bridge_addr(&dns));
    }

    // -- Undialable advertised addresses (#2819) --------------------------------

    #[test]
    fn undialable_rejects_loopback_unspecified_and_bridge() {
        use super::is_undialable_advertised_addr;
        for raw in [
            "/ip4/127.0.0.1/tcp/9000",
            "/ip4/0.0.0.0/tcp/9000",
            "/ip4/172.19.0.5/tcp/9000",   // secret-leak-ok: docker bridge constant, not a key
            "/ip4/172.18.0.3/tcp/9001",   // secret-leak-ok: docker bridge constant, not a key
            "/ip6/::1/tcp/9000",
            "/ip6/::/tcp/9000",
        ] {
            let addr: Multiaddr = raw.parse().unwrap();
            assert!(is_undialable_advertised_addr(&addr), "must reject {raw}");
        }
    }

    #[test]
    fn same_scope_peers_keep_their_scope_but_nothing_else() {
        use super::should_ingest_advertised_addr;
        let parse = |s: &str| -> Multiaddr { s.parse().unwrap() };
        let loopback = parse("/ip4/127.0.0.1/tcp/9000");
        let bridge = parse("/ip4/172.19.0.5/tcp/9000"); // secret-leak-ok: docker bridge constant, not a key
        let unspecified = parse("/ip4/0.0.0.0/tcp/9000");
        let tailnet = parse("/ip4/100.64.0.1/tcp/9000"); // secret-leak-ok: CGNAT range constant, not a key
        let remote = parse("/ip4/203.0.113.9/tcp/4001");

        // Peer on the same host (observes us at loopback): its loopback addrs
        // are dialable, its bridge addrs are not.
        assert!(should_ingest_advertised_addr(&loopback, &loopback));
        assert!(!should_ingest_advertised_addr(&bridge, &loopback));

        // Peer on the same docker network: bridge yes, loopback no.
        assert!(should_ingest_advertised_addr(&bridge, &bridge));
        assert!(!should_ingest_advertised_addr(&loopback, &bridge));

        // Remote peer: no junk class survives; routable addrs always do.
        for junk in [&loopback, &bridge, &unspecified] {
            assert!(!should_ingest_advertised_addr(junk, &remote));
            assert!(!should_ingest_advertised_addr(junk, &tailnet));
        }
        assert!(should_ingest_advertised_addr(&tailnet, &remote));
        assert!(should_ingest_advertised_addr(&remote, &loopback));

        // Unspecified is never dialable, whatever the scope.
        assert!(!should_ingest_advertised_addr(&unspecified, &loopback));
        assert!(!should_ingest_advertised_addr(&unspecified, &unspecified));
    }

    #[test]
    fn undialable_keeps_lan_tailnet_public_and_dns() {
        use super::is_undialable_advertised_addr;
        for raw in [
            // Documentation fixtures plus RFC1918 and CGNAT address classes.
            // Rejecting the latter would partition private peer meshes (#2819).
            "/ip4/192.0.2.110/tcp/9000", // TEST-NET-1 fixture
            "/ip4/100.64.0.1/tcp/9000",  // secret-leak-ok: CGNAT range constant, not a key
            "/ip4/10.1.2.3/tcp/9000",       // secret-leak-ok: private IP range constant, not a key
            "/ip4/203.0.113.10/tcp/9000",
            "/dns4/example.com/tcp/9000",
        ] {
            let addr: Multiaddr = raw.parse().unwrap();
            assert!(!is_undialable_advertised_addr(&addr), "must keep {raw}");
        }
    }

    // -- External address config ------------------------------------------------

    #[test]
    fn external_address_parsed_from_env() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::set_var(EXTERNAL_ADDR_ENV, "/ip4/203.0.113.10/tcp/8555");

        let config = P2pConfig::default();

        assert!(config.external_address.is_some());
        assert_eq!(
            config.external_address.unwrap().to_string(),
            "/ip4/203.0.113.10/tcp/8555"
        );
        std::env::remove_var(EXTERNAL_ADDR_ENV);
    }

    #[test]
    fn external_address_none_when_unset() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::remove_var(EXTERNAL_ADDR_ENV);

        let config = P2pConfig::default();

        assert!(config.external_address.is_none());
    }

    #[test]
    fn external_address_none_when_invalid() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::set_var(EXTERNAL_ADDR_ENV, "not-a-multiaddr");

        let config = P2pConfig::default();

        assert!(config.external_address.is_none());
        std::env::remove_var(EXTERNAL_ADDR_ENV);
    }

    #[test]
    fn quic_disabled_by_default() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::remove_var(QUIC_ENABLED_ENV);
        std::env::remove_var(QUIC_PORT_ENV);

        let config = P2pConfig::default();

        assert!(config.quic_listen_addresses.is_empty());
    }

    #[test]
    fn quic_listen_address_parsed_from_env() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::set_var(QUIC_ENABLED_ENV, "true");
        std::env::set_var(QUIC_PORT_ENV, "9555");

        let config = P2pConfig::default();

        assert_eq!(config.quic_listen_addresses.len(), 1);
        assert_eq!(
            config.quic_listen_addresses[0].to_string(),
            "/ip4/0.0.0.0/udp/9555/quic-v1"
        );
        std::env::remove_var(QUIC_ENABLED_ENV);
        std::env::remove_var(QUIC_PORT_ENV);
    }

    #[test]
    fn quic_port_defaults_to_tcp_port() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::set_var(TCP_PORT_ENV, "9555");
        std::env::set_var(QUIC_ENABLED_ENV, "true");
        std::env::remove_var(QUIC_PORT_ENV);

        let config = P2pConfig::default();

        assert_eq!(config.quic_listen_addresses.len(), 1);
        assert_eq!(
            config.quic_listen_addresses[0].to_string(),
            "/ip4/0.0.0.0/udp/9555/quic-v1"
        );
        std::env::remove_var(TCP_PORT_ENV);
        std::env::remove_var(QUIC_ENABLED_ENV);
    }

    #[test]
    fn quic_external_address_parsed_from_env() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::set_var(QUIC_EXTERNAL_ADDR_ENV, "/ip4/203.0.113.10/udp/9555/quic-v1");

        let config = P2pConfig::default();

        assert_eq!(
            config.quic_external_address.unwrap().to_string(),
            "/ip4/203.0.113.10/udp/9555/quic-v1"
        );
        std::env::remove_var(QUIC_EXTERNAL_ADDR_ENV);
    }

    #[test]
    fn quic_only_listener_is_valid() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        let config = P2pConfig {
            listen_addresses: Vec::new(),
            quic_listen_addresses: vec!["/ip4/127.0.0.1/udp/0/quic-v1".parse().unwrap()],
            ..P2pConfig::default()
        };

        config
            .validate()
            .expect("quic-only listener should be valid");
    }

    #[test]
    fn with_listen_preserves_opt_in_quic_settings() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::set_var(QUIC_ENABLED_ENV, "true");
        std::env::set_var(QUIC_PORT_ENV, "9555");

        let config = P2pConfig::with_listen("/ip4/127.0.0.1/tcp/0".parse().unwrap());

        assert_eq!(config.listen_addresses.len(), 1);
        assert_eq!(
            config.listen_addresses[0].to_string(),
            "/ip4/127.0.0.1/tcp/0"
        );
        assert_eq!(config.quic_listen_addresses.len(), 1);
        assert_eq!(
            config.quic_listen_addresses[0].to_string(),
            "/ip4/0.0.0.0/udp/9555/quic-v1"
        );

        std::env::remove_var(QUIC_ENABLED_ENV);
        std::env::remove_var(QUIC_PORT_ENV);
    }

    // -- AutoNAT only_global_ips config -----------------------------------------

    // -- Connection-flood caps (#2037) ------------------------------------------

    #[test]
    fn connection_limits_have_documented_defaults() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_conn_limit_env();

        let config = P2pConfig::default();

        assert_eq!(config.max_established_incoming, Some(128));
        assert_eq!(config.max_pending_incoming, Some(32));
        assert_eq!(config.max_established_per_peer, Some(4));
        assert_eq!(config.max_established_total, Some(256));
    }

    #[test]
    fn connection_limits_overridden_from_env() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_conn_limit_env();
        std::env::set_var(MAX_INCOMING_ENV, "64");
        std::env::set_var(MAX_PENDING_ENV, "16");
        std::env::set_var(MAX_PER_PEER_ENV, "2");
        std::env::set_var(MAX_TOTAL_ENV, "200");

        let config = P2pConfig::default();

        assert_eq!(config.max_established_incoming, Some(64));
        assert_eq!(config.max_pending_incoming, Some(16));
        assert_eq!(config.max_established_per_peer, Some(2));
        assert_eq!(config.max_established_total, Some(200));

        clear_conn_limit_env();
    }

    #[test]
    fn connection_limit_zero_disables_the_cap() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_conn_limit_env();
        // `0` is the "unlimited" sentinel (mirrors max_peers_per_subnet).
        std::env::set_var(MAX_INCOMING_ENV, "0");
        std::env::set_var(MAX_TOTAL_ENV, "0");

        let config = P2pConfig::default();

        assert_eq!(config.max_established_incoming, None);
        assert_eq!(config.max_established_total, None);
        // Untouched knobs keep their defaults.
        assert_eq!(config.max_pending_incoming, Some(32));

        clear_conn_limit_env();
    }

    #[test]
    fn invalid_connection_limit_env_falls_back_to_default() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_conn_limit_env();
        std::env::set_var(MAX_INCOMING_ENV, "not-a-number");

        let config = P2pConfig::default();

        assert_eq!(config.max_established_incoming, Some(128));

        clear_conn_limit_env();
    }

    #[test]
    fn only_global_ips_defaults_to_true() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        std::env::remove_var(ONLY_GLOBAL_IPS_ENV);

        let config = P2pConfig::default();

        assert!(config.only_global_ips);
    }
}

#[cfg(test)]
mod slot_separation_tests {
    use super::tests::ENV_MUTEX;
    use super::*;

    /// The wiring test. `peer_slots()` having a correct rule is worth nothing
    /// if `validate()` never calls it — the #2120 failure mode, where every
    /// test invoked the rule directly and unwiring it from `validate()` left
    /// the suite fully green.
    #[test]
    fn validate_invokes_the_reserved_outbound_slot_rule() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        let mut config = P2pConfig::default();
        assert!(config.validate().is_ok(), "defaults must be valid");

        // Inbound ceiling raised to the total: the reservation is gone.
        config.max_established_incoming = config.max_established_total;

        let err = config
            .validate()
            .expect_err("validate() must refuse a config with no reserved outbound slots");
        let msg = err.to_string();
        assert!(
            msg.contains("reserved outbound"),
            "the refusal must come from the slot rule, not some other check: {msg}"
        );
    }

    #[test]
    fn shipped_defaults_reserve_half_the_budget_for_outbound() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        let config = P2pConfig::default();
        let slots = config.peer_slots().expect("defaults must reserve slots");
        assert_eq!(slots.outbound_reserved(), 128);
        assert_eq!(slots.inbound_max(), 128);
    }

    #[test]
    fn an_unbounded_inbound_ceiling_fails_validation() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        let mut config = P2pConfig::default();
        config.max_established_incoming = None; // "unlimited"
        assert!(
            config.validate().is_err(),
            "an unlimited inbound ceiling reserves nothing and must be refused"
        );
    }

    #[test]
    fn outbound_diversity_defaults_are_observe_only() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        let config = P2pConfig::default();
        assert!(!config.outbound_diversity.enforce);
        assert!(!config.outbound_diversity.exempt_bootstrap);
    }
}
