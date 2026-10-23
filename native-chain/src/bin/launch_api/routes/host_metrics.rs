//! Host metrics endpoint.
//!
//! Exposes OS-level resource telemetry that operators need to monitor node
//! health: disk utilisation, memory pressure, CPU usage, and swap.
//!
//! Route: `GET /node/metrics/host`
//!
//! The handler blocks the tokio runtime for the brief time required by
//! `sysinfo` to sample the data.  Collection typically takes < 1 ms on
//! Linux; if this becomes a concern the call can be wrapped in
//! `spawn_blocking`.

use actix_web::{HttpRequest, HttpResponse, Responder, web};
use serde::{Deserialize, Serialize};
use sysinfo::{Disks, System};
use std::{future::Future, process::Stdio, sync::LazyLock, time::{Duration, Instant}};
use tokio::{io::AsyncReadExt, process::Command, sync::Mutex};

use crate::helpers::require_role;
use crate::state::AppState;
use wrfcoin_native_chain::security::api_auth::ApiRole;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Per-disk entry returned by the host metrics endpoint.
#[derive(Debug, Serialize, Deserialize)]
pub struct DiskInfo {
    /// Mount point (e.g. `/`, `/data`).
    pub mount_point: String,
    /// Total disk capacity in GiB (gibibytes, base-1024).
    pub total_gb: f64,
    /// Space currently in use in GiB.
    pub used_gb: f64,
    /// Available space in GiB.
    pub available_gb: f64,
    /// Utilisation as a percentage (0–100).
    pub used_percent: f64,
}

/// Per-logical-CPU core usage entry.
#[derive(Debug, Serialize, Deserialize)]
pub struct CpuCoreUsage {
    /// Core label (e.g. `cpu0`).
    pub name: String,
    /// Usage percentage at the time of sampling (0.0–100.0).
    pub usage_percent: f32,
}

/// Response body for `GET /node/metrics/host`.
#[derive(Debug, Serialize, Deserialize)]
pub struct HostMetricsResponse {
    // ---- Memory (values in MiB, base-1024) ----
    /// Total physical RAM in MiB.
    pub memory_total_mb: u64,
    /// RAM currently in use in MiB.
    pub memory_used_mb: u64,
    /// RAM free / available in MiB.
    pub memory_free_mb: u64,
    /// Memory utilisation as a percentage (0–100).
    pub memory_used_percent: f64,

    // ---- Swap ----
    /// Total swap space in MiB.
    pub swap_total_mb: u64,
    /// Swap currently in use in MiB.
    pub swap_used_mb: u64,

    // ---- CPU ----
    /// Per-core CPU usage percentages.
    pub cpu_per_core: Vec<CpuCoreUsage>,
    /// Average CPU usage across all cores.
    pub cpu_usage_avg_percent: f32,

    // ---- Disks ----
    /// Per-mount disk utilisation.
    pub disks: Vec<DiskInfo>,

    /// Optional GPU telemetry, omitted when the tool is absent or a probe fails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<Vec<GpuInfo>>,
}

// Node-local telemetry only; none of this data enters consensus or persisted state.
const GPU_CACHE_TTL: Duration = Duration::from_secs(20);
const GPU_PROBE_TIMEOUT: Duration = Duration::from_millis(1500);
const GPU_MAX_OUTPUT: usize = 8192;
const GPU_MAX_DEVICES: usize = 32;
const NVIDIA_ARGS: [&str; 2] = [
    "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu",
    "--format=csv,noheader,nounits",
];

/// NVIDIA device telemetry; memory values are MiB, matching nvidia-smi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub utilization_percent: u8,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub temperature_c: i16,
}

#[derive(Default)]
struct GpuCache {
    attempted_at: Option<Instant>,
    value: Option<Vec<GpuInfo>>,
}

static GPU_CACHE: LazyLock<Mutex<GpuCache>> = LazyLock::new(|| Mutex::new(GpuCache::default()));

fn parse_gpu_output(bytes: &[u8]) -> Option<Vec<GpuInfo>> {
    if bytes.len() > GPU_MAX_OUTPUT {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut devices = Vec::new();
    for line in text.trim().lines() {
        if devices.len() >= GPU_MAX_DEVICES {
            return None;
        }
        let mut fields = line.split(',').map(str::trim);
        let name = fields.next()?;
        if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
            return None;
        }
        let utilization_percent = fields.next()?.parse::<u8>().ok()?;
        let memory_used_mb = fields.next()?.parse::<u64>().ok()?;
        let memory_total_mb = fields.next()?.parse::<u64>().ok()?;
        let temperature_c = fields.next()?.parse::<i16>().ok()?;
        if fields.next().is_some() || utilization_percent > 100
            || memory_used_mb > memory_total_mb || memory_total_mb == 0
            || !(-40..=150).contains(&temperature_c)
        {
            return None;
        }
        devices.push(GpuInfo {
            name: name.to_owned(), utilization_percent, memory_used_mb,
            memory_total_mb, temperature_c,
        });
    }
    if devices.is_empty() { None } else { Some(devices) }
}

