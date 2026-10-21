//! Integration test — desktop peer cold-start convergence against a live node (#1613).
//!
//! ## What this tests
//!
//! A fresh peer node (clean temp `WRFCOIN_DATA_DIR`, no prior chain state) is spawned as
//! a subprocess and pointed at a single bootstrap peer. The test asserts that the local
//! chain height reaches `WRF_TEST_MIN_HEIGHT` (default 50) within `WRF_TEST_TIMEOUT_SECS`
//! (default 60) seconds of first peer connection.
//!
//! ## How to run
//!
//! ```text
//! # Build the binary first:
//! cargo build -p wrfcoin-native-chain
//!
//! # Against node host (local testnet):
//! WRF_TEST_PEER_ADDR=<NODE_LAN_IP>:9000 \
//!   cargo test -p wrfcoin-p2p cold_start -- --ignored --nocapture
//!
//! # Custom height / timeout:
//! WRF_TEST_PEER_ADDR=<NODE_LAN_IP>:9000 \
//! WRF_TEST_MIN_HEIGHT=100 \
//! WRF_TEST_TIMEOUT_SECS=90 \
//!   cargo test -p wrfcoin-p2p cold_start -- --ignored --nocapture
//!
//! # Override binary location (e.g., release build):
//! WRF_TEST_PEER_ADDR=<NODE_LAN_IP>:9000 \
//! WRF_TEST_BINARY_PATH=/path/to/target/release/launch_api \
//!   cargo test -p wrfcoin-p2p cold_start -- --ignored --nocapture
//! ```
//!
//! ## CI behaviour
//!
//! The test carries `#[ignore]` so it never runs in CI without an explicit `--ignored` flag.
//! When `WRF_TEST_PEER_ADDR` is absent the test logs a skip reason and returns successfully —
//! it does **not** fail the suite.
//!
//! ## Depends on
//!
//! - core4#1482 — desktop peer must be able to advance past genesis; the test will time-out
//!   and fail descriptively if that fix is absent.
//! - A built `launch_api` binary (debug or release) in the Cargo target tree.
//!
//! ## Does NOT
//!
//! - Mutate the live N5 node in any way.
//! - Claim the full chain synced — only the first `WRF_TEST_MIN_HEIGHT` blocks.
//! - Run against a mock; this is deliberately a live-network test.

