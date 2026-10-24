mod finality_events;
mod genesis_producers;
mod helpers;
#[cfg(feature = "p2p")]
mod da_challenge_producer;
#[cfg(feature = "p2p")]
mod p2p_events;
#[cfg(feature = "p2p")]
mod peer_reputation;
mod rate_limit;
mod routes;
mod slashing_submitter;
mod state;
mod stateful_invariants;
mod transition_extractor;
mod websocket;

use actix_cors::Cors;
use actix_web::{
    App, HttpServer,
    error::{InternalError, JsonPayloadError},
    http::{self, StatusCode},
    web,
};
use rustc_hash::{FxHashMap, FxHashSet};
#[cfg(feature = "zk")]
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
#[cfg(any(feature = "p2p", feature = "privacy"))]
use std::time::Duration;
use tokio::sync::RwLock;
#[cfg(feature = "zk")]
use tokio::sync::Semaphore;
#[cfg(feature = "privacy")]
use tokio::time::interval;
use wrfcoin_native_chain::blockchain::chain::Blockchain;
use wrfcoin_native_chain::bridge::safety_manager::BridgeSafetyManager;
use wrfcoin_native_chain::network_failover::{FailoverConfig, FailoverMonitor};
use wrfcoin_native_chain::bridge::types::BridgeSafetyConfig;
use wrfcoin_native_chain::config::settings::Config;
use wrfcoin_native_chain::consensus::DurableEvidenceStore;
use wrfcoin_native_chain::crypto::signature::PublicKey;
#[cfg(test)]
use wrfcoin_native_chain::crypto::signature::PrivateKey;
use wrfcoin_native_chain::mempool::pool::Mempool;
use wrfcoin_native_chain::security::api_auth::{ApiRole, AuthPrincipal, Identity};
use wrfcoin_shared_protocol::invariants::balance::SharedSupplyInvariant;
use wrfcoin_shared_protocol::invariants::finality::SharedFinalityInvariant;
use wrfcoin_shared_protocol::crypto::signer::{self as protocol_signer, BlockSigner};
use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};

#[cfg(feature = "p2p")]
use libp2p::Multiaddr;

#[cfg(feature = "p2p")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "p2p")]
use da_challenge_producer::{da_producer_config_from_env, run_da_challenge_producer};
#[cfg(feature = "p2p")]
use wrfcoin_p2p::{P2pConfig, P2pNode};

#[cfg(feature = "privacy")]
use wrfcoin_privacy::KeyImageRegistry;

#[cfg(feature = "execution")]
use wrfcoin_execution_engine::docker::DockerRunner;
#[cfg(feature = "execution")]
use wrfcoin_execution_engine::registry::default_registry;

use helpers::envelope_error;
use routes::configure_launch_routes;
use state::{AppState, MAX_JSON_PAYLOAD_BYTES, P2pStartupStatus, ProducerAuthorityBootStatus};
#[cfg(feature = "p2p")]
use state::P2pConsumerHealth;
use wrfcoin_consensus_engines::{ConsensusFactory, ConsensusType, TypedConsensusManager};

const STORAGE_ENCRYPTION_DISABLED_WARNING: &str = "Storage encryption is disabled. Data is written to disk in plaintext. \
     Set WRFCOIN_STORAGE_ENCRYPTION_KEY to enable encryption at rest, or set \
     ALLOW_UNENCRYPTED_STORAGE=true to suppress this error (development only). \
     See: https://github.com/wrfcoin/core4/issues/1027";
const EXPOSED_API_BIND_WARNING: &str = "API server binding to 0.0.0.0 and is reachable from all network interfaces. Set WRFCOIN_API_HOST=127.0.0.1 to restrict it to localhost.";

/// Fail-stop guard for the inbound P2P consumer task (#3224).
///
/// Owner decision 2026-09-18 (Option 3): a consumer panic is a **whole-process
/// fail-stop**, recovered externally by the compose `restart:` policy plus the
/// healthcheck. Shipped binaries build with `[profile.release] panic = "abort"`,
/// so a panic aborts the process (SIGABRT, exit 134) before this guard is ever
/// dropped: under abort it is a documented no-op, and no code in this process
/// observes, catches, or reports a panic.
///
/// Unwind builds (dev/test profiles) would otherwise let Tokio swallow the panic
/// and leave `/readyz` reporting a consumer that no longer exists. Dropping this
/// guard while the thread is unwinding aborts the process there too, so the panic
/// policy is the same in every profile rather than only in release.
#[cfg(feature = "p2p")]
struct AbortOnConsumerPanic;

#[cfg(feature = "p2p")]
impl Drop for AbortOnConsumerPanic {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "P2P_EVENT_CONSUMER_PANICKED: the inbound P2P consumer panicked; aborting the process (fail-stop policy, #3224). The container restart policy restarts the node."
            );
            std::process::abort();
        }
    }
}

/// Runs the inbound P2P consumer and reflects a **non-panic** end in readiness.
///
/// In-process supervision covers exactly one failure: the consumer future
/// returning, which happens when its event channel closes or the loop otherwise
/// ends without a panic. Outside graceful shutdown that makes `/readyz` not-ready
/// with a stable, non-leaking reason until the node is restarted. The receiver is
/// single-owner and cannot be rebuilt without restarting P2P, so there is no
/// in-process restart. A panic is NOT handled here: see [`AbortOnConsumerPanic`].
#[cfg(feature = "p2p")]
fn spawn_p2p_consumer_supervisor<F>(health: P2pConsumerHealth, consumer: F) -> tokio::task::JoinHandle<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let _fail_stop = AbortOnConsumerPanic;
        health.mark_running();
        consumer.await;
        health.finish(Some(
            "inbound P2P consumer exited because its event channel closed".to_string(),
        ));
        if health.blocks_readiness() {
            log::error!(
                "P2P_EVENT_CONSUMER_FAILED: the inbound P2P consumer exited because its event channel closed; readiness is now RED under the fail-stop policy. Restart the node after investigating P2P task logs."
            );
        } else {
            log::info!("Inbound P2P event consumer stopped during graceful shutdown");
        }
    })
}

/// Environment variable naming the trigger file of the #3224 panic drill.
///
/// Compiled only under the test-only `panic-drill` feature, which no shipped
/// build enables (`scripts/check_release_endpoints.py` fails any build profile or
/// release binary that carries it).
#[cfg(all(feature = "p2p", feature = "panic-drill"))]
const PANIC_DRILL_ENV: &str = "WRFCOIN_PANIC_DRILL_P2P_CONSUMER_TRIGGER";

/// Wraps the consumer so it panics, inside the supervised task, once the drill
/// trigger file appears. The file is removed first, so a restarted node does not
/// panic again. Without the environment variable the consumer runs unchanged.
#[cfg(all(feature = "p2p", feature = "panic-drill"))]
fn with_panic_drill<F>(consumer: F) -> impl std::future::Future<Output = ()> + Send + 'static
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    // determinism-ok: test-only panic-drill trigger path (feature never shipped); read once at spawn, never folded into consensus state
    let trigger = std::env::var_os(PANIC_DRILL_ENV).map(std::path::PathBuf::from);
    async move {
        let Some(trigger) = trigger else {
            consumer.await;
            return;
        };
        log::warn!(
            "PANIC_DRILL build (#3224): the inbound P2P consumer panics when {} appears. Never ship this binary.",
            trigger.display()
        );
        tokio::pin!(consumer);
        loop {
            tokio::select! {
                () = &mut consumer => return,
                () = tokio::time::sleep(Duration::from_millis(200)) => {
                    if std::fs::remove_file(&trigger).is_ok() {
                        panic!("PANIC_DRILL: injected inbound P2P consumer panic (#3224)");
                    }
                }
            }
        }
    }
}

/// How long an idle HTTP connection is held open before the server closes it.
///
/// actix-web's default is **5 seconds**, and that default is the direct cause of
/// the intermittent `[attester] cycle failed: … connection closed before message
/// completed` errors observed on 2026-08-12 against
/// `GET /transactions/oracle/pending`.
///
/// The mechanism is a keep-alive reuse race, and the numbers were pathologically
/// aligned:
///
/// - `oracle_attester` polls on `WRFCOIN_ATTESTER_POLL_SECS` (default **5s**);
///   the measured live cycle gap was 5.05–5.18s.
/// - Its `reqwest::Client` sets no `pool_idle_timeout`, so hyper retains an idle
///   pooled connection for its own default of 90s and will hand it back out.
/// - This server closed that connection at exactly 5s — that is, *just before*
///   every single reuse attempt.
///
/// So every cycle raced: hyper usually observes the FIN first and opens a fresh
/// connection, but when the checkout wins that race it writes a request onto a
/// socket the server has already closed, and the cycle fails. It reproduced on
/// two attesters over two unrelated transports (node container over the Docker network,
/// the workstation over a chained SSH tunnel), which is what identifies the
/// server — not either transport — as the shared cause.
///
/// # Choosing the value — the invariant, not a "wide margin"
///
/// An earlier revision picked 75s to "exceed any plausible poller interval". That
/// is not a provable property: `WRFCOIN_ATTESTER_POLL_SECS` accepts any integer
/// `>= 1` with no upper bound, so no finite keep-alive can exceed every permitted
/// interval, and 75s left a real 75–90s band in which the ordering came back.
///
/// The provable statement is about the *client's* idle retention instead. Let
/// `G` be the client's gap between requests on one connection, `C` the client's
/// idle-pool retention, and `K` this value. A reuse can only be attempted when
/// `G < C` (otherwise the client has already dropped the connection and opens a
/// fresh one), and such a reuse is unsafe only when `G > K`. So the dangerous
/// band is exactly
///
/// ```text
///     K < G < C
/// ```
///
/// which is empty once `K` is at least `C`. **The relation has to be strict —
/// `K > C`, not `K >= C`** — because equality is itself a race, not an empty
/// band. Both pinned implementations put the two events on the *same* instant:
///
/// - `hyper-util-0.1.20/src/client/legacy/pool.rs:773-776` expires an idle
///   connection only when `now.saturating_duration_since(instant) > timeout`.
///   The comparison is strict, so at an age of exactly `C` the connection has
///   **not** expired and is still eligible for checkout.
/// - `actix-http-3.12.1/src/config.rs:199-203` schedules the server's close at
///   `self.now() + dur`, i.e. exactly `K` after the connection went idle.
///
/// At `K == C` the client may therefore hand out a connection at the same
/// instant the server's close timer fires — the same FIN-observation race this
/// constant exists to remove. So the guard requires strict inequality, and
/// [`REQWEST_DEFAULT_POOL_IDLE_TIMEOUT`] is asserted against with `>` in the
/// tests, with a `K = C = 90s` mutation kept RED so the boundary cannot regress.
///
/// `C` for an un-hardened `reqwest` client is 90s
/// (`reqwest-0.12.28/src/async_impl/client.rs:301`). This value is 95s: a
/// deliberate **five-second scheduling margin** above that named bound. It is
/// explicitly *not* "the smallest value that works" — actix accepts an arbitrary
/// `Duration`, so 91s, or 90s plus a nanosecond, would satisfy `K > C` and be
/// smaller. Five seconds is chosen to absorb timer and scheduling slop on a
/// loaded host rather than to sit one tick above a cliff, while still keeping
/// retention short for the reasons in the capacity section.
///
/// # Exactly what is and is not guaranteed
///
/// **Guaranteed.** Any client whose idle retention is **strictly below 95s**
/// never reuses a connection this server has already closed — for **every**
/// request gap, including the 1s floor and the 300s upper end of the configured
/// `WRFCOIN_ATTESTER_POLL_SECS` range. The band `K < G < C` is empty because
/// `K > C`; the poll interval does not enter into it. The default `reqwest`
/// client (`C = 90s`) is inside this bound, and so is #2785's attester
/// (`C = 2s`).
///
/// A client at exactly `C = 95s` is **not** covered: that is the equality case
/// above, and it races.
///
/// **Not guaranteed.** Two cases, and this constant does not address either:
///
/// - **Clients retaining longer than 95s.** A client with
///   `pool_idle_timeout > 95s`, or a non-`reqwest` client with a longer default,
///   reopens the band at `95s <= G < C` — the lower end included, per the
///   equality case above. Example: a client holding 300s that polls every 180s
///   still races. `K > C` is a **supported-range** statement about client
///   retention, not a universal one, and raising `K` only moves the range — it
///   never removes the case.
/// - **Intermediaries.** A proxy, load balancer, or tunnel with its own idle
///   timeout below `C` can close first regardless of what this server does; the
///   server cannot override a hop it does not run. The two current transports
///   have no such hop (node container over the Docker network, the workstation over raw
///   SSH forwarding), which is why the observed fault was entirely ours.
///
/// So: this constant does **not** make the server "never the side that closes
/// first". It makes the server not close first *for clients inside the stated
/// retention bound*. The durable fix for the excluded cases is client-side —
/// #2785 sets the attester's `pool_idle_timeout` to 2s, which empties the band
/// from below for any `K > 2s` and holds regardless of intermediaries. The two
/// are complementary and neither subsumes the other: this one covers clients we
/// do not control against this server, #2785 covers that client against servers
/// and hops we do not control.
///
/// # Capacity — the measured bound, and what it does not settle
///
/// Longer retention does not change any ceiling; it changes how long an already
/// established connection occupies one. The ceilings, measured on the deployment
/// rather than quoted from the framework:
///
/// - actix's per-worker cap is 25,000 (`HttpServer::max_connections` default) and
///   the default worker count is `available_parallelism()`, which is **8** on
///   node container — a framework maximum of **200,000**. That number is not reachable
///   here and is **not** offered as a safety argument.
/// - The binding constraint is the process file-descriptor limit: the running
///   node container has `ulimit -n` = **1024** soft. Every retained connection is
///   one descriptor, shared with storage, P2P, and logging, so the real
///   worst-case retained-connection boundary is **under ~1024**, two orders of
///   magnitude below the framework cap.
/// - Measured steady-state occupancy on port 8556 is **1** established
///   connection.
///
/// What this value changes is the *arrival rate* an occupant needs to pin those
/// descriptors: sustaining N held connections requires roughly `N / K` opens per
/// second, so 5s → 95s cuts the required rate ~19×. It does not make a held
/// connection cheaper for the server — one descriptor either way — it makes the
/// FD table cheaper to *keep* full.
///
/// **Unresolved boundary, recorded rather than dismissed:** anything able to open
/// ~1024 concurrent connections can exhaust the descriptor table, and this change
/// lowers the rate needed to hold it there. That is mitigated but not eliminated
/// by reachability — the API listener is published on `127.0.0.1:8556` only
/// (verified on the deployment; port 9000/P2P is the `0.0.0.0` one), so the
/// reachable client set is processes inside node container plus the operator SSH tunnel,
/// not the internet. It is not eliminated, and it is not created by this change
/// either; explicit connection caps belong to the interface resource-ceiling work
/// (#2112) and are deliberately not redefined inside a keep-alive fix.
/// [`LAUNCH_API_KEEP_ALIVE_CEILING`] bounds future drift upward.
const LAUNCH_API_KEEP_ALIVE: std::time::Duration = std::time::Duration::from_secs(95);

/// `reqwest`'s default idle-pool retention — the `C` in the band above.
///
/// Verified from the vendored crate rather than remembered:
/// `reqwest-0.12.28/src/async_impl/client.rs:301` sets
/// `pool_idle_timeout: Some(Duration::from_secs(90))`. An un-hardened client on
/// this network is the worst case [`LAUNCH_API_KEEP_ALIVE`] has to clear.
const REQWEST_DEFAULT_POOL_IDLE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(90);

/// Upper bound on idle-connection retention.
///
/// The invariant argues for a *larger* keep-alive, so it needs a stop: without
/// one, "raise it again" is always the cheapest answer to the next race, and
/// idle residency grows without any stated limit. Five minutes is far above the
/// 90s the strict invariant requires and far below anything that could be called
/// connection retention without end.
const LAUNCH_API_KEEP_ALIVE_CEILING: std::time::Duration = std::time::Duration::from_secs(300);

/// The one concrete consensus engine constructed by `launch_api`.
///
/// `PopConsensusAdapter` owns the gossip-side producer-authority gate (#2290),
/// while the generic `TypedConsensusManager<E>` would let a different engine
/// type-check in its place and silently drop that gate (#2603).
///
/// # What this seam actually guarantees — and what it does not
///
/// The protection is this function's **monomorphic signature**, not the test
/// below. Because it returns
/// `TypedConsensusManager<PopConsensusAdapter>` concretely, an engine swap
/// routed *through* this helper is a compile error.
///
/// It does **not** prevent a swap that bypasses the helper. Nothing compels
/// `main` to keep calling it, and no test can observe which engine `main`
/// chose — extracting construction is exactly what makes it testable, and the
/// extraction is itself what a bypass would skip. The call site is therefore
/// annotated with the engine type explicitly, so a bypass has to edit a line
/// that names `PopConsensusAdapter` rather than one that merely infers it.
///
/// `launch_api_constructed_engine_rejects_unauthorized_producer` pins the
/// remaining half: that the engine this helper builds really does reject a
/// signed non-member producer, driving the shared
/// `verify_block_producer_authorized` gate through a seeded
/// `producer_set_history`. That is a real assertion about the engine; it is not
/// an assertion about `main`'s wiring.
///
/// Closing the bypass for good needs the gate moved below the engine (#2603
/// options 2/3) — a consensus-adjacent change that should not ride a pre-cut
/// PR. This is deliberately the narrow half.
fn launch_api_consensus_manager(
    adapter: wrfcoin_consensus_engines::PopConsensusAdapter,
    config: wrfcoin_consensus_engines::ConsensusConfig,
) -> TypedConsensusManager<wrfcoin_consensus_engines::PopConsensusAdapter> {
    TypedConsensusManager::new(adapter, config)
}

// ---------------------------------------------------------------------------
// Startup config error (P2P feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "p2p")]
#[derive(Debug, thiserror::Error, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum StartupConfigError {
    #[error(
        "NC_NODE_BUG_INVALID_LISTEN_MULTIADDR: invalid p2p listen address '{listen_address}': {reason}"
    )]
    InvalidListenMultiaddr {
        listen_address: String,
        reason: String,
    },
    #[error(
        "NC_NODE_CFG_INVALID_EXTERNAL_ADDR: {env_var} is set but is not a valid multiaddr \
         (operator value redacted). A node that silently drops its external address advertises \
         only unroutable bridge/loopback listen addresses to the mesh (#2819) — refusing to start instead."
    )]
    InvalidExternalAddress { env_var: &'static str },
    #[error(
        "NC_NODE_CFG_INVALID_PORT: {env_var} is not a valid non-zero TCP/UDP port (operator value redacted)"
    )]
    InvalidPort { env_var: &'static str },
    #[error(
        "NC_NODE_CFG_INVALID_PEER_ADDR: {env_var} entry {entry_index} is not a valid multiaddr (operator value redacted)"
    )]
    InvalidPeerAddress {
        env_var: &'static str,
        entry_index: usize,
    },
    #[error(
        "NC_NODE_CFG_INVALID_P2P_ROLE: the declared P2P topology role cannot be honoured: \
         {reason}. A validator that starts without its sentry allowlist, or a sentry whose \
         role word is misspelled, would run as an ordinary public peer while its operator \
         believes the topology is enforced (#2120) — refusing to start instead."
    )]
    InvalidP2pRole { reason: String },
}

#[cfg(feature = "p2p")]
impl StartupConfigError {
    const fn error_code(&self) -> &'static str {
        match self {
            Self::InvalidListenMultiaddr { .. } => "NC_NODE_BUG_INVALID_LISTEN_MULTIADDR",
            Self::InvalidExternalAddress { .. } => "NC_NODE_CFG_INVALID_EXTERNAL_ADDR",
            Self::InvalidPort { .. } => "NC_NODE_CFG_INVALID_PORT",
            Self::InvalidPeerAddress { .. } => "NC_NODE_CFG_INVALID_PEER_ADDR",
            Self::InvalidP2pRole { .. } => "NC_NODE_CFG_INVALID_P2P_ROLE",
        }
    }
}

// ---------------------------------------------------------------------------
// Startup helpers
// ---------------------------------------------------------------------------

/// Reads API tokens from environment variables and returns them as a map.
///
/// Returns `Ok(store)` when at least one token is present.
///
/// In **debug builds only**, also returns `Ok(store)` when the store is empty
/// and `WRFCOIN_ALLOW_NO_AUTH=1` is set (dev-mode bypass). This escape hatch
/// is compiled out in release builds — production binaries enforce auth unconditionally.
///
/// Returns `Err(())` when no tokens are configured (and the bypass is unavailable),
/// indicating the process should exit.
fn collect_token_store() -> Result<FxHashMap<String, AuthPrincipal>, ()> {
    let mut store = FxHashMap::default();
    for (token_env, role, wallet_env, contributor_env, oracle_node_env) in [
        (
            "WRFCOIN_OPERATOR_TOKEN",
            ApiRole::Operator,
            "WRFCOIN_OPERATOR_WALLET_ADDRESS",
            "WRFCOIN_OPERATOR_CONTRIBUTOR_ID",
            "WRFCOIN_OPERATOR_ORACLE_NODE_ID",
        ),
        (
            "WRFCOIN_CONTRIBUTOR_TOKEN",
            ApiRole::Contributor,
            "WRFCOIN_CONTRIBUTOR_WALLET_ADDRESS",
            "WRFCOIN_CONTRIBUTOR_ID",
            "WRFCOIN_CONTRIBUTOR_ORACLE_NODE_ID",
        ),
        (
            "WRFCOIN_ORACLE_TOKEN",
            ApiRole::Oracle,
            "WRFCOIN_ORACLE_WALLET_ADDRESS",
            "WRFCOIN_ORACLE_CONTRIBUTOR_ID",
            "WRFCOIN_ORACLE_NODE_ID",
        ),
        (
            "WRFCOIN_READER_TOKEN",
            ApiRole::Reader,
            "WRFCOIN_READER_WALLET_ADDRESS",
            "WRFCOIN_READER_CONTRIBUTOR_ID",
            "WRFCOIN_READER_ORACLE_NODE_ID",
        ),
    ] {
        if let Ok(token) = std::env::var(token_env) {
            let token = token.trim().to_string();
            if !token.is_empty() {
                let identity = Identity::new(
                    identity_claim_from_env(wallet_env),
                    identity_claim_from_env(contributor_env),
                    identity_claim_from_env(oracle_node_env),
                );
                let principal = if identity.is_empty() {
                    AuthPrincipal::with_role(role)
                } else {
                    AuthPrincipal::with_identity(role, identity)
                };
                store.insert(token, principal);
            }
        }
    }
    if store.is_empty() {
        // Debug-only escape hatch: WRFCOIN_ALLOW_NO_AUTH=1 bypasses token enforcement.
        // This block is absent in release builds — production binaries always reject
        // empty token stores, regardless of environment variables.
        #[cfg(debug_assertions)]
        if std::env::var("WRFCOIN_ALLOW_NO_AUTH").as_deref() == Ok("1") {
            eprintln!(
                "WARN: WRFCOIN_ALLOW_NO_AUTH=1 — ALL AUTHENTICATION DISABLED. \
                 This bypass exists only in debug builds and must never reach production."
            );
            return Ok(store);
        }
        #[cfg(debug_assertions)]
        eprintln!(
            "FATAL: No API tokens configured. Set at least WRFCOIN_OPERATOR_TOKEN.\n\
             Refusing to start without authentication. To run without auth (debug builds only),\n\
             set WRFCOIN_ALLOW_NO_AUTH=1."
        );
        #[cfg(not(debug_assertions))]
        eprintln!(
            "FATAL: No API tokens configured. Set at least WRFCOIN_OPERATOR_TOKEN.\n\
             Refusing to start without authentication."
        );
        return Err(());
    }
    Ok(store)
}

fn identity_claim_from_env(env_var: &str) -> Option<String> {
    std::env::var(env_var)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}


fn load_token_store() -> Arc<FxHashMap<String, AuthPrincipal>> {
    match collect_token_store() {
        Ok(store) => Arc::new(store),
        Err(()) => std::process::exit(1),
    }
}

/// Resolve the block signer for the snapshot-sync payload path (#1394).
///
/// * Debug builds: falls back to `EnvKeySigner` reading
///   `WRFCOIN_SNAPSHOT_SIGNER_PRIVATE_KEY` so the existing local-dev
///   workflow keeps working.
/// * Release builds: require a PKCS#11 token (`WRFCOIN_HSM_*` env vars).
///   If no HSM is configured the process **exits 1** — fail-closed so a
///   release binary cannot silently run without a hardware-backed signer.
///
/// The env-key fallback is `#[cfg(debug_assertions)]` inside
/// `protocol_signer::load_block_signer`, so release binaries physically
/// cannot contain the env-key path.
fn load_snapshot_signer(node_role: NodeRole) -> Option<Arc<dyn BlockSigner>> {
    // #2860: a public sync-only joiner has no signing token by design. It still
    // loads the independent trusted-signer set used to verify incoming snapshot
    // payloads; it simply cannot sign payloads served by this process. Producing
    // nodes continue through the unchanged release fail-closed path below.
    if node_role == NodeRole::SyncOnly {
        log::info!("[node-role] sync-only: local snapshot signing disabled");
        return None;
    }

    #[cfg(test)]
    if let Some(signer) = tests::rehearsal_daemon_signer(node_role) {
        return Some(signer);
    }
    match protocol_signer::load_block_signer(
        protocol_signer::env_vars::SNAPSHOT_SIGNER_PRIVATE_KEY,
    ) {
        Ok(Some(resolved)) => {
            protocol_signer::log_signer_banner(&resolved);
            Some(resolved.signer)
        }
        Ok(None) => {
            if node_role == NodeRole::ReceiptConsumer {
                eprintln!("FATAL: signing rehearsal requires its dedicated receiver signer");
                std::process::exit(1);
            }

            #[cfg(not(debug_assertions))]
            {
                eprintln!(
                    "FATAL: no snapshot signer configured. Release builds require \
                     a PKCS#11 token via {}, {}, {}, {} (PIN is read from the env \
                     var named by {}). See docs/operator/hsm-migration.md.",
                    protocol_signer::env_vars::HSM_MODULE_PATH,
                    protocol_signer::env_vars::HSM_TOKEN_LABEL,
                    protocol_signer::env_vars::HSM_KEY_LABEL,
                    protocol_signer::env_vars::HSM_PIN_ENV,
                    protocol_signer::env_vars::HSM_PIN_ENV,
                );
                std::process::exit(1);
            }
            #[cfg(debug_assertions)]
            {
                log::warn!(
                    "[signer] no signer configured; snapshot-sync payloads will not be signed. \
                     Set {} (debug) or the WRFCOIN_HSM_* vars (production).",
                    protocol_signer::env_vars::SNAPSHOT_SIGNER_PRIVATE_KEY,
                );
                None
            }
        }
        Err(e) => {
            eprintln!("FATAL: failed to initialise snapshot signer: {e:#}");
            std::process::exit(1);
        }
    }
}

/// Belt-and-braces override for producer-auth activation (#2290).
///
/// **This is not the source of truth.** Activation is derived from the chain's
/// own state — whether genesis seeded an authorized producer set — exactly as
/// #2398's verifier derives it. This variable can only *add* strictness on a
/// chain that has no seeded set (e.g. a staging node that wants to refuse
/// unsigned production early); it cannot switch enforcement off on a chain that
/// has one.
///
/// The previous revision made this the sole input, which is what Elsa's round-2
/// blocker 1 was about: if genesis seeded the set but the flag was omitted or
/// mistyped, the node fell back to unsigned production and built blocks every
/// peer rejects.
const PRODUCER_AUTH_REQUIRED_ENV: &str = "WRFCOIN_PRODUCER_AUTH_REQUIRED";

/// Role declaration shared with the packaged sync-node entrypoints (#2860).
const SYNC_ONLY_ENV: &str = "WRFCOIN_SYNC_ONLY";

/// The P2P topology role (#2120): `validator` | `sentry` | `public`. Read here
/// so the production role can be *derived* from the topology role — a sentry
/// never produces, so it must boot as a reader and must not be refused by the
/// #2290 producer-auth gate for lacking a signer. Same spelling as
/// `wrfcoin_p2p::config::P2P_NODE_ROLE_ENV`; kept as a local constant so the
/// derivation also exists in a build without the `p2p` feature.
const P2P_NODE_ROLE_ENV: &str = "WRFCOIN_P2P_NODE_ROLE";

/// Parse a security-relevant on/off switch **fallibly**.
///
/// Any explicit value that is not a recognised spelling is an `Err`, never a
/// silent `false`. This is the defect class Frank found independently on #2397:
/// `WRFCOIN_REQUIRE_PRODUCER_AUTH=tru` parsed as *false* and let an
/// unauthenticated fresh chain start. A security switch that silently ignores a
/// typo is worse than one that does not exist, because the operator has evidence
/// they enabled it.
///
/// Absent or empty ⇒ `Ok(None)` — "not configured", which is different from
/// "configured off" and is the only case that defers to the chain's own state.
fn parse_security_switch(var: &str, raw: Option<&str>) -> Result<Option<bool>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(Some(true)),
        "0" | "false" | "no" | "off" => Ok(Some(false)),
        other => Err(format!(
            "{var}='{other}' is not a recognised on/off value. Accepted: 1/true/yes/on, \
             0/false/no/off (case-insensitive). Refusing to guess: a typo'd security switch \
             that silently reads as OFF is how an unauthenticated node starts while its \
             operator believes it is enforcing (#2290)."
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeRole {
    Producing,
    SyncOnly,
    ReceiptConsumer,
}

const P2P_PROFILE_ENV: &str = "WRFCOIN_P2P_PROFILE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum P2pStartupProfile {
    Disabled,
    Required,
    Developer,
}

impl P2pStartupProfile {
    const fn label(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Required => "required",
            Self::Developer => "developer",
        }
    }

    const fn is_enabled(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

/// Resolve one unambiguous networking posture. Existing
/// `WRFCOIN_P2P_ENABLED=true` maps to fail-closed `required`; only the explicit
/// `developer` profile permits an ephemeral identity fallback.
fn p2p_profile_from_raw(
    profile_raw: Option<&str>,
    enabled_raw: Option<&str>,
) -> Result<P2pStartupProfile, String> {
    let legacy = parse_security_switch("WRFCOIN_P2P_ENABLED", enabled_raw)?;
    let Some(profile_raw) = profile_raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(if legacy == Some(true) {
            P2pStartupProfile::Required
        } else {
            P2pStartupProfile::Disabled
        });
    };
    let profile = match profile_raw.to_ascii_lowercase().as_str() {
        "disabled" => P2pStartupProfile::Disabled,
        "required" | "public-release" | "validator" => P2pStartupProfile::Required,
        "developer" | "demo" => P2pStartupProfile::Developer,
        other => {
            return Err(format!(
                "{P2P_PROFILE_ENV}='{other}' is invalid; expected disabled, required, or developer"
            ));
        },
    };
    if let Some(legacy) = legacy {
        if legacy != profile.is_enabled() {
            return Err(format!(
                "WRFCOIN_P2P_ENABLED conflicts with {P2P_PROFILE_ENV}={}; remove the legacy switch or make both agree",
                profile.label()
            ));
        }
    }
    Ok(profile)
}

fn p2p_profile_from_env() -> Result<P2pStartupProfile, String> {
    p2p_profile_from_raw(
        // determinism-ok: startup-only P2P profile selection, read once before block processing begins; it chooses the transport, never a state transition
        std::env::var(P2P_PROFILE_ENV).ok().as_deref(),
        // determinism-ok: startup-only P2P enable flag, read once before block processing begins; transport configuration, never consensus state
        std::env::var("WRFCOIN_P2P_ENABLED").ok().as_deref(),
    )
}

/// Resolve the node's production role fail-closed from the existing entrypoint
/// signal. An absent, empty, or explicit false value preserves the historical
/// producing role; only an explicit recognised true value narrows the node to
/// validation/relay duty.
///
/// Production resolves through [`node_role_from_raw`] (#2120), which reads the
/// same `parse_security_switch` this wraps; this single-variable form is kept
/// for the #2860 parser tests.
#[cfg(test)]
fn node_role_from_raw_sync_only(raw: Option<&str>) -> Result<NodeRole, String> {
    Ok(match parse_security_switch(SYNC_ONLY_ENV, raw)? {
        Some(true) => NodeRole::SyncOnly,
        Some(false) | None => NodeRole::Producing,
    })
}

/// What the P2P topology role says about block production (#2120).
///
/// * `sentry` ⇒ `Some(true)`: a sentry relays for its validator and never
///   produces, so it boots as a reader. This is the live seed crash-loop fix —
///   on a chain whose genesis seeded a producer set, a signer-less node that
///   is not declared sync-only is refused by the #2290 gate at every boot.
/// * `validator` ⇒ `Some(false)`: a validator exists to produce.
/// * `public` / unset ⇒ `None`: no opinion; `WRFCOIN_SYNC_ONLY` decides as
///   it always has.
/// * anything else ⇒ `Err`: a misspelled role word is refused, never guessed
///   (the same rule `P2pConfig::apply_role_env` applies later in boot).
fn p2p_role_production_posture(raw: Option<&str>) -> Result<Option<bool>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let word = raw.trim();
    if word.is_empty() {
        return Ok(None);
    }
    match word.to_ascii_lowercase().as_str() {
        "sentry" => Ok(Some(true)),
        "validator" => Ok(Some(false)),
        "public" => Ok(None),
        other => Err(format!(
            "{P2P_NODE_ROLE_ENV}='{other}' is not one of validator|sentry|public. Refusing to \
             guess: a sentry whose role word is misspelled would boot as a producer and be \
             refused by the producer-auth gate on every restart (#2120)."
        )),
    }
}

/// Resolve the production role from BOTH declarations, fail-closed (#2120).
///
/// `WRFCOIN_SYNC_ONLY` keeps its #2860 meaning. `WRFCOIN_P2P_NODE_ROLE` may
/// *imply* it (`sentry`) or *contradict* it (`validator` + sync-only, or
/// `sentry` + an explicit `WRFCOIN_SYNC_ONLY=false`); a contradiction is a
/// startup refusal naming both variables, because a node that silently picks
/// one side is a node whose operator believes the other.
fn node_role_from_raw(
    sync_only_raw: Option<&str>,
    p2p_role_raw: Option<&str>,
) -> Result<NodeRole, String> {
    let explicit = parse_security_switch(SYNC_ONLY_ENV, sync_only_raw)?;
    let implied = p2p_role_production_posture(p2p_role_raw)?;
    match (explicit, implied) {
        (Some(false), Some(true)) => Err(format!(
            "{P2P_NODE_ROLE_ENV}=sentry declares a non-producing relay, but {SYNC_ONLY_ENV} is \
             explicitly false. A sentry never produces; remove {SYNC_ONLY_ENV} or change the \
             role (#2120)."
        )),
        (Some(true), Some(false)) => Err(format!(
            "{P2P_NODE_ROLE_ENV}=validator declares a block producer, but {SYNC_ONLY_ENV} is \
             true. A validator that never produces is a misconfiguration, not a role; declare \
             it a sentry or drop {SYNC_ONLY_ENV} (#2120)."
        )),
        (Some(true), _) | (None, Some(true)) => Ok(NodeRole::SyncOnly),
        (Some(false), _) | (None, Some(false)) | (None, None) => Ok(NodeRole::Producing),
    }
}

fn node_role_from_env() -> Result<NodeRole, String> {
    // Read once at the single boot call site (the run fn's REFUSING-TO-START
    // path); the resolved role is passed by value thereafter, so two nodes
    // differing here still compute identical block validity and state roots.
    // NOTE: the gate accepts the marker only on the flagged line or the line
    // immediately above it — keep the next line directly above the read.
    // determinism-ok: node-local startup role read once at boot; it controls whether this process may invoke a producer, never block validity or state-transition results
    let sync_only = std::env::var(SYNC_ONLY_ENV).ok();
    // determinism-ok: node-local startup topology role read once at boot (#2120); it selects reader vs producer posture for this process only, never block validity or state-transition results
    let p2p_role = std::env::var(P2P_NODE_ROLE_ENV).ok();
    let role = node_role_from_raw(sync_only.as_deref(), p2p_role.as_deref())?;
    // determinism-ok: isolated node-local rehearsal startup posture, never block validity or state roots
    let rehearsal = std::env::var("WRFCOIN_NODE_SHARE_REHEARSAL").ok();
    if parse_security_switch("WRFCOIN_NODE_SHARE_REHEARSAL", rehearsal.as_deref())? != Some(true) {
        return Ok(role);
    }
    if role == NodeRole::SyncOnly {
        return Err("signing rehearsal refuses sync-only posture".into());
    }
    if p2p_profile_from_env()? != P2pStartupProfile::Disabled {
        return Err("signing rehearsal requires P2P disabled".into());
    }
    // determinism-ok: node-local isolated API binding checked once at rehearsal startup
    let host = std::env::var("WRFCOIN_API_HOST").ok();
    if host.as_deref() != Some("127.0.0.1") {
        return Err("signing rehearsal requires explicit loopback API binding".into());
    }
    // determinism-ok: local rehearsal database isolation checked before storage opens, never consensus input
    let directory = std::env::var("WRFCOIN_DATA_DIR").map_err(|_| "rehearsal database missing")?;
    let database = std::path::Path::new(&directory);
    let parent = database.parent().ok_or("rehearsal root missing")?;
    let marker = parent.join(".node-share-rehearsal-receiver");
    if !database.is_absolute() || database.exists() || parent.is_symlink()
        || !marker.is_file() || marker.is_symlink() {
        return Err("signing rehearsal requires a new database and owned receiver marker".into());
    }
    Ok(NodeRole::ReceiptConsumer)
}

/// How producer-signer resolution turned out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProducerSignerResolution {
    /// A signer was resolved and is usable.
    Resolved,
    /// No producer signer is configured at all.
    Absent,
    /// A signer was configured but failed to initialise (HSM unreachable, bad
    /// key material, invalid rotation config, …).
    InitFailed,
}

/// What the node must do given how producer-signer resolution turned out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProducerSignerDecision {
    /// Sign produced blocks with the resolved signer.
    Sign,
    /// Produce unsigned blocks — legitimate only on a chain that never
    /// configured producer auth (legacy / local dev).
    ProduceUnsigned,
    /// Refuse to start. Producing here would be actively harmful.
    RefuseToStart,
}

/// Is producer authentication active on this chain?
///
/// **Single source of truth: the chain's own state.** #2398 activates producer
/// authentication iff `chain.state.producer_set_history` is non-empty, so this
/// must read the same thing or the two sides disagree about whether a block
/// needs a signature — and the producer is the side that loses, because it
/// commits blocks locally that peers reject.
///
/// The env override may only tighten: it can require signing on a chain with no
/// seeded set, but it cannot excuse a chain that has one.
fn producer_auth_active(chain_has_producer_set: bool, env_override: Option<bool>) -> bool {
    chain_has_producer_set || env_override.unwrap_or(false)
}

/// The provisioning policy for the producer signer (#2290 P1-4).
///
/// The failure this closes: producer-signer provisioning used to **fail open**.
/// A missing or broken signer was logged and production continued with unsigned
/// blocks. On a chain whose genesis seeds the producer set, #2398 enforces
/// producer auth from block 1 — so that node would happily advance its *local*
/// chain with blocks every single peer rejects. It looks healthy, mines, and
/// silently forks itself off the network; the operator finds out when sync
/// wedges. An HSM that failed to initialise is exactly the case where this
/// happens by accident rather than by configuration.
fn producer_signer_decision(
    auth_required: bool,
    resolution: ProducerSignerResolution,
) -> ProducerSignerDecision {
    match (auth_required, resolution) {
        (_, ProducerSignerResolution::Resolved) => ProducerSignerDecision::Sign,
        (true, _) => ProducerSignerDecision::RefuseToStart,
        (false, _) => ProducerSignerDecision::ProduceUnsigned,
    }
}

/// **The startup seam.** Everything `load_producer_signer` decides, as one pure
/// function over raw inputs — so the tests exercise the real parser and the real
/// policy rather than a re-typed copy of either.
///
/// That distinction is the whole point of this revision. The previous guard
/// asserted `matches!(value, "1" | "true" | ...)` inline, which is a tautology:
/// Elsa replaced the production parser with a constant `false` and the test
/// still passed. Driving raw strings through *this* function means a broken
/// parser, a broken activation source, or a broken policy all surface as a
/// failing test.
///
/// `Err` means refuse to start, with the message to print.
fn producer_startup_decision(
    chain_has_producer_set: bool,
    raw_auth_override: Option<&str>,
    resolution: ProducerSignerResolution,
) -> Result<ProducerSignerDecision, String> {
    Ok(producer_signer_decision(
        producer_auth_required(chain_has_producer_set, raw_auth_override)?,
        resolution,
    ))
}

/// Resolve whether producer authentication is active while preserving the
/// invariant that an environment variable may tighten chain policy but never
/// disable a producer set committed by genesis.
fn producer_auth_required(
    chain_has_producer_set: bool,
    raw_auth_override: Option<&str>,
) -> Result<bool, String> {
    let env_override = parse_security_switch(PRODUCER_AUTH_REQUIRED_ENV, raw_auth_override)?;

    // An explicit `false` on a chain that HAS a seeded producer set is an
    // operator asking to disable enforcement they cannot disable. Say so rather
    // than ignoring it: silently overriding the operator is how the two sides
    // end up disagreeing without anyone noticing.
    if chain_has_producer_set && env_override == Some(false) {
        return Err(format!(
            "{PRODUCER_AUTH_REQUIRED_ENV} is set to a false value, but this chain's genesis \
             seeded an authorized producer set — producer authentication is active and \
             cannot be switched off from the environment. Every block this node produced \
             unsigned would be rejected by its peers. Remove the variable, or produce on a \
             chain with no seeded producer set (#2290)."
        ));
    }

    Ok(producer_auth_active(chain_has_producer_set, env_override))
}

/// Whether this chain's genesis seeded an authorized producer set — the
/// activation source #2398's verifier uses.
///
/// Reads `State::producer_set_history` — the exact field
/// `verify_block_producer_authorized` treats as authoritative — so the producer
/// and the verifier cannot disagree about whether this chain enforces.
fn chain_has_seeded_producer_set(
    state: &wrfcoin_shared_protocol::blockchain::state::State,
) -> bool {
    !state.producer_set_history.is_empty()
}

/// Decide whether this node can produce the next block under the producer set
/// that is authoritative at that height. This deliberately takes the current
/// height, not the last persisted block's producer, because a producer-set
/// change may become effective exactly at the next block.
fn evaluate_next_producer_authority(
    current_height: u64,
    producer_set_history: &[wrfcoin_shared_protocol::consensus::producer_set::ProducerSetChange],
    node_role: NodeRole,
    node_key: Option<&PublicKey>,
    require_auth: bool,
) -> Result<ProducerAuthorityBootStatus, String> {
    let next_height = current_height
        .checked_add(1)
        .ok_or_else(|| "cannot resolve producer authority after u64::MAX".to_string())?;
    let members = wrfcoin_shared_protocol::consensus::producer_authority::resolve_authorized_producers_at(
        producer_set_history,
        next_height,
    )?;
    let authorized_set: Vec<String> = members
        .iter()
        .map(|member| format!("{}=0x{}", member.producer_id, hex::encode(member.public_key)))
        .collect();
    let resolved_set = if authorized_set.is_empty() {
        "<empty>".to_string()
    } else {
        authorized_set.join(", ")
    };

    let Some(node_key) = node_key else {
        let status = if node_role != NodeRole::Producing {
            "follower"
        } else {
            "no_signer"
        };
        return Ok(ProducerAuthorityBootStatus {
            status: status.to_string(),
            next_height,
            node_key: None,
            authorized_set,
            message: format!(
                "{status}: no producer signer configured for next block height {next_height}; \
                 resolved authorized producer set [{resolved_set}]"
            ),
            readiness_gate: false,
        });
    };

    let node_key_hex = node_key.to_hex();
    let authorized = members.iter().any(|member| member.public_key == node_key.as_bytes());
    if authorized {
        return Ok(ProducerAuthorityBootStatus {
            status: "authorized".to_string(),
            next_height,
            node_key: Some(node_key_hex.clone()),
            authorized_set,
            message: format!(
                "producer key 0x{node_key_hex} is authorized for next block height \
                 {next_height}; resolved authorized producer set [{resolved_set}]"
            ),
            readiness_gate: false,
        });
    }

    let message = format!(
        "producer key 0x{node_key_hex} is NOT authorized for next block height \
         {next_height}; resolved authorized producer set [{resolved_set}]"
    );
    if require_auth {
        return Err(format!(
            "{message}. WRFCOIN_REQUIRE_PRODUCER_AUTH is enabled; refusing to start"
        ));
    }
    Ok(ProducerAuthorityBootStatus {
        status: "unauthorized".to_string(),
        next_height,
        node_key: Some(node_key_hex),
        authorized_set,
        message,
        readiness_gate: true,
    })
}

/// Run and log the next-block producer check once, before the consensus engine
/// is exposed through `AppState`. The signer is passed in from the same
/// resolution used to configure production, so the check cannot inspect one
/// credential while the engine uses another.
fn check_next_producer_authority(
    blockchain: &Blockchain,
    producer_signer: Option<&Arc<dyn BlockSigner>>,
    node_role: NodeRole,
) -> Result<ProducerAuthorityBootStatus, String> {
    let require_auth = parse_security_switch(
        genesis_producers::REQUIRE_PRODUCER_AUTH_ENV,
        // determinism-ok: startup-only policy read; it validates boot posture and never enters consensus state, block bytes, or state roots
        std::env::var(genesis_producers::REQUIRE_PRODUCER_AUTH_ENV)
            .ok()
            .as_deref(),
    )?
    .unwrap_or(true);
    let node_key = producer_signer.map(|signer| signer.public_key());
    let status = evaluate_next_producer_authority(
        blockchain.height,
        &blockchain.state.producer_set_history,
        node_role,
        node_key.as_ref(),
        require_auth,
    )?;
    if status.status == "authorized" {
        log::info!(
            target: "consensus",
            "Next-block producer-authority boot check passed: {}",
            status.message
        );
    } else if status.status == "unauthorized" && !require_auth {
        log::error!(
            target: "consensus",
            "PRODUCER AUTH PERMISSIVE MODE: {message}; WRFCOIN_REQUIRE_PRODUCER_AUTH=false was explicitly configured, so startup continues for operator recovery, but /readyz remains not-ready",
            message = status.message
        );
    } else {
        log::warn!(
            target: "consensus",
            "NEXT-PRODUCER BOOT CHECK: {}",
            status.message
        );
    }
    Ok(status)
}

/// Env var naming this producer's canonical payout address (#2290 §R2).
///
/// Must equal the `payout_address` of this node's entry in the genesis-seeded
/// authorized producer set, because that is what the verify-side gate compares
/// the coinbase output against.
///
/// **Interim source of truth.** Once #2398 lands, the payout is resolvable
/// on-chain from `chain.state.producer_set_history` by looking up this node's
/// producer public key, and this env var becomes a startup cross-check rather
/// than the source. Until then it is the only way the producer can know its own
/// declared payout, and binding to it is what keeps the SIGN side from emitting
/// blocks the gate rejects.
const PRODUCER_PAYOUT_ADDRESS_ENV: &str = "WRFCOIN_PRODUCER_PAYOUT_ADDRESS";

/// Resolve the producer payout binding from the environment.
///
/// A malformed address is fatal rather than ignored: silently skipping the
/// binding would restore exactly the fail-open behaviour #2290 P1-3 closes, and
/// a typo'd address is the likeliest way to get there.
fn load_producer_payout_address() -> Option<wrfcoin_shared_protocol::common::types::Address> {
    use wrfcoin_shared_protocol::common::types::Address;
    // determinism-ok: startup credential/config read, not a verifier, replay or state-transition input. It selects WHICH key material this node loads at boot; it never enters a preimage, a state transition or the state root, so two nodes with different values still compute identical roots (#2290, per #2414/#2416).
    let raw = std::env::var(PRODUCER_PAYOUT_ADDRESS_ENV).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<Address>() {
        Ok(address) => Some(address),
        Err(_) => {
            eprintln!(
                "FATAL: {PRODUCER_PAYOUT_ADDRESS_ENV}='{trimmed}' is not a canonical address. \
                 It must be the 0x-prefixed 20-byte payout_address from this node's entry in \
                 the genesis producer set — the exact string the verify-side authority gate \
                 compares the coinbase output against (#2290 §R2). Refusing to start rather \
                 than producing blocks with an unbound coinbase recipient."
            );
            std::process::exit(1);
        },
    }
}

/// Load the producer signer (#2290): the key that authenticates blocks this node
/// produces. Resolved via `load_block_signer_for`, which tries — in order — a
/// PKCS#11 HSM (with an optional producer-specific key label), a permission-
/// checked key **file**, then the debug hex env var. Distinct credentials from
/// the snapshot signer throughout.
///
/// **Fail-closed** when [`PRODUCER_AUTH_REQUIRED_ENV`] is set: a missing or
/// broken signer exits the process rather than producing blocks peers reject.
/// See [`producer_signer_decision`].
///
/// The producer pubkey is logged so it can be seeded into the genesis authorized
/// producer set.
fn load_producer_signer(
    state: &wrfcoin_shared_protocol::blockchain::state::State,
    node_role: NodeRole,
) -> Option<Arc<dyn BlockSigner>> {
    let chain_has_producer_set = chain_has_seeded_producer_set(state);
    // determinism-ok: startup credential/config read, not a verifier, replay or state-transition input. It selects WHICH key material this node loads at boot; it never enters a preimage, a state transition or the state root, so two nodes with different values still compute identical roots (#2290, per #2414/#2416).
    let raw_override = std::env::var(PRODUCER_AUTH_REQUIRED_ENV).ok();

    // #2860: validate the chain/auth posture, including the fatal case
    // where an operator tries to switch off a genesis-seeded producer set, but
    // stop before touching HSM/file/env producer credentials. This role cannot
    // use a signer because its consensus adapter is independently production-
    // disabled during startup.
    if node_role != NodeRole::Producing {
        if let Err(message) =
            producer_auth_required(chain_has_producer_set, raw_override.as_deref())
        {
            eprintln!("FATAL: {message}");
            std::process::exit(1);
        }
        return None;
    }

    let loaded = protocol_signer::load_block_signer_for(protocol_signer::SignerEnv::producer());

    let (resolution, resolved, error) = match loaded {
        Ok(Some(resolved)) => (ProducerSignerResolution::Resolved, Some(resolved), None),
        Ok(None) => (ProducerSignerResolution::Absent, None, None),
        Err(e) => (
            ProducerSignerResolution::InitFailed,
            None,
            Some(format!("{e:#}")),
        ),
    };

    let decision = match producer_startup_decision(
        chain_has_producer_set,
        raw_override.as_deref(),
        resolution,
    ) {
        Ok(decision) => decision,
        Err(message) => {
            eprintln!("FATAL: {message}");
            std::process::exit(1);
        },
    };

    match decision {
        ProducerSignerDecision::Sign => {
            // Non-panicking fail-closed branch rather than `expect`.
            //
            // `Sign` is only produced when resolution returned a signer, so the
            // `None` arm is unreachable today — but this is startup code on the
            // producer path, and an unreachable-by-construction panic here would
            // abort the node on a future refactor instead of refusing cleanly.
            // Refusing is the same outcome the surrounding policy already
            // chooses everywhere else in this function, so make it explicit.
            let Some(resolved) = resolved else {
                eprintln!(
                    "FATAL: internal inconsistency — the producer-signer policy selected \
                     Sign but no signer was resolved. Refusing to start rather than \
                     producing unsigned blocks on a chain that enforces producer \
                     authentication (#2290)."
                );
                std::process::exit(1);
            };
            protocol_signer::log_signer_banner(&resolved);
            log::info!(
                "[producer-signer] produced blocks will be signed; producer pubkey = {}",
                resolved.signer.public_key().to_hex(),
            );
            Some(resolved.signer)
        },
        ProducerSignerDecision::ProduceUnsigned => {
            log::warn!(
                // secret-leak-ok: the protocol_signer::env_vars args are env-var NAME constants, never key values
                "[producer-signer] no usable producer signer; produced blocks will be \
                 UNSIGNED. This is only legal on a chain that does not enforce producer \
                 authentication. Set {} (debug hex), {} (key file) or the WRFCOIN_HSM_* \
                 vars.{}",
                protocol_signer::env_vars::PRODUCER_SIGNER_PRIVATE_KEY,
                protocol_signer::env_vars::PRODUCER_SIGNER_KEY_FILE,
                error
                    .as_deref()
                    .map(|e| format!(" Resolution error: {e}"))
                    .unwrap_or_default(),
            );
            None
        },
        ProducerSignerDecision::RefuseToStart => {
            eprintln!(
                // secret-leak-ok: nothing interpolated here can carry key material. The three
                // `env_vars::*` arguments are compile-time constants holding env-var *names*
                // (e.g. "WRFCOIN_PRODUCER_SIGNER_PRIVATE_KEY"), never their values — the scanner
                // matches the "private" segment in the constant's identifier. The only dynamic
                // argument is `error`, which is `format!("{e:#}")` over the `load_block_signer_for`
                // Err path; every error there interpolates an env-var name, a file path, or a
                // `hex::FromHexError` (which reports position/length, not the input). Verified by
                // reading all ten error constructions in shared-protocol/src/crypto/signer.rs.
                "FATAL: this chain enforces block-producer authentication{}, but no usable \
                 block-producer signer could be resolved{}.\n\n\
                 Every block this node produced would be rejected by every peer while its \
                 own local chain advanced — a silent self-fork that looks like a healthy \
                 node until sync wedges. Refusing to start instead.\n\n\
                 Provision one of:\n  \
                 - WRFCOIN_HSM_* (production; {} selects the producer-specific key label)\n  \
                 - {} (key file, mode 0600)\n  \
                 - {} (debug builds only)\n\n(#2290)",
                if chain_has_producer_set {
                    " (genesis seeded an authorized producer set)"
                } else {
                    " (required by WRFCOIN_PRODUCER_AUTH_REQUIRED)"
                },
                error
                    .as_deref()
                    .map(|e| format!(": {e}"))
                    .unwrap_or_default(),
                protocol_signer::env_vars::HSM_PRODUCER_KEY_LABEL,
                protocol_signer::env_vars::PRODUCER_SIGNER_KEY_FILE,
                protocol_signer::env_vars::PRODUCER_SIGNER_PRIVATE_KEY,
            );
            std::process::exit(1);
        },
    }
}

fn parse_snapshot_trusted_signers(raw: &str) -> Result<FxHashSet<String>, String> {
    let mut trusted_signers = FxHashSet::default();
    for signer in raw.split(',').map(str::trim).filter(|entry| !entry.is_empty()) {
        let parsed = PublicKey::from_hex(signer)
            .map_err(|_| format!("invalid snapshot trusted signer public key '{signer}'"))?;
        // Insert canonical (lowercase) hex form, not the raw input.
        // Runtime signature verification uses to_hex() which emits lowercase.
        trusted_signers.insert(parsed.to_hex());
    }
    Ok(trusted_signers)
}

fn collect_snapshot_trusted_signers() -> Result<FxHashSet<String>, ()> {
    match std::env::var("WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS") {
        Ok(raw) => parse_snapshot_trusted_signers(&raw).map_err(|message| {
            eprintln!(
                "FATAL: {message}. Set WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS to a comma-separated list of ed25519 public keys."
            );
        }),
        Err(std::env::VarError::NotPresent) => Ok(FxHashSet::default()),
        Err(std::env::VarError::NotUnicode(_)) => {
            eprintln!(
                "FATAL: WRFCOIN_SNAPSHOT_TRUSTED_SIGNERS contains invalid unicode bytes."
            );
            Err(())
        }
    }
}

fn load_snapshot_trusted_signers() -> Arc<FxHashSet<String>> {
    match collect_snapshot_trusted_signers() {
        Ok(signers) => Arc::new(signers),
        Err(()) => std::process::exit(1),
    }
}

/// Env-var name that re-enables the node-local NFT mutation API (#1764).
const NFT_MUTATION_API_ENV: &str = "WRFCOIN_ENABLE_NFT_MUTATION_API";

/// Whether the NFT mutation endpoints (`/nft/mint`, `/nft/{id}/verify`,
/// `/nft/{id}/transfer`) are enabled. Disabled by default (#1764): NFT state is
/// part of the consensus `state_root` but NFT mutations are node-local and never
/// gossiped, so any mutation forks this node's `state_root` away from its peers
/// and wedges P2P sync. Opt back in with `WRFCOIN_ENABLE_NFT_MUTATION_API=1`
/// only on single-node / trusted-operator deployments.
fn nft_mutation_api_enabled_from_env() -> bool {
    nft_mutation_api_enabled_from_value(std::env::var(NFT_MUTATION_API_ENV).ok().as_deref())
}

/// Pure helper for [`nft_mutation_api_enabled_from_env`] so the parsing rule is
/// unit-testable without touching the process environment.
fn nft_mutation_api_enabled_from_value(value: Option<&str>) -> bool {
    match value {
        Some(v) => {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true")
        },
        None => false,
    }
}

const ORACLE_ADMIN_API_ENV: &str = "WRFCOIN_ENABLE_ORACLE_ADMIN_API";

/// Whether the node-local oracle/signing-key admin endpoints
/// (`POST /oracle/register`, `POST /oracle/config`, signing-key
/// register/revoke) are enabled. Disabled by default (#1775): `oracle_set` /
/// `oracle_quorum` and Oracle-role `signing_keys` are folded into the consensus
/// `state_root`, but these routes mutate them OUT OF BAND — there is no gossiped
/// transaction for `SetOracleConfig` / `RegisterSigningKeyEpoch` and
/// `extract_transitions` never reconstructs them, so a syncing follower can
/// never reproduce the mutation and forks its `state_root` permanently. Same
/// class as the NFT mutation API (#1764). Multi-node oracle/signing-key config
/// must be seeded deterministically and identically at genesis instead. Opt in
/// with `WRFCOIN_ENABLE_ORACLE_ADMIN_API=1` only on single-node /
/// trusted-operator deployments.
fn oracle_admin_api_enabled_from_env() -> bool {
    // Reuse the shared boolean-flag parser (`1`/`true`, default off).
    nft_mutation_api_enabled_from_value(std::env::var(ORACLE_ADMIN_API_ENV).ok().as_deref())
}

/// The initial authorized block-producer committee to seed at a FRESH genesis
/// (#2290 enforce-at-genesis). These members feed `state_root`, so they MUST be
/// PINNED and byte-identical across every node — never per-node env — or nodes
/// fork at genesis.
///
/// FALLIBLE (Alice contract, #2290): a malformed or missing-when-required genesis
/// producer config MUST be a hard startup failure, never a fall-through to an
/// empty (= unenforced) set — that empty-fall-through is the silent auth-downgrade
/// hole Dan flagged. Fail-closed, not fall-through.
///
/// Delegates to [`genesis_producers::resolve`] (merged in #2397), which owns
/// the config DTO, the pinned testnet constant, the
/// `WRFCOIN_GENESIS_PRODUCER_SET` / `WRFCOIN_NETWORK_ID` precedence and the
/// `WRFCOIN_REQUIRE_PRODUCER_AUTH` fail-closed guard. This function is only the
/// adapter: it maps the config type onto the consensus `ProducerMember` and
/// propagates every error, so an unresolvable or malformed configuration aborts
/// startup rather than yielding an empty (unenforced) set.
fn genesis_producer_members(
) -> Result<Vec<wrfcoin_shared_protocol::consensus::producer_set::ProducerMember>, String> {
    use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;
    Ok(genesis_producers::resolve()?
        .into_iter()
        .map(|m| ProducerMember {
            producer_id: m.producer_id,
            payout_address: m.payout_address,
            public_key: m.public_key,
        })
        .collect())
}

const DEBUG_SEED_API_ENV: &str = "WRFCOIN_ENABLE_DEBUG_SEED_API";
const MEMPOOL_MAX_SIZE_BYTES_ENV: &str = "WRFCOIN_MEMPOOL_MAX_SIZE_BYTES";
const MEMPOOL_MAX_TXS_PER_SENDER_ENV: &str = "WRFCOIN_MEMPOOL_MAX_TXS_PER_SENDER";
const MEMPOOL_MAX_AGE_SECS_ENV: &str = "WRFCOIN_MEMPOOL_MAX_AGE_SECS";
const MEMPOOL_MIN_FEE_RATE_ENV: &str = "WRFCOIN_MEMPOOL_MIN_FEE_RATE";

// ── #3095: upgrade-notice run-up ───────────────────────────────────────────

/// How often the run-up task reads the tip. A third of
/// `TARGET_BLOCK_TIME_SECONDS` (15s), so the "every block" ERROR cadence
/// inside the last 100 blocks observes every height instead of sampling it.
const UPGRADE_NOTICE_TICK_SECS: u64 = 5;

/// How often the same task polls peers for a notice, when the operator has
/// enabled the fetch. Ten minutes, per #3095.
const UPGRADE_NOTICE_PEER_FETCH_SECS: u64 = 600;

/// Poll each configured peer's `GET /network/upgrade` and offer whatever it
/// returns to [`accept_upgrade_notice`], which verifies before it stores.
///
/// The peers are named by `WRFCOIN_UPGRADE_NOTICE_PEER_URLS`, not discovered:
/// this node knows its P2P peers as libp2p multiaddrs and has no HTTP address
/// for them, so the fetch targets are operator configuration. That costs
/// nothing in trust — the transport was never trusted. A peer that lies,
/// omits, or serves a tampered notice changes nothing here, because the
/// signature check runs on this side.
///
/// Compiled only with `--features upgrade-notice-fetch`; without it the node
/// reads notices from `WRFCOIN_UPGRADE_NOTICE_FILE` alone and says so when
/// asked to fetch.
#[cfg(feature = "upgrade-notice-fetch")]
async fn fetch_upgrade_notices_from_peers(
    identity: &wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity,
) {
    use wrfcoin_native_chain::upgrade_notice::{
        accept_upgrade_notice, configured_peer_urls, peer_fetch_enabled,
    };

    if !peer_fetch_enabled() {
        return;
    }
    let peers = configured_peer_urls();
    if peers.is_empty() {
        log::warn!(
            "{} is enabled but {} lists no peer API URLs — no notice can be fetched (#3095)",
            wrfcoin_native_chain::upgrade_notice::PEER_FETCH_ENV,
            wrfcoin_native_chain::upgrade_notice::PEER_URLS_ENV
        );
        return;
    }
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .pool_idle_timeout(REQWEST_DEFAULT_POOL_IDLE_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            log::warn!("Upgrade-notice peer fetch disabled this cycle: {e} (#3095)");
            return;
        },
    };
    let allowlist = wrfcoin_shared_protocol::crypto::domain_signing::oracle_governance_pubkeys();
    // determinism-ok: the notice's own expiry is checked against this node's clock to decide whether to STORE it; the value never enters a preimage, a state transition or the state root, so two nodes with skewed clocks still compute identical roots
    let now = chrono::Utc::now().timestamp();

    for peer in peers {
        let url = format!("{peer}/network/upgrade");
        let body = match client.get(&url).send().await {
            Ok(response) => match response.json::<serde_json::Value>().await {
                Ok(body) => body,
                Err(e) => {
                    log::debug!("Upgrade-notice fetch from {url}: unreadable body ({e})");
                    continue;
                },
            },
            Err(e) => {
                log::debug!("Upgrade-notice fetch from {url}: {e}");
                continue;
            },
        };
        // A peer with nothing to say serves `"notice": null`; that is not an
        // error and must not be logged as one.
        let Some(raw) = body.get("notice").filter(|v| !v.is_null()) else {
            continue;
        };
        let candidate = match serde_json::from_value(raw.clone()) {
            Ok(candidate) => candidate,
            Err(e) => {
                log::warn!("Ignoring upgrade notice from {url}: not a signed notice ({e}) (#3095)");
                continue;
            },
        };
        let _ = accept_upgrade_notice(
            candidate,
            &identity.chain_id,
            &identity.network_id,
            &allowlist,
            now,
            &url,
        );
    }
}

/// Without `--features upgrade-notice-fetch` the node has no notice client.
/// An operator who asked for one is told once per cycle rather than left to
/// wonder why nothing arrives.
#[cfg(not(feature = "upgrade-notice-fetch"))]
async fn fetch_upgrade_notices_from_peers(
    _identity: &wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity,
) {
    if wrfcoin_native_chain::upgrade_notice::peer_fetch_enabled() {
        log::warn!(
            "{} is set but this binary was built without `--features upgrade-notice-fetch`; \
             upgrade notices can only arrive via {} (#3095)",
            wrfcoin_native_chain::upgrade_notice::PEER_FETCH_ENV,
            wrfcoin_native_chain::upgrade_notice::NOTICE_FILE_ENV
        );
    }
}


fn parse_positive_mempool_override<T>(
    name: &str,
    raw: Option<&str>,
    default: T,
) -> Result<T, String>
where
    T: std::str::FromStr + ToString,
{
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(default);
    };
    raw.parse::<T>()
        .ok()
        .filter(|value| {
            raw != "0"
                && raw.chars().all(|character| character.is_ascii_digit())
                && value.to_string() != "0"
        })
        .ok_or_else(|| format!("{name} must be a positive base-10 integer, got '{raw}'"))
}

fn parse_nonnegative_mempool_override(
    name: &str,
    raw: Option<&str>,
    default: u64,
) -> Result<u64, String> {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(default);
    };
    if !raw.chars().all(|character| character.is_ascii_digit()) {
        return Err(format!(
            "{name} must be a non-negative base-10 integer, got '{raw}'"
        ));
    }
    raw.parse::<u64>()
        .map_err(|_| format!("{name} is outside the supported u64 range: '{raw}'"))
}

fn mempool_config_from_values(
    max_size_bytes: Option<&str>,
    max_txs_per_sender: Option<&str>,
    max_age_secs: Option<&str>,
    min_fee_rate: Option<&str>,
) -> Result<Config, String> {
    let mut config = Config::default();
    config.mempool_max_size_bytes = parse_positive_mempool_override(
        MEMPOOL_MAX_SIZE_BYTES_ENV,
        max_size_bytes,
        config.mempool_max_size_bytes,
    )?;
    config.mempool_max_txs_per_sender = parse_positive_mempool_override(
        MEMPOOL_MAX_TXS_PER_SENDER_ENV,
        max_txs_per_sender,
        config.mempool_max_txs_per_sender,
    )?;
    config.mempool_max_age_secs = parse_positive_mempool_override(
        MEMPOOL_MAX_AGE_SECS_ENV,
        max_age_secs,
        config.mempool_max_age_secs,
    )?;
    config.mempool_min_fee_rate = parse_nonnegative_mempool_override(
        MEMPOOL_MIN_FEE_RATE_ENV,
        min_fee_rate,
        config.mempool_min_fee_rate,
    )?;
    Ok(config)
}

fn mempool_config_from_env() -> Result<Config, String> {
    mempool_config_from_values(
        // determinism-ok: startup-only node-local admission limits are copied into Config before the mempool is constructed
        std::env::var(MEMPOOL_MAX_SIZE_BYTES_ENV).ok().as_deref(),
        // determinism-ok: startup-only node-local admission limits are copied into Config before the mempool is constructed
        std::env::var(MEMPOOL_MAX_TXS_PER_SENDER_ENV)
            .ok()
            .as_deref(),
        // determinism-ok: startup-only node-local admission limits are copied into Config before the mempool is constructed
        std::env::var(MEMPOOL_MAX_AGE_SECS_ENV).ok().as_deref(),
        // determinism-ok: startup-only node-local admission limits are copied into Config before the mempool is constructed
        std::env::var(MEMPOOL_MIN_FEE_RATE_ENV).ok().as_deref(),
    )
}

/// Whether the node-local debug seeding endpoint (`POST /debug/seed-account`)
/// is enabled. Disabled by default (#1811): `seed_account` applies an
/// `ApplyReward` and mutates `stake_locked` / `spendable_balance` directly on
/// the node's `accounts` — all hashed into the consensus `state_root`
/// (`state_root_material`) — with NO gossiped transaction and no block, so
/// `extract_transitions` never reconstructs it on peers. A single Operator call
/// on a multi-node network forks this node's `state_root` permanently — the same
/// out-of-band-mutation divergence class as the NFT mutation API (#1764) and the
/// oracle/signing-key admin API (#1775). Opt in with
/// `WRFCOIN_ENABLE_DEBUG_SEED_API=1` only on single-node / trusted-operator
/// deployments where the divergence is operator-acknowledged.
fn debug_seed_api_enabled_from_env() -> bool {
    // Reuse the shared boolean-flag parser (`1`/`true`, default off).
    nft_mutation_api_enabled_from_value(std::env::var(DEBUG_SEED_API_ENV).ok().as_deref())
}

/// The operator-facing text for a #2035 genesis-identity refusal.
///
/// Extracted from `main()` so it is reachable by tests. It is deliberately NOT
/// behind `#[cfg(feature = "p2p")]` even though its only caller is: the CI job
/// that actually executes `--bin launch_api` runs it with **default** features
/// (`ci-readiness.yml`), while the `--features p2p` job is name-filtered. A
/// message defined inside the p2p block would be tested by nothing.
///
/// # Why the wording matters (#2527)
///
/// The original text said the node "built or imported the wrong genesis" — true
/// for one of the two ways to reach this, and actively misleading for the other.
/// A genesis cut that has not yet reached the runbook's re-pin step produces a
/// chain that is entirely CORRECT against a pin that is STALE, and the operator
/// is told they did something wrong when they did not.
///
/// The timing makes that worse: the refusal fires on **first restart**, not at
/// cut time (the check is skipped while the chain is empty and validates on the
/// next boot), which is exactly when someone is running a restart or WAL-recovery
/// drill. Found by Alice during the genesis-cut rehearsal.
/// Unused under default features — its only caller is `#[cfg(feature = "p2p")]`.
/// That is the deliberate trade: defined out here so the default-features test
/// job can reach it, at the cost of one dead-code allow in that configuration.
#[cfg_attr(not(feature = "p2p"), allow(dead_code))]
fn genesis_hash_mismatch_guidance(local: &str, expected: &str) -> String {
    format!(
        "Genesis hash mismatch: local chain genesis is {local} but this node \
         expects {expected} (from WRFCOIN_GENESIS_HASH / \
         WRFCOIN_NETWORK_ID=testnet). Refusing to start — a node whose genesis \
         differs from its peers' fails the p2p genesis handshake against any \
         peer that receives it (#2035). Not necessarily all of them: the \
         handshake is published once at connection establishment and never \
         retried, so a peer that never receives it never checks (#2544).\n\
         \n\
         There are two ways to reach this and they need OPPOSITE fixes:\n\
         \n\
         (a) This node built or imported the WRONG genesis. Import the canonical \
         one (see docs JOIN_TESTNET) rather than generating one.\n\
         \n\
         (b) You have just CUT a new genesis and have not yet re-pinned \
         (genesis-cut runbook step 6). The chain is correct and the EXPECTATION \
         is stale — set WRFCOIN_GENESIS_HASH to this chain's genesis, or \
         complete the re-pin. This is the common case on a first restart after a \
         cut.\n\
         \n\
         Note: this check runs AFTER WAL replay and the producer-authority boot \
         check have both completed, so reaching it means those SUCCEEDED. It is \
         not a recovery failure — do not go looking at the WAL (#2527)."
    )
}

fn api_bind_warning(host: &str) -> Option<&'static str> {
    if host == "0.0.0.0" {
        Some(EXPOSED_API_BIND_WARNING)
    } else {
        None
    }
}

/// Build a `Cors` middleware instance from explicit parameters. Separating
/// env-reading from cors-building makes the production logic unit-testable
/// without env var manipulation (refs #1828).
///
/// - `origins_env`: value of `LAUNCH_API_CORS_ORIGINS`, or `None` if unset.
/// - `is_production`: true when `NODE_ENV=production`.
///
/// Fail-closed: when `is_production` is true and `origins_env` is `None`, all
/// cross-origin browser requests are denied. A sentinel origin that no real
/// browser can match switches actix-cors from permissive to restrictive mode.
fn build_cors_from_env(origins_env: Option<&str>, is_production: bool) -> Cors {
    // Logged at most once across all worker threads.
    static PROD_MISCFG_ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();

    let cors_base = Cors::default()
        .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
        .allowed_headers(vec![
            http::header::CONTENT_TYPE,
            http::header::AUTHORIZATION,
            http::header::HeaderName::from_static("x-requested-with"),
        ])
        .max_age(3600);

    match (origins_env, is_production) {
        (Some(val), _) => {
            // Explicit allow-list: honour it regardless of environment.
            let mut cors = cors_base;
            for origin in val.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                cors = cors.allowed_origin(origin);
            }
            cors
        }
        (None, true) => {
            // Production without an explicit allow-list: fail-closed.
            PROD_MISCFG_ONCE.get_or_init(|| {
                log::error!(
                    "CORS fail-closed: NODE_ENV=production but LAUNCH_API_CORS_ORIGINS is not \
                     set. All cross-origin browser requests will be rejected. \
                     Set LAUNCH_API_CORS_ORIGINS to the public frontend URL(s). Refs #1828."
                );
            });
            cors_base.allowed_origin("http://no-cors.invalid")
        }
        (None, false) => {
            // Development/staging default: allow both standard local dev ports.
            cors_base
                .allowed_origin("http://localhost:3000")
                .allowed_origin("http://localhost:5173")
        }
    }
}

/// Read CORS config from the environment and delegate to `build_cors_from_env`.
///
/// Called inside `HttpServer::new(move || { ... })` so each worker thread gets
/// its own `Cors` instance as required by actix-web's app factory pattern.
fn build_cors() -> Cors {
    let origins_env = std::env::var("LAUNCH_API_CORS_ORIGINS").ok();
    let is_production = std::env::var("NODE_ENV")
        .map(|v| v.eq_ignore_ascii_case("production"))
        .unwrap_or(false);
    build_cors_from_env(origins_env.as_deref(), is_production)
}

#[cfg(feature = "zk")]
fn default_zk_max_provers(available_parallelism: usize) -> usize {
    available_parallelism.saturating_sub(2).max(1)
}

#[cfg(feature = "zk")]
fn zk_max_provers_from_override(override_raw: Option<&str>, available_parallelism: usize) -> usize {
    let default = default_zk_max_provers(available_parallelism);
    let Some(raw) = override_raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return default;
    };

    match raw.parse::<usize>() {
        Ok(value) if value > 0 => value,
        _ => {
            log::warn!(
                "Invalid WRFCOIN_ZK_MAX_PROVERS='{raw}', falling back to default budget of {default}"
            );
            default
        },
    }
}

#[cfg(feature = "zk")]
fn zk_max_provers_from_env() -> usize {
    let available_parallelism = std::thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1);
    zk_max_provers_from_override(
        std::env::var("WRFCOIN_ZK_MAX_PROVERS").ok().as_deref(),
        available_parallelism,
    )
}

#[cfg(feature = "privacy")]
fn mlsag_verify_budget_ms_from_env() -> u64 {
    std::env::var("WRFCOIN_MLSAG_VERIFY_BUDGET_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(wrfcoin_privacy::MLSAG_VERIFY_BUDGET_MS_DEFAULT)
}

#[cfg(feature = "privacy")]
fn spawn_privacy_key_eviction_task(state: &web::Data<AppState>) {
    let privacy_registry = state.privacy_key_registry.clone();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
            let now = chrono::Utc::now().timestamp();
            let mut guard = privacy_registry.write().await;
            let before = guard.len();
            guard.retain(|_, (expires_at, _)| *expires_at > now);
            let evicted = before.saturating_sub(guard.len());
            if evicted > 0 {
                log::debug!("Evicted {evicted} expired privacy key(s) from registry");
            }
        }
    });
}

fn launch_api_json_config() -> web::JsonConfig {
    web::JsonConfig::default()
        .limit(MAX_JSON_PAYLOAD_BYTES)
        .error_handler(|err, req| {
            let message = match &err {
                JsonPayloadError::OverflowKnownLength { .. }
                | JsonPayloadError::Overflow { .. } => {
                    format!("JSON payload exceeds {MAX_JSON_PAYLOAD_BYTES} bytes")
                },
                _ => format!("Invalid JSON payload: {err}"),
            };
            let response = envelope_error(req, StatusCode::BAD_REQUEST, "BAD_REQUEST", message);
            InternalError::from_response("", response).into()
        })
}

#[cfg(feature = "p2p")]
fn build_p2p_config(
    listen_address_raw: &str,
    bootstrap_peers_env: Option<&str>,
) -> Result<P2pConfig, StartupConfigError> {
    let listen_address = listen_address_raw.parse::<Multiaddr>().map_err(|err| {
        StartupConfigError::InvalidListenMultiaddr {
            listen_address: listen_address_raw.to_string(),
            reason: err.to_string(),
        }
    })?;
    let bootstrap_peers = parse_p2p_addr_list(
        "WRFCOIN_P2P_BOOTSTRAP",
        bootstrap_peers_env,
    )?;
    // determinism-ok: startup-only relay address list for the P2P transport; parsed and validated before block processing begins, never enters state
    let relay_raw = std::env::var("WRFCOIN_P2P_RELAY_ADDRESSES").ok();
    let relay_addresses = parse_p2p_addr_list(
        "WRFCOIN_P2P_RELAY_ADDRESSES",
        relay_raw.as_deref(),
    )?;
    let mut config = P2pConfig::with_listen(listen_address).set_bootstrap_peers(bootstrap_peers);
    config.relay_addresses = relay_addresses;
    // Fail-closed external-address validation (#2819). P2pConfig's Default
    // parses these env vars with `.ok()`, so an operator typo silently became
    // "no external address" and the node advertised only bridge/loopback
    // listen addresses. Unset stays None; set-but-unparseable refuses startup.
    config.external_address =
        parse_required_external_addr("WRFCOIN_P2P_EXTERNAL_ADDR")?;
    config.quic_external_address =
        parse_required_external_addr("WRFCOIN_P2P_QUIC_EXTERNAL_ADDR")?;
    // #2120 boot-path wiring: read WRFCOIN_P2P_NODE_ROLE / WRFCOIN_P2P_SENTRY_ALLOWLIST
    // into the config HERE, before `P2pNode::start_*` consumes it, so the
    // inbound sentry-allowlist acceptance rule runs on real connections. A
    // declared role that cannot be honoured (validator without an allowlist,
    // misspelled role word, malformed peer id) is a typed startup refusal: the
    // P2P start failure below is logged and the node carries on WITHOUT P2P,
    // which for a topology role is exactly the silent fail-open this closes.
    config
        .apply_role_env()
        .map_err(|reason| StartupConfigError::InvalidP2pRole { reason })?;
    if config.role != wrfcoin_p2p::config::NodeRole::Public {
        config
            .validate()
            .map_err(|err| StartupConfigError::InvalidP2pRole {
                reason: format!("role {} refused by P2pConfig::validate: {err}", config.role.as_str()),
            })?;
    }
    Ok(config)
}

#[cfg(feature = "p2p")]
fn parse_p2p_addr_list(
    env_var: &'static str,
    raw_list: Option<&str>,
) -> Result<Vec<Multiaddr>, StartupConfigError> {
    let mut addresses = Vec::new();
    for (index, raw) in raw_list.unwrap_or_default().split(',').enumerate() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let addr = trimmed.parse::<Multiaddr>().map_err(|_| {
            StartupConfigError::InvalidPeerAddress {
                env_var,
                entry_index: index + 1,
            }
        })?;
        addresses.push(addr);
    }
    Ok(addresses)
}

#[cfg(feature = "p2p")]
fn p2p_port_from_env(env_var: &'static str, default: u16) -> Result<u16, StartupConfigError> {
    // determinism-ok: startup-only P2P listen-port configuration; parsed before block processing begins and never enters consensus state
    match std::env::var(env_var) {
        Err(_) => Ok(default),
        Ok(raw) if raw.trim().is_empty() => Ok(default),
        Ok(raw) => raw
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .ok_or(StartupConfigError::InvalidPort { env_var }),
    }
}

/// Parse an optional external-address env var, failing closed on a value that
/// is set but not a valid multiaddr (#2819). Empty/whitespace counts as unset
/// so compose files may pass through an empty default.
#[cfg(feature = "p2p")]
fn parse_required_external_addr(
    env_var: &'static str,
) -> Result<Option<Multiaddr>, StartupConfigError> {
    // determinism-ok: startup-only advertisement config, read from build_p2p_config at boot; external multiaddrs never enter block validity, a signing preimage, or the state root
    match std::env::var(env_var) {
        Err(_) => Ok(None),
        Ok(raw) if raw.trim().is_empty() => Ok(None),
        Ok(raw) => raw
            .trim()
            .parse::<Multiaddr>()
            .map(Some)
            .map_err(|_| StartupConfigError::InvalidExternalAddress { env_var }),
    }
}

/// Resolve the genesis block hash once at boot for the `/network/info`
/// chain-identity field (#1714).
///
/// IMPORTANT: this chain's genesis is the **height-1** block (anchored on
/// `height == 1 && prev_block_hash == Hash::default()` in `chain.rs`); there is
/// no height-0 block. The genesis hash also folds in a wall-clock timestamp via
/// `Block::calculate_hash`, so it cannot be recomputed from a spec — it must be
/// read from the persisted genesis block.
///
/// We resolve it once here rather than per-request because the per-request
/// handler used `Blockchain::block_at_height(0)`, which (a) targeted the wrong
/// height and (b) reads only the ~200-block in-memory window. On a long-running
/// node the genesis block is evicted from that window, so the lookup returned
/// `None` → `"unknown"`.
///
/// Resolution order:
/// 1. The genesis block if it is still in the in-memory window (fresh/short
///    chains and tests): the lowest in-memory block when it is the height-1
///    genesis anchor.
/// 2. The lowest-height block in the durable archive (`Storage::load_lowest_block`).
///    The archive never drops its lowest blocks, so this is the genesis block
///    regardless of how far the chain has advanced.
///
/// Returns `None` only when neither source has a block (an empty,
/// never-initialized chain), in which case `/network/info` reports `"unknown"`.
// Implementation lives in helpers.rs so the route modules (which the test
// harness compiles under a different crate root) can reach it too (#2797).
pub(crate) use crate::helpers::resolve_genesis_hash;

/// Outcome of resolving the chain-wide protocol metadata recorded in genesis.
///
/// Three states, and keeping them distinct is the entire point: only a
/// genuinely empty chain may default. "I could not find it" must never be
/// silently treated as "it is not set".
#[derive(Debug, PartialEq, Eq)]
enum GenesisProtocolMetadata {
    /// No blocks anywhere — genuine first boot. Defaulting is correct here.
    EmptyChain,
    /// Genesis loaded from the durable archive.
    Loaded(Vec<(String, String)>),
    /// The chain is not empty but authoritative genesis metadata could not be
    /// read. The node must refuse to start.
    Unavailable(String),
}

/// Resolve the genesis block's metadata from **durable storage only** (#2123).
///
/// # Why not the in-memory window
///
/// The previous version read `blockchain.block_at_height(1)`, which is
/// `self.blocks.get(&1)` — the in-memory window and nothing else.
/// `Storage::load_state` restores only the most recent configured window
/// (default 200) and `Blockchain::add_block` evicts the oldest block, so on any
/// chain past ~200 blocks a restart handed the startup gate **empty** metadata.
/// `resolve_active_version(None, None, ..)` then returned V1 and armed it, and
/// the node started while its still-running peers stayed on V2 — the #2076
/// state, reached with no misconfiguration and no operator error. Just a
/// restart.
///
/// The deeper lesson, and the reason this reads durable storage *exclusively*
/// rather than checking memory first: an answer sourced from a cache depends on
/// how long the process has been running. A consensus-critical question must
/// not have an uptime-dependent answer. `load_lowest_block` is one indexed read
/// at boot, so there is nothing to optimise and every reason not to.
///
/// The loaded block is checked to actually be the height-1 genesis anchor. If
/// the archive's lowest block is anything else, that is `Unavailable` — a
/// guessed version is worse than a refusal.
fn resolve_genesis_protocol_metadata(
    blockchain: &Blockchain,
    storage: &Storage,
) -> GenesisProtocolMetadata {
    match storage.load_lowest_block() {
        Ok(Some(block)) => {
            if block.height != 1 {
                return GenesisProtocolMetadata::Unavailable(format!(
                    "lowest persisted block is height {}, not the height-1 genesis anchor; \
                     refusing to infer the chain's protocol version from it",
                    block.height
                ));
            }
            GenesisProtocolMetadata::Loaded(
                block
                    .metadata
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        },
        Ok(None) => {
            // Storage holds no blocks. Only a genuinely empty chain may default;
            // if the node has blocks in memory, storage is inconsistent and we
            // must not guess. (#3301: boot now calls this BEFORE loading, with
            // an empty pre-load chain, so this in-memory branch is unreachable
            // from boot; the pre-load `archive_empty_but_height_recorded`
            // refusal covers that inconsistency instead. Kept for the unit
            // tests that drive this function with a loaded chain.)
            if blockchain.in_memory_block_count() == 0 && blockchain.height == 0 {
                GenesisProtocolMetadata::EmptyChain
            } else {
                GenesisProtocolMetadata::Unavailable(
                    "durable archive is empty but the chain has blocks in memory — \
                     storage is inconsistent"
                        .to_string(),
                )
            }
        },
        Err(e) => GenesisProtocolMetadata::Unavailable(format!(
            "failed to read the genesis block from durable storage: {e}"
        )),
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Start the uptime clocks at boot, not at the first request (#3599).
    routes::chain::record_process_start();
    routes::network::record_process_start();

    // Initialise tracing subscriber.
    // With --features tokio-console: connects tokio-console (run `tokio-console` in a second
    // terminal) for live async task inspection.
    // Default: pretty tracing output filtered by RUST_LOG (falls back to "info").
    #[cfg(feature = "tokio-console")]
    console_subscriber::init();

    #[cfg(not(feature = "tokio-console"))]
    {
        use tracing_subscriber::{EnvFilter, fmt, prelude::*};
        tracing_subscriber::registry()
            .with(fmt::layer())
            .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
            .init();
    }

    // Keep env_logger as a fallback for code that still calls log:: macros directly.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    let node_role = node_role_from_env().map_err(|message| {
        log::error!("REFUSING TO START: {message}");
        std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
    })?;

    // #2713 fail-closed chain-identity declaration, BEFORE anything reads or
    // reports an identity.
    //
    // With WRFCOIN_CHAIN_ID / WRFCOIN_NETWORK_ID unset this node REPORTS
    // `chain_id 777` / `network_id unknown` (inline literals at
    // `routes/network.rs:191-192` and `helpers.rs:1039-1041`) while every signature
    // it produces is bound to `wrfcoin-main` / `mainnet`, because
    // `signing_chain_id()` is what folds into the signing preimage. Nothing
    // compares the two, so the split persists with no diagnostic.
    //
    // REFUSING rather than defaulting is the point, and the direction matters more
    // than the check. Defaulting the unset case to testnet would be the inverse
    // mistake #2523 deleted GENESIS_ORACLE_SET_ENV for, where "unset" validated
    // nothing and said nothing. A node that will not start is recoverable in
    // thirty seconds; a node that starts and signs under the wrong domain is not —
    // and a wrong-commit node has been measured to be invisible to the chain
    // (#2056), so nothing downstream would surface it.
    //
    // It also closes the compounding in #2717. WRFCOIN_NETWORK_ID is one of only
    // two ways the #2035 genesis-identity check further down acquires a
    // *committing expectation* (the other is an explicit WRFCOIN_GENESIS_HASH).
    // Unset, that check has nothing to enforce and is skipped — so the SAME unset
    // variable that makes this node sign under the mainnet domain also disables the
    // guard that would have caught a data volume from another chain. One unset
    // variable, two hazards, the second removing the detector for the first.
    //
    // NO BEHAVIOUR CHANGE for a correctly configured node: with both variables set
    // and non-empty this resolves them and continues, and every downstream caller
    // of signing_chain_id()/signing_network_id() sees exactly what it saw before.
    // A startup precondition is not a validity predicate — it takes no part in
    // block validation, state transitions, or state_root — so this is not a
    // consensus change and cannot fork a chain.
    let declared_identity =
        wrfcoin_shared_protocol::crypto::domain_signing::declared_chain_identity().map_err(
            |message| {
                log::error!("REFUSING TO START: {message}");
                std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
            },
        )?;

    // Run pre-flight diagnostics.
    let preflight_config = helpers::PreflightConfig::from_env();
    helpers::print_startup_banner(&preflight_config);
    log::info!(
        "Chain identity DECLARED (not defaulted): chain_id={} network_id={} (#2713)",
        declared_identity.chain_id,
        declared_identity.network_id,
    );

    if declared_identity.chain_id == "777" {
        log::warn!("This is a test network. It may be reset without notice. Balances and tokens have no monetary value. No migration to mainnet is promised or implied. Please attach no production bridges, exchange listings or irreplaceable assets. https://wrfcoin-testnet.taila1c049.ts.net/testnet-disclaimer.html");
    }

    // #2712 fail-closed oracle-governance allowlist declaration.
    //
    // `oracle_governance_pubkeys()` splits a comma list with no cardinality check,
    // so a rotation performed as `=<old>,<new>` (APPEND) rather than `=<new>`
    // (REPLACE) leaves the retired key fully authorized and nothing reports it.
    // Rotation is exactly when a human appends: the intent is "add the new key",
    // and appending is the literal expression of it.
    //
    // What is refused is the AMBIGUOUS state, not the multi-key state. A two-entry
    // allowlist is legitimate — and is also precisely what an appending rotation
    // produces, and the two are indistinguishable from the value alone. So more
    // than one key requires an explicit WRFCOIN_ORACLE_GOVERNANCE_PUBKEY_COUNT;
    // one key or none does not, which is what keeps a correctly configured node
    // unchanged (every setter in this workspace configures exactly one key, and
    // scripts/verify-oracle-governance-allowlist.sh cannot even express a
    // multi-key expectation).
    //
    // Startup only, never in the resolver: `oracle_governance_pubkeys()` is called
    // per transaction by `verify_oracle_registration_authorization`, on the
    // producer's self-screen and on every peer's `verify_block`. Refusing there
    // would make this node reject registrations its peers accept — a partition.
    let governance_key_count =
        wrfcoin_shared_protocol::crypto::domain_signing::check_oracle_governance_allowlist_declaration()
            .map_err(|message| {
                log::error!("REFUSING TO START: {message}");
                std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
            })?;
    // Report the resolved SIZE unconditionally. #2712 notes that nothing anywhere
    // reports it; this is the weakest of its three suggested directions and is not
    // sufficient alone — an operator mid-rotation has no expectation to compare a
    // number against — but combined with the refusal above it makes the resolved
    // set visible in a log an incident reads later.
    log::info!(
        "Oracle-governance allowlist resolved: {governance_key_count} key(s) (#2712)"
    );
    let preflight_report = helpers::run_preflight_checks(&preflight_config);
    helpers::print_preflight_report(&preflight_report);
    if preflight_report.has_failures() {
        println!("  FATAL: One or more pre-flight checks failed. Exiting.");
        std::process::exit(1);
    }

    // Fail fast on missing auth tokens BEFORE expensive Storage, P2P, and ZK init.
    let token_store = load_token_store();
    let mempool_config = mempool_config_from_env().map_err(|message| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid mempool configuration: {message}"),
        )
    })?;

    let host = preflight_config.api_host;
    let port = preflight_config.api_port;
    let data_dir = preflight_config.data_dir;
    if let Some(warning) = api_bind_warning(&host) {
        log::warn!("{warning}");
    }
    let db_path = std::path::Path::new(&data_dir).join("wrfcoin.redb");

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            std::io::Error::other(format!("Failed to create data directory {parent:?}: {e}"))
        })?;
    }

    let checkpoint_interval = std::env::var("WRFCOIN_CHECKPOINT_INTERVAL")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_CHECKPOINT_INTERVAL);
    log::info!(
        "Checkpoint interval: {} blocks (override via WRFCOIN_CHECKPOINT_INTERVAL)",
        checkpoint_interval
    );

    // #2238 review: the storage key's real owner is a zeroizing container held
    // through its entire lifecycle — read here, used at `open_with_encryption`
    // below — not a plain `String` that outlives a disposable audit copy. When
    // this binding drops at the end of run(), the plaintext heap buffer is
    // wiped with optimizer-resistant volatile writes. (The process's copy in
    // the OS environment block is outside our control and is not claimed to be
    // cleared.)
    let encryption_key: Option<wrfcoin_shared_protocol::security::key_roles::Zeroizing<String>> =
        std::env::var("WRFCOIN_STORAGE_ENCRYPTION_KEY").ok().map(Into::into);

    // #2128: fail-closed key-role separation over the keys this node ACTUALLY
    // loads.
    //
    // The roles here were enumerated from the key material `launch_api` reads at
    // boot, not transcribed from a design doc — which is the correction the
    // review of PR #2238 asked for. Doing that enumeration turned up
    // `StorageEncryption`, a real secret loaded every boot that the original
    // six-role list had no entry for at all.
    //
    // Separation matters because a single compromise must not span roles: the
    // key that signs blocks must not also be the key that decrypts the datastore.
    // Roles this deployment does not configure are simply absent — asserting on
    // keys a node does not have would refuse every node.
    {
        use wrfcoin_shared_protocol::security::key_roles::{
            KeyRole, assert_loaded_key_roles_distinct,
        };

        let mut loaded: Vec<(KeyRole, Vec<u8>)> = Vec::new();
        if let Some(key) = encryption_key.as_ref() {
            loaded.push((KeyRole::StorageEncryption, key.as_bytes().to_vec()));
        }
        if let Ok(signer_key) =
            std::env::var(protocol_signer::env_vars::SNAPSHOT_SIGNER_PRIVATE_KEY)
        {
            loaded.push((KeyRole::Consensus, signer_key.into_bytes()));
        }

        match assert_loaded_key_roles_distinct(loaded) {
            Ok(registry) => {
                log::info!(
                    "[keys] role separation verified across {} loaded role(s) (#2128)",
                    registry.registered_roles().len()
                );
                // `registry` drops here; its handles are ZeroizeOnDrop, so the
                // copies taken above are erased rather than left in freed heap.
            },
            Err(violation) => {
                log::error!(
                    "Refusing to start: {violation}. A single compromise would span \
                     both roles — for example the key that signs blocks would also \
                     decrypt the datastore. Generate distinct key material per role \
                     (#2128)."
                );
                return Err(std::io::Error::other(format!(
                    "key role separation violated: {violation} (#2128)"
                )));
            },
        }
    }
    if encryption_key.is_none() {
        let allow_unencrypted = std::env::var("ALLOW_UNENCRYPTED_STORAGE")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);
        if allow_unencrypted {
            log::warn!(
                "ALLOW_UNENCRYPTED_STORAGE is set — storage encryption is disabled. \
                 This is acceptable for local development but MUST NOT be used in production."
            );
        } else {
            log::error!("{STORAGE_ENCRYPTION_DISABLED_WARNING}");
            return Err(std::io::Error::other(STORAGE_ENCRYPTION_DISABLED_WARNING));
        }
    }

    let storage =
        // Borrow, do not move: the zeroizing owner keeps ownership so it wipes
        // when run() returns; `open_with_encryption` derives its cipher key
        // from the borrow and retains no plaintext (#2238).
        Storage::open_with_encryption(
            &db_path,
            encryption_key.as_ref().map(|k| k.as_str()),
            checkpoint_interval,
        )
            .map_err(|e| {
                std::io::Error::other(format!("Failed to open storage at {db_path:?}: {e}"))
            })?;

    // #1218: open the durable equivocation-evidence store so double-sign
    // proofs survive node restart. This is wired into AppState so that
    // whichever consensus engine produces equivocation evidence (today
    // PoP does not; tomorrow PoS will via
    // `ConsensusFactory::create_pos_with_evidence_store`) can persist
    // through to disk. Held as `Arc<dyn EvidenceStore>` so multiple
    // engines or audit tooling can share the same backend.
    let evidence_store: Arc<dyn wrfcoin_consensus_engines::equivocation::EvidenceStore> =
        match DurableEvidenceStore::open_in_data_dir(&data_dir) {
            Ok(store) => {
                log::info!(
                    "Equivocation evidence store opened at {} (#1218 durable slashing evidence)",
                    store.path().display()
                );
                Arc::new(store)
            },
            Err(e) => {
                // Fail closed: on a production node the slashing-evidence
                // path being down is a launch-readiness blocker. Bail out
                // of startup rather than silently fall back to in-memory
                // and lose proofs on restart.
                log::error!(
                    "Failed to open equivocation evidence store in {data_dir}: {e}. \
                     Aborting startup — durable slashing evidence is a launch-readiness requirement (#1218)."
                );
                return Err(std::io::Error::other(format!(
                    "evidence store open failed: {e}"
                )));
            },
        };

    // #2123: fail-closed signing-protocol-version gate.
    //
    // Sits beside the #2035 genesis check for the same reason: both refuse to
    // start rather than let a node diverge silently. This one stops a single
    // node reaching protocol v2 from local configuration while its peers and
    // external signers stay v1 — the #2076 asymmetry, which halted the earn
    // loop for 28 hours because nothing errored; signatures simply stopped
    // verifying, which is indistinguishable from invalid signatures.
    //
    // Genesis metadata is the source of truth. `WRFCOIN_SIGNING_PROTOCOL_VERSION`
    // may only *agree* with it; disagreeing — or asking for v2 on a chain whose
    // genesis does not pin v2 — is a refusal. Runs before block production.
    //
    // #3301: this gate runs BEFORE `storage.load_state()`. Loading replays the
    // WAL (checkpoint + per-block transitions + finalization), and replay must
    // run under the chain's real signing version, activation table and feature
    // schedule — the ones this gate installs from durable block 1. Run after
    // the load, a restart re-applied every block past a feature's activation
    // under pre-activation rules (`feature_active` = false for everything),
    // then refused to boot on the #2219 tip commitment. With nothing
    // scheduled (777/testnet today) the installed values equal the defaults
    // replay used before, so that boot is unchanged.
    //
    // The metadata comes from `storage.load_lowest_block()` alone; the
    // in-memory chain is empty before the load by construction, and the
    // "storage empty but memory has blocks" inconsistency it used to detect is
    // re-checked right after the load instead.
    let gate_and_load = wrfcoin_native_chain::storage::gate_then_load_state(&storage, |storage| {
        // #3301: the gate decides "uncut" from the durable archive alone, before
        // anything is loaded. An archive with no block but a recorded height is
        // inconsistent storage: refuse HERE, so nothing is loaded (let alone
        // replayed) under a configuration-derived protocol.
        match storage.archive_empty_but_height_recorded() {
            Ok(false) => {},
            Ok(true) => {
                log::error!(
                    "Refusing to start: the durable archive holds no blocks but storage records \
                     a persisted height — storage is inconsistent (#2123/#3301)"
                );
                return Err(std::io::Error::other(
                    "durable archive is empty but storage records a persisted height — storage \
                     is inconsistent (#2123/#3301)",
                ));
            },
            Err(e) => {
                return Err(std::io::Error::other(format!(
                    "failed to read the durable archive before loading state: {e} (#3301)"
                )));
            },
        }
        let pre_load_chain = Blockchain::default();
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            ChainIdentity, GENESIS_ACTIVATION_SCHEDULE_ENV, SIGNING_PROTOCOL_VERSION_ENV,
            SigningProtocolVersion, install_uncut_chain_protocol,
            resolve_uncut_chain_protocol_with_schedule,
        };

        // Resolved from DURABLE STORAGE, not the in-memory window. Reading
        // `block_at_height(1)` meant that once genesis was evicted — any chain
        // past the ~200-block retention window — a restart saw empty metadata,
        // defaulted to v1 and started, while peers stayed on v2. That is the
        // #2076 state reached by an ordinary restart.
        // #2545: is this chain not cut yet? That is NOT the same as "genesis
        // records no version", and collapsing the two is why the cut could not
        // activate V2. Captured before the metadata is flattened.
        let chain_is_uncut = matches!(
            resolve_genesis_protocol_metadata(&pre_load_chain, &storage),
            GenesisProtocolMetadata::EmptyChain
        );
        let genesis_metadata: Vec<(String, String)> =
            match resolve_genesis_protocol_metadata(&pre_load_chain, &storage) {
                GenesisProtocolMetadata::Loaded(metadata) => metadata,
                // Only a genuinely empty chain may default.
                GenesisProtocolMetadata::EmptyChain => {
                    log::info!(
                        "No blocks yet — this chain is not cut. Configuration decides the \
                         version genesis will RECORD at block 1 (#2545)"
                    );
                    Vec::new()
                },
                // Non-empty chain, metadata unreadable: refuse. Starting on a
                // guessed version is how one node silently diverges.
                GenesisProtocolMetadata::Unavailable(why) => {
                    log::error!(
                        "Refusing to start: cannot determine the chain's signing protocol \
                         version — {why}. The node will not guess, because guessing v1 on a \
                         v2 chain reproduces #2076 (signatures silently stop verifying). \
                         Restore the durable archive or re-import genesis (#2123)."
                    );
                    return Err(std::io::Error::other(format!(
                        "genesis protocol metadata unavailable on a non-empty chain: {why} \
                         (#2123)"
                    )));
                },
            };
        let pairs: Vec<(&str, &str)> = genesis_metadata
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let identity = ChainIdentity::from_config();
        // determinism-ok: startup checks this local request against durable genesis metadata, then installs the resolved process-wide version before replay or validation
        let configured = std::env::var(SIGNING_PROTOCOL_VERSION_ENV)
            .ok()
            .and_then(|raw| SigningProtocolVersion::parse(&raw));

        // #2545: on an uncut chain the operator is not overriding a chain-wide
        // decision, they are MAKING it — so configuration is authoritative and
        // block 1 must then record it. On a chain that already has blocks,
        // nothing changes: a genesis that pins no version still refuses V2,
        // which is the #2076 protection.
        // #3093: computed BEFORE the gate from `pairs`; consulted after
        // the gate has passed (a refused gate never reaches the deadline).
        let upgrade_required_by_genesis =
            wrfcoin_shared_protocol::crypto::protocol_support::upgrade_required_at(
                pairs.iter().copied(),
            );

        let gate_result = if chain_is_uncut {
            // Resolve AND INSTALL. The first draft only resolved, so the node
            // logged "v2 active" while `SigningProtocolVersion::active()` still
            // returned the V1 default — block 1 then recorded protocol_version
            // v1 on a node that believed it was V2. That is the #2076 asymmetry
            // reintroduced by the fix meant to prevent it, and only the live
            // cut caught it: the pure resolver's unit tests all passed.
            // #1396: the uncut path must arm the activation SCHEDULE as well as
            // the scalar version. `assert_startup_protocol_compatibility` (the
            // else-branch) installs both; this branch installed only the
            // version, so on a freshly cut chain `try_activation_table()`
            // returned None and p2p startup REFUSED — "no protocol activation
            // schedule installed" — leaving every node of a new cut with
            // networking silently disabled. #2500 added that refusal and #2556
            // added this branch on the same day; neither accounted for the
            // other.
            //
            // Resolved through `resolve_activation_schedule` rather than
            // hardcoding a table so the two branches cannot drift: on an uncut
            // chain the metadata is empty, so it yields the identity schedule
            // answering `resolved` at every height — byte-for-byte what a chain
            // with no schedule does. Version first, then table, matching the
            // arming order the else-branch documents.
            // #3092: the cut may declare an activation schedule; it is a
            // cut-time input to the WRITER of block 1 (validated here exactly
            // as a genesis-borne schedule, then stamped into genesis by
            // `stamp_genesis_chain_metadata`), never a validation reference —
            // on a chain with blocks the else-branch reads genesis and this
            // variable is not consulted.
            // determinism-ok: startup-only read of a cut declaration on an uncut chain; the value is recorded into block 1 and every later read is from the block
            let cut_schedule = std::env::var(GENESIS_ACTIVATION_SCHEDULE_ENV).ok();
            resolve_uncut_chain_protocol_with_schedule(
                configured,
                &identity,
                cut_schedule.as_deref(),
            )
            .map_err(|e| e.to_string())
            .map(|resolved| {
                // All halves arrive together by construction, so this branch
                // cannot arm a version without a schedule (nor a schedule
                // without its feature view). Version first, matching the
                // arming order the else-branch documents.
                if let Some(raw) = resolved.schedule_raw.as_deref() {
                    log::info!(
                        "Uncut chain: block 1 will record protocol_activation_schedule = {raw:?} \
                         (#3092)"
                    );
                }
                install_uncut_chain_protocol(resolved)
            })
        } else {
            // #3277 S5: durable block 1 (whose metadata is `pairs`, read
            // above) keys the version, the schedule, the per-chain bootstrap
            // and the schedule-extension authority. One read, fail-closed: a
            // storage error or a lowest block that is not block 1 refuses
            // boot (core4#3368 review P2-1/P3-2).
            wrfcoin_native_chain::upgrade::arm_block1_protocol_at_boot(
                &storage,
                configured,
                &identity,
            )
        };

        match gate_result {
            Ok(version) => {
                log::info!("Signing protocol {version} active for chain {identity} (#2123)")
            },
            Err(e) => {
                log::error!("{e}");
                return Err(std::io::Error::other(format!(
                    "signing protocol version gate failed: {e} (#2123)"
                )));
            },
        }

        Ok::<_, std::io::Error>((identity, upgrade_required_by_genesis))
    });
    let ((protocol_identity, upgrade_required_by_genesis), loaded_chain) =
        match gate_and_load {
            Ok(outcome) => outcome,
            Err(wrfcoin_native_chain::storage::GateThenLoadError::Gate(e)) => return Err(e),
            Err(wrfcoin_native_chain::storage::GateThenLoadError::Load(e)) => {
                log::error!(
                    "Failed to load state from storage: {e}. Refusing to start. \
                     If this references a legacy incompatible WAL entry, preserve the node-data volume \
                     and treat it as a persistence-format migration issue instead of a Docker startup issue."
                );
                return Err(std::io::Error::other(format!(
                    "storage state load failed: {e}"
                )));
            },
        };

    #[allow(unused_mut)] // mut is required when feature = "zk" is active
    let mut blockchain = match loaded_chain {
        Some(chain) => {
            log::info!("Restored blockchain from storage (height={})", chain.height);
            chain
        },
        None => {
            log::info!("No persisted state found, starting fresh blockchain");
            let mut chain = Blockchain::default();
            // #2290 enforce-at-genesis: seed the initial authorized producer set
            // so producer-auth is enforced from block 1. Members MUST be pinned +
            // identical across all nodes (they feed state_root). FAIL-CLOSED: a
            // config error aborts startup (never a silent empty/unenforced chain).
            let genesis_producers = genesis_producer_members().map_err(|e| {
                std::io::Error::other(format!("genesis producer set config invalid (#2290): {e}"))
            })?;
            if !genesis_producers.is_empty() {
                let n = genesis_producers.len();
                chain.state.seed_genesis_producer_set(genesis_producers).map_err(|e| {
                    std::io::Error::other(format!("genesis producer set invalid (#2290): {e:?}"))
                })?;
                chain.state.recalculate_state_root();
                log::info!(
                    "Seeded genesis producer set ({n} member(s)) — block-producer \
                     authentication enforced from block 1 (#2290)"
                );
            }
            // #2501: persist the genesis state as the height-0 replay anchor,
            // BEFORE any block exists. This branch is the only place the chain's
            // real genesis state is constructed, and that state is NOT
            // `State::new()` — the producer set just seeded above lives in it and
            // in no `StateTransition`, so the WAL cannot carry it.
            //
            // Without the anchor, a restart with no checkpoint replays a faithful
            // log onto a default genesis and loses the set.
            //
            // Reached when the node is stopped ABRUPTLY below height 100: the
            // periodic checkpoint fires only at multiples of
            // `DEFAULT_CHECKPOINT_INTERVAL` (100), and the graceful-shutdown
            // `force_checkpoint` below never ran. A clean shutdown leaves a
            // checkpoint at any height > 0 and does not reach this.
            //
            // FAIL-CLOSED, and deliberately so: if the anchor cannot be written,
            // the node's future ability to recover from its own WAL is already
            // gone. Better to refuse now, with an empty chain and nothing at
            // stake, than at the first abrupt restart.
            storage.persist_genesis_state(&chain.state).await.map_err(|e| {
                std::io::Error::other(format!(
                    "failed to persist the genesis replay anchor: {e} — refusing to \
                     start a chain that could not be recovered from its own WAL (#2501)"
                ))
            })?;
            chain
        },
    };
    log::info!(
        "In-memory block window: {} blocks (env WRFCOIN_MAX_IN_MEMORY_BLOCKS, default {})",
        blockchain.max_in_memory_blocks(),
        wrfcoin_shared_protocol::blockchain::chain::DEFAULT_MAX_IN_MEMORY_BLOCKS,
    );

    // Resolve the genesis hash once at boot so /network/info can always report
    // a stable chain identity even after the genesis (height-1) block is
    // evicted from the in-memory window (#1714). `storage` is still owned here,
    // before it is moved into AppState below.
    let genesis_hash = resolve_genesis_hash(&blockchain, &storage);
    match genesis_hash.as_deref() {
        Some(h) => log::info!("Genesis hash resolved at boot: {h}"),
        None => log::warn!(
            "Genesis hash could not be resolved at boot (the chain is empty — no \
             height-1 genesis anchor yet); /network/info will report \"unknown\""
        ),
    }

    {
        let identity = protocol_identity;
        // #3093: can this binary follow the chain's genesis schedule? The
        // lowest-height scheduled feature it does not implement is the
        // height at which it must stop CLEANLY (H − 1), never fork silently
        // (#1972 class). If the local tip is already there, refuse to start.
        {
            use wrfcoin_native_chain::upgrade::{
                install_upgrade_deadline, EXIT_CODE_UPGRADE_REQUIRED, UpgradeDeadline,
            };
            use wrfcoin_shared_protocol::crypto::protocol_support::{
                boot_refused, upgrade_required_message, PROTOCOL_VERSION,
            };
            match upgrade_required_by_genesis {
                Some(required) if boot_refused(&required, blockchain.height) => {
                    log::error!(
                        "REFUSING TO START: {} — the local tip is already at height {}, at or \
                         beyond the activation; this binary cannot even replay its own tip on the \
                         old rules",
                        upgrade_required_message(&required, blockchain.height),
                        blockchain.height
                    );
                    std::process::exit(EXIT_CODE_UPGRADE_REQUIRED);
                },
                Some(required) => {
                    log::warn!(
                        "UPGRADE REQUIRED by height {}: the genesis schedule activates feature {:?} \
                         there and this binary (protocol v{PROTOCOL_VERSION}) does not implement \
                         it. This node will produce, apply and relay through height {} and then \
                         stop cleanly (#3093). Install the release announced for that height \
                         before it is reached.",
                        required.height,
                        required.feature,
                        required.height.saturating_sub(1)
                    );
                    install_upgrade_deadline(Some(UpgradeDeadline::from_required(&required)));
                },
                None => install_upgrade_deadline(None),
            }
        }

        // #3277 S3: applied schedule extensions in the restored canonical state.
        // The WAL replay already refreshed the effective schedule block by
        // block (`gate_then_load_state`); this runs after the #3093 genesis
        // deadline is installed so an extension can only TIGHTEN it, and it
        // refuses to start exactly as the genesis check above does when the
        // tip is already at or past an activation this binary cannot follow.
        if let Some(deadline) = wrfcoin_native_chain::upgrade::refresh_schedule_from_canonical_state(
            &blockchain.state,
            "boot load",
        ) {
            if blockchain.height >= deadline.height {
                log::error!(
                    "REFUSING TO START: an applied feature-schedule extension activates feature \
                     {:?} at height {}, the local tip is already at height {}, and this binary \
                     (protocol v{}) does not implement it (#3277)",
                    deadline.feature,
                    deadline.height,
                    blockchain.height,
                    deadline.running_protocol_version
                );
                std::process::exit(wrfcoin_native_chain::upgrade::EXIT_CODE_UPGRADE_REQUIRED);
            }
        }

        // #3095: a governance-signed upgrade notice, if the operator placed
        // one on disk. Read here — after the #3093 genesis deadline is
        // installed — because a notice may only TIGHTEN that deadline, never
        // relax it. The file path is untrusted input: the notice changes
        // nothing until `verify_upgrade_notice` has passed it against this
        // chain's identity and the governance allowlist.
        {
            use wrfcoin_native_chain::upgrade_notice::{
                accept_upgrade_notice, read_notice_file, NOTICE_FILE_ENV,
            };
            // determinism-ok: startup read of an operator-supplied notice path; the notice is signature-verified before it changes anything and never enters state_root
            if let Ok(path) = std::env::var(NOTICE_FILE_ENV) {
                let path = path.trim().to_string();
                if !path.is_empty() {
                    let allowlist =
                        wrfcoin_shared_protocol::crypto::domain_signing::oracle_governance_pubkeys();
                    match read_notice_file(std::path::Path::new(&path)) {
                        Ok(signed) => {
                            let _ = accept_upgrade_notice(
                                signed,
                                &identity.chain_id,
                                &identity.network_id,
                                &allowlist,
                                // determinism-ok: boot-time expiry check on an operator-supplied notice; the value never enters a preimage, a state transition or the state root
                                chrono::Utc::now().timestamp(),
                                &format!("{NOTICE_FILE_ENV}={path}"),
                            );
                        },
                        Err(why) => log::warn!(
                            "Ignoring {NOTICE_FILE_ENV}={path}: {why} (#3095) — node behaviour is \
                             unchanged"
                        ),
                    }
                    // Whatever the outcome, say where the answer now lives.
                    log::info!(
                        "Upgrade notice status at boot: {} — see GET /network/upgrade (#3095)",
                        wrfcoin_native_chain::upgrade_notice::upgrade_status(blockchain.height)
                    );
                }
            }
        }

        // #3093 / #2157 rollback protection: never boot a KNOWN-VULNERABLE
        // older version on a volume a newer version has run. A plain
        // downgrade is allowed and logged — rolling an image back on purpose
        // is the #3091 recovery path. Recorded after the check so the next
        // boot sees this binary's version.
        {
            use wrfcoin_native_chain::upgrade::EXIT_CODE_ROLLBACK_REFUSED;
            use wrfcoin_shared_protocol::crypto::protocol_support::{
                rollback_decision, rollback_refused_message, RollbackDecision, PROTOCOL_VERSION,
            };
            let recorded = match storage.load_recorded_protocol_version() {
                Ok(recorded) => recorded,
                Err(e) => {
                    log::error!(
                        "REFUSING TO START: the recorded protocol version is unreadable ({e}); \
                         fail-closed, because a rollback onto a newer volume cannot be ruled out \
                         (#3093)"
                    );
                    std::process::exit(EXIT_CODE_ROLLBACK_REFUSED);
                },
            };
            match rollback_decision(recorded) {
                RollbackDecision::Refuse {
                    recorded,
                    running,
                    advisory,
                } => {
                    log::error!("REFUSING TO START: {}", rollback_refused_message(recorded, running, advisory));
                    std::process::exit(EXIT_CODE_ROLLBACK_REFUSED);
                },
                RollbackDecision::DowngradeNoted { recorded } => log::warn!(
                    "Protocol downgrade: this volume was last run by protocol v{recorded}, this \
                     binary is protocol v{PROTOCOL_VERSION} (not known vulnerable; proceeding, #3093)"
                ),
                RollbackDecision::Proceed => {},
            }
            storage.record_protocol_version(PROTOCOL_VERSION).map_err(|e| {
                std::io::Error::other(format!(
                    "failed to record protocol v{PROTOCOL_VERSION} on the volume: {e} (#3093)"
                ))
            })?;
        }
    }

    // #2219: whole-replay integrity gate. Deliberately here — AFTER the
    // protocol gate installed the signing version — and not inside
    // `Storage::load_state`, which runs before the gate and would evaluate at
    // the V1 default, silently skipping the check on every real V2 boot
    // (codex P1 on PR #2357). At V2, refusing to start from a state that does
    // not match the tip block's committed post-state root is the point.
    if let Err(e) = wrfcoin_native_chain::storage::verify_tip_state_root_commitment(&blockchain) {
        log::error!("{e}");
        return Err(std::io::Error::other(format!(
            "{} — refusing to start from divergent persisted state (#2219)",
            e.error_code()
        )));
    }

    // #2501: the deployment declared it requires producer auth — so the chain we
    // are about to run must actually have a set.
    //
    // Placed here, with the other producer-auth boot checks and AFTER the
    // protocol gate, because it is the same class: a property of the chain we
    // just loaded rather than of the config we just read.
    //
    // `resolve` (above, fresh-genesis branch only) already refuses when the flag
    // is set and no set resolves. This covers the branch it cannot see: a node
    // started against PERSISTED state never re-resolves, so a volume holding a
    // chain with no producer set reaches no guard at all — the set is not
    // "resolved empty", it is never resolved. Two ordinary ways to get there: a
    // cut that reuses a data volume (silently continuing the OLD chain
    // unenforced instead of starting the new one), and checkpoint-less WAL
    // recovery losing the set (#2501).
    //
    // Deliberately keyed on the OPERATOR'S DECLARED INTENT rather than on the
    // chain, because a reused legacy volume — unsigned blocks, genuinely empty
    // set — is indistinguishable from a legitimate legacy chain when you only
    // look at the chain. The flag is the missing information.
    {
        // determinism-ok: startup-only read of a deployment declaration; the value is consumed once here and never enters a state transition, block validation, or state_root.
        let raw = std::env::var(genesis_producers::REQUIRE_PRODUCER_AUTH_ENV).ok();
        if let Err(e) = genesis_producers::check_restored_chain_producer_set(
            raw.as_deref(),
            blockchain.state.producer_set_history.is_empty(),
        ) {
            log::error!("REFUSING TO START: {e}");
            return Err(std::io::Error::other(format!(
                "PRODUCER_AUTH_REQUIRED_BUT_UNSEEDED — {e}"
            )));
        }
    }

    // #2290: fail-closed BOOT RE-VERIFICATION of producer authority.
    //
    // `Blockchain::from_persisted` is a pure constructor and `load_state`
    // checks only the tip state-root commitment, so a restart inherits whatever
    // is on disk. Enforcement at the acceptance boundaries (gossip, IBD) and on
    // the production path (`/blocks/mine`) is necessary but not sufficient: it
    // only covers routes we have enumerated. This is the backstop for the ones
    // we have NOT — a chain that acquired an unauthorized block by ANY route,
    // including a downgrade to an older binary that lacked a gate, must refuse
    // to start rather than silently continue building on it.
    //
    // Same shared gate as everywhere else. Unenforced when the chain never
    // configured a producer set (empty history — legacy/dev), exactly as at the
    // other call sites, so this cannot brick an unconfigured chain.
    //
    // Placed AFTER `assert_startup_protocol_compatibility` for the same reason
    // the tip-commitment check is: running it inside `load_state` would evaluate
    // before the signing protocol version is installed.
    //
    // Scans the DURABLE BLOCK TABLE, not `blockchain.blocks()`. The first
    // version of this check used the in-memory window, which `load_state`
    // restores to at most `configured_max_in_memory_blocks()` entries (default
    // 200) — so on a 201-block chain with a 1-block window it examined ONE block
    // and silently trusted 200, including exactly the older-than-retention
    // unauthorized block it exists to catch. The scan is chunked
    // (`PERSISTED_BLOCK_SCAN_CHUNK`) so a long chain costs bounded memory.
    {
        let history = blockchain.state.producer_set_history.clone();
        match wrfcoin_native_chain::storage::verify_persisted_producer_authority(&storage, &history)
        {
            Err(e @ wrfcoin_native_chain::storage::PersistedProducerAuthorityError::Archive(_)) => {
                log::error!(
                    "REFUSING TO START: {e}. Fail-closed — an archive that cannot be scanned \
                     cannot be certified free of unauthorized blocks (#2290)."
                );
                return Err(std::io::Error::other(format!(
                    "PRODUCER_AUTH_PERSISTED_VIOLATION — {e}; refusing to start (#2290)"
                )));
            },
            Err(e) => {
                log::error!(
                    "REFUSING TO START: {e}. This chain contains a block the network would \
                     reject, so continuing would build on a fork (#2290)."
                );
                return Err(std::io::Error::other(format!(
                    "PRODUCER_AUTH_PERSISTED_VIOLATION — {e}; refusing to start (#2290)"
                )));
            },
            Ok(0) => {},
            Ok(n) => log::info!(
                "Producer-authority boot check passed: {n} persisted block(s) verified against \
                 the on-chain authorized producer set (#2290)"
            ),
        }
    }

    // #3091 write-after-verify: `load_state` may have migrated the latest
    // checkpoint from an older persistence format IN MEMORY. Only now — after
    // WAL replay, the #2219 tip commitment and the #2290 producer-authority
    // backstop all passed on the migrated state — is it written back. Until
    // this point the volume held exactly what the older binary wrote, so a
    // failed boot or an image rollback still reads the original bytes.
    //
    // FAIL-CLOSED on a write failure, for the same reason as the #2501 anchor:
    // a volume that cannot take this write cannot take the next block either,
    // and refusing now, before anything is at stake, beats failing mid-run.
    match storage.commit_migrated_checkpoints() {
        Ok(written) if written.is_empty() => {},
        Ok(written) => {
            for (height, from) in written {
                log::info!(
                    "storage: migrated checkpoint at height {height} from format {from} to {} \
                     (#3091) — in-place upgrade, no volume wipe",
                    wrfcoin_native_chain::storage::CHECKPOINT_FORMAT_VERSION
                );
            }
        },
        Err(e) => {
            log::error!(
                "REFUSING TO START: the migrated checkpoint could not be written back: {e} \
                 (#3091). The original bytes are untouched; roll the image back or fix the \
                 volume."
            );
            return Err(std::io::Error::other(format!(
                "checkpoint migration write-back failed: {e} (#3091)"
            )));
        },
    }

    // #2035: fail-closed genesis-identity check. The genesis block folds in a
    // wall-clock timestamp (see `resolve_genesis_hash` above / `block.rs`), so a
    // node that *builds* its own genesis instead of importing the canonical one
    // gets a different hash and then silently fails the p2p genesis handshake
    // against every peer ("why won't my node connect?" — the #1 predicted
    // launch-day support ticket). Assert at boot that the locally-resolved
    // genesis matches the pinned expectation — the same source of truth the
    // handshake uses (`wrfcoin_p2p::expected_genesis_hash`) — and refuse to
    // start with a clear, actionable message on mismatch.
    //
    // Fires only when BOTH a *committing* expectation exists (an explicit
    // `WRFCOIN_GENESIS_HASH`, or the pinned testnet hash under
    // `WRFCOIN_NETWORK_ID=testnet`) AND a local genesis actually resolved. A
    // fresh node with no block yet (`None`) is skipped — it will build/import
    // genesis and this check validates it on the next boot. Dev / mainnet with
    // nothing pinned (expectation `None`) is unaffected. Gated on `feature =
    // "p2p"` because genesis divergence only matters for a peering node.
    #[cfg(feature = "p2p")]
    {
        if let (Some(expected), Some(local)) =
            (wrfcoin_p2p::expected_genesis_hash(), genesis_hash.as_deref())
        {
            // `resolve_genesis_hash` returns a `0x`-prefixed hex; the pinned
            // constant / env are bare hex. Normalise both before comparing.
            let norm = |h: &str| h.trim_start_matches("0x").to_ascii_lowercase();
            if norm(&expected) != norm(local) {
                log::error!("{}", genesis_hash_mismatch_guidance(local, &expected));
                return Err(std::io::Error::other(format!(
                    "genesis hash mismatch: local {local} != expected {expected} (#2035)"
                )));
            }
            log::info!(
                "Genesis identity verified against pinned expectation {expected} (#2035)"
            );
        }
    }

    // INV-SUPPLY-1 (#2097): attach the supervisor handle to the state the node
    // actually runs on.
    //
    // `State::supply_invariant` is `#[serde(skip)]`, so the state just loaded
    // from storage has `None` there — the detector writes its latch into a
    // handle nobody holds and `check_supply_conservation` silently skips the
    // publish. This is the single line that makes the check reach a supervisor,
    // and it must run on the loaded `blockchain`, not on a fresh `State`: the
    // instance registered below has to be the same one the state writes to.
    //
    // Deliberately not feature-gated. Supply conservation is a property of the
    // ledger, not of p2p/zk/privacy, so a node built without those features
    // must still be watched.
    let supply_invariant = SharedSupplyInvariant::new();
    blockchain.state.supply_invariant = Some(supply_invariant.clone());

    #[cfg(feature = "zk")]
    {
        let zk_verifier: Arc<dyn wrfcoin_shared_protocol::consensus::zk_verification::ZkVerifier> =
            Arc::new(wrfcoin_native_chain::zk_service::ArkworksZkVerifier);
        blockchain.state.set_zk_verifier(zk_verifier);
    }
    #[cfg(feature = "zk")]
    {
        if let Err(e) = wrfcoin_native_chain::zk_service::prewarm_confidential_transfer_keys() {
            log::warn!("Failed to pre-warm ZK 1x1 keys: {e}");
        }
    }

    // Resolve P2P posture once. A configured network is required and every
    // identity/config/transport failure aborts startup. Only the explicit,
    // visibly SIMULATED developer profile may use an in-memory identity.
    let p2p_profile = p2p_profile_from_env().map_err(|message| {
        log::error!("REFUSING TO START: {message}");
        std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
    })?;

    #[cfg(not(feature = "p2p"))]
    let p2p_startup = match p2p_profile {
        P2pStartupProfile::Disabled => P2pStartupStatus::default(),
        P2pStartupProfile::Required | P2pStartupProfile::Developer => {
            let message = format!(
                "P2P profile '{}' is configured but this binary was compiled without the p2p feature",
                p2p_profile.label()
            );
            log::error!("REFUSING TO START: {message}");
            return Err(std::io::Error::other(message));
        },
    };

    #[cfg(feature = "p2p")]
    let (p2p_client, p2p_event_rx, p2p_summary_info, p2p_startup) = {
        if p2p_profile.is_enabled() {
            let p2p_port = p2p_port_from_env("WRFCOIN_P2P_PORT", 8555).map_err(|err| {
                log::error!("P2P startup config rejected (code={}): {err}", err.error_code());
                std::io::Error::new(std::io::ErrorKind::InvalidInput, err.to_string())
            })?;
            let quic_port = std::env::var("WRFCOIN_P2P_QUIC_ENABLED")
                .ok()
                .map(|raw| parse_security_switch("WRFCOIN_P2P_QUIC_ENABLED", Some(&raw)))
                .transpose()
                .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?
                .flatten()
                .unwrap_or(false)
                .then(|| p2p_port_from_env("WRFCOIN_P2P_QUIC_PORT", p2p_port))
                .transpose()
                .map_err(|err| {
                    log::error!("P2P startup config rejected (code={}): {err}", err.error_code());
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, err.to_string())
                })?;

            let bootstrap_peers_raw = std::env::var("WRFCOIN_P2P_BOOTSTRAP").ok();
            // determinism-ok: startup-only transport bind address; never block validity, identity, or state transitions
            let p2p_host: std::net::Ipv4Addr = std::env::var("WRFCOIN_P2P_HOST")
                .unwrap_or_else(|_| "0.0.0.0".into()).parse()
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid WRFCOIN_P2P_HOST"))?;
            let listen_address_raw = format!("/ip4/{p2p_host}/tcp/{p2p_port}");
            let identity_path = std::path::Path::new(&data_dir).join("peer_identity.key");
            let (keypair, identity_kind) = match wrfcoin_p2p::load_or_generate_keypair(&identity_path) {
                Ok(kp) => (kp, "persistent"),
                Err(error) if p2p_profile == P2pStartupProfile::Developer => {
                    log::warn!(
                        "SIMULATED developer P2P profile: persistent identity unavailable ({error}); using an ephemeral identity"
                    );
                    (libp2p::identity::Keypair::generate_ed25519(), "ephemeral")
                },
                Err(error) => {
                    let message = format!(
                        "required P2P identity could not be loaded or created at {}: {error}",
                        identity_path.display()
                    );
                    log::error!("REFUSING TO START: {message}");
                    return Err(std::io::Error::other(message));
                },
            };

            let initial_chain_state = wrfcoin_p2p::InitialChainState {
                height: blockchain.height,
                tip_hash: blockchain.tip.into(),
                state_root: blockchain.state.state_root.0,
            };
            let config = build_p2p_config(&listen_address_raw, bootstrap_peers_raw.as_deref())
                .map_err(|err| {
                    log::error!(
                        "P2P startup config rejected (code={}): {} — refusing to start",
                        err.error_code(),
                        err
                    );
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, err.to_string())
                })?;
            match P2pNode::start_with_keypair_and_initial_chain_state(
                    config,
                    keypair,
                    initial_chain_state,
                )
                .await
            {
                    Ok(handle) => {
                        let peer_id = handle.local_peer_id;
                        let peer_id_str = peer_id.to_string();
                        let (client, event_rx) = handle.into_parts();
                        if let Some(quic_port) = quic_port {
                            log::info!(
                                "P2P networking started on tcp/{p2p_port} and udp/{quic_port}/quic-v1 (peer_id={peer_id_str})"
                            );
                        } else {
                            log::info!(
                                "P2P networking started on tcp/{p2p_port} (peer_id={peer_id_str})"
                            );
                        }
                        helpers::print_p2p_ready(&peer_id_str, p2p_port, quic_port);
                        helpers::write_peer_id_file(&data_dir, &peer_id_str);
                        (
                            Some(client),
                            Some(event_rx),
                            Some((peer_id_str, p2p_port, quic_port)),
                            P2pStartupStatus {
                                state: "running",
                                profile: p2p_profile.label(),
                                required: p2p_profile == P2pStartupProfile::Required,
                                identity: identity_kind,
                                simulated: p2p_profile == P2pStartupProfile::Developer,
                                detail: None,
                            },
                        )
                    },
                    Err(error) => {
                        let message = format!("P2P transport failed to start: {error}");
                        log::error!("REFUSING TO START: {message}");
                        return Err(std::io::Error::other(message));
                    },
            }
        } else {
            log::info!("P2P networking disabled by resolved profile");
            (None, None, None, P2pStartupStatus::default())
        }
    };

    // Instantiate consensus engine (Proof-of-Publishing, static dispatch via TypedConsensusManager)
    let pop_config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
    let pop_adapter = ConsensusFactory::create_pop_adapter(&pop_config);
    if node_role != NodeRole::Producing {
        // #2860: a sync-only node validates and relays authenticated blocks but
        // never creates one. Disable production at the consensus adapter seam,
        // before any current or future caller can mutate chain/mempool state,
        // and deliberately do not resolve credentials this role cannot use.
        pop_adapter.set_block_production_enabled(false);
        log::info!(
            "[node-role] non-producing: block production disabled; producer signer not required"
        );
    }
    // #2290: inject the producer signer so blocks this node builds are signed.
    // Core consensus, not feature-gated. No signer ⇒ unsigned blocks (dev/genesis
    // / pre-activation), unless producer auth is active. In sync-only mode this
    // call validates auth posture but deliberately skips credential resolution.
    let producer_signer = load_producer_signer(&blockchain.state, node_role);
    let producer_authority_boot = check_next_producer_authority(
        &blockchain,
        producer_signer.as_ref(),
        node_role,
    )
    .map_err(|message| {
        log::error!("REFUSING TO START: {message}");
        std::io::Error::other(format!(
            "PRODUCER_NOT_AUTHORIZED_FOR_NEXT_BLOCK — {message}"
        ))
    })?;
    if let Some(producer_signer) = producer_signer {
        pop_adapter.set_producer_signer(producer_signer).await;
    }
    // #2290 §R2 / P1-3: bind the coinbase recipient to this producer's member
    // payout address, so a block that would pay anyone else is refused locally
    // instead of being rejected by every peer.
    if let Some(payout) = load_producer_payout_address() {
        log::info!(
            "[producer-payout] coinbase recipient is bound to {} (#2290 §R2)",
            payout.to_hex()
        );
        pop_adapter.set_producer_payout_address(&payout).await;
    }
    // Optional cut input: only block 1 records it. Followers and later blocks
    // use the persisted genesis value, never this process configuration.
    // determinism-ok: cut-only genesis input; block 1 records it as chain content, followers and later blocks read the persisted genesis value (same shape as cut_oracle_declaration_from_env, #2523)
    if let Ok(raw) = std::env::var("WRFCOIN_GENESIS_FAUCET_ADDRESS") {
        let address = wrfcoin_shared_protocol::common::types::Address::from_hex(&raw)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput,
                "invalid WRFCOIN_GENESIS_FAUCET_ADDRESS"))?;
        pop_adapter.set_genesis_faucet_address(address).await;
    } else {
        // #3361: required at the cut of any chain that funds a faucet slice.
        // Enforced where it matters, when this node tries to cut block 1; here
        // it is only a warning, so a follower is never held hostage to it.
        use wrfcoin_shared_protocol::consensus::proof_of_publishing::{EmissionPartition, EmissionRole};
        let chain_id = wrfcoin_shared_protocol::crypto::domain_signing::signing_chain_id();
        if EmissionPartition::at_height(1, Some(&chain_id)).slice(EmissionRole::Faucet) > 0 {
            log::warn!(
                "[genesis-faucet] no WRFCOIN_GENESIS_FAUCET_ADDRESS configured on chain {chain_id}, \
                 which funds a faucet slice. This node can sync and produce above height 1, but \
                 will REFUSE to cut a block 1 (#3361). Set it before a cut."
            );
        }
    }
    // Cut inputs only: persisted block 1 is the verification reference.
    let mut cut_policy = std::collections::BTreeMap::new();
    for (env, key) in [
        ("WRFCOIN_GENESIS_NODE_SHARE_EPOCH_BLOCKS", wrfcoin_shared_protocol::crypto::chain_binding::GENESIS_NODE_EPOCH_KEY),
        ("WRFCOIN_GENESIS_NODE_SHARE_CHALLENGE_BLOCKS", wrfcoin_shared_protocol::crypto::chain_binding::GENESIS_NODE_CHALLENGE_KEY),
        ("WRFCOIN_GENESIS_FLAT_REWARD_BASE_UNITS", wrfcoin_shared_protocol::crypto::chain_binding::GENESIS_FLAT_REWARD_UNITS_KEY),
        ("WRFCOIN_GENESIS_FLAT_REWARD_SECONDS", wrfcoin_shared_protocol::crypto::chain_binding::GENESIS_FLAT_REWARD_KEY),
        ("WRFCOIN_GENESIS_FAUCET_ALLOCATION_BASE_UNITS", wrfcoin_shared_protocol::crypto::chain_binding::GENESIS_FAUCET_ALLOCATION_KEY),
        ("WRFCOIN_GENESIS_SCHEDULE_GOVERNANCE_KEYS", wrfcoin_shared_protocol::crypto::chain_binding::GENESIS_GOVERNANCE_KEYS_KEY),
    ] {
        // determinism-ok: cut-only writer input recorded in block 1; never a verifier reference
        if let Ok(value) = std::env::var(env) { cut_policy.insert(key.to_string(), value); }
    }
    pop_adapter.set_genesis_policy_metadata(cut_policy).await;
    // #2523: resolve the cut's oracle posture and inject it, so a block 1 built
    // here declares the canonical set as CHAIN CONTENT. This is the only place
    // in the node that reads the oracle-set configuration; every validation —
    // here and on every peer — reads the declaration back out of the block.
    //
    // FAIL-CLOSED on a malformed value, and deliberately NOT fail-closed on an
    // absent one *here*: absence is refused at the moment it matters, when this
    // node tries to cut block 1, so a node that only ever syncs or produces
    // above height 1 is not held hostage to a cut variable it will never use.
    match wrfcoin_shared_protocol::crypto::genesis_oracle::cut_oracle_declaration_from_env() {
        Ok(Some((set, quorum))) => {
            log::info!(
                "[genesis-oracle] block 1 will declare oracle_set={set} oracle_quorum={quorum} \
                 (#2523 — genesis decides; local configuration may only agree)"
            );
            pop_adapter
                .set_genesis_oracle_declaration(set, quorum)
                .await;
        },
        Ok(None) => {
            log::warn!(
                "[genesis-oracle] no genesis oracle declaration configured. This node can sync \
                 and produce above height 1, but will REFUSE to cut a block 1 (#2523). Set \
                 WRFCOIN_GENESIS_ORACLE_SET before a cut."
            );
        },
        Err(e) => {
            log::error!(
                "[genesis-oracle] cut configuration is invalid: {e} Refusing to start rather \
                 than run with a declaration this node cannot honour."
            );
            return Err(std::io::Error::other(format!(
                "genesis oracle declaration invalid (#2523): {e}"
            )));
        },
    }

    #[cfg(feature = "zk")]
    let zk_verifier: Arc<dyn wrfcoin_shared_protocol::consensus::zk_verification::ZkVerifier> =
        Arc::new(wrfcoin_native_chain::zk_service::ArkworksZkVerifier);
    #[cfg(feature = "zk")]
    pop_adapter.set_zk_verifier(zk_verifier.clone()).await;
    #[cfg(feature = "zk")]
    blockchain.state.set_zk_verifier(zk_verifier.clone());
    #[cfg(feature = "privacy")]
    {
        let mlsag_budget = mlsag_verify_budget_ms_from_env();
        pop_adapter.set_mlsag_verify_budget_ms(mlsag_budget).await;
        log::info!("MLSAG verification budget set to {mlsag_budget}ms per block");
    }
    // The engine type is named explicitly, not inferred: #2603's hazard is a
    // silent substitution here, and an annotated binding turns that into an
    // edit of a line that says `PopConsensusAdapter` out loud. The helper's
    // monomorphic return type is what makes a swap *through* it a compile
    // error; this annotation is what makes a swap *around* it visible.
    let consensus_manager: TypedConsensusManager<wrfcoin_consensus_engines::PopConsensusAdapter> =
        launch_api_consensus_manager(pop_adapter, pop_config);
    log::info!(
        "Consensus engine started: {}",
        consensus_manager.algorithm_name()
    );
    #[cfg(feature = "zk")]
    let zk_max_provers = zk_max_provers_from_env();
    #[cfg(feature = "zk")]
    log::info!("ZK proof generation concurrency budget set to {zk_max_provers}");

    // BFT finality signal channel (#1216).
    //
    // The sender is stored in AppState so any future consensus source
    // (HotStuff commit path, BftFinalityGadget, manual admin override)
    // can clone it and push finalized heights into the chain's reorg
    // rejection guard. The receiver is owned exclusively by the
    // `run_finality_event_loop` task spawned below — there is a single
    // consumer so we do not need to multiplex.
    let (finality_signal_tx, finality_signal_rx) =
        wrfcoin_shared_protocol::consensus::finality::finality_signal_channel();

    // BFT finality gadget binding for inbound p2p vote/cert fan-out (#1388).
    //
    // The local gadget instance is intentionally unattached here — the
    // active consensus engine on this node is PoP, not PoS, so there is
    // no per-block self-vote source yet. A follow-up PR will construct
    // the gadget with a populated validator set sourced from the chain
    // state and attach it to the engine via
    // `ProofOfStake::attach_bft_finality`. Until then, peers' inbound
    // votes are dropped (logged at debug), but inbound *certificates*
    // still advance local finality through the channel sender below —
    // that is the multi-node BFT-cert-gossip path the plan calls out.
    #[cfg(feature = "p2p")]
    let bft_finality_for_p2p: Option<Arc<RwLock<wrfcoin_consensus_engines::BftFinalityGadget>>> =
        None;

    // #1798: build the WAN failover config once so the monitor (in AppState)
    // and the background probe task share identical probe parameters.
    let failover_config = FailoverConfig::default();

    // Load permissionless replay-seen map persisted on last run (#1845).
    // Entries that exceeded TTL on the previous run were pruned before persist,
    // but prune again here in case the node was down longer than the TTL.
    let permissionless_replay_seen_init = {
        use crate::state::PERMISSIONLESS_REPLAY_TTL_SECS;
        let now = chrono::Utc::now().timestamp();
        let loaded = storage.load_permissionless_replay().unwrap_or_else(|e| {
            log::warn!("Could not load persisted replay-seen map, starting fresh: {e}");
            std::collections::HashMap::new()
        });
        let pruned: FxHashMap<String, i64> = loaded
            .into_iter()
            .filter(|(_, seen_at)| *seen_at > now - PERMISSIONLESS_REPLAY_TTL_SECS)
            .collect();
        log::info!(
            "Loaded {} permissionless replay-seen entries from storage (#1845)",
            pruned.len()
        );
        pruned
    };

    // #1969: node-local weather-gossip signing key. Derived from the stable
    // libp2p ed25519 peer identity (same `peer_identity.key` file the P2P stack
    // loads) so the V2 broadcast signature's pubkey is tied to this node's
    // identity and survives restarts. Falls back to a fresh random key when the
    // identity is missing or not ed25519 — broadcasts still verify, the pubkey
    // just rotates on restart.
    #[cfg(feature = "p2p")]
    let weather_gossip_signing_key: Arc<
        wrfcoin_native_chain::crypto::signature::PrivateKey,
    > = {
        use wrfcoin_native_chain::crypto::signature::PrivateKey;
        let identity_path = std::path::Path::new(&data_dir).join("peer_identity.key");
        let derived = wrfcoin_p2p::load_or_generate_keypair(&identity_path)
            .ok()
            .and_then(|kp| kp.try_into_ed25519().ok())
            .and_then(|kp| PrivateKey::from_bytes(&kp.secret().as_ref()[..32]).ok())
            .unwrap_or_else(|| PrivateKey::generate(&mut rand::thread_rng()));
        Arc::new(derived)
    };

    #[cfg(feature = "p2p")]
    let p2p_consumer_health = P2pConsumerHealth::new(p2p_event_rx.is_some());

    let state = web::Data::new(AppState {
        consensus: Arc::new(consensus_manager),
        failover_monitor: Arc::new(RwLock::new(FailoverMonitor::new(failover_config.clone()))),
        blockchain: Arc::new(RwLock::new(blockchain)),
        snapshot_service: Arc::new(crate::routes::snapshot::SnapshotServiceState::default()),
        genesis_hash: crate::state::GenesisHashCell::new(genesis_hash),
        producer_authority_boot: Arc::new(producer_authority_boot),
        p2p_startup: Arc::new(p2p_startup),
        nft_mutation_api_enabled: nft_mutation_api_enabled_from_env(),
        oracle_admin_api_enabled: oracle_admin_api_enabled_from_env(),
        debug_seed_api_enabled: debug_seed_api_enabled_from_env(),
        mempool: Arc::new(RwLock::new(Mempool::new(&mempool_config))),
        snapshot_sync_seen: Arc::new(RwLock::new(FxHashSet::default())),
        snapshot_signing_key: load_snapshot_signer(node_role),
        snapshot_trusted_signers: load_snapshot_trusted_signers(),
        bridge_safety: Arc::new(RwLock::new(BridgeSafetyManager::new(
            BridgeSafetyConfig::default(),
        ))),
        token_store,
        legacy_submit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
        permissionless_submit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
        forecast_quotas: Arc::new(RwLock::new(Default::default())),
        contribution_commit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
        permissionless_replay_seen: Arc::new(RwLock::new(permissionless_replay_seen_init)),
        storage: Arc::new(RwLock::new(storage)),
        evidence_store: evidence_store.clone(),
        failed_transactions_total: Arc::new(AtomicU64::new(0)),
        zk_proofs_per_block_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        trust_reward_metrics_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        mining_tx_count_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        stake_metrics_cache: Arc::new(std::sync::Mutex::new(None)),
        block_production_lock: Arc::new(tokio::sync::Mutex::new(())),
        partition_mode: Arc::new(RwLock::new(
            wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode::default(),
        )),
        node_capability: Arc::new(RwLock::new(
            wrfcoin_native_chain::network_failover::NodeCapabilitySnapshot::default(),
        )),
        state_diverged: Arc::new(AtomicBool::new(false)),
        #[cfg(feature = "p2p")]
        tx_gossip_backpressure_streak: Arc::new(std::sync::Mutex::new(0)),
        #[cfg(feature = "p2p")]
        p2p_consumer_health: p2p_consumer_health.clone(),
        event_tx: websocket::new_event_channel().0,
        finality_signal_tx,
        #[cfg(feature = "p2p")]
        p2p: p2p_client,
        #[cfg(feature = "p2p")]
        weather_gossip_signing_key,
        #[cfg(feature = "p2p")]
        da_challenge_runtime: crate::da_challenge_producer::DaChallengeRuntime::new(),
        #[cfg(feature = "privacy")]
        key_image_registry: Arc::new(KeyImageRegistry::new()),
        #[cfg(feature = "privacy")]
        privacy_key_registry: Arc::new(RwLock::new(FxHashMap::default())),
        #[cfg(feature = "zk")]
        zk_verifier,
        #[cfg(feature = "zk")]
        zk_prover_gate: Arc::new(Semaphore::new(zk_max_provers)),
        #[cfg(feature = "execution")]
        execution_registry: Arc::new(default_registry()),
        #[cfg(feature = "execution")]
        execution_runtime: match DockerRunner::new() {
            Ok(runner) => Arc::new(runner),
            Err(e) => {
                log::warn!("Docker unavailable ({e}), execution runs will fail at runtime");
                Arc::new(wrfcoin_execution_engine::docker::NoOpContainerRuntime)
            },
        },
        #[cfg(feature = "execution")]
        execution_results: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            execution_token_quotas: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            execution_quota_config: crate::state::ExecutionQuotaConfig::from_env(),
            #[cfg(feature = "execution")]
            forecast_task_assignments: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            forecast_node_heartbeats: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            forecast_task_results: Arc::new(RwLock::new(FxHashMap::default())),
        });

    #[cfg(feature = "privacy")]
    spawn_privacy_key_eviction_task(&state);

    #[cfg(feature = "p2p")]
    {
        let mut da_failure_rx = state.da_challenge_runtime.subscribe_failures();
        tokio::spawn(async move {
            while let Ok(failure) = da_failure_rx.recv().await {
                log::warn!(
                    "Observed DA challenge failure event: peer={} fragment={} reason={}",
                    failure.peer_id,
                    failure.fragment_id,
                    failure.reason
                );
            }
        });
    }

    // Spawn the BFT finality signal consumer (#1216).
    //
    // This task is the wiring that closes the gap in the reorg
    // rejection path. It listens on the receiver half of the channel
    // created above, and every time a `FinalitySignal` arrives it
    // forwards the height into `Blockchain::set_finalized_height`,
    // which arms the BFT finality guard in `plan_reorg`.
    //
    // Until a BFT source (HotStuff commit path, BftFinalityGadget)
    // actually emits on the sender half, this loop simply blocks on
    // `recv` — zero cost, zero effect. As soon as *any* source starts
    // emitting, the guard becomes live without further wiring.
    //
    // #1388: no in-tree code path attaches a sink to a BftFinalityGadget or
    // HotStuff instance yet (testnet runs PoP, which has no BFT). Log a
    // loud warning so operators do not mistake the silently-inert guard
    // for an active BFT check. The companion
    // `wrfcoin_chain_bft_finality_active` Prometheus gauge stays at 0
    // until the consumer latches it on the first real signal.
    log::warn!(
        "BFT finality guard is INERT at boot: no BFT source is attached to \
         the finality channel in this build. `plan_reorg` falls back to the \
         probabilistic `tip - MAX_REORG_DEPTH` depth cap. Monitor \
         `wrfcoin_chain_bft_finality_active` — it will stay at 0 until a \
         BftFinalityGadget or HotStuff commit path calls emit_finality_signal \
         (#1388)."
    );
    {
        let bc_for_finality = state.blockchain.clone();
        let storage_for_finality = state.storage.clone();
        let partition_mode_for_finality = state.partition_mode.clone();
        let mempool_for_finality = state.mempool.clone();

        // #2096: the consumer's exit is SUPERVISED, not discarded.
        //
        // It used to be `tokio::spawn(..)` with the JoinHandle dropped, so a
        // detected INV-FINALITY-1 breach only stopped one background task —
        // the node carried on producing blocks after finalizing two different
        // blocks at one height. A safety violation that a caller can swallow
        // is not an enforced invariant. Same defect class as #2056, where
        // block assembly discarded an apply error and silently dropped
        // transfers.
        // Register the stateful invariants. The finality consumer feeds the
        // finality handle and `State::check_supply_conservation` feeds the
        // supply handle attached above; the registry polls both, so a latched
        // safety violation in either domain is visible without knowing anything
        // about either. Future stateful invariants (cross-chain, account,
        // sensor) register the same way — one line in `build_shipped_registry`
        // and one id in `REQUIRED_STATEFUL_INVARIANTS`.
        let finality_invariant = SharedFinalityInvariant::new();
        let stateful_registry = stateful_invariants::build_shipped_registry(
            &finality_invariant,
            &supply_invariant,
        );
        // Boot gate (#2250 review): refuse to start with an unarmed detector.
        // Logging and continuing would be the failure this whole change is
        // about — a node that reports nothing because nothing is watching looks
        // exactly like a node with nothing to report.
        if let Err(e) = stateful_invariants::assert_shipped_registry_complete(&stateful_registry) {
            log::error!("Refusing to start: {e} (#2097)");
            return Err(std::io::Error::other(e));
        }
        log::info!(
            "stateful invariants armed: {:?}",
            stateful_registry.ids()
        );

        // Poll the registry in production. Until now `check_all()` had no
        // production caller at all: the registry was built, logged, and never
        // read, so INV-FINALITY-1's latch was only ever acted on through the
        // consumer's own exit and INV-SUPPLY-1's would not have been acted on
        // at all.
        {
            let registry = stateful_registry.clone();
            let partition_mode_for_invariants = state.partition_mode.clone();
            let mempool_for_invariants = state.mempool.clone();
            tokio::spawn(async move {
                let exit = stateful_invariants::run_stateful_invariant_supervisor(
                    registry,
                    partition_mode_for_invariants,
                    mempool_for_invariants,
                    stateful_invariants::DEFAULT_POLL_INTERVAL,
                )
                .await;
                // The supervisor has already forced ReadOnly. Terminating is
                // the last step, for the same reason as the finality path
                // below: a node that has observed value created from nothing
                // must not stay up, and an operator restarting it without
                // reading the logs would erase the in-process record.
                log::error!(
                    "terminating after a stateful-invariant violation ({}); \
                     see docs/security/EMERGENCY_RUNBOOK.md §1",
                    exit.summary(),
                );
                std::process::exit(1);
            });
        }

        let consumer = tokio::spawn(finality_events::run_finality_event_loop(
            finality_signal_rx,
            bc_for_finality,
            Some(storage_for_finality),
            finality_invariant,
        ));
        tokio::spawn(async move {
            let exit = finality_events::supervise_finality_consumer(
                consumer,
                partition_mode_for_finality,
                mempool_for_finality,
            )
            .await;
            if exit.is_fatal() {
                // The supervisor has already forced ReadOnly. Terminating is
                // the last step: a node that finalized conflicting blocks must
                // not stay up, and an operator restarting it without reading
                // the logs would erase the invariant's in-process record of
                // the conflict.
                log::error!(
                    "terminating after a consensus-safety violation ({exit:?}); \
                     see docs/security/EMERGENCY_RUNBOOK.md §1"
                );
                std::process::exit(1);
            }
        });
    }

    // #1798: WAN failover probe task. Periodically probes primary-WAN
    // connectivity and feeds the result into the shared `FailoverMonitor`, so
    // `/node/network/failover` and `readyz` reflect live primary↔LTE/5G state.
    // The probe (`probe_connectivity`) is a blocking TCP connect, so it runs on
    // the blocking pool; the monitor update is brief. Operational only — this
    // task never touches consensus state.
    {
        let failover_monitor = state.failover_monitor.clone();
        let probe_interval = failover_config.probe_interval;
        let probe_timeout = failover_config.probe_timeout;
        let probe_targets = failover_config.probe_targets.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(probe_interval);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let targets = probe_targets.clone();
                let result = tokio::task::spawn_blocking(move || {
                    wrfcoin_native_chain::network_failover::probe_connectivity(
                        &targets,
                        probe_timeout,
                    )
                })
                .await;
                let result = match result {
                    Ok(result) => result,
                    Err(e) => {
                        log::warn!("WAN failover probe task panicked: {e}");
                        continue;
                    },
                };
                let mut monitor = failover_monitor.write().await;
                if let Some(event) = monitor.on_probe_result(result) {
                    log::info!(
                        "WAN failover transition: {event:?} → state={} metered={}",
                        monitor.state(),
                        monitor.is_metered(),
                    );
                }
            }
        });
    }

    // #1393: slashing-report submitter. Scans the durable evidence store
    // for equivocation proofs that have not yet been turned into on-chain
    // transactions and enqueues one signed `SlashingReport` per new
    // record into the local mempool. The confirmed cursor under
    // `<data_dir>` is advanced only on block inclusion (#1393 round-4
    // P1) so mempool-evicted evidence is re-submitted on the next scan;
    // the state-layer `slashing_evidence_seen` dedup is the second line
    // of defence.
    //
    // Signer reuse (#1393 round-4 P0): the snapshot signer is also the
    // reporter signer. Operators running release builds always have one
    // configured via HSM; debug builds without a signer run in unsigned
    // mode where the apply arm rejects reports with
    // SP_SLASHING_REPORT_UNAUTHENTICATED — safe, just dormant.
    slashing_submitter::spawn_submitter_task(
        state.evidence_store.clone(),
        state.mempool.clone(),
        std::path::PathBuf::from(&data_dir),
        state.snapshot_signing_key.clone(),
        state.blockchain.clone(),
        state.event_tx.subscribe(),
    );

    // Shared peer-reputation tracker (#1406 follow-up): written by the P2P
    // event consumer on `PeerPenalized`, read by the `/network/peers/reputation`
    // operator route. Operational/node-local only — never hashed into the
    // consensus state root.
    #[cfg(feature = "p2p")]
    let peer_reputation: crate::peer_reputation::SharedPeerReputation = std::sync::Arc::new(
        tokio::sync::RwLock::new(crate::peer_reputation::PeerReputationTracker::new()),
    );

    // Spawn P2P event consumer to process incoming blocks/transactions from peers.
    #[cfg(feature = "p2p")]
    if let Some(event_rx) = p2p_event_rx {
        let bc = state.blockchain.clone();
        let mp = state.mempool.clone();
        let st = state.storage.clone();
        let cs = state.consensus.clone();
        let p2p = state.p2p.clone();
        let da_runtime = state.da_challenge_runtime.clone();
        let bft = bft_finality_for_p2p.clone();
        let fin_tx = Some(state.finality_signal_tx.clone());
        #[cfg(feature = "execution")]
        let forecast_task_assignments = state.forecast_task_assignments.clone();
        #[cfg(feature = "execution")]
        let forecast_node_heartbeats = state.forecast_node_heartbeats.clone();
        #[cfg(feature = "execution")]
        let forecast_task_results = state.forecast_task_results.clone();
        let state_diverged = state.state_diverged.clone();
        // #1724: select how the state_diverged readiness latch may be cleared
        // from the environment at the production spawn site, then inject it into
        // the event loop. Keeping the env read here (rather than inside the loop)
        // makes the runtime gate exercisable in tests without env globals.
        let recovery_policy =
            wrfcoin_native_chain::p2p_divergence::StateDivergenceRecoveryPolicy::from_env();
        let consumer = p2p_events::run_p2p_event_loop(
            event_rx,
            bc,
            mp,
            st,
            cs,
            p2p,
            da_runtime,
            bft,
            fin_tx,
            #[cfg(feature = "execution")]
            forecast_task_assignments,
            #[cfg(feature = "execution")]
            forecast_node_heartbeats,
            #[cfg(feature = "execution")]
            forecast_task_results,
            state_diverged,
            recovery_policy,
            peer_reputation.clone(),
            // #2389: sign P2P-ingested weather commit authorizations with the
            // node's stable weather identity key so they pass #1848 admission.
            state.weather_gossip_signing_key.clone(),
        );
        #[cfg(feature = "panic-drill")]
        let consumer = with_panic_drill(consumer);
        spawn_p2p_consumer_supervisor(p2p_consumer_health.clone(), consumer);

        // Bootstrap peer timeout alarm (#979): if bootstrap peers were configured
        // but no peers connect within 60 seconds, warn the operator.
        let has_bootstrap = std::env::var("WRFCOIN_P2P_BOOTSTRAP")
            .ok()
            .filter(|s| !s.is_empty())
            .is_some();
        if has_bootstrap {
            let p2p_client_for_alarm = state.p2p.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(60)).await;
                if let Some(client) = &p2p_client_for_alarm {
                    let stats = client.stats();
                    // blocks_received > 0 or txs_received > 0 implies at least
                    // one peer was connected at some point.
                    let had_traffic = stats.blocks_received > 0
                        || stats.txs_received > 0
                        || stats.weather_msgs_received > 0;
                    if !had_traffic {
                        log::warn!(
                            "No P2P traffic received after 60 seconds despite bootstrap peers being configured. Checklist:\n  \
                             - Are bootstrap peer addresses correct? (expected libp2p multiaddrs such as /ip4/<IP>/tcp/<PORT>/p2p/<PEER_ID> or /ip4/<IP>/udp/<PORT>/quic-v1/p2p/<PEER_ID>)\n  \
                             - Is port 9000 open in your firewall / security group?\n  \
                             - Are the bootstrap peers running and reachable?\n  \
                             - Check WRFCOIN_P2P_EXTERNAL_ADDR if behind NAT/Docker"
                        );
                    }
                }
            });
        }
    }

    #[cfg(feature = "p2p")]
    if let Some(p2p_client) = state.p2p.clone() {
        let da_config = da_producer_config_from_env(&Config::default());
        if da_config.interval_secs == 0 || da_config.peer_count == 0 {
            log::info!(
                "DA challenge producer disabled by config (interval_secs={}, peer_count={})",
                da_config.interval_secs,
                da_config.peer_count
            );
        } else {
            let blockchain = state.blockchain.clone();
            let da_runtime = state.da_challenge_runtime.clone();
            tokio::spawn(run_da_challenge_producer(
                blockchain,
                p2p_client,
                da_runtime,
                da_config,
            ));
        }
    }

    #[cfg(feature = "privacy")]
    {
        let privacy_state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(
                state::PRIVACY_KEY_TTL_SECS as u64,
            ));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await;

            loop {
                interval.tick().await;
                let removed = routes::privacy::privacy_handlers::prune_expired_private_keys(
                    privacy_state.get_ref(),
                )
                .await;
                if removed > 0 {
                    log::debug!("Pruned {removed} expired privacy keys from the registry");
                }
            }
        });
    }

    // #3095: the upgrade run-up. One task owns both halves of "the operator
    // must not be able to miss this": the height-driven warning cadence, and
    // the optional peer fetch that lets a node learn about a notice it was
    // never given a file for.
    //
    // The tick is 5s — a third of `TARGET_BLOCK_TIME_SECONDS` — so the
    // ERROR-per-block cadence inside the last 100 blocks observes every tip
    // rather than sampling it. `log_run_up` is idempotent per height, so the
    // faster tick costs log lines nothing: between blocks it returns `None`.
    {
        let notice_blockchain = state.blockchain.clone();
        let notice_identity =
            wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::from_config();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(UPGRADE_NOTICE_TICK_SECS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            // Ticks, not wall-clock: the interval already IS the clock, and
            // counting its ticks keeps this loop free of a time read.
            let ticks_per_peer_fetch =
                (UPGRADE_NOTICE_PEER_FETCH_SECS / UPGRADE_NOTICE_TICK_SECS).max(1);
            let mut tick: u64 = 0;
            loop {
                interval.tick().await;
                let tip = notice_blockchain.read().await.height;
                let _ = wrfcoin_native_chain::upgrade_notice::log_run_up(tip);

                if tick % ticks_per_peer_fetch == 0 {
                    fetch_upgrade_notices_from_peers(&notice_identity).await;
                }
                tick = tick.wrapping_add(1);
            }
        });
    }

    // Capture references for the shutdown handler before `state` is moved
    // into the HttpServer closure.
    let shutdown_storage = state.storage.clone();
    let shutdown_blockchain = state.blockchain.clone();
    #[cfg(feature = "p2p")]
    let shutdown_p2p = state.p2p.clone();
    #[cfg(feature = "p2p")]
    let shutdown_p2p_consumer_health = state.p2p_consumer_health.clone();

    // Global per-IP HTTP rate limiter (issue #1080).
    let rl_state = rate_limit::RateLimiterState::new(state.token_store.clone());

    let server = HttpServer::new(move || {
        let app = App::new()
            .wrap(rate_limit::RateLimiter::new(rl_state.clone()))
            .wrap(build_cors())
            .app_data(state.clone())
            .app_data(launch_api_json_config());
        // #3586: only the isolated ReceiptConsumer gets the large receiver-ingest budget.
        let app = if node_role == NodeRole::ReceiptConsumer {
            app.app_data(routes::node_receipts::ReceiptConsumerIngestLimit(
                routes::node_receipts::MAX_NODE_RECEIVER_SNAPSHOT_INGEST_BYTES,
            ))
        } else {
            app
        };
        // #1406 follow-up: expose the shared peer-reputation tracker to the
        // operator read route as its own `web::Data` (keeps it out of AppState,
        // which is not p2p-gated).
        #[cfg(feature = "p2p")]
        let app = app.app_data(web::Data::new(peer_reputation.clone()));
        app.configure(configure_launch_routes)
    })
    .bind((host.as_str(), port))?
    // Do not close an idle pooled connection before a client whose retention is
    // within the supported bound would have dropped it — see
    // LAUNCH_API_KEEP_ALIVE for what that bound covers and what it excludes.
    // Without this the actix default of 5s collides with the attester's 5s poll
    // and drops roughly one request in 400.
    .keep_alive(LAUNCH_API_KEEP_ALIVE)
    .shutdown_timeout(30)
    .run();

    let server_handle = server.handle();

    // Print post-startup readiness summary (#954).
    {
        let chain_height = shutdown_blockchain.read().await.height;
        #[cfg(feature = "p2p")]
        let p2p_info = p2p_summary_info
            .as_ref()
            .map(|(id, p2p_port, quic_port)| (id.as_str(), *p2p_port, *quic_port, 0usize));
        #[cfg(not(feature = "p2p"))]
        let p2p_info: Option<(&str, u16, Option<u16>, usize)> = None;
        helpers::print_post_startup_summary(&host, port, chain_height, p2p_info);
    }

    // Spawn a task that waits for SIGTERM or SIGINT — or the #3093 upgrade
    // halt — then orchestrates a graceful shutdown: P2P disconnect -> storage
    // checkpoint -> HTTP stop. The upgrade halt additionally exits with its
    // distinct code once the shutdown is complete.
    let upgrade_halt_signal = wrfcoin_native_chain::upgrade::halt_signal();
    tokio::spawn(async move {
        let sigterm = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to register SIGTERM handler")
                .recv()
                .await;
        };
        let sigint = async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to register SIGINT handler");
        };
        let upgrade_halt = upgrade_halt_signal.notified();

        let mut exit_upgrade_required = false;
        tokio::select! {
            _ = sigterm => log::info!("Received SIGTERM, initiating graceful shutdown..."),
            _ = sigint => log::info!("Received SIGINT, initiating graceful shutdown..."),
            _ = upgrade_halt => {
                log::error!(
                    "EXIT_UPGRADE_REQUIRED: upgrade deadline reached, initiating clean shutdown \
                     (#3093)"
                );
                exit_upgrade_required = true;
            },
        }

        // Phase 1: Shut down P2P networking.
        #[cfg(feature = "p2p")]
        {
            shutdown_p2p_consumer_health.begin_shutdown();
            if let Some(ref client) = shutdown_p2p {
                log::info!("Stopping P2P networking...");
                let _ = client
                    .command_tx
                    .send(wrfcoin_p2p::P2pCommand::Shutdown)
                    .await;
            }
        }

        // Phase 2: Write a final storage checkpoint for fast restart.
        {
            let chain = shutdown_blockchain.read().await;
            let height = chain.height;
            let state_snapshot = chain.state.clone();
            drop(chain);

            if height > 0 {
                log::info!("Writing shutdown checkpoint at height {height}...");
                let storage = shutdown_storage.read().await;
                if let Err(e) = storage.force_checkpoint(&state_snapshot, height).await {
                    log::error!("Shutdown checkpoint failed: {e}");
                }
            }
        }

        // Phase 3: Stop the HTTP server (finishes in-flight requests).
        log::info!("Stopping HTTP server...");
        server_handle.stop(true).await;

        log::info!("Graceful shutdown complete.");
        if exit_upgrade_required {
            log::error!(
                "Stopped cleanly at the upgrade deadline; exiting with code {} \
                 (EXIT_UPGRADE_REQUIRED, #3093). Install the announced release and restart.",
                wrfcoin_native_chain::upgrade::EXIT_CODE_UPGRADE_REQUIRED
            );
            std::process::exit(wrfcoin_native_chain::upgrade::EXIT_CODE_UPGRADE_REQUIRED);
        }
    });

    server.await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    #[path = "node_share_slice6b.rs"]
    mod node_share_slice6b;
    #[path = "node_bond_slice6a.rs"]
    mod node_bond_slice6a;
    #[path = "node_receipt_slice3.rs"]
    mod node_receipt_slice3;
    #[path = "node_settlement_slice4.rs"]
    mod node_settlement_slice4;
    #[path = "node_fraud_slice5.rs"]
    mod node_fraud_slice5;

    #[cfg(unix)]
    #[path = "payout_signer_3499.rs"]
    mod payout_signer_3499;

    #[path = "snapshot_finality_3331.rs"]
    mod snapshot_finality_3331;

    #[path = "snapshot_authority_888.rs"]
    mod snapshot_authority_888;

    #[::core::prelude::v1::test]
    fn p2p_profile_is_fail_closed_and_developer_mode_is_explicit() {
        assert_eq!(
            p2p_profile_from_raw(None, Some("true")),
            Ok(P2pStartupProfile::Required)
        );
        assert_eq!(
            p2p_profile_from_raw(Some("developer"), None),
            Ok(P2pStartupProfile::Developer)
        );
        assert_eq!(
            p2p_profile_from_raw(Some("disabled"), None),
            Ok(P2pStartupProfile::Disabled)
        );
        assert!(p2p_profile_from_raw(None, Some("tru")).is_err());
        assert!(p2p_profile_from_raw(Some("public-ish"), None).is_err());
        assert!(p2p_profile_from_raw(Some("disabled"), Some("true")).is_err());
    }

    #[cfg(feature = "p2p")]
    #[::core::prelude::v1::test]
    fn p2p_config_rejects_malformed_bootstrap_and_relay_addresses() {
        let operator_value = "credential@10.0.0.1:9000";
        let bootstrap = build_p2p_config(
            "/ip4/127.0.0.1/tcp/8555",
            Some(operator_value),
        )
        .unwrap_err();
        assert_eq!(bootstrap.error_code(), "NC_NODE_CFG_INVALID_PEER_ADDR");
        let rendered = bootstrap.to_string();
        assert!(rendered.contains("WRFCOIN_P2P_BOOTSTRAP entry 1"));
        assert!(!rendered.contains(operator_value));
        assert!(!rendered.contains("10.0.0.1"));

        let relay = parse_p2p_addr_list(
            "WRFCOIN_P2P_RELAY_ADDRESSES",
            Some("also-not-a-multiaddr"),
        )
        .unwrap_err();
        assert_eq!(relay.error_code(), "NC_NODE_CFG_INVALID_PEER_ADDR");
        assert!(!relay.to_string().contains("also-not-a-multiaddr"));
    }

    /// #3224: the in-process half of the fail-stop policy. The consumer drains a
    /// real channel; dropping every sender ends it without a panic, and `/readyz`
    /// must go not-ready with the stable public reason.
    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn closed_p2p_consumer_channel_turns_readiness_red_3224() {
        let health = P2pConsumerHealth::new(true);
        assert!(health.blocks_readiness(), "a consumer that has not started is not ready");
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<u8>(1);
        let supervisor = spawn_p2p_consumer_supervisor(health.clone(), async move {
            while event_rx.recv().await.is_some() {}
        });
        event_tx.send(1).await.expect("consumer is draining");
        tokio::task::yield_now().await;
        assert_eq!(health.status(), state::P2pConsumerStatus::Running);
        assert!(!health.blocks_readiness());

        drop(event_tx);
        supervisor.await.expect("a channel-closed consumer ends without a panic");
        assert_eq!(health.status(), state::P2pConsumerStatus::Failed);
        let (readiness, blocks) = routes::chain::readyz_p2p_consumer_check(&health);
        assert!(blocks, "an unexpected consumer end must make /readyz fail");
        assert_eq!(readiness["status"], "failed");
        assert_eq!(readiness["readiness_gate"], true);
        assert_eq!(readiness["policy"], "fail_stop");
        assert_eq!(readiness["detail"], "inbound P2P consumer is unavailable");
        assert!(
            !readiness.to_string().contains("event channel closed"),
            "the internal failure detail stays server-side"
        );
        assert!(health.detail().expect("failure detail").contains("event channel closed"));
    }

    /// Child half of `consumer_panic_aborts_the_process_in_unwind_builds_3224`.
    #[cfg(all(feature = "p2p", unix))]
    const CONSUMER_PANIC_CHILD_ENV: &str = "WRFCOIN_3224_CONSUMER_PANIC_CHILD";
    #[cfg(all(feature = "p2p", unix))]
    const CONSUMER_PANIC_SURVIVED_EXIT: i32 = 42;

    /// #3224: a consumer panic is a whole-process fail-stop in the unwind (test)
    /// profile too, not a task death the supervisor could observe. The test binary
    /// re-runs itself as a child that panics inside the supervised consumer; the
    /// child must die of SIGABRT and never reach the "survived" exit. The release
    /// (`panic = "abort"`) half is proved on the built artifact by the panic drill.
    #[cfg(all(feature = "p2p", unix))]
    #[::core::prelude::v1::test]
    fn consumer_panic_aborts_the_process_in_unwind_builds_3224() {
        use std::os::unix::process::ExitStatusExt;
        const NAME: &str = "tests::consumer_panic_aborts_the_process_in_unwind_builds_3224";
        if std::env::var_os(CONSUMER_PANIC_CHILD_ENV).is_some() {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("child runtime");
            runtime.block_on(async {
                let health = P2pConsumerHealth::new(true);
                let supervisor = spawn_p2p_consumer_supervisor(health, async {
                    panic!("injected inbound-consumer panic (#3224 child)");
                });
                let _ = supervisor.await;
            });
            std::process::exit(CONSUMER_PANIC_SURVIVED_EXIT);
        }
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
            .env(CONSUMER_PANIC_CHILD_ENV, "1")
            .output()
            .expect("spawn child test process");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_ne!(
            output.status.code(),
            Some(CONSUMER_PANIC_SURVIVED_EXIT),
            "the process survived a consumer panic; stderr:\n{stderr}"
        );
        assert_eq!(
            output.status.signal(),
            Some(6),
            "a consumer panic must abort the process (SIGABRT); status {:?}, stderr:\n{stderr}",
            output.status
        );
        assert!(stderr.contains("P2P_EVENT_CONSUMER_PANICKED"), "stderr:\n{stderr}");
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn graceful_p2p_consumer_stop_is_not_misclassified_as_failure_3224() {
        let health = P2pConsumerHealth::new(true);
        health.begin_shutdown();
        let supervisor = spawn_p2p_consumer_supervisor(health.clone(), async {});
        supervisor.await.expect("supervisor itself must stay alive");
        assert_eq!(health.status(), state::P2pConsumerStatus::GracefulStop);
        assert!(!health.blocks_readiness());
        assert_eq!(health.detail(), None);
    }

    // ---- #2290 P3: the genesis producer-set resolver is actually WIRED ------
    //
    // Dan's blocking finding on #2398 was that `genesis_producer_members()`
    // returned a hard-coded `Ok(vec![])`, so enforce-at-genesis was dead code:
    // a fresh chain seeded nothing and produced blocks unauthenticated. These
    // drive the REAL process environment through the REAL startup resolver, so
    // they cannot pass against a stub — that is the point.
    //
    // They serialize on `genesis_producers`' own `ENV_LOCK` (not a second
    // mutex) because they mutate the same process-global variables.
    use crate::genesis_producers::tests::with_env;

    /// A fresh testnet genesis must resolve the PINNED ceremony committee
    /// through the real startup path — proving the wiring is live, not stubbed.
    ///
    /// This assertion was inverted by the 2026-07-24 key ceremony. It
    /// previously required a hard startup ABORT, because
    /// `TESTNET_GENESIS_PRODUCER_SET` was `None` and minting an unauthenticated
    /// testnet chain had to be impossible. The set is now pinned, so the correct
    /// outcome is the real committee. The unpinned-abort rule still exists and
    /// is still tested — at the resolver level by
    /// `testnet_without_a_pinned_set_is_a_hard_failure`, which passes `None`
    /// explicitly rather than depending on the constant.
    ///
    /// Asserting the exact ceremony values (not merely "two members") is what
    /// keeps this a wiring proof: a stub could return a count, but not these.
    #[::core::prelude::v1::test]
    fn genesis_producer_members_resolves_the_pinned_testnet_committee() {
        let members = with_env(None, Some("testnet"), None, genesis_producer_members)
            .expect("fresh testnet genesis must resolve the pinned producer set");

        assert_eq!(
            members.len(),
            3,
            "the roster seats exactly three producers (#2971)"
        );
        assert_eq!(members[0].producer_id, "n5-node-a");
        assert_eq!(
            members[0].payout_address,
            "0x680d51f7543ca70029866956376306d47ce60ed0"
        );
        assert_eq!(members[1].producer_id, "producer-standby");
        assert_eq!(
            members[1].payout_address,
            "0x39ee2e595a55c71256282fe5c42a398594985b1a"
        );
        // mango-mini: owner-designated payout (2026-09-10, #3078) and a real
        // SoftHSM-backed producer key. Asserted by value for the same reason
        // as the other two: a stub could return a count but not these exact
        // strings, and the newest seat must not be the one that goes unwatched.
        assert_eq!(members[2].producer_id, "mango-mini");
        assert_eq!(
            members[2].payout_address,
            "0xc536d2026661522cb8631b3c723d4af43784a990"
        );
    }

    /// A malformed fail-closed switch must abort startup too — proving the
    /// error propagates through the adapter rather than being swallowed.
    #[::core::prelude::v1::test]
    fn genesis_producer_members_aborts_on_malformed_require_auth() {
        let err = with_env(None, None, Some("tru"), genesis_producer_members)
            .expect_err("a typo'd fail-closed switch must abort startup");
        assert!(
            err.contains("WRFCOIN_REQUIRE_PRODUCER_AUTH"),
            "expected the malformed-switch abort, got: {err}"
        );
    }

    /// A configured set must reach the caller with fields intact — the mapping
    /// from Alice's config DTO onto the consensus `ProducerMember` is lossless,
    /// and `payout_address` stays the canonical `0x`-hex `Address` form (NOT
    /// `PublicKey::to_address()`'s `wrf` form, which would break the coinbase
    /// match at the gate).
    #[::core::prelude::v1::test]
    fn genesis_producer_members_maps_configured_set_losslessly() {
        let payout = format!("0x{}", "ab".repeat(20));
        let spec = format!(
            r#"{{"version":1,"members":[{{"producer_id":"n5-node-a","payout_address":"{payout}","public_key":"{}"}}]}}"#,
            "11".repeat(32)
        );
        let members = with_env(Some(&spec), None, None, genesis_producer_members)
            .expect("a valid explicit set must resolve");
        assert_eq!(members.len(), 1, "the configured member must survive");
        assert_eq!(members[0].producer_id, "n5-node-a");
        assert_eq!(members[0].payout_address, payout);
        assert_eq!(members[0].public_key, [0x11u8; 32]);
    }

    // `actix_web::test` is imported later in this module. Item imports have
    // module-wide scope, so qualify synchronous tests to avoid resolving to
    // Actix Web's async-only `#[test]` attribute macro.
    // ------------------------------------------------------------------
    // #2290 P1-4 / blocker 1 — provisioning fails CLOSED, driven through the
    // real startup seam (`producer_startup_decision`), never a re-typed copy of
    // the parser or the policy.
    // ------------------------------------------------------------------

    /// A chain whose genesis seeded an authorized producer set must not start
    /// without a producer signer — **with no env flag involved at all.**
    ///
    /// This is Elsa's round-2 blocker 1. The previous revision computed
    /// activation solely from `WRFCOIN_PRODUCER_AUTH_REQUIRED`, so an enabled
    /// chain whose operator omitted that flag took the `ProduceUnsigned` path
    /// and built blocks every peer rejects. Activation now comes from the same
    /// on-chain source #2398's verifier uses.
    #[::core::prelude::v1::test]
    fn seeded_producer_set_alone_refuses_to_start_without_a_signer_2290_b1() {
        assert_eq!(
            producer_startup_decision(true, None, ProducerSignerResolution::Absent),
            Ok(ProducerSignerDecision::RefuseToStart),
            "a chain with a seeded producer set must refuse to start without a signer even \
             when no env flag is set — the chain's own state is the activation source"
        );
        assert_eq!(
            producer_startup_decision(true, None, ProducerSignerResolution::InitFailed),
            Ok(ProducerSignerDecision::RefuseToStart),
            "a broken signer on an enabled chain is equally fatal"
        );
    }

    /// Sync-only is an explicit role boundary, not an auth-enforcement toggle.
    /// The same chain still authenticates every received block; this declaration
    /// only says that this process will never enter a producer path (#2860).
    #[::core::prelude::v1::test]
    fn sync_only_role_is_explicit_and_fail_closed_2860() {
        for on in ["1", "true", "TRUE", "Yes", "on", "  on  "] {
            assert_eq!(
                node_role_from_raw_sync_only(Some(on)),
                Ok(NodeRole::SyncOnly),
                "{on:?} must select the non-producing role"
            );
        }
        for producing in [None, Some(""), Some("   "), Some("0"), Some("false"), Some("off")] {
            assert_eq!(
                node_role_from_raw_sync_only(producing),
                Ok(NodeRole::Producing),
                "only an explicit true value may narrow the role"
            );
        }
        for typo in ["tru", "sync", "2", "off!"] {
            let err = node_role_from_raw_sync_only(Some(typo))
                .expect_err("a malformed role declaration must abort startup");
            assert!(
                err.contains(SYNC_ONLY_ENV) && err.contains(typo),
                "the refusal must name the variable and offending value: {err}"
            );
        }
    }

    /// #2120 boot follow-up: the P2P topology role derives the production
    /// posture. A sentry is a reader by definition (the live seed crash-loop:
    /// a signer-less sentry not declared sync-only was refused by the #2290
    /// gate on every restart); a validator produces; `public`/unset defers to
    /// `WRFCOIN_SYNC_ONLY`; a contradiction or a misspelling refuses to start
    /// naming the variables, never guessing.
    #[::core::prelude::v1::test]
    fn p2p_topology_role_derives_production_posture_2120() {
        // Sentry alone ⇒ reader, no producer signer required.
        for sentry in ["sentry", "Sentry", "  SENTRY "] {
            assert_eq!(
                node_role_from_raw(None, Some(sentry)),
                Ok(NodeRole::SyncOnly),
                "{sentry:?} must boot as a reader"
            );
        }
        // Sentry + explicit sync-only agree.
        assert_eq!(node_role_from_raw(Some("true"), Some("sentry")), Ok(NodeRole::SyncOnly));
        // Validator alone ⇒ producer (unchanged posture for the producing nodes).
        assert_eq!(node_role_from_raw(None, Some("validator")), Ok(NodeRole::Producing));
        assert_eq!(node_role_from_raw(Some("false"), Some("validator")), Ok(NodeRole::Producing));
        // Public / unset defer to WRFCOIN_SYNC_ONLY exactly as before #2120.
        for public in [None, Some("public"), Some(""), Some("  ")] {
            assert_eq!(node_role_from_raw(None, public), Ok(NodeRole::Producing));
            assert_eq!(node_role_from_raw(Some("1"), public), Ok(NodeRole::SyncOnly));
            assert_eq!(node_role_from_raw(Some("off"), public), Ok(NodeRole::Producing));
        }
        // Contradictions refuse, naming both variables.
        let err = node_role_from_raw(Some("false"), Some("sentry"))
            .expect_err("sentry + explicit producing must refuse");
        assert!(err.contains(P2P_NODE_ROLE_ENV) && err.contains(SYNC_ONLY_ENV), "{err}");
        let err = node_role_from_raw(Some("true"), Some("validator"))
            .expect_err("validator + sync-only must refuse");
        assert!(err.contains(P2P_NODE_ROLE_ENV) && err.contains(SYNC_ONLY_ENV), "{err}");
        // A misspelled role word is a refusal, not `public`.
        for typo in ["sentri", "seed", "reader", "validator "] {
            let word = typo.trim();
            if word == "validator" {
                continue;
            }
            let err = node_role_from_raw(None, Some(typo))
                .expect_err("a malformed topology role must abort startup");
            assert!(err.contains(P2P_NODE_ROLE_ENV) && err.contains(word), "{err}");
        }
        // A typo'd WRFCOIN_SYNC_ONLY is still refused even when the role is fine.
        assert!(node_role_from_raw(Some("tru"), Some("sentry")).is_err());
    }

    /// #2120 boot-path wiring: `build_p2p_config` (called before
    /// `P2pNode::start_*`) applies `WRFCOIN_P2P_NODE_ROLE` /
    /// `WRFCOIN_P2P_SENTRY_ALLOWLIST`, and a declared role that cannot be
    /// honoured is a typed startup refusal rather than a logged P2P start
    /// failure that leaves the node running as `public` without P2P.
    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn build_p2p_config_applies_role_env_before_p2p_start_2120() {
        use wrfcoin_p2p::config::NodeRole as P2pRole;
        let _guard = crate::helpers::test_env::lock();
        let clear = || {
            std::env::remove_var("WRFCOIN_P2P_NODE_ROLE");
            std::env::remove_var("WRFCOIN_P2P_SENTRY_ALLOWLIST");
        };
        clear();

        // Unset: exactly today's behaviour — public, no allowlist.
        let config = build_p2p_config("/ip4/0.0.0.0/tcp/9000", None).expect("default config");
        assert_eq!(config.role, P2pRole::Public);
        assert!(config.sentry_allowlist.is_none());

        // Sentry: the role reaches the config the swarm is started with.
        std::env::set_var("WRFCOIN_P2P_NODE_ROLE", "sentry");
        let config = build_p2p_config("/ip4/0.0.0.0/tcp/9000", None).expect("sentry config");
        assert_eq!(config.role, P2pRole::Sentry);

        // Validator with its sentries: role + parsed allowlist.
        let a = libp2p::identity::Keypair::generate_ed25519().public().to_peer_id();
        let b = libp2p::identity::Keypair::generate_ed25519().public().to_peer_id();
        std::env::set_var("WRFCOIN_P2P_NODE_ROLE", "validator");
        std::env::set_var("WRFCOIN_P2P_SENTRY_ALLOWLIST", format!("{a},{b}"));
        let config = build_p2p_config("/ip4/0.0.0.0/tcp/9000", None).expect("validator config");
        assert_eq!(config.role, P2pRole::Validator);
        assert_eq!(config.sentry_allowlist.as_deref(), Some(&[a, b][..]));

        // Validator WITHOUT an allowlist: refused at config time, typed.
        std::env::remove_var("WRFCOIN_P2P_SENTRY_ALLOWLIST");
        let err = build_p2p_config("/ip4/0.0.0.0/tcp/9000", None)
            .expect_err("a validator without sentries must not start");
        assert_eq!(err.error_code(), "NC_NODE_CFG_INVALID_P2P_ROLE");
        assert!(err.to_string().contains("validator"), "{err}");

        // Misspelled role word: refused, never read as public.
        std::env::set_var("WRFCOIN_P2P_NODE_ROLE", "sentri");
        let err = build_p2p_config("/ip4/0.0.0.0/tcp/9000", None)
            .expect_err("a misspelled role must not silently become public");
        assert_eq!(err.error_code(), "NC_NODE_CFG_INVALID_P2P_ROLE");
        assert!(err.to_string().contains("sentri"), "{err}");

        // Malformed allowlist entry on a validator: refused, typed.
        std::env::set_var("WRFCOIN_P2P_NODE_ROLE", "validator");
        std::env::set_var("WRFCOIN_P2P_SENTRY_ALLOWLIST", "not-a-peer-id");
        let err = build_p2p_config("/ip4/0.0.0.0/tcp/9000", None)
            .expect_err("a malformed allowlist entry must refuse");
        assert_eq!(err.error_code(), "NC_NODE_CFG_INVALID_P2P_ROLE");
        clear();
    }

    /// Source-order pin (#2120): the role env is applied inside
    /// `build_p2p_config`, and that call precedes `P2pNode::start_*` in the
    /// boot path — the one-line follow-up #3111 recorded. If someone moves the
    /// swarm start above the config build, or drops the `apply_role_env` call,
    /// this fails before a sentry ever boots as `public` again.
    #[cfg(feature = "p2p")]
    #[::core::prelude::v1::test]
    fn role_env_is_applied_before_p2p_start_source_pin_2120() {
        // Search production code only: the test module quotes these very
        // strings, so an unscoped `find` would match its own literals.
        let full = include_str!("main.rs");
        let src = &full[..full.find("\nmod tests {").expect("tests module")];
        let build_fn = src
            .find("fn build_p2p_config(")
            .expect("build_p2p_config must exist");
        let apply = src[build_fn..]
            .find(".apply_role_env()")
            .map(|i| i + build_fn)
            .expect("build_p2p_config must call P2pConfig::apply_role_env");
        let build_call = src
            .find("build_p2p_config(&listen_address_raw")
            .expect("the boot path must build the config from env");
        let start = src
            .find("P2pNode::start_with_keypair_and_initial_chain_state(")
            .expect("the boot path must start the swarm");
        assert!(apply < build_call, "apply_role_env must live inside build_p2p_config");
        assert!(
            build_call < start,
            "the config (with the role applied) must be built before the swarm starts"
        );
    }

    /// **The typo case, driven through the production parser.**
    ///
    /// Frank found the sibling defect on #2397: `...=tru` parsed as *false* and
    /// let an unauthenticated fresh chain start. An unrecognised value must
    /// abort startup, not read as off.
    ///
    /// The previous guard asserted `matches!(v, "1" | "true" | ...)` inline — a
    /// tautology that survived Elsa replacing the real parser with `false`.
    /// This drives the raw string through `producer_startup_decision`, so a
    /// parser that ignores its input fails here.
    #[::core::prelude::v1::test]
    fn a_mistyped_auth_switch_aborts_startup_rather_than_reading_as_off_2290_b1() {
        for typo in ["tru", "ture", "enabled", "yes please", "2", "off!"] {
            let result = producer_startup_decision(
                false,
                Some(typo),
                ProducerSignerResolution::Absent,
            );
            let err = result.expect_err(&format!(
                "{typo:?} must abort startup — silently reading an unrecognised security \
                 switch as OFF is exactly how an unauthenticated node starts while its \
                 operator believes it is enforcing (#2290 blocker 1)"
            ));
            assert!(
                err.contains(PRODUCER_AUTH_REQUIRED_ENV) && err.contains(typo),
                "the refusal must name the variable and the offending value, got: {err}"
            );
        }
    }

    /// Recognised spellings must actually work — a switch that cannot be set is
    /// a switch that gets deleted. Driven through the seam, so this also proves
    /// the parser is genuinely consulted.
    #[::core::prelude::v1::test]
    fn recognised_auth_switch_spellings_are_honoured_2290_b1() {
        for on in ["1", "true", "TRUE", "Yes", "on", "  on  "] {
            assert_eq!(
                producer_startup_decision(false, Some(on), ProducerSignerResolution::Absent),
                Ok(ProducerSignerDecision::RefuseToStart),
                "{on:?} must tighten a chain with no seeded set into fail-closed"
            );
        }
        for off in ["0", "false", "No", "off"] {
            assert_eq!(
                producer_startup_decision(false, Some(off), ProducerSignerResolution::Absent),
                Ok(ProducerSignerDecision::ProduceUnsigned),
                "{off:?} on a chain with no seeded set leaves unsigned production legal"
            );
        }
        // Absent and empty are "not configured", which is not the same as off.
        assert_eq!(
            producer_startup_decision(false, None, ProducerSignerResolution::Absent),
            Ok(ProducerSignerDecision::ProduceUnsigned),
        );
        assert_eq!(
            producer_startup_decision(false, Some("   "), ProducerSignerResolution::Absent),
            Ok(ProducerSignerDecision::ProduceUnsigned),
        );
    }

    /// The override may only tighten. An operator who sets the flag to *false*
    /// on a chain that seeded a producer set is asking to disable enforcement
    /// they cannot disable — that must be a loud refusal, not a silent
    /// override, or the two sides disagree with nobody noticing.
    #[::core::prelude::v1::test]
    fn the_env_override_cannot_switch_off_an_enabled_chain_2290_b1() {
        let err = producer_startup_decision(
            true,
            Some("false"),
            ProducerSignerResolution::Resolved,
        )
        .expect_err(
            "a seeded producer set cannot be de-activated from the environment; asking to \
             must fail loudly rather than being ignored",
        );
        assert!(
            err.contains("cannot be switched off"),
            "the refusal must explain why, got: {err}"
        );
    }

    /// Scope guard: a chain that never seeded a producer set, with no override,
    /// keeps producing unsigned blocks exactly as before. Without this the fix
    /// would break every legacy and local-dev node.
    #[::core::prelude::v1::test]
    fn unsigned_production_stays_legal_on_a_chain_with_no_producer_set_2290_b1() {
        assert_eq!(
            producer_startup_decision(false, None, ProducerSignerResolution::Absent),
            Ok(ProducerSignerDecision::ProduceUnsigned),
        );
        assert_eq!(
            producer_startup_decision(false, None, ProducerSignerResolution::InitFailed),
            Ok(ProducerSignerDecision::ProduceUnsigned),
        );
    }

    /// A resolved signer is used in every mode.
    #[::core::prelude::v1::test]
    fn resolved_producer_signer_is_always_used_2290_b1() {
        assert_eq!(
            producer_startup_decision(true, None, ProducerSignerResolution::Resolved),
            Ok(ProducerSignerDecision::Sign),
        );
        assert_eq!(
            producer_startup_decision(false, None, ProducerSignerResolution::Resolved),
            Ok(ProducerSignerDecision::Sign),
        );
    }

    /// The load-bearing boot check must inspect the producer set for the block
    /// the node is about to produce, not the set that authorized the last
    /// persisted block. The future change below is deliberately effective at
    /// height 8 while the restored node is at height 7; the old genesis key is
    /// therefore an unauthorized producer at the next height and must refuse
    /// startup even when the auth switch is unset. The explicit false value is
    /// the opt-in recovery mode and remains visible as not-ready.
    #[::core::prelude::v1::test]
    fn next_producer_boot_check_refuses_a_key_missing_from_the_next_set() {
        use wrfcoin_shared_protocol::consensus::producer_set::{
            ProducerMember, ProducerSetChange,
        };

        fn member_of(pk: &PublicKey, producer_id: &str) -> ProducerMember {
            let key: [u8; 32] = pk.as_bytes().try_into().unwrap();
            ProducerMember {
                producer_id: producer_id.to_string(),
                payout_address: Address(key[..20].try_into().unwrap()).to_string(),
                public_key: key,
            }
        }

        let mut genesis_rng = StdRng::seed_from_u64(2972);
        let genesis_private_key = PrivateKey::generate(&mut genesis_rng);
        let genesis_public_key = PublicKey::from_private_key(&genesis_private_key);
        let mut next_rng = StdRng::seed_from_u64(2973);
        let next_private_key = PrivateKey::generate(&mut next_rng);
        let next_public_key = PublicKey::from_private_key(&next_private_key);
        let mut node_rng = StdRng::seed_from_u64(2974);
        let node_private_key = PrivateKey::generate(&mut node_rng);
        let node_public_key = PublicKey::from_private_key(&node_private_key);

        let mut blockchain = Blockchain::default();
        blockchain.height = 7;
        blockchain
            .state
            .seed_genesis_producer_set(vec![member_of(&genesis_public_key, "genesis")])
            .expect("genesis producer set should validate");
        blockchain.state.producer_set_history.push(ProducerSetChange::new(
            1,
            8,
            vec![member_of(&next_public_key, "next")],
        ));

        let unauthorized_signer = signer_from_test_private_key(node_private_key);
        let refusal = with_env(None, None, None, || {
            check_next_producer_authority(
                &blockchain,
                Some(&unauthorized_signer),
                NodeRole::Producing,
            )
        })
        .expect_err("a non-member producer signer must refuse default startup");
        assert!(refusal.contains("height 8"), "refusal must name next height: {refusal}");
        assert!(
            refusal.contains(&node_public_key.to_hex()),
            "refusal must name this node's key: {refusal}"
        );
        assert!(
            refusal.contains("next=0x") && refusal.contains("NOT authorized"),
            "refusal must name the resolved set and failed membership: {refusal}"
        );

        let authorized_signer = signer_from_test_private_key(next_private_key);
        let authorized = with_env(None, None, None, || {
            check_next_producer_authority(
                &blockchain,
                Some(&authorized_signer),
                NodeRole::Producing,
            )
        })
        .expect("the next-height member must pass the boot check");
        assert_eq!(authorized.status, "authorized");
        assert_eq!(authorized.next_height, 8);
        assert!(authorized.message.contains("next=0x"));

        let follower = with_env(None, None, None, || {
            check_next_producer_authority(&blockchain, None, NodeRole::SyncOnly)
        })
        .expect("a pure follower must not be blocked by producer authority");
        assert_eq!(follower.status, "follower");
        assert!(!follower.readiness_gate);
        assert!(follower.message.contains("no producer signer"));

        let permissive = with_env(None, None, Some("false"), || {
            check_next_producer_authority(
                &blockchain,
                Some(&unauthorized_signer),
                NodeRole::Producing,
            )
        })
        .expect("explicit false is the opt-in permissive recovery mode");
        assert_eq!(permissive.status, "unauthorized");
        assert!(permissive.readiness_gate);
    }

    #[::core::prelude::v1::test]
    fn mempool_config_defaults_remain_compatible_without_overrides() {
        let expected = Config::default();
        let actual = mempool_config_from_values(None, None, None, None).unwrap();

        assert_eq!(actual.mempool_max_size_bytes, expected.mempool_max_size_bytes);
        assert_eq!(
            actual.mempool_max_txs_per_sender,
            expected.mempool_max_txs_per_sender
        );
        assert_eq!(actual.mempool_max_age_secs, expected.mempool_max_age_secs);
        assert_eq!(actual.mempool_min_fee_rate, expected.mempool_min_fee_rate);
    }

    #[::core::prelude::v1::test]
    fn mempool_config_accepts_public_testnet_caps() {
        let config = mempool_config_from_values(
            Some("10485760"),
            Some("64"),
            Some("3600"),
            Some("0"),
        )
        .unwrap();

        assert_eq!(config.mempool_max_size_bytes, 10 * 1024 * 1024);
        assert_eq!(config.mempool_max_txs_per_sender, 64);
        assert_eq!(config.mempool_max_age_secs, 3600);
        assert_eq!(config.mempool_min_fee_rate, 0);
    }

    #[::core::prelude::v1::test]
    fn mempool_config_rejects_invalid_explicit_caps() {
        for (value, expected_name) in [
            (
                mempool_config_from_values(Some("0"), None, None, None),
                MEMPOOL_MAX_SIZE_BYTES_ENV,
            ),
            (
                mempool_config_from_values(None, Some("-1"), None, None),
                MEMPOOL_MAX_TXS_PER_SENDER_ENV,
            ),
            (
                mempool_config_from_values(None, None, Some("not-a-number"), None),
                MEMPOOL_MAX_AGE_SECS_ENV,
            ),
            (
                mempool_config_from_values(None, None, None, Some("-1")),
                MEMPOOL_MIN_FEE_RATE_ENV,
            ),
        ] {
            let error = value.expect_err("explicit invalid cap must fail startup");
            assert!(
                error.contains(expected_name),
                "error '{error}' must identify {expected_name}"
            );
        }
    }

    // -----------------------------------------------------------------
    // #2123 regression: restart after genesis eviction
    //
    // These drive `resolve_genesis_protocol_metadata` — the function the boot
    // path actually calls — rather than feeding metadata into the pure gate.
    // That distinction is the whole point of the finding: the 70 existing tests
    // pass because they inject metadata directly, so they never exercise the
    // acquisition path that was broken. Same shape as the #2231 registry
    // problem, where a self-consistent suite validated an incomplete thing.
    // -----------------------------------------------------------------

    fn genesis_regression_db_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "wrfcoin-2123-genesis-{}-{}.redb",
            std::process::id(),
            fastrand::u64(..)
        ))
    }

    /// Build a chain of `height_count` blocks with `protocol_version` recorded in
    /// the height-1 genesis block, persisting every block durably.
    ///
    /// `window` is the in-memory retention window; making it much smaller than
    /// `height_count` is what forces genesis out of memory, reproducing a
    /// long-running node.
    async fn build_chain_with_genesis_version(
        path: &std::path::Path,
        window: usize,
        height_count: u64,
        protocol_version: Option<&str>,
    ) {
        use wrfcoin_shared_protocol::blockchain::block::Block;
        use wrfcoin_shared_protocol::crypto::hash::Hash;

        let mut storage = Storage::open(path, 1_000).unwrap();
        // Small in-memory window so genesis is evicted, reproducing a
        // long-running node without building 200+ blocks.
        let mut blockchain = Blockchain::with_capacity(window);

        for height in 1..=height_count {
            let prev = if height == 1 {
                Hash::default()
            } else {
                blockchain.tip
            };
            let mut block = Block::new(3, prev, vec![], 1, height);
            if height == 1 {
                block
                    .metadata
                    .insert("chain_id".to_string(), "wrfcoin-testnet".to_string());
                block
                    .metadata
                    .insert("network_id".to_string(), "testnet".to_string());
                if let Some(v) = protocol_version {
                    block
                        .metadata
                        .insert("protocol_version".to_string(), v.to_string());
                }
            }
            let hash = block.calculate_hash();
            block.set_hash(hash);
            blockchain.add_block(block.clone()).unwrap();
            storage
                .persist_block(
                    block,
                    vec![],
                    storage
                        .checkpoint_state_bytes(height, &blockchain.state)
                        .unwrap(),
                )
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn genesis_protocol_metadata_survives_eviction_from_the_memory_window() {
        // THE regression. 250 blocks is past the default 200-block retention
        // window, so `Storage::load_state` does not restore genesis — exactly
        // what a restart on any real chain sees. Before this fix the gate got
        // empty metadata here, defaulted to v1 and started, while peers stayed
        // on v2.
        let path = genesis_regression_db_path();
        build_chain_with_genesis_version(&path, 5, 250, Some("2")).await;

        // Reopen as a restart would.
        let storage = Storage::open(&path, 1_000).unwrap();
        // A restart: reload from durable storage into a small window, exactly
        // as a long-running node does.
        let blockchain = storage
            .load_state()
            .unwrap()
            .expect("a persisted chain must reload");

        assert!(
            blockchain.block_at_height(1).is_none(),
            "genesis must be evicted from the in-memory window, or this test is \
             not reproducing the reported condition"
        );

        match resolve_genesis_protocol_metadata(&blockchain, &storage) {
            GenesisProtocolMetadata::Loaded(metadata) => {
                let version = metadata
                    .iter()
                    .find(|(k, _)| k == "protocol_version")
                    .map(|(_, v)| v.as_str());
                assert_eq!(
                    version,
                    Some("2"),
                    "the durable archive must still yield the chain's pinned \
                     protocol version after eviction"
                );
            },
            other => panic!("expected Loaded after eviction, got {other:?}"),
        }

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_restarted_node_cannot_install_v1_on_a_v2_chain() {
        // The plain-language claim the manager asked to be able to state. Drive
        // the real acquisition path, then the real resolver, and assert the node
        // lands on v2 — not v1.
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            ChainIdentity, SigningProtocolVersion, genesis_protocol_version, resolve_active_version,
        };

        let path = genesis_regression_db_path();
        build_chain_with_genesis_version(&path, 5, 250, Some("2")).await;

        let storage = Storage::open(&path, 1_000).unwrap();
        // A restart: reload from durable storage into a small window, exactly
        // as a long-running node does.
        let blockchain = storage
            .load_state()
            .unwrap()
            .expect("a persisted chain must reload");

        let metadata = match resolve_genesis_protocol_metadata(&blockchain, &storage) {
            GenesisProtocolMetadata::Loaded(m) => m,
            other => panic!("expected Loaded, got {other:?}"),
        };
        let pairs: Vec<(&str, &str)> = metadata
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let genesis_version = genesis_protocol_version(pairs).expect("parses");
        let identity = ChainIdentity::new("wrfcoin-testnet", "testnet");
        let effective = resolve_active_version(genesis_version, None, &identity)
            .expect("a v2 chain must resolve to v2");

        assert_eq!(
            effective,
            SigningProtocolVersion::V2,
            "a restarted node on a v2 chain must install v2 — installing v1 here \
             is the #2076 divergence"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_non_empty_chain_with_no_durable_genesis_refuses_rather_than_defaulting() {
        // "I could not find it" must never be read as "it is not set". A
        // non-empty chain whose archive cannot supply genesis is Unavailable,
        // which the boot path turns into a refusal to start.
        let path = genesis_regression_db_path();
        build_chain_with_genesis_version(&path, 5, 10, Some("2")).await;

        let storage = Storage::open(&path, 1_000).unwrap();
        // A restart: reload from durable storage into a small window, exactly
        // as a long-running node does.
        let blockchain = storage
            .load_state()
            .unwrap()
            .expect("a persisted chain must reload");

        // A separate, empty archive stands in for a lost/corrupt durable store
        // while the node still holds blocks in memory.
        let empty_path = genesis_regression_db_path();
        let empty_storage = Storage::open(&empty_path, 1_000).unwrap();

        match resolve_genesis_protocol_metadata(&blockchain, &empty_storage) {
            GenesisProtocolMetadata::Unavailable(why) => {
                assert!(
                    why.contains("inconsistent"),
                    "the refusal must say why, got: {why}"
                );
            },
            other => panic!(
                "a non-empty chain with no durable genesis must be Unavailable, got {other:?}"
            ),
        }

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&empty_path);
    }

    #[tokio::test]
    async fn a_genuinely_empty_chain_is_allowed_to_default() {
        // The other side: a fail-closed gate that refuses first boot is an
        // outage, not a gate.
        let path = genesis_regression_db_path();
        let storage = Storage::open(&path, 1_000).unwrap();
        let blockchain = Blockchain::default();

        assert_eq!(
            resolve_genesis_protocol_metadata(&blockchain, &storage),
            GenesisProtocolMetadata::EmptyChain
        );

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn an_unpinned_chain_still_resolves_after_eviction() {
        // A pre-#2123 chain records no protocol_version. After eviction it must
        // still load (as Loaded-with-no-version), not be mistaken for empty.
        let path = genesis_regression_db_path();
        build_chain_with_genesis_version(&path, 5, 250, None).await;

        let storage = Storage::open(&path, 1_000).unwrap();
        // A restart: reload from durable storage into a small window, exactly
        // as a long-running node does.
        let blockchain = storage
            .load_state()
            .unwrap()
            .expect("a persisted chain must reload");

        match resolve_genesis_protocol_metadata(&blockchain, &storage) {
            GenesisProtocolMetadata::Loaded(metadata) => {
                assert!(
                    !metadata.iter().any(|(k, _)| k == "protocol_version"),
                    "an unpinned chain records no protocol_version"
                );
                assert!(
                    metadata.iter().any(|(k, _)| k == "chain_id"),
                    "but its identity metadata must still be readable"
                );
            },
            other => panic!("expected Loaded, got {other:?}"),
        }

        let _ = std::fs::remove_file(&path);
    }

    use actix_web::http::header::RETRY_AFTER;
    use actix_web::{App, http::StatusCode, test};
    use rand::{SeedableRng, rngs::StdRng};
    use serde_json::json;
    use wrfcoin_shared_protocol::crypto::signer::test_support::FixtureSigner;
    #[::core::prelude::v1::test]
    #[ignore = "owned daemon subprocess entry; exercised by the registered receiver fixture"]
    fn node_share_rehearsal_daemon_entry() {
        assert!(std::env::var_os("WRFCOIN_NODE_SHARE_TEST_DAEMON").is_some());
        main().expect("rehearsal daemon startup");
    }

    /// Wrap a synthetic `PrivateKey` in a test-only `FixtureSigner` so existing test
    /// fixtures can continue to pass a plain key through to `AppState`
    /// without reaching into the HSM path (#1394).
    pub(crate) fn rehearsal_daemon_signer(node_role: NodeRole) -> Option<Arc<dyn BlockSigner>> {
        if node_role == NodeRole::ReceiptConsumer
        // determinism-ok: cfg(test)-only owned-daemon hardware fixture switch; absent from shipped binaries
        && std::env::var_os("WRFCOIN_NODE_SHARE_TEST_DAEMON").is_some() {
        // Synthetic hardware substitute for the real startup/install/endorsement
        // subprocess regression. Absent from every shipped binary.
        // Synthetic genesis has no production bootstrap pin; supply the fixture's
        // public authority while retaining the real startup/restore/signing path.
        // determinism-ok: cfg(test)-only public synthetic genesis authority fixture; production authority still comes exclusively from pinned block 1
        let genesis = std::env::var("WRFCOIN_GENESIS_HASH").ok()?;
        // determinism-ok: cfg(test)-only public governance fixture for synthetic genesis; no shipped verifier uses these fixture values
        let keys = std::env::var("WRFCOIN_ORACLE_GOVERNANCE_PUBKEYS").ok()?;
        wrfcoin_shared_protocol::governance::schedule_extension::install_schedule_extension_authority_for_tests(
            "777", "testnet", genesis.trim_start_matches("0x"),
            keys.split(',').map(str::to_owned).collect());
        return PrivateKey::from_bytes(&[22;32]).ok().map(signer_from_test_private_key);
    }
        None
    }

    pub(crate) fn signer_from_test_private_key(pk: PrivateKey) -> Arc<dyn BlockSigner> {
        Arc::new(FixtureSigner::from_private_key(pk, "test"))
    }
    #[cfg(feature = "p2p")]
    use wrfcoin_p2p::P2pClient;
    use wrfcoin_native_chain::blockchain::block::Block;
    use wrfcoin_native_chain::blockchain::state::{
        AuditEventKind, AuditEventRecord, LawEnforcementLevel, LawRecord, LawStatus,
        QualityPolicyEventRecord, QualityPolicyEventType, StateTransition,
    };
    use wrfcoin_native_chain::blockchain::transaction::{
        Layer2BatchSubmitPayload, OracleAttestationPayload, Transaction, TransactionInput,
        TransactionOutput, TransactionType, commit_authorization_message,
    };
    use wrfcoin_native_chain::common::types::{Address, Balance};
    use wrfcoin_native_chain::crypto::hash::Hash;
    use wrfcoin_native_chain::crypto::signature::PublicKey;
    use wrfcoin_native_chain::storage::{DEFAULT_CHECKPOINT_INTERVAL, Storage};
    use wrfcoin_shared_protocol::weather::validation::DataQualityBand;

    use crate::routes::governance::{
        QualityGovernanceProposalBuildResponse, TrustGovernanceProposalBuildResponse,
        TrustWeightsResponse,
    };
    use crate::routes::network::AuditLogResponse;
    use crate::routes::quality::{
        QualityEventsResponse, QualitySummaryResponse, TrustRewardImpactSummaryResponse,
        TrustRewardImpactTimelineResponse,
    };
    use crate::routes::signing_keys::SigningKeyRoleSnapshotResponse;
    use crate::routes::snapshot::{
        SnapshotEntriesResponse, SnapshotProofResponse, SnapshotSummaryResponse,
        SnapshotSyncPayloadResponse, build_snapshot_entries,
    };
    use crate::state::{
        CONTRIBUTION_COMMIT_RATE_LIMIT_MAX_REQUESTS,
        LEGACY_SUBMIT_RATE_LIMIT_MAX_KEYS, LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS,
        LEGACY_SUBMIT_RATE_LIMIT_WINDOW_SECS, LEGACY_SUBMIT_RETRY_AFTER_SECS,
        PERMISSIONLESS_SUBMIT_RATE_LIMIT_MAX_KEYS, PERMISSIONLESS_SUBMIT_MAX_REQUESTS,
        PERMISSIONLESS_SUBMIT_WINDOW_SECS, SlidingWindowCounter,
    };

    const TEST_OPERATOR_TOKEN: &str = "test-operator-token";
    const TEST_CONTRIBUTOR_TOKEN: &str = "test-contributor-token";
    const TEST_ORACLE_TOKEN: &str = "test-oracle-token";
    const TEST_READER_TOKEN: &str = "test-reader-token";

    /// Token-store tests mutate process-global env (`WRFCOIN_OPERATOR_TOKEN`
    /// and friends), so they take THE process-env lock.
    ///
    /// This was previously a second, independent mutex. `WRFCOIN_OPERATOR_TOKEN`
    /// is also cleared by `helpers::preflight_config_defaults` under the
    /// centralized lock, so the two never excluded each other: a token-store
    /// test could have its token cleared between `set_var` and
    /// `collect_token_store()`, making an AUTH test observe the
    /// no-tokens-configured state (#2398 review).
    fn token_store_env_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::helpers::test_env::lock()
    }

    /// Callers MUST hold [`token_store_env_lock`] — this mutates process-global
    /// env. Every current caller does; verified by audit (#2398 review).
    fn clear_token_store_env() {
        for env_var in [
            "WRFCOIN_OPERATOR_TOKEN",
            "WRFCOIN_OPERATOR_WALLET_ADDRESS",
            "WRFCOIN_OPERATOR_CONTRIBUTOR_ID",
            "WRFCOIN_OPERATOR_ORACLE_NODE_ID",
            "WRFCOIN_CONTRIBUTOR_TOKEN",
            "WRFCOIN_CONTRIBUTOR_WALLET_ADDRESS",
            "WRFCOIN_CONTRIBUTOR_ID",
            "WRFCOIN_CONTRIBUTOR_ORACLE_NODE_ID",
            "WRFCOIN_ORACLE_TOKEN",
            "WRFCOIN_ORACLE_WALLET_ADDRESS",
            "WRFCOIN_ORACLE_CONTRIBUTOR_ID",
            "WRFCOIN_ORACLE_NODE_ID",
            "WRFCOIN_READER_TOKEN",
            "WRFCOIN_READER_WALLET_ADDRESS",
            "WRFCOIN_READER_CONTRIBUTOR_ID",
            "WRFCOIN_READER_ORACLE_NODE_ID",
            "WRFCOIN_ALLOW_NO_AUTH",
        ] {
            std::env::remove_var(env_var);
        }
    }

    #[tokio::test]
    async fn storage_encryption_warning_mentions_required_env_var() {
        assert!(STORAGE_ENCRYPTION_DISABLED_WARNING.contains("WRFCOIN_STORAGE_ENCRYPTION_KEY"));
        assert!(STORAGE_ENCRYPTION_DISABLED_WARNING.contains("plaintext"));
        assert!(STORAGE_ENCRYPTION_DISABLED_WARNING.contains("1027"));
        assert!(STORAGE_ENCRYPTION_DISABLED_WARNING.contains("ALLOW_UNENCRYPTED_STORAGE"));
    }

    /// #2527: the refusal must name the freshly-cut case, not only the
    /// wrong-genesis one.
    ///
    /// The original text said the node "built or imported the wrong genesis" —
    /// correct for one of the two ways to reach this, and actively misleading for
    /// the other. A cut that has not yet reached the runbook's re-pin step
    /// produces a chain that is CORRECT against a pin that is STALE, and the
    /// operator is told they made a mistake they did not make.
    ///
    /// It matters because of when it fires: on **first restart**, not at cut time
    /// — which is exactly when someone is running a restart or WAL-recovery
    /// drill, and the message is the last thing they see.
    //
    // `actix_web::test` is imported into this module and shadows the built-in
    // `#[test]`, so a synchronous test must qualify it — same as
    // `nft_mutation_api_env_parsing_defaults_disabled` further down.
    #[::core::prelude::v1::test]
    fn genesis_hash_mismatch_guidance_names_both_causes_and_the_recovery_ordering_2527() {
        let msg = super::genesis_hash_mismatch_guidance("aaaa1111", "a3301def");

        // The two hashes must both appear, or the operator cannot tell which is which.
        assert!(msg.contains("aaaa1111"), "must name the LOCAL genesis: {msg}");
        assert!(msg.contains("a3301def"), "must name the EXPECTED genesis: {msg}");

        // Cause (a): wrong genesis imported.
        assert!(
            msg.contains("WRONG genesis"),
            "must still cover the wrong-genesis case: {msg}"
        );

        // Cause (b): freshly cut, not yet re-pinned. THE addition.
        assert!(
            msg.contains("CUT a new genesis"),
            "must name the freshly-cut case — without it the message tells an \
             operator who did the right thing that they did the wrong thing: {msg}"
        );
        assert!(
            msg.contains("re-pin"),
            "must point at the re-pin step, which is the actual fix for case (b): {msg}"
        );
        assert!(
            msg.contains("WRFCOIN_GENESIS_HASH"),
            "must name the variable that resolves it: {msg}"
        );

        // Alice's ordering finding: reaching this means WAL replay and the
        // producer-authority check already SUCCEEDED, so it is not a recovery
        // failure. Without this, a drill reaching it goes looking at the WAL.
        assert!(
            msg.contains("not a recovery failure"),
            "must say the refusal does not indicate a WAL/recovery problem: {msg}"
        );
    }

    #[tokio::test]
    async fn api_bind_warning_only_triggers_for_wildcard_host() {
        assert_eq!(api_bind_warning("0.0.0.0"), Some(EXPOSED_API_BIND_WARNING));
        assert_eq!(api_bind_warning("127.0.0.1"), None);
        assert_eq!(api_bind_warning("localhost"), None);
    }

    #[tokio::test]
    async fn parse_snapshot_trusted_signers_dedupes_and_trims() {
        let (_snapshot_sk, snapshot_pk) = keypair(301);
        let parsed = parse_snapshot_trusted_signers(&format!(" {snapshot_pk} , {snapshot_pk} "))
            .expect("trusted signers should parse");
        assert_eq!(parsed.len(), 1);
        assert!(parsed.contains(&snapshot_pk));
    }

    #[tokio::test]
    async fn parse_snapshot_trusted_signers_rejects_invalid_public_key() {
        let error =
            parse_snapshot_trusted_signers("not-a-real-public-key").expect_err("invalid signer");
        assert!(error.contains("invalid snapshot trusted signer public key"));
    }

    #[tokio::test]
    async fn parse_snapshot_trusted_signers_normalizes_hex_case() {
        // Regression test for #1390: uppercase hex input should normalize to canonical lowercase.
        let (_snapshot_sk, snapshot_pk_lower) = keypair(301);
        // Convert to uppercase for testing case normalization.
        let snapshot_pk_upper = snapshot_pk_lower.to_uppercase();

        let parsed = parse_snapshot_trusted_signers(&snapshot_pk_upper)
            .expect("uppercase hex should parse");

        // The parsed set should contain the canonical (lowercase) form.
        assert!(parsed.contains(&snapshot_pk_lower),
                "parsed signers should contain canonical lowercase hex form");
        assert!(!parsed.contains(&snapshot_pk_upper),
                "parsed signers should NOT contain uppercase input");
    }

    fn test_token_store() -> Arc<FxHashMap<String, AuthPrincipal>> {
        let mut store = FxHashMap::default();
        store.insert(
            TEST_OPERATOR_TOKEN.to_string(),
            AuthPrincipal::with_role(ApiRole::Operator),
        );
        store.insert(
            TEST_CONTRIBUTOR_TOKEN.to_string(),
            AuthPrincipal::with_role(ApiRole::Contributor),
        );
        store.insert(
            TEST_ORACLE_TOKEN.to_string(),
            AuthPrincipal::with_role(ApiRole::Oracle),
        );
        store.insert(
            TEST_READER_TOKEN.to_string(),
            AuthPrincipal::with_role(ApiRole::Reader),
        );
        Arc::new(store)
    }

    /// Tests use an in-memory evidence store — they do not need restart
    /// persistence. Production startup opens a `DurableEvidenceStore`.
    fn test_evidence_store()
    -> Arc<dyn wrfcoin_consensus_engines::equivocation::EvidenceStore> {
        Arc::new(wrfcoin_consensus_engines::equivocation::InMemoryEvidenceStore::new())
    }

    fn trusted_snapshot_signers_for_test(
        snapshot_signing_key: Option<&Arc<dyn BlockSigner>>,
    ) -> Arc<FxHashSet<String>> {
        let mut trusted_signers = FxHashSet::default();
        if let Some(signer) = snapshot_signing_key {
            trusted_signers.insert(signer.public_key().to_string());
        }
        Arc::new(trusted_signers)
    }

    fn test_consensus() -> Arc<TypedConsensusManager<wrfcoin_consensus_engines::PopConsensusAdapter>>
    {
        let pop_config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
        let pop_adapter = ConsensusFactory::create_pop_adapter(&pop_config);
        // #2523: these harness chains have no oracle ceremony, and must say so.
        // A block 1 that declares nothing is refused — that is the rule, and the
        // test harness is not exempt from stating what kind of chain it is.
        pop_adapter.declare_no_genesis_oracles_blocking();
        #[cfg(feature = "zk")]
        {
            let zk_verifier: Arc<
                dyn wrfcoin_shared_protocol::consensus::zk_verification::ZkVerifier,
            > = Arc::new(wrfcoin_native_chain::zk_service::ArkworksZkVerifier);
            pop_adapter.set_zk_verifier_blocking(zk_verifier);
        }
        Arc::new(TypedConsensusManager::new(pop_adapter, pop_config))
    }

    #[tokio::test]
    async fn launch_api_constructed_engine_rejects_unauthorized_producer() {
        // #2603 option 1: this deliberately goes through the same named
        // constructor production uses, rather than a Pop adapter test fixture.
        // `TypedConsensusManager<E>` is generic, so a different E could
        // type-check while losing the producer-auth gate. The good control
        // proves the block reaches the acceptance seam; the bad control differs
        // only in its signed, non-member producer identity.
        use rand::SeedableRng;
        use wrfcoin_shared_protocol::common::types::Address;
        use wrfcoin_shared_protocol::consensus::block_producer_auth::sign_block_producer;
        use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            ChainIdentity, SigningProtocolVersion,
        };
        use wrfcoin_shared_protocol::crypto::signature::PublicKey;

        fn member_and_key(seed: u64) -> (PrivateKey, ProducerMember) {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let private_key = PrivateKey::generate(&mut rng);
            let public_key = PublicKey::from_private_key(&private_key);
            let key: [u8; 32] = public_key.as_bytes().try_into().unwrap();
            (
                private_key,
                ProducerMember {
                    producer_id: format!("producer-{seed}"),
                    payout_address: Address(key[..20].try_into().unwrap()).to_string(),
                    public_key: key,
                },
            )
        }

        async fn signed_block(
            consensus: &TypedConsensusManager<wrfcoin_consensus_engines::PopConsensusAdapter>,
            chain: &Blockchain,
            payout: &str,
            signer: &PrivateKey,
        ) -> wrfcoin_shared_protocol::blockchain::block::Block {
            let coinbase =
                wrfcoin_shared_protocol::blockchain::transaction::Transaction::new_coinbase(
                    payout, 0, 1,
                );
            let mut block = consensus
                .create_block(vec![coinbase], &chain.tip.to_string(), payout, chain)
                .await
                .expect("launch_api PoP engine must construct the control block");
            let attestation = sign_block_producer(
                &block,
                signer,
                ChainIdentity::active(),
                SigningProtocolVersion::active(),
            );
            block.producer_pubkey = attestation.producer_pubkey;
            block.producer_sig = attestation.producer_sig.to_vec();
            block
        }

        let (member_key, member) = member_and_key(2603);
        let (outsider_key, outsider) = member_and_key(2604);
        let mut chain = Blockchain::default();
        chain
            .state
            .seed_genesis_producer_set(vec![member.clone()])
            .expect("seeded test chain must enforce producer authority from block 1");

        let config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
        let adapter = ConsensusFactory::create_pop_adapter(&config);
        // Match launch_api's explicit `WRFCOIN_GENESIS_ORACLE_SET=none` posture.
        // Without a declared posture, block 1 must refuse before this test can
        // reach producer authentication (#2523).
        adapter.declare_no_genesis_oracles_blocking();
        let consensus = launch_api_consensus_manager(adapter, config);

        let authorized = signed_block(
            &consensus,
            &chain,
            &member.payout_address,
            &member_key,
        )
        .await;
        assert!(
            consensus.validate_block(&authorized, &chain).await.unwrap(),
            "control: launch_api's constructed engine must accept a seeded producer"
        );

        let unauthorized =
            signed_block(&consensus, &chain, &outsider.payout_address, &outsider_key).await;
        assert!(
            !consensus.validate_block(&unauthorized, &chain).await.unwrap(),
            "launch_api's constructed engine must reject a signed non-member producer (#2603)"
        );
    }

    #[cfg(feature = "zk")]
    fn test_state_with_zk_prover_budget(
        snapshot_signing_key: Option<PrivateKey>,
        zk_max_provers: usize,
    ) -> web::Data<AppState> {
        let snapshot_signing_key = snapshot_signing_key.map(signer_from_test_private_key);
        let tmp = std::env::temp_dir().join(format!("wrfcoin-test-{}.redb", fastrand::u64(..)));
        let storage =
            Storage::open(&tmp, DEFAULT_CHECKPOINT_INTERVAL).expect("test storage should open");
        let mut blockchain = Blockchain::default();
        let zk_verifier: Arc<dyn wrfcoin_shared_protocol::consensus::zk_verification::ZkVerifier> =
            Arc::new(wrfcoin_native_chain::zk_service::ArkworksZkVerifier);
        blockchain.state.set_zk_verifier(zk_verifier);

        let (finality_signal_tx, _rx) =
            wrfcoin_shared_protocol::consensus::finality::finality_signal_channel();
        let snapshot_trusted_signers =
            trusted_snapshot_signers_for_test(snapshot_signing_key.as_ref());

        web::Data::new(AppState {
            consensus: test_consensus(),
            failover_monitor: Arc::new(RwLock::new(FailoverMonitor::new(
                FailoverConfig::default(),
            ))),
            blockchain: Arc::new(RwLock::new(blockchain)),
            snapshot_service: Arc::new(crate::routes::snapshot::SnapshotServiceState::default()),
            genesis_hash: crate::state::GenesisHashCell::new(None),
            producer_authority_boot: Arc::new(ProducerAuthorityBootStatus::default()),
            p2p_startup: Arc::new(P2pStartupStatus::default()),
            nft_mutation_api_enabled: true,
            oracle_admin_api_enabled: true,
            debug_seed_api_enabled: false,
            mempool: Arc::new(RwLock::new(Mempool::new(&Config::default()))),
            snapshot_sync_seen: Arc::new(RwLock::new(FxHashSet::default())),
            snapshot_signing_key,
            snapshot_trusted_signers,
            bridge_safety: Arc::new(RwLock::new(BridgeSafetyManager::new(
                BridgeSafetyConfig::default(),
            ))),
            token_store: test_token_store(),
            legacy_submit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
            permissionless_submit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
        forecast_quotas: Arc::new(RwLock::new(Default::default())),
        contribution_commit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
            permissionless_replay_seen: Arc::new(RwLock::new(FxHashMap::default())),
            storage: Arc::new(RwLock::new(storage)),
            evidence_store: test_evidence_store(),
            failed_transactions_total: Arc::new(AtomicU64::new(0)),
            zk_proofs_per_block_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        trust_reward_metrics_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        mining_tx_count_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        stake_metrics_cache: Arc::new(std::sync::Mutex::new(None)),
            block_production_lock: Arc::new(tokio::sync::Mutex::new(())),
        partition_mode: Arc::new(RwLock::new(
            wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode::default(),
        )),
        node_capability: Arc::new(RwLock::new(
            wrfcoin_native_chain::network_failover::NodeCapabilitySnapshot::default(),
        )),
            state_diverged: Arc::new(AtomicBool::new(false)),
            #[cfg(feature = "p2p")]
            tx_gossip_backpressure_streak: Arc::new(std::sync::Mutex::new(0)),
            #[cfg(feature = "p2p")]
            p2p_consumer_health: P2pConsumerHealth::new(false),
            event_tx: websocket::new_event_channel().0,
            finality_signal_tx,
            #[cfg(feature = "p2p")]
            p2p: None,
            #[cfg(feature = "p2p")]
            weather_gossip_signing_key: Arc::new(PrivateKey::generate(&mut rand::thread_rng())),
            #[cfg(feature = "p2p")]
            da_challenge_runtime: crate::da_challenge_producer::DaChallengeRuntime::new(),
            #[cfg(feature = "privacy")]
            key_image_registry: Arc::new(KeyImageRegistry::new()),
            #[cfg(feature = "privacy")]
            privacy_key_registry: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "zk")]
            zk_verifier: Arc::new(wrfcoin_native_chain::zk_service::ArkworksZkVerifier),
            #[cfg(feature = "zk")]
            zk_prover_gate: Arc::new(Semaphore::new(zk_max_provers)),
            #[cfg(feature = "execution")]
            execution_registry: Arc::new(default_registry()),
            #[cfg(feature = "execution")]
            execution_runtime: match DockerRunner::new() {
                Ok(runner) => Arc::new(runner),
                Err(_) => Arc::new(wrfcoin_execution_engine::docker::NoOpContainerRuntime),
            },
            #[cfg(feature = "execution")]
            execution_results: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            execution_token_quotas: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            execution_quota_config: crate::state::ExecutionQuotaConfig::default(),
            #[cfg(feature = "execution")]
            forecast_task_assignments: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            forecast_node_heartbeats: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            forecast_task_results: Arc::new(RwLock::new(FxHashMap::default())),
        })
    }

    fn test_state(snapshot_signing_key: Option<PrivateKey>) -> web::Data<AppState> {
        #[cfg(feature = "zk")]
        {
            return test_state_with_zk_prover_budget(
                snapshot_signing_key,
                zk_max_provers_from_override(None, 4),
            );
        }
        // Map to a `BlockSigner` unconditionally so this fallback body type-checks
        // under `--features zk` too (where it is dead code past the early return
        // above, but is still compiled). Matches `test_state_with_zk_prover_budget`
        // and `test_state_with_block_capacity`.
        let snapshot_signing_key = snapshot_signing_key.map(signer_from_test_private_key);

        let tmp = std::env::temp_dir().join(format!("wrfcoin-test-{}.redb", fastrand::u64(..)));
        let storage =
            Storage::open(&tmp, DEFAULT_CHECKPOINT_INTERVAL).expect("test storage should open");
        let blockchain = {
            #[cfg(feature = "zk")]
            {
                let mut blockchain = Blockchain::default();
                let zk_verifier: Arc<
                    dyn wrfcoin_shared_protocol::consensus::zk_verification::ZkVerifier,
                > = Arc::new(wrfcoin_native_chain::zk_service::ArkworksZkVerifier);
                blockchain.state.set_zk_verifier(zk_verifier);
                blockchain
            }
            #[cfg(not(feature = "zk"))]
            {
                Blockchain::default()
            }
        };
        let (finality_signal_tx, _rx) =
            wrfcoin_shared_protocol::consensus::finality::finality_signal_channel();
        let snapshot_trusted_signers =
            trusted_snapshot_signers_for_test(snapshot_signing_key.as_ref());

        web::Data::new(AppState {
            consensus: test_consensus(),
            failover_monitor: Arc::new(RwLock::new(FailoverMonitor::new(
                FailoverConfig::default(),
            ))),
            blockchain: Arc::new(RwLock::new(blockchain)),
            snapshot_service: Arc::new(crate::routes::snapshot::SnapshotServiceState::default()),
            genesis_hash: crate::state::GenesisHashCell::new(None),
            producer_authority_boot: Arc::new(ProducerAuthorityBootStatus::default()),
            p2p_startup: Arc::new(P2pStartupStatus::default()),
            nft_mutation_api_enabled: true,
            oracle_admin_api_enabled: true,
            debug_seed_api_enabled: false,
            mempool: Arc::new(RwLock::new(Mempool::new(&Config::default()))),
            snapshot_sync_seen: Arc::new(RwLock::new(FxHashSet::default())),
            snapshot_signing_key,
            snapshot_trusted_signers,
            bridge_safety: Arc::new(RwLock::new(BridgeSafetyManager::new(
                BridgeSafetyConfig::default(),
            ))),
            token_store: test_token_store(),
            legacy_submit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
            permissionless_submit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
        forecast_quotas: Arc::new(RwLock::new(Default::default())),
        contribution_commit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
            permissionless_replay_seen: Arc::new(RwLock::new(FxHashMap::default())),
            storage: Arc::new(RwLock::new(storage)),
            evidence_store: test_evidence_store(),
            failed_transactions_total: Arc::new(AtomicU64::new(0)),
            zk_proofs_per_block_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        trust_reward_metrics_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        mining_tx_count_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        stake_metrics_cache: Arc::new(std::sync::Mutex::new(None)),
            block_production_lock: Arc::new(tokio::sync::Mutex::new(())),
        partition_mode: Arc::new(RwLock::new(
            wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode::default(),
        )),
        node_capability: Arc::new(RwLock::new(
            wrfcoin_native_chain::network_failover::NodeCapabilitySnapshot::default(),
        )),
            state_diverged: Arc::new(AtomicBool::new(false)),
            #[cfg(feature = "p2p")]
            tx_gossip_backpressure_streak: Arc::new(std::sync::Mutex::new(0)),
            #[cfg(feature = "p2p")]
            p2p_consumer_health: P2pConsumerHealth::new(false),
            event_tx: websocket::new_event_channel().0,
            finality_signal_tx,
            #[cfg(feature = "p2p")]
            p2p: None,
            #[cfg(feature = "p2p")]
            weather_gossip_signing_key: Arc::new(PrivateKey::generate(&mut rand::thread_rng())),
            #[cfg(feature = "p2p")]
            da_challenge_runtime: crate::da_challenge_producer::DaChallengeRuntime::new(),
            #[cfg(feature = "privacy")]
            key_image_registry: Arc::new(KeyImageRegistry::new()),
            #[cfg(feature = "privacy")]
            privacy_key_registry: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "zk")]
            zk_verifier: Arc::new(wrfcoin_native_chain::zk_service::ArkworksZkVerifier),
            #[cfg(feature = "zk")]
            zk_prover_gate: Arc::new(Semaphore::new(zk_max_provers_from_override(None, 4))),
            #[cfg(feature = "execution")]
            execution_registry: Arc::new(default_registry()),
            #[cfg(feature = "execution")]
            execution_runtime: match DockerRunner::new() {
                Ok(runner) => Arc::new(runner),
                Err(_) => Arc::new(wrfcoin_execution_engine::docker::NoOpContainerRuntime),
            },
            #[cfg(feature = "execution")]
            execution_results: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            execution_token_quotas: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            execution_quota_config: crate::state::ExecutionQuotaConfig::default(),
            #[cfg(feature = "execution")]
            forecast_task_assignments: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            forecast_node_heartbeats: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            forecast_task_results: Arc::new(RwLock::new(FxHashMap::default())),
        })
    }

    #[cfg(feature = "p2p")]
    fn test_state_with_p2p(
        snapshot_signing_key: Option<PrivateKey>,
        p2p: wrfcoin_p2p::P2pClient,
    ) -> web::Data<AppState> {
        let state_data = test_state(snapshot_signing_key);
        let mut state = (*state_data.get_ref()).clone();
        state.p2p = Some(p2p);
        web::Data::new(state)
    }

    fn test_state_with_block_capacity(
        snapshot_signing_key: Option<PrivateKey>,
        max_in_memory_blocks: usize,
    ) -> web::Data<AppState> {
        let snapshot_signing_key = snapshot_signing_key.map(signer_from_test_private_key);
        #[cfg(feature = "zk")]
        {
            let tmp = std::env::temp_dir().join(format!("wrfcoin-test-{}.redb", fastrand::u64(..)));
            let storage =
                Storage::open(&tmp, DEFAULT_CHECKPOINT_INTERVAL).expect("test storage should open");
            let mut blockchain = Blockchain::with_capacity(max_in_memory_blocks);
            let zk_verifier: Arc<
                dyn wrfcoin_shared_protocol::consensus::zk_verification::ZkVerifier,
            > = Arc::new(wrfcoin_native_chain::zk_service::ArkworksZkVerifier);
            blockchain.state.set_zk_verifier(zk_verifier);

            let (finality_signal_tx, _rx) =
                wrfcoin_shared_protocol::consensus::finality::finality_signal_channel();
            let snapshot_trusted_signers =
                trusted_snapshot_signers_for_test(snapshot_signing_key.as_ref());

            return web::Data::new(AppState {
                consensus: test_consensus(),
                failover_monitor: Arc::new(RwLock::new(FailoverMonitor::new(
                    FailoverConfig::default(),
                ))),
                blockchain: Arc::new(RwLock::new(blockchain)),
                snapshot_service: Arc::new(crate::routes::snapshot::SnapshotServiceState::default()),
                genesis_hash: crate::state::GenesisHashCell::new(None),
                producer_authority_boot: Arc::new(ProducerAuthorityBootStatus::default()),
                p2p_startup: Arc::new(P2pStartupStatus::default()),
                nft_mutation_api_enabled: true,
                oracle_admin_api_enabled: true,
                debug_seed_api_enabled: false,
                mempool: Arc::new(RwLock::new(Mempool::new(&Config::default()))),
                snapshot_sync_seen: Arc::new(RwLock::new(FxHashSet::default())),
                snapshot_signing_key,
                snapshot_trusted_signers,
                bridge_safety: Arc::new(RwLock::new(BridgeSafetyManager::new(
                    BridgeSafetyConfig::default(),
                ))),
                token_store: test_token_store(),
                legacy_submit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
                permissionless_submit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
        forecast_quotas: Arc::new(RwLock::new(Default::default())),
        contribution_commit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
                permissionless_replay_seen: Arc::new(RwLock::new(FxHashMap::default())),
                storage: Arc::new(RwLock::new(storage)),
                evidence_store: test_evidence_store(),
                failed_transactions_total: Arc::new(AtomicU64::new(0)),
                zk_proofs_per_block_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        trust_reward_metrics_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        mining_tx_count_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        stake_metrics_cache: Arc::new(std::sync::Mutex::new(None)),
                block_production_lock: Arc::new(tokio::sync::Mutex::new(())),
        partition_mode: Arc::new(RwLock::new(
            wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode::default(),
        )),
        node_capability: Arc::new(RwLock::new(
            wrfcoin_native_chain::network_failover::NodeCapabilitySnapshot::default(),
        )),
                state_diverged: Arc::new(AtomicBool::new(false)),
                #[cfg(feature = "p2p")]
                tx_gossip_backpressure_streak: Arc::new(std::sync::Mutex::new(0)),
                #[cfg(feature = "p2p")]
                p2p_consumer_health: P2pConsumerHealth::new(false),
                event_tx: websocket::new_event_channel().0,
                finality_signal_tx,
                #[cfg(feature = "p2p")]
                p2p: None,
                #[cfg(feature = "p2p")]
                weather_gossip_signing_key: Arc::new(PrivateKey::generate(&mut rand::thread_rng())),
                #[cfg(feature = "p2p")]
                da_challenge_runtime: crate::da_challenge_producer::DaChallengeRuntime::new(),
                #[cfg(feature = "privacy")]
                key_image_registry: Arc::new(KeyImageRegistry::new()),
                #[cfg(feature = "privacy")]
                privacy_key_registry: Arc::new(RwLock::new(FxHashMap::default())),
                #[cfg(feature = "zk")]
                zk_verifier: Arc::new(wrfcoin_native_chain::zk_service::ArkworksZkVerifier),
                #[cfg(feature = "zk")]
                zk_prover_gate: Arc::new(Semaphore::new(zk_max_provers_from_override(None, 4))),
                #[cfg(feature = "execution")]
                execution_registry: Arc::new(default_registry()),
                #[cfg(feature = "execution")]
                execution_runtime: match DockerRunner::new() {
                    Ok(runner) => Arc::new(runner),
                    Err(_) => Arc::new(wrfcoin_execution_engine::docker::NoOpContainerRuntime),
                },
                #[cfg(feature = "execution")]
                execution_results: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            execution_token_quotas: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            execution_quota_config: crate::state::ExecutionQuotaConfig::default(),
            #[cfg(feature = "execution")]
            forecast_task_assignments: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            forecast_node_heartbeats: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            forecast_task_results: Arc::new(RwLock::new(FxHashMap::default())),
        });
        }

        let tmp = std::env::temp_dir().join(format!("wrfcoin-test-{}.redb", fastrand::u64(..)));
        let storage =
            Storage::open(&tmp, DEFAULT_CHECKPOINT_INTERVAL).expect("test storage should open");
        let blockchain = Blockchain::with_capacity(max_in_memory_blocks);
        let (finality_signal_tx, _rx) =
            wrfcoin_shared_protocol::consensus::finality::finality_signal_channel();
        let snapshot_trusted_signers =
            trusted_snapshot_signers_for_test(snapshot_signing_key.as_ref());
        web::Data::new(AppState {
            consensus: test_consensus(),
            failover_monitor: Arc::new(RwLock::new(FailoverMonitor::new(
                FailoverConfig::default(),
            ))),
            blockchain: Arc::new(RwLock::new(blockchain)),
            snapshot_service: Arc::new(crate::routes::snapshot::SnapshotServiceState::default()),
            genesis_hash: crate::state::GenesisHashCell::new(None),
            producer_authority_boot: Arc::new(ProducerAuthorityBootStatus::default()),
            p2p_startup: Arc::new(P2pStartupStatus::default()),
            nft_mutation_api_enabled: true,
            oracle_admin_api_enabled: true,
            debug_seed_api_enabled: false,
            mempool: Arc::new(RwLock::new(Mempool::new(&Config::default()))),
            snapshot_sync_seen: Arc::new(RwLock::new(FxHashSet::default())),
            snapshot_signing_key,
            snapshot_trusted_signers,
            bridge_safety: Arc::new(RwLock::new(BridgeSafetyManager::new(
                BridgeSafetyConfig::default(),
            ))),
            token_store: test_token_store(),
            legacy_submit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
            permissionless_submit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
        forecast_quotas: Arc::new(RwLock::new(Default::default())),
        contribution_commit_rate_limit: Arc::new(RwLock::new(FxHashMap::default())),
            permissionless_replay_seen: Arc::new(RwLock::new(FxHashMap::default())),
            storage: Arc::new(RwLock::new(storage)),
            evidence_store: test_evidence_store(),
            failed_transactions_total: Arc::new(AtomicU64::new(0)),
            zk_proofs_per_block_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        trust_reward_metrics_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        mining_tx_count_cache: Arc::new(std::sync::Mutex::new(FxHashMap::default())),
        stake_metrics_cache: Arc::new(std::sync::Mutex::new(None)),
            block_production_lock: Arc::new(tokio::sync::Mutex::new(())),
        partition_mode: Arc::new(RwLock::new(
            wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode::default(),
        )),
        node_capability: Arc::new(RwLock::new(
            wrfcoin_native_chain::network_failover::NodeCapabilitySnapshot::default(),
        )),
            state_diverged: Arc::new(AtomicBool::new(false)),
            #[cfg(feature = "p2p")]
            tx_gossip_backpressure_streak: Arc::new(std::sync::Mutex::new(0)),
            #[cfg(feature = "p2p")]
            p2p_consumer_health: P2pConsumerHealth::new(false),
            event_tx: websocket::new_event_channel().0,
            finality_signal_tx,
            #[cfg(feature = "p2p")]
            p2p: None,
            #[cfg(feature = "p2p")]
            weather_gossip_signing_key: Arc::new(PrivateKey::generate(&mut rand::thread_rng())),
            #[cfg(feature = "p2p")]
            da_challenge_runtime: crate::da_challenge_producer::DaChallengeRuntime::new(),
            #[cfg(feature = "privacy")]
            key_image_registry: Arc::new(KeyImageRegistry::new()),
            #[cfg(feature = "privacy")]
            privacy_key_registry: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "zk")]
            zk_verifier: Arc::new(wrfcoin_native_chain::zk_service::ArkworksZkVerifier),
            #[cfg(feature = "zk")]
            zk_prover_gate: Arc::new(Semaphore::new(zk_max_provers_from_override(None, 4))),
            #[cfg(feature = "execution")]
            execution_registry: Arc::new(default_registry()),
            #[cfg(feature = "execution")]
            execution_runtime: match DockerRunner::new() {
                Ok(runner) => Arc::new(runner),
                Err(_) => Arc::new(wrfcoin_execution_engine::docker::NoOpContainerRuntime),
            },
            #[cfg(feature = "execution")]
            execution_results: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            execution_token_quotas: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            execution_quota_config: crate::state::ExecutionQuotaConfig::default(),
            #[cfg(feature = "execution")]
            forecast_task_assignments: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            forecast_node_heartbeats: Arc::new(RwLock::new(FxHashMap::default())),
            #[cfg(feature = "execution")]
            forecast_task_results: Arc::new(RwLock::new(FxHashMap::default())),
        })
    }

    fn keypair(seed: u64) -> (PrivateKey, String) {
        let mut rng = StdRng::seed_from_u64(seed);
        let private_key = PrivateKey::generate(&mut rng);
        let public_key = PublicKey::from_private_key(&private_key).to_string();
        (private_key, public_key)
    }

    fn test_mempool_tx(tag: u32) -> Transaction {
        Transaction::new(
            vec![TransactionInput {
                txid: Hash::default(),
                vout: tag,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 1,
                address: format!("wrf-test-addr-{tag}"),
                script: "OP_TRUE".to_string(),
                nft_data: None,
            }],
        )
    }

    #[actix_web::test]
    async fn host_metrics_gpu_keeps_operator_auth_and_existing_fields() {
        let app = test::init_service(test_app(test_state(None))).await;
        let path = "/node/metrics/host";
        let response = test::call_service(&app, test::TestRequest::get().uri(path).to_request()).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        for token in [TEST_READER_TOKEN, TEST_CONTRIBUTOR_TOKEN, TEST_ORACLE_TOKEN] {
            let response = test::call_service(&app, test::TestRequest::get().uri(path)
                .insert_header(("Authorization", format!("Bearer {token}"))) // secret-leak-ok: fixed test tokens, never deployed credentials.
                .to_request()).await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        let response = test::call_service(&app, test::TestRequest::get().uri(path)
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed test bearer, never a deployed credential.
            .to_request()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(response).await;
        for field in ["memory_total_mb", "memory_used_mb", "memory_free_mb", "memory_used_percent",
            "swap_total_mb", "swap_used_mb", "cpu_per_core", "cpu_usage_avg_percent", "disks"]
        {
            assert!(body.get(field).is_some(), "existing field {field} must remain");
        }
        if let Some(gpu) = body.get("gpu") {
            assert!(gpu.is_array(), "optional GPU section must not be null or an error");
        }
    }

    fn test_app(
        state: web::Data<AppState>,
    ) -> actix_web::App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .app_data(state)
            .app_data(launch_api_json_config())
            .configure(configure_launch_routes)
    }

    /// #3586: only the isolated ReceiptConsumer (marker app data registered in `main`)
    /// may post a receiver-ingest body above 2 MiB; every other role, and
    /// bonded-ingest, keep the 2 MiB cap. Bodies are valid JSON of the wrong shape,
    /// so a size pass surfaces as 400 (shape) and a size refusal as 413.
    #[actix_web::test]
    async fn receiver_ingest_limit_is_larger_only_for_the_receipt_consumer() {
        use routes::node_receipts::{
            MAX_NODE_BONDED_SNAPSHOT_INGEST_BYTES, MAX_NODE_RECEIVER_SNAPSHOT_INGEST_BYTES,
            ReceiptConsumerIngestLimit,
        };
        assert_eq!(MAX_NODE_BONDED_SNAPSHOT_INGEST_BYTES, 2 * 1024 * 1024);
        assert_eq!(MAX_NODE_RECEIVER_SNAPSHOT_INGEST_BYTES, 1024 * 1024 * 1024);
        let big = format!("{{\"pad\":\"{}\"}}", "x".repeat(3 * 1024 * 1024));
        let small = "{\"pad\":\"x\"}".to_string();
        let post = |path: &str, body: &str| {
            test::TestRequest::post()
                .uri(path)
                .insert_header(("Content-Type", "application/json"))
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: fixed test bearer defined in main.rs; never a deployed credential.
                .set_payload(body.to_string())
                .to_request()
        };
        let normal = test::init_service(test_app(test_state(None))).await;
        let consumer = test::init_service(
            test_app(test_state(None))
                .app_data(ReceiptConsumerIngestLimit(MAX_NODE_RECEIVER_SNAPSHOT_INGEST_BYTES)),
        )
        .await;
        let path = "/network/snapshot-sync/receiver-ingest";
        // Normal role: over 2 MiB is refused with 413, under it reaches shape validation.
        assert_eq!(test::call_service(&normal, post(path, &big)).await.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(test::call_service(&normal, post(path, &small)).await.status(), StatusCode::BAD_REQUEST);
        // Receipt consumer: the same 3 MiB body passes the size limit.
        assert_eq!(test::call_service(&consumer, post(path, &big)).await.status(), StatusCode::BAD_REQUEST);
        // The consumer marker does not raise any other route's cap.
        // (the ordinary ingest route keeps the app-wide 64 KiB limit and its 400 overflow envelope)
        let other = test::call_service(&consumer, post("/network/snapshot-sync/ingest", &big)).await;
        assert_eq!(other.status(), StatusCode::BAD_REQUEST);
        let other = String::from_utf8_lossy(&test::read_body(other).await).into_owned();
        assert!(other.contains("exceeds"), "{other}");
    }

    #[cfg(feature = "zk")]
    #[::core::prelude::v1::test]
    fn zk_default_budget_reserves_two_cores_with_floor_of_one() {
        assert_eq!(default_zk_max_provers(1), 1);
        assert_eq!(default_zk_max_provers(2), 1);
        assert_eq!(default_zk_max_provers(3), 1);
        assert_eq!(default_zk_max_provers(4), 2);
        assert_eq!(default_zk_max_provers(8), 6);
    }

    #[cfg(feature = "zk")]
    #[::core::prelude::v1::test]
    fn zk_budget_override_uses_positive_env_value() {
        assert_eq!(zk_max_provers_from_override(Some("5"), 8), 5);
        assert_eq!(zk_max_provers_from_override(Some(" 2 "), 8), 2);
    }

    #[cfg(feature = "zk")]
    #[::core::prelude::v1::test]
    fn zk_budget_override_falls_back_for_invalid_values() {
        assert_eq!(zk_max_provers_from_override(Some("0"), 8), 6);
        assert_eq!(zk_max_provers_from_override(Some("nope"), 8), 6);
        assert_eq!(zk_max_provers_from_override(Some(""), 8), 6);
        assert_eq!(zk_max_provers_from_override(None, 8), 6);
    }

    #[cfg(feature = "zk")]
    #[actix_web::test]
    async fn zk_generation_endpoint_returns_429_when_gate_is_exhausted() {
        let state = test_state_with_zk_prover_budget(None, 1);
        let held_permit = state
            .zk_prover_gate
            .clone()
            .acquire_owned()
            .await
            .expect("test gate permit should be available");
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/zk/range-proof/generate")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({
                    "value": 42,
                    "blinding_factor": "11".repeat(32),
                }))
                .to_request(),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after = resp
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .expect("saturated ZK responses should include Retry-After");
        assert_eq!(retry_after, "1");
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(
            body["error"],
            "ZK proof generation capacity exhausted; retry later."
        );

        drop(held_permit);
    }

    #[cfg(feature = "zk")]
    #[actix_web::test]
    async fn zk_verify_endpoint_bypasses_generation_gate() {
        let state = test_state_with_zk_prover_budget(None, 1);
        let held_permit = state
            .zk_prover_gate
            .clone()
            .acquire_owned()
            .await
            .expect("test gate permit should be available");
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/zk/range-proof/verify")
                // Verify requires Reader; without it we 401 before ever reaching
                // the gate-bypass logic this test is asserting on.
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .set_json(json!({
                    "proof": "not-hex",
                    "commitment": "22".repeat(32),
                }))
                .to_request(),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value = test::read_body_json(resp).await;
        let error = body["error"]
            .as_str()
            .expect("verify failures should return a string error");
        assert!(
            error.contains("hex decode error"),
            "unexpected verify error: {error}"
        );

        drop(held_permit);
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn invalid_startup_listen_multiaddr_returns_typed_error() {
        let err =
            build_p2p_config("not-a-multiaddr", Some("")).expect_err("invalid listen must fail");
        assert_eq!(err.error_code(), "NC_NODE_BUG_INVALID_LISTEN_MULTIADDR");
        match err {
            StartupConfigError::InvalidListenMultiaddr {
                listen_address,
                reason,
            } => {
                assert_eq!(listen_address, "not-a-multiaddr");
                assert!(!reason.trim().is_empty());
            },
            other => panic!("expected InvalidListenMultiaddr, got {other:?}"),
        }
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn unparseable_external_addr_refuses_startup_config() {
        // #2819: a set-but-unparseable external address must be a typed
        // startup error, not a silent None — silent None is how a node ends
        // up advertising only bridge/loopback listen addresses to the mesh.
        std::env::set_var("WRFCOIN_P2P_EXTERNAL_ADDR", "192.168.0.5:9000");
        let err = build_p2p_config("/ip4/0.0.0.0/tcp/9000", None)
            .expect_err("unparseable external addr must fail closed");
        std::env::remove_var("WRFCOIN_P2P_EXTERNAL_ADDR");
        assert_eq!(err.error_code(), "NC_NODE_CFG_INVALID_EXTERNAL_ADDR");
        let rendered = err.to_string();
        assert!(rendered.contains("WRFCOIN_P2P_EXTERNAL_ADDR"));
        assert!(!rendered.contains("192.168.0.5"));

        let invalid_port = StartupConfigError::InvalidPort {
            env_var: "WRFCOIN_P2P_PORT",
        }
        .to_string();
        assert!(invalid_port.contains("WRFCOIN_P2P_PORT"));
        assert!(invalid_port.contains("operator value redacted"));

        // Unset and empty both mean "no external address", not an error.
        std::env::set_var("WRFCOIN_P2P_EXTERNAL_ADDR", "  ");
        let config = build_p2p_config("/ip4/0.0.0.0/tcp/9000", None)
            .expect("empty external addr is unset");
        std::env::remove_var("WRFCOIN_P2P_EXTERNAL_ADDR");
        assert!(config.external_address.is_none());

        // A valid multiaddr is carried into the config.
        std::env::set_var("WRFCOIN_P2P_EXTERNAL_ADDR", "/ip4/100.64.0.1/tcp/9000");
        let config = build_p2p_config("/ip4/0.0.0.0/tcp/9000", None)
            .expect("valid external addr must parse");
        std::env::remove_var("WRFCOIN_P2P_EXTERNAL_ADDR");
        assert_eq!(
            config.external_address.map(|a| a.to_string()),
            Some("/ip4/100.64.0.1/tcp/9000".to_string())
        );
    }

    #[actix_web::test]
    async fn debug_seed_account_is_gated_off_by_default_and_does_not_mutate_state() {
        // #1811 detector: with the consensus-safety gate off by default,
        // POST /debug/seed-account must return 503 and leave the state_root
        // untouched — it cannot mutate state_root-hashed accounts/stake on a
        // release multi-node deployment. test_state sets debug_seed_api_enabled
        // = false (the production default via debug_seed_api_enabled_from_env).
        let state = test_state(None);
        let root_before = state.blockchain.read().await.state.state_root.0;

        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;

        let req = test::TestRequest::post()
            .uri("/debug/seed-account")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
            .set_json(json!({
                "address": "0x1111111111111111111111111111111111111111",
                "amount": 1_000_000u128,
            }))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "gated debug seed route must return 503 by default"
        );

        // The gate short-circuits before any mutation: state_root is unchanged.
        let root_after = state.blockchain.read().await.state.state_root.0;
        assert_eq!(
            root_before, root_after,
            "a gated debug seed call must not mutate consensus state_root"
        );
    }

    #[actix_web::test]
    async fn node_network_failover_endpoint_reflects_probe_transitions() {
        // #1798 detector: injecting simulated probe transitions into the shared
        // FailoverMonitor must move the /node/network/failover endpoint through
        // PrimaryOnline -> FailoverActive -> recovery (PrimaryOnline), proving
        // the monitor is actually wired into AppState and served by the route.
        use wrfcoin_native_chain::network_failover::{
            FailoverConfig, FailoverMonitor, ProbeResult, WanState,
        };

        let state = test_state(None);
        // Install a fast-transition monitor so the test is deterministic without
        // real network probes or grace-period waits.
        let fast_config = FailoverConfig {
            probe_failure_threshold: 1,
            grace_period: std::time::Duration::ZERO,
            primary_recovery_threshold: 1,
            ..FailoverConfig::default()
        };
        *state.failover_monitor.write().await = FailoverMonitor::new(fast_config);

        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;

        let get_failover = || {
            test::TestRequest::get()
                .uri("/node/network/failover")
                .to_request()
        };

        // 1. Initial state: primary online, not metered.
        let resp = test::call_service(&app, get_failover()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["wan_state"], "PrimaryOnline");
        assert_eq!(body["metered"], false);
        assert_eq!(body["failover_count"], 0);

        // 2. Drive to FailoverActive: one failure enters Probing, a second
        //    (with zero grace) trips failover.
        {
            let mut monitor = state.failover_monitor.write().await;
            monitor.on_probe_result(ProbeResult::Unreachable);
            monitor.on_probe_result(ProbeResult::Unreachable);
            assert_eq!(monitor.state(), WanState::FailoverActive);
        }
        let resp = test::call_service(&app, get_failover()).await;
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["wan_state"], "FailoverActive");
        assert_eq!(body["metered"], true);
        assert_eq!(body["failover_count"], 1);

        // 3. Recovery: a reachable probe restores PrimaryOnline (recovery
        //    threshold 1), and the failover_count is retained.
        {
            let mut monitor = state.failover_monitor.write().await;
            monitor.on_probe_result(ProbeResult::Reachable);
            assert_eq!(monitor.state(), WanState::PrimaryOnline);
        }
        let resp = test::call_service(&app, get_failover()).await;
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["wan_state"], "PrimaryOnline");
        assert_eq!(body["metered"], false);
        assert_eq!(body["failover_count"], 1);
    }

    #[actix_web::test]
    async fn signing_key_epoch_register_revoke_and_query_routes_work() {
        let (_oracle_sk, oracle_id) = keypair(101);
        let (_rotated_sk, rotated_pk) = keypair(102);
        let state = test_state(None);

        {
            let mut blockchain = state.blockchain.write().await;
            blockchain
                .apply_state_transition(StateTransition::SetOracleConfig {
                    oracle_set: vec![oracle_id.clone()],
                    oracle_quorum: 1,
                })
                .expect("oracle config should initialize");
        }

        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;

        let register_request = test::TestRequest::post()
            .uri("/transactions/signing-keys/register")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
            .set_json(json!({
                "identity": oracle_id,
                "role": "oracle",
                "epoch": 1,
                "public_key": rotated_pk
            }))
            .to_request();
        let register_response = test::call_service(&app, register_request).await;
        assert_eq!(register_response.status(), StatusCode::OK);

        // #1258/#1267 restricted signing-key registry snapshots to Operators.
        let list_request = test::TestRequest::get()
            .uri("/transactions/signing-keys/oracle")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
            .to_request();
        let list_response = test::call_service(&app, list_request).await;
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_payload: SigningKeyRoleSnapshotResponse =
            test::read_body_json(list_response).await;
        assert_eq!(list_payload.role, "oracle");
        assert!(
            list_payload
                .identities
                .iter()
                .any(|item| item.epochs.iter().any(|entry| entry.epoch == 1))
        );

        let revoke_request = test::TestRequest::post()
            .uri("/transactions/signing-keys/revoke")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
            .set_json(json!({
                "identity": list_payload.identities[0].identity,
                "role": "oracle",
                "epoch": 1
            }))
            .to_request();
        let revoke_response = test::call_service(&app, revoke_request).await;
        assert_eq!(revoke_response.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn snapshot_proof_and_inclusion_verify_routes_work() {
        let state = test_state(None);
        {
            let mut blockchain = state.blockchain.write().await;
            let address = "0x1111111111111111111111111111111111111111"
                .parse::<Address>()
                .expect("valid address");
            blockchain
                .apply_state_transition(StateTransition::ApplyReward {
                    address,
                    amount: Balance(250),
                    reason: "test-seed".to_string(),
                })
                .expect("reward transition should seed state");
        }

        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;

        let summary_request = test::TestRequest::get()
            .uri("/state/snapshot")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .to_request();
        let summary_response = test::call_service(&app, summary_request).await;
        assert_eq!(summary_response.status(), StatusCode::OK);
        let summary: SnapshotSummaryResponse = test::read_body_json(summary_response).await;
        assert!(summary.entry_count > 0);

        let entries_request = test::TestRequest::get()
            .uri("/state/snapshot/entries?offset=0&limit=10")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .to_request();
        let entries_response = test::call_service(&app, entries_request).await;
        assert_eq!(entries_response.status(), StatusCode::OK);
        let entries_payload: SnapshotEntriesResponse = test::read_body_json(entries_response).await;
        assert!(!entries_payload.entries.is_empty());

        let first_key_hex = entries_payload.entries[0].key_hex.clone();
        let proof_request = test::TestRequest::get()
            .uri(&format!("/state/snapshot/proof/{first_key_hex}"))
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .to_request();
        let proof_response = test::call_service(&app, proof_request).await;
        assert_eq!(proof_response.status(), StatusCode::OK);
        let proof_payload: SnapshotProofResponse = test::read_body_json(proof_response).await;

        let blockchain = state.blockchain.read().await;
        let state_entries = build_snapshot_entries(&blockchain.state).expect("snapshot export");
        let first = state_entries
            .iter()
            .find(|(key, _)| hex::encode(key) == first_key_hex)
            .expect("proof key must exist in state entries");
        let verify_request = test::TestRequest::post()
            .uri("/state/snapshot/verify-inclusion")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .set_json(json!({
                "proof": proof_payload.proof,
                "key_hex": hex::encode(&first.0),
                "value_hex": hex::encode(&first.1)
            }))
            .to_request();
        let verify_response = test::call_service(&app, verify_request).await;
        assert_eq!(verify_response.status(), StatusCode::OK);
        let verify_json: serde_json::Value = test::read_body_json(verify_response).await;
        assert_eq!(
            verify_json.get("valid").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[actix_web::test]
    async fn snapshot_proof_routes_reject_bad_input_and_tampering() {
        let state = test_state(None);
        {
            let mut blockchain = state.blockchain.write().await;
            let address = "0x1111111111111111111111111111111111111111"
                .parse::<Address>()
                .expect("valid address");
            blockchain
                .apply_state_transition(StateTransition::ApplyReward {
                    address,
                    amount: Balance(250),
                    reason: "test-seed".to_string(),
                })
                .expect("reward transition should seed state");
        }

        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;

        let bad_hex_request = test::TestRequest::get()
            .uri("/state/snapshot/proof/not-hex")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .to_request();
        let bad_hex_response = test::call_service(&app, bad_hex_request).await;
        assert_eq!(bad_hex_response.status(), StatusCode::BAD_REQUEST);

        let missing_key_request = test::TestRequest::get()
            .uri(&format!(
                "/state/snapshot/proof/{}",
                hex::encode(b"missing/key")
            ))
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .to_request();
        let missing_key_response = test::call_service(&app, missing_key_request).await;
        assert_eq!(missing_key_response.status(), StatusCode::NOT_FOUND);

        let entries_request = test::TestRequest::get()
            .uri("/state/snapshot/entries?offset=0&limit=10")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .to_request();
        let entries_response = test::call_service(&app, entries_request).await;
        assert_eq!(entries_response.status(), StatusCode::OK);
        let entries_payload: SnapshotEntriesResponse = test::read_body_json(entries_response).await;
        let first_key_hex = entries_payload.entries[0].key_hex.clone();

        let proof_request = test::TestRequest::get()
            .uri(&format!("/state/snapshot/proof/{first_key_hex}"))
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .to_request();
        let proof_response = test::call_service(&app, proof_request).await;
        assert_eq!(proof_response.status(), StatusCode::OK);
        let proof_payload: SnapshotProofResponse = test::read_body_json(proof_response).await;

        let blockchain = state.blockchain.read().await;
        let state_entries = build_snapshot_entries(&blockchain.state).expect("snapshot export");
        let first = state_entries
            .iter()
            .find(|(key, _)| hex::encode(key) == first_key_hex)
            .expect("proof key must exist in state entries");

        let bad_verify_key_request = test::TestRequest::post()
            .uri("/state/snapshot/verify-inclusion")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .set_json(json!({
                "proof": proof_payload.proof.clone(),
                "key_hex": "not-hex",
                "value_hex": hex::encode(&first.1)
            }))
            .to_request();
        let bad_verify_key_response = test::call_service(&app, bad_verify_key_request).await;
        assert_eq!(bad_verify_key_response.status(), StatusCode::BAD_REQUEST);

        let bad_verify_value_request = test::TestRequest::post()
            .uri("/state/snapshot/verify-inclusion")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .set_json(json!({
                "proof": proof_payload.proof.clone(),
                "key_hex": hex::encode(&first.0),
                "value_hex": "xyz"
            }))
            .to_request();
        let bad_verify_value_response = test::call_service(&app, bad_verify_value_request).await;
        assert_eq!(bad_verify_value_response.status(), StatusCode::BAD_REQUEST);

        let tampered_verify_request = test::TestRequest::post()
            .uri("/state/snapshot/verify-inclusion")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .set_json(json!({
                "proof": proof_payload.proof,
                "key_hex": hex::encode(&first.0),
                "value_hex": hex::encode(b"tampered")
            }))
            .to_request();
        let tampered_verify_response = test::call_service(&app, tampered_verify_request).await;
        assert_eq!(tampered_verify_response.status(), StatusCode::OK);
        let tampered_verify_json: serde_json::Value =
            test::read_body_json(tampered_verify_response).await;
        assert_eq!(
            tampered_verify_json
                .get("valid")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
    }

    /// Lane C (#3126): the serve/fetch/receipt loop over the real routes —
    /// the export pages carry values that reproduce the signed root, an
    /// ingest naming the server yields a consumer-signed receipt, the server
    /// accepts it once (a replay is 409) and lists it for the operator.
    #[actix_web::test]
    async fn snapshot_export_cache_rotates_and_is_node_scoped_3129() {
        let state = test_state(None);
        let other = test_state(None);
        let app = test::init_service(App::new().app_data(state.clone()).configure(configure_launch_routes)).await;
        let other_app = test::init_service(App::new().app_data(other.clone()).configure(configure_launch_routes)).await;
        for _ in 0..2 {
            let response = test::call_service(&app, test::TestRequest::get()
                .uri("/state/snapshot/export?offset=0&limit=1")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
                .to_request()).await;
            assert_eq!(response.status(), StatusCode::OK);
        }
        assert_eq!(state.snapshot_service.export_builds.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(other.snapshot_service.export_builds.load(std::sync::atomic::Ordering::Relaxed), 0);
        let response = test::call_service(&other_app, test::TestRequest::get()
            .uri("/state/snapshot/export?limit=1")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
            .to_request()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(other.snapshot_service.export_builds.load(std::sync::atomic::Ordering::Relaxed), 1);
        {
            let mut chain = state.blockchain.write().await;
            *chain = crate::routes::snapshot::snapshot_test_support::with_chain_committing_to_its_state(
                std::mem::take(&mut *chain), 5);
        }
        let response = test::call_service(&app, test::TestRequest::get()
            .uri("/state/snapshot/export?limit=1")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
            .to_request()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let page: crate::routes::snapshot::SnapshotExportResponse = test::read_body_json(response).await;
        assert_eq!(page.height, 5);
        assert_eq!(state.snapshot_service.export_builds.load(std::sync::atomic::Ordering::Relaxed), 2);
        assert_eq!(other.snapshot_service.export_builds.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[actix_web::test]
    async fn snapshot_export_ingest_and_service_receipt_round_trip_3126() {
        use wrfcoin_shared_protocol::blockchain::snapshot_sync::{
            build_state_snapshot,
        };
        use wrfcoin_shared_protocol::consensus::node_service_receipt::SignedNodeServiceReceipt;

        let (snapshot_sk, _snapshot_pk) = keypair(3126);
        let state = test_state(Some(snapshot_sk.clone()));
        // Current snapshots carry the source genesis and block window.
        let source_genesis = {
            let mut chain = state.blockchain.write().await;
            *chain = crate::routes::snapshot::snapshot_test_support::with_chain_committing_to_its_state(
                std::mem::take(&mut *chain), 5,
            );
            chain.block_at_height(1).expect("genesis").hash.expect("hashed").to_hex()
        };
        let _genesis_pin = crate::routes::snapshot::TestGenesisPin::set(source_genesis);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;

        // 1. Export pages carry values; all pages together reproduce the served root.
        let served: SnapshotSyncPayloadResponse = {
            let resp = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri("/state/snapshot/sync-payload")
                    .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
                    .to_request(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::OK);
            test::read_body_json(resp).await
        };
        let mut offset = 0usize;
        let mut fetched: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        loop {
            let resp = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri(&format!("/state/snapshot/export?offset={offset}&limit=3"))
                    .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
                    .to_request(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::OK);
            let page: crate::routes::snapshot::SnapshotExportResponse = test::read_body_json(resp).await;
            assert_eq!(page.snapshot_root_hex, served.payload.snapshot_root_hex, "every page is bound to the served root");
            assert_eq!(page.height, served.payload.height);
            for kv in &page.entries {
                fetched.push((hex::decode(&kv.key_hex).unwrap(), hex::decode(&kv.value_hex).unwrap()));
            }
            match page.next_offset {
                Some(next) => offset = next,
                None => break,
            }
        }
        assert_eq!(fetched.len() as u64, served.payload.entry_count);
        let rebuilt = build_state_snapshot(served.payload.height, served.payload.state_root_hex.clone(), &fetched);
        assert_eq!(rebuilt.snapshot_root_hex, served.payload.snapshot_root_hex, "exported values reproduce the signed root");

        assert_eq!(state.snapshot_service.export_builds.load(std::sync::atomic::Ordering::Relaxed), 1,
            "signed payload and all export pages compute one cut");

        // 2. Ingest at height 5 naming the server: a consumer-signed receipt comes back.
        let server_id = crate::routes::snapshot::server_service_id();
        // Ingest exactly the exported, signed source into a separate fresh consumer.
        let signed_at_5 = served.signed_payload.expect("source signs its snapshot");
        let consumer = test_state(Some(snapshot_sk.clone()));
        let consumer_app = test::init_service(
            App::new()
                .app_data(consumer.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        ).await;
        let entries_json = fetched
            .iter()
            .map(|(k, v)| json!({ "key_hex": hex::encode(k), "value_hex": hex::encode(v) }))
            .collect::<Vec<_>>();
        let resp = test::call_service(
            &consumer_app,
            test::TestRequest::post()
                .uri("/network/snapshot-sync/ingest")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
                .set_json(json!({ "signed_payload": signed_at_5, "entries": entries_json, "served_by": server_id }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["applied"], true);
        let receipt: SignedNodeServiceReceipt = serde_json::from_value(body["receipt"].clone())
            .expect("a node with a snapshot signer issues a consumer-signed receipt");
        assert_eq!(receipt.receipt.server_id, server_id);
        assert_eq!(receipt.receipt.chain_height, 5);
        assert_eq!(receipt.receipt.units, served.payload.entry_count);

        // Self-minted, correctly signed claims for a root/height/units never
        // exported by this node are refused before ledger admission.
        for field in ["root", "height", "units", "service"] {
            let mut wrong = receipt.receipt.clone();
            match field {
                "root" => wrong.subject_hash = [0x42; 32],
                "height" => wrong.chain_height += 1,
                "units" => wrong.units += 1,
                _ => wrong.service = wrfcoin_shared_protocol::consensus::node_service_receipt::NodeService::SyncRange,
            }
            let wrong = SignedNodeServiceReceipt::sign(wrong, &snapshot_sk,
                &wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active(),
                wrfcoin_shared_protocol::crypto::chain_binding::protocol_version_at(receipt.receipt.chain_height));
            let response = test::call_service(&app, test::TestRequest::post()
                .uri("/network/snapshot-sync/receipt")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
                .set_json(json!({ "receipt": wrong })).to_request()).await;
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
            let refusal: serde_json::Value = test::read_body_json(response).await;
            assert!(refusal.to_string().contains("RECEIPT_SUBJECT_NOT_SERVED"));
        }

        // A different caller advances the export cut before the original consumer returns.
        {
            let mut chain = state.blockchain.write().await;
            *chain = crate::routes::snapshot::snapshot_test_support::with_chain_committing_to_its_state(
                std::mem::take(&mut *chain), 6);
        }
        let advanced = test::call_service(&app, test::TestRequest::get()
            .uri("/state/snapshot/export?limit=1")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
            .to_request()).await;
        assert_eq!(advanced.status(), StatusCode::OK);
        let advanced: crate::routes::snapshot::SnapshotExportResponse = test::read_body_json(advanced).await;
        assert_eq!(advanced.height, 6);

        // 3. The server accepts the receipt once; the same receipt again is a replay.
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/network/snapshot-sync/receipt")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
                .set_json(json!({ "receipt": receipt }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "a fresh, correctly addressed receipt is recorded");
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/network/snapshot-sync/receipt")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
                .set_json(json!({ "receipt": receipt }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CONFLICT, "the same consumer/server/snapshot is recorded once");

        // A forged receipt (signature bytes flipped) is refused.
        let mut forged = receipt.clone();
        forged.signature[10] ^= 0x01;
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/network/snapshot-sync/receipt")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
                .set_json(json!({ "receipt": forged }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

        // 4. The operator ledger lists it.
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/network/snapshot-sync/receipts")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let ledger: serde_json::Value = test::read_body_json(resp).await;
        assert!(ledger["count"].as_u64().unwrap() >= 1);
        assert_eq!(ledger["server_id"], server_id);

        // Rotate signer keys through the real mounted route: one Reader still
        // has its own 32-entry window, and cannot evict unexpired evidence.
        for i in 1..=32 {
            let (sk, _pk_hex) = keypair(312_900 + i);
            let claim = SignedNodeServiceReceipt::sign(
                wrfcoin_shared_protocol::consensus::node_service_receipt::NodeServiceReceipt {
                    consumer_pubkey: wrfcoin_shared_protocol::crypto::signature::PublicKey::from_private_key(&sk).as_bytes().try_into().unwrap(),
                    ..receipt.receipt.clone()
                }, &sk,
                &wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active(),
                wrfcoin_shared_protocol::crypto::chain_binding::protocol_version_at(receipt.receipt.chain_height));
            let response = test::call_service(&app, test::TestRequest::post()
                .uri("/network/snapshot-sync/receipt")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
                .set_json(json!({ "receipt": claim })).to_request()).await;
            assert_eq!(response.status(), if i < 32 { StatusCode::OK } else { StatusCode::TOO_MANY_REQUESTS });
        }
        // A separately authenticated caller retains its admission budget after the Reader flood.
        let (other_sk, _) = keypair(400_001);
        let other_claim = SignedNodeServiceReceipt::sign(
            wrfcoin_shared_protocol::consensus::node_service_receipt::NodeServiceReceipt {
                consumer_pubkey: wrfcoin_shared_protocol::crypto::signature::PublicKey::from_private_key(&other_sk).as_bytes().try_into().unwrap(),
                ..receipt.receipt.clone()
            }, &other_sk,
            &wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity::active(),
            wrfcoin_shared_protocol::crypto::chain_binding::protocol_version_at(receipt.receipt.chain_height));
        let response = test::call_service(&app, test::TestRequest::post()
            .uri("/network/snapshot-sync/receipt")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
            .set_json(json!({ "receipt": other_claim })).to_request()).await;
        assert_eq!(response.status(), StatusCode::OK, "other callers are not starved by rotated consumer keys");
        let response = test::call_service(&app, test::TestRequest::post()
            .uri("/network/snapshot-sync/receipt")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
            .set_json(json!({ "receipt": receipt })).to_request()).await;
        assert_eq!(response.status(), StatusCode::CONFLICT, "the original evidence survives key flooding");
        let response = test::call_service(&consumer_app, test::TestRequest::get()
            .uri("/network/snapshot-sync/receipts")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: test-only synthetic bearer fixture, never a deployed credential.
            .to_request()).await;
        let other_ledger: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(other_ledger["count"], 0, "receipt evidence is node-scoped");
    }

    #[actix_web::test]
    async fn snapshot_sync_verify_and_ingest_routes_enforce_replay_protection() {
        // After #1405 ingest applies remote state rather than acting as a
        // no-op validator, the test forges a signed payload at height 5 for
        // this fresh node (#3288 3b: ingest runs only on a fresh node), then
        // reuses the same payload to exercise replay protection.
        let (snapshot_sk, _snapshot_pk) = keypair(202);
        let state = test_state(Some(snapshot_sk.clone()));
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;

        let payload_request = test::TestRequest::get()
            .uri("/state/snapshot/sync-payload")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .to_request();
        let payload_response = test::call_service(&app, payload_request).await;
        assert_eq!(payload_response.status(), StatusCode::OK);
        let payload_body: SnapshotSyncPayloadResponse =
            test::read_body_json(payload_response).await;
        let signed_verify_only = payload_body
            .signed_payload
            .expect("snapshot signing key should produce signed payload");

        let verify_request = test::TestRequest::post()
            .uri("/state/snapshot/sync-payload/verify")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .set_json(json!({ "signed_payload": signed_verify_only }))
            .to_request();
        let verify_response = test::call_service(&app, verify_request).await;
        assert_eq!(verify_response.status(), StatusCode::OK);

        // Build entries from the current (default) state and craft a payload
        // at height=5 so it is strictly ahead of local (height=0).
        // #3288: the signed state_root_hex is this state's recomputed root.
        // #3288 3b: the snapshot also carries genesis plus the block window,
        // the tip committing to that root.
        let (payload_at_5, snapshot_entries, _source_chain) = {
            let blockchain = state.blockchain.read().await;
            crate::routes::snapshot::snapshot_test_support::snapshot_of(&blockchain.state, 5)
        };
        // #3288 3b: ingest requires a pinned genesis; pin the source's.
        let _genesis_pin = crate::routes::snapshot::TestGenesisPin::set(
            _source_chain[0].hash.expect("hashed").to_hex(),
        );
        let signed_at_5 =
            wrfcoin_shared_protocol::blockchain::snapshot_sync::sign_snapshot_sync_payload(
                payload_at_5,
                &snapshot_sk,
                chrono::Utc::now().timestamp(),
                "ingest-apply-202".to_string(),
            );
        let entries_json = snapshot_entries
            .iter()
            .map(|(k, v)| json!({ "key_hex": hex::encode(k), "value_hex": hex::encode(v) }))
            .collect::<Vec<_>>();

        let ingest_request = test::TestRequest::post()
            .uri("/network/snapshot-sync/ingest")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
            .set_json(
                json!({ "signed_payload": signed_at_5, "entries": entries_json }),
            )
            .to_request();
        let ingest_response = test::call_service(&app, ingest_request).await;
        assert_eq!(ingest_response.status(), StatusCode::OK);

        // Local height should now be 5; replay should fail because the
        // nonce is in the seen set (and the node is no longer fresh).
        {
            let blockchain = state.blockchain.read().await;
            assert_eq!(blockchain.height, 5);
        }
        let replay_request = test::TestRequest::post()
            .uri("/network/snapshot-sync/ingest")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
            .set_json(
                json!({ "signed_payload": signed_at_5, "entries": entries_json }),
            )
            .to_request();
        let replay_response = test::call_service(&app, replay_request).await;
        assert_eq!(replay_response.status(), StatusCode::CONFLICT);
    }

    /// #3288 3b (review N1): a signed state root the entries do not produce is
    /// refused and the (snapshot root, signed root) PAIR is blacklisted; the
    /// same entries signed with the root they do produce still install.
    #[actix_web::test]
    async fn a_wrong_signed_state_root_is_blacklisted_as_a_pair_3288() {
        let (snapshot_sk, _snapshot_pk) = keypair(3289);
        let state = test_state(Some(snapshot_sk.clone()));
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;
        // A state unique to this test, so the process-wide blacklist cannot
        // collide with another test's snapshot.
        {
            let mut blockchain = state.blockchain.write().await;
            blockchain
                .apply_state_transition(StateTransition::ApplyReward {
                    address: "0x3289328932893289328932893289328932893289".parse::<Address>().unwrap(),
                    amount: Balance(3289),
                    reason: "n1-pair-blacklist".to_string(),
                })
                .expect("seed");
        }
        let (honest, entries, chain) = {
            let blockchain = state.blockchain.read().await;
            crate::routes::snapshot::snapshot_test_support::snapshot_of(&blockchain.state, 7)
        };
        let _genesis_pin = crate::routes::snapshot::TestGenesisPin::set(chain[0].hash.unwrap().to_hex());
        let entries_json = entries
            .iter()
            .map(|(k, v)| json!({ "key_hex": hex::encode(k), "value_hex": hex::encode(v) }))
            .collect::<Vec<_>>();
        let mut wrong = honest.clone();
        wrong.state_root_hex = "32".repeat(32);
        let ingest = |payload, nonce: &str| {
            let signed = wrfcoin_shared_protocol::blockchain::snapshot_sync::sign_snapshot_sync_payload(
                payload,
                &snapshot_sk,
                chrono::Utc::now().timestamp(),
                nonce.to_string(),
            );
            test::TestRequest::post()
                .uri("/network/snapshot-sync/ingest")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: test-only fixture bearer token, not a deployed credential
                .set_json(json!({ "signed_payload": signed, "entries": entries_json }))
                .to_request()
        };
        let code = |body: serde_json::Value| body.to_string();

        let first = test::call_service(&app, ingest(wrong.clone(), "n1-wrong-1")).await;
        assert_eq!(first.status(), StatusCode::CONFLICT);
        assert!(code(test::read_body_json(first).await).contains("SNAPSHOT_STATE_ROOT_MISMATCH"));

        let again = test::call_service(&app, ingest(wrong, "n1-wrong-2")).await;
        assert_eq!(again.status(), StatusCode::CONFLICT);
        assert!(
            code(test::read_body_json(again).await).contains("SNAPSHOT_ROOT_BLACKLISTED"),
            "the conflicting pair is refused before any work on resubmission"
        );

        let ok = test::call_service(&app, ingest(honest, "n1-honest")).await;
        assert_eq!(ok.status(), StatusCode::OK, "the honest signature over the same entries installs");
        assert_eq!(state.blockchain.read().await.height, 7);
    }

    /// #3288 3b (A): a node without a pinned genesis refuses ingest over
    /// HTTP with the exact message, even for a perfect, trusted snapshot.
    #[actix_web::test]
    async fn snapshot_sync_ingest_refuses_on_an_unpinned_node_3288() {
        let _env = crate::helpers::test_env::lock();
        let _pin = crate::routes::snapshot::TestGenesisPin::unpinned();
        let (snapshot_sk, _snapshot_pk) = keypair(3288);
        let state = test_state(Some(snapshot_sk.clone()));
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;
        let (payload, entries, _chain) = {
            let blockchain = state.blockchain.read().await;
            crate::routes::snapshot::snapshot_test_support::snapshot_of(&blockchain.state, 5)
        };
        let signed = wrfcoin_shared_protocol::blockchain::snapshot_sync::sign_snapshot_sync_payload(
            payload,
            &snapshot_sk,
            chrono::Utc::now().timestamp(),
            "ingest-unpinned-3288".to_string(),
        );
        let entries_json = entries
            .iter()
            .map(|(k, v)| json!({ "key_hex": hex::encode(k), "value_hex": hex::encode(v) }))
            .collect::<Vec<_>>();
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/network/snapshot-sync/ingest")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: test-only fixture bearer token, not a deployed credential
                .set_json(json!({ "signed_payload": signed, "entries": entries_json }))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body: serde_json::Value = test::read_body_json(response).await;
        assert!(
            body.to_string()
                .contains("snapshot ingest requires a pinned genesis (WRFCOIN_GENESIS_HASH)"),
            "{body}"
        );
        assert_eq!(state.blockchain.read().await.height, 0, "nothing installed");
    }

    #[actix_web::test]
    async fn snapshot_sync_ingest_rejects_tampered_entry_before_apply() {
        // #1405: the ingest path recomputes the Merkle root over the
        // submitted entries BEFORE mutating any state. Flipping a byte in
        // one of the submitted entries must cause a CONFLICT response and
        // must leave the local chain height and state untouched.
        let (snapshot_sk, _snapshot_pk) = keypair(207);
        let state = test_state(Some(snapshot_sk.clone()));
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;

        // Seed the local default state with an account so the snapshot
        // actually carries an `account/...` entry we can tamper with.
        {
            let mut blockchain = state.blockchain.write().await;
            let address = "0x1111111111111111111111111111111111111111"
                .parse::<Address>()
                .expect("valid address");
            blockchain
                .apply_state_transition(StateTransition::ApplyReward {
                    address,
                    amount: Balance(1_000_000),
                    reason: "tampered-entry-test".to_string(),
                })
                .expect("state transition should succeed");
        }

        // Build a valid signed payload at height=9 (local is 0) and entries
        // that match the local state.
        let (payload_at_9, snapshot_entries, _source_chain) = {
            let blockchain = state.blockchain.read().await;
            crate::routes::snapshot::snapshot_test_support::snapshot_of(&blockchain.state, 9)
        };
        // #3288 3b: ingest requires a pinned genesis; pin the source's.
        let _genesis_pin = crate::routes::snapshot::TestGenesisPin::set(
            _source_chain[0].hash.expect("hashed").to_hex(),
        );
        let signed_at_9 =
            wrfcoin_shared_protocol::blockchain::snapshot_sync::sign_snapshot_sync_payload(
                payload_at_9,
                &snapshot_sk,
                chrono::Utc::now().timestamp(),
                "ingest-tamper-207".to_string(),
            );

        // Snapshot local height/state before the tampered call so we can
        // assert they are untouched afterwards.
        let pre_height = {
            let blockchain = state.blockchain.read().await;
            blockchain.height
        };
        let pre_state_root = {
            let blockchain = state.blockchain.read().await;
            blockchain.state.state_root.0
        };

        // Tamper with one account entry's value bytes.
        let mut entries_json = snapshot_entries
            .iter()
            .map(|(k, v)| json!({ "key_hex": hex::encode(k), "value_hex": hex::encode(v) }))
            .collect::<Vec<_>>();
        let target = entries_json
            .iter_mut()
            .find(|e| {
                let key_hex = e["key_hex"].as_str().unwrap_or("");
                let key_bytes = hex::decode(key_hex).unwrap_or_default();
                std::str::from_utf8(&key_bytes)
                    // #3288 v2: one entry per account under state/accounts/.
                    .map(|s| s.starts_with("state/accounts/"))
                    .unwrap_or(false)
            })
            .expect("at least one account entry in snapshot");
        let mut bytes = hex::decode(target["value_hex"].as_str().unwrap()).unwrap();
        bytes[0] ^= 0xff;
        target["value_hex"] = json!(hex::encode(bytes));

        let ingest_request = test::TestRequest::post()
            .uri("/network/snapshot-sync/ingest")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
            .set_json(json!({
                "signed_payload": signed_at_9,
                "entries": entries_json,
            }))
            .to_request();
        let ingest_response = test::call_service(&app, ingest_request).await;
        assert_eq!(ingest_response.status(), StatusCode::CONFLICT);

        // Local chain must be untouched: height and state root unchanged.
        let blockchain = state.blockchain.read().await;
        assert_eq!(blockchain.height, pre_height);
        assert_eq!(blockchain.state.state_root.0, pre_state_root);
    }

    #[actix_web::test]
    async fn snapshot_sync_routes_reject_tampered_entry_count_and_stale_payloads() {
        let (snapshot_sk, _snapshot_pk) = keypair(203);
        let state = test_state(Some(snapshot_sk.clone()));
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;

        let payload_request = test::TestRequest::get()
            .uri("/state/snapshot/sync-payload")
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
            .to_request();
        let payload_response = test::call_service(&app, payload_request).await;
        assert_eq!(payload_response.status(), StatusCode::OK);
        let payload_body: SnapshotSyncPayloadResponse =
            test::read_body_json(payload_response).await;
        let signed = payload_body
            .signed_payload
            .expect("snapshot signing key should produce signed payload");

        let mut tampered_payload = signed.payload.clone();
        tampered_payload.entry_count = tampered_payload.entry_count.saturating_add(1);
        let tampered_signed =
            wrfcoin_shared_protocol::blockchain::snapshot_sync::sign_snapshot_sync_payload(
                tampered_payload,
                &snapshot_sk,
                chrono::Utc::now().timestamp(),
                "tampered-entry-count".to_string(),
            );
        let tampered_verify = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/state/snapshot/sync-payload/verify")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .set_json(json!({ "signed_payload": tampered_signed }))
                .to_request(),
        )
        .await;
        assert_eq!(tampered_verify.status(), StatusCode::CONFLICT);

        {
            let mut blockchain = state.blockchain.write().await;
            let address = "0x7777777777777777777777777777777777777777"
                .parse::<Address>()
                .expect("valid address");
            blockchain
                .apply_state_transition(StateTransition::ApplyReward {
                    address,
                    amount: Balance(1),
                    reason: "snapshot-stale-payload-test".to_string(),
                })
                .expect("state transition should succeed");
        }

        let stale_verify = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/state/snapshot/sync-payload/verify")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .set_json(json!({ "signed_payload": signed }))
                .to_request(),
        )
        .await;
        assert_eq!(stale_verify.status(), StatusCode::CONFLICT);

        let stale_ingest = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/network/snapshot-sync/ingest")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "signed_payload": signed }))
                .to_request(),
        )
        .await;
        assert_eq!(stale_ingest.status(), StatusCode::CONFLICT);
    }

    #[actix_web::test]
    async fn snapshot_sync_verify_route_requires_trusted_signers_configuration() {
        let (snapshot_sk, _snapshot_pk) = keypair(204);
        let mut state = (*test_state(Some(snapshot_sk)).get_ref()).clone();
        state.snapshot_trusted_signers = Arc::new(FxHashSet::default());
        let state = web::Data::new(state);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;

        let payload_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/state/snapshot/sync-payload")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(payload_response.status(), StatusCode::OK);
        let payload_body: SnapshotSyncPayloadResponse =
            test::read_body_json(payload_response).await;
        let signed = payload_body
            .signed_payload
            .expect("snapshot signing key should produce signed payload");

        let verify_response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/state/snapshot/sync-payload/verify")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .set_json(json!({ "signed_payload": signed }))
                .to_request(),
        )
        .await;
        assert_eq!(verify_response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[actix_web::test]
    async fn snapshot_sync_ingest_rejects_untrusted_signer() {
        let (snapshot_sk, _snapshot_pk) = keypair(205);
        let (_other_sk, other_pk) = keypair(206);
        let mut state = (*test_state(Some(snapshot_sk)).get_ref()).clone();
        state.snapshot_trusted_signers = Arc::new(FxHashSet::from_iter([other_pk]));
        let state = web::Data::new(state);
        let app = test::init_service(
            App::new()
                .app_data(state.clone())
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;

        let payload_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/state/snapshot/sync-payload")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(payload_response.status(), StatusCode::OK);
        let payload_body: SnapshotSyncPayloadResponse =
            test::read_body_json(payload_response).await;
        let signed = payload_body
            .signed_payload
            .expect("snapshot signing key should produce signed payload");

        let ingest_response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/network/snapshot-sync/ingest")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "signed_payload": signed }))
                .to_request(),
        )
        .await;
        assert_eq!(ingest_response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[actix_web::test]
    async fn quality_event_routes_support_filtering_history_and_summary() {
        let state = test_state(None);
        let participant_a = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .parse::<Address>()
            .expect("valid participant a");
        let participant_b = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            .parse::<Address>()
            .expect("valid participant b");

        {
            let mut blockchain = state.blockchain.write().await;
            blockchain
                .state
                .governance
                .params
                .quality_event_retention_policy
                .hot_event_limit = 2;
            let policy = blockchain
                .state
                .governance
                .params
                .data_quality_policy
                .clone();
            blockchain
                .state
                .append_quality_policy_event(QualityPolicyEventRecord {
                    event_id: 0,
                    event_type: QualityPolicyEventType::ChallengeTriggered,
                    block_height: 10,
                    commit_id: "commit-warning-a".to_string(),
                    participant: participant_a,
                    quality_band: DataQualityBand::WarningOnly,
                    quality_score: Some(
                        wrfcoin_shared_protocol::weather::validation::DataQualityScore {
                            overall: 0.55,
                            range_score: 0.55,
                            consistency_score: 0.55,
                        },
                    ),
                    scoring_multiplier: policy.warning_only_scoring_multiplier,
                    reward_multiplier: policy.warning_only_reward_multiplier,
                    warning_only: true,
                    trigger_challenge: true,
                    counts_toward_low_quality_streak: false,
                    low_quality_streak: None,
                    low_quality_slash_threshold: None,
                    slashed_amount: None,
                    policy_snapshot: policy.clone(),
                });
            blockchain
                .state
                .append_quality_policy_event(QualityPolicyEventRecord {
                    event_id: 0,
                    event_type: QualityPolicyEventType::ChallengeTriggered,
                    block_height: 11,
                    commit_id: "commit-low-b".to_string(),
                    participant: participant_b,
                    quality_band: DataQualityBand::Low,
                    quality_score: Some(
                        wrfcoin_shared_protocol::weather::validation::DataQualityScore {
                            overall: 0.20,
                            range_score: 0.20,
                            consistency_score: 0.20,
                        },
                    ),
                    scoring_multiplier: policy.low_quality_scoring_multiplier,
                    reward_multiplier: policy.low_quality_reward_multiplier,
                    warning_only: false,
                    trigger_challenge: true,
                    counts_toward_low_quality_streak: true,
                    low_quality_streak: Some(2),
                    low_quality_slash_threshold: Some(policy.low_quality_slash_streak),
                    slashed_amount: None,
                    policy_snapshot: policy.clone(),
                });
            blockchain
                .state
                .append_quality_policy_event(QualityPolicyEventRecord {
                    event_id: 0,
                    event_type: QualityPolicyEventType::SlashingApplied,
                    block_height: 11,
                    commit_id: "commit-low-b".to_string(),
                    participant: participant_b,
                    quality_band: DataQualityBand::Low,
                    quality_score: Some(
                        wrfcoin_shared_protocol::weather::validation::DataQualityScore {
                            overall: 0.20,
                            range_score: 0.20,
                            consistency_score: 0.20,
                        },
                    ),
                    scoring_multiplier: policy.low_quality_scoring_multiplier,
                    reward_multiplier: policy.low_quality_reward_multiplier,
                    warning_only: false,
                    trigger_challenge: true,
                    counts_toward_low_quality_streak: true,
                    low_quality_streak: Some(policy.low_quality_slash_streak),
                    low_quality_slash_threshold: Some(policy.low_quality_slash_streak),
                    slashed_amount: Some(Balance(policy.low_quality_slash_amount)),
                    policy_snapshot: policy,
                });
        }

        let app = test::init_service(test_app(state.clone())).await;

        let filtered_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/quality/events?participant=0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb&event_type=challenge&band=low")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(filtered_response.status(), StatusCode::OK);
        let filtered: QualityEventsResponse = test::read_body_json(filtered_response).await;
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.items[0].event_type, "challenge");
        assert_eq!(filtered.items[0].quality_band, "low");
        assert!(filtered.archived_records_omitted);
        assert_eq!(filtered.archive.hot_event_count, 2);
        assert_eq!(filtered.archive.archived_event_count, 1);
        assert_eq!(filtered.archive.archived_through_event_id, Some(1));

        let history_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/quality/participants/0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/history?event_type=slashing")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(history_response.status(), StatusCode::OK);
        let history: QualityEventsResponse = test::read_body_json(history_response).await;
        assert_eq!(history.total, 1);
        assert_eq!(history.items[0].event_type, "slashing");
        assert_eq!(history.items[0].commit_id, "commit-low-b");
        assert!(history.archived_records_omitted);

        let summary_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/quality/summary")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(summary_response.status(), StatusCode::OK);
        let summary: QualitySummaryResponse = test::read_body_json(summary_response).await;
        assert_eq!(summary.total_events, 3);
        assert_eq!(summary.challenge_events, 2);
        assert_eq!(summary.slashing_events, 1);
        assert_eq!(summary.band_counters.warning_only, 1);
        assert_eq!(summary.band_counters.low, 2);
        assert!(summary.archived_aggregates_included);
        assert!(!summary.archived_aggregates_omitted);
        assert_eq!(summary.archive.archived_event_count, 1);

        let participant_summary_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(
                    "/transactions/quality/summary?participant=0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                )
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(participant_summary_response.status(), StatusCode::OK);
        let participant_summary: QualitySummaryResponse =
            test::read_body_json(participant_summary_response).await;
        assert_eq!(participant_summary.total_events, 2);
        assert_eq!(participant_summary.challenge_events, 1);
        assert_eq!(participant_summary.slashing_events, 1);
        assert!(!participant_summary.archived_aggregates_included);
        assert!(participant_summary.archived_aggregates_omitted);
    }

    #[actix_web::test]
    async fn quality_governance_build_route_validates_and_generates_payload() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let valid_response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/governance/quality/proposals/build")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({
                    "target": "warning_threshold",
                    "value": 0.55
                }))
                .to_request(),
        )
        .await;
        assert_eq!(valid_response.status(), StatusCode::OK);
        let valid_payload: QualityGovernanceProposalBuildResponse =
            test::read_body_json(valid_response).await;
        assert_eq!(
            valid_payload.proposal_payload.parameter,
            "quality.warning_threshold"
        );
        assert_eq!(valid_payload.proposal_payload.value, "0.55");
        assert_eq!(valid_payload.preview.band_outcomes.len(), 4);

        let invalid_response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/governance/quality/proposals/build")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({
                    "target": "warning_threshold",
                    "value": 0.95
                }))
                .to_request(),
        )
        .await;
        assert_eq!(invalid_response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[actix_web::test]
    async fn trust_governance_build_route_validates_and_generates_payload() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let valid_response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/governance/trust/proposals/build")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({
                    "target": "unverified_weight",
                    "value": 0.90
                }))
                .to_request(),
        )
        .await;
        assert_eq!(valid_response.status(), StatusCode::OK);
        let valid_payload: TrustGovernanceProposalBuildResponse =
            test::read_body_json(valid_response).await;
        assert_eq!(
            valid_payload.proposal_payload.parameter,
            "trust.unverified_weight"
        );
        assert_eq!(valid_payload.proposal_payload.value, "0.9");
        assert_eq!(valid_payload.preview.class_multipliers.len(), 3);
        assert!(
            valid_payload
                .preview
                .ordering_hints
                .iter()
                .all(|hint| hint.satisfied)
        );

        let invalid_ordering_response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/governance/trust/proposals/build")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({
                    "target": "trusted_provider_weight",
                    "value": 0.25
                }))
                .to_request(),
        )
        .await;
        assert_eq!(
            invalid_ordering_response.status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[actix_web::test]
    #[serial_test::serial(core_metrics)]
    async fn trust_reward_impact_routes_and_metrics_expose_expected_shapes() {
        let state = test_state(None);
        {
            let mut blockchain = state.blockchain.write().await;

            let mut block_10 = Block::new(1, [0u8; 32].into(), vec![], 1, 10);
            block_10
                .metadata
                .insert("rewards_minted_count".to_string(), "2".to_string());
            block_10
                .metadata
                .insert("rewards_minted_total".to_string(), "140".to_string());
            block_10.metadata.insert(
                "rewards_trust_unverified_count".to_string(),
                "1".to_string(),
            );
            block_10.metadata.insert(
                "rewards_trust_verified_station_count".to_string(),
                "1".to_string(),
            );
            block_10.metadata.insert(
                "rewards_trust_trusted_provider_count".to_string(),
                "0".to_string(),
            );
            block_10
                .metadata
                .insert("rewards_quality_high_count".to_string(), "1".to_string());
            block_10.metadata.insert(
                "rewards_quality_borderline_count".to_string(),
                "1".to_string(),
            );
            block_10
                .metadata
                .insert("rewards_quality_warning_count".to_string(), "0".to_string());
            block_10.metadata.insert(
                "rewards_skipped_low_quality_count".to_string(),
                "0".to_string(),
            );
            block_10.metadata.insert(
                "rewards_trust_weights".to_string(),
                "unverified=0.60,verified_station=0.80,trusted_provider=1.00".to_string(),
            );
            block_10.metadata.insert(
                "rewards_quality_multipliers".to_string(),
                "borderline(score=0.90,reward=0.80),warning(score=0.60,reward=0.40),low(score=0.30,reward=0.20)".to_string(),
            );
            let hash_10 = block_10.calculate_hash();
            block_10.set_hash(hash_10);
            blockchain
                .add_block(block_10)
                .expect("block 10 should be added");

            let mut block_11 = Block::new(1, hash_10, vec![], 1, 11);
            block_11
                .metadata
                .insert("rewards_minted_count".to_string(), "4".to_string());
            block_11
                .metadata
                .insert("rewards_minted_total".to_string(), "310".to_string());
            block_11.metadata.insert(
                "rewards_trust_unverified_count".to_string(),
                "1".to_string(),
            );
            block_11.metadata.insert(
                "rewards_trust_verified_station_count".to_string(),
                "2".to_string(),
            );
            block_11.metadata.insert(
                "rewards_trust_trusted_provider_count".to_string(),
                "1".to_string(),
            );
            block_11
                .metadata
                .insert("rewards_quality_high_count".to_string(), "2".to_string());
            block_11.metadata.insert(
                "rewards_quality_borderline_count".to_string(),
                "1".to_string(),
            );
            block_11
                .metadata
                .insert("rewards_quality_warning_count".to_string(), "1".to_string());
            block_11.metadata.insert(
                "rewards_skipped_low_quality_count".to_string(),
                "0".to_string(),
            );
            block_11.metadata.insert(
                "rewards_trust_weights".to_string(),
                "unverified=0.60,verified_station=0.80,trusted_provider=1.00".to_string(),
            );
            block_11.metadata.insert(
                "rewards_quality_multipliers".to_string(),
                "borderline(score=0.95,reward=0.90),warning(score=0.70,reward=0.50),low(score=0.35,reward=0.25)".to_string(),
            );
            let hash_11 = block_11.calculate_hash();
            block_11.set_hash(hash_11);
            blockchain
                .add_block(block_11)
                .expect("block 11 should be added");
        }

        let app = test::init_service(test_app(state.clone())).await;

        let trust_weights_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/governance/trust/weights")
                .to_request(),
        )
        .await;
        assert_eq!(trust_weights_response.status(), StatusCode::OK);
        let trust_weights: TrustWeightsResponse =
            test::read_body_json(trust_weights_response).await;
        assert_eq!(trust_weights.bounds.min, 0.0);
        assert_eq!(trust_weights.bounds.max, 1.0);
        assert!(
            trust_weights
                .ordering_hints
                .iter()
                .all(|hint| hint.satisfied)
        );

        let timeline_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/rewards/trust/timeline?from_block=10&to_block=11")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(timeline_response.status(), StatusCode::OK);
        let timeline: TrustRewardImpactTimelineResponse =
            test::read_body_json(timeline_response).await;
        assert_eq!(timeline.total, 2);
        assert_eq!(timeline.items[0].block_height, 11);
        assert_eq!(timeline.items[0].trust_class_counts.verified_station, 2);
        assert_eq!(timeline.items[0].joint_multiplier_matrix.len(), 4);

        let summary_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/rewards/trust/summary?from_block=10&to_block=11")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(summary_response.status(), StatusCode::OK);
        let summary: TrustRewardImpactSummaryResponse =
            test::read_body_json(summary_response).await;
        assert_eq!(summary.block_count, 2);
        assert_eq!(summary.minted_count, 6);
        assert_eq!(summary.minted_total, 450);
        assert_eq!(summary.trust_class_counts.unverified, 2);
        assert_eq!(summary.trust_class_counts.verified_station, 3);
        assert_eq!(summary.trust_class_counts.trusted_provider, 1);
        assert_eq!(summary.quality_band_counts.high, 3);
        assert_eq!(summary.quality_band_counts.borderline, 2);
        assert_eq!(summary.quality_band_counts.warning_only, 1);

        let metrics_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/metrics")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(metrics_response.status(), StatusCode::OK);
        let metrics_text = String::from_utf8(test::read_body(metrics_response).await.to_vec())
            .expect("metrics endpoint should return utf-8 prometheus text");
        assert!(
            metrics_text.contains("wrfcoin_blockchain_block_height 11"),
            "unexpected metrics output: {metrics_text}"
        );
    }

    #[actix_web::test]
    async fn governance_law_book_routes_support_filters_pagination_and_history() {
        let state = test_state(None);
        {
            let mut blockchain = state.blockchain.write().await;
            let laws = &mut blockchain.state.governance.law_book.laws;
            laws.insert(
                "LAW-001-CID-MANDATORY".to_string(),
                LawRecord {
                    law_id: "LAW-001-CID-MANDATORY".to_string(),
                    title: "CID mandatory for untrusted contributors".to_string(),
                    category: "data_submission".to_string(),
                    plain_language_summary:
                        "Uncalibrated stations must provide CID-backed reveals.".to_string(),
                    enforcement_level: LawEnforcementLevel::Consensus,
                    status: LawStatus::Active,
                    version: 1,
                    amendments: vec![
                        wrfcoin_native_chain::blockchain::state::LawAmendmentRecord {
                            proposal_id: "law-001-create".to_string(),
                            updated_at_block: 5,
                            summary: "law created".to_string(),
                            version: 1,
                        },
                    ],
                },
            );
            laws.insert(
                "LAW-002-METAR-INLINE-PRIVILEGE".to_string(),
                LawRecord {
                    law_id: "LAW-002-METAR-INLINE-PRIVILEGE".to_string(),
                    title: "METAR inline privilege for calibrated stations".to_string(),
                    category: "data_submission".to_string(),
                    plain_language_summary:
                        "Calibrated inline reveals must include plausible METAR; CID reveals bypass this check."
                            .to_string(),
                    enforcement_level: LawEnforcementLevel::Consensus,
                    status: LawStatus::Active,
                    version: 2,
                    amendments: vec![
                        wrfcoin_native_chain::blockchain::state::LawAmendmentRecord {
                            proposal_id: "law-002-create".to_string(),
                            updated_at_block: 6,
                            summary: "law created".to_string(),
                            version: 1,
                        },
                        wrfcoin_native_chain::blockchain::state::LawAmendmentRecord {
                            proposal_id: "law-002-amend".to_string(),
                            updated_at_block: 9,
                            summary: "summary amended".to_string(),
                            version: 2,
                        },
                    ],
                },
            );
            laws.insert(
                "LAW-010-API-RATE-LIMIT".to_string(),
                LawRecord {
                    law_id: "LAW-010-API-RATE-LIMIT".to_string(),
                    title: "API burst limits for untrusted clients".to_string(),
                    category: "resource_limits".to_string(),
                    plain_language_summary: "Apply per-tier API rate caps.".to_string(),
                    enforcement_level: LawEnforcementLevel::Api,
                    status: LawStatus::Draft,
                    version: 1,
                    amendments: vec![
                        wrfcoin_native_chain::blockchain::state::LawAmendmentRecord {
                            proposal_id: "law-010-create".to_string(),
                            updated_at_block: 7,
                            summary: "law created".to_string(),
                            version: 1,
                        },
                    ],
                },
            );
        }

        let app = test::init_service(test_app(state.clone())).await;

        let first_page_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/governance/laws?status=active&enforcement=consensus&limit=1")
                .to_request(),
        )
        .await;
        assert_eq!(first_page_response.status(), StatusCode::OK);
        let first_page_json: serde_json::Value = test::read_body_json(first_page_response).await;
        assert_eq!(
            first_page_json.get("ok").and_then(|value| value.as_bool()),
            Some(true)
        );
        let first_items = first_page_json["data"]["items"]
            .as_array()
            .expect("items should be an array");
        assert_eq!(first_items.len(), 1);
        assert_eq!(first_items[0]["law_id"], "LAW-001-CID-MANDATORY");
        assert!(
            first_items[0].get("amendments").is_none(),
            "list response should omit amendments by default"
        );
        let next_cursor = first_page_json["data"]["next_cursor"]
            .as_str()
            .expect("next cursor should be present")
            .to_string();
        assert_eq!(next_cursor, "LAW-001-CID-MANDATORY");

        let second_page_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!(
                    "/governance/laws?status=active&enforcement=consensus&limit=1&cursor={next_cursor}"
                ))
                .to_request(),
        )
        .await;
        assert_eq!(second_page_response.status(), StatusCode::OK);
        let second_page_json: serde_json::Value = test::read_body_json(second_page_response).await;
        let second_items = second_page_json["data"]["items"]
            .as_array()
            .expect("items should be an array");
        assert_eq!(second_items.len(), 1);
        assert_eq!(second_items[0]["law_id"], "LAW-002-METAR-INLINE-PRIVILEGE");
        assert!(
            second_page_json["data"]["next_cursor"].is_null(),
            "final page should not expose a cursor"
        );

        let draft_filter_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/governance/laws?status=draft")
                .to_request(),
        )
        .await;
        assert_eq!(draft_filter_response.status(), StatusCode::OK);
        let draft_filter_json: serde_json::Value =
            test::read_body_json(draft_filter_response).await;
        let draft_items = draft_filter_json["data"]["items"]
            .as_array()
            .expect("items should be an array");
        assert_eq!(draft_items.len(), 1);
        assert_eq!(draft_items[0]["law_id"], "LAW-010-API-RATE-LIMIT");

        let law_detail_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/governance/laws/LAW-002-METAR-INLINE-PRIVILEGE")
                .to_request(),
        )
        .await;
        assert_eq!(law_detail_response.status(), StatusCode::OK);
        let law_detail_json: serde_json::Value = test::read_body_json(law_detail_response).await;
        assert_eq!(law_detail_json["data"]["version"], 2);
        assert_eq!(
            law_detail_json["data"]["plain_language_rendering"]["headline"],
            "LAW-002-METAR-INLINE-PRIVILEGE: METAR inline privilege for calibrated stations"
        );
        let amendments = law_detail_json["data"]["amendments"]
            .as_array()
            .expect("amendments should be included by default");
        assert_eq!(amendments.len(), 2);
        assert_eq!(amendments[0]["proposal_id"], "law-002-create");
        assert_eq!(amendments[1]["proposal_id"], "law-002-amend");
        let historical_versions = law_detail_json["data"]["historical_versions"]
            .as_array()
            .expect("historical_versions should be included by default");
        assert_eq!(historical_versions.len(), 2);
        assert_eq!(historical_versions[1]["version"], 2);

        let law_detail_without_history = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/governance/laws/LAW-002-METAR-INLINE-PRIVILEGE?include_amendments=false&include_history=false")
                .to_request(),
        )
        .await;
        assert_eq!(law_detail_without_history.status(), StatusCode::OK);
        let law_detail_without_history_json: serde_json::Value =
            test::read_body_json(law_detail_without_history).await;
        assert!(
            law_detail_without_history_json["data"]
                .get("amendments")
                .is_none(),
            "include_amendments=false should omit amendment history"
        );
        assert!(
            law_detail_without_history_json["data"]
                .get("historical_versions")
                .is_none(),
            "include_history=false should omit historical versions"
        );

        let search_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/governance/laws?status=active&q=metar%20inline")
                .to_request(),
        )
        .await;
        assert_eq!(search_response.status(), StatusCode::OK);
        let search_json: serde_json::Value = test::read_body_json(search_response).await;
        assert_eq!(
            search_json["data"]["items"][0]["law_id"],
            "LAW-002-METAR-INLINE-PRIVILEGE"
        );
        assert_eq!(search_json["data"]["search"]["query"], "metar inline");
        assert_eq!(search_json["data"]["search"]["matched_law_count"], 1);
    }

    #[actix_web::test]
    async fn governance_law_book_routes_validate_inputs_and_not_found() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let invalid_status_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/governance/laws?status=enabled")
                .to_request(),
        )
        .await;
        assert_eq!(invalid_status_response.status(), StatusCode::BAD_REQUEST);
        let invalid_status_json: serde_json::Value =
            test::read_body_json(invalid_status_response).await;
        assert_eq!(invalid_status_json["error"]["code"], "VALIDATION_ERROR");

        let invalid_law_id_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/governance/laws/law-001-cid-mandatory")
                .to_request(),
        )
        .await;
        assert_eq!(invalid_law_id_response.status(), StatusCode::BAD_REQUEST);
        let invalid_law_id_json: serde_json::Value =
            test::read_body_json(invalid_law_id_response).await;
        assert_eq!(invalid_law_id_json["error"]["code"], "VALIDATION_ERROR");

        let not_found_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/governance/laws/LAW-999-NOT-FOUND")
                .to_request(),
        )
        .await;
        assert_eq!(not_found_response.status(), StatusCode::NOT_FOUND);
        let not_found_json: serde_json::Value = test::read_body_json(not_found_response).await;
        assert_eq!(not_found_json["error"]["code"], "NOT_FOUND");

        let invalid_history_combo_response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/governance/laws/LAW-001-CID-MANDATORY?include_amendments=false&include_history=true")
                .to_request(),
        )
        .await;
        assert_eq!(
            invalid_history_combo_response.status(),
            StatusCode::BAD_REQUEST
        );
        let invalid_history_combo_json: serde_json::Value =
            test::read_body_json(invalid_history_combo_response).await;
        assert_eq!(
            invalid_history_combo_json["error"]["code"],
            "VALIDATION_ERROR"
        );
    }

    // -----------------------------------------------------------------------
    // Auth enforcement tests
    // -----------------------------------------------------------------------

    /// The role boundary is enforced below the HTTP surface. Model the exact
    /// joiner shape: a restored/seeded producer-auth chain, no producer signer,
    /// and a consensus adapter configured as sync-only. `/blocks/mine` must
    /// refuse before touching any live or durable state (#2860).
    #[actix_web::test]
    async fn sync_only_seeded_chain_refuses_block_production_atomically_2860() {
        use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;

        let base = test_state(None);
        let mut inner = base.get_ref().clone();
        let config = ConsensusFactory::default_config(ConsensusType::ProofOfPublishing);
        let adapter = ConsensusFactory::create_pop_adapter(&config);
        adapter.declare_no_genesis_oracles_blocking();
        adapter.set_block_production_enabled(false);
        inner.consensus = Arc::new(TypedConsensusManager::new(adapter, config));
        let state = web::Data::new(inner);

        {
            let mut blockchain = state.blockchain.write().await;
            blockchain
                .state
                .seed_genesis_producer_set(vec![ProducerMember {
                    producer_id: "ceremony-producer".to_string(),
                    payout_address: format!("0x{}", "ab".repeat(20)),
                    public_key: [0x11u8; 32],
                }])
                .expect("joiner fixture must carry the genesis producer set");
            blockchain.state.recalculate_state_root();
        }

        let (height_before, tip_before, root_before, blocks_before) = {
            let blockchain = state.blockchain.read().await;
            (
                blockchain.height,
                blockchain.tip,
                blockchain.state.state_root.0,
                blockchain.blocks().len(),
            )
        };
        let stored_before = state
            .storage
            .read()
            .await
            .load_blocks_range(0, 64)
            .map(|blocks| blocks.len())
            .unwrap_or(0);

        let app = test::init_service(test_app(state.clone())).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(&json!({
                    "miner_address": "0x2222222222222222222222222222222222222222"
                }))
                .to_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(body["error"]["code"], "SYNC_ONLY", "{body}");

        let blockchain = state.blockchain.read().await;
        assert_eq!(blockchain.height, height_before);
        assert_eq!(blockchain.tip, tip_before);
        assert_eq!(blockchain.state.state_root.0, root_before);
        assert_eq!(blockchain.blocks().len(), blocks_before);
        drop(blockchain);
        let stored_after = state
            .storage
            .read()
            .await
            .load_blocks_range(0, 64)
            .map(|blocks| blocks.len())
            .unwrap_or(0);
        assert_eq!(stored_after, stored_before);
    }

    /// #2290: `/blocks/mine` is the testnet's REAL production path (the N5
    /// collector drives it), and before this gate it created and PERSISTED a
    /// block without ever consulting the producer-authority check that gossip
    /// and IBD both run. A node whose key is not in the authorized set would
    /// mine, persist, and after restart trust blocks the whole network rejects
    /// — a silent self-fork on the one node that matters.
    ///
    /// Seeds a producer set whose member is NOT the miner, then asserts the
    /// refusal is ATOMIC across every observable surface.
    ///
    /// An earlier revision of this test asserted only the durable storage block
    /// count, and that is precisely how the real defect shipped: the route
    /// built the block against the LIVE chain and gated afterwards, so an
    /// unauthorized block was refused with 409 and never persisted — while the
    /// running node had already accepted it (height 0 -> 1, tip and state_root
    /// advanced). Storage was clean and the node was forked from itself.
    ///
    /// So this snapshots SIX things before the request and requires every one
    /// unchanged after: chain height, tip, `state.state_root`, in-memory block
    /// count, mempool transaction count, and durable storage block count.
    #[actix_web::test]
    async fn mine_block_is_refused_and_persists_nothing_when_producer_not_authorized() {
        use wrfcoin_shared_protocol::consensus::producer_set::ProducerMember;

        let state = test_state(None);

        // An authorized set that does NOT contain the miner below.
        let authorized_payout = format!("0x{}", "ab".repeat(20));
        {
            let mut blockchain = state.blockchain.write().await;
            blockchain
                .state
                .seed_genesis_producer_set(vec![ProducerMember {
                    producer_id: "authorized-node".to_string(),
                    payout_address: authorized_payout,
                    public_key: [0x11u8; 32],
                }])
                .expect("seeding a valid producer set must succeed");
            blockchain.state.recalculate_state_root();
        }

        // Seed the mempool so a swallowed drain is observable. On the broken
        // path the engine removes these while assembling the block, and the
        // refusal leaves them gone.
        {
            let mut mempool = state.mempool.write().await;
            mempool
                .add_transaction(test_mempool_tx(1))
                .expect("first mempool tx should be accepted");
            mempool
                .add_transaction(test_mempool_tx(2))
                .expect("second mempool tx should be accepted");
        }

        // ---- snapshot BEFORE -------------------------------------------
        let (height_before, tip_before, state_root_before, in_memory_blocks_before) = {
            let blockchain = state.blockchain.read().await;
            (
                blockchain.height,
                blockchain.tip,
                blockchain.state.state_root.0,
                blockchain.blocks().len(),
            )
        };
        let mempool_txs_before = state.mempool.read().await.size();
        let stored_blocks_before = {
            let storage = state.storage.read().await;
            storage.load_blocks_range(0, 64).map(|b| b.len()).unwrap_or(0)
        };
        assert_eq!(
            mempool_txs_before, 2,
            "test setup should have seeded two mempool entries"
        );

        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(&json!({
                    "miner_address": "0x2222222222222222222222222222222222222222"
                }))
                .to_request(),
        )
        .await;

        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "an unauthorized producer must be refused, not allowed to mine"
        );
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(
            body["error"]["code"], "PRODUCER_NOT_AUTHORIZED",
            "the refusal must name the producer-auth cause: {body}"
        );

        // ---- snapshot AFTER: every surface must be byte-identical -------
        let (height_after, tip_after, state_root_after, in_memory_blocks_after) = {
            let blockchain = state.blockchain.read().await;
            (
                blockchain.height,
                blockchain.tip,
                blockchain.state.state_root.0,
                blockchain.blocks().len(),
            )
        };
        let mempool_txs_after = state.mempool.read().await.size();
        let stored_blocks_after = {
            let storage = state.storage.read().await;
            storage.load_blocks_range(0, 64).map(|b| b.len()).unwrap_or(0)
        };

        assert_eq!(
            height_after, height_before,
            "a refused block must NOT advance the live chain height — \
             the running node would be forked from every peer"
        );
        assert_eq!(
            tip_after, tip_before,
            "a refused block must NOT become the live chain tip"
        );
        assert_eq!(
            state_root_after, state_root_before,
            "a refused block must NOT mutate the consensus state_root"
        );
        assert_eq!(
            in_memory_blocks_after, in_memory_blocks_before,
            "a refused block must NOT be added to the in-memory block map"
        );
        assert_eq!(
            mempool_txs_after, mempool_txs_before,
            "a refused block must NOT consume mempool transactions — \
             they would be silently lost, never mined and never persisted"
        );
        assert_eq!(
            stored_blocks_after, stored_blocks_before,
            "an unauthorized block must NEVER be persisted — a restart would trust it"
        );
    }

    #[actix_web::test]
    async fn mine_block_requires_operator_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let address = "0x1111111111111111111111111111111111111111";
        let body = json!({ "miner_address": address });

        // No auth -> 401
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Reader -> 403
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Contributor -> 403
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Operator -> 201
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[actix_web::test]
    async fn layer2_submit_batch_requires_operator_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let body = json!({
            "batch_id": 1,
            "prev_state_root": "0xprev",
            "state_root": "0xnext",
            "proof_hex": "deadbeef",
            "commitment_hex": "cafebabe",
            "operator": "operator-1"
        });

        // No auth -> 401
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/layer2/submit-batch")
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Contributor -> 403
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/layer2/submit-batch")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Operator -> 201
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/layer2/submit-batch")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[cfg(feature = "execution")]
    #[actix_web::test]
    async fn execution_run_requires_contributor_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let body = json!({
            "model_id": "model-does-not-exist",
            "region": "us-west-1",
            "horizon_hours": 6
        });

        // No auth -> 401
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/execution/run")
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Reader -> 403
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/execution/run")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Contributor -> reaches handler-level model lookup.
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/execution/run")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[cfg(feature = "execution")]
    #[actix_web::test]
    async fn execution_models_requires_reader_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/execution/models")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/execution/models")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert!(
            body.get("models").and_then(|v| v.as_array()).is_some(),
            "authorized execution model list should return a models array"
        );
    }

    #[cfg(feature = "execution")]
    #[actix_web::test]
    async fn execution_result_requires_reader_auth() {
        let state = test_state(None);
        {
            let mut results = state.execution_results.write().await;
            results.insert(
                "run-auth-test".to_string(),
                wrfcoin_execution_engine::types::ModelRunResult {
                    run_id: "run-auth-test".to_string(),
                    success: true,
                    exit_code: 0,
                    execution_time_secs: 1.25,
                    output: None,
                    error: None,
                },
            );
        }
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/execution/result/run-auth-test")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/execution/result/run-auth-test")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["run_id"].as_str(), Some("run-auth-test"));
        assert_eq!(body["success"].as_bool(), Some(true));
    }

    #[actix_web::test]
    async fn oracle_config_update_requires_operator_auth() {
        let (_oracle_sk, oracle_id) = keypair(301);
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let body = json!({
            "oracle_set": [oracle_id],
            "oracle_quorum": 1
        });

        // No auth -> 401
        let resp = test::call_service(
            &app,
            test::TestRequest::put()
                .uri("/transactions/oracle/config")
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Reader -> 403
        let resp = test::call_service(
            &app,
            test::TestRequest::put()
                .uri("/transactions/oracle/config")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Operator -> 200
        let resp = test::call_service(
            &app,
            test::TestRequest::put()
                .uri("/transactions/oracle/config")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// Build (auth_pubkey_hex, auth_signature_hex) for a commit body (#1848).
    fn sign_commit_auth(station_id: &str, commit_hash: &str, private_key: &PrivateKey) -> (String, String) {
        let msg = commit_authorization_message(station_id, commit_hash, "contribution");
        let sig = private_key.sign(&msg);
        let pk = PublicKey::from_private_key(private_key);
        (pk.to_hex(), sig.to_hex())
    }

    #[actix_web::test]
    async fn contribution_commit_requires_contributor_role() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        // Role-check uses a body without auth fields; role is checked before auth.
        let body_no_auth = json!({
            "contributor": "test-contributor",
            "station_id": "station-001",
            "commit_hash": "a".repeat(64)
        });

        // Build a signed body for contributor-role assertions (#1848).
        let (sk, _) = keypair(0xC0FF);
        let commit_hash_a = "a".repeat(64);
        let (auth_pubkey, auth_signature) =
            sign_commit_auth("station-001", &commit_hash_a, &sk);
        let body_signed = json!({
            "contributor": "test-contributor",
            "station_id": "station-001",
            "commit_hash": commit_hash_a,
            "auth_pubkey": auth_pubkey,
            "auth_signature": auth_signature,
        });

        // No auth -> 401
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/contribution/commit")
                .set_json(&body_no_auth)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Reader -> 403
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/contribution/commit")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .set_json(&body_no_auth)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Oracle -> 403 (wrong role)
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/contribution/commit")
                .insert_header(("Authorization", format!("Bearer {TEST_ORACLE_TOKEN}")))
                .set_json(&body_no_auth)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Contributor -> 201 (signed commit)
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/contribution/commit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(&body_signed)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Operator -> 201 (operator satisfies all roles, different commit)
        let (sk2, _) = keypair(0xC0FE);
        let commit_hash_b = "b".repeat(64);
        let (auth_pubkey2, auth_signature2) =
            sign_commit_auth("station-002", &commit_hash_b, &sk2);
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/contribution/commit")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "contributor": "test-contributor-2",
                    "station_id": "station-002",
                    "commit_hash": commit_hash_b,
                    "auth_pubkey": auth_pubkey2,
                    "auth_signature": auth_signature2,
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[actix_web::test]
    async fn contribution_commit_rate_limit_returns_429_per_contributor() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (rate_sk, _) = keypair(0xBEEF);

        for index in 0..CONTRIBUTION_COMMIT_RATE_LIMIT_MAX_REQUESTS {
            let commit_hash = format!("{index:064x}");
            let (auth_pubkey, auth_signature) =
                sign_commit_auth("station-001", &commit_hash, &rate_sk);
            let resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/transactions/contribution/commit")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                    .set_json(json!({
                        "contributor": "bursting-contributor",
                        "station_id": "station-001",
                        "commit_hash": commit_hash,
                        "auth_pubkey": auth_pubkey,
                        "auth_signature": auth_signature,
                    }))
                    .to_request(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::CREATED);
        }

        // One more with the same pubkey: rate-limited before auth is checked.
        let commit_hash_f = "f".repeat(64);
        let (auth_pubkey_f, auth_signature_f) =
            sign_commit_auth("station-001", &commit_hash_f, &rate_sk);
        let limited = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/contribution/commit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({
                    "contributor": "bursting-contributor",
                    "station_id": "station-001",
                    "commit_hash": commit_hash_f,
                    "auth_pubkey": auth_pubkey_f,
                    "auth_signature": auth_signature_f,
                }))
                .to_request(),
        )
        .await;
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after = limited
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .expect("429 responses should include Retry-After");
        assert_eq!(retry_after, LEGACY_SUBMIT_RETRY_AFTER_SECS.to_string());
        let limited_body: serde_json::Value = test::read_body_json(limited).await;
        assert_error_envelope(&limited_body);

        let (ind_sk, _) = keypair(0xDEAD);
        let commit_hash_e = "e".repeat(64);
        let (auth_pubkey_e, auth_signature_e) =
            sign_commit_auth("station-002", &commit_hash_e, &ind_sk);
        let independent = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/contribution/commit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({
                    "contributor": "independent-contributor",
                    "station_id": "station-002",
                    "commit_hash": commit_hash_e,
                    "auth_pubkey": auth_pubkey_e,
                    "auth_signature": auth_signature_e,
                }))
                .to_request(),
        )
        .await;
        assert_eq!(independent.status(), StatusCode::CREATED);
    }

    #[actix_web::test]
    async fn contribution_reveal_validates_commit_hash_and_accepts_matching_payload() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let contributor = "test-contributor";
        let station_id = "station-reveal-001";
        let nonce = "reveal-nonce-1";
        let timestamp = chrono::Utc::now().timestamp();
        let payload = json!({
            "temperature_c": 23.5,
            "humidity_percent": 54
        });
        let valid_commit_hash =
            Transaction::compute_commit_id(&payload, nonce, station_id, timestamp, "contribution");

        let mismatch_resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/contribution/reveal")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({
                    "contributor": contributor,
                    "station_id": station_id,
                    "payload": payload,
                    "nonce": nonce,
                    "commit_hash": "a".repeat(64),
                    "timestamp": timestamp
                }))
                .to_request(),
        )
        .await;
        assert_eq!(mismatch_resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let valid_resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/contribution/reveal")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({
                    "contributor": contributor,
                    "station_id": station_id,
                    "payload": payload,
                    "nonce": nonce,
                    "commit_hash": valid_commit_hash,
                    "timestamp": timestamp
                }))
                .to_request(),
        )
        .await;
        assert_eq!(valid_resp.status(), StatusCode::CREATED);
    }

    #[actix_web::test]
    async fn oracle_attestation_requires_oracle_role() {
        use wrfcoin_native_chain::security::key_registry::SigningRole;

        let (oracle_sk, oracle_id) = keypair(401);
        let state = test_state(None);
        {
            let mut blockchain = state.blockchain.write().await;
            blockchain
                .apply_state_transition(StateTransition::SetOracleConfig {
                    oracle_set: vec![oracle_id.clone()],
                    oracle_quorum: 1,
                })
                .unwrap();
            // Register the oracle's signing key so verify_signature_for_identity_role passes (#1844).
            blockchain
                .apply_state_transition(StateTransition::RegisterSigningKeyEpoch {
                    identity: oracle_id.clone(),
                    role: SigningRole::Oracle,
                    epoch: 1,
                    public_key: oracle_id.clone(),
                    activated_at: 0,
                })
                .unwrap();
        }
        let app = test::init_service(test_app(state.clone())).await;

        let timestamp = chrono::Utc::now().timestamp();
        let commit_hash = "c".repeat(64);
        let nonce = "test-nonce-oracle-attestation-1844";
        let payload = OracleAttestationPayload {
            commit_id: commit_hash.clone(),
            oracle_id: oracle_id.clone(),
            approved: true,
            signature: None,
            confidence: None,
            timestamp,
            nonce: Some(nonce.to_string()),
        };
        let message = payload.signing_message();
        let signature = oracle_sk.sign(message.as_bytes());
        let body = json!({
            "oracle_id": oracle_id,
            "commit_hash": commit_hash,
            "approved": true,
            "signature": signature.to_string(),
            "timestamp": timestamp,
            "nonce": nonce
        });

        // No auth -> 401
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/oracle/attestation")
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Reader -> 403
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/oracle/attestation")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Contributor -> 403
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/oracle/attestation")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Oracle -> 201
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/oracle/attestation")
                .insert_header(("Authorization", format!("Bearer {TEST_ORACLE_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[actix_web::test]
    async fn stake_routes_return_state_backed_fields_and_consistent_metrics() {
        let state = test_state(None);
        let primary = "0x3333333333333333333333333333333333333333"
            .parse::<Address>()
            .expect("valid address");
        let secondary = "0x4444444444444444444444444444444444444444"
            .parse::<Address>()
            .expect("valid address");

        let expected_metrics = {
            let mut blockchain = state.blockchain.write().await;
            blockchain.state.create_account(primary, Balance::new(500));
            blockchain
                .state
                .create_account(secondary, Balance::new(250));

            if let Some(account) = blockchain.state.accounts.get_mut(&primary) {
                account.stake_required = Balance::new(200);
                account.stake_locked = Balance::new(150);
                account.spendable_balance = Balance::new(350);
                account.earned_balance_total = Balance::new(42);
            }
            if let Some(account) = blockchain.state.accounts.get_mut(&secondary) {
                account.stake_required = Balance::new(100);
                account.stake_locked = Balance::new(20);
                account.spendable_balance = Balance::new(230);
                account.earned_balance_total = Balance::new(7);
            }

            let mut account_count = 0usize;
            let mut total_stake_required = 0u128;
            let mut total_stake_locked = 0u128;
            let mut total_spendable_balance = 0u128;
            let mut total_earned_balance = 0u128;
            for account in blockchain.state.accounts.values() {
                account_count += 1;
                total_stake_required =
                    total_stake_required.saturating_add(account.stake_required.into_inner());
                total_stake_locked =
                    total_stake_locked.saturating_add(account.stake_locked.into_inner());
                total_spendable_balance =
                    total_spendable_balance.saturating_add(account.spendable_balance.into_inner());
                total_earned_balance =
                    total_earned_balance.saturating_add(account.earned_balance_total.into_inner());
            }

            (
                account_count,
                total_stake_required,
                total_stake_locked,
                total_spendable_balance,
                total_earned_balance,
            )
        };

        let app = test::init_service(test_app(state.clone())).await;

        let stake_resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/stake/0x3333333333333333333333333333333333333333")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(stake_resp.status(), StatusCode::OK);
        let stake_json: serde_json::Value = test::read_body_json(stake_resp).await;
        assert_eq!(
            stake_json.get("address").and_then(|v| v.as_str()),
            Some("0x3333333333333333333333333333333333333333")
        );
        // Monetary fields serialize as decimal STRINGS (#2680, routes::decimal).
        assert_eq!(
            stake_json.get("stake_required").and_then(|v| v.as_str()),
            Some("200")
        );
        assert_eq!(
            stake_json.get("stake_locked").and_then(|v| v.as_str()),
            Some("150")
        );
        assert_eq!(
            stake_json.get("spendable_balance").and_then(|v| v.as_str()),
            Some("350")
        );
        assert_eq!(
            stake_json
                .get("earned_balance_total")
                .and_then(|v| v.as_str()),
            Some("42")
        );

        let metrics_resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/stake/metrics")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(metrics_resp.status(), StatusCode::OK);
        let metrics_json: serde_json::Value = test::read_body_json(metrics_resp).await;
        assert_eq!(
            metrics_json.get("account_count").and_then(|v| v.as_u64()),
            Some(expected_metrics.0 as u64)
        );
        assert_eq!(
            metrics_json
                .get("total_stake_required")
                .and_then(|v| v.as_str()),
            Some(expected_metrics.1.to_string().as_str())
        );
        assert_eq!(
            metrics_json
                .get("total_stake_locked")
                .and_then(|v| v.as_str()),
            Some(expected_metrics.2.to_string().as_str())
        );
        assert_eq!(
            metrics_json
                .get("total_spendable_balance")
                .and_then(|v| v.as_str()),
            Some(expected_metrics.3.to_string().as_str())
        );
        assert_eq!(
            metrics_json
                .get("total_earned_balance")
                .and_then(|v| v.as_str()),
            Some(expected_metrics.4.to_string().as_str())
        );
    }

    #[actix_web::test]
    async fn get_routes_do_not_require_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        for uri in ["/health"] {
            let resp =
                test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "GET {uri} should be public (no auth required)"
            );
        }

        // These reads are intentionally NOT public: #1261/#1268 restricted
        // /network/audit-log to Operators; bridge-safety exposure/audit-log,
        // snapshot endpoints, stake status, oracle config/status, contribution
        // statuses, quality event/summary reads, legacy address financial
        // reads, oracle leaderboard identity reads, and quality participant
        // history require a Reader token.
        // Unauthenticated callers must get 401.
        for uri in [
            "/network/audit-log",
            "/bridge/safety/exposure",
            "/bridge/safety/audit-log",
            // #1839: snapshot endpoints now require Reader auth
            "/state/snapshot",
            "/state/snapshot/entries",
            "/state/snapshot/sync-payload",
            // #1841: per-address stake status now requires Reader auth
            "/transactions/stake/0x1111111111111111111111111111111111111111",
            // #1840: oracle identity set now requires Reader auth
            "/transactions/oracle/config",
            "/transactions/oracle/status",
            // #1842: contribution status scan now requires Reader auth
            "/transactions/contributions/status",
            // #1884: quality events/summary now require Reader auth
            "/transactions/quality/events",
            "/transactions/quality/summary",
            // #1885: legacy address financial/stake disclosure requires Reader auth
            "/addresses/0x1111111111111111111111111111111111111111",
            // #1886: oracle leaderboard identity reads require Reader auth
            "/oracle/leaderboard",
            // #1843: per-participant slashing history now requires Reader auth
            "/transactions/quality/participants/0x1111111111111111111111111111111111111111/history",
            // #1986: aggregate stake metrics, bridge-safety status and NFT reads
            // were the last unauthenticated reads; they now require Reader auth.
            "/transactions/stake/metrics",
            "/bridge/safety/status",
            "/nft/list",
            "/nft/any-id",
        ] {
            let resp =
                test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
            assert_eq!(
                resp.status(),
                StatusCode::UNAUTHORIZED,
                "GET {uri} must require auth (#1839/#1840/#1841/#1842/#1843/#1884/#1885/#1886)"
            );
        }
    }

    /// /metrics and /prometheus must reject unauthenticated callers and accept
    /// valid Reader (or higher) tokens. Closes #1288.
    #[actix_web::test]
    #[serial_test::serial(core_metrics)]
    async fn metrics_endpoints_require_reader_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        for uri in ["/metrics", "/prometheus"] {
            // No auth header → 401 UNAUTHORIZED
            let resp = test::call_service(
                &app,
                test::TestRequest::get().uri(uri).to_request(),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::UNAUTHORIZED,
                "GET {uri} with no auth should return 401"
            );

            // Invalid (unknown) token → 403 FORBIDDEN
            let resp = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri(uri)
                    .insert_header(("Authorization", "Bearer invalid-token"))
                    .to_request(),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::FORBIDDEN,
                "GET {uri} with invalid token should return 403"
            );

            // Valid Reader token → 200 OK
            let resp = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri(uri)
                    .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                    .to_request(),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "GET {uri} with Reader token should return 200"
            );

            // Operator token also satisfies Reader → 200 OK
            let resp = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri(uri)
                    .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                    .to_request(),
            )
            .await;
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "GET {uri} with Operator token should return 200 (Operator satisfies Reader)"
            );
        }
    }

    async fn assert_mempool_purge_route_removes_stale_entries(route: &str) {
        let state = test_state(None);
        {
            let mut mempool = state.mempool.write().await;
            mempool
                .add_transaction(test_mempool_tx(1))
                .expect("first mempool tx should be accepted");
            mempool
                .add_transaction(test_mempool_tx(2))
                .expect("second mempool tx should be accepted");
            assert_eq!(mempool.size(), 2, "test setup should seed two mempool entries");
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri(route)
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "{route} must return 200");

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(
            body["removed"].as_u64(),
            Some(2),
            "{route} must report that both stale transactions were purged"
        );
        assert_eq!(
            body["remaining"].as_u64(),
            Some(0),
            "{route} must report an empty mempool after purge"
        );

        let mempool = state.mempool.read().await;
        assert_eq!(mempool.size(), 0, "{route} must actually clear the stale entries");
    }

    #[actix_web::test]
    async fn operator_mempool_purge_route_removes_stale_entries() {
        assert_mempool_purge_route_removes_stale_entries("/operator/mempool/purge?max_age_secs=1")
            .await;
    }

    #[actix_web::test]
    async fn mempool_purge_alias_route_removes_stale_entries() {
        assert_mempool_purge_route_removes_stale_entries("/mempool/purge?max_age_secs=1").await;
    }

    #[actix_web::test]
    async fn network_audit_log_returns_deterministic_filtered_results() {
        let state = test_state(None);
        {
            let mut blockchain = state.blockchain.write().await;
            blockchain
                .apply_state_transition(StateTransition::RecordAuditEvent {
                    event: AuditEventRecord {
                        event_id: 0,
                        block_height: 9,
                        timestamp: 1_700_000_001,
                        kind: AuditEventKind::BridgeSafetyHalt,
                        subject_id: Some("bridge-1".to_string()),
                        actor_id: Some("operator-a".to_string()),
                        details: Some(json!({ "reason": "manual" })),
                    },
                })
                .expect("first audit event");
            blockchain
                .apply_state_transition(StateTransition::RecordAuditEvent {
                    event: AuditEventRecord {
                        event_id: 0,
                        block_height: 10,
                        timestamp: 1_700_000_002,
                        kind: AuditEventKind::GovernanceProposalExecuted,
                        subject_id: Some("prop-1".to_string()),
                        actor_id: Some("operator-a".to_string()),
                        details: Some(json!({ "yes_votes": 5 })),
                    },
                })
                .expect("second audit event");
        }
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/network/audit-log?kind=governance_proposal_executed")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let payload: AuditLogResponse = test::read_body_json(resp).await;
        assert_eq!(payload.total, 1);
        assert_eq!(payload.items[0].event_id, 2);
        assert_eq!(payload.items[0].subject_id.as_deref(), Some("prop-1"));
    }

    #[actix_web::test]
    async fn bridge_halt_route_persists_unified_audit_event() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let halt_resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/bridge/safety/halt")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "reason": "manual halt for audit" }))
                .to_request(),
        )
        .await;
        assert_eq!(halt_resp.status(), StatusCode::OK);

        let audit_resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/network/audit-log?kind=bridge_safety_halt")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(audit_resp.status(), StatusCode::OK);
        let payload: AuditLogResponse = test::read_body_json(audit_resp).await;
        assert_eq!(payload.total, 1);
        assert_eq!(payload.items[0].kind, AuditEventKind::BridgeSafetyHalt);
        assert_eq!(
            payload.items[0]
                .details
                .as_ref()
                .and_then(|details| details.get("reason"))
                .and_then(|reason| reason.as_str()),
            Some("manual halt for audit")
        );
    }

    #[actix_web::test]
    async fn invalid_token_returns_forbidden() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", "Bearer invalid-token-12345"))
                .set_json(json!({
                    "miner_address": "0x1111111111111111111111111111111111111111"
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn malformed_auth_header_returns_unauthorized() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        // "Basic" scheme instead of "Bearer"
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", "Basic dXNlcjpwYXNz"))
                .set_json(json!({
                    "miner_address": "0x1111111111111111111111111111111111111111"
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    fn assert_envelope_top_level(body: &serde_json::Value) {
        assert!(body.get("ok").and_then(|value| value.as_bool()).is_some());
        let meta = body
            .get("meta")
            .and_then(|value| value.as_object())
            .expect("meta must be an object");
        assert!(
            meta.get("request_id")
                .and_then(|value| value.as_str())
                .map(|value| !value.is_empty())
                .unwrap_or(false),
            "meta.request_id must be present",
        );
        assert!(
            meta.get("timestamp")
                .and_then(|value| value.as_str())
                .map(|value| !value.is_empty())
                .unwrap_or(false),
            "meta.timestamp must be present",
        );
        assert!(
            meta.get("api_version")
                .and_then(|value| value.as_str())
                .map(|value| !value.is_empty())
                .unwrap_or(false),
            "meta.api_version must be present",
        );
    }

    fn assert_success_envelope(body: &serde_json::Value) {
        assert_envelope_top_level(body);
        assert_eq!(body["ok"].as_bool(), Some(true));
        assert!(
            body.get("data")
                .map(|value| !value.is_null())
                .unwrap_or(false),
            "data must be present for success responses",
        );
        assert!(
            body.get("error")
                .map(|value| value.is_null())
                .unwrap_or(true),
            "error must be absent for success responses",
        );
    }

    fn assert_error_envelope(body: &serde_json::Value) {
        assert_envelope_top_level(body);
        assert_eq!(body["ok"].as_bool(), Some(false));
        assert!(
            body.get("data")
                .map(|value| value.is_null())
                .unwrap_or(true),
            "data must be absent for error responses",
        );
        let error = body
            .get("error")
            .and_then(|value| value.as_object())
            .expect("error must be an object for error responses");
        assert!(
            error
                .get("code")
                .and_then(|value| value.as_str())
                .map(|value| !value.is_empty())
                .unwrap_or(false),
            "error.code must be present",
        );
        assert!(
            error
                .get("message")
                .and_then(|value| value.as_str())
                .map(|value| !value.is_empty())
                .unwrap_or(false),
            "error.message must be present",
        );
    }

    fn nft_metadata_json(creator_hex: &str) -> serde_json::Value {
        // Address serializes as a newtype over [u8; 20], so serde expects an
        // array of 20 numbers.  Convert the hex string to that representation.
        let addr: Address = creator_hex.parse().expect("valid test address");
        let creator_value = serde_json::to_value(addr).expect("serialize address");
        json!({
            "name": "Test NFT",
            "description": "A test computational NFT",
            "data_type": "weather",
            "region": null,
            "accuracy": null,
            "creation_time": 1000,
            "creator": creator_value,
            "metadata_uri": null,
            "properties": {}
        })
    }

    #[actix_web::test]
    #[serial_test::serial(core_metrics)]
    async fn envelope_smoke_suite_covers_legacy_compat_endpoints() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let blockchain_resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/blockchain")
                .insert_header(("X-Request-Id", "smoke-blockchain"))
                .to_request(),
        )
        .await;
        assert_eq!(blockchain_resp.status(), StatusCode::OK);
        let blockchain_request_id = blockchain_resp
            .headers()
            .get("X-Request-Id")
            .and_then(|value| value.to_str().ok())
            .expect("response must include X-Request-Id")
            .to_string();
        let blockchain_body: serde_json::Value = test::read_body_json(blockchain_resp).await;
        assert_success_envelope(&blockchain_body);
        assert_eq!(
            blockchain_body["meta"]["request_id"].as_str(),
            Some(blockchain_request_id.as_str()),
        );

        let unauth_submit = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .set_json(json!({"from":"a","to":"b","amount":1}))
                .to_request(),
        )
        .await;
        assert_eq!(unauth_submit.status(), StatusCode::UNAUTHORIZED);
        let unauth_submit_body: serde_json::Value = test::read_body_json(unauth_submit).await;
        assert_error_envelope(&unauth_submit_body);

        let bad_key_submit = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", "Bearer invalid-token"))
                .set_json(json!({"from":"a","to":"b","amount":1}))
                .to_request(),
        )
        .await;
        assert_eq!(bad_key_submit.status(), StatusCode::FORBIDDEN);
        let bad_key_submit_body: serde_json::Value = test::read_body_json(bad_key_submit).await;
        assert_error_envelope(&bad_key_submit_body);

        let invalid_submit = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({"from":"","to":"","amount":0}))
                .to_request(),
        )
        .await;
        assert_eq!(invalid_submit.status(), StatusCode::BAD_REQUEST);
        let invalid_submit_body: serde_json::Value = test::read_body_json(invalid_submit).await;
        assert_error_envelope(&invalid_submit_body);

        let address_resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/addresses/0x1111111111111111111111111111111111111111")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(address_resp.status(), StatusCode::NOT_FOUND);
        let address_body: serde_json::Value = test::read_body_json(address_resp).await;
        assert_error_envelope(&address_body);

        let mint_resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/mint")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "id": "nft-smoke-001",
                    "owner": "0x1111111111111111111111111111111111111111",
                    "metadata": nft_metadata_json("0x1111111111111111111111111111111111111111")
                }))
                .to_request(),
        )
        .await;
        assert_eq!(mint_resp.status(), StatusCode::CREATED);

        let nft_resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/nft/nft-smoke-001")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(nft_resp.status(), StatusCode::OK);
        let nft_request_id = nft_resp
            .headers()
            .get("X-Request-Id")
            .and_then(|value| value.to_str().ok())
            .expect("response must include X-Request-Id")
            .to_string();
        let nft_body: serde_json::Value = test::read_body_json(nft_resp).await;
        assert_success_envelope(&nft_body);
        assert_eq!(
            nft_body["meta"]["request_id"].as_str(),
            Some(nft_request_id.as_str()),
        );

        let metrics_resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/metrics")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(metrics_resp.status(), StatusCode::OK);
        let metrics_body = String::from_utf8(test::read_body(metrics_resp).await.to_vec())
            .expect("metrics endpoint should return utf-8 prometheus text");
        assert!(
            metrics_body.contains("wrfcoin_blockchain_block_height"),
            "unexpected metrics output: {metrics_body}"
        );

        let heatmap_resp = test::call_service(
            &app,
            test::TestRequest::get().uri("/heatmap/json").to_request(),
        )
        .await;
        assert_eq!(heatmap_resp.status(), StatusCode::OK);
        let heatmap_body: serde_json::Value = test::read_body_json(heatmap_resp).await;
        assert_success_envelope(&heatmap_body);

        for endpoint in ["/cross_chain", "/cross_chain/status"] {
            let resp =
                test::call_service(&app, test::TestRequest::get().uri(endpoint).to_request()).await;
            assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body: serde_json::Value = test::read_body_json(resp).await;
            assert_error_envelope(&body);
            assert_eq!(body["error"]["code"], "NOT_AVAILABLE");
        }

        for endpoint in [
            "/enhanced_security/status",
            "/layer2/status",
            "/evm_compatibility/status",
        ] {
            let resp =
                test::call_service(&app, test::TestRequest::get().uri(endpoint).to_request()).await;
            assert_eq!(
                resp.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "{endpoint} should return disabled-feature fallback",
            );
            let body: serde_json::Value = test::read_body_json(resp).await;
            assert_error_envelope(&body);
        }
    }

    #[actix_web::test]
    async fn readyz_returns_200_for_fresh_single_operator_state() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/readyz")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "ready");
        assert_eq!(body["checks"]["storage"]["status"], "ok");
        assert_eq!(body["checks"]["block_production"]["status"], "ok");
        assert_eq!(body["checks"]["p2p_startup"]["status"], "disabled");
    }

    #[actix_web::test]
    async fn readyz_reports_failed_required_p2p_as_not_ready() {
        let base = test_state(None);
        let state = web::Data::new(AppState {
            p2p_startup: Arc::new(P2pStartupStatus {
                state: "failed",
                profile: "required",
                required: true,
                identity: "none",
                simulated: false,
                detail: Some("transport failed".to_string()),
            }),
            ..(**base).clone()
        });
        let app = test::init_service(test_app(state)).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/readyz")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["checks"]["p2p_startup"]["status"], "failed");
        assert_eq!(body["checks"]["p2p_startup"]["required"], true);
        assert_eq!(body["checks"]["p2p_startup"]["readiness_gate"], true);
    }

    /// #2188: the light-node routes serve a header projection whose hash is
    /// the block's own, and chunk proofs that verify against the served
    /// commitment; an out-of-range chunk and an unknown height are 404.
    #[actix_web::test]
    async fn light_node_routes_serve_a_verifiable_header_and_chunk_proofs_2188() {
        let state = test_state(None);
        let block_hash = {
            let mut blockchain = state.blockchain.write().await;
            let mut block = Block::new(1, Hash::default(), Vec::new(), 1, 1);
            block.timestamp = chrono::Utc::now();
            block.set_hash(block.calculate_hash());
            let hash = block.hash.unwrap();
            blockchain.add_block(block).expect("test block should be added");
            hash
        };
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(&app, test::TestRequest::get().uri("/chain/header/1").to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        let header: wrfcoin_shared_protocol::blockchain::light_header::LightHeader =
            serde_json::from_value(body["data"]["header"].clone()).expect("header projection");
        assert_eq!(header.height, 1);
        assert_eq!(header.verify_declared_hash().unwrap(), block_hash);
        let commitment: wrfcoin_da::DaCommitment =
            serde_json::from_value(body["data"]["da_commitment"].clone()).expect("commitment");
        assert_eq!(body["data"]["da_commitment_source"], "served");

        let resp = test::call_service(&app, test::TestRequest::get().uri("/da/chunk/1/0").to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        let proof: wrfcoin_da::ChunkInclusionProof =
            serde_json::from_value(body["data"]["proof"].clone()).expect("proof");
        assert!(commitment.verify(&proof), "served chunk proves under the served commitment");

        let uri = format!("/da/chunk/1/{}", commitment.total_chunks);
        let resp = test::call_service(&app, test::TestRequest::get().uri(&uri).to_request()).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let resp = test::call_service(&app, test::TestRequest::get().uri("/chain/header/999").to_request()).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn readyz_blocks_on_an_unauthorized_next_block_producer() {
        let base = test_state(None);
        let boot = ProducerAuthorityBootStatus {
            status: "unauthorized".to_string(),
            next_height: 8,
            node_key: Some("node-key".to_string()),
            authorized_set: vec!["next=0xset-key".to_string()],
            message: "producer key node-key is NOT authorized".to_string(),
            readiness_gate: true,
        };
        let state = web::Data::new(AppState {
            producer_authority_boot: Arc::new(boot),
            p2p_startup: Arc::new(P2pStartupStatus::default()),
            ..(**base).clone()
        });
        let app = test::init_service(test_app(state)).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/readyz")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "not_ready");
        assert_eq!(
            body["checks"]["next_block_producer_authority"]["status"],
            "unauthorized"
        );
        assert_eq!(
            body["checks"]["next_block_producer_authority"]["readiness_gate"],
            true
        );
    }

    #[actix_web::test]
    async fn readyz_keeps_healthy_testnet_cadence_ready() {
        let state = test_state(None);
        {
            let mut blockchain = state.blockchain.write().await;
            let mut recent_block = Block::new(1, Hash::default(), Vec::new(), 1, 1);
            recent_block.timestamp = chrono::Utc::now() - chrono::Duration::seconds(301);
            recent_block.set_hash(recent_block.calculate_hash());
            blockchain
                .add_block(recent_block)
                .expect("recent test block should be added");
        }
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/readyz")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "ready");
        assert_eq!(body["checks"]["block_production"]["status"], "ok");
    }

    #[actix_web::test]
    async fn readyz_returns_503_for_stale_block_production() {
        let state = test_state(None);
        {
            let mut blockchain = state.blockchain.write().await;
            let mut stale_block = Block::new(1, Hash::default(), Vec::new(), 1, 1);
            stale_block.timestamp = chrono::Utc::now()
                - chrono::Duration::seconds(crate::helpers::NODE_SYNC_STALE_AFTER_SECS + 1);
            stale_block.set_hash(stale_block.calculate_hash());
            blockchain
                .add_block(stale_block)
                .expect("stale test block should be added");
        }
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/readyz")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "not_ready");
        assert_eq!(body["checks"]["block_production"]["status"], "stale");
    }

    #[actix_web::test]
    async fn readyz_returns_503_when_state_has_diverged() {
        let state = test_state(None);
        state
            .state_diverged
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/readyz")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "not_ready");
        assert_eq!(body["checks"]["state_divergence"]["status"], "diverged");
        assert_eq!(body["checks"]["state_divergence"]["readiness_gate"], true);
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn readyz_treats_empty_blocks_mesh_as_diagnostic_not_a_gate() {
        let mut mesh_peers_by_topic = std::collections::BTreeMap::new();
        mesh_peers_by_topic.insert("blocks".to_string(), 0);
        mesh_peers_by_topic.insert("transactions".to_string(), 1);
        mesh_peers_by_topic.insert("weather".to_string(), 1);
        mesh_peers_by_topic.insert("forecast".to_string(), 1);

        let p2p = P2pClient::from_stats(wrfcoin_p2p::NetworkStats {
            connected_peers: 1,
            mesh_peers_by_topic,
            blocks_received: 0,
            blocks_published: 0,
            txs_received: 0,
            txs_published: 0,
            weather_msgs_received: 0,
            weather_msgs_published: 0,
            weather_msgs_invalid_sig: 0,
            forecast_tasks_received: 0,
            forecast_tasks_published: 0,
            bytes_inbound: 0,
            bytes_outbound: 0,
            inbound_blocks: Default::default(),
            inbound_txs: Default::default(),
            inbound_weather: Default::default(),
            inbound_forecast: Default::default(),
            outbound_blocks: Default::default(),
            outbound_txs: Default::default(),
            outbound_weather: Default::default(),
            outbound_forecast: Default::default(),
            finality_votes_received: 0,
            finality_votes_published: 0,
            finality_certificates_received: 0,
            finality_certificates_published: 0,
            messages_rejected: 0,
            uptime_secs: 0,
            bootstrap_redial_attempts_success: 0,
            bootstrap_redial_attempts_fail: 0,
            // Spread rather than an exhaustive list: this fixture cares about
            // peer and mesh counts, and enumerating every field made unrelated
            // metrics additions break it (the #2388 feature-matrix rot).
            ..Default::default()
        });
        let state = test_state_with_p2p(None, p2p);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/readyz")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .to_request(),
        )
        .await;
        // #a8867c91e demoted the p2p mesh check to diagnostic-only on
        // single-operator testnets: an empty blocks mesh is surfaced (status +
        // hint) but must NOT block readiness, so /readyz still returns 200/ready.
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "ready");
        assert_eq!(body["checks"]["p2p_mesh"]["status"], "empty_mesh");
        assert_eq!(body["checks"]["p2p_mesh"]["mesh_peers"], 0);
        assert_eq!(body["checks"]["p2p_mesh"]["readiness_gate"], false);
    }

    #[cfg(feature = "p2p")]
    /// #2820: the per-peer sync rotation surface. Non-vacuous on both sides:
    /// the Reader-gated read returns the published entries (including the
    /// ineligibility reason that was invisible during the 2026-08-27 stall),
    /// and the unauthenticated read is refused.
    #[actix_web::test]
    async fn network_peers_sync_exposes_rotation_eligibility_2820() {
        let p2p = P2pClient::for_testing(libp2p::PeerId::random());
        p2p.sync_peers.publish(vec![
            wrfcoin_p2p::sync_surface::SyncPeerSurfaceEntry {
                peer_id: "12D3KooWEligible".to_string(),
                advertised_height: 14_020,
                required_height: 14_010,
                advertisement_age_secs: Some(3),
                connection_age_secs: Some(600),
                in_flight_requests: 0,
                consecutive_transport_failures: 0,
                cooling_down: false,
                retry_after_ms: 0,
                banned: false,
                metered: false,
                priority: 10,
                eligible: true,
                eligible_via: Some("strict"),
                ineligible_reason: None,
            },
            wrfcoin_p2p::sync_surface::SyncPeerSurfaceEntry {
                peer_id: "12D3KooWStale".to_string(),
                advertised_height: 13_900,
                required_height: 14_010,
                advertisement_age_secs: Some(720),
                connection_age_secs: Some(3_600),
                in_flight_requests: 0,
                consecutive_transport_failures: 2,
                cooling_down: true,
                retry_after_ms: 4_500,
                banned: false,
                metered: false,
                priority: -3,
                eligible: false,
                eligible_via: None,
                ineligible_reason: Some("cooldown"),
            },
        ]);
        let state = test_state_with_p2p(None, p2p);
        let app = test::init_service(test_app(state.clone())).await;

        // Unauthenticated: topology stays hidden (#1829 precedent).
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/network/peers/sync")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/network/peers/sync")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["total"], 2);
        assert_eq!(body["eligible"], 1);
        assert_eq!(body["peers"][0]["advertised_height"], 14_020);
        assert_eq!(body["peers"][0]["eligible"], true);
        assert_eq!(body["peers"][0]["eligible_via"], "strict");
        assert_eq!(
            body["peers"][0]["ineligible_reason"],
            serde_json::Value::Null
        );
        assert_eq!(body["peers"][1]["advertised_height"], 13_900);
        assert_eq!(body["peers"][1]["required_height"], 14_010);
        assert_eq!(body["peers"][1]["advertisement_age_secs"], 720);
        assert_eq!(body["peers"][1]["connection_age_secs"], 3_600);
        assert_eq!(body["peers"][1]["cooling_down"], true);
        assert_eq!(body["peers"][1]["retry_after_ms"], 4_500);
        assert_eq!(body["peers"][1]["consecutive_transport_failures"], 2);
        assert_eq!(body["peers"][1]["eligible"], false);
        assert_eq!(body["peers"][1]["ineligible_reason"], "cooldown");
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn network_stats_exposes_mesh_peers_by_topic() {
        let mut mesh_peers_by_topic = std::collections::BTreeMap::new();
        mesh_peers_by_topic.insert("blocks".to_string(), 2);
        mesh_peers_by_topic.insert("transactions".to_string(), 1);
        mesh_peers_by_topic.insert("weather".to_string(), 1);
        mesh_peers_by_topic.insert("forecast".to_string(), 0);

        let p2p = P2pClient::from_stats(wrfcoin_p2p::NetworkStats {
            connected_peers: 3,
            mesh_peers_by_topic,
            blocks_received: 7,
            blocks_published: 4,
            txs_received: 5,
            txs_published: 6,
            weather_msgs_received: 8,
            weather_msgs_published: 9,
            weather_msgs_invalid_sig: 0,
            forecast_tasks_received: 10,
            forecast_tasks_published: 11,
            bytes_inbound: 0,
            bytes_outbound: 0,
            inbound_blocks: Default::default(),
            inbound_txs: Default::default(),
            inbound_weather: Default::default(),
            inbound_forecast: Default::default(),
            outbound_blocks: Default::default(),
            outbound_txs: Default::default(),
            outbound_weather: Default::default(),
            outbound_forecast: Default::default(),
            finality_votes_received: 0,
            finality_votes_published: 0,
            finality_certificates_received: 0,
            finality_certificates_published: 0,
            messages_rejected: 0,
            uptime_secs: 12,
            sync_local_height: 11_016,
            sync_target_height: 12_801,
            sync_state: "downloading".to_string(),
            sync_pending_ranges: 2,
            sync_in_flight_ranges: 3,
            sync_outstanding_ranges: 5,
            sync_requests_dispatched: 13,
            sync_responses_received: 11,
            sync_transport_failures: 2,
            sync_ranges_requeued: 2,
            sync_peer_rotations: 1,
            sync_last_resort_dispatches: 9,
            sync_connection_refresh_attempts: 1,
            sync_inbound_requests: 17,
            sync_responses_sent: 16,
            sync_inbound_failures: 1,
            sync_reconnect_status_broadcasts: 4,
            bootstrap_redial_attempts_success: 0,
            bootstrap_redial_attempts_fail: 0,
            // Spread rather than an exhaustive list: this fixture cares about
            // peer and mesh counts, and enumerating every field made unrelated
            // metrics additions break it (the #2388 feature-matrix rot).
            ..Default::default()
        });
        let state = test_state_with_p2p(None, p2p);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/network/stats")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["connected_peers"], 3);
        assert_eq!(body["mesh_peers_by_topic"]["blocks"], 2);
        assert_eq!(body["mesh_peers_by_topic"]["transactions"], 1);
        assert_eq!(body["mesh_peers_by_topic"]["forecast"], 0);
        assert_eq!(body["sync_local_height"], 11_016);
        assert_eq!(body["sync_target_height"], 12_801);
        assert_eq!(body["sync_state"], "downloading");
        assert_eq!(body["sync_pending_ranges"], 2);
        assert_eq!(body["sync_in_flight_ranges"], 3);
        assert_eq!(body["sync_outstanding_ranges"], 5);
        assert_eq!(body["sync_requests_dispatched"], 13);
        assert_eq!(body["sync_responses_received"], 11);
        assert_eq!(body["sync_transport_failures"], 2);
        assert_eq!(body["sync_ranges_requeued"], 2);
        assert_eq!(body["sync_peer_rotations"], 1);
        assert_eq!(body["sync_last_resort_dispatches"], 9);
        assert_eq!(body["sync_connection_refresh_attempts"], 1);
        assert_eq!(body["sync_inbound_requests"], 17);
        assert_eq!(body["sync_responses_sent"], 16);
        assert_eq!(body["sync_inbound_failures"], 1);
        assert_eq!(body["sync_reconnect_status_broadcasts"], 4);
    }

    #[actix_web::test]
    async fn transactions_submit_rate_limit_returns_429_with_retry_after() {
        use wrfcoin_native_chain::blockchain::transaction::transparent_transfer_authorization_message;

        if crate::helpers::test_env::isolated_chain_domain(
            "tests::transactions_submit_rate_limit_returns_429_with_retry_after",
            "wrfcoin-main",
            "mainnet",
            "0x2727272727272727272727272727272727272727",
            "none",
        ) {
            return;
        }
        let _env = crate::helpers::test_env::lock();
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (from_sk, from_pk_hex) = keypair(0x1846_02);
        let from_pk = PublicKey::from_private_key(&from_sk);
        let from = format!("0x{}", hex::encode(Address::from_public_key(&from_pk).0));
        let to = "0x2222222222222222222222222222222222222222";
        let signed_payload = |nonce: u64| {
            let amount = 42u64;
            let auth_signature = from_sk
                .sign(&transparent_transfer_authorization_message(
                    &from, to, amount, nonce,
                ))
                .to_string();
            json!({
                "from": from,
                "to": to,
                "amount": amount,
                "nonce": nonce.to_string(),
                "transparent_opt_out": true,
                "auth_pubkey": from_pk_hex,
                "auth_signature": auth_signature,
            })
        };

        for nonce in 0..LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS {
            let resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/transactions/submit")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                    .set_json(signed_payload(nonce as u64))
                    .to_request(),
            )
            .await;
            let status = resp.status();
            let body: serde_json::Value = test::read_body_json(resp).await;
            assert_eq!(
                status,
                StatusCode::ACCEPTED,
                "submission error code: {}",
                body["error"]["code"],
            );
            assert_success_envelope(&body);
        }

        let limited = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(signed_payload(
                    LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS as u64,
                ))
                .to_request(),
        )
        .await;
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after = limited
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .expect("429 responses should include Retry-After");
        assert_eq!(retry_after, LEGACY_SUBMIT_RETRY_AFTER_SECS.to_string());
        let limited_body: serde_json::Value = test::read_body_json(limited).await;
        assert_error_envelope(&limited_body);
    }

    /// #2169 on chain 888: `passkey_p256_auth` is scheduled by an applied
    /// governance extension (h3400), not genesis. `/transactions/submit`
    /// refuses a passkey transfer while the feature is not active at tip+1,
    /// accepts it (through mempool admission) once it is, and leaves the
    /// ed25519 path unchanged on both sides of the height.
    #[actix_web::test]
    async fn transactions_submit_accepts_passkey_transfer_only_once_active_at_next_height() {
        use wrfcoin_native_chain::blockchain::transaction::transparent_transfer_authorization_message;
        use wrfcoin_shared_protocol::accounts::passkey::test_support::SoftwareAuthenticator;
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            ConsensusFeature, refresh_feature_schedule_extensions,
        };

        // Own process: the extension overlay is process-global.
        if crate::helpers::test_env::isolated_chain_domain(
            "tests::transactions_submit_accepts_passkey_transfer_only_once_active_at_next_height",
            "wrfcoin-main",
            "mainnet",
            "0x3400340034003400340034003400340034003400",
            "none",
        ) {
            return;
        }
        const H: u64 = 3_400;
        let _env = crate::helpers::test_env::lock();
        refresh_feature_schedule_extensions(vec![(
            ConsensusFeature::PasskeyP256Auth.name().to_string(),
            H,
        )]);
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let to = "0x3333333333333333333333333333333333333333";
        let amount = 42u64;

        let auth = SoftwareAuthenticator::new(&[0x34; 32], "wallet.wrfcoin.test");
        let passkey_from = auth.account().to_hex();
        let passkey_body = |nonce: u64, signed_amount: u64| {
            json!({
                "from": passkey_from,
                "to": to,
                "amount": amount,
                "nonce": nonce.to_string(),
                "transparent_opt_out": true,
                "passkey_auth": auth.authorize_transfer(to, signed_amount, nonce),
            })
        };
        let (ed_sk, ed_pk_hex) = keypair(0x3400_01);
        let ed_from = format!(
            "0x{}",
            hex::encode(Address::from_public_key(&PublicKey::from_private_key(&ed_sk)).0)
        );
        let ed_body = |nonce: u64| {
            json!({
                "from": ed_from,
                "to": to,
                "amount": amount,
                "nonce": nonce.to_string(),
                "transparent_opt_out": true,
                "auth_pubkey": ed_pk_hex,
                "auth_signature": ed_sk
                    .sign(&transparent_transfer_authorization_message(&ed_from, to, amount, nonce))
                    .to_string(),
            })
        };
        let submit = |body: serde_json::Value| {
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(body)
                .to_request()
        };

        // Tip = H-2: the next block (H-1) is below activation.
        state.blockchain.write().await.height = H - 2;
        let resp = test::call_service(&app, submit(passkey_body(0, amount))).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_error_envelope(&body);
        assert_eq!(body["error"]["code"], "FEATURE_NOT_ACTIVE", "{body}");
        assert_eq!(state.mempool.read().await.size(), 0, "nothing enqueued before activation");

        // ed25519 is accepted below activation exactly as before.
        let resp = test::call_service(&app, submit(ed_body(0))).await;
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        // Tip = H-1: the next block (H) is at activation.
        state.blockchain.write().await.height = H - 1;
        // A passkey assertion over a different amount is refused.
        let resp = test::call_service(&app, submit(passkey_body(0, amount + 1))).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["error"]["code"], "AUTHORIZATION_INVALID", "{body}");

        // The valid passkey transfer is accepted and admitted to the mempool.
        let resp = test::call_service(&app, submit(passkey_body(0, amount))).await;
        let status = resp.status();
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert_success_envelope(&body);
        assert_eq!(state.mempool.read().await.size(), 2, "ed25519 + passkey transfers pending");

        refresh_feature_schedule_extensions(Vec::new());
    }

    #[actix_web::test]
    async fn legacy_submit_rate_limit_helper_enforces_current_window() {
        let state = test_state(None);
        let req = test::TestRequest::post()
            .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
            .to_http_request();

        for _ in 0..LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS {
            assert!(
                crate::helpers::enforce_legacy_submit_rate_limit(&req, state.get_ref())
                    .await
                    .is_ok()
            );
        }

        let limited = crate::helpers::enforce_legacy_submit_rate_limit(&req, state.get_ref())
            .await
            .expect_err("request above the current window should be rejected");
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after = limited
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .expect("limited helper response should include Retry-After");
        assert_eq!(retry_after, LEGACY_SUBMIT_RETRY_AFTER_SECS.to_string());
    }

    #[actix_web::test]
    async fn legacy_submit_rate_limit_helper_allows_after_stale_window_expires() {
        let state = test_state(None);
        let req = test::TestRequest::post()
            .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
            .to_http_request();
        let now = chrono::Utc::now().timestamp_millis();
        let stale_ts = now - LEGACY_SUBMIT_RATE_LIMIT_WINDOW_SECS * 1_000 - 1;

        {
            let mut guard = state.legacy_submit_rate_limit.write().await;
            let mut counter = SlidingWindowCounter::new(
                LEGACY_SUBMIT_RATE_LIMIT_WINDOW_SECS * 1_000,
                LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS,
            );
            for _ in 0..LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS {
                assert!(counter.check_and_record(stale_ts));
            }
            guard.insert(TEST_CONTRIBUTOR_TOKEN.to_string(), counter);
        }

        assert!(
            crate::helpers::enforce_legacy_submit_rate_limit(&req, state.get_ref())
                .await
                .is_ok()
        );
    }

    #[actix_web::test]
    async fn legacy_submit_rate_limit_helper_sweeps_expired_keys_when_over_capacity() {
        let state = test_state(None);
        let req = test::TestRequest::post()
            .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
            .to_http_request();
        let now = chrono::Utc::now().timestamp_millis();
        let stale_ts = now - LEGACY_SUBMIT_RATE_LIMIT_WINDOW_SECS * 1_000 - 1;

        {
            let mut guard = state.legacy_submit_rate_limit.write().await;
            for index in 0..=LEGACY_SUBMIT_RATE_LIMIT_MAX_KEYS {
                let mut counter = SlidingWindowCounter::new(
                    LEGACY_SUBMIT_RATE_LIMIT_WINDOW_SECS * 1_000,
                    LEGACY_SUBMIT_RATE_LIMIT_MAX_REQUESTS,
                );
                assert!(counter.check_and_record(stale_ts));
                guard.insert(format!("expired-token-{index}"), counter);
            }
        }

        assert!(
            crate::helpers::enforce_legacy_submit_rate_limit(&req, state.get_ref())
                .await
                .is_ok()
        );

        let guard = state.legacy_submit_rate_limit.read().await;
        assert_eq!(guard.len(), 1);
        drop(guard);

        assert!(
            crate::helpers::enforce_legacy_submit_rate_limit(&req, state.get_ref())
                .await
                .is_ok(),
            "the retained token should still have room for a second request"
        );
        let limited = crate::helpers::enforce_legacy_submit_rate_limit(&req, state.get_ref())
            .await
            .expect_err("the third request should hit the configured rate limit");
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    /// Regression test for #2077: the permissionless-submit rate limiter is
    /// keyed by an anonymous signer fingerprint (no bearer token, no session),
    /// so an attacker can mint a fresh key per request simply by rotating
    /// keypairs. Without a capacity cap + sweep — mirroring
    /// `enforce_legacy_submit_rate_limit`'s existing bound — that map grows
    /// without limit and OOMs the node.
    #[actix_web::test]
    async fn permissionless_submit_rate_limit_helper_sweeps_expired_keys_when_over_capacity() {
        let state = test_state(None);
        let req = test::TestRequest::post().to_http_request();
        let now = chrono::Utc::now().timestamp_millis();
        let stale_ts = now - PERMISSIONLESS_SUBMIT_WINDOW_SECS * 1_000 - 1;

        {
            let mut guard = state.permissionless_submit_rate_limit.write().await;
            for index in 0..=PERMISSIONLESS_SUBMIT_RATE_LIMIT_MAX_KEYS {
                let mut counter = SlidingWindowCounter::new(
                    PERMISSIONLESS_SUBMIT_WINDOW_SECS * 1_000,
                    PERMISSIONLESS_SUBMIT_MAX_REQUESTS,
                );
                assert!(counter.check_and_record(stale_ts));
                guard.insert(format!("expired-signer-{index}"), counter);
            }
        }

        assert!(
            crate::helpers::enforce_permissionless_submit_rate_limit(
                &req,
                state.get_ref(),
                "fresh-signer",
            )
            .await
            .is_ok()
        );

        let guard = state.permissionless_submit_rate_limit.read().await;
        // The sweep drops every stale entry; only the newly-inserted key remains.
        assert_eq!(guard.len(), 1);
        drop(guard);

        for _ in 1..PERMISSIONLESS_SUBMIT_MAX_REQUESTS {
            assert!(
                crate::helpers::enforce_permissionless_submit_rate_limit(
                    &req,
                    state.get_ref(),
                    "fresh-signer",
                )
                .await
                .is_ok(),
                "the retained signer should still have room within its own budget"
            );
        }
        let limited = crate::helpers::enforce_permissionless_submit_rate_limit(
            &req,
            state.get_ref(),
            "fresh-signer",
        )
        .await
        .expect_err("exceeding the per-signer budget should be rejected");
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    /// Regression test for #2077: when the map is still saturated with
    /// *live* (non-expired) entries after a full sweep, new anonymous
    /// signers must be rejected with 503 rather than growing the map
    /// further — this is what actually bounds memory under a sustained
    /// keypair-rotation attack, since a sweep alone cannot help if every
    /// tracked key is still within its window.
    #[actix_web::test]
    async fn permissionless_submit_rate_limit_helper_rejects_when_capacity_exhausted_by_live_keys()
    {
        let state = test_state(None);
        let req = test::TestRequest::post().to_http_request();
        let now = chrono::Utc::now().timestamp_millis();

        {
            let mut guard = state.permissionless_submit_rate_limit.write().await;
            for index in 0..=PERMISSIONLESS_SUBMIT_RATE_LIMIT_MAX_KEYS {
                let mut counter = SlidingWindowCounter::new(
                    PERMISSIONLESS_SUBMIT_WINDOW_SECS * 1_000,
                    PERMISSIONLESS_SUBMIT_MAX_REQUESTS,
                );
                assert!(counter.check_and_record(now));
                guard.insert(format!("live-signer-{index}"), counter);
            }
        }

        let rejected = crate::helpers::enforce_permissionless_submit_rate_limit(
            &req,
            state.get_ref(),
            "brand-new-signer",
        )
        .await
        .expect_err("a saturated map of live entries should reject new anonymous signers");
        assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
        let retry_after = rejected
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .expect("503 capacity response should include Retry-After");
        assert_eq!(retry_after, LEGACY_SUBMIT_RETRY_AFTER_SECS.to_string());
    }

    #[cfg(feature = "privacy")]
    #[actix_web::test]
    async fn privacy_key_prune_helper_removes_only_expired_entries() {
        let state = test_state(None);
        let now = chrono::Utc::now().timestamp();

        {
            let mut guard = state.privacy_key_registry.write().await;
            guard.insert("expired".to_string(), (now - 1, [1u8; 32]));
            guard.insert(
                "active".to_string(),
                (now + state::PRIVACY_KEY_TTL_SECS, [2u8; 32]),
            );
        }

        let removed = crate::routes::privacy::privacy_handlers::prune_expired_private_keys_at(
            state.get_ref(),
            now,
        )
        .await;
        assert_eq!(removed, 1);

        let guard = state.privacy_key_registry.read().await;
        assert!(!guard.contains_key("expired"));
        assert!(guard.contains_key("active"));
    }

    #[actix_web::test]
    async fn default_transfer_path_is_feature_disabled() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let payload = json!({
            "from": "0x1111111111111111111111111111111111111111",
            "to": "0x2222222222222222222222222222222222222222",
            "amount": 42,
            "nonce": "1001"
        });

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(&payload)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_error_envelope(&body);
        assert_eq!(
            body["error"]["code"].as_str(),
            Some("FEATURE_DISABLED")
        );
        let message = body["error"]["message"].as_str().unwrap_or_default();
        assert!(message.contains("transparent_opt_out=true"));
        assert!(message.contains("#1727"));
        assert!(message.contains("#2080"));
        assert!(message.contains("#1165"));

        let mempool = state.mempool.read().await;
        assert!(mempool.get_all_transactions().is_empty());
        drop(mempool);
        assert!(state.permissionless_replay_seen.read().await.is_empty());
        assert!(state.legacy_submit_rate_limit.read().await.is_empty());
    }

    #[::core::prelude::v1::test]
    fn placeholder_confidential_payload_is_rejected_by_apply() {
        use wrfcoin_shared_protocol::blockchain::state::{StateError, StateTransition};

        let payload = crate::helpers::build_default_confidential_transfer_payload(
            "0x1111111111111111111111111111111111111111",
            "0x2222222222222222222222222222222222222222",
            42,
        );
        let mut state = wrfcoin_native_chain::blockchain::state::State::new();
        let err = state
            .apply_transition(StateTransition::ApplyConfidentialTransfer {
                payload,
                current_block: 0,
            })
            .expect_err("hash-shaped placeholder privacy data must never apply");
        assert!(matches!(err, StateError::Privacy(_)), "unexpected error: {err}");
    }

    /// #1846 regression — /transactions/submit deduplicates on from+nonce.
    ///
    /// Rate limit is 2 req/window so this test stays within budget:
    ///  request 1 (nonce=42) → consumed slot 1 → accepted
    ///  request 2 (nonce=42) → replay check fires before rate limit → 409 (slot not consumed)
    ///  request 3 (nonce=43) → consumed slot 2 → accepted
    #[actix_web::test]
    async fn legacy_submit_nonce_replay_protection_regression_1846() {
        let _env = hold_signing_domain_3337();
        use wrfcoin_native_chain::blockchain::transaction::transparent_transfer_authorization_message;

        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let (from_sk, from_pk_hex) = keypair(0x1846_01);
        let from_pk = PublicKey::from_private_key(&from_sk);
        let from = format!("0x{}", hex::encode(Address::from_public_key(&from_pk).0));
        let to = "0x2222222222222222222222222222222222222222";
        let signed_payload = |nonce: u64| {
            let amount = 10u64;
            let auth_signature = from_sk
                .sign(&transparent_transfer_authorization_message(
                    &from, to, amount, nonce,
                ))
                .to_string();
            json!({
                "from": from,
                "to": to,
                "amount": amount,
                "nonce": nonce.to_string(),
                "transparent_opt_out": true,
                "auth_pubkey": from_pk_hex,
                "auth_signature": auth_signature,
            })
        };

        // Slot 1 — first submission with nonce=42 must succeed.
        let resp1 = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(signed_payload(42))
                .to_request(),
        )
        .await;
        assert_eq!(
            resp1.status(),
            StatusCode::ACCEPTED,
            "first submit with nonce=42 must be accepted"
        );

        // Replay — rejected before rate limit, so does not consume a slot.
        let resp2 = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(signed_payload(42))
                .to_request(),
        )
        .await;
        assert_eq!(
            resp2.status(),
            StatusCode::CONFLICT,
            "replay of same from+nonce must be rejected with 409"
        );

        // Slot 2 — different nonce, not a replay, must be accepted.
        let resp3 = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(signed_payload(43))
                .to_request(),
        )
        .await;
        assert_eq!(
            resp3.status(),
            StatusCode::ACCEPTED,
            "different nonce must be accepted"
        );
    }

    /// #3258 regression — a rejected unsigned transfer must not reserve the
    /// caller's nonce before authorization is proven.
    #[actix_web::test]
    async fn rejected_unsigned_transfer_does_not_burn_nonce() {
        let _env = hold_signing_domain_3337();
        use wrfcoin_native_chain::blockchain::transaction::transparent_transfer_authorization_message;

        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let (from_sk, from_pk_hex) = keypair(0x3258_01);
        let from_pk = PublicKey::from_private_key(&from_sk);
        let from = format!("0x{}", hex::encode(Address::from_public_key(&from_pk).0));
        let to = "0x2222222222222222222222222222222222222222";
        let amount = 10u64;
        let nonce = 77u64;
        let unsigned = json!({
            "from": from,
            "to": to,
            "amount": amount,
            "nonce": nonce.to_string(),
            "transparent_opt_out": true,
        });

        let rejected = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}"))) // secret-leak-ok: test-only constant token
                .set_json(&unsigned)
                .to_request(),
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);

        let signature = from_sk
            .sign(&transparent_transfer_authorization_message(
                &from, to, amount, nonce,
            ))
            .to_string();
        let signed = json!({
            "from": from,
            "to": to,
            "amount": amount,
            "nonce": nonce.to_string(),
            "transparent_opt_out": true,
            "auth_pubkey": from_pk_hex,
            "auth_signature": signature,
        });
        let accepted = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}"))) // secret-leak-ok: test-only constant token
                .set_json(&signed)
                .to_request(),
        )
        .await;
        assert_eq!(
            accepted.status(),
            StatusCode::ACCEPTED,
            "the same nonce must remain usable after rejected authorization"
        );
    }

        /// #3258 review R1 — authorization failures are rate-limited. The legacy
    /// limiter runs BEFORE the authorization check, so a contributor-token
    /// holder cannot drive unlimited ed25519 verifications with forged
    /// authorizations. Keyed by the caller's token: it spends only the
    /// caller's own budget, never the victim's.
    #[actix_web::test]
    async fn forged_transfer_authorizations_are_rate_limited_3258() {
        use wrfcoin_native_chain::blockchain::transaction::transparent_transfer_authorization_message;

        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let (victim_sk, _) = keypair(0x3258_02);
        let victim_pk = PublicKey::from_private_key(&victim_sk);
        let from = format!("0x{}", hex::encode(Address::from_public_key(&victim_pk).0));
        let (attacker_sk, attacker_pk_hex) = keypair(0x3258_03);
        let to = "0x2222222222222222222222222222222222222222";

        let mut statuses = Vec::new();
        for nonce in 1u64..=5 {
            let forged = attacker_sk
                .sign(&transparent_transfer_authorization_message(&from, to, 10, nonce))
                .to_string();
            let resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/transactions/submit")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}"))) // secret-leak-ok: test-only constant token
                    .set_json(json!({
                        "from": from,
                        "to": to,
                        "amount": 10,
                        "nonce": nonce.to_string(),
                        "transparent_opt_out": true,
                        "auth_pubkey": attacker_pk_hex,
                        "auth_signature": forged,
                    }))
                    .to_request(),
            )
            .await;
            statuses.push(resp.status().as_u16());
        }
        assert_eq!(
            statuses,
            vec![400, 400, 429, 429, 429],
            "forged authorizations must be charged to the caller's legacy submit budget"
        );
    }

    // --- #3337: a valid transfer that is not admitted keeps its nonce -------

    fn address_hex_3337(pk: &PublicKey) -> String {
        format!("0x{}", hex::encode(Address::from_public_key(pk).0))
    }

    /// Hold the process-env lock for a whole test that signs and submits
    /// transparent transfers (#3337 build container flake). The transfer preimage folds
    /// in `signing_chain_id()`/`signing_network_id()`, read from the process
    /// env on EVERY call: when the test signs, when the handler verifies, and
    /// again when the mempool re-verifies at admission. Tests such as
    /// `genesis_producers::with_env` set `WRFCOIN_NETWORK_ID=testnet` under
    /// this lock; a test that does not hold it can have the domain flip
    /// between those reads and see a spurious 400 (AUTHORIZATION_INVALID or
    /// TRANSACTION_REJECTED).
    fn hold_signing_domain_3337() -> std::sync::MutexGuard<'static, ()> {
        crate::helpers::test_env::lock()
    }

    /// A signed legacy transparent transfer from `sk` at `nonce`.
    fn signed_transfer_3337(sk: &PrivateKey, pk_hex: &str, nonce: u64) -> serde_json::Value {
        use wrfcoin_native_chain::blockchain::transaction::transparent_transfer_authorization_message;
        let from = address_hex_3337(&PublicKey::from_private_key(sk));
        let to = "0x2222222222222222222222222222222222222222";
        let signature = sk
            .sign(&transparent_transfer_authorization_message(&from, to, 10, nonce))
            .to_string();
        json!({
            "from": from,
            "to": to,
            "amount": 10,
            "nonce": nonce.to_string(),
            "transparent_opt_out": true,
            "auth_pubkey": pk_hex,
            "auth_signature": signature,
        })
    }

    /// The same transfer with an authorization signed by the wrong key: a
    /// 400 AUTHORIZATION_INVALID, like the issue's `mainnet`-signed step B.
    fn forged_transfer_3337(owner: &PrivateKey, forger: &PrivateKey, nonce: u64) -> serde_json::Value {
        let forger_pk_hex = PublicKey::from_private_key(forger).to_string();
        let mut body = signed_transfer_3337(forger, &forger_pk_hex, nonce);
        body["from"] = json!(address_hex_3337(&PublicKey::from_private_key(owner)));
        body
    }

    /// POST /transactions/submit with the ONE shared contributor bearer — the
    /// backend relays every user's wallet send with a single token. Yields
    /// `(status, body)`.
    macro_rules! submit_3337 {
        ($app:expr, $body:expr) => {{
            let resp = test::call_service(
                $app,
                test::TestRequest::post()
                    .uri("/transactions/submit")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}"))) // secret-leak-ok: test-only constant token
                    .set_json($body)
                    .to_request(),
            )
            .await;
            let status = resp.status().as_u16();
            let json: serde_json::Value = test::read_body_json(resp).await;
            (status, json)
        }};
    }

    /// Every legacy limiter window elapses (the issue waited 65 s); the 600 s
    /// replay window does not.
    async fn elapse_legacy_rate_window_3337(state: &web::Data<AppState>) {
        state.legacy_submit_rate_limit.write().await.clear();
    }

    /// #3337: the measured sequence. On the shared relay bearer: A a valid
    /// transfer, B a bad authorization, C the user's valid next transfer, and
    /// D the same C body after the rate window. Before the fix C was 429
    /// (B had spent the shared token's 2/min) and D was 409 with the nonce
    /// burned for 10 minutes: `[202, 400, 429, 409]`. Now B is charged only
    /// to the failed-authorization budget, C is admitted, and D is a genuine
    /// replay of an admitted transfer.
    #[actix_web::test]
    async fn issue_sequence_valid_transfer_after_a_failed_auth_is_admitted_3337() {
        let _env = hold_signing_domain_3337();
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (user_sk, user_pk) = keypair(0x3337_01);
        let (forger_sk, _) = keypair(0x3337_02);

        let a = submit_3337!(&app, &signed_transfer_3337(&user_sk, &user_pk, 0));
        let b = submit_3337!(&app, &forged_transfer_3337(&user_sk, &forger_sk, 1));
        let c_body = signed_transfer_3337(&user_sk, &user_pk, 1);
        let c = submit_3337!(&app, &c_body);
        elapse_legacy_rate_window_3337(&state).await;
        let d = submit_3337!(&app, &c_body);

        assert_eq!(
            vec![a.0, b.0, c.0, d.0],
            vec![202, 400, 202, 409],
            "A valid, B bad auth, C valid next nonce, D = C after the window; bodies: \
             {a:?} {b:?} {c:?} {d:?}"
        );
        assert_eq!(b.1["error"]["code"], "AUTHORIZATION_INVALID");
        assert_eq!(state.mempool.read().await.get_all_transactions().len(), 2);
    }

    /// #3337 acceptance: a valid transfer the limiter refuses keeps its
    /// from+nonce, and the SAME signed body is admitted once the window
    /// allows. Before the fix the retry was 409 REPLAY_REJECTED.
    #[actix_web::test]
    async fn rate_limited_valid_transfer_keeps_its_nonce_3337() {
        let _env = hold_signing_domain_3337();
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (sk, pk) = keypair(0x3337_03);

        let mut statuses = Vec::new();
        for nonce in 0..=1 {
            statuses.push(submit_3337!(&app, &signed_transfer_3337(&sk, &pk, nonce)).0);
        }
        let third = signed_transfer_3337(&sk, &pk, 2);
        let limited = submit_3337!(&app, &third);
        statuses.push(limited.0);
        elapse_legacy_rate_window_3337(&state).await;
        let retry = submit_3337!(&app, &third);
        statuses.push(retry.0);

        assert_eq!(
            statuses,
            vec![202, 202, 429, 202],
            "the rate-limited transfer must be admitted on retry at the same nonce; \
             limited={limited:?} retry={retry:?}"
        );
        assert_eq!(limited.1["error"]["code"], "RATE_LIMITED");
        assert_eq!(state.mempool.read().await.get_all_transactions().len(), 3);
    }

    /// #3337: a valid transfer the mempool refuses (400 TRANSACTION_REJECTED)
    /// keeps its from+nonce too. Before the fix the retry was 409.
    #[actix_web::test]
    async fn mempool_refused_transfer_keeps_its_nonce_3337() {
        let _env = hold_signing_domain_3337();
        use wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode;
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (sk, pk) = keypair(0x3337_04);
        let body = signed_transfer_3337(&sk, &pk, 0);

        state.mempool.write().await.set_partition_mode(PartitionMode::ReadOnly);
        let refused = submit_3337!(&app, &body);
        state.mempool.write().await.set_partition_mode(PartitionMode::Normal);
        let retry = submit_3337!(&app, &body);

        assert_eq!(
            (refused.0, retry.0),
            (400, 202),
            "refused={refused:?} retry={retry:?}"
        );
        assert_eq!(refused.1["error"]["code"], "TRANSACTION_REJECTED");
    }

    /// #3337 / #3258 R1: failed authorizations stay charged to the caller's
    /// bearer (bounded ed25519 work per token), but in a budget of their own,
    /// so one relayed user's failures cannot starve another user's valid
    /// sends on the same shared relay token.
    #[actix_web::test]
    async fn one_users_failed_auth_does_not_starve_another_on_the_shared_token_3337() {
        let _env = hold_signing_domain_3337();
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (victim_sk, _) = keypair(0x3337_05);
        let (forger_sk, _) = keypair(0x3337_06);
        let (other_sk, other_pk) = keypair(0x3337_07);

        let mut forged = Vec::new();
        for nonce in 1..=3 {
            forged.push(
                submit_3337!(&app, &forged_transfer_3337(&victim_sk, &forger_sk, nonce))
                    .0,
            );
        }
        assert_eq!(forged, vec![400, 400, 429], "failed auth is still rate-limited (#3258 R1)");

        let other = submit_3337!(&app, &signed_transfer_3337(&other_sk, &other_pk, 0));
        assert_eq!(other.0, 202, "another user's valid send is unaffected: {other:?}");

        let forged_again =
            submit_3337!(&app, &forged_transfer_3337(&victim_sk, &forger_sk, 4));
        assert_eq!(forged_again.0, 429, "a valid send does not refill the failure budget");
    }

    /// #3337: per-`from` budgets must not make a token unbounded. Distinct
    /// signers on one bearer are admitted up to the per-token ceiling, then
    /// refused — and a refused signer's nonce is not burned.
    #[actix_web::test]
    async fn distinct_signers_on_one_token_are_bounded_by_the_token_ceiling_3337() {
        let _env = hold_signing_domain_3337();
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let ceiling = crate::helpers::legacy_submit_token_max_requests();
        for i in 0..ceiling as u64 {
            let (sk, pk) = keypair(0x3337_1000 + i);
            let (status, body) = submit_3337!(&app, &signed_transfer_3337(&sk, &pk, 0));
            assert_eq!(status, 202, "signer {i} is within the token ceiling: {body:?}");
        }
        let (sk, pk) = keypair(0x3337_2000);
        let over = signed_transfer_3337(&sk, &pk, 0);
        let (status, body) = submit_3337!(&app, &over);
        assert_eq!(status, 429, "one past the token ceiling is refused: {body:?}");
        elapse_legacy_rate_window_3337(&state).await;
        let (status, body) = submit_3337!(&app, &over);
        assert_eq!(status, 202, "and is admitted at the same nonce later: {body:?}");
    }

    /// #3337 review P1: a signer over its OWN per-account budget is refused
    /// without charging the shared per-token ceiling, so its retry loop can
    /// never exhaust the relay token for other signers.
    #[actix_web::test]
    async fn a_signer_over_its_budget_does_not_drain_the_token_ceiling_3337() {
        let _env = hold_signing_domain_3337();
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (hot_sk, hot_pk) = keypair(0x3337_08);
        for nonce in 0..=1 {
            let (status, body) = submit_3337!(&app, &signed_transfer_3337(&hot_sk, &hot_pk, nonce));
            assert_eq!(status, 202, "{body:?}");
        }
        let retry = signed_transfer_3337(&hot_sk, &hot_pk, 2);
        for attempt in 0..crate::helpers::legacy_submit_token_max_requests() + 5 {
            let (status, _) = submit_3337!(&app, &retry);
            assert_eq!(status, 429, "retry {attempt} of the over-budget signer");
        }
        let (other_sk, other_pk) = keypair(0x3337_09);
        let (status, body) = submit_3337!(&app, &signed_transfer_3337(&other_sk, &other_pk, 0));
        assert_eq!(status, 202, "the shared token ceiling is untouched by refused retries: {body:?}");
    }

    /// #3337 review P1: the durable replay reservation happens only AFTER the
    /// authenticated rate limit admits the transfer, so a rate-limited retry
    /// loop costs no storage write; the replay REFUSAL before it is read-only,
    /// so a replay still spends no budget (#1846).
    #[::core::prelude::v1::test]
    fn legacy_submit_reserves_the_replay_key_only_after_the_rate_limit_3337() {
        let src = include_str!("routes/transactions.rs");
        let handler = &src[src
            .find("pub async fn submit_transaction_compat(")
            .expect("legacy handler")..];
        let handler = &handler[..handler
            .find("fn build_permissionless_signing_message(")
            .expect("handler end")];
        let at = |needle: &str| {
            handler
                .find(needle)
                .unwrap_or_else(|| panic!("{needle} missing from the legacy handler"))
        };
        let refuse = at("refuse_seen_permissionless_replay_key(&req, &state, &replay_key)");
        let limit = at("enforce_legacy_authenticated_submit_rate_limit(&req, &state, from)");
        let reserve = at("enforce_permissionless_replay_protection(&req, &state, &replay_key)");
        let enqueue = at("enqueue_protocol_transaction_with_nonce(");
        assert!(
            refuse < limit && limit < reserve && reserve < enqueue,
            "order must be: read-only replay refusal, rate limit, durable reservation, enqueue"
        );
    }

    /// #3258 review R3 — the unauthenticated permissionless lane records its
    /// replay key only after the signature verifies, so a junk-signature
    /// request naming a victim's public key and nonce cannot burn that nonce.
    #[actix_web::test]
    async fn junk_signature_does_not_burn_a_permissionless_nonce_3258() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (victim_sk, victim_pk) = keypair(0x3258_04);
        let (attacker_sk, _) = keypair(0x3258_05);
        let nonce = "nonce-victim-1";
        let timestamp = chrono::Utc::now().timestamp();
        let payload = json!({
            "from": "0x1111111111111111111111111111111111111111",
            "to": "0x2222222222222222222222222222222222222222",
            "amount": 5
        });
        let signing_message = format!(
            "permissionless-v1|{}|{}|{}|{}|{}",
            victim_pk,
            nonce,
            timestamp,
            "data_analysis",
            serde_json::to_string(&payload).expect("payload should serialize")
        );
        let body_with = |signature: String| {
            json!({
                "signer_public_key": victim_pk,
                "signature": signature,
                "nonce": nonce,
                "timestamp": timestamp,
                "tx_type": "data_analysis",
                "payload": payload
            })
        };

        let persisted = |state: &web::Data<AppState>| {
            let state = state.clone();
            async move {
                state
                    .storage
                    .read()
                    .await
                    .load_permissionless_replay()
                    .expect("read persisted replay map")
                    .len()
            }
        };
        assert!(state.permissionless_replay_seen.read().await.is_empty());
        assert_eq!(persisted(&state).await, 0);

        // Forged requests, no Authorization header: a well-formed signature by
        // someone else (parses, does not verify) and an all-zero signature.
        let junk = attacker_sk.sign(signing_message.as_bytes()).to_string();
        for signature in [junk, "00".repeat(64)] {
            let forged = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/transactions/permissionless/submit")
                    .set_json(body_with(signature))
                    .to_request(),
            )
            .await;
            assert_eq!(forged.status(), StatusCode::UNPROCESSABLE_ENTITY);
        }
        // #3306 acceptance, asserted after the genuine request below so the
        // old order's RED reports the burned nonce (409) first.
        let in_memory_after_forged = state.permissionless_replay_seen.read().await.len();
        let persisted_after_forged = persisted(&state).await;

        let genuine = victim_sk.sign(signing_message.as_bytes()).to_string();
        let accepted = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/permissionless/submit")
                .set_json(body_with(genuine))
                .to_request(),
        )
        .await;
        assert_eq!(
            accepted.status(),
            StatusCode::CREATED,
            "the victim's genuine request at the same nonce must be accepted"
        );
        // A forged request neither grows the in-memory replay map nor writes
        // it to storage.
        assert_eq!(in_memory_after_forged, 0, "forged requests must not add replay entries");
        assert_eq!(persisted_after_forged, 0, "forged requests must not persist the replay map");
        // Control: the verified request IS recorded and persisted, so the two
        // "unchanged" assertions above can observe a write when one happens.
        assert_eq!(state.permissionless_replay_seen.read().await.len(), 1);
        assert_eq!(persisted(&state).await, 1);
    }

/// #1882 regression — /transactions/submit without nonce must fail closed.
    #[actix_web::test]
    async fn legacy_submit_without_nonce_rejected_1882() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({
                    "from": "0x1111111111111111111111111111111111111111",
                    "to": "0x2222222222222222222222222222222222222222",
                    "amount": 10
                }))
                .to_request(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "legacy submit without nonce must be rejected before enqueue"
        );
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_error_envelope(&body);
    }

    #[actix_web::test]
    async fn transactions_submit_allows_explicit_transparent_opt_out() {
        use wrfcoin_native_chain::blockchain::transaction::transparent_transfer_authorization_message;

        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        // #1727/#1755: /transactions/submit now requires a verifiable
        // from-account authorization (auth_pubkey + auth_signature). Derive the
        // sender from a real keypair and sign the canonical auth message.
        let (from_sk, from_pk_hex) = keypair(0x1755_01);
        let from_pk = PublicKey::from_private_key(&from_sk);
        let from_addr = format!("0x{}", hex::encode(Address::from_public_key(&from_pk).0));
        let to_addr = "0x2222222222222222222222222222222222222222";
        let amount = 7u64;
        let nonce = 0u64;
        let auth_signature = from_sk
            .sign(&transparent_transfer_authorization_message(
                &from_addr, to_addr, amount, nonce,
            ))
            .to_string();
        let payload = json!({
            "from": from_addr,
            "to": to_addr,
            "amount": amount,
            "transparent_opt_out": true,
            "transparent_opt_out_reason": "regulatory-reporting",
            "auth_pubkey": from_pk_hex,
            "auth_signature": auth_signature,
            "nonce": nonce.to_string(),
        });

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(&payload)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_success_envelope(&body);
        assert_eq!(
            body["data"]["privacy_mode"].as_str(),
            Some("transparent_opt_out")
        );
        assert_eq!(body["data"]["transparent_opt_out"].as_bool(), Some(true));

        let mempool = state.mempool.read().await;
        let txs = mempool.get_all_transactions();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].tx_type, TransactionType::Transfer);
        let transparent_payload = txs[0]
            .parse_transparent_transfer_payload()
            .expect("transparent payload should decode");
        assert!(transparent_payload.transparent_opt_out);
        assert!(transparent_payload.opt_out_audit_tag.is_some());
    }

    #[actix_web::test]
    async fn mine_block_applies_explicit_transparent_opt_out_transfer() {
        // Signing and verification both read the process chain domain. Other
        // tests clear it during preflight, so run the whole round trip in a
        // child rather than racing those writers between signing and submit.
        // This harness explicitly declares no genesis oracles; preserve its
        // mainnet signing defaults rather than selecting the public testnet,
        // where a no-oracle block-one declaration is correctly refused.
        if crate::helpers::test_env::isolated_chain_domain(
            "tests::mine_block_applies_explicit_transparent_opt_out_transfer",
            "wrfcoin-main",
            "mainnet",
            "0x2727272727272727272727272727272727272727",
            "none",
        ) {
            return;
        }
        use wrfcoin_native_chain::blockchain::transaction::transparent_transfer_authorization_message;

        let state = test_state(None);
        // #1727/#1755: derive the sender from a keypair so the transparent
        // transfer carries a verifiable from-account authorization.
        let (from_sk, from_pk_hex) = keypair(0x1755_02);
        let from_pk = PublicKey::from_private_key(&from_sk);
        let from_addr = format!("0x{}", hex::encode(Address::from_public_key(&from_pk).0));
        let to_addr = "0x2222222222222222222222222222222222222222";
        {
            let mut blockchain = state.blockchain.write().await;
            blockchain
                .state
                .create_account(from_addr.parse::<Address>().unwrap(), Balance(100));
            blockchain
                .state
                .create_account(to_addr.parse::<Address>().unwrap(), Balance(1));
        }

        let app = test::init_service(test_app(state.clone())).await;
        let amount = 25u64;
        let nonce = 0u64;
        let auth_signature = from_sk
            .sign(&transparent_transfer_authorization_message(
                &from_addr, to_addr, amount, nonce,
            ))
            .to_string();
        let submit_payload = json!({
            "from": from_addr,
            "to": to_addr,
            "amount": amount,
            "transparent_opt_out": true,
            "auth_pubkey": from_pk_hex,
            "auth_signature": auth_signature,
            "nonce": nonce.to_string(),
        });
        let submit_resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(&submit_payload)
                .to_request(),
        )
        .await;
        assert_eq!(submit_resp.status(), StatusCode::ACCEPTED);

        let mine_resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({"miner_address": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}))
                .to_request(),
        )
        .await;
        assert_eq!(mine_resp.status(), StatusCode::CREATED);

        let blockchain = state.blockchain.read().await;
        let from = blockchain
            .state
            .get_account(&from_addr.parse::<Address>().unwrap())
            .expect("from account should exist");
        let to = blockchain
            .state
            .get_account(&to_addr.parse::<Address>().unwrap())
            .expect("to account should exist");
        assert_eq!(from.balance, 75);
        assert_eq!(to.balance, 26);
    }

    #[actix_web::test]
    async fn address_response_includes_nonce_zero_for_fresh_account() {
        let state = test_state(None);
        let address = Address::new([0x38; 20]);
        state
            .blockchain
            .write()
            .await
            .state
            .create_account(address, Balance(10));
        let app = test::init_service(test_app(state)).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/addresses/{address}"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["data"]["nonce"], "0");
    }

    #[actix_web::test]
    async fn address_nonce_increments_after_applied_transfer() {
        use wrfcoin_native_chain::blockchain::transaction::transparent_transfer_authorization_message;

        let state = test_state(None);
        let (from_sk, from_pk_hex) = keypair(0x3238);
        let from_pk = PublicKey::from_private_key(&from_sk);
        let from_address = Address::from_public_key(&from_pk);
        let from_text = format!("0x{}", hex::encode(from_address.0));
        let to_text = "0x2222222222222222222222222222222222222222";
        {
            let mut blockchain = state.blockchain.write().await;
            blockchain.state.create_account(from_address, Balance(100));
            blockchain
                .state
                .create_account(to_text.parse::<Address>().unwrap(), Balance(1));
        }
        let app = test::init_service(test_app(state)).await;
        let signature = from_sk
            .sign(&transparent_transfer_authorization_message(
                &from_text, to_text, 25, 0,
            ))
            .to_string();
        let submit = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/submit")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({
                    "from": from_text,
                    "to": to_text,
                    "amount": 25,
                    "transparent_opt_out": true,
                    "auth_pubkey": from_pk_hex,
                    "auth_signature": signature,
                    "nonce": "0",
                }))
                .to_request(),
        )
        .await;
        assert_eq!(submit.status(), StatusCode::ACCEPTED);

        let mine = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({"miner_address": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}))
                .to_request(),
        )
        .await;
        assert_eq!(mine.status(), StatusCode::CREATED);

        let address_resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/addresses/{from_text}"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(address_resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(address_resp).await;
        assert_eq!(body["data"]["nonce"], "1");
    }

    /// #3337 review hardening: a handler that stalls past the replay TTL
    /// must not free a key a LATER legitimate submission has re-reserved.
    /// Simulated without sleeping: reserve, then overwrite the stored entry
    /// with a later reservation's timestamp (what expiry + re-reservation
    /// leaves behind), then let the stalled handler release.
    #[actix_web::test]
    async fn a_stalled_release_does_not_free_a_later_reservation_3337() {
        let state = test_state(None);
        let req = test::TestRequest::post().to_http_request();
        let key = "legacy:0x1111111111111111111111111111111111111111:7";
        let reserved_at =
            crate::helpers::enforce_permissionless_replay_protection(&req, state.get_ref(), key)
                .await
                .expect("first reservation");
        let later = chrono::Utc::now().timestamp() + 700;
        state
            .permissionless_replay_seen
            .write()
            .await
            .insert(key.to_string(), later);

        crate::helpers::release_permissionless_replay_key(state.get_ref(), key, reserved_at)
            .await;

        assert_eq!(
            state.permissionless_replay_seen.read().await.get(key).copied(),
            Some(later),
            "the stalled handler's release must leave the later reservation in place"
        );
        // Control: the matching reservation IS released.
        crate::helpers::release_permissionless_replay_key(state.get_ref(), key, later).await;
        assert_eq!(state.permissionless_replay_seen.read().await.get(key), None);
    }

    // --- #3337 follow-up: the permissionless lane keeps unadmitted nonces --

    /// A signed permissionless submission of `payload` at `nonce`.
    fn permissionless_body_3337(
        sk: &PrivateKey,
        pk: &str,
        nonce: &str,
        payload: serde_json::Value,
    ) -> serde_json::Value {
        let timestamp = chrono::Utc::now().timestamp();
        let message = format!(
            "permissionless-v1|{pk}|{nonce}|{timestamp}|data_analysis|{}",
            serde_json::to_string(&payload).expect("payload serializes")
        );
        json!({
            "signer_public_key": pk,
            "signature": sk.sign(message.as_bytes()).to_string(),
            "nonce": nonce,
            "timestamp": timestamp,
            "tx_type": "data_analysis",
            "payload": payload,
        })
    }

    fn permissionless_payload_3337(amount: u64) -> serde_json::Value {
        json!({
            "from": "0x1111111111111111111111111111111111111111",
            "to": "0x2222222222222222222222222222222222222222",
            "amount": amount,
        })
    }

    macro_rules! permissionless_submit_3337 {
        ($app:expr, $body:expr) => {{
            let resp = test::call_service(
                $app,
                test::TestRequest::post()
                    .uri("/transactions/permissionless/submit")
                    .set_json($body)
                    .to_request(),
            )
            .await;
            let status = resp.status().as_u16();
            let bytes = test::read_body(resp).await;
            (status, String::from_utf8_lossy(&bytes).to_string())
        }};
    }

    /// #3337 follow-up: a verified permissionless submission that the signer
    /// throttle refuses (429) keeps its signer+nonce: the same signed body is
    /// admitted once the window allows. Before, the retry was 409.
    #[actix_web::test]
    async fn permissionless_rate_limited_submission_keeps_its_nonce_3337() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (sk, pk) = keypair(0x3337_21);
        for i in 0..PERMISSIONLESS_SUBMIT_MAX_REQUESTS {
            let body = permissionless_body_3337(&sk, &pk, &format!("n-{i}"), permissionless_payload_3337(5));
            let (status, text) = permissionless_submit_3337!(&app, &body);
            assert_eq!(status, 201, "submission {i} within the throttle: {text}");
        }
        let over = permissionless_body_3337(&sk, &pk, "n-over", permissionless_payload_3337(5));
        let limited = permissionless_submit_3337!(&app, &over);
        state.permissionless_submit_rate_limit.write().await.clear();
        let retry = permissionless_submit_3337!(&app, &over);
        assert_eq!(
            (limited.0, retry.0),
            (429, 201),
            "limited={limited:?} retry={retry:?}"
        );
    }

    /// #3337 follow-up: a verified submission the mempool refuses keeps its
    /// signer+nonce. Before, the retry was 409.
    #[actix_web::test]
    async fn permissionless_mempool_refused_submission_keeps_its_nonce_3337() {
        use wrfcoin_shared_protocol::consensus::partition_policy::PartitionMode;
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (sk, pk) = keypair(0x3337_22);
        let body = permissionless_body_3337(&sk, &pk, "n-0", permissionless_payload_3337(5));
        state.mempool.write().await.set_partition_mode(PartitionMode::ReadOnly);
        let refused = permissionless_submit_3337!(&app, &body);
        state.mempool.write().await.set_partition_mode(PartitionMode::Normal);
        let retry = permissionless_submit_3337!(&app, &body);
        assert_eq!((refused.0, retry.0), (400, 201), "refused={refused:?} retry={retry:?}");
    }

    /// #3337 follow-up: a verified submission whose payload is refused (422)
    /// never entered the mempool, so a corrected payload signed at the SAME
    /// nonce is admitted. Before, the corrected submission was 409.
    #[actix_web::test]
    async fn permissionless_invalid_payload_does_not_burn_its_nonce_3337() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (sk, pk) = keypair(0x3337_23);
        let bad = permissionless_body_3337(&sk, &pk, "n-0", permissionless_payload_3337(0));
        let good = permissionless_body_3337(&sk, &pk, "n-0", permissionless_payload_3337(5));
        let refused = permissionless_submit_3337!(&app, &bad);
        let corrected = permissionless_submit_3337!(&app, &good);
        assert_eq!(
            (refused.0, corrected.0),
            (422, 201),
            "refused={refused:?} corrected={corrected:?}"
        );
        let replay = permissionless_submit_3337!(&app, &good);
        assert_eq!(replay.0, 409, "an ADMITTED submission is still replay-protected");
    }

    /// #3337 follow-up: as on the legacy lane, the durable replay reservation
    /// happens only after the throttle and payload validation, and the replay
    /// REFUSAL before them is read-only, so a verified replay spends no
    /// budget and a throttled retry loop writes no storage.
    #[::core::prelude::v1::test]
    fn permissionless_submit_reserves_the_replay_key_only_after_admission_checks_3337() {
        let src = include_str!("routes/transactions.rs");
        let handler = &src[src
            .find("pub async fn submit_permissionless_transaction(")
            .expect("permissionless handler")..];
        let handler = &handler[..handler
            .find("pub async fn get_participation_policy(")
            .expect("handler end")];
        let at = |needle: &str| {
            handler
                .find(needle)
                .unwrap_or_else(|| panic!("{needle} missing from the permissionless handler"))
        };
        let refuse = at("refuse_seen_permissionless_replay_key(&req, &state, &replay_key)");
        let limit = at("enforce_permissionless_submit_rate_limit(&req, &state, &signer_fingerprint)");
        let validated = at("payload.from and payload.to must be valid hex addresses");
        let reserve = at("enforce_permissionless_replay_protection(&req, &state, &replay_key)");
        let submit = at("submit_protocol_transaction(");
        assert!(
            refuse < limit && limit < validated && validated < reserve && reserve < submit,
            "order must be: read-only replay refusal, throttle, payload validation, \
             durable reservation, submit"
        );
    }

    #[actix_web::test]
    async fn permissionless_lane_verifies_signature_and_replay_protection() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (signer_sk, signer_pk) = keypair(777);
        let nonce = "nonce-1";
        let timestamp = chrono::Utc::now().timestamp();
        let payload = json!({
            "from": "0x1111111111111111111111111111111111111111",
            "to": "0x2222222222222222222222222222222222222222",
            "amount": 5
        });
        let signing_message = format!(
            "permissionless-v1|{}|{}|{}|{}|{}",
            signer_pk,
            nonce,
            timestamp,
            "data_analysis",
            serde_json::to_string(&payload).expect("payload should serialize")
        );
        let signature = signer_sk.sign(signing_message.as_bytes()).to_string();

        let request_body = json!({
            "signer_public_key": signer_pk,
            "signature": signature,
            "nonce": nonce,
            "timestamp": timestamp,
            "tx_type": "data_analysis",
            "payload": payload
        });

        let first = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/permissionless/submit")
                .set_json(&request_body)
                .to_request(),
        )
        .await;
        assert_eq!(first.status(), StatusCode::CREATED);

        let replay = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/permissionless/submit")
                .set_json(&request_body)
                .to_request(),
        )
        .await;
        assert_eq!(replay.status(), StatusCode::CONFLICT);
    }

    #[actix_web::test]
    async fn permissionless_lane_rejects_invalid_signature() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (_signer_sk, signer_pk) = keypair(778);
        let payload = json!({
            "from": "0x1111111111111111111111111111111111111111",
            "to": "0x2222222222222222222222222222222222222222",
            "amount": 5
        });
        let body = json!({
            "signer_public_key": signer_pk,
            "signature": "deadbeef",
            "nonce": "nonce-invalid",
            "timestamp": chrono::Utc::now().timestamp(),
            "tx_type": "data_analysis",
            "payload": payload
        });

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/permissionless/submit")
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[actix_web::test]
    async fn participation_policy_endpoint_states_no_geofence_exclusion() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/participation/policy")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        let rules = body["data"]["human_readable_rules"]
            .as_array()
            .expect("rules should be present");
        let has_no_geofence_rule = rules.iter().any(|line| {
            line.as_str()
                .map(|text| text.to_ascii_lowercase().contains("geofence"))
                .unwrap_or(false)
        });
        assert!(
            has_no_geofence_rule,
            "policy should explicitly forbid geofence exclusion"
        );
    }

    /// core4#3239 (owner Option A): the permissionless submit route is a
    /// participation signal, not a transfer, so its success response must say
    /// it moves no funds. The transfer-shaped payload fields stay as they are.
    #[actix_web::test]
    async fn permissionless_submit_declares_no_fund_movement() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let (sk, pk) = keypair(0x3239_01);
        let body = permissionless_body_3337(&sk, &pk, "n-0", permissionless_payload_3337(5));
        let (status, text) = permissionless_submit_3337!(&app, &body);
        assert_eq!(status, 201, "{text}");
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("json body");
        assert_eq!(parsed["effect"], "none", "{text}");
        assert_eq!(parsed["moves_funds"], false, "{text}");
    }

    /// core4#3239: the participation policy says the permissionless route
    /// moves no funds.
    #[actix_web::test]
    async fn participation_policy_states_permissionless_submit_moves_no_funds() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/participation/policy")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        let rules = body["data"]["human_readable_rules"]
            .as_array()
            .expect("rules should be present");
        assert!(
            rules.iter().any(|line| line
                .as_str()
                .map(|text| text.to_ascii_lowercase().contains("moves no funds"))
                .unwrap_or(false)),
            "policy should say the permissionless submit moves no funds: {rules:?}"
        );
    }

    // -----------------------------------------------------------------------
    // NFT endpoint tests
    // -----------------------------------------------------------------------

    #[actix_web::test]
    async fn test_mint_nft() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let owner = "0x1111111111111111111111111111111111111111";

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/mint")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "id": "nft-001",
                    "owner": owner,
                    "metadata": nft_metadata_json(owner)
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[actix_web::test]
    async fn test_mint_nft_duplicate() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let owner = "0x1111111111111111111111111111111111111111";
        let body = json!({
            "id": "nft-dup",
            "owner": owner,
            "metadata": nft_metadata_json(owner)
        });

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/mint")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp2 = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/mint")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(&body)
                .to_request(),
        )
        .await;
        assert_eq!(resp2.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn test_get_nft() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let owner = "0x1111111111111111111111111111111111111111";

        // Mint first
        let _ = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/mint")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "id": "nft-get-001",
                    "owner": owner,
                    "metadata": nft_metadata_json(owner)
                }))
                .to_request(),
        )
        .await;

        // GET the NFT. #1986 put NFT reads behind Reader auth.
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/nft/nft-get-001")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["data"]["id"], "nft-get-001");
    }

    #[actix_web::test]
    async fn test_get_nft_not_found() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/nft/nonexistent-nft")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn test_list_nfts() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let owner = "0x1111111111111111111111111111111111111111";

        for id in &["nft-list-a", "nft-list-b"] {
            let mint_resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/nft/mint")
                    .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                    .set_json(json!({
                        "id": id,
                        "owner": owner,
                        "metadata": nft_metadata_json(owner)
                    }))
                    .to_request(),
            )
            .await;
            assert_eq!(mint_resp.status(), StatusCode::CREATED);
        }

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/nft/list")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let nfts = body["items"].as_array().expect("items should be an array");
        assert_eq!(nfts.len(), 2);
    }

    #[actix_web::test]
    async fn test_list_nfts_filter_by_owner() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let owner_a = "0x1111111111111111111111111111111111111111";
        let owner_b = "0x2222222222222222222222222222222222222222";

        let mint_a = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/mint")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "id": "nft-owner-a",
                    "owner": owner_a,
                    "metadata": nft_metadata_json(owner_a)
                }))
                .to_request(),
        )
        .await;
        assert_eq!(mint_a.status(), StatusCode::CREATED);

        let mint_b = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/mint")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "id": "nft-owner-b",
                    "owner": owner_b,
                    "metadata": nft_metadata_json(owner_b)
                }))
                .to_request(),
        )
        .await;
        assert_eq!(mint_b.status(), StatusCode::CREATED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/nft/list?owner={owner_a}"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let nfts = body["items"].as_array().expect("items should be an array");
        assert_eq!(nfts.len(), 1);
    }

    #[actix_web::test]
    async fn test_verify_nft() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let owner = "0x1111111111111111111111111111111111111111";
        let verifier = "0x3333333333333333333333333333333333333333";

        // Mint first
        let _ = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/mint")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "id": "nft-verify-001",
                    "owner": owner,
                    "metadata": nft_metadata_json(owner)
                }))
                .to_request(),
        )
        .await;

        // Verify
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/nft-verify-001/verify")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({
                    "verifier": verifier,
                    "confidence": 0.85,
                    "result": true
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn test_transfer_nft() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let owner = "0x1111111111111111111111111111111111111111";
        let new_owner = "0x4444444444444444444444444444444444444444";

        // Mint first
        let _ = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/mint")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "id": "nft-transfer-001",
                    "owner": owner,
                    "metadata": nft_metadata_json(owner)
                }))
                .to_request(),
        )
        .await;

        // Transfer
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/nft-transfer-001/transfer")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "to": new_owner }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        // Confirm new owner via GET
        let resp2 = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/nft/nft-transfer-001")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp2.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp2).await;
        let expected_owner = serde_json::to_value(new_owner.parse::<Address>().unwrap()).unwrap();
        assert_eq!(body["data"]["owner"], expected_owner);
    }

    #[actix_web::test]
    async fn test_transfer_nft_not_found() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let owner_a = "0x1111111111111111111111111111111111111111";
        let owner_b = "0x2222222222222222222222222222222222222222";

        // Mint with owner_a
        let _ = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/mint")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "id": "nft-wrong-owner",
                    "owner": owner_a,
                    "metadata": nft_metadata_json(owner_a)
                }))
                .to_request(),
        )
        .await;

        // Transfer looks up current owner internally, so the from is always correct.
        // The "wrong owner" scenario means trying to transfer an NFT that doesn't exist
        // or the state transition itself rejects. Since transfer_nft reads the current
        // owner from state, the only way to get NotNFTOwner is if the NFT was concurrently
        // modified. Instead, let's test transferring a non-existent NFT.
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/nft-does-not-exist/transfer")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "to": owner_b }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn test_mint_nft_unauthorized() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let owner = "0x1111111111111111111111111111111111111111";

        // Reader token should not be allowed to mint
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/mint")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .set_json(json!({
                    "id": "nft-unauth",
                    "owner": owner,
                    "metadata": nft_metadata_json(owner)
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // -----------------------------------------------------------------------
    // NFT mutation gating — state-root divergence guard (#1764)
    //
    // NFT state is hashed into the consensus state_root, but NFT mutations are
    // applied only via these node-local, non-gossiped endpoints (dropped from
    // block extraction) with wall-clock timestamps. A single mutation therefore
    // forks the producer's state_root away from every applier that never saw the
    // call, tripping the P2P divergence latch. These tests prove the default
    // build refuses every NFT-mutation endpoint *before* touching state, so the
    // `nfts` map — and thus the consensus state_root material — cannot change.
    // -----------------------------------------------------------------------

    /// A test `AppState` with the NFT mutation API disabled (the production
    /// default). Reuses `test_state` and flips the single gate flag, mirroring
    /// the clone-and-override pattern used by `test_state_with_p2p`.
    fn test_state_nft_mutations_disabled(
        snapshot_signing_key: Option<PrivateKey>,
    ) -> web::Data<AppState> {
        let state_data = test_state(snapshot_signing_key);
        let mut state = (*state_data.get_ref()).clone();
        state.nft_mutation_api_enabled = false;
        web::Data::new(state)
    }

    #[actix_web::test]
    async fn nft_mint_disabled_returns_503_and_leaves_state_root_unchanged() {
        let state = test_state_nft_mutations_disabled(None);
        let app = test::init_service(test_app(state.clone())).await;
        let owner = "0x1111111111111111111111111111111111111111";

        let root_before = state.blockchain.read().await.state.state_root_material();
        let nfts_before = state.blockchain.read().await.state.nfts.len();

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/mint")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "id": "nft-gated-001",
                    "owner": owner,
                    "metadata": nft_metadata_json(owner)
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        let blockchain = state.blockchain.read().await;
        // No NFT was added and the consensus state_root material is byte-identical:
        // a disabled-node mint cannot diverge from a peer that never saw the call.
        assert_eq!(blockchain.state.nfts.len(), nfts_before);
        assert_eq!(blockchain.state.state_root_material(), root_before);
    }

    /// #1775: oracle/signing-key admin routes default OFF. Clone test_state and
    /// flip only that gate (mirrors `test_state_nft_mutations_disabled`).
    fn test_state_oracle_admin_disabled(
        snapshot_signing_key: Option<PrivateKey>,
    ) -> web::Data<AppState> {
        let state_data = test_state(snapshot_signing_key);
        let mut state = (*state_data.get_ref()).clone();
        state.oracle_admin_api_enabled = false;
        web::Data::new(state)
    }

    #[actix_web::test]
    async fn oracle_config_disabled_returns_503_and_leaves_state_root_unchanged() {
        let state = test_state_oracle_admin_disabled(None);
        let app = test::init_service(test_app(state.clone())).await;

        let root_before = state.blockchain.read().await.state.state_root_material();

        let resp = test::call_service(
            &app,
            test::TestRequest::put()
                .uri("/transactions/oracle/config")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "oracle_set": ["0x1111111111111111111111111111111111111111"],
                    "oracle_quorum": 1
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        // Out-of-band oracle config was refused: state_root material byte-identical,
        // so a disabled node cannot fork from a peer that never saw the call (#1775).
        assert_eq!(
            state.blockchain.read().await.state.state_root_material(),
            root_before
        );
    }

    #[actix_web::test]
    async fn signing_key_register_disabled_returns_503_and_leaves_state_root_unchanged() {
        let state = test_state_oracle_admin_disabled(None);
        let app = test::init_service(test_app(state.clone())).await;

        let root_before = state.blockchain.read().await.state.state_root_material();

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/signing-keys/register")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "identity": "oracle-gated",
                    "role": "oracle",
                    "epoch": 0,
                    "public_key": "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
                    "activated_at": 0
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            state.blockchain.read().await.state.state_root_material(),
            root_before
        );
    }

    #[actix_web::test]
    async fn nft_transfer_disabled_returns_503_and_leaves_state_root_unchanged() {
        let state = test_state_nft_mutations_disabled(None);
        let app = test::init_service(test_app(state.clone())).await;

        let root_before = state.blockchain.read().await.state.state_root_material();

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/nft-anything/transfer")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "to": "0x2222222222222222222222222222222222222222"
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            state.blockchain.read().await.state.state_root_material(),
            root_before
        );
    }

    #[actix_web::test]
    async fn nft_verify_disabled_returns_503_and_leaves_state_root_unchanged() {
        let state = test_state_nft_mutations_disabled(None);
        let app = test::init_service(test_app(state.clone())).await;

        let root_before = state.blockchain.read().await.state.state_root_material();

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/nft-anything/verify")
                .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                .set_json(json!({
                    "verifier": "0x3333333333333333333333333333333333333333",
                    "confidence": 0.9,
                    "result": true
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            state.blockchain.read().await.state.state_root_material(),
            root_before
        );
    }

    /// Read endpoints stay available even when the mutation API is disabled.
    #[actix_web::test]
    async fn nft_read_endpoints_available_when_mutations_disabled() {
        let state = test_state_nft_mutations_disabled(None);
        let app = test::init_service(test_app(state.clone())).await;

        // #1986: NFT reads require Reader auth; the mutation gate is orthogonal.
        let list_resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/nft/list")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(list_resp.status(), StatusCode::OK);

        let get_resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/nft/nonexistent")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        // 404 (not 503): the read path runs; the NFT simply does not exist.
        assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
    }

    // `actix_web::test` is imported into this module, shadowing the built-in
    // `#[test]` attribute, so the synchronous unit test below must qualify it.
    #[::core::prelude::v1::test]
    fn nft_mutation_api_env_parsing_defaults_disabled() {
        // Default (unset) is disabled — the safe multi-node testnet posture.
        assert!(!nft_mutation_api_enabled_from_value(None));
        // Only explicit opt-in values enable it.
        assert!(nft_mutation_api_enabled_from_value(Some("1")));
        assert!(nft_mutation_api_enabled_from_value(Some("true")));
        assert!(nft_mutation_api_enabled_from_value(Some("TRUE")));
        assert!(nft_mutation_api_enabled_from_value(Some(" true ")));
        // Anything else stays disabled.
        assert!(!nft_mutation_api_enabled_from_value(Some("0")));
        assert!(!nft_mutation_api_enabled_from_value(Some("false")));
        assert!(!nft_mutation_api_enabled_from_value(Some("yes")));
        assert!(!nft_mutation_api_enabled_from_value(Some("")));
    }

    // -----------------------------------------------------------------------
    // Live-network guard tests (#1856)
    //
    // These routes call apply_state_transition() directly — bypassing block
    // production and gossip — so they are only safe at genesis (height 0).
    // After the first block is mined the guard must return 409 CONFLICT and
    // leave the state_root material byte-identical to what it was before.
    // -----------------------------------------------------------------------

    #[actix_web::test]
    async fn nft_mint_at_height_1_returns_409_and_leaves_state_root_unchanged() {
        let mut base = (*test_state(None).get_ref()).clone();
        base.nft_mutation_api_enabled = true;
        let state = web::Data::new(base);
        let app = test::init_service(test_app(state.clone())).await;

        // Advance chain to height 1.
        let mine = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                .to_request(),
        )
        .await;
        assert_eq!(mine.status(), StatusCode::CREATED, "test setup: mine must succeed");

        let root_before = state.blockchain.read().await.state.state_root_material();
        let owner = "0x1111111111111111111111111111111111111111";

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/nft/mint")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "id": "nft-live-guard-001",
                    "owner": owner,
                    "metadata": nft_metadata_json(owner)
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CONFLICT, "#1856: live-network mint must be rejected");
        assert_eq!(
            state.blockchain.read().await.state.state_root_material(),
            root_before,
            "#1856: state_root must not change when live-network guard fires"
        );
    }

    #[actix_web::test]
    async fn oracle_config_update_at_height_1_returns_409_and_leaves_state_root_unchanged() {
        let mut base = (*test_state(None).get_ref()).clone();
        base.oracle_admin_api_enabled = true;
        let state = web::Data::new(base);
        let app = test::init_service(test_app(state.clone())).await;

        // Advance chain to height 1.
        let mine = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                .to_request(),
        )
        .await;
        assert_eq!(mine.status(), StatusCode::CREATED, "test setup: mine must succeed");

        let root_before = state.blockchain.read().await.state.state_root_material();

        let resp = test::call_service(
            &app,
            test::TestRequest::put()
                .uri("/transactions/oracle/config")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "oracle_set": ["0x1111111111111111111111111111111111111111"],
                    "oracle_quorum": 1
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CONFLICT, "#1856: live-network oracle config update must be rejected");
        assert_eq!(
            state.blockchain.read().await.state.state_root_material(),
            root_before,
            "#1856: state_root must not change when live-network guard fires"
        );
    }

    #[actix_web::test]
    async fn signing_key_register_at_height_1_returns_409_and_leaves_state_root_unchanged() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        // Advance chain to height 1.
        let mine = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                .to_request(),
        )
        .await;
        assert_eq!(mine.status(), StatusCode::CREATED, "test setup: mine must succeed");

        let root_before = state.blockchain.read().await.state.state_root_material();

        // Use a real keypair so public_key validation passes before the height guard.
        let (_sk, oracle_pk) = keypair(201);
        let now_ts = chrono::Utc::now().timestamp();
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/signing-keys/register")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({
                    "identity": oracle_pk,
                    "role": "oracle",
                    "epoch": 1,
                    "public_key": oracle_pk,
                    "activated_at": now_ts
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CONFLICT, "#1856: live-network signing key register must be rejected");
        assert_eq!(
            state.blockchain.read().await.state.state_root_material(),
            root_before,
            "#1856: state_root must not change when live-network guard fires"
        );
    }

    // -----------------------------------------------------------------------
    // Privacy API auth + key_id tests (issues #452 & #453)
    // -----------------------------------------------------------------------

    #[cfg(feature = "privacy")]
    mod privacy_api_tests {
        use super::*;

        /// Privacy POST endpoints paired with minimally valid JSON bodies.
        fn privacy_post_endpoints_with_bodies() -> Vec<(&'static str, serde_json::Value)> {
            vec![
                ("/privacy/stealth-keys/generate", json!({})),
                (
                    "/privacy/stealth-address/create",
                    json!({
                        "spend_key": "aa".repeat(32),
                        "view_key": "bb".repeat(32)
                    }),
                ),
                (
                    "/privacy/stealth-address/check",
                    json!({
                        "one_time_address": "aa".repeat(32),
                        "ephemeral_public_key": "bb".repeat(32),
                        "private_view_key": "cc".repeat(32),
                        "public_spend_key": "dd".repeat(32)
                    }),
                ),
                ("/privacy/commitment/create", json!({ "value": 100 })),
                (
                    "/privacy/commitment/verify",
                    json!({
                        "commitment": "aa".repeat(32),
                        "blinding_factor": "bb".repeat(32),
                        "value": 100
                    }),
                ),
                (
                    "/privacy/range-proof/create",
                    json!({
                        "value": 100,
                        "blinding_factor": "aa".repeat(32)
                    }),
                ),
                (
                    "/privacy/range-proof/verify",
                    json!({
                        "proof_json": "{}",
                        "commitment_hex": "aa".repeat(32)
                    }),
                ),
                (
                    "/privacy/ring-signature/create",
                    json!({
                        "message": "test",
                        "key_id": "pk-test",
                        "decoy_public_keys": []
                    }),
                ),
                (
                    "/privacy/ring-signature/verify",
                    json!({
                        "signature_json": "{}",
                        "message": "test"
                    }),
                ),
                (
                    "/privacy/payload/encrypt",
                    json!({
                        "plaintext": "hello",
                        "recipient_public_key": "aa".repeat(32)
                    }),
                ),
                (
                    "/privacy/payload/decrypt",
                    json!({
                        "payload_json": "{}",
                        "key_id": "pk-test"
                    }),
                ),
                (
                    "/privacy/keys/register",
                    json!({
                        "private_key": "aa".repeat(32)
                    }),
                ),
            ]
        }

        #[actix_web::test]
        async fn privacy_endpoints_reject_unauthenticated_requests() {
            let state = test_state(None);
            let app = test::init_service(test_app(state.clone())).await;

            for (endpoint, body) in privacy_post_endpoints_with_bodies() {
                let resp = test::call_service(
                    &app,
                    test::TestRequest::post()
                        .uri(endpoint)
                        .set_json(&body)
                        .to_request(),
                )
                .await;
                assert_eq!(
                    resp.status(),
                    StatusCode::UNAUTHORIZED,
                    "expected 401 for unauthenticated POST to {endpoint}",
                );
            }

            // GET /privacy/key-images should also require auth (Reader role).
            let resp = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri("/privacy/key-images")
                    .to_request(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        #[actix_web::test]
        async fn privacy_endpoints_reject_invalid_tokens() {
            let state = test_state(None);
            let app = test::init_service(test_app(state.clone())).await;

            for (endpoint, body) in privacy_post_endpoints_with_bodies() {
                let resp = test::call_service(
                    &app,
                    test::TestRequest::post()
                        .uri(endpoint)
                        .insert_header(("Authorization", "Bearer bad-token-999"))
                        .set_json(&body)
                        .to_request(),
                )
                .await;
                assert_eq!(
                    resp.status(),
                    StatusCode::FORBIDDEN,
                    "expected 403 for invalid token POST to {endpoint}",
                );
            }
        }

        #[actix_web::test]
        async fn privacy_stealth_keys_generate_succeeds_with_auth() {
            let state = test_state(None);
            let app = test::init_service(test_app(state.clone())).await;

            let resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/privacy/stealth-keys/generate")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                    .set_json(json!({}))
                    .to_request(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::OK);
        }

        #[actix_web::test]
        async fn privacy_key_register_returns_key_id() {
            let state = test_state(None);
            let app = test::init_service(test_app(state.clone())).await;

            let fake_key = "aa".repeat(32);
            let resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/privacy/keys/register")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                    .set_json(json!({ "private_key": fake_key }))
                    .to_request(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::OK);

            let body: serde_json::Value = test::read_body_json(resp).await;
            let key_id = body["key_id"].as_str().expect("key_id should be a string");
            assert!(key_id.starts_with("pk-"), "key_id should start with pk-");
            assert!(
                body["expires_at"].is_number(),
                "expires_at should be present"
            );
        }

        #[actix_web::test]
        async fn ring_signature_create_uses_key_id_not_raw_private_key() {
            let state = test_state(None);
            let app = test::init_service(test_app(state.clone())).await;

            // First register a key.
            let fake_key = "bb".repeat(32);
            let register_resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/privacy/keys/register")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                    .set_json(json!({ "private_key": fake_key }))
                    .to_request(),
            )
            .await;
            assert_eq!(register_resp.status(), StatusCode::OK);
            let body: serde_json::Value = test::read_body_json(register_resp).await;
            let key_id = body["key_id"]
                .as_str()
                .expect("register response should contain key_id")
                .to_string();

            // Attempt ring signature creation with key_id.
            let resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/privacy/ring-signature/create")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                    .set_json(json!({
                        "message": "test-message",
                        "key_id": key_id,
                        "decoy_public_keys": []
                    }))
                    .to_request(),
            )
            .await;
            assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
            assert_ne!(resp.status(), StatusCode::FORBIDDEN);
            assert_ne!(resp.status(), StatusCode::NOT_FOUND);
        }

        #[actix_web::test]
        async fn ring_signature_create_rejects_unknown_key_id() {
            let state = test_state(None);
            let app = test::init_service(test_app(state.clone())).await;

            let resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/privacy/ring-signature/create")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                    .set_json(json!({
                        "message": "test-message",
                        "key_id": "pk-nonexistent",
                        "decoy_public_keys": []
                    }))
                    .to_request(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        }

        #[actix_web::test]
        async fn ring_signature_api_signs_binary_canonical_preimages_2121() {
            let state = test_state(None);
            let app = test::init_service(test_app(state.clone())).await;
            let private_key = [7u8; 32];
            let decoy_public_keys = (1u64..=6)
                .map(|value| {
                    let scalar = wrfcoin_privacy::Scalar::from(value);
                    hex::encode(wrfcoin_privacy::scalar_to_public_key(&scalar))
                })
                .collect::<Vec<_>>();

            let register_resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/privacy/keys/register")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                    .set_json(json!({ "private_key": hex::encode(private_key) }))
                    .to_request(),
            )
            .await;
            assert_eq!(register_resp.status(), StatusCode::OK);
            let register_body: serde_json::Value = test::read_body_json(register_resp).await;
            let key_id = register_body["key_id"].as_str().expect("registered key id");

            // Includes non-UTF-8 bytes: the production route must sign the
            // canonical binary preimage exactly, without a text conversion.
            let message = [0xff, 0x00, 0x80, 0x01, 0x7f];
            let create_resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/privacy/ring-signature/create")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                    .set_json(json!({
                        "message_hex": hex::encode(message),
                        "key_id": key_id,
                        "decoy_public_keys": decoy_public_keys,
                    }))
                    .to_request(),
            )
            .await;
            assert_eq!(create_resp.status(), StatusCode::OK);
            let create_body: serde_json::Value = test::read_body_json(create_resp).await;
            assert_eq!(create_body["signed_message"], hex::encode(message));
            let signature_json = create_body["signature"]
                .as_str()
                .expect("serialized ring signature");

            let verify_resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/privacy/ring-signature/verify")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                    .set_json(json!({
                        "signature_json": signature_json,
                        "message_hex": hex::encode(message),
                    }))
                    .to_request(),
            )
            .await;
            assert_eq!(verify_resp.status(), StatusCode::OK);
            let verify_body: serde_json::Value = test::read_body_json(verify_resp).await;
            assert_eq!(verify_body["valid"], true);
        }

        #[actix_web::test]
        async fn payload_decrypt_uses_key_id() {
            let state = test_state(None);
            let app = test::init_service(test_app(state.clone())).await;

            // Register a key.
            let fake_key = "cc".repeat(32);
            let register_resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/privacy/keys/register")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                    .set_json(json!({ "private_key": fake_key }))
                    .to_request(),
            )
            .await;
            let body: serde_json::Value = test::read_body_json(register_resp).await;
            let key_id = body["key_id"]
                .as_str()
                .expect("register response should contain key_id")
                .to_string();

            let resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/privacy/payload/decrypt")
                    .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
                    .set_json(json!({
                        "payload_json": "{}",
                        "key_id": key_id
                    }))
                    .to_request(),
            )
            .await;
            // Should resolve key_id but fail on invalid payload — 400 not 404.
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        }

        #[actix_web::test]
        async fn key_images_endpoint_requires_reader_auth() {
            let state = test_state(None);
            let app = test::init_service(test_app(state.clone())).await;

            let resp = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri("/privacy/key-images")
                    .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                    .to_request(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }

    #[actix_web::test]
    async fn ntp_check_returns_result() {
        let result = helpers::check_ntp_sync();
        assert!(
            ["timedatectl", "timesync-file", "skipped", "unavailable"].contains(&result.source),
            "unexpected NTP source: {}",
            result.source
        );
    }

    // ── Health/version/readyz auth gating tests (#911) ───────────────

    #[actix_web::test]
    async fn health_unauthenticated_returns_minimal() {
        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let req = test::TestRequest::get().uri("/health").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "ok");
        assert!(
            body.get("version").is_none(),
            "version must not be disclosed"
        );
        assert!(
            body.get("uptime_seconds").is_none(),
            "uptime must not be disclosed"
        );
        assert!(
            body.get("service").is_none(),
            "service must not be disclosed"
        );
    }

    #[actix_web::test]
    async fn health_operator_returns_full() {
        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let req = test::TestRequest::get()
            .uri("/health")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "ok");
        assert!(body.get("version").is_some(), "operator should see version");
        assert!(
            body.get("uptime_seconds").is_some(),
            "operator should see uptime"
        );
        assert!(body.get("service").is_some(), "operator should see service");
    }

    #[actix_web::test]
    async fn health_localhost_returns_full() {
        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let req = test::TestRequest::get()
            .uri("/health")
            .peer_addr("127.0.0.1:12345".parse().unwrap())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert!(
            body.get("version").is_some(),
            "localhost should see version"
        );
    }

    #[actix_web::test]
    async fn version_unauthenticated_returns_401() {
        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let req = test::TestRequest::get().uri("/version").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn version_contributor_returns_403() {
        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let req = test::TestRequest::get()
            .uri("/version")
            .insert_header(("Authorization", format!("Bearer {TEST_CONTRIBUTOR_TOKEN}")))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn version_operator_returns_full() {
        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let req = test::TestRequest::get()
            .uri("/version")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert!(body.get("version").is_some());
        assert!(body.get("service").is_some());
    }

    #[actix_web::test]
    async fn layer2_submit_batch_unauthenticated_returns_401() {
        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let req = test::TestRequest::post()
            .uri("/layer2/submit-batch")
            .set_json(&Layer2BatchSubmitPayload {
                batch_id: 42,
                prev_state_root: "prev-state-root".to_string(),
                state_root: "next-state-root".to_string(),
                proof_hex: "00".to_string(),
                commitment_hex: "11".to_string(),
                operator: "operator-a".to_string(),
            })
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn readyz_unauthenticated_returns_minimal() {
        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let req = test::TestRequest::get().uri("/readyz").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "ready");
        assert!(body.get("checks").is_none(), "checks must not be disclosed");
        assert!(
            body.get("uptime_seconds").is_none(),
            "uptime must not be disclosed"
        );
    }

    #[actix_web::test]
    async fn readyz_operator_returns_full() {
        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let req = test::TestRequest::get()
            .uri("/readyz")
            .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "ready");
        assert!(body.get("checks").is_some(), "operator should see checks");
        assert!(
            body.get("uptime_seconds").is_some(),
            "operator should see uptime"
        );
    }

    async fn seed_block_with_timestamp(
        state: &web::Data<AppState>,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) {
        let mut blockchain = state.blockchain.write().await;
        let mut block = Block::new(3, blockchain.tip, Vec::new(), 1, blockchain.height + 1);
        block.timestamp = timestamp;
        let hash = block.calculate_hash();
        block.set_hash(hash);
        blockchain
            .add_block(block)
            .expect("seed block should be added to chain");
    }

    fn parse_response_timestamp(value: &serde_json::Value) -> chrono::DateTime<chrono::FixedOffset> {
        chrono::DateTime::parse_from_rfc3339(
            value
                .as_str()
                .expect("timestamp should be serialized as RFC3339 string"),
        )
        .expect("timestamp should parse as RFC3339")
    }

    fn assert_chain_block_summary_matches_block(summary: &serde_json::Value, block: &Block) {
        let expected_hash = block
            .hash
            .expect("stored block should have a finalized hash")
            .to_string();
        let expected_prev_hash = block.prev_block_hash.to_string();

        assert_eq!(summary["height"].as_u64(), Some(block.height));
        assert_eq!(summary["hash"].as_str(), Some(expected_hash.as_str()));
        assert_eq!(summary["prev_block_hash"].as_str(), Some(expected_prev_hash.as_str()));
        assert_eq!(
            parse_response_timestamp(&summary["timestamp"]).with_timezone(&chrono::Utc),
            block.timestamp,
        );
        assert_eq!(summary["tx_count"].as_u64(), Some(block.transactions.len() as u64));
    }

    // ── Node introspection endpoints (#985) ─────────────────────────────

    #[actix_web::test]
    async fn network_info_returns_expected_fields() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get().uri("/network/info").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert!(body["node_version"].is_string());
        assert!(body["sync_status"].is_string());
        assert!(body["chain_height"].is_number());
        // Without P2P, peer_count should be 0.
        assert_eq!(body["peer_count"].as_u64().unwrap_or(u64::MAX), 0);
        // Empty chain → genesis.
        assert_eq!(body["sync_status"].as_str().unwrap_or(""), "genesis");
    }

    #[actix_web::test]
    async fn network_info_keeps_healthy_testnet_cadence_synced() {
        let state = test_state(None);
        {
            let mut blockchain = state.blockchain.write().await;
            let mut recent_block = Block::new(1, Hash::default(), Vec::new(), 1, 1);
            recent_block.timestamp = chrono::Utc::now() - chrono::Duration::seconds(301);
            recent_block.set_hash(recent_block.calculate_hash());
            blockchain
                .add_block(recent_block)
                .expect("recent test block should be added");
        }
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get().uri("/network/info").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(
            body["sync_status"].as_str().unwrap_or(""),
            "synced",
            "N5 Pro study cadence shows healthy blocks can arrive slightly over 5 minutes apart",
        );
    }

    #[actix_web::test]
    async fn node_sync_status_keeps_healthy_testnet_cadence_synced() {
        let state = test_state(None);
        {
            let mut blockchain = state.blockchain.write().await;
            let mut recent_block = Block::new(1, Hash::default(), Vec::new(), 1, 1);
            recent_block.timestamp = chrono::Utc::now() - chrono::Duration::seconds(301);
            recent_block.set_hash(recent_block.calculate_hash());
            blockchain
                .add_block(recent_block)
                .expect("recent test block should be added");
        }
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/node/sync/status")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["sync_status"].as_str().unwrap_or(""), "synced");
        assert!(
            body["block_age_secs"].as_i64().unwrap_or_default() >= 301,
            "block age should reflect the slower-but-healthy cadence under test",
        );
    }

    #[actix_web::test]
    async fn chain_blocks_empty_chain_returns_empty_array() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get().uri("/chain/blocks").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert!(body["ok"].as_bool().unwrap_or(false));
        let data = &body["data"];
        assert_eq!(data["chain_height"].as_u64().unwrap_or(u64::MAX), 0);
        assert!(
            data["blocks"]
                .as_array()
                .expect("blocks should be array")
                .is_empty()
        );
    }

    #[actix_web::test]
    async fn chain_blocks_returns_mined_blocks() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        // Mine a block so the chain is non-empty.
        let mine_resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                .to_request(),
        )
        .await;
        assert_eq!(mine_resp.status(), StatusCode::CREATED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/blocks?limit=5")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let blocks = body["data"]["blocks"]
            .as_array()
            .expect("blocks should be array");
        assert!(!blocks.is_empty());
        let first = &blocks[0];
        assert!(first["height"].is_number());
        assert!(first["timestamp"].is_string());
        assert!(first["tx_count"].is_number());
    }

    #[actix_web::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn chain_blocks_timestamps_do_not_regress_across_adjacent_heights() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let seeded_timestamp = chrono::DateTime::from_timestamp(
            (chrono::Utc::now() + chrono::Duration::seconds(5)).timestamp(),
            0,
        )
        .expect("test timestamp should be in range");
        seed_block_with_timestamp(&state, seeded_timestamp).await;

        let mine_resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                .to_request(),
        )
        .await;
        assert_eq!(mine_resp.status(), StatusCode::CREATED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/blocks?limit=2")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let blocks = body["data"]["blocks"]
            .as_array()
            .expect("blocks should be array");
        assert_eq!(blocks.len(), 2, "expected the seeded block and the mined block");
        assert_eq!(blocks[0]["height"].as_u64(), Some(1));
        assert_eq!(blocks[1]["height"].as_u64(), Some(2));
        assert!(blocks[0]["hash"].is_string());
        assert!(blocks[0]["prev_block_hash"].is_string());
        assert!(blocks[1]["hash"].is_string());
        assert!(blocks[1]["prev_block_hash"].is_string());
        assert!(blocks[1]["tx_count"].is_number());

        let first_timestamp = parse_response_timestamp(&blocks[0]["timestamp"]);
        let second_timestamp = parse_response_timestamp(&blocks[1]["timestamp"]);
        assert!(
            second_timestamp > first_timestamp,
            "adjacent block timestamps must be strictly increasing: {first_timestamp} then {second_timestamp}"
        );
    }

    #[actix_web::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn chain_blocks_returns_persisted_blocks_when_older_heights_are_evicted() {
        let state = test_state_with_block_capacity(None, 1);
        let app = test::init_service(test_app(state.clone())).await;

        for _ in 0..2 {
            let mine_resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/blocks/mine")
                    .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                    .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                    .to_request(),
            )
            .await;
            assert_eq!(mine_resp.status(), StatusCode::CREATED);
        }

        {
            let blockchain = state.blockchain.read().await;
            assert_eq!(blockchain.height, 2);
            assert_eq!(blockchain.blocks().len(), 1, "height 1 should be evicted from memory");
            assert!(blockchain.block_at_height(1).is_none());
            assert!(blockchain.block_at_height(2).is_some());
        }

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/blocks?limit=2")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let blocks = body["data"]["blocks"]
            .as_array()
            .expect("blocks should be array");
        assert_eq!(body["data"]["chain_height"].as_u64(), Some(2));
        assert_eq!(body["data"]["returned"].as_u64(), Some(2));
        assert_eq!(blocks.len(), 2, "API should fall back to persisted blocks");
        assert_eq!(blocks[0]["height"].as_u64(), Some(1));
        assert_eq!(blocks[1]["height"].as_u64(), Some(2));
        assert!(blocks[0]["timestamp"].is_string());
        assert!(blocks[1]["timestamp"].is_string());
        assert!(blocks[0]["hash"].is_string());
        assert!(blocks[1]["hash"].is_string());
        assert!(blocks[0]["prev_block_hash"].is_string());
        assert!(blocks[1]["prev_block_hash"].is_string());
        assert!(blocks[0]["tx_count"].is_number());
        assert!(blocks[1]["tx_count"].is_number());
    }

    #[actix_web::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn chain_blocks_return_wider_persisted_ranges_in_oldest_to_newest_order() {
        let state = test_state_with_block_capacity(None, 1);
        let app = test::init_service(test_app(state.clone())).await;

        for _ in 0..4 {
            let mine_resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/blocks/mine")
                    .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                    .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                    .to_request(),
            )
            .await;
            assert_eq!(mine_resp.status(), StatusCode::CREATED);
        }

        {
            let blockchain = state.blockchain.read().await;
            assert_eq!(blockchain.height, 4);
            assert_eq!(blockchain.blocks().len(), 1, "only the newest block should remain in memory");
            assert!(blockchain.block_at_height(1).is_none());
            assert!(blockchain.block_at_height(2).is_none());
            assert!(blockchain.block_at_height(3).is_none());
            assert!(blockchain.block_at_height(4).is_some());
        }

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/blocks?limit=4")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let blocks = body["data"]["blocks"]
            .as_array()
            .expect("blocks should be array");
        assert_eq!(body["data"]["chain_height"].as_u64(), Some(4));
        assert_eq!(body["data"]["returned"].as_u64(), Some(4));
        assert_eq!(blocks.len(), 4, "API should merge persisted and in-memory ranges");

        let expected_blocks = {
            let storage = state.storage.read().await;
            storage
                .load_blocks_range(1, 4)
                .expect("persisted blocks should load for parity checks")
        };
        assert_eq!(expected_blocks.len(), 4, "all mined blocks should persist to storage");

        for (index, (summary, expected_block)) in blocks.iter().zip(expected_blocks.iter()).enumerate() {
            assert_eq!(
                summary["height"].as_u64(),
                Some((index + 1) as u64),
                "response should remain oldest-to-newest across storage and memory boundaries",
            );
            assert_chain_block_summary_matches_block(summary, expected_block);
        }
    }

    #[actix_web::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn chain_blocks_limit_zero_clamps_to_latest_block() {
        let state = test_state_with_block_capacity(None, 1);
        let app = test::init_service(test_app(state.clone())).await;

        for _ in 0..3 {
            let mine_resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/blocks/mine")
                    .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                    .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                    .to_request(),
            )
            .await;
            assert_eq!(mine_resp.status(), StatusCode::CREATED);
        }

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/blocks?limit=0")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let blocks = body["data"]["blocks"]
            .as_array()
            .expect("blocks should be array");
        assert_eq!(body["data"]["chain_height"].as_u64(), Some(3));
        assert_eq!(body["data"]["returned"].as_u64(), Some(1));
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["height"].as_u64(), Some(3));
    }

    #[actix_web::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn chain_blocks_large_limits_return_all_available_blocks_without_contract_regression() {
        let state = test_state_with_block_capacity(None, 1);
        let app = test::init_service(test_app(state.clone())).await;

        for _ in 0..3 {
            let mine_resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/blocks/mine")
                    .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                    .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                    .to_request(),
            )
            .await;
            assert_eq!(mine_resp.status(), StatusCode::CREATED);
        }

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/blocks?limit=9999")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let blocks = body["data"]["blocks"]
            .as_array()
            .expect("blocks should be array");
        assert_eq!(body["data"]["chain_height"].as_u64(), Some(3));
        assert_eq!(body["data"]["returned"].as_u64(), Some(3));
        assert_eq!(blocks.len(), 3, "limit above max cap and chain height should return all available blocks");
        assert_eq!(blocks[0]["height"].as_u64(), Some(1));
        assert_eq!(blocks[1]["height"].as_u64(), Some(2));
        assert_eq!(blocks[2]["height"].as_u64(), Some(3));
    }

    #[actix_web::test]
    async fn chain_blocks_invalid_limit_inputs_preserve_bad_request_behavior() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        for uri in [
            "/chain/blocks?limit=abc",
            "/chain/blocks?limit=%zz",
            "/chain/blocks?limit=2&limit=1",
        ] {
            let resp = test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{uri} should preserve current rejection behavior");
        }
    }

    #[actix_web::test]
    async fn chain_block_by_height_not_found() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/block/99999")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn chain_block_by_height_found_after_mine() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let mine_resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                .to_request(),
        )
        .await;
        assert_eq!(mine_resp.status(), StatusCode::CREATED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get().uri("/chain/block/1").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let data = &body["data"];
        assert_eq!(data["height"].as_u64().unwrap_or(0), 1);
        assert!(data["timestamp"].is_string());
        assert!(data["transactions"].is_array());
        assert!(data["difficulty"].is_number());
    }

    #[actix_web::test]
    async fn chain_block_detail_exposes_authenticated_producer_attestation_2404() {
        use wrfcoin_shared_protocol::consensus::block_producer_auth::sign_block_producer;
        use wrfcoin_shared_protocol::crypto::chain_binding::{
            ChainIdentity, SigningProtocolVersion,
        };

        let state = test_state(None);
        let signer = PrivateKey::from_bytes(&[0x24; 32]).expect("test producer key is valid");
        {
            let mut blockchain = state.blockchain.write().await;
            let mut block = Block::new(3, blockchain.tip, Vec::new(), 1, blockchain.height + 1);
            let attestation = sign_block_producer(
                &block,
                &signer,
                ChainIdentity::active(),
                SigningProtocolVersion::active(),
            );
            block.producer_pubkey = attestation.producer_pubkey;
            block.producer_sig = attestation.producer_sig.to_vec();
            block.set_hash(block.calculate_hash());
            blockchain
                .add_block(block)
                .expect("authenticated test block should be accepted");
        }

        let app = test::init_service(test_app(state)).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/block/1")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let data = &body["data"];
        let pubkey = data["producer_pubkey"]
            .as_array()
            .expect("producer_pubkey must be serialized as bytes");
        let signature = data["producer_sig"]
            .as_array()
            .expect("producer_sig must be serialized as bytes");
        assert_eq!(pubkey.len(), 32);
        assert!(pubkey.iter().any(|byte| byte.as_u64().unwrap_or(0) != 0));
        assert_eq!(signature.len(), 64);
        assert!(signature.iter().any(|byte| byte.as_u64().unwrap_or(0) != 0));
    }

    #[actix_web::test]
    async fn tx_status_pending_then_included_with_height() {
        let state = test_state(None);
        let tx = test_mempool_tx(3237);
        let tx_id = tx.hash();
        state
            .mempool
            .write()
            .await
            .add_transaction(tx.clone())
            .expect("transaction should enter the test mempool");
        let app = test::init_service(test_app(state.clone())).await;

        let pending = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/transactions/{tx_id}/status"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(pending.status(), StatusCode::OK);
        let pending_body: serde_json::Value = test::read_body_json(pending).await;
        assert_eq!(pending_body["data"]["status"], "pending");
        assert_eq!(pending_body["data"]["window_limited"], true);
        assert!(pending_body["data"].get("block_height").is_none());

        state.mempool.write().await.remove_transaction(&tx_id);
        let (height, _) = seal_block_2787(&state, vec![tx]).await;
        let included = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/transactions/{tx_id}/status"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(included.status(), StatusCode::OK);
        let included_body: serde_json::Value = test::read_body_json(included).await;
        assert_eq!(included_body["data"]["status"], "included");
        assert_eq!(included_body["data"]["block_height"], height);
    }

    #[actix_web::test]
    async fn tx_status_unknown_for_random_id() {
        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let unknown = "77".repeat(32);
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/transactions/{unknown}/status"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["data"]["status"], "unknown");
        assert_eq!(body["data"]["window_limited"], true);
        assert!(body["data"].get("block_height").is_none());
    }

    #[actix_web::test]
    async fn address_transactions_paginates_and_bounds_limit() {
        let state = test_state(None);
        let address = Address::new([0x37; 20]);
        let mut txs = Vec::new();
        for tag in 1..=3 {
            let mut tx = test_mempool_tx(3237 + tag);
            tx.sender_address = address;
            tx.set_id();
            txs.push(tx);
        }
        seal_block_2787(&state, txs).await;
        let app = test::init_service(test_app(state)).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!(
                    "/addresses/{address}/transactions?limit=999&offset=1"
                ))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["data"]["limit"], 100);
        assert_eq!(body["data"]["offset"], 1);
        assert_eq!(body["data"]["total_in_window"], 3);
        assert_eq!(body["data"]["transactions"].as_array().unwrap().len(), 2);
        assert!(body["data"]["window_start_height"].is_number());
        assert_eq!(body["data"]["window_limited"], true);
    }

    // State-backed credits are not transactions: the canonical paid record,
    // not a reveal sender or nominal block metadata amount, names the payee.
    #[actix_web::test]
    async fn address_transactions_received_credit_late_quorum_clamp_and_readonly() {
        use wrfcoin_shared_protocol::blockchain::state::ContributionRewardRecord;
        use wrfcoin_shared_protocol::oracle::attestation::{AttestationRecord, AttestationSet};
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;

        let state = test_state(None);
        let owner = Address::new([0x71; 20]);
        let cap = state
            .blockchain
            .read()
            .await
            .state
            .emission_for_tests()
            .per_user_emission_cap(0);
        let planned = ContributionRewardRecord {
            address: owner,
            amount: Balance::new(cap * 100),
            reason: "contribution:late-quorum".into(),
            station_id: "weather-station".into(),
            provenance_class: ContributionProvenanceClass::Real,
        };
        state
            .blockchain
            .write()
            .await
            .state
            .apply_transition(StateTransition::ApplyContributionRewardDecision {
                record: planned.clone(),
                current_block: 0,
            })
            .unwrap();
        let app = test::init_service(test_app(state.clone())).await;
        let uri = format!("/addresses/{owner}/transactions");
        let pending = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&uri)
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed #[cfg(test)] TEST_READER_TOKEN fixture for an in-process TestRequest and in-memory test_token_store; no runtime credentials
                .to_request(),
        )
        .await;
        let pending: serde_json::Value = test::read_body_json(pending).await;
        assert_eq!(
            pending["data"]["credit_events"],
            json!([]),
            "no payment before quorum"
        );
        {
            let mut chain = state.blockchain.write().await;
            let mut quorum = AttestationSet::default();
            for oracle in ["oracle-a", "oracle-b"] {
                quorum.add(AttestationRecord {
                    oracle_id: oracle.into(),
                    approved: true,
                    confidence: 1.0,
                    timestamp: 0,
                });
            }
            chain.state.oracle_quorum = 2;
            chain
                .state
                .attestations
                .insert("late-quorum".into(), quorum);
            for _ in 0..2 {
                chain
                    .state
                    .apply_transition(StateTransition::ApplyContributionRewardDecision {
                        record: planned.clone(),
                        current_block: 0,
                    })
                    .unwrap();
            }
            assert_eq!(
                chain
                    .state
                    .get_account(&owner)
                    .unwrap()
                    .earned_balance_total,
                Balance::new(cap)
            );
            // Two retained observations of the nominal decision must not duplicate
            // the paid record or replace its smaller, actually credited amount.
            for _ in 0..2 {
                let mut block = Block::new(3, chain.tip, vec![], 1, chain.height + 1);
                block.metadata.insert(
                    "reward_entries".into(),
                    serde_json::to_string(&vec![planned.clone()]).unwrap(),
                );
                block.set_hash(block.calculate_hash());
                chain.add_block(block).unwrap();
            }
        }
        let (before_root, before_bytes) = {
            let chain = state.blockchain.read().await;
            (
                chain.state.state_root_material(),
                bincode::serialize(&chain.state).unwrap(),
            )
        };
        for _ in 0..2 {
            let response = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri(&uri)
                    .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed #[cfg(test)] TEST_READER_TOKEN fixture for an in-process TestRequest and in-memory test_token_store; no runtime credentials
                    .to_request(),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let body: serde_json::Value = test::read_body_json(response).await;
            let credits = body["data"]["credit_events"].as_array().unwrap();
            assert_eq!(credits.len(), 1);
            assert_eq!(credits[0]["event_id"], "contribution:late-quorum");
            assert_eq!(credits[0]["direction"], "received");
            assert_eq!(credits[0]["amount_base_units"], cap.to_string());
            assert_eq!(credits[0]["kind"], "weather_reward");
            assert_eq!(credits[0]["settlement"], "paid");
            assert!(credits[0]["tx_id"].is_null());
            assert!(credits[0]["block_height"].is_null());
            assert!(credits[0]["counterparty"].is_null());
            assert!(body["data"].get("credit_total").is_none());
        }
        let stranger = Address::new([0x72; 20]);
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/addresses/{stranger}/transactions"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed #[cfg(test)] TEST_READER_TOKEN fixture for an in-process TestRequest and in-memory test_token_store; no runtime credentials
                .to_request(),
        )
        .await;
        let body: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(body["data"]["credit_events"], json!([]));
        let chain = state.blockchain.read().await;
        assert_eq!(chain.state.state_root_material(), before_root);
        assert_eq!(
            bincode::serialize(&chain.state).unwrap(),
            before_bytes,
            "history reads must not mutate any state"
        );
    }

    #[actix_web::test]
    async fn address_transactions_received_credit_snapshot_pagination_and_exclusions() {
        use wrfcoin_shared_protocol::blockchain::state::ContributionRewardRecord;
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass as Provenance;
        let state = test_state(None);
        let owner = Address::new([0x73; 20]);
        {
            let mut chain = state.blockchain.write().await;
            // Snapshot fixtures: no corresponding blocks in memory. Amounts
            // above JS precision must remain strings. Insertion order is reversed.
            for (id, reason, amount, provenance, finalized) in [
                ("zz", "forecast:zz", u128::MAX, Provenance::Real, true),
                ("a", "contribution:a", 100_000_001, Provenance::Real, true),
                (
                    "pending",
                    "contribution:pending",
                    1,
                    Provenance::Real,
                    false,
                ),
                (
                    "synthetic",
                    "contribution:synthetic",
                    1, // hostile snapshot: provenance must exclude even positive amounts
                    Provenance::Synthetic,
                    true,
                ),
                (
                    "unclassified",
                    "contribution:unclassified",
                    1,
                    Provenance::Unclassified,
                    true,
                ),
                ("zero", "contribution:zero", 0, Provenance::Real, true),
                ("bad-key", "contribution:a", 1, Provenance::Real, true),
            ] {
                chain.state.contribution_reward_records.insert(
                    id.into(),
                    ContributionRewardRecord {
                        address: owner,
                        amount: Balance::new(amount),
                        reason: reason.into(),
                        station_id: "snapshot-station".into(),
                        provenance_class: provenance,
                    },
                );
                if finalized {
                    chain.state.finalized_contribution_rewards.insert(id.into());
                }
            }
        }
        let app = test::init_service(test_app(state)).await;
        for (query, expected, cursor) in [
            ("credit_limit=1", "contribution:a", json!("a")),
            (
                "credit_limit=1&credit_after=zero",
                "forecast:zz",
                serde_json::Value::Null,
            ),
        ] {
            let resp = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri(&format!("/addresses/{owner}/transactions?{query}"))
                    .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed #[cfg(test)] TEST_READER_TOKEN fixture for an in-process TestRequest and in-memory test_token_store; no runtime credentials
                    .to_request(),
            )
            .await;
            let body: serde_json::Value = test::read_body_json(resp).await;
            let data = &body["data"];
            assert!(data.get("credit_total").is_none());
            assert!(data["credit_records_examined"].as_u64().unwrap() <= 2);
            assert_eq!(data["credit_events"].as_array().unwrap().len(), 1);
            assert_eq!(data["credit_events"][0]["event_id"], expected);
            assert_eq!(data["credit_next_cursor"], cursor);
            assert_eq!(data["credit_order"], "commit_id_ascending");
            assert_eq!(
                data["credit_events"][0]["amount_base_units"],
                if expected.starts_with("forecast") {
                    u128::MAX.to_string()
                } else {
                    "100000001".into()
                }
            );
            assert_eq!(
                data["total_in_window"], 0,
                "credit count must not misstate transaction window"
            );
        }
        let all = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/addresses/{owner}/transactions?credit_limit=100"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed #[cfg(test)] TEST_READER_TOKEN fixture for an in-process TestRequest and in-memory test_token_store; no runtime credentials
                .to_request(),
        )
        .await;
        let all: serde_json::Value = test::read_body_json(all).await;
        assert_eq!(all["data"]["credit_events"].as_array().unwrap().len(), 2);
        let sparse = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!(
                    "/addresses/{owner}/transactions?credit_limit=1&credit_after=a"
                ))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed #[cfg(test)] TEST_READER_TOKEN fixture for an in-process TestRequest and in-memory test_token_store; no runtime credentials
                .to_request(),
        )
        .await;
        let sparse: serde_json::Value = test::read_body_json(sparse).await;
        assert_eq!(sparse["data"]["credit_events"], json!([]));
        assert_eq!(
            sparse["data"]["credit_next_cursor"], "bad-key",
            "empty pages advance over examined records"
        );
        let no_auth = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/addresses/{owner}/transactions"))
                .to_request(),
        )
        .await;
        assert_eq!(no_auth.status(), StatusCode::UNAUTHORIZED);
        let end = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/addresses/{owner}/transactions?credit_after=zz"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed #[cfg(test)] TEST_READER_TOKEN fixture for an in-process TestRequest and in-memory test_token_store; no runtime credentials
                .to_request(),
        )
        .await;
        let end: serde_json::Value = test::read_body_json(end).await;
        assert_eq!(end["data"]["credit_events"], json!([]));
        assert!(end["data"]["credit_next_cursor"].is_null());
    }

    #[actix_web::test]
    async fn address_transactions_received_transfer_payload_identity_and_self() {
        let state = test_state(None);
        let owner = Address::new([0x74; 20]);
        let peer = Address::new([0x75; 20]);
        let mut txs = vec![];
        for (tag, from, to) in [(1, peer, owner), (2, owner, peer), (3, owner, owner)] {
            let mut tx = test_mempool_tx(7300 + tag);
            tx.tx_type = TransactionType::Transfer;
            tx.sender_address = Address::new([0x76; 20]); // legacy field is NOT authoritative
            tx.data = Some(
                json!({ "from": from.to_string(), "to": to.to_string(),
                "amount": u64::MAX, "transparent_opt_out": true })
                .to_string(),
            );
            tx.set_id();
            txs.push(tx);
        }
        seal_block_2787(&state, txs).await;
        let app = test::init_service(test_app(state)).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/addresses/{owner}/transactions"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed #[cfg(test)] TEST_READER_TOKEN fixture for an in-process TestRequest and in-memory test_token_store; no runtime credentials
                .to_request(),
        )
        .await;
        let body: serde_json::Value = test::read_body_json(resp).await;
        let txs = body["data"]["transactions"].as_array().unwrap();
        assert_eq!(txs.len(), 3);
        for (tx, direction, counterparty) in [
            (&txs[0], "self", owner),
            (&txs[1], "sent", peer),
            (&txs[2], "received", peer),
        ] {
            assert_eq!(tx["direction"], direction);
            assert_eq!(tx["counterparty"], counterparty.to_string());
            assert_eq!(tx["amount_base_units"], u64::MAX.to_string());
            assert_eq!(tx["kind"], "transfer");
            assert!(tx["event_id"].as_str().unwrap().starts_with("transaction:"));
        }
        assert_eq!(txs[1]["from"], owner.to_string());
        assert_eq!(txs[2]["to"], owner.to_string());
        let legacy_sender = Address::new([0x76; 20]);
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/addresses/{legacy_sender}/transactions"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed #[cfg(test)] TEST_READER_TOKEN fixture for an in-process TestRequest and in-memory test_token_store; no runtime credentials
                .to_request(),
        )
        .await;
        let body: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(
            body["data"]["transactions"],
            json!([]),
            "legacy sender must not override canonical parties"
        );
    }

    #[actix_web::test]
    async fn address_transactions_received_credit_bounds_sparse_large_state_scan() {
        use wrfcoin_shared_protocol::blockchain::state::ContributionRewardRecord;
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;
        let state = test_state(None);
        let owner = Address::new([0x77; 20]);
        {
            let mut chain = state.blockchain.write().await;
            for n in (0..10_000).rev() {
                let id = format!("{n:08}");
                chain.state.contribution_reward_records.insert(
                    id.clone(),
                    ContributionRewardRecord {
                        address: if n == 5001 {
                            owner
                        } else {
                            Address::new([0x78; 20])
                        },
                        amount: Balance::new(1),
                        reason: format!("contribution:{id}"),
                        station_id: "bounded-scan".into(),
                        provenance_class: ContributionProvenanceClass::Real,
                    },
                );
                chain.state.finalized_contribution_rewards.insert(id);
            }
        }
        let app = test::init_service(test_app(state)).await;
        for (cursor, next, count) in [
            ("", "00000002", 0),
            ("&credit_after=00004999", "00005002", 1),
        ] {
            let response = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri(&format!(
                        "/addresses/{owner}/transactions?credit_limit=3{cursor}"
                    ))
                    .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed #[cfg(test)] TEST_READER_TOKEN fixture for an in-process TestRequest and in-memory test_token_store; no runtime credentials
                    .to_request(),
            )
            .await;
            let body: serde_json::Value = test::read_body_json(response).await;
            assert_eq!(
                body["data"]["credit_records_examined"], 4,
                "limit+1 bounds examined records, including non-beneficiaries"
            );
            assert_eq!(body["data"]["credit_scan_limit"], 4);
            assert_eq!(body["data"]["credit_next_cursor"], next);
            assert_eq!(
                body["data"]["credit_events"].as_array().unwrap().len(),
                count
            );
            assert!(
                body["data"].get("credit_total").is_none(),
                "no global count scan"
            );
        }
    }

    #[actix_web::test]
    async fn address_transactions_received_transfer_rejects_invalid_recipient_membership() {
        let state = test_state(None);
        let owner = Address::new([0x79; 20]);
        let peer = Address::new([0x80; 20]);
        let mut txs = vec![];
        for (tag, amount, opt_out) in [(1, 0, true), (2, 1, false)] {
            let mut tx = test_mempool_tx(7400 + tag);
            tx.tx_type = TransactionType::Transfer;
            tx.sender_address = peer;
            tx.data = Some(
                json!({ "from": peer.to_string(), "to": owner.to_string(),
                "amount": amount, "transparent_opt_out": opt_out })
                .to_string(),
            );
            tx.set_id();
            txs.push(tx);
        }
        seal_block_2787(&state, txs).await;
        let app = test::init_service(test_app(state)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/addresses/{owner}/transactions"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed #[cfg(test)] TEST_READER_TOKEN fixture for an in-process TestRequest and in-memory test_token_store; no runtime credentials
                .to_request(),
        )
        .await;
        let body: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(
            body["data"]["total_in_window"], 0,
            "invalid transfer payloads do not claim recipient membership"
        );
        assert_eq!(body["data"]["transactions"], json!([]));
    }

    #[actix_web::test]
    async fn mempool_pending_is_bounded_and_payload_free() {
        let state = test_state(None);
        {
            let mut mempool = state.mempool.write().await;
            for tag in 1..=3 {
                mempool
                    .add_transaction(test_mempool_tx(3300 + tag))
                    .expect("test transaction should enter mempool");
            }
        }
        let app = test::init_service(test_app(state)).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/mempool/pending?limit=2")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        let transactions = body["data"]["transactions"].as_array().unwrap();
        assert_eq!(transactions.len(), 2);
        assert_eq!(body["data"]["limit"], 2);
        assert_eq!(body["data"]["total_pending"], 3);
        assert_eq!(body["data"]["window_limited"], true);
        assert!(transactions.iter().all(|tx| {
            tx.get("data").is_none() && tx.get("payload").is_none() && tx.get("tx_id").is_some()
        }));
    }

    #[actix_web::test]
    async fn existing_transactions_literal_routes_not_shadowed() {
        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        for path in [
            "/transactions/quality/events",
            "/transactions/oracle/status",
            "/transactions/contributions/status",
        ] {
            let resp = test::call_service(
                &app,
                test::TestRequest::get()
                    .uri(path)
                    .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                    .to_request(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::OK, "literal route {path}");
        }
    }

    #[actix_web::test]
    async fn address_transactions_matches_noncanonical_recipient_text() {
        let state = test_state(None);
        let address = Address::new([0xab; 20]);
        let noncanonical_to = format!("0x{}", hex::encode(address.0).to_uppercase());
        let mut tx = test_mempool_tx(3267);
        tx.tx_type = TransactionType::Transfer;
        tx.data = Some(
            json!({
                "from": "0x1111111111111111111111111111111111111111",
                "to": noncanonical_to,
                "amount": 1,
                "transparent_opt_out": true,
            })
            .to_string(),
        );
        tx.set_id();
        seal_block_2787(&state, vec![tx]).await;

        let app = test::init_service(test_app(state)).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/addresses/{address}/transactions"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["data"]["total_in_window"], 1);
        assert_eq!(body["data"]["transactions"].as_array().unwrap().len(), 1);
    }

    // ---------------------------------------------------------------------
    // #2787 canonical block transaction proof projection.
    //
    // The quorum proof needs to show that two *named* oracle identities
    // attested the *same* commit, and that the reveal for that commit landed
    // no earlier. `sender_address` cannot carry that: it is a legacy
    // compatibility field, and the authoritative oracle identity lives only in
    // the signed canonical payload. Every fixture below therefore gives the
    // sender a different value from the payload's `oracle_id`, so a projection
    // that derived identity from the sender would disagree with the assertion.
    // ---------------------------------------------------------------------

    fn canonical_attestation_tx_2787(
        sender_byte: u8,
        commit_id: &str,
        oracle_id: &str,
        approved: bool,
        nonce: &str,
    ) -> Transaction {
        let payload = OracleAttestationPayload {
            commit_id: commit_id.to_string(),
            oracle_id: oracle_id.to_string(),
            approved,
            signature: None,
            confidence: Some(0.9),
            timestamp: 1,
            nonce: Some(nonce.to_string()),
        };
        let mut tx = Transaction::new(Vec::new(), Vec::new());
        tx.tx_type = TransactionType::OracleAttestation;
        tx.sender_address = Address::new([sender_byte; 20]);
        tx.data =
            Some(serde_json::to_string(&payload).expect("attestation payload must serialize"));
        tx
    }

    fn canonical_reveal_tx_2787(sender_byte: u8, commit_id: &str) -> Transaction {
        let payload = json!({
            "commit_id": commit_id,
            "station_id": "PWS-EXAMPLE-PROOF-2787",
            "timestamp": 1,
            "contribution_type": "weather_observation",
            "data": { "temperature_c": 1.5 },
            "nonce": "reveal-nonce",
        });
        let mut tx = Transaction::new(Vec::new(), Vec::new());
        tx.tx_type = TransactionType::ContributionReveal;
        tx.sender_address = Address::new([sender_byte; 20]);
        tx.data = Some(payload.to_string());
        tx
    }

    fn undecodable_tx_2787(tx_type: TransactionType, sender_byte: u8) -> Transaction {
        let mut tx = Transaction::new(Vec::new(), Vec::new());
        tx.tx_type = tx_type;
        tx.sender_address = Address::new([sender_byte; 20]);
        tx.data = Some("{ this is not canonical payload json".to_string());
        tx
    }

    /// Seal `transactions` into the next block and return its canonical height
    /// and hash, so a test can bind projected payloads to the exact block.
    async fn seal_block_2787(
        state: &web::Data<AppState>,
        transactions: Vec<Transaction>,
    ) -> (u64, String) {
        let mut blockchain = state.blockchain.write().await;
        let height = blockchain.height + 1;
        let mut block = Block::new(3, blockchain.tip, transactions, 1, height);
        block.set_hash(block.calculate_hash());
        let hash = block
            .hash
            .as_ref()
            .expect("sealed test block must carry a hash")
            .to_string();
        blockchain
            .add_block(block)
            .expect("canonical proof test block should be accepted");
        (height, hash)
    }

    #[actix_web::test]
    async fn chain_block_detail_projects_canonical_quorum_payloads_2787() {
        const COMMIT: &str = "commit-2787-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        const ORACLE_A: &str = "oracle-2787-a";
        const ORACLE_B: &str = "oracle-2787-b";

        let state = test_state(None);
        let (height, block_hash) = seal_block_2787(
            &state,
            vec![
                canonical_attestation_tx_2787(0x11, COMMIT, ORACLE_A, true, "nonce-a"),
                canonical_attestation_tx_2787(0x22, COMMIT, ORACLE_B, true, "nonce-b"),
                canonical_reveal_tx_2787(0x33, COMMIT),
            ],
        )
        .await;

        let app = test::init_service(test_app(state)).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/chain/block/{height}"))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let data = &body["data"];

        // Canonical block binding: the projection is only meaningful if it is
        // anchored to this exact height and hash.
        assert_eq!(
            data["height"].as_u64().expect("height must be numeric"),
            height
        );
        assert_eq!(
            data["hash"].as_str().expect("hash must be a string"),
            block_hash
        );

        let transactions = data["transactions"]
            .as_array()
            .expect("transactions must be an array");
        assert_eq!(transactions.len(), 3);

        let attestations: Vec<&serde_json::Value> = transactions
            .iter()
            .filter(|tx| tx["type"] == "OracleAttestation")
            .collect();
        assert_eq!(attestations.len(), 2);

        for attestation in &attestations {
            assert_eq!(attestation["payload"]["commit_id"], COMMIT);
            assert_eq!(attestation["payload"]["approved"], true);
            assert!(
                attestation["payload_error"].is_null(),
                "a decodable attestation must not be marked undecodable"
            );
            // The projected identity must come from the canonical payload, not
            // from the legacy sender field.
            assert_ne!(
                attestation["payload"]["oracle_id"], attestation["sender"],
                "oracle identity must not be derived from sender_address"
            );
        }

        let mut projected_ids: Vec<&str> = attestations
            .iter()
            .map(|attestation| {
                attestation["payload"]["oracle_id"]
                    .as_str()
                    .expect("oracle_id must be a string")
            })
            .collect();
        projected_ids.sort_unstable();
        assert_eq!(projected_ids, vec![ORACLE_A, ORACLE_B]);

        let mut projected_nonces: Vec<&str> = attestations
            .iter()
            .map(|attestation| {
                attestation["payload"]["nonce"]
                    .as_str()
                    .expect("nonce must be a non-null string")
            })
            .collect();
        projected_nonces.sort_unstable();
        assert_eq!(projected_nonces, vec!["nonce-a", "nonce-b"]);

        let reveal = transactions
            .iter()
            .find(|tx| tx["type"] == "ContributionReveal")
            .expect("reveal transaction must be projected");
        assert_eq!(reveal["payload"]["commit_id"], COMMIT);
        assert!(
            reveal["payload"]["oracle_id"].is_null(),
            "reveal projection is deliberately narrower than attestation"
        );
    }

    #[actix_web::test]
    async fn chain_block_detail_projects_duplicate_oracle_identity_faithfully_2787() {
        const COMMIT: &str = "commit-2787-duplicate";
        const ORACLE_A: &str = "oracle-2787-a";

        let state = test_state(None);
        let (height, _) = seal_block_2787(
            &state,
            vec![
                canonical_attestation_tx_2787(0x11, COMMIT, ORACLE_A, true, "nonce-a"),
                // Same identity, different sender and nonce: the endpoint must
                // report both faithfully so a verifier can reject the duplicate
                // rather than silently see a satisfied two-identity quorum.
                canonical_attestation_tx_2787(0x22, COMMIT, ORACLE_A, true, "nonce-b"),
            ],
        )
        .await;

        let app = test::init_service(test_app(state)).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/chain/block/{height}"))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let attestations: Vec<&serde_json::Value> = body["data"]["transactions"]
            .as_array()
            .expect("transactions must be an array")
            .iter()
            .filter(|tx| tx["type"] == "OracleAttestation")
            .collect();
        assert_eq!(attestations.len(), 2);
        for attestation in &attestations {
            assert_eq!(attestation["payload"]["oracle_id"], ORACLE_A);
        }
        assert_ne!(
            attestations[0]["sender"], attestations[1]["sender"],
            "fixture must keep senders distinct so only the payload reveals the duplicate"
        );
    }

    #[actix_web::test]
    async fn chain_block_detail_marks_undecodable_proof_payload_2787() {
        let state = test_state(None);
        let (height, _) = seal_block_2787(
            &state,
            vec![
                undecodable_tx_2787(TransactionType::OracleAttestation, 0x11),
                undecodable_tx_2787(TransactionType::ContributionReveal, 0x22),
                // An unrelated malformed transaction must not affect the
                // endpoint or acquire a proof projection.
                undecodable_tx_2787(TransactionType::Transfer, 0x33),
            ],
        )
        .await;

        let app = test::init_service(test_app(state)).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/chain/block/{height}"))
                .to_request(),
        )
        .await;
        // The whole block endpoint stays available.
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let transactions = body["data"]["transactions"]
            .as_array()
            .expect("transactions must be an array");
        assert_eq!(transactions.len(), 3);

        for tx_type in ["OracleAttestation", "ContributionReveal"] {
            let entry = transactions
                .iter()
                .find(|tx| tx["type"] == tx_type)
                .expect("proof-relevant transaction must be projected");
            assert_eq!(entry["payload_error"], "undecodable_canonical_payload");
            assert!(
                entry["payload"].is_null(),
                "an undecodable payload must not be presented as decoded"
            );
        }

        let unrelated = transactions
            .iter()
            .find(|tx| tx["type"] == "Transfer")
            .expect("unrelated transaction must still be listed");
        assert!(unrelated["payload"].is_null());
        assert!(
            unrelated["payload_error"].is_null(),
            "a non-proof-relevant transaction must not be marked undecodable"
        );
    }

    #[actix_web::test]
    async fn chain_block_detail_omits_projection_for_unrelated_transactions_2787() {
        let state = test_state(None);
        let (height, _) =
            seal_block_2787(&state, vec![test_mempool_tx(1), test_mempool_tx(2)]).await;

        let app = test::init_service(test_app(state)).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/chain/block/{height}"))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let transactions = body["data"]["transactions"]
            .as_array()
            .expect("transactions must be an array");
        assert_eq!(transactions.len(), 2);
        for entry in transactions {
            // Backward compatibility: the pre-#2787 shape is unchanged.
            assert!(entry["type"].is_string());
            assert!(entry["sender"].is_string());
            assert!(entry["payload"].is_null());
            assert!(entry["payload_error"].is_null());
        }
    }

    #[actix_web::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn chain_block_by_height_returns_clamped_timestamp_without_contract_regression() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let seeded_timestamp = chrono::DateTime::from_timestamp(
            (chrono::Utc::now() + chrono::Duration::seconds(5)).timestamp(),
            0,
        )
        .expect("test timestamp should be in range");
        seed_block_with_timestamp(&state, seeded_timestamp).await;

        let mine_resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                .to_request(),
        )
        .await;
        assert_eq!(mine_resp.status(), StatusCode::CREATED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/block/2")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let data = &body["data"];
        assert_eq!(data["height"].as_u64(), Some(2));
        assert!(data["hash"].is_string());
        assert!(data["prev_block_hash"].is_string());
        assert!(data["timestamp"].is_string());
        assert!(data["tx_count"].is_number());
        assert!(data["difficulty"].is_number());
        assert!(data["version"].is_number());
        assert!(data["nonce"].is_number());
        assert!(data["metadata"].is_object());
        assert!(data["transactions"].is_array());

        let actual_timestamp = parse_response_timestamp(&data["timestamp"]).with_timezone(&chrono::Utc);
        let expected_timestamp = seeded_timestamp + chrono::Duration::seconds(1);
        // RFC3339 serialization has second precision; compare at Unix-second granularity.
        assert_eq!(
            actual_timestamp.timestamp(),
            expected_timestamp.timestamp(),
            "mined block should clamp to one second after the previous height when wall clock would regress",
        );
    }

    #[actix_web::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn chain_block_by_height_reads_persisted_block_when_evicted_from_memory() {
        let state = test_state_with_block_capacity(None, 1);
        let app = test::init_service(test_app(state.clone())).await;

        for _ in 0..2 {
            let mine_resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/blocks/mine")
                    .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                    .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                    .to_request(),
            )
            .await;
            assert_eq!(mine_resp.status(), StatusCode::CREATED);
        }

        {
            let blockchain = state.blockchain.read().await;
            assert!(blockchain.block_at_height(1).is_none(), "height 1 should be evicted");
            assert!(blockchain.block_at_height(2).is_some());
        }

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/block/1")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let data = &body["data"];
        assert_eq!(data["height"].as_u64(), Some(1));
        assert!(data["timestamp"].is_string());
        assert!(data["hash"].is_string());
        assert!(data["prev_block_hash"].is_string());
        assert!(data["tx_count"].is_number());
        assert!(data["difficulty"].is_number());
        assert!(data["version"].is_number());
        assert!(data["nonce"].is_number());
        assert!(data["metadata"].is_object());
        assert!(data["transactions"].is_array());
    }

    #[actix_web::test]
    #[serial_test::serial(upgrade_deadline)]
    async fn chain_block_by_height_matches_chain_blocks_summary_for_persisted_block() {
        let state = test_state_with_block_capacity(None, 1);
        let app = test::init_service(test_app(state.clone())).await;

        for _ in 0..3 {
            let mine_resp = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/blocks/mine")
                    .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                    .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                    .to_request(),
            )
            .await;
            assert_eq!(mine_resp.status(), StatusCode::CREATED);
        }

        let blocks_resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/blocks?limit=3")
                .to_request(),
        )
        .await;
        assert_eq!(blocks_resp.status(), StatusCode::OK);

        let blocks_body: serde_json::Value = test::read_body_json(blocks_resp).await;
        let summary = blocks_body["data"]["blocks"]
            .as_array()
            .expect("blocks should be array")
            .first()
            .cloned()
            .expect("persisted block summary should be present");
        assert_eq!(summary["height"].as_u64(), Some(1));

        let detail_resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/block/1")
                .to_request(),
        )
        .await;
        assert_eq!(detail_resp.status(), StatusCode::OK);

        let detail_body: serde_json::Value = test::read_body_json(detail_resp).await;
        let detail = &detail_body["data"];
        assert_eq!(detail["height"], summary["height"]);
        assert_eq!(detail["hash"], summary["hash"]);
        assert_eq!(detail["prev_block_hash"], summary["prev_block_hash"]);
        assert_eq!(detail["timestamp"], summary["timestamp"]);
        assert_eq!(detail["tx_count"], summary["tx_count"]);
        assert!(detail["transactions"].is_array());
        assert!(detail["metadata"].is_object());
    }

    // -----------------------------------------------------------------------
    // Block detail metadata contract tests (Round 7 TDD)
    //
    // The node host testnet archive shows that every mined block's detail
    // response includes operator-facing metadata keys set by the PoP
    // consensus engine: reward totals, useful-work indicators, quality
    // enforcement state, and role statistics.  No prior test verified these
    // specific keys, so a silent rename/removal would go undetected.
    //
    // These tests pin the metadata contract by mining a block through the
    // full PoP path and asserting the keys an operator should rely on.
    // -----------------------------------------------------------------------

    #[actix_web::test]
    async fn chain_block_detail_metadata_includes_reward_total() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let mine_resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                .to_request(),
        )
        .await;
        assert_eq!(mine_resp.status(), StatusCode::CREATED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/block/1")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let metadata = &body["data"]["metadata"];
        assert!(
            metadata.is_object(),
            "metadata must be an object"
        );

        // reward_total is set by distribute_rewards() in PoP consensus.
        // Live node host blocks show this as a string-encoded integer.
        let reward_total = metadata
            .get("reward_total")
            .expect("metadata must include reward_total");
        assert!(
            reward_total.is_string(),
            "reward_total should be a string: got {:?}",
            reward_total
        );
        let parsed: u64 = reward_total
            .as_str()
            .unwrap()
            .parse()
            .expect("reward_total must parse as u64");
        assert!(parsed > 0, "reward_total must be positive for a mined block");

        // Pin the exact genesis-tier value.  Test blocks have no weather
        // contributions, so the PoP engine takes the empty-liveness path
        // and divides the coinbase by LIVENESS_BLOCK_REWARD_FRACTION (10).
        //
        // adjusted_block_reward(1) = 5_000_000_000 base units = 50 WRF
        //                            (genesis tier, block_time=14400)
        // empty-liveness penalty:     / 10 = 500_000_000 = 5 WRF
        //
        // Base units, not coins (#2485). Before that fix these were 50 and 5
        // base units — 0.0000005 and 0.00000005 WRF. The `expected` value below
        // is derived from the schedule rather than hardcoded, so this assertion
        // tracked the rescale on its own; only the arithmetic in this comment
        // needed correcting.
        //
        // Live node host blocks have a real contributor, so they get the full
        // 50 WRF (qualifying-liveness path).  Both paths are correct —
        // reward_total reflects the actual coinbase payout, not the schedule
        // maximum. NOTE: N5 archive blocks were produced by the defective build,
        // so their recorded reward_total values are base-unit figures.
        //
        // ADR-003 (#2846): the coinbase draws the CONTRIBUTION slice of E(t)
        // rather than all of it, so the liveness fraction now reduces
        // `coinbase_allowance` — the single source of truth the producer builds
        // to. Reading the helper rather than re-deriving `slice * share / 10_000`
        // keeps this assertion from going stale if the owner moves the
        // coinbase/contribution dial, which is exactly what it is there for.
        use wrfcoin_shared_protocol::consensus::proof_of_publishing::{
            LIVENESS_BLOCK_REWARD_FRACTION, adjusted_block_reward,
        };
        let expected = (state.blockchain.read().await.state.emission_for_tests().coinbase_allowance(1) as u64) / LIVENESS_BLOCK_REWARD_FRACTION;
        assert!(
            (expected as u128) < adjusted_block_reward(1) as u128,
            "the coinbase must be a strict fraction of E(t) under the partition"
        );
        assert_eq!(
            parsed, expected,
            "reward_total at height 1 must match liveness-adjusted reward = {expected}"
        );
    }

    #[actix_web::test]
    async fn chain_block_detail_metadata_includes_useful_work_fields() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let mine_resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                .to_request(),
        )
        .await;
        assert_eq!(mine_resp.status(), StatusCode::CREATED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/block/1")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let metadata = &body["data"]["metadata"];

        // useful_work_liveness_fallback is set when publishing score is below
        // threshold (the common path on a test or single-node chain).
        // Live node host: all 1555 blocks show this field.
        assert!(
            metadata.get("useful_work_liveness_fallback").is_some(),
            "metadata must include useful_work_liveness_fallback (set by PoP liveness path)"
        );

        // useful_work_forecast_reveals counts forecast reveal transactions
        // with valid manifests.  Zero is the expected value on a test chain.
        let forecast_reveals = metadata
            .get("useful_work_forecast_reveals")
            .expect("metadata must include useful_work_forecast_reveals");
        assert!(
            forecast_reveals.is_string(),
            "useful_work_forecast_reveals should be a string: got {:?}",
            forecast_reveals
        );

        // useful_work_quality_contributions counts quality-passing
        // contribution transactions.
        let quality_contributions = metadata
            .get("useful_work_quality_contributions")
            .expect("metadata must include useful_work_quality_contributions");
        assert!(
            quality_contributions.is_string(),
            "useful_work_quality_contributions should be a string: got {:?}",
            quality_contributions
        );
    }

    #[actix_web::test]
    async fn chain_block_detail_metadata_includes_consensus_policy_fields() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let mine_resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/blocks/mine")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}")))
                .set_json(json!({ "miner_address": "0x1111111111111111111111111111111111111111" }))
                .to_request(),
        )
        .await;
        assert_eq!(mine_resp.status(), StatusCode::CREATED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/chain/block/1")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let metadata = &body["data"]["metadata"];

        // publishing_score is always set by create_block().
        let publishing_score = metadata
            .get("publishing_score")
            .expect("metadata must include publishing_score");
        assert!(publishing_score.is_string(), "publishing_score should be a string");

        // quality_enforcement_active reflects whether the current height is
        // past the quality enforcement activation height.
        let qea = metadata
            .get("quality_enforcement_active")
            .expect("metadata must include quality_enforcement_active");
        assert!(
            qea.as_str() == Some("true") || qea.as_str() == Some("false"),
            "quality_enforcement_active must be 'true' or 'false': got {:?}",
            qea
        );

        // task_assigner_enabled reflects the PoP task assigner policy.
        let tae = metadata
            .get("task_assigner_enabled")
            .expect("metadata must include task_assigner_enabled");
        assert!(
            tae.as_str() == Some("true") || tae.as_str() == Some("false"),
            "task_assigner_enabled must be 'true' or 'false': got {:?}",
            tae
        );
    }

    // -----------------------------------------------------------------------
    // Route response contract tests (TDD Round 12, Lane A)
    //
    // These tests pin the response shape of operator-critical endpoints that
    // previously had zero test coverage.  They assert field presence and types
    // so that silent renames/removals are caught in CI.
    // -----------------------------------------------------------------------

    #[actix_web::test]
    async fn mining_stats_returns_valid_response() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get().uri("/mining/stats").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;

        // chain_height must be a non-negative number.
        let chain_height = body
            .get("chain_height")
            .expect("response must include chain_height");
        assert!(chain_height.is_number(), "chain_height must be a number");
        assert!(
            chain_height.as_u64().is_some(),
            "chain_height must be a non-negative integer"
        );

        // blocks_in_memory must be present and numeric.
        assert!(
            body.get("blocks_in_memory")
                .and_then(|v| v.as_u64())
                .is_some(),
            "blocks_in_memory must be a non-negative integer"
        );

        // total_transactions must be present and numeric.
        assert!(
            body.get("total_transactions")
                .and_then(|v| v.as_u64())
                .is_some(),
            "total_transactions must be a non-negative integer"
        );

        // difficulty must be present and numeric.
        assert!(
            body.get("difficulty").and_then(|v| v.as_u64()).is_some(),
            "difficulty must be a non-negative integer"
        );

        // avg_block_time_secs must be present and a number (float).
        let avg = body
            .get("avg_block_time_secs")
            .expect("response must include avg_block_time_secs");
        assert!(avg.is_number(), "avg_block_time_secs must be a number");
    }

    #[actix_web::test]
    async fn node_resources_weather_acceptance_is_nullable_and_state_backed() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let request = || test::TestRequest::get().uri("/node/resources").to_request();
        let response = test::call_service(&app, request()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(body.get("weather_last_accepted_block_unix"), Some(&serde_json::Value::Null));
        assert_eq!(body.get("weather_last_accepted_block_age_secs"), Some(&serde_json::Value::Null));
        for field in ["chain_status", "chain_last_block_age_secs", "chain_block_interval_secs",
            "uptime_secs", "blocks_in_memory", "account_count", "nft_count", "pending_transactions"]
        {
            assert!(body.get(field).is_some(), "retain {field}");
        }
        let newest = chrono::Utc::now().timestamp() - 120;
        {
            let mut chain = state.blockchain.write().await;
            chain.state.station_weather_accepted_block_unix.insert("older".into(), newest - 3600);
            chain.state.station_weather_accepted_block_unix.insert("newer".into(), newest);
        }
        let response = test::call_service(&app, request()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(body["weather_last_accepted_block_unix"], newest);
        let upper = chrono::Utc::now().timestamp().saturating_sub(newest);
        let age = body["weather_last_accepted_block_age_secs"].as_i64().expect("age");
        assert!((120..=upper).contains(&age));
    }

    #[actix_web::test]
    async fn node_resources_returns_system_metrics() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/node/resources")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;

        // uptime_secs must be a non-negative integer.
        assert!(
            body.get("uptime_secs").and_then(|v| v.as_u64()).is_some(),
            "uptime_secs must be a non-negative integer"
        );

        // blocks_in_memory must be present.
        assert!(
            body.get("blocks_in_memory")
                .and_then(|v| v.as_u64())
                .is_some(),
            "blocks_in_memory must be a non-negative integer"
        );

        // account_count must be present.
        assert!(
            body.get("account_count")
                .and_then(|v| v.as_u64())
                .is_some(),
            "account_count must be a non-negative integer"
        );

        // nft_count must be present.
        assert!(
            body.get("nft_count").and_then(|v| v.as_u64()).is_some(),
            "nft_count must be a non-negative integer"
        );

        // pending_transactions must be present.
        assert!(
            body.get("pending_transactions")
                .and_then(|v| v.as_u64())
                .is_some(),
            "pending_transactions must be a non-negative integer"
        );
    }

    // ---------------------------------------------------------------------------
    // /node/capability tests (#1801)
    // ---------------------------------------------------------------------------

    #[actix_web::test]
    async fn node_capability_returns_200_with_default_schema() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/node/capability")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;

        // All five contract fields must be present.
        assert_eq!(body["wan_state"], "PrimaryOnline", "default wan_state should be PrimaryOnline");
        assert_eq!(body["is_metered"], false, "default is_metered should be false");
        assert_eq!(body["failover_count"], 0, "default failover_count should be 0");
        assert_eq!(body["mesh_peer_count"], 0, "default mesh_peer_count should be 0");
        assert_eq!(body["ap_mode_enabled"], false, "default ap_mode_enabled should be false");
    }

    #[actix_web::test]
    async fn node_capability_reflects_updated_snapshot() {
        use wrfcoin_native_chain::network_failover::{NodeCapabilitySnapshot, WanState};

        let state = test_state(None);

        // Simulate the failover monitor writing a new snapshot.
        {
            let mut snap = state.node_capability.write().await;
            *snap = NodeCapabilitySnapshot {
                wan_state: WanState::FailoverActive,
                is_metered: true,
                failover_count: 2,
                mesh_peer_count: 5,
                ap_mode_enabled: true,
            };
        }

        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/node/capability")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["wan_state"], "FailoverActive");
        assert_eq!(body["is_metered"], true);
        assert_eq!(body["failover_count"], 2);
        assert_eq!(body["mesh_peer_count"], 5);
        assert_eq!(body["ap_mode_enabled"], true);
    }

    /// Verify that capability snapshot data is propagated into the `capability_profile`
    /// section of `GET /node/capability`, mapping to `NodeCapabilityProfile.telemetry_*`
    /// field names (#1802).
    #[actix_web::test]
    async fn node_capability_profile_telemetry_fields_populated_from_snapshot() {
        use wrfcoin_native_chain::network_failover::{NodeCapabilitySnapshot, WanState};

        let state = test_state(None);
        {
            let mut snap = state.node_capability.write().await;
            *snap = NodeCapabilitySnapshot {
                wan_state: WanState::FailoverActive,
                is_metered: true,
                failover_count: 1,
                mesh_peer_count: 3,
                ap_mode_enabled: true,
            };
        }

        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/node/capability")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let profile = &body["capability_profile"];
        assert_eq!(
            profile["telemetry_ap_active"], true,
            "ap_mode_enabled should map to telemetry_ap_active"
        );
        assert_eq!(
            profile["telemetry_lte_active"], true,
            "is_metered should map to telemetry_lte_active"
        );
        assert_eq!(
            profile["telemetry_mesh_peer_count"], 3,
            "mesh_peer_count should map to telemetry_mesh_peer_count"
        );
    }

    #[actix_web::test]
    async fn blockchain_summary_returns_chain_state() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get().uri("/blockchain").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_success_envelope(&body);

        let data = &body["data"];

        // height must be a non-negative number.
        let height = data
            .get("height")
            .expect("data must include height");
        assert!(height.is_number(), "height must be a number");
        assert!(
            height.as_u64().is_some(),
            "height must be a non-negative integer"
        );

        // accounts (account count) must be present.
        assert!(
            data.get("accounts").and_then(|v| v.as_u64()).is_some(),
            "data.accounts must be a non-negative integer"
        );

        // nfts (nft count) must be present.
        assert!(
            data.get("nfts").and_then(|v| v.as_u64()).is_some(),
            "data.nfts must be a non-negative integer"
        );

        // pending_transactions must be present.
        assert!(
            data.get("pending_transactions")
                .and_then(|v| v.as_u64())
                .is_some(),
            "data.pending_transactions must be a non-negative integer"
        );

        // tip must be a non-empty string.
        assert!(
            data.get("tip")
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            "data.tip must be a non-empty string"
        );
    }

    #[actix_web::test]
    async fn oracle_leaderboard_returns_array_or_empty() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/oracle/leaderboard")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;

        // entries must be an array (may be empty on a fresh chain).
        let entries = body
            .get("entries")
            .expect("response must include entries");
        assert!(entries.is_array(), "entries must be an array");

        // total_oracles must be present and numeric.
        assert!(
            body.get("total_oracles")
                .and_then(|v| v.as_u64())
                .is_some(),
            "total_oracles must be a non-negative integer"
        );

        // On a fresh chain with no oracles, entries should be empty.
        assert!(
            entries.as_array().unwrap().is_empty(),
            "entries should be empty on a fresh chain with no registered oracles"
        );
        assert_eq!(
            body["total_oracles"].as_u64(),
            Some(0),
            "total_oracles should be 0 on a fresh chain"
        );
    }

    #[actix_web::test]
    async fn network_info_includes_network_id_and_chain_id() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get().uri("/network/info").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;

        // network_id must be a non-empty string.
        let network_id = body
            .get("network_id")
            .expect("response must include network_id");
        assert!(
            network_id
                .as_str()
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            "network_id must be a non-empty string"
        );

        // chain_id must be a non-empty string.
        let chain_id = body
            .get("chain_id")
            .expect("response must include chain_id");
        assert!(
            chain_id
                .as_str()
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            "chain_id must be a non-empty string"
        );

        // Default chain_id when env var is not set should be "777".
        assert_eq!(
            chain_id.as_str(),
            Some("777"),
            "chain_id should default to 777 when WRFCOIN_CHAIN_ID is not set"
        );
    }

    // ── /network/info includes local_peer_id field (infra#392) ───────────

    #[actix_web::test]
    async fn network_info_includes_local_peer_id_field() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get().uri("/network/info").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        // Field must be present; null when p2p is not enabled.
        assert!(
            body.get("local_peer_id").is_some(),
            "/network/info must include local_peer_id key"
        );
    }

    // ── /network/peer_id endpoint (infra#392) ────────────────────────────

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn network_peer_id_returns_expected_shape() {
        use wrfcoin_p2p::P2pClient;

        let known_peer_id = libp2p::PeerId::random();
        let p2p_client = P2pClient::for_testing(known_peer_id);

        let state = test_state(None);
        // Override p2p field with the stub client.
        let state_with_p2p = web::Data::new(AppState {
            p2p: Some(p2p_client),
            ..(**state).clone()
        });

        let app = test::init_service(test_app(state_with_p2p)).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get().uri("/network/peer_id").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body: serde_json::Value = test::read_body_json(resp).await;
        let peer_id = body["peer_id"].as_str().expect("peer_id must be a string");
        assert!(!peer_id.is_empty(), "peer_id must be non-empty");
        assert_eq!(
            peer_id,
            known_peer_id.to_string(),
            "peer_id must match the configured local peer"
        );
        let listen_addrs = body["listen_addrs"]
            .as_array()
            .expect("listen_addrs must be an array");
        assert!(!listen_addrs.is_empty(), "listen_addrs must have at least one entry");
        assert!(
            listen_addrs[0].as_str().unwrap_or("").contains("/p2p/"),
            "listen_addrs[0] must include the /p2p/<peer_id> suffix"
        );
    }

    #[cfg(feature = "p2p")]
    #[actix_web::test]
    async fn network_peer_id_returns_503_when_p2p_disabled() {
        let state = test_state(None); // p2p: None
        let app = test::init_service(test_app(state)).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get().uri("/network/peer_id").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // -----------------------------------------------------------------------
    // Token store fail-fast tests
    // -----------------------------------------------------------------------

    /// When no token env-vars are set and ALLOW_NO_AUTH is absent,
    /// `collect_token_store` must return Err (fail-fast).
    #[tokio::test]
    async fn collect_token_store_returns_err_when_no_tokens_and_no_bypass() {
        let _guard = token_store_env_lock();
        clear_token_store_env();

        let result = collect_token_store();
        assert!(result.is_err(), "expected Err when no tokens and no bypass");
    }

    /// When WRFCOIN_ALLOW_NO_AUTH=1 and no tokens are set, the function should
    /// return Ok with an empty map (dev-mode bypass, debug builds only).
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn collect_token_store_allows_empty_when_bypass_is_set() {
        let _guard = token_store_env_lock();
        clear_token_store_env();
        std::env::set_var("WRFCOIN_ALLOW_NO_AUTH", "1");

        let result = collect_token_store();
        clear_token_store_env();

        let store = result.expect("expected Ok when ALLOW_NO_AUTH=1");
        assert!(store.is_empty(), "store should be empty in bypass mode");
    }

    /// When at least one token is set, collect_token_store returns Ok with
    /// that token in the map, regardless of ALLOW_NO_AUTH.
    #[tokio::test]
    async fn collect_token_store_returns_ok_with_configured_tokens() {
        let _guard = token_store_env_lock();
        clear_token_store_env();
        let secret = format!("test-secret-{}", fastrand::u64(..));
        std::env::set_var("WRFCOIN_OPERATOR_TOKEN", &secret);

        let result = collect_token_store();
        clear_token_store_env();

        let store = result.expect("expected Ok when operator token is set");
        assert_eq!(store.len(), 1);
        assert_eq!(
            store.get(&secret),
            Some(&AuthPrincipal::with_role(ApiRole::Operator))
        );
    }

    #[tokio::test]
    async fn collect_token_store_preserves_configured_identity_claims() {
        let _guard = token_store_env_lock();
        clear_token_store_env();
        let secret = format!("reader-secret-{}", fastrand::u64(..));
        std::env::set_var("WRFCOIN_READER_TOKEN", &secret);
        std::env::set_var("WRFCOIN_READER_WALLET_ADDRESS", " 0xabc123 ");
        std::env::set_var("WRFCOIN_READER_CONTRIBUTOR_ID", " station-7 ");

        let result = collect_token_store();

        clear_token_store_env();

        let store = result.expect("expected Ok when reader token is set");
        assert_eq!(store.len(), 1);
        assert_eq!(
            store.get(&secret),
            Some(&AuthPrincipal::with_identity(
                ApiRole::Reader,
                Identity::new(
                    Some("0xabc123".to_string()),
                    Some("station-7".to_string()),
                    None,
                ),
            ))
        );
    }

    /// Whitespace-only tokens are treated as absent.
    #[tokio::test]
    async fn collect_token_store_ignores_whitespace_only_tokens() {
        let _guard = token_store_env_lock();
        clear_token_store_env();
        std::env::set_var("WRFCOIN_OPERATOR_TOKEN", "   ");

        let result = collect_token_store();
        clear_token_store_env();

        assert!(
            result.is_err(),
            "whitespace-only token should be treated as absent"
        );
    }

    #[tokio::test]
    async fn collect_token_store_ignores_whitespace_only_identity_claims() {
        let _guard = token_store_env_lock();
        clear_token_store_env();
        let secret = format!("contributor-secret-{}", fastrand::u64(..));
        std::env::set_var("WRFCOIN_CONTRIBUTOR_TOKEN", &secret);
        std::env::set_var("WRFCOIN_CONTRIBUTOR_WALLET_ADDRESS", "   ");
        std::env::set_var("WRFCOIN_CONTRIBUTOR_ID", "\t");
        std::env::set_var("WRFCOIN_CONTRIBUTOR_ORACLE_NODE_ID", "\n");

        let result = collect_token_store();

        clear_token_store_env();

        let store = result.expect("expected Ok when contributor token is set");
        assert_eq!(
            store.get(&secret),
            Some(&AuthPrincipal::with_role(ApiRole::Contributor))
        );
    }

    /// In release builds the WRFCOIN_ALLOW_NO_AUTH bypass must be absent —
    /// setting it must not prevent collect_token_store from returning Err.
    ///
    /// This test is compiled only in non-debug builds (i.e., `--release`).
    /// It verifies that the compile-time gate works: even with ALLOW_NO_AUTH=1,
    /// production binaries reject empty token stores.
    #[cfg(not(debug_assertions))]
    #[tokio::test]
    async fn collect_token_store_bypass_is_absent_in_release_builds() {
        let _guard = token_store_env_lock();
        clear_token_store_env();
        std::env::set_var("WRFCOIN_ALLOW_NO_AUTH", "1");

        let result = collect_token_store();
        clear_token_store_env();

        assert!(
            result.is_err(),
            "ALLOW_NO_AUTH must have no effect in release builds — bypass is compiled out"
        );
    }

    // -----------------------------------------------------------------------
    // Operator metric contract tests (TDD Round 13, Lane C)
    //
    // These tests pin the Prometheus metric names and label keys that the
    // operator observability stack depends on. The contract is sourced from:
    //   * infra/testnet/grafana-dashboards/node-overview.json
    //   * infra/testnet/grafana-dashboards/weather-data.json
    //   * infra/testnet/alert-rules.yml
    //   * infra/testnet/alert-rules-blockchain-health.yml
    //
    // A silent rename in a refactor would break every Grafana panel and
    // AlertManager rule that references these names. Any change that adds,
    // removes, or renames a pinned metric must be landed together with a
    // matching dashboard/alert update — these tests will fail until both
    // sides move.
    //
    // Scope constraint: production code must NOT be modified to make a failing
    // test pass. If a metric referenced here no longer appears in /metrics,
    // that is an ops visibility regression and should be reported, not
    // silently patched.
    // -----------------------------------------------------------------------

    /// The /metrics endpoint must expose the core chain-health metric names
    /// that Grafana panels in `node-overview.json` and the baseline
    /// `alert-rules.yml` PromQL expressions query directly.
    #[actix_web::test]
    #[serial_test::serial(core_metrics)]
    async fn metrics_endpoint_exposes_core_chain_counters() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/metrics")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}")))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "/metrics must return 200");
        let body = String::from_utf8(test::read_body(resp).await.to_vec())
            .expect("metrics endpoint must return utf-8 prometheus text");

        // Sourced from infra/testnet/grafana-dashboards/node-overview.json
        // panels and infra/testnet/alert-rules.yml expressions. Every name
        // below is load-bearing for the operator observability stack.
        const REQUIRED_CORE_METRICS: &[&str] = &[
            "wrfcoin_chain_height",
            "wrfcoin_blockchain_block_height",
            "wrfcoin_blockchain_mempool_size",
            "wrfcoin_network_peer_count",
            "wrfcoin_blockchain_transactions_processed_total",
            "wrfcoin_blockchain_transactions_failed_total",
        ];

        for metric in REQUIRED_CORE_METRICS {
            assert!(
                body.contains(metric),
                "core chain metric `{metric}` missing from /metrics — \
                 this would break Grafana node-overview dashboard panels. \
                 Body preview (first 2KB): {preview}",
                preview = &body[..body.len().min(2048)],
            );
        }
    }

    /// The /metrics endpoint must expose the 5 fork / reorg / finality
    /// metrics referenced by `infra/testnet/alert-rules-blockchain-health.yml`
    /// (#1148). Six operator alerts — `ChainForkDetected`, `ChainForkRateHigh`,
    /// `ChainReorgDetected`, `ChainReorgOccurred`, `FinalityLag`,
    /// `FinalityStalled` — all depend on these names existing verbatim. Before
    /// #1148, these metrics existed in the alert rules but were never
    /// registered in the Rust code, so all six alerts were silently dead.
    #[actix_web::test]
    #[serial_test::serial(core_metrics)]
    async fn metrics_endpoint_exposes_fork_reorg_finality_counters() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/metrics")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test; TEST_READER_TOKEN is a fixed #[cfg(test)] fixture string
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "/metrics must return 200");
        let body = String::from_utf8(test::read_body(resp).await.to_vec())
            .expect("metrics endpoint must return utf-8 prometheus text");

        // Sourced from infra/testnet/alert-rules-blockchain-health.yml alert
        // expressions. Every name below is load-bearing for six operator
        // alerts. Do NOT rename any of these without coordinating an alert
        // rule migration.
        const REQUIRED_FORK_REORG_METRICS: &[&str] = &[
            "wrfcoin_chain_fork_count",
            "wrfcoin_chain_fork_active",
            "wrfcoin_chain_reorg_depth",
            "wrfcoin_chain_reorg_total",
            "wrfcoin_chain_finalized_height",
        ];

        for metric in REQUIRED_FORK_REORG_METRICS {
            assert!(
                body.contains(metric),
                "fork/reorg/finality metric `{metric}` missing from /metrics — \
                 this would break the {{ChainForkDetected, ChainForkRateHigh, \
                 ChainReorgDetected, ChainReorgOccurred, FinalityLag, \
                 FinalityStalled}} operator alerts. Body preview (first 2KB): \
                 {preview}",
                preview = &body[..body.len().min(2048)],
            );
        }
    }

    /// The /metrics endpoint must expose reward- and slashing-related counters
    /// that the weather-data dashboard and slashing alerts depend on. This
    /// pins the Round 8 metric-label contract (issue #683) so a refactor
    /// cannot silently drop the `reason`-labelled slashing counter.
    #[actix_web::test]
    #[serial_test::serial(core_metrics)]
    async fn metrics_endpoint_exposes_reward_counters() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/metrics")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test; TEST_READER_TOKEN is a fixed #[cfg(test)] fixture string
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "/metrics must return 200");
        let body = String::from_utf8(test::read_body(resp).await.to_vec())
            .expect("metrics endpoint must return utf-8 prometheus text");

        // `wrfcoin_blockchain_slashing_events_total` is queried by
        // infra/testnet/grafana-dashboards/weather-data.json as
        // `rate(wrfcoin_blockchain_slashing_events_total[5m])` and its
        // `reason` label is pre-registered with all SlashingReason variants
        // (see shared-protocol/src/monitoring/mod.rs:87-90).
        assert!(
            body.contains("wrfcoin_blockchain_slashing_events_total"),
            "slashing counter missing from /metrics — would break \
             slashing alerts and weather-data dashboard"
        );

        // Privacy-proof observation counter. These payloads are structurally
        // present; this metric deliberately makes no verification claim (#3250).
        // Registered in shared-protocol/src/monitoring/mod.rs:82.
        assert!(
            body.contains("wrfcoin_blockchain_zk_proofs_observed_total"),
            "zk-proofs-observed counter missing from /metrics"
        );
        assert!(
            !body.contains("wrfcoin_blockchain_zk_verified_proofs_total"),
            "/metrics must not claim structural privacy payloads were verified"
        );

        // Privacy key-image double-spend counter — registered at
        // shared-protocol/src/monitoring/mod.rs:91, surfaces double-spend
        // rejections in the privacy module.
        assert!(
            body.contains("wrfcoin_privacy_key_image_double_spend_total"),
            "key-image double-spend counter missing from /metrics"
        );

        // #2464 late-quorum detector. Must be present on a healthy chain with
        // an explicit 0: the issue's observability defect is precisely that a
        // missing-quorum chain looked identical to a healthy one because no
        // series existed at all.
        assert!(
            body.contains("wrfcoin_oracle_late_quorum_unpaid"),
            "late-quorum-unpaid gauge missing from /metrics — an absent series \
             is indistinguishable from a healthy chain (#2464)"
        );
    }

    /// #2464's reconciliation endpoint is production-facing evidence, not a
    /// helper-only diagnostic. A late-quorum record evicted from the hot chain
    /// window must still be returned through the registered Reader-authenticated
    /// HTTP route from the durable archive.
    #[actix_web::test]
    #[serial_test::serial(late_quorum_base)]
    async fn late_quorum_unpaid_route_reads_evicted_durable_records() {
        use actix_web::test;
        use wrfcoin_native_chain::blockchain::block::Block;

        let state = test_state_with_block_capacity(None, 4);
        let mut parent = state.blockchain.read().await.tip;
        for height in 1..=6u64 {
            let mut block = Block::new(1, parent, vec![], 1, height);
            if height == 1 {
                block.metadata.insert(
                    "oracle_reward_late_quorum_count".to_string(),
                    "3".to_string(),
                );
                block.metadata.insert(
                    "oracle_reward_late_quorum_commit_ids".to_string(),
                    "commit-1-a,commit-1-b,commit-1-c".to_string(),
                );
            }
            let hash = block.calculate_hash();
            block.set_hash(hash);
            parent = hash;
            state
                .storage
                .write()
                .await
                .persist_block(block.clone(), vec![], None)
                .await
                .expect("late-quorum block must persist before the route reads it");
            state
                .blockchain
                .write()
                .await
                .add_block(block)
                .expect("late-quorum block must enter the hot chain window");
        }
        assert!(
            !state.blockchain.read().await.blocks().contains_key(&1),
            "setup must evict the only late-quorum record from the hot window"
        );

        let app = test::init_service(test_app(state.clone())).await;
        let unauthorized = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/oracle/late-quorum/unpaid")
                .to_request(),
        )
        .await;
        assert_eq!(
            unauthorized.status(),
            StatusCode::UNAUTHORIZED,
            "the reconciliation backlog must not be exposed without Reader auth"
        );

        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/oracle/late-quorum/unpaid")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test; TEST_READER_TOKEN is a fixed #[cfg(test)] fixture string
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(body["ok"], true);
        assert_eq!(body["data"]["total_unpaid"], 3);
        assert_eq!(body["data"]["blocks_with_unpaid"], 1);
        assert_eq!(body["data"]["entries"][0]["height"], 1);
        assert_eq!(
            body["data"]["entries"][0]["commit_ids"],
            serde_json::json!(["commit-1-a", "commit-1-b", "commit-1-c"]),
            "the route must expose durable reconciliation identifiers, not just a count"
        );
    }

    /// The chain_height gauge exposed at /metrics must equal the blockchain's
    /// actual tip height after blocks are added. Catches bugs where the
    /// metrics handler reports a stale or zero value even though the chain
    /// has advanced.
    #[actix_web::test]
    #[serial_test::serial(core_metrics)]
    async fn metrics_chain_height_matches_blockchain_state() {
        let state = test_state(None);

        // Seed three contiguous blocks at heights 1, 2, 3 directly via the
        // blockchain state (same pattern used by the governance-dashboard
        // tests earlier in this module).
        {
            let mut blockchain = state.blockchain.write().await;

            let mut block_1 = Block::new(1, Hash::default(), vec![], 1, 1);
            let h1 = block_1.calculate_hash();
            block_1.set_hash(h1);
            blockchain
                .add_block(block_1)
                .expect("block 1 should be added");

            let mut block_2 = Block::new(1, h1, vec![], 1, 2);
            let h2 = block_2.calculate_hash();
            block_2.set_hash(h2);
            blockchain
                .add_block(block_2)
                .expect("block 2 should be added");

            let mut block_3 = Block::new(1, h2, vec![], 1, 3);
            let h3 = block_3.calculate_hash();
            block_3.set_hash(h3);
            blockchain
                .add_block(block_3)
                .expect("block 3 should be added");

            assert_eq!(blockchain.height, 3, "test setup: chain must be at height 3");
        }

        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/metrics")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test; TEST_READER_TOKEN is a fixed #[cfg(test)] fixture string
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "/metrics must return 200");
        let body = String::from_utf8(test::read_body(resp).await.to_vec())
            .expect("metrics endpoint must return utf-8 prometheus text");

        // Parse `wrfcoin_chain_height <value>` from the prometheus text
        // exposition. This is the launch-readiness gate alias expected by
        // infra/testnet/scripts/testnet-launch-readiness.sh (#1620).
        let launch_gate_line = body
            .lines()
            .find(|l| {
                l.starts_with("wrfcoin_chain_height ")
                    || l.starts_with("wrfcoin_chain_height{")
            })
            .unwrap_or_else(|| {
                panic!(
                    "wrfcoin_chain_height line missing from /metrics body. \
                     Body preview: {}",
                    &body[..body.len().min(2048)]
                )
            });
        let launch_gate_value: u64 = launch_gate_line
            .split_whitespace()
            .last()
            .and_then(|tok| tok.parse().ok())
            .unwrap_or_else(|| panic!("failed to parse height from metric line: {launch_gate_line}"));

        // Parse `wrfcoin_blockchain_block_height <value>` from the prometheus
        // text exposition. The gauge is written by the /metrics handler via
        // build_runtime_monitoring_snapshot -> CORE_METRICS.apply_snapshot,
        // so its value in the body reflects the chain tip at request time.
        let line = body
            .lines()
            .find(|l| {
                l.starts_with("wrfcoin_blockchain_block_height ")
                    || l.starts_with("wrfcoin_blockchain_block_height{")
            })
            .unwrap_or_else(|| {
                panic!(
                    "wrfcoin_blockchain_block_height line missing from /metrics body. \
                     Body preview: {}",
                    &body[..body.len().min(2048)]
                )
            });
        let value: u64 = line
            .split_whitespace()
            .last()
            .and_then(|tok| tok.parse().ok())
            .unwrap_or_else(|| panic!("failed to parse height from metric line: {line}"));

        assert_eq!(
            value, 3,
            "wrfcoin_blockchain_block_height gauge must equal blockchain tip \
             height (3). Observed line: {line}"
        );
        assert_eq!(
            launch_gate_value, 3,
            "wrfcoin_chain_height gauge must equal blockchain tip height (3). \
             Observed line: {launch_gate_line}"
        );
    }

    #[actix_web::test]
    #[serial_test::serial(core_metrics)]
    async fn metrics_state_diverged_gauge_tracks_app_state() {
        use wrfcoin_shared_protocol::monitoring::CORE_METRICS;

        const PEER_ID: &str = "12D3KooWMetricsEndpointTestPeer";
        // #2589: a peer earns its `peer_id` series at ADMISSION, not at connect.
        CORE_METRICS.admit_state_divergence_peer(PEER_ID);
        let state = test_state(None);
        state
            .state_diverged
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/metrics")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test; TEST_READER_TOKEN is a fixed #[cfg(test)] fixture string
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "/metrics must return 200");
        let body = String::from_utf8(test::read_body(resp).await.to_vec())
            .expect("metrics endpoint must return utf-8 prometheus text");

        assert!(
            body.contains("wrfcoin_chain_state_diverged"),
            "state divergence gauge missing from /metrics"
        );
        assert!(
            body.contains("wrfcoin_chain_state_diverged_total"),
            "state divergence counter missing from /metrics"
        );
        assert!(
            body.contains("wrfcoin_state_divergence_total"),
            "launch-readiness state divergence counter missing from /metrics"
        );
        assert!(
            body.contains(&format!(
                "wrfcoin_state_divergence_total{{peer_id=\"{PEER_ID}\"}} 0"
            )),
            "connected peer must be pre-initialized at zero before divergence"
        );
        let value = parse_prometheus_u64_metric(&body, "wrfcoin_chain_state_diverged");
        assert_eq!(value, 1, "state divergence gauge must reflect AppState flag");
    }

    fn append_empty_blocks(blockchain: &mut Blockchain, count: u64) {
        for _ in 0..count {
            let next_height = blockchain.height + 1;
            let mut block = Block::new(1, blockchain.tip, Vec::new(), 1, next_height);
            block.set_hash(block.calculate_hash());
            blockchain
                .add_block(block)
                .expect("test block should be added");
        }
    }

    fn parse_prometheus_u64_metric(body: &str, metric_name: &str) -> u64 {
        let line = body
            .lines()
            .find(|line| {
                line.starts_with(&format!("{metric_name} "))
                    || line.starts_with(&format!("{metric_name}{{"))
            })
            .unwrap_or_else(|| {
                panic!(
                    "{metric_name} line missing from /metrics body. Body preview: {}",
                    &body[..body.len().min(2048)]
                )
            });

        line.split_whitespace()
            .last()
            .and_then(|token| token.parse().ok())
            .unwrap_or_else(|| panic!("failed to parse value from metric line: {line}"))
    }

    /// The finalized-height gauge must become non-zero after the chain grows
    /// past MAX_REORG_DEPTH and must continue advancing one-for-one with the
    /// tip after that floor. This is the operator contract behind the
    /// FinalityLag and FinalityStalled alerts checked during the 48h test.
    #[actix_web::test]
    #[serial_test::serial(core_metrics)]
    async fn metrics_finalized_height_advances_with_tip_after_reorg_floor() {
        use wrfcoin_shared_protocol::blockchain::chain::MAX_REORG_DEPTH;

        let state = test_state(None);
        {
            let mut blockchain = state.blockchain.write().await;
            append_empty_blocks(&mut blockchain, MAX_REORG_DEPTH + 3);
            assert_eq!(
                blockchain.height,
                MAX_REORG_DEPTH + 3,
                "test setup must push the chain past the finality floor"
            );
        }

        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/metrics")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test; TEST_READER_TOKEN is a fixed #[cfg(test)] fixture string
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "/metrics must return 200");
        let body = String::from_utf8(test::read_body(resp).await.to_vec())
            .expect("metrics endpoint must return utf-8 prometheus text");
        let first_tip = parse_prometheus_u64_metric(&body, "wrfcoin_blockchain_block_height");
        let first_finalized = parse_prometheus_u64_metric(&body, "wrfcoin_chain_finalized_height");

        assert_eq!(
            first_tip,
            MAX_REORG_DEPTH + 3,
            "tip height should reflect the seeded blockchain state"
        );
        assert_eq!(
            first_finalized, 3,
            "finalized height must be non-zero after the chain exceeds \
             MAX_REORG_DEPTH"
        );
        assert_eq!(
            first_tip - first_finalized,
            MAX_REORG_DEPTH,
            "finality gap must equal the structural reorg floor"
        );

        {
            let mut blockchain = state.blockchain.write().await;
            append_empty_blocks(&mut blockchain, 2);
        }

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/metrics")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test; TEST_READER_TOKEN is a fixed #[cfg(test)] fixture string
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "/metrics must return 200");
        let body = String::from_utf8(test::read_body(resp).await.to_vec())
            .expect("metrics endpoint must return utf-8 prometheus text");
        let second_tip = parse_prometheus_u64_metric(&body, "wrfcoin_blockchain_block_height");
        let second_finalized =
            parse_prometheus_u64_metric(&body, "wrfcoin_chain_finalized_height");

        assert_eq!(second_tip, first_tip + 2, "tip should advance by both blocks");
        assert_eq!(
            second_finalized,
            first_finalized + 2,
            "finalized height must advance one-for-one with the tip once the \
             chain is beyond MAX_REORG_DEPTH"
        );
        assert_eq!(
            second_tip - second_finalized,
            MAX_REORG_DEPTH,
            "the baseline finality lag must remain pinned to MAX_REORG_DEPTH"
        );
    }

    /// The /metrics endpoint must expose the label keys that dashboard PromQL
    /// templates depend on. Grafana panels use `{{topic}}` in legendFormat,
    /// alert rules group by `reason`, and p2p message counters are split by
    /// `direction`. Renaming any of these label keys would break every
    /// query that interpolates them.
    #[actix_web::test]
    #[serial_test::serial(core_metrics)]
    async fn metrics_endpoint_labels_are_stable() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/metrics")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test; TEST_READER_TOKEN is a fixed #[cfg(test)] fixture string
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "/metrics must return 200");
        let body = String::from_utf8(test::read_body(resp).await.to_vec())
            .expect("metrics endpoint must return utf-8 prometheus text");

        // `topic` label on p2p bandwidth counters — used by
        // weather-data.json panels as `{{topic}}` in legendFormat.
        // P2P_TOPICS pre-registers all four topic values on every call to
        // apply_p2p_snapshot so the label keys appear immediately.
        assert!(
            body.contains("wrfcoin_p2p_bytes_received_total{topic=")
                || body.contains("wrfcoin_p2p_bytes_received_total {topic="),
            "`topic` label key missing from wrfcoin_p2p_bytes_received_total — \
             would break weather-data dashboard per-topic P2P bandwidth panels. \
             Body preview: {}",
            &body[..body.len().min(2048)]
        );
        assert!(
            body.contains("wrfcoin_p2p_bytes_sent_total{topic=")
                || body.contains("wrfcoin_p2p_bytes_sent_total {topic="),
            "`topic` label key missing from wrfcoin_p2p_bytes_sent_total"
        );

        // `reason` label on slashing events — pre-initialized for all
        // SlashingReason variants in CoreMetrics::new so the label key is
        // always present in the exposition format, even before any
        // slashing has happened.
        assert!(
            body.contains("wrfcoin_blockchain_slashing_events_total{reason=")
                || body.contains("wrfcoin_blockchain_slashing_events_total {reason="),
            "`reason` label key missing from wrfcoin_blockchain_slashing_events_total — \
             would break slashing alert grouping"
        );

        // `direction` label on p2p message counters — used together with
        // `topic` to distinguish sent vs received per topic.
        assert!(
            body.contains("wrfcoin_p2p_messages_total{"),
            "wrfcoin_p2p_messages_total missing from /metrics"
        );
        // The direction label must be one of the two stable values.
        assert!(
            body.contains("direction=\"sent\"") || body.contains("direction=\"received\""),
            "`direction` label key missing from wrfcoin_p2p_messages_total — \
             would break per-direction P2P traffic panels. Body preview: {}",
            &body[..body.len().min(2048)]
        );
    }

    // ---------------------------------------------------------------------------
    // CORS middleware tests (#1274)
    // ---------------------------------------------------------------------------

    /// Helper: build a minimal App with the CORS middleware applied, wired to
    /// the full route set.  Tests set `LAUNCH_API_CORS_ORIGINS` via
    /// `std::env::set_var` before calling this so `build_cors()` picks up the
    /// test-specific allow-list.
    fn cors_test_app(
        state: web::Data<AppState>,
    ) -> actix_web::App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .wrap(build_cors())
            .app_data(state)
            .app_data(launch_api_json_config())
            .configure(configure_launch_routes)
    }

    #[actix_web::test]
    async fn cors_allows_configured_origin_preflight() {
        let _guard = crate::helpers::test_env::lock();
        // `LAUNCH_API_CORS_ORIGINS` is process-global and `build_cors()` reads it,
        // so these tests take THE process-env lock (#2398 review). A comment
        // claiming isolation is not isolation.
        std::env::set_var(
            "LAUNCH_API_CORS_ORIGINS",
            "http://localhost:3000,http://localhost:5173",
        );

        let state = test_state(None);
        let app = test::init_service(cors_test_app(state)).await;

        let req = test::TestRequest::default()
            .method(actix_web::http::Method::OPTIONS)
            .uri("/chain/info")
            .insert_header(("Origin", "http://localhost:3000"))
            .insert_header(("Access-Control-Request-Method", "GET"))
            .to_request();

        let resp = test::call_service(&app, req).await;
        let status = resp.status().as_u16();
        // actix-cors returns 200 or 204 for a valid preflight.
        assert!(
            status == 200 || status == 204,
            "expected 200/204 for allowed origin preflight, got {status}"
        );
        let acao = resp
            .headers()
            .get("Access-Control-Allow-Origin")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert_eq!(
            acao, "http://localhost:3000",
            "Access-Control-Allow-Origin header should echo the allowed origin"
        );
    }

    #[actix_web::test]
    async fn cors_rejects_disallowed_origin_preflight() {
        let _guard = crate::helpers::test_env::lock();
        std::env::set_var(
            "LAUNCH_API_CORS_ORIGINS",
            "http://localhost:3000,http://localhost:5173",
        );

        let state = test_state(None);
        let app = test::init_service(cors_test_app(state)).await;

        let req = test::TestRequest::default()
            .method(actix_web::http::Method::OPTIONS)
            .uri("/chain/info")
            .insert_header(("Origin", "http://evil.example.com"))
            .insert_header(("Access-Control-Request-Method", "GET"))
            .to_request();

        let resp = test::call_service(&app, req).await;
        let status = resp.status().as_u16();
        // actix-cors rejects disallowed origins with 400.
        assert!(
            status == 400 || status == 403,
            "expected 400/403 for disallowed origin preflight, got {status}"
        );
    }

    #[actix_web::test]
    async fn cors_preflight_returns_access_control_headers() {
        let _guard = crate::helpers::test_env::lock();
        std::env::set_var(
            "LAUNCH_API_CORS_ORIGINS",
            "http://localhost:3000,http://localhost:5173",
        );

        let state = test_state(None);
        let app = test::init_service(cors_test_app(state)).await;

        let req = test::TestRequest::default()
            .method(actix_web::http::Method::OPTIONS)
            .uri("/chain/info")
            .insert_header(("Origin", "http://localhost:5173"))
            .insert_header(("Access-Control-Request-Method", "POST"))
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(
            resp.headers().contains_key("Access-Control-Allow-Origin"),
            "preflight response must include Access-Control-Allow-Origin"
        );
        assert!(
            resp.headers().contains_key("Access-Control-Allow-Methods"),
            "preflight response must include Access-Control-Allow-Methods"
        );
    }

    /// Fail-closed: in production without an explicit allow-list every
    /// cross-origin preflight must be rejected (refs #1828).
    /// Calls `build_cors_from_env` directly to avoid env-var races with
    /// concurrently executing CORS tests.
    #[actix_web::test]
    async fn cors_production_without_explicit_origins_denies_all_preflight() {
        let state = test_state(None);
        let app = test::init_service(
            App::new()
                .wrap(build_cors_from_env(None, true))
                .app_data(state)
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;

        let req = test::TestRequest::default()
            .method(actix_web::http::Method::OPTIONS)
            .uri("/chain/info")
            .insert_header(("Origin", "http://testnet.wrfcoin.io"))
            .insert_header(("Access-Control-Request-Method", "GET"))
            .to_request();

        let resp = test::call_service(&app, req).await;
        let status = resp.status().as_u16();
        assert!(
            status == 400 || status == 403,
            "production without LAUNCH_API_CORS_ORIGINS must deny all CORS preflight, got {status}"
        );
    }

    /// Explicit allow-list in production is always honoured (refs #1828).
    /// Calls `build_cors_from_env` directly to avoid env-var races with
    /// concurrently executing CORS tests.
    #[actix_web::test]
    async fn cors_production_with_explicit_origins_allows_configured() {
        let state = test_state(None);
        let app = test::init_service(
            App::new()
                .wrap(build_cors_from_env(Some("https://testnet.wrfcoin.io"), true))
                .app_data(state)
                .app_data(launch_api_json_config())
                .configure(configure_launch_routes),
        )
        .await;

        let req = test::TestRequest::default()
            .method(actix_web::http::Method::OPTIONS)
            .uri("/chain/info")
            .insert_header(("Origin", "https://testnet.wrfcoin.io"))
            .insert_header(("Access-Control-Request-Method", "GET"))
            .to_request();

        let resp = test::call_service(&app, req).await;
        let status = resp.status().as_u16();
        assert!(
            status == 200 || status == 204,
            "production with explicit LAUNCH_API_CORS_ORIGINS must allow configured origin, got {status}"
        );
        let acao = resp
            .headers()
            .get("Access-Control-Allow-Origin")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert_eq!(
            acao, "https://testnet.wrfcoin.io",
            "Access-Control-Allow-Origin should echo the allowed testnet origin"
        );
    }

    // -----------------------------------------------------------------------
    // #1839 Snapshot endpoint auth tests
    // -----------------------------------------------------------------------

    #[actix_web::test]
    async fn snapshot_summary_requires_reader_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get().uri("/state/snapshot").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/state/snapshot")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test; TEST_READER_TOKEN is a fixed #[cfg(test)] fixture string
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn snapshot_entries_requires_reader_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/state/snapshot/entries?limit=10")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/state/snapshot/entries?limit=10")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test; TEST_READER_TOKEN is a fixed #[cfg(test)] fixture string
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn snapshot_sync_payload_requires_reader_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/state/snapshot/sync-payload")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/state/snapshot/sync-payload")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test; TEST_READER_TOKEN is a fixed #[cfg(test)] fixture string
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // -----------------------------------------------------------------------
    // #1841 Stake status auth test
    // -----------------------------------------------------------------------

    #[actix_web::test]
    async fn stake_status_requires_reader_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let addr = "0x1111111111111111111111111111111111111111";

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/transactions/stake/{addr}"))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/transactions/stake/{addr}"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test; TEST_READER_TOKEN is a fixed #[cfg(test)] fixture string
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // #1840 — oracle config/status must gate the identity set behind Reader auth
    #[actix_web::test]
    async fn oracle_config_requires_reader_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/oracle/config")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/oracle/config")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: test-only fixture constant in an in-process TestRequest; never reaches an output path
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn oracle_status_requires_reader_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/oracle/status")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/oracle/status")
                .insert_header((
                    "Authorization",
                    format!(
                        "Bearer {TEST_READER_TOKEN}", // secret-leak-ok: fixed test token is non-production fixture data
                    ),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // #1842 — contribution status scan must gate behind Reader auth
    #[actix_web::test]
    async fn contribution_statuses_requires_reader_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/contributions/status")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/contributions/status")
                .insert_header((
                    "Authorization",
                    format!(
                        "Bearer {TEST_READER_TOKEN}", // secret-leak-ok: fixed test token is non-production fixture data
                    ),
                ))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // #1843 — per-participant slashing history must gate behind Reader auth
    #[actix_web::test]
    async fn quality_participant_history_requires_reader_auth() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let addr = "0x1111111111111111111111111111111111111111";

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/transactions/quality/participants/{addr}/history"))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/transactions/quality/participants/{addr}/history"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: fixed public test-only token literal
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// #1845 regression — permissionless replay-seen map survives a node restart.
    #[tokio::test]
    async fn permissionless_replay_seen_survives_restart_regression_1845() {
        use crate::state::PERMISSIONLESS_REPLAY_TTL_SECS;
        use wrfcoin_native_chain::storage::Storage;

        let db_path = std::env::temp_dir().join(format!(
            "wrfcoin-replay-test-{}.redb",
            fastrand::u64(..)
        ));

        // --- "first run" — write a live entry and a stale entry ---
        let storage = Storage::open(&db_path, 100).expect("open storage");
        let now = chrono::Utc::now().timestamp();
        let mut map = std::collections::HashMap::new();
        map.insert("live-key".to_string(), now);
        // Stale: older than TTL so it should be pruned on next load.
        map.insert(
            "stale-key".to_string(),
            now - PERMISSIONLESS_REPLAY_TTL_SECS - 1,
        );
        storage
            .save_permissionless_replay(&map)
            .expect("save_permissionless_replay");
        drop(storage);

        // --- "restart" — reload from the same path and apply TTL prune ---
        let storage2 = Storage::open(&db_path, 100).expect("reopen storage");
        let loaded = storage2
            .load_permissionless_replay()
            .expect("load_permissionless_replay");
        drop(storage2);
        let _ = std::fs::remove_file(&db_path);

        // Prune on reload (mirrors the production startup path).
        let now2 = chrono::Utc::now().timestamp();
        let pruned: std::collections::HashMap<_, _> = loaded
            .into_iter()
            .filter(|(_, seen_at)| *seen_at > now2 - PERMISSIONLESS_REPLAY_TTL_SECS)
            .collect();

        assert!(
            pruned.contains_key("live-key"),
            "live key must survive restart"
        );
        assert!(
            !pruned.contains_key("stale-key"),
            "TTL-expired key must be pruned on reload"
        );
    }

    // -----------------------------------------------------------------------
    // Genesis oracle registration submission (#2530)
    // -----------------------------------------------------------------------

    use wrfcoin_native_chain::crypto::domain_signing::{signing_chain_id, signing_network_id};

    /// Build a ceremony the way the CLI does, under an explicit chain identity.
    fn test_ceremony(
        chain_id: &str,
        network_id: &str,
        identities: &[String],
        quorum: usize,
    ) -> wrfcoin_shared_protocol::oracle::ceremony::GenesisOracleCeremony {
        let signing_pubkeys: std::collections::BTreeMap<String, String> = identities
            .iter()
            .map(|id| (id.clone(), id.clone()))
            .collect();
        wrfcoin_shared_protocol::oracle::ceremony::GenesisOracleCeremony::new(
            chain_id,
            network_id,
            identities.to_vec(),
            quorum,
            signing_pubkeys,
            1,
            1_700_000_000,
            // `submitted_at` must be current: block assembly drops anything
            // outside the submission window, and the route now refuses it at
            // admission. A fixed literal here would make every happy-path test
            // fail as the clock advanced past it.
            chrono::Utc::now().timestamp(),
        )
        .expect("ceremony inputs valid") // panic-ok: test helper, inputs are literals
    }

    fn registration_batch_body(
        chain_id: &str,
        network_id: &str,
        transactions: &[Transaction],
    ) -> serde_json::Value {
        json!({
            "chain_id": chain_id,
            "network_id": network_id,
            "transactions": transactions,
        })
    }

    /// The whole point of the route: the transactions land in the mempool with
    /// the ids the ceremony gave them. Rebuilding them (as
    /// `enqueue_protocol_transaction` does) would give each node a different id
    /// for the same registration, so block 1 would differ.
    #[actix_web::test]
    async fn oracle_registrations_are_admitted_verbatim_with_ceremony_ids() {
        let _guard = crate::helpers::test_env::lock();
        let (gov_sk, gov_pk) = keypair(2530);
        unsafe {
            std::env::set_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
                &gov_pk,
            );
        }

        let identities: Vec<String> = (0..3).map(|i| keypair(2531 + i).1).collect();
        let (chain_id, network_id) = (signing_chain_id(), signing_network_id());
        let ceremony = test_ceremony(&chain_id, &network_id, &identities, 2);
        let transactions = ceremony.build(&gov_sk);
        let expected_ids: Vec<String> = transactions
            .iter()
            .map(|tx| tx.id.as_ref().expect("ceremony sets the id").to_hex())
            .collect();

        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/oracle/registrations")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .set_json(registration_batch_body(&chain_id, &network_id, &transactions))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let mempool = state.mempool.read().await;
        let pooled: Vec<String> = mempool
            .get_transactions()
            .iter()
            .filter_map(|tx| tx.id.as_ref().map(|id| id.to_hex()))
            .collect();
        for id in &expected_ids {
            assert!(
                pooled.contains(id),
                "registration {id} must be admitted with its ceremony id, not a rebuilt one"
            );
        }
        assert_eq!(pooled.len(), 3, "every registration must be admitted");

        unsafe {
            std::env::remove_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
            );
        }
    }

    /// A batch whose members disagree on the set is the exact shape #2496
    /// rejects at block validation. Refusing it at admission means nothing
    /// partial reaches the mempool, so block 1 cannot be cut from a half-batch.
    #[actix_web::test]
    async fn oracle_registrations_reject_a_disagreeing_batch_without_admitting_any() {
        let _guard = crate::helpers::test_env::lock();
        let (gov_sk, gov_pk) = keypair(2540);
        unsafe {
            std::env::set_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
                &gov_pk,
            );
        }

        let identities: Vec<String> = (0..3).map(|i| keypair(2541 + i).1).collect();
        let (chain_id, network_id) = (signing_chain_id(), signing_network_id());
        let full = test_ceremony(&chain_id, &network_id, &identities, 2).build(&gov_sk);
        // The incremental shape: the first operator registers alone, as if the
        // set were being grown one member at a time.
        let partial = test_ceremony(&chain_id, &network_id, &identities[..1], 1).build(&gov_sk);
        let mixed = vec![full[0].clone(), partial[0].clone(), full[2].clone()];

        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/oracle/registrations")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .set_json(registration_batch_body(&chain_id, &network_id, &mixed))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let mempool = state.mempool.read().await;
        assert_eq!(
            mempool.get_transactions().len(),
            0,
            "a rejected batch must admit nothing at all"
        );

        unsafe {
            std::env::remove_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
            );
        }
    }

    /// A ceremony run under the wrong chain identity produces registrations
    /// that cannot authorize anywhere. Without this check the operator sees
    /// only "not authorized by an allowlisted governance key" and goes looking
    /// for a key problem that does not exist.
    #[actix_web::test]
    async fn oracle_registrations_name_a_chain_identity_mismatch() {
        let _guard = crate::helpers::test_env::lock();
        let (gov_sk, gov_pk) = keypair(2550);
        unsafe {
            std::env::set_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
                &gov_pk,
            );
        }

        let identities: Vec<String> = (0..3).map(|i| keypair(2551 + i).1).collect();
        // Signed for a different chain than the node runs.
        let (node_chain_id, network_id) = (signing_chain_id(), signing_network_id());
        let foreign_chain_id = format!("{node_chain_id}-elsewhere");
        let transactions =
            test_ceremony(&foreign_chain_id, &network_id, &identities, 2).build(&gov_sk);

        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/oracle/registrations")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .set_json(registration_batch_body(&foreign_chain_id, &network_id, &transactions))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body: serde_json::Value = test::read_body_json(resp).await;
        let message = body.to_string();
        assert!(
            message.contains(&foreign_chain_id) && message.contains(&node_chain_id),
            "the error must name both chain identities, got: {message}"
        );

        let mempool = state.mempool.read().await;
        assert_eq!(mempool.get_transactions().len(), 0);

        unsafe {
            std::env::remove_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
            );
        }
    }

    /// Registrations signed by a key outside the allowlist must not enter the
    /// mempool, even when every other field is well formed.
    #[actix_web::test]
    async fn oracle_registrations_reject_an_unallowlisted_governance_key() {
        let _guard = crate::helpers::test_env::lock();
        let (rogue_sk, _rogue_pk) = keypair(2560);
        let (_allowed_sk, allowed_pk) = keypair(2561);
        unsafe {
            std::env::set_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
                &allowed_pk,
            );
        }

        let identities: Vec<String> = (0..3).map(|i| keypair(2562 + i).1).collect();
        let (chain_id, network_id) = (signing_chain_id(), signing_network_id());
        let transactions = test_ceremony(&chain_id, &network_id, &identities, 2).build(&rogue_sk);

        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/oracle/registrations")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .set_json(registration_batch_body(&chain_id, &network_id, &transactions))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let mempool = state.mempool.read().await;
        assert_eq!(mempool.get_transactions().len(), 0);

        unsafe {
            std::env::remove_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
            );
        }
    }

    /// Alice hit this on a real cut rehearsal: a ceremony artifact with a
    /// `submitted_at` 60 days stale was accepted (201, accepted:3), mined
    /// (201), and block 1 came out `transaction_count: 1` — coinbase only. The
    /// chain came up N=0/K=2 and the only trace was
    /// `pop_skipped_mempool_transactions=3` buried in block metadata.
    ///
    /// `filter_by_submission_timing` drops out-of-window transactions during
    /// assembly with a `warn!` long after the submitter got its 201. Rejecting
    /// at admission is the only point where anyone is still listening.
    #[actix_web::test]
    async fn oracle_registrations_reject_a_stale_submitted_at() {
        let _guard = crate::helpers::test_env::lock();
        let (gov_sk, gov_pk) = keypair(2590);
        unsafe {
            std::env::set_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
                &gov_pk,
            );
        }

        let (chain_id, network_id) = (signing_chain_id(), signing_network_id());
        let identities: Vec<String> = (0..3).map(|i| keypair(2591 + i).1).collect();
        let signing_pubkeys: std::collections::BTreeMap<String, String> = identities
            .iter()
            .map(|id| (id.clone(), id.clone()))
            .collect();

        // Sixty days stale, as measured on the rehearsal.
        let stale_submitted_at = chrono::Utc::now().timestamp() - 60 * 24 * 3600;
        let ceremony = wrfcoin_shared_protocol::oracle::ceremony::GenesisOracleCeremony::new(
            &chain_id,
            &network_id,
            identities.clone(),
            2,
            signing_pubkeys,
            1,
            1_700_000_000,
            stale_submitted_at,
        )
        .expect("ceremony inputs valid"); // panic-ok: test helper, inputs are literals
        let transactions = ceremony.build(&gov_sk);

        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/oracle/registrations")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .set_json(registration_batch_body(&chain_id, &network_id, &transactions))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body: serde_json::Value = test::read_body_json(resp).await;
        let message = body.to_string();
        // Assert on the reported delta, not on prose: the operator's diagnostic
        // is "how stale, exactly", and it is what distinguishes a 60-day-old
        // artifact from one that merely drifted past the margin.
        //
        // Parse it rather than matching an exact number. The fixture reads the
        // clock to build `submitted_at` and the route reads it again; crossing
        // one second between the two makes the delta 5184001 and an exact
        // `contains("5184000")` fails at random. An independent verifier hit
        // exactly that while running an unrelated mutation control, which is
        // the expensive way to find a flake — it masqueraded as that control
        // breaking a second test.
        let floor = (60 * 24 * 3600) as u64;
        let reported: u64 = message
            .split("has a submitted_at ")
            .nth(1)
            .and_then(|rest| rest.split('s').next())
            .and_then(|digits| digits.parse().ok())
            .unwrap_or_else(|| {
                // panic-ok: test assertion on a malformed diagnostic
                panic!("the error must report a numeric delta, got: {message}")
            });
        assert!(
            (floor..floor + 300).contains(&reported),
            "the error must report the actual ~{floor}s delta, got {reported} in: {message}"
        );
        assert!(
            message.contains("admission margin"),
            "the error must name the admission margin, got: {message}"
        );

        let mempool = state.mempool.read().await;
        assert_eq!(
            mempool.get_transactions().len(),
            0,
            "a batch assembly would drop must not enter the mempool at all"
        );

        unsafe {
            std::env::remove_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
            );
        }
    }

    /// Binds the admission MARGIN, not merely the window.
    ///
    /// This artifact sits inside the 3600s window block assembly enforces, so
    /// the naive check accepted it — and then assembly could still drop it,
    /// because `producer_block_timestamp` clamps forward past the previous
    /// block and past median-time-past, and more time passes between admission
    /// and assembly. For a past-dated transaction the assembly delta is always
    /// >= the delta measured at admission.
    ///
    /// A ceremony artifact is cheap to rebuild, so halfway to expiry is
    /// refused rather than admitted-and-maybe-discarded. Deleting the margin
    /// (using the full window) must fail this test and only this test.
    #[actix_web::test]
    async fn oracle_registrations_reject_an_artifact_halfway_to_expiry() {
        let _guard = crate::helpers::test_env::lock();
        let (gov_sk, gov_pk) = keypair(2600);
        unsafe {
            std::env::set_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
                &gov_pk,
            );
        }

        let window = wrfcoin_shared_protocol::consensus::proof_of_publishing::
            DEFAULT_SUBMISSION_TIME_WINDOW_SECS as i64;
        // Comfortably inside the assembly window, comfortably outside half of
        // it: accepted before the margin existed, refused now.
        let submitted_at = chrono::Utc::now().timestamp() - (window * 3 / 4);

        let (chain_id, network_id) = (signing_chain_id(), signing_network_id());
        let identities: Vec<String> = (0..3).map(|i| keypair(2601 + i).1).collect();
        let signing_pubkeys: std::collections::BTreeMap<String, String> = identities
            .iter()
            .map(|id| (id.clone(), id.clone()))
            .collect();
        let ceremony = wrfcoin_shared_protocol::oracle::ceremony::GenesisOracleCeremony::new(
            &chain_id,
            &network_id,
            identities.clone(),
            2,
            signing_pubkeys,
            1,
            1_700_000_000,
            submitted_at,
        )
        .expect("ceremony inputs valid"); // panic-ok: test helper, inputs are literals
        let transactions = ceremony.build(&gov_sk);

        // Guard the premise: if this artifact were outside the assembly window
        // the test would pass for the wrong reason and prove nothing about the
        // margin.
        let delta = chrono::Utc::now().timestamp() - submitted_at;
        assert!(
            delta < window,
            "premise broken: artifact must be INSIDE the {window}s assembly window, delta {delta}s"
        );
        assert!(
            delta > window / 2,
            "premise broken: artifact must be outside the {}s margin, delta {delta}s",
            window / 2
        );

        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/oracle/registrations")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .set_json(registration_batch_body(&chain_id, &network_id, &transactions))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body: serde_json::Value = test::read_body_json(resp).await;
        let message = body.to_string();
        assert!(
            message.contains("admission margin"),
            "the error must name the margin, got: {message}"
        );

        let mempool = state.mempool.read().await;
        assert_eq!(mempool.get_transactions().len(), 0);

        unsafe {
            std::env::remove_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
            );
        }
    }

    /// Station registrations are admitted only through the operator route: that
    /// route (Operator role, authorization checked) still admits a signed one
    /// into the mempool, and a non-operator bearer does not.
    #[actix_web::test]
    async fn station_registrations_are_admitted_through_the_operator_route() {
        use wrfcoin_shared_protocol::blockchain::transaction::StationRegistrationPayload;
        use wrfcoin_shared_protocol::crypto::signature::{PrivateKey, PublicKey};
        use wrfcoin_shared_protocol::station_registry::{
            ContributionProvenanceClass, DataQualityTier,
        };

        let _guard = crate::helpers::test_env::lock();
        let gov = PrivateKey::from_bytes(&[0x60; 32]).expect("governance key");
        unsafe {
            std::env::set_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
                PublicKey::from_private_key(&gov).to_hex(),
            );
        }
        let owner = PrivateKey::from_bytes(&[0xA1; 32]).expect("owner key");
        let (chain_id, network_id) = (signing_chain_id(), signing_network_id());
        let mut payload = StationRegistrationPayload::new(
            "ws-operator-route",
            PublicKey::from_private_key(&owner).to_address(),
            46.7867,
            -92.1005,
            "test-real-station",
            DataQualityTier::Gold,
            ContributionProvenanceClass::Real,
        );
        payload.sign_ownership_with_domain(&chain_id, &network_id, &owner);
        let tx = Transaction::new_station_registration_with_domain(
            &chain_id, &network_id, payload, &gov,
        );

        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let body = serde_json::json!({
            "chain_id": chain_id, "network_id": network_id, "transactions": [tx],
        });
        let post = |token: &str| {
            test::TestRequest::post()
                .uri("/transactions/station/registrations")
                .insert_header(("Authorization", format!("Bearer {token}"))) // secret-leak-ok: test-only constant tokens, never a real credential
                .set_json(body.clone())
                .to_request()
        };

        let reader = test::call_service(&app, post(TEST_READER_TOKEN)).await;
        assert_eq!(reader.status(), StatusCode::FORBIDDEN, "not an operator: refused");
        assert_eq!(state.mempool.read().await.size(), 0);

        let operator = test::call_service(&app, post(TEST_OPERATOR_TOKEN)).await;
        assert_eq!(operator.status(), StatusCode::CREATED, "the operator route still admits it");
        assert_eq!(state.mempool.read().await.size(), 1);

        unsafe {
            std::env::remove_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
            );
        }
    }

    /// Found by an independent verifier, not by me.
    ///
    /// Validating the whole batch before admitting any of it is NOT sufficient
    /// for all-or-nothing: `mempool.add_transaction` can still reject item N
    /// after items 1..N-1 are in. The earlier version of this route returned
    /// there and left the first transaction behind — a PARTIAL oracle set,
    /// which is the incremental shape #2496 rejects, reached without anyone
    /// ever constructing an incremental artifact.
    ///
    /// A duplicate is the cheapest way to fail admission mid-loop.
    #[actix_web::test]
    async fn oracle_registrations_roll_back_when_the_mempool_rejects_mid_batch() {
        let _guard = crate::helpers::test_env::lock();
        let (gov_sk, gov_pk) = keypair(2610);
        unsafe {
            std::env::set_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
                &gov_pk,
            );
        }

        let (chain_id, network_id) = (signing_chain_id(), signing_network_id());
        let identities: Vec<String> = (0..3).map(|i| keypair(2611 + i).1).collect();
        let built = test_ceremony(&chain_id, &network_id, &identities, 2).build(&gov_sk);
        // tx[0] repeated: the second copy fails inside add_transaction, after
        // the first is already in.
        let batch = vec![built[0].clone(), built[0].clone()];

        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/oracle/registrations")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .set_json(registration_batch_body(&chain_id, &network_id, &batch))
                .to_request(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "the duplicate must fail inside mempool.add_transaction"
        );

        let mempool = state.mempool.read().await;
        assert_eq!(
            mempool.get_transactions().len(),
            0,
            "the already-admitted transaction must be rolled back, not left behind              as a partial oracle set"
        );

        unsafe {
            std::env::remove_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
            );
        }
    }

    /// The ceremony batch is O(N^2): every registration carries the absolute
    /// oracle set, so the request body grows quadratically in the set size and
    /// meets `MAX_JSON_PAYLOAD_BYTES` somewhere above 20 oracles. This pins the
    /// working range — if payload growth ever crosses the limit at the launch
    /// set size, block 1 becomes unsubmittable through this route and this test
    /// says so instead of the cut discovering it.
    #[actix_web::test]
    async fn oracle_registrations_admit_a_launch_sized_set() {
        let _guard = crate::helpers::test_env::lock();
        let (gov_sk, gov_pk) = keypair(2580);
        unsafe {
            std::env::set_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
                &gov_pk,
            );
        }

        const SET_SIZE: u64 = 20;
        let (chain_id, network_id) = (signing_chain_id(), signing_network_id());
        let identities: Vec<String> = (0..SET_SIZE).map(|i| keypair(2581 + i).1).collect();
        let transactions =
            test_ceremony(&chain_id, &network_id, &identities, 11).build(&gov_sk);

        let body = registration_batch_body(&chain_id, &network_id, &transactions);
        assert!(
            body.to_string().len() < crate::state::MAX_JSON_PAYLOAD_BYTES,
            "a {SET_SIZE}-oracle batch must still fit the JSON payload limit"
        );

        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/oracle/registrations")
                .insert_header(("Authorization", format!("Bearer {TEST_OPERATOR_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .set_json(body)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        let mempool = state.mempool.read().await;
        assert_eq!(mempool.get_transactions().len(), SET_SIZE as usize);

        unsafe {
            std::env::remove_var(
                wrfcoin_native_chain::crypto::domain_signing::ORACLE_GOVERNANCE_PUBKEYS_ENV,
            );
        }
    }

    /// The route mutates shared node state (the mempool block 1 is cut from),
    /// so it is operator-gated like the rest of the submission surface.
    #[actix_web::test]
    async fn oracle_registrations_require_operator_role() {
        let state = test_state(None);
        let app = test::init_service(test_app(state.clone())).await;
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/transactions/oracle/registrations")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .set_json(json!({
                    "chain_id": signing_chain_id(),
                    "network_id": signing_network_id(),
                    "transactions": [],
                }))
                .to_request(),
        )
        .await;
        assert!(
            resp.status() == StatusCode::FORBIDDEN || resp.status() == StatusCode::UNAUTHORIZED,
            "reader token must not reach the mempool, got {}",
            resp.status()
        );
    }

    /// #3342: register `ids` with distinct coordinates via the canonical
    /// transition, returning the state root after the last one.
    async fn seed_station_list_3342(state: &web::Data<AppState>, ids: &[&str]) -> String {
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;
        let mut blockchain = state.blockchain.write().await;
        blockchain.height = 150;
        blockchain.set_finalized_height(120);
        for (i, id) in ids.iter().enumerate() {
            let mut station = canonical_station_2780(
                "0x465230af3383c16d024c5f4b87ed2d1b595be148",
                if i == 1 {
                    ContributionProvenanceClass::Synthetic
                } else {
                    ContributionProvenanceClass::Real
                },
                i != 2,
            );
            station.station_id = (*id).to_string();
            station.location.latitude = 10.123456 + i as f64;
            station.location.longitude = 20.654321 - i as f64;
            blockchain
                .apply_state_transition(StateTransition::ApplyStationRegistration { station })
                .expect("canonical station transition should apply");
        }
        hex::encode(blockchain.state.state_root.0)
    }

    macro_rules! get_station_list_3342 {
        ($app:expr, $uri:expr, $token:expr $(,)?) => {{
            let mut request = test::TestRequest::get().uri($uri);
            let token: Option<&str> = $token;
            if let Some(token) = token {
                request = request.insert_header(("Authorization", format!("Bearer {token}"))); // secret-leak-ok: test-only constant tokens (TEST_READER_TOKEN / TEST_OPERATOR_TOKEN), never a real credential
            }
            let response = test::call_service($app, request.to_request()).await;
            let status = response.status();
            let body = test::read_body(response).await;
            let json: serde_json::Value =
                serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
            (status, json)
        }};
    }

    /// #3342: exact coordinates are Operator-only — anonymous is 401 and a
    /// Reader bearer (which every Contributor/Oracle bearer satisfies) is 403.
    #[actix_web::test]
    async fn station_list_is_operator_only_3342() {
        let state = test_state(None);
        seed_station_list_3342(&state, &["PWS-A"]).await;
        let app = test::init_service(test_app(state)).await;

        let (anon, _) = get_station_list_3342!(&app, "/transactions/station/registrations", None);
        assert_eq!(anon, StatusCode::UNAUTHORIZED);
        let (reader, body) = get_station_list_3342!(
            &app,
            "/transactions/station/registrations",
            Some(TEST_READER_TOKEN), // secret-leak-ok: a test-only constant token, never a real credential
        );
        assert_eq!(reader, StatusCode::FORBIDDEN);
        assert!(
            body.get("stations").is_none(),
            "a refused read must carry no rows"
        );
    }

    /// #3342: the operator read returns every registry field the backend sync
    /// needs — exact coordinates, owner, provenance, activity — in id order,
    /// with the same height/root context as the per-station read.
    #[actix_web::test]
    async fn station_list_returns_exact_registry_rows_in_id_order_3342() {
        let state = test_state(None);
        let expected_root = seed_station_list_3342(&state, &["PWS-C", "PWS-A", "PWS-B"]).await;
        let app = test::init_service(test_app(state)).await;

        let (status, body) = get_station_list_3342!(
            &app,
            "/transactions/station/registrations",
            Some(TEST_OPERATOR_TOKEN), // secret-leak-ok: a test-only constant token, never a real credential
        );
        assert_eq!(status, StatusCode::OK);
        let ids: Vec<&str> = body["stations"]
            .as_array()
            .expect("stations array")
            .iter()
            .map(|s| s["station_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["PWS-A", "PWS-B", "PWS-C"]);
        // PWS-C was seeded first (i = 0): real, active, unrounded coordinates.
        let c = &body["stations"][2];
        assert_eq!(c["latitude"], 10.123456);
        assert_eq!(c["longitude"], 20.654321);
        assert_eq!(c["elevation_meters"], 350.0);
        assert_eq!(
            c["owner_address"],
            "0x465230af3383c16d024c5f4b87ed2d1b595be148"
        );
        assert_eq!(c["provenance_class"], "real");
        assert_eq!(c["is_active"], true);
        // PWS-A (i = 1) is synthetic, PWS-B (i = 2) inactive: listed as-is, the
        // consumer decides — the node does not filter the registry.
        assert_eq!(body["stations"][0]["provenance_class"], "synthetic");
        assert_eq!(body["stations"][1]["is_active"], false);
        assert_eq!(body["total"], 3);
        assert_eq!(body["has_more"], false);
        assert!(body["next_cursor"].is_null());
        assert_eq!(body["observed_at_height"], 150);
        assert_eq!(body["settled_height"], 120);
        assert_eq!(body["state_root_hex"], expected_root);
    }

    /// #3342: keyset pagination walks the registry exactly once.
    #[actix_web::test]
    async fn station_list_paginates_by_keyset_3342() {
        let state = test_state(None);
        seed_station_list_3342(&state, &["PWS-A", "PWS-B", "PWS-C"]).await;
        let app = test::init_service(test_app(state)).await;
        let op = Some(TEST_OPERATOR_TOKEN); // secret-leak-ok: a test-only constant token, never a real credential

        let (status, first) =
            get_station_list_3342!(&app, "/transactions/station/registrations?limit=2", op);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(first["stations"].as_array().unwrap().len(), 2);
        assert_eq!(first["has_more"], true);
        assert_eq!(first["next_cursor"], "PWS-B");
        assert_eq!(first["total"], 3);

        let (_, second) = get_station_list_3342!(
            &app,
            "/transactions/station/registrations?limit=2&after=PWS-B",
            op,
        );
        assert_eq!(second["stations"].as_array().unwrap().len(), 1);
        assert_eq!(second["stations"][0]["station_id"], "PWS-C");
        assert_eq!(second["has_more"], false);
        assert!(second["next_cursor"].is_null());

        // A cursor past the end is an empty page, not an error.
        let (status, empty) =
            get_station_list_3342!(&app, "/transactions/station/registrations?after=PWS-Z", op);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(empty["stations"].as_array().unwrap().len(), 0);

        // Oversized limits clamp; they are not refused.
        let (status, all) =
            get_station_list_3342!(&app, "/transactions/station/registrations?limit=100000", op);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(all["stations"].as_array().unwrap().len(), 3);
    }

    /// #3343 review: a tiny page must not collect or sort the registry. With
    /// limit = 1 over 20,000 stations the selection heap never holds more than
    /// 2 entries and uses O(n) comparisons, far below a full sort (~n log2 n).
    #[::core::prelude::v1::test]
    fn station_page_selection_is_bounded_for_a_tiny_page_3343() {
        use crate::routes::station::{STATION_PAGE_COMPARISONS, select_station_page};
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;
        const N: usize = 20_000;
        let template = canonical_station_2780(
            "0x465230af3383c16d024c5f4b87ed2d1b595be148",
            ContributionProvenanceClass::Real,
            true,
        );
        // Deterministic scramble so input order is neither sorted nor reversed.
        let stations: Vec<_> = (0..N)
            .map(|i| {
                let mut s = template.clone();
                s.station_id = format!("PWS-{:06}", (i * 7_919) % N);
                s
            })
            .collect();

        STATION_PAGE_COMPARISONS.with(|c| c.set(0));
        let (page, has_more, peak) = select_station_page(stations.iter(), None, 1);
        let comparisons = STATION_PAGE_COMPARISONS.with(|c| c.get());
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].station_id, "PWS-000000");
        assert!(has_more);
        assert!(
            peak <= 2,
            "heap must hold at most limit + 1 entries, held {peak}"
        );
        let full_sort_floor = N * 14; // ~ N * log2(N) for N = 20,000
        assert!(
            comparisons <= 4 * N,
            "limit=1 used {comparisons} comparisons for {N} entries (full sort would be ~{full_sort_floor})"
        );

        // A cursor near the end: the page is found, work stays bounded.
        STATION_PAGE_COMPARISONS.with(|c| c.set(0));
        let (tail, more, peak) = select_station_page(stations.iter(), Some("PWS-019997"), 1);
        assert_eq!(tail[0].station_id, "PWS-019998");
        assert!(more);
        assert!(peak <= 2);
        assert!(STATION_PAGE_COMPARISONS.with(|c| c.get()) <= 4 * N);
    }

    /// #3343: the bounded selection returns exactly what collect + sort did,
    /// for every limit/cursor combination over a scrambled registry.
    #[::core::prelude::v1::test]
    fn station_page_selection_matches_a_full_sort_3343() {
        use crate::routes::station::select_station_page;
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;
        let template = canonical_station_2780(
            "0x465230af3383c16d024c5f4b87ed2d1b595be148",
            ContributionProvenanceClass::Real,
            true,
        );
        let stations: Vec<_> = (0..97)
            .map(|i| {
                let mut s = template.clone();
                s.station_id = format!("S-{:03}", (i * 31) % 97);
                s
            })
            .collect();
        let mut sorted: Vec<&str> = stations.iter().map(|s| s.station_id.as_str()).collect();
        sorted.sort_unstable();
        for limit in [1usize, 2, 7, 96, 97, 500] {
            for after in [
                None,
                Some("S-000"),
                Some("S-050"),
                Some("S-096"),
                Some("S-999"),
            ] {
                let expected: Vec<&str> = sorted
                    .iter()
                    .copied()
                    .filter(|id| after.is_none_or(|c| *id > c))
                    .collect();
                let (page, has_more, peak) = select_station_page(stations.iter(), after, limit);
                let got: Vec<&str> = page.iter().map(|s| s.station_id.as_str()).collect();
                assert_eq!(
                    got,
                    expected.iter().copied().take(limit).collect::<Vec<_>>(),
                    "limit {limit} after {after:?}"
                );
                assert_eq!(
                    has_more,
                    expected.len() > limit,
                    "limit {limit} after {after:?}"
                );
                assert!(peak <= limit + 1);
            }
        }
    }

    #[actix_web::test]
    async fn station_list_rejects_zero_limit_and_malformed_cursor_3342() {
        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let op = Some(TEST_OPERATOR_TOKEN); // secret-leak-ok: a test-only constant token, never a real credential
        let (zero, _) =
            get_station_list_3342!(&app, "/transactions/station/registrations?limit=0", op);
        assert_eq!(zero, StatusCode::UNPROCESSABLE_ENTITY);
        let (bad, _) =
            get_station_list_3342!(&app, "/transactions/station/registrations?after=a%20b", op);
        assert_eq!(bad, StatusCode::UNPROCESSABLE_ENTITY);
        let (empty_registry, body) =
            get_station_list_3342!(&app, "/transactions/station/registrations", op);
        assert_eq!(empty_registry, StatusCode::OK);
        assert_eq!(body["total"], 0);
    }

    fn canonical_station_2780(
        owner_address: &str,
        provenance_class: wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass,
        is_active: bool,
    ) -> wrfcoin_shared_protocol::station_registry::WeatherStation {
        use wrfcoin_shared_protocol::station_registry::{
            DataQualityTier, StationLocation, WeatherStation,
        };

        WeatherStation {
            station_id: "PWS-EXAMPLE-AMBIENT-01".to_string(),
            owner_address: owner_address.to_string(),
            location: StationLocation {
                latitude: 41.0,
                longitude: -77.0,
                elevation_meters: Some(350.0),
            },
            hardware_model: "Ambient WS-2902".to_string(),
            calibration_date: None,
            is_active,
            reputation_score: 0,
            data_quality_tier: DataQualityTier::Gold,
            provenance_class,
            ring_public_key: None,
        }
    }

    #[actix_web::test]
    async fn canonical_station_reader_requires_auth_and_returns_context_2780() {
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;

        let state = test_state(None);
        let expected_root = {
            let mut blockchain = state.blockchain.write().await;
            blockchain.height = 150;
            blockchain.set_finalized_height(120);
            blockchain
                .apply_state_transition(StateTransition::ApplyStationRegistration {
                    station: canonical_station_2780(
                        "0x465230af3383c16d024c5f4b87ed2d1b595be148",
                        ContributionProvenanceClass::Real,
                        true,
                    ),
                })
                .expect("canonical station transition should apply");
            hex::encode(blockchain.state.state_root.0)
        };
        let app = test::init_service(test_app(state)).await;

        let unauthenticated = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/station/registrations/PWS-EXAMPLE-AMBIENT-01")
                .to_request(),
        )
        .await;
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/station/registrations/PWS-EXAMPLE-AMBIENT-01")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(response).await;
        assert_eq!(body["station_id"], "PWS-EXAMPLE-AMBIENT-01");
        assert_eq!(
            body["owner_address"],
            "0x465230af3383c16d024c5f4b87ed2d1b595be148"
        );
        assert_eq!(body["provenance_class"], "real");
        assert_eq!(body["is_active"], true);
        // #3285: the registered elevation, for the collectors' sea-level key.
        assert_eq!(body["elevation_meters"], 350.0);
        assert_eq!(body["observed_at_height"], 150);
        assert_eq!(body["settled_height"], 120);
        assert_eq!(body["state_root_hex"], expected_root);
        assert!(body.get("registration_transaction_id").is_none());
        assert!(body.get("registration_inclusion_height").is_none());
    }

    /// #3345 / #3347 (contract C): the owner-ledger read route reports the
    /// nonce the next owner tx must carry, the freeze / deactivation flags and
    /// a pending recovery with its notice window; a station with no entry
    /// reports the empty default; an unregistered station is 404.
    #[actix_web::test]
    async fn station_owner_ledger_reader_reports_the_entry_3345() {
        use wrfcoin_shared_protocol::blockchain::station_owner::{
            PendingStationOwnerRecovery, StationFreeze, StationOwnerLedgerEntry,
        };
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;

        let state = test_state(None);
        {
            let mut blockchain = state.blockchain.write().await;
            blockchain.height = 150;
            blockchain.set_finalized_height(120);
            blockchain
                .apply_state_transition(StateTransition::ApplyStationRegistration {
                    station: canonical_station_2780(
                        "0x465230af3383c16d024c5f4b87ed2d1b595be148",
                        ContributionProvenanceClass::Real,
                        true,
                    ),
                })
                .expect("canonical station transition should apply");
        }
        let app = test::init_service(test_app(state.clone())).await;
        let uri = "/transactions/station/registrations/PWS-EXAMPLE-AMBIENT-01/owner-ledger"; // pragma: allowlist secret -- synthetic station route

        let anon = test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
        assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);

        let (missing, _) = get_station_list_3342!(
            &app,
            "/transactions/station/registrations/NO-SUCH-STATION/owner-ledger",
            Some(TEST_READER_TOKEN),
        );
        assert_eq!(missing, StatusCode::NOT_FOUND);

        // No ledger entry: the empty default, nonce 0.
        let (status, body) = get_station_list_3342!(&app, uri, Some(TEST_READER_TOKEN));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["station_id"], "PWS-EXAMPLE-AMBIENT-01");
        assert_eq!(body["owner_ledger"]["owner_nonce"], 0);
        assert_eq!(body["owner_ledger"]["frozen"], serde_json::Value::Null);
        assert_eq!(body["owner_ledger"]["owner_deactivated"], false);
        assert_eq!(body["owner_ledger"]["pending_recovery"], serde_json::Value::Null);
        assert_eq!(body["rewards_suspended"], false);
        assert_eq!(body["features"]["station_owner_consent"], false);
        assert_eq!(body["features"]["station_owner_actions"], false);
        assert_eq!(body["features"]["station_active_reward_gate"], false);
        assert_eq!(body["recovery_notice_blocks"], 34_000);
        assert_eq!(body["observed_at_height"], 150);
        assert_eq!(body["settled_height"], 120);

        // A populated entry is reported verbatim, with the derived notice height.
        state.blockchain.write().await.state.station_owner_ledger.insert(
            "PWS-EXAMPLE-AMBIENT-01".to_string(),
            StationOwnerLedgerEntry {
                owner_nonce: 3,
                frozen: Some(StationFreeze { since_height: 11 }),
                owner_deactivated: true,
                pending_recovery: Some(PendingStationOwnerRecovery {
                    from_owner_address: "wrf-from".to_string(),
                    to_owner_address: "wrf-to".to_string(),
                    recovery_nonce: 3,
                    filed_at_height: 100,
                }),
            },
        );
        let (_, body) = get_station_list_3342!(&app, uri, Some(TEST_READER_TOKEN));
        assert_eq!(body["owner_ledger"]["owner_nonce"], 3);
        assert_eq!(body["owner_ledger"]["frozen"]["since_height"], 11);
        assert_eq!(body["owner_ledger"]["owner_deactivated"], true);
        let pending = &body["owner_ledger"]["pending_recovery"];
        assert_eq!(pending["recovery_nonce"], 3);
        assert_eq!(pending["filed_at_height"], 100);
        assert_eq!(pending["executable_at_height"], 100 + 34_000);
        assert_eq!(pending["to_owner_address"], "wrf-to");
        // The features are unscheduled here, so the ledger half of the reward
        // predicate is not yet in force.
        assert_eq!(body["rewards_suspended"], false);
    }

    /// #3345 / #3347 (contract A): the submit route needs the Contributor role,
    /// refuses an empty batch and a foreign chain identity, and — with the
    /// features unscheduled — refuses an owner tx with a named code instead of
    /// letting it be mined as an inert no-op.
    #[actix_web::test]
    async fn station_owner_submit_refuses_with_named_codes_3345() {
        use wrfcoin_shared_protocol::blockchain::transaction::{
            Transaction, TransactionInput, TransactionOutput, TransactionType,
        };
        use wrfcoin_shared_protocol::crypto::chain_binding::ChainIdentity;

        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;
        let identity = ChainIdentity::active();
        let uri = "/transactions/station/owner-actions";
        let post = |token: Option<&str>, body: serde_json::Value| {
            let mut request = test::TestRequest::post().uri(uri).set_json(body);
            if let Some(token) = token {
                request = request.insert_header(("Authorization", format!("Bearer {token}"))); // secret-leak-ok: test-only constant tokens, never a real credential
            }
            request.to_request()
        };

        let mut tx = Transaction::new(
            vec![TransactionInput {
                txid: wrfcoin_shared_protocol::crypto::hash::hash_string("owner-tx-input"),
                vout: 0,
                signature: None,
                public_key: None,
                coinbase_data: None,
            }],
            vec![TransactionOutput {
                amount: 0,
                address: "wrf-owner".to_string(),
                script: String::new(),
                nft_data: None,
            }],
        );
        tx.tx_type = TransactionType::StationRegistration;
        tx.data = Some(
            serde_json::json!({
                "tx_kind": "station_owner_action_v1", "station_id": "ws-x",
                "owner_address": "wrf-owner", "owner_pubkey": "00",
                "action": "report_stolen", "action_nonce": 0, "expires_at_height": 10,
            })
            .to_string(),
        );
        tx.set_id();
        let good_identity = serde_json::json!({
            "chain_id": identity.chain_id, "network_id": identity.network_id,
            "transactions": [tx],
        });

        let anon = test::call_service(&app, post(None, good_identity.clone())).await;
        assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);
        let reader =
            test::call_service(&app, post(Some(TEST_READER_TOKEN), good_identity.clone())).await;
        assert_eq!(reader.status(), StatusCode::FORBIDDEN, "Reader may not submit");

        let empty = test::call_service(
            &app,
            post(
                Some(TEST_CONTRIBUTOR_TOKEN),
                serde_json::json!({
                    "chain_id": identity.chain_id, "network_id": identity.network_id,
                    "transactions": [],
                }),
            ),
        )
        .await;
        assert_eq!(empty.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let foreign = test::call_service(
            &app,
            post(
                Some(TEST_CONTRIBUTOR_TOKEN),
                serde_json::json!({
                    "chain_id": "not-this-chain", "network_id": identity.network_id,
                    "transactions": [tx],
                }),
            ),
        )
        .await;
        assert_eq!(foreign.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body: serde_json::Value = test::read_body_json(foreign).await;
        assert_eq!(body["error"]["code"], "CHAIN_IDENTITY_MISMATCH");

        let inert =
            test::call_service(&app, post(Some(TEST_CONTRIBUTOR_TOKEN), good_identity)).await;
        assert_eq!(inert.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body: serde_json::Value = test::read_body_json(inert).await;
        assert_eq!(body["error"]["code"], "OWNER_TX_FEATURE_NOT_ACTIVE");
    }

    /// #3285: a station registered without an elevation reports `null`, so
    /// a collector submits no sea-level pressure key rather than guessing.
    #[actix_web::test]
    async fn canonical_station_reader_reports_a_missing_elevation_as_null_3285() {
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;

        let state = test_state(None);
        {
            let mut station = canonical_station_2780(
                "0x465230af3383c16d024c5f4b87ed2d1b595be148",
                ContributionProvenanceClass::Real,
                true,
            );
            station.location.elevation_meters = None;
            let mut blockchain = state.blockchain.write().await;
            blockchain
                .apply_state_transition(StateTransition::ApplyStationRegistration { station })
                .expect("canonical station transition should apply");
        }
        let app = test::init_service(test_app(state)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/station/registrations/PWS-EXAMPLE-AMBIENT-01")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(response).await;
        assert!(body["elevation_meters"].is_null(), "{body}");
    }

    #[actix_web::test]
    async fn canonical_station_reader_follows_registry_state_change_2780() {
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;

        let state = test_state(None);
        {
            let mut blockchain = state.blockchain.write().await;
            blockchain.height = 200;
            blockchain
                .apply_state_transition(StateTransition::ApplyStationRegistration {
                    station: canonical_station_2780(
                        "0x1111111111111111111111111111111111111111",
                        ContributionProvenanceClass::Real,
                        true,
                    ),
                })
                .expect("initial canonical station transition should apply");
        }
        let app = test::init_service(test_app(state.clone())).await;
        let first = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/station/registrations/PWS-EXAMPLE-AMBIENT-01")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .to_request(),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        let first: serde_json::Value = test::read_body_json(first).await;

        {
            let mut blockchain = state.blockchain.write().await;
            blockchain.height = 201;
            blockchain
                .apply_state_transition(StateTransition::ApplyStationRegistration {
                    station: canonical_station_2780(
                        "0x2222222222222222222222222222222222222222",
                        ContributionProvenanceClass::Synthetic,
                        false,
                    ),
                })
                .expect("replacement canonical station transition should apply");
        }

        let second = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/station/registrations/PWS-EXAMPLE-AMBIENT-01")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .to_request(),
        )
        .await;
        assert_eq!(second.status(), StatusCode::OK);
        let second: serde_json::Value = test::read_body_json(second).await;

        assert_eq!(first["owner_address"], "0x1111111111111111111111111111111111111111");
        assert_eq!(first["provenance_class"], "real");
        assert_eq!(first["is_active"], true);
        assert_eq!(first["settled_height"], 100);
        assert_eq!(second["owner_address"], "0x2222222222222222222222222222222222222222");
        assert_eq!(second["provenance_class"], "synthetic");
        assert_eq!(second["is_active"], false);
        assert_eq!(second["observed_at_height"], 201);
        assert_eq!(second["settled_height"], 101);
        assert_ne!(first["state_root_hex"], second["state_root_hex"]);
    }

    #[actix_web::test]
    async fn canonical_station_reader_blocks_inflight_writer_for_coherent_snapshot_2780() {
        use wrfcoin_shared_protocol::station_registry::ContributionProvenanceClass;

        const STATION_ID: &str = "PWS-TIOGA-CONCURRENT-2780";
        let state = test_state(None);
        let old_root = {
            let mut blockchain = state.blockchain.write().await;
            blockchain.height = 300;
            let mut station = canonical_station_2780(
                "0x3333333333333333333333333333333333333333",
                ContributionProvenanceClass::Real,
                true,
            );
            station.station_id = STATION_ID.to_string();
            blockchain
                .apply_state_transition(StateTransition::ApplyStationRegistration { station })
                .expect("initial concurrent canonical station transition should apply");
            hex::encode(blockchain.state.state_root.0)
        };

        let seam = routes::station::install_canonical_station_read_test_seam(STATION_ID);
        let app = test::init_service(test_app(state.clone())).await;
        let request = test::TestRequest::get()
            .uri(&format!("/transactions/station/registrations/{STATION_ID}"))
            .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
            .to_request();

        let writer_state = state.clone();
        let writer_seam = seam.clone();
        let writer = async move {
            writer_seam.record_read.wait().await;

            // In the production implementation the route still owns its one
            // blockchain read guard here, so a writer cannot acquire. A
            // split-read mutant drops that guard and deterministically takes
            // this immediate branch before the handler collects context.
            let acquired_at_boundary = if let Ok(mut blockchain) =
                writer_state.blockchain.try_write()
            {
                blockchain.height = 301;
                let mut station = canonical_station_2780(
                    "0x4444444444444444444444444444444444444444",
                    ContributionProvenanceClass::Synthetic,
                    false,
                );
                station.station_id = STATION_ID.to_string();
                blockchain
                    .apply_state_transition(StateTransition::ApplyStationRegistration { station })
                    .expect("immediate replacement transition should apply");
                true
            } else {
                false
            };

            writer_seam.resume_context.wait().await;
            if !acquired_at_boundary {
                let mut blockchain = writer_state.blockchain.write().await;
                blockchain.height = 301;
                let mut station = canonical_station_2780(
                    "0x4444444444444444444444444444444444444444",
                    ContributionProvenanceClass::Synthetic,
                    false,
                );
                station.station_id = STATION_ID.to_string();
                blockchain
                    .apply_state_transition(StateTransition::ApplyStationRegistration { station })
                    .expect("blocked replacement transition should apply");
            }
            acquired_at_boundary
        };

        let (response, writer_acquired_at_boundary) =
            tokio::join!(test::call_service(&app, request), writer,);
        routes::station::clear_canonical_station_read_test_seam(STATION_ID);

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = test::read_body_json(response).await;
        assert!(
            !writer_acquired_at_boundary,
            "a writer acquired between the station record and returned context"
        );
        assert_eq!(
            body["owner_address"],
            "0x3333333333333333333333333333333333333333"
        );
        assert_eq!(body["provenance_class"], "real");
        assert_eq!(body["is_active"], true);
        assert_eq!(body["observed_at_height"], 300);
        assert_eq!(body["state_root_hex"], old_root);
    }

    #[actix_web::test]
    async fn canonical_station_reader_distinguishes_invalid_and_missing_ids_2780() {
        let state = test_state(None);
        let app = test::init_service(test_app(state)).await;

        let missing = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/transactions/station/registrations/well-formed-but-missing")
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .to_request(),
        )
        .await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let invalid_id = "x".repeat(129);
        let invalid = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/transactions/station/registrations/{invalid_id}"))
                .insert_header(("Authorization", format!("Bearer {TEST_READER_TOKEN}"))) // secret-leak-ok: a test-only constant token, never a real credential
                .to_request(),
        )
        .await;
        assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    // ---- keep-alive reuse race (#2782) -------------------------------------
    //
    // These two tests are deliberately a pair. The behavioural one proves the
    // VALUE is large enough; the source guard proves the value is actually
    // APPLIED to the server we ship. Either alone is passable while the bug is
    // live: a behavioural test that builds its own server stays green after
    // someone deletes `.keep_alive(..)` from the real builder chain, and a
    // source grep stays green if the constant is lowered back to 5s.

    /// An idle connection must survive longer than a poller's gap between
    /// requests.
    ///
    /// Mutation proof: set `LAUNCH_API_KEEP_ALIVE` back to actix's 5s default
    /// (or delete the `.keep_alive(..)` call below and let the default apply)
    /// and the second request on the reused socket reads 0 bytes, because the
    /// server has already sent FIN. That zero-byte read is precisely what hyper
    /// surfaces to `oracle_attester` as
    /// `connection closed before message completed`.
    #[actix_web::test]
    async fn server_keep_alive_outlives_a_pollers_request_gap_2782() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        /// How long the probe leaves the socket idle before reusing it. Above
        /// actix's 5s default, so the default would already have closed it.
        const PROBE_IDLE: std::time::Duration = std::time::Duration::from_secs(6);

        // Precondition for this probe only: it idles PROBE_IDLE below and then
        // reuses the socket, so the value has to exceed that or the test proves
        // nothing. This is deliberately NOT a claim about poller intervals —
        // no finite keep-alive dwarfs the configured range, and the property
        // that does hold (`K > C`, on client retention) is asserted in the
        // guard test against REQWEST_DEFAULT_POOL_IDLE_TIMEOUT.
        assert!(
            LAUNCH_API_KEEP_ALIVE > PROBE_IDLE,
            "keep-alive {LAUNCH_API_KEEP_ALIVE:?} must exceed this probe's idle \
             gap {PROBE_IDLE:?}, or the reuse below proves nothing"
        );

        let server = actix_web::HttpServer::new(|| {
            App::new().route(
                "/probe",
                web::get().to(|| async { actix_web::HttpResponse::Ok().finish() }),
            )
        })
        .keep_alive(LAUNCH_API_KEEP_ALIVE)
        .bind(("127.0.0.1", 0))
        .expect("bind an ephemeral port");
        let addr = server.addrs()[0];
        let running = server.run();
        let handle = running.handle();
        tokio::spawn(running);

        const REQUEST: &[u8] = b"GET /probe HTTP/1.1\r\nHost: probe\r\n\r\n";
        let mut sock = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect to the probe server");
        let mut buf = [0u8; 512];

        sock.write_all(REQUEST).await.expect("write first request");
        let first = sock.read(&mut buf).await.expect("read first response");
        assert!(
            String::from_utf8_lossy(&buf[..first]).starts_with("HTTP/1.1 200"),
            "probe server did not answer the first request"
        );

        // Idle for longer than actix's 5s default — the same gap the attester
        // leaves between cycles.
        tokio::time::sleep(PROBE_IDLE).await;

        // Reuse the SAME socket, exactly as a pooled hyper connection would.
        sock.write_all(REQUEST).await.expect("write second request");
        let second = sock.read(&mut buf).await.expect("read second response");
        assert!(
            second > 0,
            "server closed a 6s-idle connection: keep-alive regressed and the \
             attester reuse race is live again (#2782)"
        );
        assert!(
            String::from_utf8_lossy(&buf[..second]).starts_with("HTTP/1.1 200"),
            "reused connection did not answer the second request"
        );

        handle.stop(true).await;
    }

    /// Strip Rust comments and string/char literals, leaving only code tokens.
    ///
    /// The guard below searches for a *call*. A previous revision searched raw
    /// source text, and a review demonstrated the hole: delete the production
    /// call, write the exact needle in a comment between `.bind(..)` and
    /// `.run();`, and the suite stayed green while the shipped server had
    /// reverted to actix's 5s default. Text containment cannot tell a call from
    /// a comment, so the text has to stop containing comments.
    ///
    /// Removed spans become spaces (newlines kept) so the result stays
    /// line-shaped and readable in a failure message. Offsets are only ever used
    /// *within* the stripped string, so no correspondence back to the original
    /// is relied on.
    fn strip_comments_and_literals(source: &str) -> String {
        let bytes = source.as_bytes();
        let mut out = vec![b' '; bytes.len()];
        let mut i = 0usize;

        while i < bytes.len() {
            let rest = &bytes[i..];
            // Line comment: blank to end of line, keeping the newline.
            if rest.starts_with(b"//") {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            // Block comment: Rust nests them, so track depth.
            if rest.starts_with(b"/*") {
                let mut depth = 1usize;
                i += 2;
                while i < bytes.len() && depth > 0 {
                    if bytes[i..].starts_with(b"/*") {
                        depth += 1;
                        i += 2;
                    } else if bytes[i..].starts_with(b"*/") {
                        depth -= 1;
                        i += 2;
                    } else {
                        if bytes[i] == b'\n' {
                            out[i] = b'\n';
                        }
                        i += 1;
                    }
                }
                continue;
            }
            // Raw string: r"..", r#".."#, r##".."##, …
            if rest.starts_with(b"r\"") || rest.starts_with(b"r#") {
                let mut j = i + 1;
                let mut hashes = 0usize;
                while j < bytes.len() && bytes[j] == b'#' {
                    hashes += 1;
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'"' {
                    j += 1;
                    let mut terminator = Vec::with_capacity(hashes + 1);
                    terminator.push(b'"');
                    terminator.extend(std::iter::repeat_n(b'#', hashes));
                    while j < bytes.len() && !bytes[j..].starts_with(&terminator) {
                        if bytes[j] == b'\n' {
                            out[j] = b'\n';
                        }
                        j += 1;
                    }
                    i = (j + terminator.len()).min(bytes.len());
                    continue;
                }
            }
            // Ordinary string or char literal, honouring backslash escapes.
            if bytes[i] == b'"' || bytes[i] == b'\'' {
                let quote = bytes[i];
                let mut j = i + 1;
                while j < bytes.len() {
                    if bytes[j] == b'\\' {
                        j += 2;
                        continue;
                    }
                    if bytes[j] == quote {
                        j += 1;
                        break;
                    }
                    if bytes[j] == b'\n' {
                        out[j] = b'\n';
                    }
                    j += 1;
                }
                i = j.min(bytes.len());
                continue;
            }
            out[i] = bytes[i];
            i += 1;
        }

        String::from_utf8_lossy(&out).into_owned()
    }

    /// The shipped server must actually *call* `.keep_alive(LAUNCH_API_KEEP_ALIVE)`,
    /// and the constant must satisfy the ordering invariant.
    ///
    /// Three properties, because each covers a hole the others leave open:
    ///
    /// 1. the call exists **as code** in the production `.bind(..)`→`.run();`
    ///    chain — comments and string literals are stripped first, which is what
    ///    closes the review finding;
    /// 2. `LAUNCH_API_KEEP_ALIVE > REQWEST_DEFAULT_POOL_IDLE_TIMEOUT` — the
    ///    invariant is `K > C`, **strictly**: at `K == C` hyper still treats the
    ///    connection as live (its expiry test is `age > timeout`) while actix's
    ///    close timer is already due, so equality races rather than emptying the
    ///    band;
    /// 3. the value stays under [`LAUNCH_API_KEEP_ALIVE_CEILING`], so "raise it
    ///    again" cannot become unbounded idle retention.
    // `#[test]` alone resolves to `actix_web::test` inside this module (it is in
    // scope via the test-support imports) and would demand an async fn, so this
    // guard uses the same attribute as its behavioural sibling.
    #[actix_web::test]
    async fn main_applies_the_keep_alive_constant_to_the_shipped_server_2782() {
        // Needles are split so this test's own source cannot satisfy the search
        // even before stripping.
        let call_needle = concat!(".keep_alive(", "LAUNCH_API_KEEP_ALIVE)");
        let code = strip_comments_and_literals(include_str!("main.rs"));

        // The stripper is exercised on its own inputs first: a guard that cannot
        // fail is worse than no guard, and this one's whole value is that it
        // ignores comments.
        assert!(
            !strip_comments_and_literals("// .keep_alive(LAUNCH_API_KEEP_ALIVE)")
                .contains(call_needle),
            "the stripper must erase line comments"
        );
        assert!(
            !strip_comments_and_literals("/* .keep_alive(LAUNCH_API_KEEP_ALIVE) */")
                .contains(call_needle),
            "the stripper must erase block comments"
        );
        assert!(
            !strip_comments_and_literals("let s = \".keep_alive(LAUNCH_API_KEEP_ALIVE)\";")
                .contains(call_needle),
            "the stripper must erase string literals"
        );
        assert!(
            strip_comments_and_literals("    .keep_alive(LAUNCH_API_KEEP_ALIVE)")
                .contains(call_needle),
            "the stripper must keep real code"
        );

        let start = code
            .find(".bind((host.as_str(), port))?")
            .expect("the launch_api builder must still bind host/port");
        let chain = &code[start..];
        let end = chain
            .find(".run();")
            .expect("the launch_api builder must still call .run()");
        assert!(
            chain[..end].contains(call_needle),
            "the launch_api HttpServer builder must CALL \
             .keep_alive(LAUNCH_API_KEEP_ALIVE) between .bind(..) and .run(); \
             a comment or string mentioning it does not count, and without the \
             call actix's 5s default applies and the reuse race returns (#2782)"
        );

        assert!(
            LAUNCH_API_KEEP_ALIVE > REQWEST_DEFAULT_POOL_IDLE_TIMEOUT,
            "keep-alive {LAUNCH_API_KEEP_ALIVE:?} must be STRICTLY greater than \
             the client's idle-pool retention {REQWEST_DEFAULT_POOL_IDLE_TIMEOUT:?}. \
             Equality is not an empty band: hyper expires a pooled connection only \
             when its age is strictly greater than the timeout, so at K == C it is \
             still eligible for checkout at the very instant actix's close timer is \
             due. Below or at C, a poll interval in between reopens the race for any \
             client that has not set its own pool_idle_timeout (#2782/#2785)"
        );
        assert!(
            LAUNCH_API_KEEP_ALIVE <= LAUNCH_API_KEEP_ALIVE_CEILING,
            "keep-alive {LAUNCH_API_KEEP_ALIVE:?} exceeds the documented \
             retention ceiling {LAUNCH_API_KEEP_ALIVE_CEILING:?}"
        );
    }
}