fn nvidia_command() -> Command {
    let mut command = Command::new("nvidia-smi");
    command.args(NVIDIA_ARGS);
    command
}

/// Async pipe I/O, bounded before allocation growth; stderr cannot fill a pipe.
/// The caller applies the deadline. Dropping this future kills the child, including
/// on request cancellation, output overflow, and timeout.
async fn read_gpu_probe(mut command: Command) -> Option<Vec<GpuInfo>> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped())
        .stderr(Stdio::null()).kill_on_drop(true).spawn().ok()?;
    let stdout = child.stdout.take()?;
    let mut bytes = Vec::new();
    stdout.take((GPU_MAX_OUTPUT + 1) as u64).read_to_end(&mut bytes).await.ok()?;
    if bytes.len() > GPU_MAX_OUTPUT || !child.wait().await.ok()?.success() {
        return None;
    }
    parse_gpu_output(&bytes)
}

/// Serializes refreshes across requests, including failures. Record the attempt
/// before awaiting so a cancelled request also consumes its cache window.
async fn cached_gpu<F, Fut>(cache: &Mutex<GpuCache>, deadline: Duration, probe: F) -> Option<Vec<GpuInfo>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Option<Vec<GpuInfo>>>,
{
    let mut cache = cache.lock().await;
    if cache.attempted_at.is_some_and(|at| at.elapsed() < GPU_CACHE_TTL) {
        return cache.value.clone();
    }
    // determinism-ok: monotonic cache expiry for node-local GPU telemetry, never consensus input.
    cache.attempted_at = Some(Instant::now());
    cache.value = None;
    cache.value = tokio::time::timeout(deadline, probe()).await.ok().flatten();
    cache.value.clone()
}