use std::{
    env,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

// ── tunables ──────────────────────────────────────────────────────────────────

/// Local API port chosen to avoid clashing with a running N5 node (default 8556).
const TEST_API_PORT: u16 = 18_556;

/// Documentation-only target; configure the operator peer explicitly for live runs.
const DEFAULT_PEER_HOST_PORT: &str = "192.0.2.10:9000";

/// Default minimum chain height to consider the SLO met.
const DEFAULT_MIN_HEIGHT: u64 = 50;

/// Default wall-clock budget for the entire test (seconds).
const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// How often we poll the local API.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How long we wait for the API port to come up before starting the SLO clock.
const API_STARTUP_GRACE: Duration = Duration::from_secs(10);

// ── test ──────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "requires a live peer; set WRF_TEST_PEER_ADDR and run with -- --ignored --nocapture"]
fn cold_start_convergence_50_blocks_in_60s() {
    // ── resolve tunables from env ─────────────────────────────────────────────
    let peer_host_port = env::var("WRF_TEST_PEER_ADDR").unwrap_or_else(|_| {
        // WRF_TEST_PEER_ADDR absent: treat as operator skip, not a test failure.
        // We still enter the function (because #[ignore] was overridden with --ignored),
        // so we print a helpful message and return early.
        println!(
            "[cold-start] SKIP — WRF_TEST_PEER_ADDR is not set.\n\
             Set it to run this test, e.g.:\n\
             \n\
             WRF_TEST_PEER_ADDR={}  cargo test -p wrfcoin-p2p cold_start -- --ignored --nocapture\n",
            DEFAULT_PEER_HOST_PORT,
        );
        // Signal the skip via a sentinel; checked immediately below.
        String::new()
    });
    if peer_host_port.is_empty() {
        return;
    }

    let min_height: u64 = env::var("WRF_TEST_MIN_HEIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MIN_HEIGHT);

    let timeout_secs: u64 = env::var("WRF_TEST_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS);

    // Convert host:port → libp2p multiaddr.
    let multiaddr = peer_host_port_to_multiaddr(&peer_host_port);

    println!("[cold-start] peer={peer_host_port}  multiaddr={multiaddr}");
    println!("[cold-start] SLO: height >= {min_height} within {timeout_secs}s");

    // ── clean temp state dir ──────────────────────────────────────────────────
    let state_dir = {
        let mut p = env::temp_dir();
        p.push(format!("wrf-cold-start-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&p).expect("create temp state dir");
        p
    };
    let _cleanup = DirCleanup(state_dir.clone());

    println!("[cold-start] state_dir={}", state_dir.display());

    // ── locate binary ─────────────────────────────────────────────────────────
    let binary = locate_launch_api_binary();
    println!("[cold-start] binary={}", binary.display());

    // ── spawn peer ────────────────────────────────────────────────────────────
    let mut child = Command::new(&binary)
        .env("WRFCOIN_API_HOST", "127.0.0.1")
        .env("WRFCOIN_API_PORT", TEST_API_PORT.to_string())
        .env("WRFCOIN_DATA_DIR", state_dir.to_str().unwrap())
        .env("WRFCOIN_P2P_BOOTSTRAP", &multiaddr)
        // Quiet logging — only warn+ from most crates; p2p at info for sync signals.
        .env("RUST_LOG", "warn,wrfcoin_p2p=info,launch_api=info")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|e| panic!("[cold-start] failed to spawn {}: {e}", binary.display()));
    let _kill = KillOnDrop(&mut child);

    // ── wait for API port to open (startup grace period) ─────────────────────
    let api_addr: SocketAddr = format!("127.0.0.1:{TEST_API_PORT}")
        .parse()
        .expect("valid socket addr");

    println!("[cold-start] waiting up to {:.0}s for API port ...", API_STARTUP_GRACE.as_secs_f32());
    let grace_start = Instant::now();
    loop {
        if TcpStream::connect_timeout(&api_addr, Duration::from_millis(200)).is_ok() {
            println!("[cold-start] API port open at {:.1}s", grace_start.elapsed().as_secs_f32());
            break;
        }
        if grace_start.elapsed() >= API_STARTUP_GRACE {
            panic!(
                "[cold-start] ❌ API port did not open within {:.0}s — \
                 binary may have failed to start (check stderr above)",
                API_STARTUP_GRACE.as_secs_f32()
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    // ── poll until height SLO or timeout ─────────────────────────────────────
    let slo_deadline = Duration::from_secs(timeout_secs);
    let slo_start = Instant::now();
    let mut last_height: u64 = 0;
    let mut first_nonzero_at: Option<Duration> = None;

    loop {
        std::thread::sleep(POLL_INTERVAL);
        let elapsed = slo_start.elapsed();

        if let Some(h) = poll_height(api_addr) {
            if h > 0 && first_nonzero_at.is_none() {
                first_nonzero_at = Some(elapsed);
                println!(
                    "[cold-start] first block committed at {:.1}s (height={h})",
                    elapsed.as_secs_f32()
                );
            }
            if h != last_height {
                println!("[cold-start] height={h} elapsed={:.1}s", elapsed.as_secs_f32());
                last_height = h;
            }
            if h >= min_height {
                let elapsed_f = elapsed.as_secs_f32();
                println!(
                    "[cold-start] ✅ height={h} >= {min_height} reached in {elapsed_f:.1}s  \
                     (SLO={timeout_secs}s, margin={:.1}s)",
                    timeout_secs as f32 - elapsed_f
                );
                return;
            }
        }

        if elapsed >= slo_deadline {
            break;
        }
    }

    // ── SLO missed — fail with diagnostics ───────────────────────────────────
    let elapsed = slo_start.elapsed();
    panic!(
        "[cold-start] ❌ height={last_height} after {:.1}s — did not reach \
         min_height={min_height} within SLO={timeout_secs}s\n\
         \n\
         Diagnostics:\n\
         - peer:        {peer_host_port}\n\
         - binary:      {}\n\
         - state_dir:   {}\n\
         - first block: {}\n\
         \n\
         If height stayed at 0, check that core4#1482 (IBD past genesis) is present\n\
         in the binary being tested and that the peer is reachable.",
        elapsed.as_secs_f32(),
        binary.display(),
        state_dir.display(),
        first_nonzero_at
            .map(|d| format!("t+{:.1}s", d.as_secs_f32()))
            .unwrap_or_else(|| "never (height never left 0)".to_string()),
    );
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Poll `GET /blockchain` and return the `height` field, or `None` on any error.
fn poll_height(api_addr: SocketAddr) -> Option<u64> {
    let mut stream = TcpStream::connect_timeout(&api_addr, Duration::from_secs(2)).ok()?;
    stream
        .write_all(b"GET /blockchain HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .ok()?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    // HTTP response: headers CRLF CRLF body
    let body_start = raw.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    let body = std::str::from_utf8(&raw[body_start..]).ok()?;
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    // Primary: { ok, data: { height, ... } }
    // Fallback: { height, ... } (older builds / alternate serialisation)
    v["data"]["height"].as_u64().or_else(|| v["height"].as_u64())
}

/// Convert a plain `host:port` string to the libp2p multiaddr expected by
/// `WRFCOIN_P2P_BOOTSTRAP`.  Handles IPv4 addresses and hostnames.
///
/// Examples:
/// - `<NODE_LAN_IP>:9000` → `/ip4/<NODE_LAN_IP>/tcp/9000`
/// - `my-peer.local:9000` → `/dns4/my-peer.local/tcp/9000`
fn peer_host_port_to_multiaddr(host_port: &str) -> String {
    let (host, port) = match host_port.rsplit_once(':') {
        Some(pair) => pair,
        None => panic!("[cold-start] WRF_TEST_PEER_ADDR must be host:port, got: {host_port}"),
    };
    if host.parse::<std::net::Ipv4Addr>().is_ok() {
        format!("/ip4/{host}/tcp/{port}")
    } else {
        format!("/dns4/{host}/tcp/{port}")
    }
}

/// Locate the `launch_api` binary.
///
/// Search order:
/// 1. `WRF_TEST_BINARY_PATH` env var (operator override, e.g. release build path)
/// 2. `target/debug/launch_api` relative to workspace root
/// 3. `target/release/launch_api` relative to workspace root
/// 4. `launch_api` on PATH (custom install)
fn locate_launch_api_binary() -> PathBuf {
    if let Ok(p) = env::var("WRF_TEST_BINARY_PATH") {
        let path = PathBuf::from(&p);
        assert!(
            path.exists(),
            "[cold-start] WRF_TEST_BINARY_PATH={p} does not exist"
        );
        return path;
    }

    // `CARGO_MANIFEST_DIR` for wrfcoin-p2p is `<workspace>/p2p/`; workspace root is one level up.
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("canonicalize workspace root");

    for profile in &["debug", "release"] {
        let candidate = workspace_root.join("target").join(profile).join("launch_api");
        if candidate.exists() {
            return candidate;
        }
    }

    // Last resort — let the OS find it.
    PathBuf::from("launch_api")
}

// ── RAII guards ───────────────────────────────────────────────────────────────

struct DirCleanup(PathBuf);
impl Drop for DirCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct KillOnDrop<'a>(&'a mut Child);
impl Drop for KillOnDrop<'_> {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
