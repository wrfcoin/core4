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

    HttpResponse::Ok().json(collect_host_metrics())
}

#[cfg(test)]
mod tests {
    use super::*;

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