async fn add_gpu<F, Fut>(mut metrics: HostMetricsResponse, cache: &Mutex<GpuCache>, deadline: Duration, probe: F) -> HostMetricsResponse
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Option<Vec<GpuInfo>>>,
{
    metrics.gpu = cached_gpu(cache, deadline, probe).await;
    metrics
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Collect host metrics from `sysinfo`.
///
/// Shared between the HTTP handler and tests so that collection logic
/// lives in one place.  If the handler changes, the tests change too.
fn collect_host_metrics() -> HostMetricsResponse {
    let mut sys = System::new_all();
    sys.refresh_all();

    // ---- Memory (bytes -> MiB) ----
    const BYTES_PER_MIB: u64 = 1024 * 1024;
    let memory_total_mb = sys.total_memory() / BYTES_PER_MIB;
    let memory_used_mb = sys.used_memory() / BYTES_PER_MIB;
    let memory_free_mb = memory_total_mb.saturating_sub(memory_used_mb);
    let memory_used_percent = if memory_total_mb > 0 {
        (memory_used_mb as f64 / memory_total_mb as f64) * 100.0
    } else {
        0.0
    };

    let swap_total_mb = sys.total_swap() / BYTES_PER_MIB;
    let swap_used_mb = sys.used_swap() / BYTES_PER_MIB;

    // ---- CPU ----
    let cpu_per_core: Vec<CpuCoreUsage> = sys
        .cpus()
        .iter()
        .map(|cpu| CpuCoreUsage {
            name: cpu.name().to_string(),
            usage_percent: cpu.cpu_usage(),
        })
        .collect();

    let cpu_usage_avg_percent = if cpu_per_core.is_empty() {
        0.0
    } else {
        cpu_per_core.iter().map(|c| c.usage_percent).sum::<f32>()
            / cpu_per_core.len() as f32
    };

    // ---- Disks ----
    const BYTES_PER_GIB: f64 = (1024 * 1024 * 1024) as f64;
    let disks_info = Disks::new_with_refreshed_list();
    let disks: Vec<DiskInfo> = disks_info
        .list()
        .iter()
        .map(|disk| {
            let total = disk.total_space() as f64;
            let available = disk.available_space() as f64;
            let used = total - available;
            let used_percent = if total > 0.0 { (used / total) * 100.0 } else { 0.0 };
            DiskInfo {
                mount_point: disk.mount_point().to_string_lossy().into_owned(),
                total_gb: total / BYTES_PER_GIB,
                used_gb: used / BYTES_PER_GIB,
                available_gb: available / BYTES_PER_GIB,
                used_percent,
            }
        })
        .collect();

    HostMetricsResponse {
        memory_total_mb,
        memory_used_mb,
        memory_free_mb,
        memory_used_percent,
        swap_total_mb,
        swap_used_mb,
        cpu_per_core,
        cpu_usage_avg_percent,
        disks,
        gpu: None,
    }
}

/// `GET /node/metrics/host`
///
/// Returns host-level resource metrics (disk, memory, CPU, swap) collected
/// via `sysinfo`. Restricted to the Operator role to prevent unauthorized
/// exposure of host disk/memory/CPU/swap data.
pub async fn get_host_metrics(
    req: HttpRequest,
    state: web::Data<AppState>,
) -> impl Responder {
    if let Err(resp) = require_role(&req, &state, ApiRole::Operator) {
        return resp;
    }

    let metrics = add_gpu(collect_host_metrics(), &GPU_CACHE, GPU_PROBE_TIMEOUT,
        || read_gpu_probe(nvidia_command())).await;
    HttpResponse::Ok().json(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_GPU: &[u8] = b"Example GPU, 42, 1024, 8192, 55\nOther GPU, 0, 0, 4096, 28\n";

    #[test]
    fn gpu_parser_accepts_multiple_devices_and_rejects_invalid_values() {
        let devices = parse_gpu_output(SAMPLE_GPU).expect("valid sample");
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].utilization_percent, 42);
        assert_eq!(devices[0].memory_used_mb, 1024);
        assert_eq!(devices[0].memory_total_mb, 8192);
        assert_eq!(devices[0].temperature_c, 55);
        for bad in ["", "garbage", "GPU, 101, 1, 2, 50", "GPU, NaN, 1, 2, 50",
            "GPU, 1, 3, 2, 50", "GPU, 1, 0, 0, 50", "GPU, 1, -1, 2, 50",
            "GPU, 1, 1, 2, 999", "GPU, 1, 1, 2, N/A", "GPU, 1, 1, 2, 50, extra",
            "GPU, 1, 18446744073709551616, 2, 50"]
        {
            assert!(parse_gpu_output(bad.as_bytes()).is_none(), "{bad}");
        }
        assert!(parse_gpu_output(&[0xff]).is_none());
        assert!(parse_gpu_output(&vec![b'x'; GPU_MAX_OUTPUT + 1]).is_none());
        assert!(parse_gpu_output("GPU, 0, 0, 1, 20\n".repeat(GPU_MAX_DEVICES + 1).as_bytes()).is_none());
    }

    #[test]
    fn gpu_command_has_only_fixed_arguments() {
        let command = nvidia_command();
        assert_eq!(command.as_std().get_program(), "nvidia-smi");
        assert_eq!(command.as_std().get_args().collect::<Vec<_>>(), NVIDIA_ARGS);
        assert!(GPU_PROBE_TIMEOUT <= Duration::from_secs(2));
    }

    #[actix_web::test]
    async fn gpu_success_is_additive_and_cached_across_concurrent_requests() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let cache = Mutex::new(GpuCache::default());
        let calls = AtomicUsize::new(0);
        let probe = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            parse_gpu_output(SAMPLE_GPU)
        };
        let (first, second) = tokio::join!(
            cached_gpu(&cache, GPU_PROBE_TIMEOUT, probe),
            cached_gpu(&cache, GPU_PROBE_TIMEOUT, probe),
        );
        assert_eq!(first, second);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let metrics = collect_host_metrics();
        let before = serde_json::to_value(&metrics).expect("metrics JSON");
        assert!(before.get("gpu").is_none());
        let metrics = add_gpu(metrics, &cache, GPU_PROBE_TIMEOUT, probe).await;
        let mut after = serde_json::to_value(metrics).expect("GPU metrics JSON");
        assert_eq!(after["gpu"][0]["name"], "Example GPU");
        after.as_object_mut().expect("object").remove("gpu");
        assert_eq!(after, before, "all existing fields must remain identical");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        cache.lock().await.attempted_at = Some(Instant::now() - GPU_CACHE_TTL);
        assert!(cached_gpu(&cache, GPU_PROBE_TIMEOUT, probe).await.is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[actix_web::test]
    async fn gpu_missing_garbage_and_timeout_are_omitted_and_failure_cached() {
        for failure in 0..3 {
            let cache = Mutex::new(GpuCache::default());
            let metrics = add_gpu(collect_host_metrics(), &cache, Duration::from_millis(10), || async {
                match failure {
                    0 => read_gpu_probe(Command::new("/nonexistent-wrfcoin-gpu-probe")).await,
                    1 => parse_gpu_output(b"not GPU CSV"),
                    _ => std::future::pending().await,
                }
            }).await;
            assert!(serde_json::to_value(metrics).expect("JSON").get("gpu").is_none());
            assert!(cached_gpu(&cache, GPU_PROBE_TIMEOUT, || async {
                panic!("failure must be cached, not forked at request rate")
            }).await.is_none());
        }
    }

    #[actix_web::test]
    async fn gpu_cancelled_refresh_is_also_cached() {
        use std::sync::Arc;
        let cache = Arc::new(Mutex::new(GpuCache::default()));
        let entered = Arc::new(tokio::sync::Notify::new());
        let task = tokio::spawn({
            let cache = Arc::clone(&cache);
            let entered = Arc::clone(&entered);
            async move {
                cached_gpu(&cache, GPU_PROBE_TIMEOUT, || async {
                    entered.notify_one();
                    std::future::pending().await
                }).await
            }
        });
        entered.notified().await;
        task.abort();
        assert!(task.await.expect_err("cancelled").is_cancelled());
        assert!(cached_gpu(&cache, GPU_PROBE_TIMEOUT, || async {
            panic!("cancelled attempt must consume its cache window")
        }).await.is_none());
    }

    #[cfg(unix)]
    #[actix_web::test]
    async fn gpu_subprocess_failures_output_limit_and_timeout_are_safe() {
        let cache = Mutex::new(GpuCache::default());
        // Real process boundaries, without PATH mutation, a GPU, or a shell.
        assert!(read_gpu_probe(Command::new("/usr/bin/false")).await.is_none());
        let mut success = Command::new("/usr/bin/printf");
        success.arg("GPU, 42, 1, 2, 55\n");
        assert!(read_gpu_probe(success).await.is_some());
        let mut flood = Command::new("/usr/bin/printf");
        flood.arg("x".repeat(GPU_MAX_OUTPUT + 1));
        assert!(read_gpu_probe(flood).await.is_none());
        let mut slow = Command::new("/usr/bin/sleep");
        slow.arg("30");
        let started = Instant::now();
        assert!(cached_gpu(&cache, Duration::from_millis(20), || read_gpu_probe(slow)).await.is_none());
        assert!(started.elapsed() < Duration::from_secs(2));
    }


    #[test]
    fn response_roundtrips_through_json() -> Result<(), serde_json::Error> {
        let metrics = collect_host_metrics();
        let json = serde_json::to_string(&metrics)?;
        let recovered: HostMetricsResponse = serde_json::from_str(&json)?;
        assert_eq!(recovered.memory_total_mb, metrics.memory_total_mb);
        assert_eq!(recovered.swap_total_mb, metrics.swap_total_mb);
        assert_eq!(recovered.disks.len(), metrics.disks.len());
        assert_eq!(recovered.cpu_per_core.len(), metrics.cpu_per_core.len());
        Ok(())
    }

    #[test]
    fn memory_percent_within_bounds() {
        let metrics = collect_host_metrics();
        assert!(
            (0.0..=100.0).contains(&metrics.memory_used_percent),
            "memory_used_percent should be 0–100, got {}",
            metrics.memory_used_percent
        );
    }

    #[test]
    fn memory_used_does_not_exceed_total() {
        let metrics = collect_host_metrics();
        assert!(
            metrics.memory_used_mb <= metrics.memory_total_mb,
            "used ({}) should not exceed total ({})",
            metrics.memory_used_mb,
            metrics.memory_total_mb
        );
    }

    #[test]
    fn swap_used_does_not_exceed_total() {
        let metrics = collect_host_metrics();
        assert!(
            metrics.swap_used_mb <= metrics.swap_total_mb,
            "swap used ({}) should not exceed total ({})",
            metrics.swap_used_mb,
            metrics.swap_total_mb
        );
    }

    #[test]
    fn disk_entries_have_non_negative_values() {
        let metrics = collect_host_metrics();
        for disk in &metrics.disks {
            assert!(
                disk.total_gb >= 0.0,
                "disk {} total_gb should be non-negative",
                disk.mount_point
            );
            assert!(
                disk.used_gb >= 0.0,
                "disk {} used_gb should be non-negative",
                disk.mount_point
            );
            assert!(
                disk.available_gb >= 0.0,
                "disk {} available_gb should be non-negative",
                disk.mount_point
            );
            assert!(
                (0.0..=100.0).contains(&disk.used_percent),
                "disk {} used_percent should be 0–100, got {}",
                disk.mount_point,
                disk.used_percent
            );
        }
    }

    #[test]
    fn cpu_avg_within_bounds() {
        let metrics = collect_host_metrics();
        assert!(
            (0.0..=100.0).contains(&metrics.cpu_usage_avg_percent),
            "cpu_usage_avg_percent should be 0–100, got {}",
            metrics.cpu_usage_avg_percent
        );
    }

    // Integration tests for authentication gate
    // Note: Full HTTP handler tests require a running server or actix-web test infrastructure.
    // These unit tests verify the collection logic; authentication is enforced by the handler signature
    // which requires HttpRequest and AppState parameters with require_role() check.
    // End-to-end auth tests can be added to the integration test suite if needed.
}
